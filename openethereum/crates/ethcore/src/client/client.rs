// Copyright 2015-2020 Parity Technologies (UK) Ltd.
// This file is part of OpenEthereum.

// OpenEthereum is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// OpenEthereum is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with OpenEthereum.  If not, see <http://www.gnu.org/licenses/>.

use std::{
    cmp,
    collections::{BTreeMap, HashSet, VecDeque},
    convert::TryFrom,
    io::{BufRead, BufReader},
    str::{from_utf8, FromStr},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering as AtomicOrdering},
        Arc, Weak,
    },
    time::{Duration, Instant},
};

use blockchain::{
    BlockChain, BlockChainDB, BlockNumberKey, BlockProvider, BlockReceipts, ExtrasInsert,
    ImportRoute, TransactionAddress, TreeRoute,
};
use bytes::{Bytes, ToPretty};
use call_contract::CallContract;
use db::{DBTransaction, DBValue, KeyValueDB};
use ethcore_miner::pool::VerifiedTransaction;
use ethereum_types::{Address, H256, H264, U256};
use hash::keccak;
use itertools::Itertools;
use parking_lot::{Mutex, RwLock};
use rand::rngs::OsRng;
use rlp::{PayloadInfo, Rlp};
use rustc_hex::FromHex;
use trie::{Trie, TrieFactory, TrieSpec};
use types::{
    ancestry_action::AncestryAction,
    data_format::DataFormat,
    encoded,
    filter::Filter,
    header::{ExtendedHeader, Header},
    log_entry::LocalizedLogEntry,
    receipt::{LocalizedReceipt, TypedReceipt},
    transaction::{
        self, Action, LocalizedTransaction, SignedTransaction, TypedTransaction,
        UnverifiedTransaction,
    },
    BlockNumber,
};
use vm::{EnvInfo, LastHashes};

use ansi_term::Colour;
use block::{enact_verified, ClosedBlock, Drain, LockedBlock, OpenBlock, SealedBlock};
use call_contract::RegistryInfo;
use client::{
    ancient_import::AncientVerifier,
    bad_blocks,
    traits::{ForceUpdateSealing, TransactionRequest},
    AccountData, BadBlocks, Balance, BlockChain as BlockChainTrait, BlockChainClient,
    BlockChainReset, BlockId, BlockInfo, BlockProducer, BroadcastProposalBlock, Call,
    CallAnalytics, ChainInfo, ChainMessageType, ChainNotify, ChainRoute, ClientConfig,
    ClientIoMessage, EngineInfo, ImportBlock, ImportExportBlocks, ImportSealedBlock, IoClient,
    Mode, NewBlocks, Nonce, PrepareOpenBlock, ProvingBlockChainClient, PruningInfo, ReopenBlock,
    ScheduleInfo, SealedBlockImporter, StateClient, StateInfo, StateOrBlock, TraceFilter, TraceId,
    TransactionId, TransactionInfo, UncleId,
};
use engines::{
    epoch::PendingTransition, EngineError, EpochTransition, EthEngine, ForkChoice, SealingState,
    MAX_UNCLE_AGE,
};
use error::{
    BlockError, CallError, Error, Error as EthcoreError, ErrorKind as EthcoreErrorKind,
    EthcoreResult, ExecutionError, ImportErrorKind, QueueErrorKind,
};
use executive::{contract_address, Executed, Executive, TransactOptions};
use factory::{Factories, VmFactory};
use io::IoChannel;
use miner::{Miner, MinerService};
use snapshot::{self, io as snapshot_io, SnapshotClient};
use spec::Spec;
use state::{self, State};
use state_db::StateDB;
use stats::{PrometheusMetrics, PrometheusRegistry};
use trace::{
    self, Database as TraceDatabase, ImportRequest as TraceImportRequest, LocalizedTrace, TraceDB,
};
use transaction_ext::Transaction;
use verification::{
    self,
    queue::kind::{blocks::Unverified, BlockLike},
    BlockQueue, PreverifiedBlock, Verifier,
};
use vm::Schedule;
// re-export
pub use blockchain::CacheSize as BlockChainCacheSize;
use db::{keys::BlockDetails, Readable, Writable};
pub use reth_util::queue::ExecutionQueue;
pub use types::{block_status::BlockStatus, blockchain_info::BlockChainInfo};
pub use verification::QueueInfo as BlockQueueInfo;
use_contract!(registry, "res/contracts/registrar.json");

const ANCIENT_BLOCKS_QUEUE_SIZE: usize = 4096;
// Max number of blocks imported at once.
const ANCIENT_BLOCKS_BATCH_SIZE: usize = 4;
const MAX_QUEUE_SIZE_TO_SLEEP_ON: usize = 2;
const MIN_HISTORY_SIZE: u64 = 8;

/// Report on the status of a client.
#[derive(Default, Clone, Debug, Eq, PartialEq)]
pub struct ClientReport {
    /// How many blocks have been imported so far.
    pub blocks_imported: usize,
    /// How many transactions have been applied so far.
    pub transactions_applied: usize,
    /// How much gas has been processed so far.
    pub gas_processed: U256,
    /// Internal structure item sizes
    pub item_sizes: BTreeMap<String, usize>,
}

impl ClientReport {
    /// Alter internal reporting to reflect the additional `block` has been processed.
    pub fn accrue_block(&mut self, header: &Header, transactions: usize) {
        self.blocks_imported += 1;
        self.transactions_applied += transactions;
        self.gas_processed = self.gas_processed + *header.gas_used();
    }
}

impl<'a> ::std::ops::Sub<&'a ClientReport> for ClientReport {
    type Output = Self;

    fn sub(mut self, other: &'a ClientReport) -> Self {
        self.blocks_imported -= other.blocks_imported;
        self.transactions_applied -= other.transactions_applied;
        self.gas_processed = self.gas_processed - other.gas_processed;

        self
    }
}

struct SleepState {
    last_activity: Option<Instant>,
    last_autosleep: Option<Instant>,
}

impl SleepState {
    fn new(awake: bool) -> Self {
        SleepState {
            last_activity: match awake {
                false => None,
                true => Some(Instant::now()),
            },
            last_autosleep: match awake {
                false => Some(Instant::now()),
                true => None,
            },
        }
    }
}

struct Importer {
    /// Lock used during block import
    pub import_lock: Mutex<()>, // FIXME Maybe wrap the whole `Importer` instead?

    /// Used to verify blocks
    pub verifier: Box<dyn Verifier<Client>>,

    /// Queue containing pending blocks
    pub block_queue: BlockQueue,

    /// Handles block sealing
    pub miner: Arc<Miner>,

    /// Ancient block verifier: import an ancient sequence of blocks in order from a starting epoch
    pub ancient_verifier: AncientVerifier,

    /// Ethereum engine to be used during import
    pub engine: Arc<dyn EthEngine>,

    /// A lru cache of recently detected bad blocks
    pub bad_blocks: bad_blocks::BadBlocks,
}

/// Blockchain database client backed by a persistent database. Owns and manages a blockchain and a block queue.
/// Call `import_block()` to import a block asynchronously; `flush_queue()` flushes the queue.
pub struct Client {
    /// Flag used to disable the client forever. Not to be confused with `liveness`.
    enabled: AtomicBool,

    /// Operating mode for the client
    mode: Mutex<Mode>,

    chain: RwLock<Arc<BlockChain>>,
    tracedb: RwLock<TraceDB<BlockChain>>,
    engine: Arc<dyn EthEngine>,

    /// Client configuration
    config: ClientConfig,

    /// Database pruning strategy to use for StateDB
    pruning: journaldb::Algorithm,

    /// Don't prune the state we're currently snapshotting
    snapshotting_at: AtomicU64,

    /// Client uses this to store blocks, traces, etc.
    db: RwLock<Arc<dyn BlockChainDB>>,

    state_db: RwLock<StateDB>,

    /// Report on the status of client
    report: RwLock<ClientReport>,

    sleep_state: Mutex<SleepState>,

    /// Flag changed by `sleep` and `wake_up` methods. Not to be confused with `enabled`.
    liveness: AtomicBool,
    io_channel: RwLock<IoChannel<ClientIoMessage>>,

    /// List of actors to be notified on certain chain events
    notify: RwLock<Vec<Weak<dyn ChainNotify>>>,

    /// Queued transactions from IO
    queue_transactions: IoChannelQueue,
    /// Ancient blocks import queue
    /// Queued ancient blocks, make sure they are imported in order.
    queued_ancient_blocks: Arc<RwLock<HashSet<H256>>>,
    queued_ancient_blocks_executer: Mutex<Option<ExecutionQueue<(Unverified, Bytes)>>>,
    /// Consensus messages import queue
    queue_consensus_message: IoChannelQueue,

    last_hashes: RwLock<VecDeque<H256>>,
    factories: Factories,

    /// Number of eras kept in a journal before they are pruned
    history: u64,

    /// An action to be done if a mode/spec_name change happens
    on_user_defaults_change: Mutex<Option<Box<dyn FnMut(Option<Mode>) + 'static + Send>>>,

    registrar_address: Option<Address>,

    /// A closure to call when we want to restart the client
    exit_handler: Mutex<Option<Box<dyn Fn(String) + 'static + Send>>>,

    importer: Importer,
}

impl Importer {
    pub fn new(
        config: &ClientConfig,
        engine: Arc<dyn EthEngine>,
        message_channel: IoChannel<ClientIoMessage>,
        miner: Arc<Miner>,
    ) -> Result<Importer, EthcoreError> {
        let block_queue = BlockQueue::new(
            config.queue.clone(),
            engine.clone(),
            message_channel.clone(),
            config.verifier_type.verifying_seal(),
        );

        Ok(Importer {
            import_lock: Mutex::new(()),
            verifier: verification::new(config.verifier_type.clone()),
            block_queue,
            miner,
            ancient_verifier: AncientVerifier::new(engine.clone()),
            engine,
            bad_blocks: Default::default(),
        })
    }

    // t_nb 6.0 This is triggered by a message coming from a block queue when the block is ready for insertion
    pub fn import_verified_blocks(&self, client: &Client) -> usize {
        // Shortcut out if we know we're incapable of syncing the chain.
        trace!(target: "block_import", "fn import_verified_blocks");
        if !client.enabled.load(AtomicOrdering::SeqCst) {
            self.block_queue.reset_verification_ready_signal();
            return 0;
        }

        let max_blocks_to_import = client.config.max_round_blocks_to_import;
        let (
            imported_blocks,
            import_results,
            invalid_blocks,
            imported,
            proposed_blocks,
            duration,
            has_more_blocks_to_import,
        ) = {
            let mut imported_blocks = Vec::with_capacity(max_blocks_to_import);
            let mut invalid_blocks = HashSet::new();
            let proposed_blocks = Vec::with_capacity(max_blocks_to_import);
            let mut import_results = Vec::with_capacity(max_blocks_to_import);

            let _import_lock = self.import_lock.lock();
            let blocks = self.block_queue.drain(max_blocks_to_import);
            if blocks.is_empty() {
                debug!(target: "block_import", "block_queue is empty");
                self.block_queue.resignal_verification();
                return 0;
            }
            trace_time!("import_verified_blocks");
            let start = Instant::now();

            for block in blocks {
                let header = block.header.clone();
                let bytes = block.bytes.clone();
                let hash = header.hash();

                let is_invalid = invalid_blocks.contains(header.parent_hash());
                if is_invalid {
                    debug!(
                        target: "block_import",
                        "Refusing block #{}({}) with invalid parent {}",
                        header.number(),
                        header.hash(),
                        header.parent_hash()
                    );
                    invalid_blocks.insert(hash);
                    continue;
                }
                // t_nb 7.0 check and lock block
                match self.check_and_lock_block(&bytes, block, client) {
                    Ok((closed_block, pending)) => {
                        imported_blocks.push(hash);
                        let transactions_len = closed_block.transactions.len();
                        trace!(target:"block_import","Block #{}({}) check pass",header.number(),header.hash());
                        // t_nb 8.0 commit block to db
                        let route = self.commit_block(
                            closed_block,
                            &header,
                            encoded::Block::new(bytes),
                            pending,
                            client,
                        );
                        trace!(target:"block_import","Block #{}({}) commited",header.number(),header.hash());
                        import_results.push(route);
                        client
                            .report
                            .write()
                            .accrue_block(&header, transactions_len);
                    }
                    Err(err) => {
                        self.bad_blocks.report(
                            bytes,
                            format!("{:?}", err),
                            self.engine.params().eip1559_transition,
                        );
                        invalid_blocks.insert(hash);
                    }
                }
            }

            let imported = imported_blocks.len();
            let invalid_blocks = invalid_blocks.into_iter().collect::<Vec<H256>>();

            if !invalid_blocks.is_empty() {
                self.block_queue.mark_as_bad(&invalid_blocks);
            }
            let has_more_blocks_to_import = !self.block_queue.mark_as_good(&imported_blocks);
            (
                imported_blocks,
                import_results,
                invalid_blocks,
                imported,
                proposed_blocks,
                start.elapsed(),
                has_more_blocks_to_import,
            )
        };

        {
            if !imported_blocks.is_empty() {
                trace!(target:"block_import","Imported block, notify rest of system");
                let route = ChainRoute::from(import_results.as_ref());

                // t_nb 10 Notify miner about new included block.
                if !has_more_blocks_to_import {
                    self.miner.chain_new_blocks(
                        client,
                        &imported_blocks,
                        &invalid_blocks,
                        route.enacted(),
                        route.retracted(),
                        false,
                    );
                }

                // t_nb 11 notify rest of system about new block inclusion
                client.notify(|notify| {
                    notify.new_blocks(NewBlocks::new(
                        imported_blocks.clone(),
                        invalid_blocks.clone(),
                        route.clone(),
                        Vec::new(),
                        proposed_blocks.clone(),
                        duration,
                        has_more_blocks_to_import,
                    ));
                });
            }
        }
        trace!(target:"block_import","Flush block to db");
        let db = client.db.read();
        db.key_value().flush().expect("DB flush failed.");

        self.block_queue.resignal_verification();
        trace!(target:"block_import","Resignal verifier");
        imported
    }

    // t_nb 6.0.1 check and lock block,
    fn check_and_lock_block(
        &self,
        bytes: &[u8],
        block: PreverifiedBlock,
        client: &Client,
    ) -> EthcoreResult<(LockedBlock, Option<PendingTransition>)> {
        let engine = &*self.engine;
        let header = block.header.clone();

        // Check the block isn't so old we won't be able to enact it.
        // t_nb 7.1 check if block is older then last pruned block
        let best_block_number = client.chain.read().best_block_number();
        if client.pruning_info().earliest_state > header.number() {
            warn!(target: "client", "Block import failed for #{} ({})\nBlock is ancient (current best block: #{}).", header.number(), header.hash(), best_block_number);
            bail!("Block is ancient");
        }

        // t_nb 7.2 Check if parent is in chain
        let parent = match client.block_header_decoded(BlockId::Hash(*header.parent_hash())) {
            Some(h) => h,
            None => {
                warn!(target: "client", "Block import failed for #{} ({}): Parent not found ({}) ", header.number(), header.hash(), header.parent_hash());
                bail!("Parent not found");
            }
        };

        let chain = client.chain.read();
        // t_nb 7.3 verify block family
        let verify_family_result = self.verifier.verify_block_family(
            &header,
            &parent,
            engine,
            Some(verification::FullFamilyParams {
                block: &block,
                block_provider: &**chain,
                client,
            }),
        );

        if let Err(e) = verify_family_result {
            warn!(target: "client", "Stage 3 block verification failed for #{} ({})\nError: {:?}", header.number(), header.hash(), e);
            bail!(e);
        };

        // t_nb 7.4 verify block external
        let verify_external_result = self.verifier.verify_block_external(&header, engine);
        if let Err(e) = verify_external_result {
            warn!(target: "client", "Stage 4 block verification failed for #{} ({})\nError: {:?}", header.number(), header.hash(), e);
            bail!(e);
        };

        // Enact Verified Block
        // t_nb 7.5 Get build last hashes. Get parent state db. Get epoch_transition
        let last_hashes = client.build_last_hashes(header.parent_hash());

        let db = client
            .state_db
            .read()
            .boxed_clone_canon(header.parent_hash());

        let is_epoch_begin = chain
            .epoch_transition(parent.number(), *header.parent_hash())
            .is_some();

        if header.number() >= engine.params().validate_service_transactions_transition {
            // Check if zero gas price transactions are certified to be service transactions
            // using the Certifier contract. If they are not certified, the block is treated as invalid.
            let service_transaction_checker = self.miner.service_transaction_checker();
            if service_transaction_checker.is_some() {
                match service_transaction_checker.unwrap().refresh_cache(client) {
                    Ok(true) => {
                        trace!(target: "client", "Service transaction cache was refreshed successfully");
                    }
                    Ok(false) => {
                        trace!(target: "client", "Registrar or/and service transactions contract does not exist");
                    }
                    Err(e) => {
                        error!(target: "client", "Error occurred while refreshing service transaction cache: {}", e)
                    }
                };
            };
            for t in &block.transactions {
                if t.has_zero_gas_price() {
                    match self.miner.service_transaction_checker() {
                        None => {
                            let e = "Service transactions are not allowed. You need to enable Certifier contract.";
                            warn!(target: "client", "Service tx checker error: {:?}", e);
                            bail!(e);
                        }
                        Some(ref checker) => match checker.check(client, &t) {
                            Ok(true) => {}
                            Ok(false) => {
                                let e = format!(
                                    "Service transactions are not allowed for the sender {:?}",
                                    t.sender()
                                );
                                warn!(target: "client", "Service tx checker error: {:?}", e);
                                bail!(e);
                            }
                            Err(e) => {
                                debug!(target: "client", "Unable to verify service transaction: {:?}", e);
                                warn!(target: "client", "Service tx checker error: {:?}", e);
                                bail!(e);
                            }
                        },
                    }
                };
            }
        }

        // t_nb 8.0 Block enacting. Execution of transactions.
        let enact_result = enact_verified(
            block,
            engine,
            client.tracedb.read().tracing_enabled(),
            db,
            &parent,
            last_hashes,
            client.factories.clone(),
            is_epoch_begin,
            &mut chain.ancestry_with_metadata_iter(*header.parent_hash()),
        );

        let mut locked_block = match enact_result {
            Ok(b) => b,
            Err(e) => {
                warn!(target: "client", "Block import failed for #{} ({})\nError: {:?}", header.number(), header.hash(), e);
                bail!(e);
            }
        };

        // t_nb 7.6 Strip receipts for blocks before validate_receipts_transition,
        // if the expected receipts root header does not match.
        // (i.e. allow inconsistency in receipts outcome before the transition block)
        if header.number() < engine.params().validate_receipts_transition
            && header.receipts_root() != locked_block.header.receipts_root()
        {
            locked_block.strip_receipts_outcomes();
        }

        // t_nb 7.7 Final Verification. See if block that we created (executed) matches exactly with block that we received.
        if let Err(e) = self
            .verifier
            .verify_block_final(&header, &locked_block.header)
        {
            warn!(target: "client", "Stage 5 block verification failed for #{} ({})\nError: {:?}", header.number(), header.hash(), e);
            bail!(e);
        }

        let pending = self.check_epoch_end_signal(
            &header,
            bytes,
            &locked_block.receipts,
            locked_block.state.db(),
            client,
        )?;

        Ok((locked_block, pending))
    }

    /// Import a block with transaction receipts.
    ///
    /// The block is guaranteed to be the next best blocks in the
    /// first block sequence. Does no sealing or transaction validation.
    fn import_old_block(
        &self,
        unverified: Unverified,
        receipts_bytes: &[u8],
        db: &dyn KeyValueDB,
        chain: &BlockChain,
    ) -> EthcoreResult<()> {
        let receipts = TypedReceipt::decode_rlp_list(&Rlp::new(receipts_bytes))
            .unwrap_or_else(|e| panic!("Receipt bytes should be valid: {:?}", e));
        let _import_lock = self.import_lock.lock();

        if unverified.header.number() >= chain.best_block_header().number() {
            panic!("Ancient block number is higher then best block number");
        }

        {
            trace_time!("import_old_block");
            // verify the block, passing the chain for updating the epoch verifier.
            let mut rng = OsRng;
            self.ancient_verifier
                .verify(&mut rng, &unverified.header, &chain)?;

            // Commit results
            let mut batch = DBTransaction::new();
            chain.insert_unordered_block(
                &mut batch,
                encoded::Block::new(unverified.bytes),
                receipts,
                None,
                false,
                true,
            );
            // Final commit to the DB
            db.write_buffered(batch);
            chain.commit();
        }
        db.flush().expect("DB flush failed.");
        Ok(())
    }

    // NOTE: the header of the block passed here is not necessarily sealed, as
    // it is for reconstructing the state transition.
    //
    // The header passed is from the original block data and is sealed.
    // TODO: should return an error if ImportRoute is none, issue #9910
    fn commit_block<B>(
        &self,
        block: B,
        header: &Header,
        block_data: encoded::Block,
        pending: Option<PendingTransition>,
        client: &Client,
    ) -> ImportRoute
    where
        B: Drain,
    {
        let hash = &header.hash();
        let number = header.number();
        let parent = header.parent_hash();
        let chain = client.chain.read();
        let mut is_finalized = false;

        // Commit results
        let block = block.drain();
        debug_assert_eq!(header.hash(), block_data.header_view().hash());

        let mut batch = DBTransaction::new();

        // t_nb 9.1 Gather all ancestry actions. (Used only by AuRa)
        let ancestry_actions = self
            .engine
            .ancestry_actions(&header, &mut chain.ancestry_with_metadata_iter(*parent));

        let receipts = block.receipts;
        let traces = block.traces.drain();
        let best_hash = chain.best_block_hash();

        let new = ExtendedHeader {
            header: header.clone(),
            is_finalized,
            parent_total_difficulty: chain
                .block_details(&parent)
                .expect("Parent block is in the database; qed")
                .total_difficulty,
        };

        let best = {
            let hash = best_hash;
            let header = chain
                .block_header_data(&hash)
                .expect("Best block is in the database; qed")
                .decode(self.engine.params().eip1559_transition)
                .expect("Stored block header is valid RLP; qed");
            let details = chain
                .block_details(&hash)
                .expect("Best block is in the database; qed");

            ExtendedHeader {
                parent_total_difficulty: details.total_difficulty - *header.difficulty(),
                is_finalized: details.is_finalized,
                header: header,
            }
        };

        // t_nb 9.2 calcuate route between current and latest block.
        let route = chain.tree_route(best_hash, *parent).expect("forks are only kept when it has common ancestors; tree route from best to prospective's parent always exists; qed");

        // t_nb 9.3 Check block total difficulty
        let fork_choice = if route.is_from_route_finalized {
            ForkChoice::Old
        } else {
            self.engine.fork_choice(&new, &best)
        };

        // t_nb 9.4 CHECK! I *think* this is fine, even if the state_root is equal to another
        // already-imported block of the same number.
        // TODO: Prove it with a test.
        let mut state = block.state.drop().1;

        // t_nb 9.5 check epoch end signal, potentially generating a proof on the current
        // state. Write transition into db.
        if let Some(pending) = pending {
            chain.insert_pending_transition(&mut batch, header.hash(), pending);
        }

        // t_nb 9.6 push state to database Transaction. (It calls journal_under from JournalDB)
        state
            .journal_under(&mut batch, number, hash)
            .expect("DB commit failed");

        let finalized: Vec<_> = ancestry_actions
            .into_iter()
            .map(|ancestry_action| {
                let AncestryAction::MarkFinalized(a) = ancestry_action;

                if a != header.hash() {
                    // t_nb 9.7 if there are finalized ancester, mark that chainge in block in db. (Used by AuRa)
                    chain
                        .mark_finalized(&mut batch, a)
                        .expect("Engine's ancestry action must be known blocks; qed");
                } else {
                    // we're finalizing the current block
                    is_finalized = true;
                }

                a
            })
            .collect();

        // t_nb 9.8 insert block
        let route = chain.insert_block(
            &mut batch,
            block_data,
            receipts.clone(),
            ExtrasInsert {
                fork_choice: fork_choice,
                is_finalized,
            },
        );

        // t_nb 9.9 insert traces (if they are enabled)
        client.tracedb.read().import(
            &mut batch,
            TraceImportRequest {
                traces: traces.into(),
                block_hash: hash.clone(),
                block_number: number,
                enacted: route.enacted.clone(),
                retracted: route.retracted.len(),
            },
        );

        let is_canon = route.enacted.last().map_or(false, |h| h == hash);

        // t_nb 9.10 sync cache
        state.sync_cache(&route.enacted, &route.retracted, is_canon);
        // Final commit to the DB
        // t_nb 9.11 Write Transaction to database (cached)
        client.db.read().key_value().write_buffered(batch);
        // t_nb 9.12 commit changed to become current greatest by applying pending insertion updates (Sync point)
        chain.commit();

        // t_nb 9.13 check epoch end. Related only to AuRa and it seems light engine
        self.check_epoch_end(&header, &finalized, &chain, client);

        // t_nb 9.14 update last hashes. They are build in step 7.5
        client.update_last_hashes(&parent, hash);

        // t_nb 9.15 prune ancient states
        if let Err(e) = client.prune_ancient(state, &chain) {
            warn!("Failed to prune ancient state data: {}", e);
        }

        route
    }

    // check for epoch end signal and write pending transition if it occurs.
    // state for the given block must be available.
    fn check_epoch_end_signal(
        &self,
        header: &Header,
        block_bytes: &[u8],
        receipts: &[TypedReceipt],
        state_db: &StateDB,
        client: &Client,
    ) -> EthcoreResult<Option<PendingTransition>> {
        use engines::EpochChange;

        let hash = header.hash();
        let auxiliary = ::machine::AuxiliaryData {
            bytes: Some(block_bytes),
            receipts: Some(&receipts),
        };

        match self.engine.signals_epoch_end(header, auxiliary) {
            EpochChange::Yes(proof) => {
                use engines::Proof;

                let proof = match proof {
                    Proof::Known(proof) => proof,
                    Proof::WithState(with_state) => {
                        let env_info = EnvInfo {
                            number: header.number(),
                            author: header.author().clone(),
                            timestamp: header.timestamp(),
                            difficulty: header.difficulty().clone(),
                            last_hashes: client.build_last_hashes(header.parent_hash()),
                            gas_used: U256::default(),
                            gas_limit: u64::max_value().into(),
                            base_fee: header.base_fee(),
                        };

                        let call = move |addr, data| {
                            let mut state_db = state_db.boxed_clone();
                            let backend = ::state::backend::Proving::new(state_db.as_hash_db_mut());

                            let transaction = client.contract_call_tx(
                                BlockId::Hash(*header.parent_hash()),
                                addr,
                                data,
                            );

                            let mut state = State::from_existing(
                                backend,
                                header.state_root().clone(),
                                self.engine.account_start_nonce(header.number()),
                                client.factories.clone(),
                            )
                            .expect("state known to be available for just-imported block; qed");

                            let options = TransactOptions::with_no_tracing().dont_check_nonce();
                            let machine = self.engine.machine();
                            let schedule = machine.schedule(env_info.number);
                            let res = Executive::new(&mut state, &env_info, &machine, &schedule)
                                .transact(&transaction, options);

                            let res = match res {
                                Err(e) => {
                                    trace!(target: "client", "Proved call failed: {}", e);
                                    Err(e.to_string())
                                }
                                Ok(res) => Ok((res.output, state.drop().1.extract_proof())),
                            };

                            res.map(|(output, proof)| {
                                (output, proof.into_iter().map(|x| x.into_vec()).collect())
                            })
                        };

                        match with_state.generate_proof(&call) {
                            Ok(proof) => proof,
                            Err(e) => {
                                warn!(target: "client", "Failed to generate transition proof for block {}: {}", hash, e);
                                warn!(target: "client", "Snapshots produced by this client may be incomplete");
                                return Err(EngineError::FailedSystemCall(e).into());
                            }
                        }
                    }
                };

                debug!(target: "client", "Block {} signals epoch end.", hash);

                Ok(Some(PendingTransition { proof: proof }))
            }
            EpochChange::No => Ok(None),
            EpochChange::Unsure(_) => {
                warn!(target: "client", "Detected invalid engine implementation.");
                warn!(target: "client", "Engine claims to require more block data, but everything provided.");
                Err(EngineError::InvalidEngine.into())
            }
        }
    }

    // check for ending of epoch and write transition if it occurs.
    fn check_epoch_end<'a>(
        &self,
        header: &'a Header,
        finalized: &'a [H256],
        chain: &BlockChain,
        client: &Client,
    ) {
        let is_epoch_end = self.engine.is_epoch_end(
            header,
            finalized,
            &(|hash| client.block_header_decoded(BlockId::Hash(hash))),
            &(|hash| chain.get_pending_transition(hash)), // TODO: limit to current epoch.
        );

        if let Some(proof) = is_epoch_end {
            debug!(target: "client", "Epoch transition at block {}", header.hash());

            let mut batch = DBTransaction::new();
            chain.insert_epoch_transition(
                &mut batch,
                header.number(),
                EpochTransition {
                    block_hash: header.hash(),
                    block_number: header.number(),
                    proof: proof,
                },
            );

            // always write the batch directly since epoch transition proofs are
            // fetched from a DB iterator and DB iterators are only available on
            // flushed data.
            client
                .db
                .read()
                .key_value()
                .write(batch)
                .expect("DB flush failed");
        }
    }
}

impl Client {
    /// Create a new client with given parameters.
    /// The database is assumed to have been initialized with the correct columns.
    pub fn new(
        config: ClientConfig,
        spec: &Spec,
        db: Arc<dyn BlockChainDB>,
        miner: Arc<Miner>,
        message_channel: IoChannel<ClientIoMessage>,
    ) -> Result<Arc<Client>, ::error::Error> {
        let trie_spec = match config.fat_db {
            true => TrieSpec::Fat,
            false => TrieSpec::Secure,
        };

        let trie_factory = TrieFactory::new(trie_spec);
        let factories = Factories {
            vm: VmFactory::new(config.vm_type.clone(), config.jump_table_size),
            trie: trie_factory,
            accountdb: Default::default(),
        };

        let journal_db = journaldb::new(db.key_value().clone(), config.pruning, ::db::COL_STATE);
        let mut state_db = StateDB::new(journal_db, config.state_cache_size);
        if state_db.journal_db().is_empty() {
            // Sets the correct state root.
            state_db = spec.ensure_db_good(state_db, &factories)?;
            let mut batch = DBTransaction::new();
            state_db.journal_under(&mut batch, 0, &spec.genesis_header().hash())?;
            db.key_value().write(batch)?;
        }

        let gb = spec.genesis_block();
        let chain = Arc::new(BlockChain::new(
            config.blockchain.clone(),
            &gb,
            db.clone(),
            spec.params().eip1559_transition,
        ));
        let tracedb = RwLock::new(TraceDB::new(
            config.tracing.clone(),
            db.clone(),
            chain.clone(),
        ));

        trace!(
            "Cleanup journal: DB Earliest = {:?}, Latest = {:?}",
            state_db.journal_db().earliest_era(),
            state_db.journal_db().latest_era()
        );

        let history = if config.history < MIN_HISTORY_SIZE {
            info!(target: "client", "Ignoring pruning history parameter of {}\
				, falling back to minimum of {}",
				config.history, MIN_HISTORY_SIZE);
            MIN_HISTORY_SIZE
        } else {
            config.history
        };

        if !chain
            .block_header_data(&chain.best_block_hash())
            .map_or(true, |h| state_db.journal_db().contains(&h.state_root()))
        {
            warn!(
                "State root not found for block #{} ({:x})",
                chain.best_block_number(),
                chain.best_block_hash()
            );
        }

        let engine = spec.engine.clone();

        let awake = match config.mode {
            Mode::Dark(..) | Mode::Off => false,
            _ => true,
        };

        let importer = Importer::new(&config, engine.clone(), message_channel.clone(), miner)?;

        let registrar_address = engine
            .additional_params()
            .get("registrar")
            .and_then(|s| Address::from_str(s).ok());
        if let Some(ref addr) = registrar_address {
            trace!(target: "client", "Found registrar at {}", addr);
        }

        let client = Arc::new(Client {
            enabled: AtomicBool::new(true),
            sleep_state: Mutex::new(SleepState::new(awake)),
            liveness: AtomicBool::new(awake),
            mode: Mutex::new(config.mode.clone()),
            chain: RwLock::new(chain),
            tracedb,
            engine,
            pruning: config.pruning.clone(),
            snapshotting_at: AtomicU64::new(0),
            db: RwLock::new(db.clone()),
            state_db: RwLock::new(state_db),
            report: RwLock::new(Default::default()),
            io_channel: RwLock::new(message_channel),
            notify: RwLock::new(Vec::new()),
            queue_transactions: IoChannelQueue::new(config.transaction_verification_queue_size),
            queued_ancient_blocks: Default::default(),
            queued_ancient_blocks_executer: Default::default(),
            queue_consensus_message: IoChannelQueue::new(usize::max_value()),
            last_hashes: RwLock::new(VecDeque::new()),
            factories,
            history,
            on_user_defaults_change: Mutex::new(None),
            registrar_address,
            exit_handler: Mutex::new(None),
            importer,
            config,
        });

        let exec_client = client.clone();

        let queued = client.queued_ancient_blocks.clone();
        let queued_ancient_blocks_executer = ExecutionQueue::new(
            ANCIENT_BLOCKS_QUEUE_SIZE,
            ANCIENT_BLOCKS_BATCH_SIZE,
            move |ancient_block: Vec<(Unverified, Bytes)>| {
                trace_time!("import_ancient_block");
                for (unverified, receipts_bytes) in ancient_block {
                    let hash = unverified.hash();
                    if !exec_client.chain.read().is_known(&unverified.parent_hash()) {
                        queued.write().remove(&hash);
                        continue;
                    }
                    let result = exec_client.importer.import_old_block(
                        unverified,
                        &receipts_bytes,
                        &**exec_client.db.read().key_value(),
                        &*exec_client.chain.read(),
                    );
                    if let Err(e) = result {
                        error!(target: "client", "Error importing ancient block: {}", e);

                        let mut queued = queued.write();
                        queued.clear();
                    }
                    // remove from pending
                    queued.write().remove(&hash);
                }
            },
            "ancient_block_exec",
        );

        client
            .queued_ancient_blocks_executer
            .lock()
            .replace(queued_ancient_blocks_executer);

        // prune old states.
        {
            let state_db = client.state_db.read().boxed_clone();
            let chain = client.chain.read();
            client.prune_ancient(state_db, &chain)?;
        }

        // ensure genesis epoch proof in the DB.
        {
            let chain = client.chain.read();
            let gh = spec.genesis_header();
            if chain.epoch_transition(0, gh.hash()).is_none() {
                trace!(target: "client", "No genesis transition found.");

                let proof = client.with_proving_caller(BlockId::Number(0), |call| {
                    client.engine.genesis_epoch_data(&gh, call)
                });
                let proof = match proof {
                    Ok(proof) => proof,
                    Err(e) => {
                        warn!(target: "client", "Error generating genesis epoch data: {}. Snapshots generated may not be complete.", e);
                        Vec::new()
                    }
                };

                debug!(target: "client", "Obtained genesis transition proof: {:?}", proof);

                let mut batch = DBTransaction::new();
                chain.insert_epoch_transition(
                    &mut batch,
                    0,
                    EpochTransition {
                        block_hash: gh.hash(),
                        block_number: 0,
                        proof: proof,
                    },
                );

                client.db.read().key_value().write_buffered(batch);
            }
        }

        // ensure buffered changes are flushed.
        client.db.read().key_value().flush()?;
        Ok(client)
    }

    /// signals shutdown of application. We do cleanup here.
    pub fn shutdown(&self) {
        let mut abe = self.queued_ancient_blocks_executer.lock();
        if abe.is_some() {
            abe.as_mut().unwrap().end()
        }
        *abe = None;
    }

    /// Wakes up client if it's a sleep.
    pub fn keep_alive(&self) {
        let should_wake = match *self.mode.lock() {
            Mode::Dark(..) | Mode::Passive(..) => true,
            _ => false,
        };
        if should_wake {
            self.wake_up();
            (*self.sleep_state.lock()).last_activity = Some(Instant::now());
        }
    }

    /// Adds an actor to be notified on certain events
    pub fn add_notify(&self, target: Arc<dyn ChainNotify>) {
        self.notify.write().push(Arc::downgrade(&target));
    }

    /// Returns engine reference.
    pub fn engine(&self) -> &dyn EthEngine {
        &*self.engine
    }

    fn notify<F>(&self, f: F)
    where
        F: Fn(&dyn ChainNotify),
    {
        for np in &*self.notify.read() {
            if let Some(n) = np.upgrade() {
                f(&*n);
            }
        }
    }

    /// Register an action to be done if a mode/spec_name change happens.
    pub fn on_user_defaults_change<F>(&self, f: F)
    where
        F: 'static + FnMut(Option<Mode>) + Send,
    {
        *self.on_user_defaults_change.lock() = Some(Box::new(f));
    }

    /// Flush the block import queue.
    pub fn flush_queue(&self) {
        self.importer.block_queue.flush();
        while !self.importer.block_queue.is_empty() {
            self.import_verified_blocks();
        }
    }

    /// The env info as of the best block.
    pub fn latest_env_info(&self) -> EnvInfo {
        self.env_info(BlockId::Latest)
            .expect("Best block header always stored; qed")
    }

    /// The env info as of a given block.
    /// returns `None` if the block unknown.
    pub fn env_info(&self, id: BlockId) -> Option<EnvInfo> {
        self.block_header(id).map(|header| EnvInfo {
            number: header.number(),
            author: header.author(),
            timestamp: header.timestamp(),
            difficulty: header.difficulty(),
            last_hashes: self.build_last_hashes(&header.parent_hash()),
            gas_used: U256::default(),
            gas_limit: header.gas_limit(),
            base_fee: if header.number() >= self.engine.params().eip1559_transition {
                Some(header.base_fee())
            } else {
                None
            },
        })
    }

    fn build_last_hashes(&self, parent_hash: &H256) -> Arc<LastHashes> {
        {
            let hashes = self.last_hashes.read();
            if hashes.front().map_or(false, |h| h == parent_hash) {
                let mut res = Vec::from(hashes.clone());
                res.resize(256, H256::default());
                return Arc::new(res);
            }
        }
        let mut last_hashes = LastHashes::new();
        last_hashes.resize(256, H256::default());
        last_hashes[0] = parent_hash.clone();
        let chain = self.chain.read();
        for i in 0..255 {
            match chain.block_details(&last_hashes[i]) {
                Some(details) => {
                    last_hashes[i + 1] = details.parent.clone();
                }
                None => break,
            }
        }
        let mut cached_hashes = self.last_hashes.write();
        *cached_hashes = VecDeque::from(last_hashes.clone());
        Arc::new(last_hashes)
    }

    /// This is triggered by a message coming from a block queue when the block is ready for insertion
    pub fn import_verified_blocks(&self) -> usize {
        self.importer.import_verified_blocks(self)
    }

    // use a state-proving closure for the given block.
    fn with_proving_caller<F, T>(&self, id: BlockId, with_call: F) -> T
    where
        F: FnOnce(&::machine::Call) -> T,
    {
        let call = |a, d| {
            let tx = self.contract_call_tx(id, a, d);
            let (result, items) = self
                .prove_transaction(tx, id)
                .ok_or_else(|| format!("Unable to make call. State unavailable?"))?;

            let items = items.into_iter().map(|x| x.to_vec()).collect();
            Ok((result, items))
        };

        with_call(&call)
    }

    // t_nb 9.15 prune ancient states until below the memory limit or only the minimum amount remain.
    fn prune_ancient(
        &self,
        mut state_db: StateDB,
        chain: &BlockChain,
    ) -> Result<(), ::error::Error> {
        let latest_era = match state_db.journal_db().latest_era() {
            Some(n) => n,
            None => return Ok(()),
        };

        // prune all ancient eras until we're below the memory target,
        // but have at least the minimum number of states.
        loop {
            let needs_pruning = state_db.journal_db().is_pruned()
                && state_db.journal_db().journal_size() >= self.config.history_mem;

            if !needs_pruning {
                break;
            }
            match state_db.journal_db().earliest_era() {
                Some(earliest_era) if earliest_era + self.history <= latest_era => {
                    let freeze_at = self.snapshotting_at.load(AtomicOrdering::SeqCst);
                    if freeze_at > 0 && freeze_at == earliest_era {
                        // Note: journal_db().mem_used() can be used for a more accurate memory
                        // consumption measurement but it can be expensive so sticking with the
                        // faster `journal_size()` instead.
                        trace!(target: "pruning", "Pruning is paused at era {} (snapshot under way); earliest era={}, latest era={}, journal_size={} – Not pruning.",
						       freeze_at, earliest_era, latest_era, state_db.journal_db().journal_size());
                        break;
                    }
                    trace!(target: "client", "Pruning state for ancient era {}", earliest_era);
                    match chain.block_hash(earliest_era) {
                        Some(ancient_hash) => {
                            let mut batch = DBTransaction::new();
                            state_db.mark_canonical(&mut batch, earliest_era, &ancient_hash)?;
                            self.db.read().key_value().write_buffered(batch);
                            state_db.journal_db().flush();
                        }
                        None => {
                            debug!(target: "client", "Missing expected hash for block {}", earliest_era)
                        }
                    }
                }
                _ => break, // means that every era is kept, no pruning necessary.
            }
        }

        Ok(())
    }

    // t_nb 9.14 update last hashes. They are build in step 7.5
    fn update_last_hashes(&self, parent: &H256, hash: &H256) {
        let mut hashes = self.last_hashes.write();
        if hashes.front().map_or(false, |h| h == parent) {
            if hashes.len() > 255 {
                hashes.pop_back();
            }
            hashes.push_front(hash.clone());
        }
    }

    /// Get shared miner reference.
    #[cfg(test)]
    pub fn miner(&self) -> Arc<Miner> {
        self.importer.miner.clone()
    }

    #[cfg(test)]
    pub fn state_db(&self) -> ::parking_lot::RwLockReadGuard<StateDB> {
        self.state_db.read()
    }

    #[cfg(test)]
    pub fn chain(&self) -> Arc<BlockChain> {
        self.chain.read().clone()
    }

    /// Replace io channel. Useful for testing.
    pub fn set_io_channel(&self, io_channel: IoChannel<ClientIoMessage>) {
        *self.io_channel.write() = io_channel;
    }

    /// Get a copy of the best block's state.
    pub fn latest_state_and_header(&self) -> (State<StateDB>, Header) {
        let mut nb_tries = 5;
        // Here, we are taking latest block and then latest state. If in between those two calls `best` block got prunned app will panic.
        // This is something that should not happend often and it is edge case.
        // Locking read best_block lock would be more straighforward, but can introduce overlaping locks,
        // because of this we are just taking 5 tries to get best state in most cases it will work on first try.
        while nb_tries != 0 {
            let header = self.best_block_header();
            match State::from_existing(
                self.state_db.read().boxed_clone_canon(&header.hash()),
                *header.state_root(),
                self.engine.account_start_nonce(header.number()),
                self.factories.clone(),
            ) {
                Ok(ret) => return (ret, header),
                Err(_) => {
                    warn!("Couldn't fetch state of best block header: {:?}", header);
                    nb_tries -= 1;
                }
            }
        }
        panic!("Couldn't get latest state in 5 tries");
    }

    /// Attempt to get a copy of a specific block's final state.
    ///
    /// This will not fail if given BlockId::Latest.
    /// Otherwise, this can fail (but may not) if the DB prunes state or the block
    /// is unknown.
    pub fn state_at(&self, id: BlockId) -> Option<State<StateDB>> {
        // fast path for latest state.
        if let BlockId::Latest = id {
            let (state, _) = self.latest_state_and_header();
            return Some(state);
        }

        let block_number = self.block_number(id)?;

        self.block_header(id).and_then(|header| {
            let db = self.state_db.read().boxed_clone();

            // early exit for pruned blocks
            if db.is_pruned() && self.pruning_info().earliest_state > block_number {
                return None;
            }

            let root = header.state_root();
            State::from_existing(
                db,
                root,
                self.engine.account_start_nonce(block_number),
                self.factories.clone(),
            )
            .ok()
        })
    }

    /// Attempt to get a copy of a specific block's beginning state.
    ///
    /// This will not fail if given BlockId::Latest.
    /// Otherwise, this can fail (but may not) if the DB prunes state.
    pub fn state_at_beginning(&self, id: BlockId) -> Option<State<StateDB>> {
        match self.block_number(id) {
            None => None,
            Some(0) => self.state_at(id),
            Some(n) => self.state_at(BlockId::Number(n - 1)),
        }
    }

    /// Get a copy of the best block's state.
    pub fn state(&self) -> impl StateInfo {
        let (state, _) = self.latest_state_and_header();
        state
    }

    /// Get info on the cache.
    pub fn blockchain_cache_info(&self) -> BlockChainCacheSize {
        self.chain.read().cache_size()
    }

    /// Get the report.
    pub fn report(&self) -> ClientReport {
        let mut report = self.report.read().clone();
        self.state_db.read().get_sizes(&mut report.item_sizes);
        report
    }

    /// Tick the client.
    // TODO: manage by real events.
    pub fn tick(&self, prevent_sleep: bool) {
        self.check_garbage();
        if !prevent_sleep {
            self.check_snooze();
        }
    }

    fn check_garbage(&self) {
        self.chain.read().collect_garbage();
        self.importer.block_queue.collect_garbage();
        self.tracedb.read().collect_garbage();
    }

    fn check_snooze(&self) {
        let mode = self.mode.lock().clone();
        match mode {
            Mode::Dark(timeout) => {
                let mut ss = self.sleep_state.lock();
                if let Some(t) = ss.last_activity {
                    if Instant::now() > t + timeout {
                        self.sleep(false);
                        ss.last_activity = None;
                    }
                }
            }
            Mode::Passive(timeout, wakeup_after) => {
                let mut ss = self.sleep_state.lock();
                let now = Instant::now();
                if let Some(t) = ss.last_activity {
                    if now > t + timeout {
                        self.sleep(false);
                        ss.last_activity = None;
                        ss.last_autosleep = Some(now);
                    }
                }
                if let Some(t) = ss.last_autosleep {
                    if now > t + wakeup_after {
                        self.wake_up();
                        ss.last_activity = Some(now);
                        ss.last_autosleep = None;
                    }
                }
            }
            _ => {}
        }
    }

    /// Take a snapshot at the given block.
    /// If the ID given is "latest", this will default to 1000 blocks behind.
    pub fn take_snapshot<W: snapshot_io::SnapshotWriter + Send>(
        &self,
        writer: W,
        at: BlockId,
        p: &snapshot::Progress,
    ) -> Result<(), EthcoreError> {
        let db = self.state_db.read().journal_db().boxed_clone();
        let block_number = self
            .block_number(at)
            .ok_or_else(|| snapshot::Error::InvalidStartingBlock(at))?;

        if db.is_pruned() && self.pruning_info().earliest_state > block_number {
            return Err(snapshot::Error::OldBlockPrunedDB.into());
        }

        let history = ::std::cmp::min(self.history, 1000);

        let (snapshot_block_number, start_hash) = match at {
            BlockId::Latest => {
                let best_block_number = self.chain_info().best_block_number;
                let start_num = match db.earliest_era() {
                    Some(era) => ::std::cmp::max(era, best_block_number.saturating_sub(history)),
                    None => best_block_number.saturating_sub(history),
                };

                match self.block_hash(BlockId::Number(start_num)) {
                    Some(h) => (start_num, h),
                    None => return Err(snapshot::Error::InvalidStartingBlock(at).into()),
                }
            }
            _ => match self.block_hash(at) {
                Some(hash) => (block_number, hash),
                None => return Err(snapshot::Error::InvalidStartingBlock(at).into()),
            },
        };

        let processing_threads = self.config.snapshot.processing_threads;
        let chunker = self
            .engine
            .snapshot_components()
            .ok_or(snapshot::Error::SnapshotsUnsupported)?;
        self.snapshotting_at
            .store(snapshot_block_number, AtomicOrdering::SeqCst);
        {
            scopeguard::defer! {{
                info!(target: "snapshot", "Re-enabling pruning.");
                self.snapshotting_at.store(0, AtomicOrdering::SeqCst)
            }};
            snapshot::take_snapshot(
                chunker,
                &self.chain.read(),
                start_hash,
                db.as_hash_db(),
                writer,
                p,
                processing_threads,
            )?;
        }
        Ok(())
    }

    /// Ask the client what the history parameter is.
    pub fn pruning_history(&self) -> u64 {
        self.history
    }

    fn block_hash(chain: &BlockChain, id: BlockId) -> Option<H256> {
        match id {
            BlockId::Hash(hash) => Some(hash),
            BlockId::Number(number) => chain.block_hash(number),
            BlockId::Earliest => chain.block_hash(0),
            BlockId::Latest => Some(chain.best_block_hash()),
        }
    }

    fn transaction_address(&self, id: TransactionId) -> Option<TransactionAddress> {
        match id {
            TransactionId::Hash(ref hash) => self.chain.read().transaction_address(hash),
            TransactionId::Location(id, index) => {
                Self::block_hash(&self.chain.read(), id).map(|hash| TransactionAddress {
                    block_hash: hash,
                    index: index,
                })
            }
        }
    }

    fn wake_up(&self) {
        if !self.liveness.load(AtomicOrdering::SeqCst) {
            self.liveness.store(true, AtomicOrdering::SeqCst);
            self.notify(|n| n.start());
            info!(target: "mode", "wake_up: Waking.");
        }
    }

    fn sleep(&self, force: bool) {
        if self.liveness.load(AtomicOrdering::SeqCst) {
            // only sleep if the import queue is mostly empty.
            if force || (self.queue_info().total_queue_size() <= MAX_QUEUE_SIZE_TO_SLEEP_ON) {
                self.liveness.store(false, AtomicOrdering::SeqCst);
                self.notify(|n| n.stop());
                info!(target: "mode", "sleep: Sleeping.");
            } else {
                info!(target: "mode", "sleep: Cannot sleep - syncing ongoing.");
                // TODO: Consider uncommenting.
                //(*self.sleep_state.lock()).last_activity = Some(Instant::now());
            }
        }
    }

    // transaction for calling contracts from services like engine.
    // from the null sender, with 50M gas.
    fn contract_call_tx(
        &self,
        block_id: BlockId,
        address: Address,
        data: Bytes,
    ) -> SignedTransaction {
        let from = Address::default();
        TypedTransaction::Legacy(transaction::Transaction {
            nonce: self
                .nonce(&from, block_id)
                .unwrap_or_else(|| self.engine.account_start_nonce(0)),
            action: Action::Call(address),
            gas: U256::from(50_000_000),
            gas_price: U256::default(),
            value: U256::default(),
            data: data,
        })
        .fake_sign(from)
    }

    fn do_virtual_call(
        machine: &::machine::EthereumMachine,
        env_info: &EnvInfo,
        state: &mut State<StateDB>,
        t: &SignedTransaction,
        analytics: CallAnalytics,
    ) -> Result<Executed, CallError> {
        fn call<V, T>(
            state: &mut State<StateDB>,
            env_info: &EnvInfo,
            machine: &::machine::EthereumMachine,
            state_diff: bool,
            transaction: &SignedTransaction,
            options: TransactOptions<T, V>,
        ) -> Result<Executed<T::Output, V::Output>, CallError>
        where
            T: trace::Tracer,
            V: trace::VMTracer,
        {
            let options = options.dont_check_nonce().save_output_from_contract();
            let original_state = if state_diff {
                Some(state.clone())
            } else {
                None
            };
            let schedule = machine.schedule(env_info.number);

            let mut ret = Executive::new(state, env_info, &machine, &schedule)
                .transact_virtual(transaction, options)?;

            if let Some(original) = original_state {
                ret.state_diff = Some(state.diff_from(original).map_err(ExecutionError::from)?);
            }
            Ok(ret)
        }

        let state_diff = analytics.state_diffing;

        match (analytics.transaction_tracing, analytics.vm_tracing) {
            (true, true) => call(
                state,
                env_info,
                machine,
                state_diff,
                t,
                TransactOptions::with_tracing_and_vm_tracing(),
            ),
            (true, false) => call(
                state,
                env_info,
                machine,
                state_diff,
                t,
                TransactOptions::with_tracing(),
            ),
            (false, true) => call(
                state,
                env_info,
                machine,
                state_diff,
                t,
                TransactOptions::with_vm_tracing(),
            ),
            (false, false) => call(
                state,
                env_info,
                machine,
                state_diff,
                t,
                TransactOptions::with_no_tracing(),
            ),
        }
    }

    fn block_number_ref(&self, id: &BlockId) -> Option<BlockNumber> {
        match *id {
            BlockId::Number(number) => Some(number),
            BlockId::Hash(ref hash) => self.chain.read().block_number(hash),
            BlockId::Earliest => Some(0),
            BlockId::Latest => Some(self.chain.read().best_block_number()),
        }
    }

    /// Retrieve a decoded header given `BlockId`
    ///
    /// This method optimizes access patterns for latest block header
    /// to avoid excessive RLP encoding, decoding and hashing.
    fn block_header_decoded(&self, id: BlockId) -> Option<Header> {
        match id {
            BlockId::Latest => Some(self.chain.read().best_block_header()),
            BlockId::Hash(ref hash) if hash == &self.chain.read().best_block_hash() => {
                Some(self.chain.read().best_block_header())
            }
            BlockId::Number(number) if number == self.chain.read().best_block_number() => {
                Some(self.chain.read().best_block_header())
            }
            _ => self
                .block_header(id)
                .and_then(|h| h.decode(self.engine.params().eip1559_transition).ok()),
        }
    }
}

impl snapshot::DatabaseRestore for Client {
    /// Restart the client with a new backend
    fn restore_db(&self, new_db: &str) -> Result<(), EthcoreError> {
        trace!(target: "snapshot", "Replacing client database with {:?}", new_db);

        let _import_lock = self.importer.import_lock.lock();
        let mut state_db = self.state_db.write();
        let mut chain = self.chain.write();
        let mut tracedb = self.tracedb.write();
        self.importer.miner.clear();
        let db = self.db.write();
        db.restore(new_db)?;

        let cache_size = state_db.cache_size();
        *state_db = StateDB::new(
            journaldb::new(db.key_value().clone(), self.pruning, ::db::COL_STATE),
            cache_size,
        );
        *chain = Arc::new(BlockChain::new(
            self.config.blockchain.clone(),
            &[],
            db.clone(),
            self.engine.params().eip1559_transition,
        ));
        *tracedb = TraceDB::new(self.config.tracing.clone(), db.clone(), chain.clone());
        Ok(())
    }
}

impl BlockChainReset for Client {
    fn reset(&self, num: u32) -> Result<(), String> {
        if num as u64 > self.pruning_history() {
            return Err("Attempting to reset to block with pruned state".into());
        } else if num == 0 {
            return Err("invalid number of blocks to reset".into());
        }

        let mut blocks_to_delete = Vec::with_capacity(num as usize);
        let mut best_block_hash = self.chain.read().best_block_hash();
        let mut batch = DBTransaction::with_capacity(blocks_to_delete.len());

        for _ in 0..num {
            let current_header = self
                .chain
                .read()
                .block_header_data(&best_block_hash)
                .expect(
                "best_block_hash was fetched from db; block_header_data should exist in db; qed",
            );
            best_block_hash = current_header.parent_hash();

            let (number, hash) = (current_header.number(), current_header.hash());
            batch.delete(::db::COL_HEADERS, hash.as_bytes());
            batch.delete(::db::COL_BODIES, hash.as_bytes());
            Writable::delete::<BlockDetails, H264>(&mut batch, ::db::COL_EXTRA, &hash);
            Writable::delete::<H256, BlockNumberKey>(&mut batch, ::db::COL_EXTRA, &number);

            blocks_to_delete.push((number, hash));
        }

        let hashes = blocks_to_delete
            .iter()
            .map(|(_, hash)| hash)
            .collect::<Vec<_>>();
        info!(
            "Deleting block hashes {}",
            Colour::Red.bold().paint(format!("{:#?}", hashes))
        );

        let mut best_block_details = Readable::read::<BlockDetails, H264>(
            &**self.db.read().key_value(),
            ::db::COL_EXTRA,
            &best_block_hash,
        )
        .expect("block was previously imported; best_block_details should exist; qed");

        let (_, last_hash) = blocks_to_delete
            .last()
            .expect("num is > 0; blocks_to_delete can't be empty; qed");
        // remove the last block as a child so that it can be re-imported
        // ethcore/blockchain/src/blockchain.rs/Blockchain::is_known_child()
        best_block_details.children.retain(|h| *h != *last_hash);
        batch.write(::db::COL_EXTRA, &best_block_hash, &best_block_details);
        // update the new best block hash
        batch.put(::db::COL_EXTRA, b"best", best_block_hash.as_bytes());

        self.db
            .read()
            .key_value()
            .write(batch)
            .map_err(|err| format!("could not delete blocks; io error occurred: {}", err))?;

        info!(
            "New best block hash {}",
            Colour::Green.bold().paint(format!("{:?}", best_block_hash))
        );

        Ok(())
    }
}

impl Nonce for Client {
    fn nonce(&self, address: &Address, id: BlockId) -> Option<U256> {
        self.state_at(id).and_then(|s| s.nonce(address).ok())
    }
}

impl Balance for Client {
    fn balance(&self, address: &Address, state: StateOrBlock) -> Option<U256> {
        match state {
            StateOrBlock::State(s) => s.balance(address).ok(),
            StateOrBlock::Block(id) => self.state_at(id).and_then(|s| s.balance(address).ok()),
        }
    }
}

impl AccountData for Client {}

impl ChainInfo for Client {
    fn chain_info(&self) -> BlockChainInfo {
        let mut chain_info = self.chain.read().chain_info();
        chain_info.pending_total_difficulty =
            chain_info.total_difficulty + self.importer.block_queue.total_difficulty();
        chain_info
    }
}

impl BlockInfo for Client {
    fn block_header(&self, id: BlockId) -> Option<encoded::Header> {
        let chain = self.chain.read();

        Self::block_hash(&chain, id).and_then(|hash| chain.block_header_data(&hash))
    }

    fn best_block_header(&self) -> Header {
        self.chain.read().best_block_header()
    }

    fn block(&self, id: BlockId) -> Option<encoded::Block> {
        let chain = self.chain.read();

        Self::block_hash(&chain, id).and_then(|hash| chain.block(&hash))
    }

    fn code_hash(&self, address: &Address, id: BlockId) -> Option<H256> {
        self.state_at(id)
            .and_then(|s| s.code_hash(address).unwrap_or(None))
    }
}

impl TransactionInfo for Client {
    fn transaction_block(&self, id: TransactionId) -> Option<H256> {
        self.transaction_address(id).map(|addr| addr.block_hash)
    }
}

impl BlockChainTrait for Client {}

impl RegistryInfo for Client {
    fn registry_address(&self, name: String, block: BlockId) -> Option<Address> {
        use ethabi::FunctionOutputDecoder;

        let address = self.registrar_address?;

        let (data, decoder) = registry::functions::get_address::call(keccak(name.as_bytes()), "A");
        let value = decoder
            .decode(&self.call_contract(block, address, data).ok()?)
            .ok()?;
        if value.is_zero() {
            None
        } else {
            Some(value)
        }
    }
}

impl CallContract for Client {
    fn call_contract(
        &self,
        block_id: BlockId,
        address: Address,
        data: Bytes,
    ) -> Result<Bytes, String> {
        let state_pruned = || CallError::StatePruned.to_string();
        let state = &mut self.state_at(block_id).ok_or_else(&state_pruned)?;
        let header = self
            .block_header_decoded(block_id)
            .ok_or_else(&state_pruned)?;

        let transaction = self.contract_call_tx(block_id, address, data);

        self.call(&transaction, Default::default(), state, &header)
            .map_err(|e| format!("{:?}", e))
            .map(|executed| executed.output)
    }
}

impl ImportBlock for Client {
    // t_nb 2.0 import block to client
    fn import_block(&self, unverified: Unverified) -> EthcoreResult<H256> {
        // t_nb 2.1 check if header hash is known to us.
        if self.chain.read().is_known(&unverified.hash()) {
            bail!(EthcoreErrorKind::Import(ImportErrorKind::AlreadyInChain));
        }

        // t_nb 2.2 check if parent is known
        let status = self.block_status(BlockId::Hash(unverified.parent_hash()));
        if status == BlockStatus::Unknown {
            bail!(EthcoreErrorKind::Block(BlockError::UnknownParent(
                unverified.parent_hash()
            )));
        }

        let raw = if self.importer.block_queue.is_empty() {
            Some((
                unverified.bytes.clone(),
                unverified.header.hash(),
                *unverified.header.difficulty(),
            ))
        } else {
            None
        };

        // t_nb 2.3
        match self.importer.block_queue.import(unverified) {
            Ok(hash) => {
                // t_nb 2.4 If block is okay and the queue is empty we propagate the block in a `PriorityTask` to be rebrodcasted
                if let Some((raw, hash, difficulty)) = raw {
                    self.notify(move |n| n.block_pre_import(&raw, &hash, &difficulty));
                }
                Ok(hash)
            }
            // t_nb 2.5 if block is not okay print error. we only care about block errors (not import errors)
            Err((Some(block), EthcoreError(EthcoreErrorKind::Block(err), _))) => {
                self.importer.bad_blocks.report(
                    block.bytes,
                    err.to_string(),
                    self.engine.params().eip1559_transition,
                );
                bail!(EthcoreErrorKind::Block(err))
            }
            Err((None, EthcoreError(EthcoreErrorKind::Block(err), _))) => {
                error!(target: "client", "BlockError {} detected but it was missing raw_bytes of the block", err);
                bail!(EthcoreErrorKind::Block(err))
            }
            Err((_, e)) => Err(e),
        }
    }
}

impl StateClient for Client {
    type State = State<::state_db::StateDB>;

    fn latest_state_and_header(&self) -> (Self::State, Header) {
        Client::latest_state_and_header(self)
    }

    fn state_at(&self, id: BlockId) -> Option<Self::State> {
        Client::state_at(self, id)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown()
    }
}

impl Call for Client {
    type State = State<::state_db::StateDB>;

    fn call(
        &self,
        transaction: &SignedTransaction,
        analytics: CallAnalytics,
        state: &mut Self::State,
        header: &Header,
    ) -> Result<Executed, CallError> {
        let env_info = EnvInfo {
            number: header.number(),
            author: header.author().clone(),
            timestamp: header.timestamp(),
            difficulty: header.difficulty().clone(),
            last_hashes: self.build_last_hashes(header.parent_hash()),
            gas_used: U256::default(),
            gas_limit: U256::max_value(),
            //if gas pricing is not defined, force base_fee to zero
            base_fee: if transaction.effective_gas_price(header.base_fee()).is_zero() {
                Some(0.into())
            } else {
                header.base_fee()
            },
        };
        let machine = self.engine.machine();

        Self::do_virtual_call(&machine, &env_info, state, transaction, analytics)
    }

    fn call_many(
        &self,
        transactions: &[(SignedTransaction, CallAnalytics)],
        state: &mut Self::State,
        header: &Header,
    ) -> Result<Vec<Executed>, CallError> {
        let mut env_info = EnvInfo {
            number: header.number(),
            author: header.author().clone(),
            timestamp: header.timestamp(),
            difficulty: header.difficulty().clone(),
            last_hashes: self.build_last_hashes(header.parent_hash()),
            gas_used: U256::default(),
            gas_limit: U256::max_value(),
            base_fee: header.base_fee(),
        };

        let mut results = Vec::with_capacity(transactions.len());
        let machine = self.engine.machine();

        for &(ref t, analytics) in transactions {
            //if gas pricing is not defined, force base_fee to zero
            if t.effective_gas_price(header.base_fee()).is_zero() {
                env_info.base_fee = Some(0.into());
            } else {
                env_info.base_fee = header.base_fee()
            }

            let ret = Self::do_virtual_call(machine, &env_info, state, t, analytics)?;
            env_info.gas_used = ret.cumulative_gas_used;
            results.push(ret);
        }

        Ok(results)
    }

    fn estimate_gas(
        &self,
        t: &SignedTransaction,
        state: &Self::State,
        header: &Header,
    ) -> Result<U256, CallError> {
        let (mut upper, max_upper, env_info) = {
            let init = *header.gas_limit();
            let max = init * U256::from(10);

            let env_info = EnvInfo {
                number: header.number(),
                author: header.author().clone(),
                timestamp: header.timestamp(),
                difficulty: header.difficulty().clone(),
                last_hashes: self.build_last_hashes(header.parent_hash()),
                gas_used: U256::default(),
                gas_limit: max,
                base_fee: if t.effective_gas_price(header.base_fee()).is_zero() {
                    Some(0.into())
                } else {
                    header.base_fee()
                },
            };

            (init, max, env_info)
        };

        let sender = t.sender();
        let options = || TransactOptions::with_tracing().dont_check_nonce();

        let exec = |gas| {
            let mut tx = t.as_unsigned().clone();
            tx.tx_mut().gas = gas;
            let tx = tx.fake_sign(sender);

            let mut clone = state.clone();
            let machine = self.engine.machine();
            let schedule = machine.schedule(env_info.number);
            Executive::new(&mut clone, &env_info, &machine, &schedule)
                .transact_virtual(&tx, options())
        };

        let cond = |gas| exec(gas).ok().map_or(false, |r| r.exception.is_none());

        if !cond(upper) {
            upper = max_upper;
            match exec(upper) {
                Ok(v) => {
                    if let Some(exception) = v.exception {
                        return Err(CallError::Exceptional(exception));
                    }
                }
                Err(_e) => {
                    trace!(target: "estimate_gas", "estimate_gas failed with {}", upper);
                    let err = ExecutionError::Internal(format!(
                        "Requires higher than upper limit of {}",
                        upper
                    ));
                    return Err(err.into());
                }
            }
        }
        let lower = t
            .tx()
            .gas_required(&self.engine.schedule(env_info.number))
            .into();
        if cond(lower) {
            trace!(target: "estimate_gas", "estimate_gas succeeded with {}", lower);
            return Ok(lower);
        }

        /// Find transition point between `lower` and `upper` where `cond` changes from `false` to `true`.
        /// Returns the lowest value between `lower` and `upper` for which `cond` returns true.
        /// We assert: `cond(lower) = false`, `cond(upper) = true`
        fn binary_chop<F, E>(mut lower: U256, mut upper: U256, mut cond: F) -> Result<U256, E>
        where
            F: FnMut(U256) -> bool,
        {
            while upper - lower > 1.into() {
                let mid = (lower + upper) / 2;
                trace!(target: "estimate_gas", "{} .. {} .. {}", lower, mid, upper);
                let c = cond(mid);
                match c {
                    true => upper = mid,
                    false => lower = mid,
                };
                trace!(target: "estimate_gas", "{} => {} .. {}", c, lower, upper);
            }
            Ok(upper)
        }

        // binary chop to non-excepting call with gas somewhere between 21000 and block gas limit
        trace!(target: "estimate_gas", "estimate_gas chopping {} .. {}", lower, upper);
        binary_chop(lower, upper, cond)
    }
}

impl EngineInfo for Client {
    fn engine(&self) -> &dyn EthEngine {
        Client::engine(self)
    }
}

impl BadBlocks for Client {
    fn bad_blocks(&self) -> Vec<(Unverified, String)> {
        self.importer
            .bad_blocks
            .bad_blocks(self.engine.params().eip1559_transition)
    }
}

impl BlockChainClient for Client {
    fn replay(&self, id: TransactionId, analytics: CallAnalytics) -> Result<Executed, CallError> {
        let address = self
            .transaction_address(id)
            .ok_or(CallError::TransactionNotFound)?;
        let block = BlockId::Hash(address.block_hash);

        const PROOF: &'static str =
            "The transaction address contains a valid index within block; qed";
        Ok(self
            .replay_block_transactions(block, analytics)?
            .nth(address.index)
            .expect(PROOF)
            .1)
    }

    fn replay_block_transactions(
        &self,
        block: BlockId,
        analytics: CallAnalytics,
    ) -> Result<Box<dyn Iterator<Item = (H256, Executed)>>, CallError> {
        let mut env_info = self.env_info(block).ok_or(CallError::StatePruned)?;
        let body = self.block_body(block).ok_or(CallError::StatePruned)?;
        let mut state = self
            .state_at_beginning(block)
            .ok_or(CallError::StatePruned)?;
        let txs = body.transactions();
        let engine = self.engine.clone();

        const PROOF: &'static str =
            "Transactions fetched from blockchain; blockchain transactions are valid; qed";
        const EXECUTE_PROOF: &'static str = "Transaction replayed; qed";

        Ok(Box::new(txs.into_iter().map(move |t| {
            let transaction_hash = t.hash();
            let t = SignedTransaction::new(t).expect(PROOF);
            let machine = engine.machine();
            let x = Self::do_virtual_call(machine, &env_info, &mut state, &t, analytics)
                .expect(EXECUTE_PROOF);
            env_info.gas_used = env_info.gas_used + x.gas_used;
            (transaction_hash, x)
        })))
    }

    fn mode(&self) -> Mode {
        let r = self.mode.lock().clone().into();
        trace!(target: "mode", "Asked for mode = {:?}. returning {:?}", &*self.mode.lock(), r);
        r
    }

    fn disable(&self) {
        self.set_mode(Mode::Off);
        self.enabled.store(false, AtomicOrdering::SeqCst);
        self.clear_queue();
    }

    fn set_mode(&self, new_mode: Mode) {
        trace!(target: "mode", "Client::set_mode({:?})", new_mode);
        if !self.enabled.load(AtomicOrdering::SeqCst) {
            return;
        }
        {
            let mut mode = self.mode.lock();
            *mode = new_mode.clone().into();
            trace!(target: "mode", "Mode now {:?}", &*mode);
            if let Some(ref mut f) = *self.on_user_defaults_change.lock() {
                trace!(target: "mode", "Making callback...");
                f(Some((&*mode).clone()))
            }
        }
        match new_mode {
            Mode::Active => self.wake_up(),
            Mode::Off => self.sleep(true),
            _ => {
                (*self.sleep_state.lock()).last_activity = Some(Instant::now());
            }
        }
    }

    fn spec_name(&self) -> String {
        self.config.spec_name.clone()
    }

    fn is_canon(&self, hash: &H256) -> bool {
        self.chain.read().is_canon(hash)
    }

    fn set_spec_name(&self, new_spec_name: String) -> Result<(), ()> {
        trace!(target: "mode", "Client::set_spec_name({:?})", new_spec_name);
        if !self.enabled.load(AtomicOrdering::SeqCst) {
            return Err(());
        }
        if let Some(ref h) = *self.exit_handler.lock() {
            (*h)(new_spec_name);
            Ok(())
        } else {
            warn!("Not hypervised; cannot change chain.");
            Err(())
        }
    }

    fn block_number(&self, id: BlockId) -> Option<BlockNumber> {
        self.block_number_ref(&id)
    }

    fn block_body(&self, id: BlockId) -> Option<encoded::Body> {
        let chain = self.chain.read();

        Self::block_hash(&chain, id).and_then(|hash| chain.block_body(&hash))
    }

    fn block_status(&self, id: BlockId) -> BlockStatus {
        let chain = self.chain.read();
        match Self::block_hash(&chain, id) {
            Some(ref hash) if chain.is_known(hash) => BlockStatus::InChain,
            Some(hash) => self.importer.block_queue.status(&hash).into(),
            None => BlockStatus::Unknown,
        }
    }

    fn is_processing_fork(&self) -> bool {
        let chain = self.chain.read();
        self.importer
            .block_queue
            .is_processing_fork(&chain.best_block_hash(), &chain)
    }

    fn block_total_difficulty(&self, id: BlockId) -> Option<U256> {
        let chain = self.chain.read();

        Self::block_hash(&chain, id)
            .and_then(|hash| chain.block_details(&hash))
            .map(|d| d.total_difficulty)
    }

    fn storage_root(&self, address: &Address, id: BlockId) -> Option<H256> {
        self.state_at(id)
            .and_then(|s| s.storage_root(address).ok())
            .and_then(|x| x)
    }

    fn block_hash(&self, id: BlockId) -> Option<H256> {
        let chain = self.chain.read();
        Self::block_hash(&chain, id)
    }

    fn code(&self, address: &Address, state: StateOrBlock) -> Option<Option<Bytes>> {
        let result = match state {
            StateOrBlock::State(s) => s.code(address).ok(),
            StateOrBlock::Block(id) => self.state_at(id).and_then(|s| s.code(address).ok()),
        };

        // Converting from `Option<Option<Arc<Bytes>>>` to `Option<Option<Bytes>>`
        result.map(|c| c.map(|c| (&*c).clone()))
    }

    fn storage_at(&self, address: &Address, position: &H256, state: StateOrBlock) -> Option<H256> {
        match state {
            StateOrBlock::State(s) => s.storage_at(address, position).ok(),
            StateOrBlock::Block(id) => self
                .state_at(id)
                .and_then(|s| s.storage_at(address, position).ok()),
        }
    }

    fn list_accounts(
        &self,
        id: BlockId,
        after: Option<&Address>,
        count: u64,
    ) -> Option<Vec<Address>> {
        if !self.factories.trie.is_fat() {
            trace!(target: "fatdb", "list_accounts: Not a fat DB");
            return None;
        }

        let state = match self.state_at(id) {
            Some(state) => state,
            _ => return None,
        };

        let (root, db) = state.drop();
        let db = &db.as_hash_db();
        let trie = match self.factories.trie.readonly(db, &root) {
            Ok(trie) => trie,
            _ => {
                trace!(target: "fatdb", "list_accounts: Couldn't open the DB");
                return None;
            }
        };

        let mut iter = match trie.iter() {
            Ok(iter) => iter,
            _ => return None,
        };

        if let Some(after) = after {
            if let Err(e) = iter.seek(after.as_bytes()) {
                trace!(target: "fatdb", "list_accounts: Couldn't seek the DB: {:?}", e);
            } else {
                // Position the iterator after the `after` element
                iter.next();
            }
        }

        let accounts = iter
            .filter_map(|item| item.ok().map(|(addr, _)| Address::from_slice(&addr)))
            .take(count as usize)
            .collect();

        Some(accounts)
    }

    fn list_storage(
        &self,
        id: BlockId,
        account: &Address,
        after: Option<&H256>,
        count: u64,
    ) -> Option<Vec<H256>> {
        if !self.factories.trie.is_fat() {
            trace!(target: "fatdb", "list_storage: Not a fat DB");
            return None;
        }

        let state = match self.state_at(id) {
            Some(state) => state,
            _ => return None,
        };

        let root = match state.storage_root(account) {
            Ok(Some(root)) => root,
            _ => return None,
        };

        let (_, db) = state.drop();
        let account_db = &self
            .factories
            .accountdb
            .readonly(db.as_hash_db(), keccak(account));
        let account_db = &account_db.as_hash_db();
        let trie = match self.factories.trie.readonly(account_db, &root) {
            Ok(trie) => trie,
            _ => {
                trace!(target: "fatdb", "list_storage: Couldn't open the DB");
                return None;
            }
        };

        let mut iter = match trie.iter() {
            Ok(iter) => iter,
            _ => return None,
        };

        if let Some(after) = after {
            if let Err(e) = iter.seek(after.as_bytes()) {
                trace!(target: "fatdb", "list_storage: Couldn't seek the DB: {:?}", e);
            } else {
                // Position the iterator after the `after` element
                iter.next();
            }
        }

        let keys = iter
            .filter_map(|item| item.ok().map(|(key, _)| H256::from_slice(&key)))
            .take(count as usize)
            .collect();

        Some(keys)
    }

    fn block_transaction(&self, id: TransactionId) -> Option<LocalizedTransaction> {
        self.transaction_address(id)
            .and_then(|address| self.chain.read().transaction(&address))
    }

    fn queued_transaction(&self, hash: H256) -> Option<Arc<VerifiedTransaction>> {
        self.importer.miner.transaction(&hash)
    }

    fn uncle(&self, id: UncleId) -> Option<encoded::Header> {
        let index = id.position;
        self.block_body(id.block)
            .and_then(|body| body.view().uncle_rlp_at(index))
            .map(encoded::Header::new)
    }

    fn transaction_receipt(&self, id: TransactionId) -> Option<LocalizedReceipt> {
        // NOTE Don't use block_receipts here for performance reasons
        let address = self.transaction_address(id)?;
        let hash = address.block_hash;
        let chain = self.chain.read();
        let number = chain.block_number(&hash)?;
        let body = chain.block_body(&hash)?;
        let header = chain.block_header_data(&hash)?;
        let mut receipts = chain.block_receipts(&hash)?.receipts;
        receipts.truncate(address.index + 1);

        let transaction = body
            .view()
            .localized_transaction_at(&hash, number, address.index)?;
        let receipt = receipts.pop()?;
        let gas_used = receipts.last().map_or_else(|| 0.into(), |r| r.gas_used);
        let no_of_logs = receipts
            .into_iter()
            .map(|receipt| receipt.logs.len())
            .sum::<usize>();
        let base_fee = if number >= self.engine().params().eip1559_transition {
            Some(header.base_fee())
        } else {
            None
        };

        let receipt = transaction_receipt(
            self.engine().machine(),
            transaction,
            receipt,
            gas_used,
            no_of_logs,
            base_fee,
        );
        Some(receipt)
    }

    fn localized_block_receipts(&self, id: BlockId) -> Option<Vec<LocalizedReceipt>> {
        let hash = self.block_hash(id)?;

        let chain = self.chain.read();
        let receipts = chain.block_receipts(&hash)?;
        let number = chain.block_number(&hash)?;
        let body = chain.block_body(&hash)?;
        let header = chain.block_header_data(&hash)?;
        let engine = self.engine.clone();
        let base_fee = if number >= engine.params().eip1559_transition {
            Some(header.base_fee())
        } else {
            None
        };

        let mut gas_used = 0.into();
        let mut no_of_logs = 0;

        Some(
            body.view()
                .localized_transactions(&hash, number)
                .into_iter()
                .zip(receipts.receipts)
                .map(move |(transaction, receipt)| {
                    let result = transaction_receipt(
                        engine.machine(),
                        transaction,
                        receipt,
                        gas_used,
                        no_of_logs,
                        base_fee,
                    );
                    gas_used = result.cumulative_gas_used;
                    no_of_logs += result.logs.len();
                    result
                })
                .collect(),
        )
    }

    fn tree_route(&self, from: &H256, to: &H256) -> Option<TreeRoute> {
        let chain = self.chain.read();
        match chain.is_known(from) && chain.is_known(to) {
            true => chain.tree_route(from.clone(), to.clone()),
            false => None,
        }
    }

    fn find_uncles(&self, hash: &H256) -> Option<Vec<H256>> {
        self.chain.read().find_uncle_hashes(hash, MAX_UNCLE_AGE)
    }

    fn block_receipts(&self, hash: &H256) -> Option<BlockReceipts> {
        self.chain.read().block_receipts(hash)
    }

    fn queue_info(&self) -> BlockQueueInfo {
        self.importer.block_queue.queue_info()
    }

    fn is_queue_empty(&self) -> bool {
        self.importer.block_queue.is_empty()
    }

    fn clear_queue(&self) {
        self.importer.block_queue.clear();
    }

    fn additional_params(&self) -> BTreeMap<String, String> {
        self.engine.additional_params().into_iter().collect()
    }

    fn logs(&self, filter: Filter) -> Result<Vec<LocalizedLogEntry>, BlockId> {
        let chain = self.chain.read();

        // First, check whether `filter.from_block` and `filter.to_block` is on the canon chain. If so, we can use the
        // optimized version.
        let is_canon = |id| {
            match id {
                // If it is referred by number, then it is always on the canon chain.
                &BlockId::Earliest | &BlockId::Latest | &BlockId::Number(_) => true,
                // If it is referred by hash, we see whether a hash -> number -> hash conversion gives us the same
                // result.
                &BlockId::Hash(ref hash) => chain.is_canon(hash),
            }
        };

        let blocks = if is_canon(&filter.from_block) && is_canon(&filter.to_block) {
            // If we are on the canon chain, use bloom filter to fetch required hashes.
            //
            // If we are sure the block does not exist (where val > best_block_number), then return error. Note that we
            // don't need to care about pending blocks here because RPC query sets pending back to latest (or handled
            // pending logs themselves).
            let from = match self.block_number_ref(&filter.from_block) {
                Some(val) if val <= chain.best_block_number() => val,
                _ => return Err(filter.from_block.clone()),
            };
            let to = match self.block_number_ref(&filter.to_block) {
                Some(val) if val <= chain.best_block_number() => val,
                _ => return Err(filter.to_block.clone()),
            };

            // If from is greater than to, then the current bloom filter behavior is to just return empty
            // result. There's no point to continue here.
            if from > to {
                return Err(filter.to_block.clone());
            }

            chain
                .blocks_with_bloom(&filter.bloom_possibilities(), from, to)
                .into_iter()
                .filter_map(|n| chain.block_hash(n))
                .collect::<Vec<H256>>()
        } else {
            // Otherwise, we use a slower version that finds a link between from_block and to_block.
            let from_hash = Self::block_hash(&chain, filter.from_block)
                .ok_or_else(|| filter.from_block.clone())?;
            let from_number = chain
                .block_number(&from_hash)
                .ok_or_else(|| BlockId::Hash(from_hash))?;
            let to_hash =
                Self::block_hash(&chain, filter.to_block).ok_or_else(|| filter.to_block.clone())?;

            let blooms = filter.bloom_possibilities();
            let bloom_match = |header: &encoded::Header| {
                blooms
                    .iter()
                    .any(|bloom| header.log_bloom().contains_bloom(bloom))
            };

            let (blocks, last_hash) = {
                let mut blocks = Vec::new();
                let mut current_hash = to_hash;

                loop {
                    let header = chain
                        .block_header_data(&current_hash)
                        .ok_or_else(|| BlockId::Hash(current_hash))?;
                    if bloom_match(&header) {
                        blocks.push(current_hash);
                    }

                    // Stop if `from` block is reached.
                    if header.number() <= from_number {
                        break;
                    }
                    current_hash = header.parent_hash();
                }

                blocks.reverse();
                (blocks, current_hash)
            };

            // Check if we've actually reached the expected `from` block.
            if last_hash != from_hash || blocks.is_empty() {
                // In this case, from_hash is the cause (for not matching last_hash).
                return Err(BlockId::Hash(from_hash));
            }

            blocks
        };

        Ok(chain.logs(blocks, |entry| filter.matches(entry), filter.limit))
    }

    fn filter_traces(&self, filter: TraceFilter) -> Option<Vec<LocalizedTrace>> {
        if !self.tracedb.read().tracing_enabled() {
            return None;
        }

        let start = self.block_number(filter.range.start)?;
        let end = self.block_number(filter.range.end)?;

        let db_filter = trace::Filter {
            range: start as usize..end as usize,
            from_address: filter.from_address.into(),
            to_address: filter.to_address.into(),
        };

        let traces = self
            .tracedb
            .read()
            .filter(&db_filter)
            .into_iter()
            .skip(filter.after.unwrap_or(0))
            .take(filter.count.unwrap_or(usize::max_value()))
            .collect();
        Some(traces)
    }

    fn trace(&self, trace: TraceId) -> Option<LocalizedTrace> {
        if !self.tracedb.read().tracing_enabled() {
            return None;
        }

        let trace_address = trace.address;
        self.transaction_address(trace.transaction)
            .and_then(|tx_address| {
                self.block_number(BlockId::Hash(tx_address.block_hash))
                    .and_then(|number| {
                        self.tracedb
                            .read()
                            .trace(number, tx_address.index, trace_address)
                    })
            })
    }

    fn transaction_traces(&self, transaction: TransactionId) -> Option<Vec<LocalizedTrace>> {
        if !self.tracedb.read().tracing_enabled() {
            return None;
        }

        self.transaction_address(transaction)
            .and_then(|tx_address| {
                self.block_number(BlockId::Hash(tx_address.block_hash))
                    .and_then(|number| {
                        self.tracedb
                            .read()
                            .transaction_traces(number, tx_address.index)
                    })
            })
    }

    fn block_traces(&self, block: BlockId) -> Option<Vec<LocalizedTrace>> {
        if !self.tracedb.read().tracing_enabled() {
            return None;
        }

        self.block_number(block)
            .and_then(|number| self.tracedb.read().block_traces(number))
    }

    fn last_hashes(&self) -> LastHashes {
        (*self.build_last_hashes(&self.chain.read().best_block_hash())).clone()
    }

    fn transactions_to_propagate(&self) -> Vec<Arc<VerifiedTransaction>> {
        const PROPAGATE_FOR_BLOCKS: u32 = 4;
        const MIN_TX_TO_PROPAGATE: usize = 256;

        let block_gas_limit = *self.best_block_header().gas_limit();
        let min_tx_gas: U256 = self.latest_schedule().tx_gas.into();

        let max_len = if min_tx_gas.is_zero() {
            usize::max_value()
        } else {
            cmp::max(
                MIN_TX_TO_PROPAGATE,
                cmp::min(
                    (block_gas_limit / min_tx_gas) * PROPAGATE_FOR_BLOCKS,
                    // never more than usize
                    usize::max_value().into(),
                )
                .as_u64() as usize,
            )
        };
        self.importer
            .miner
            .ready_transactions(self, max_len, ::miner::PendingOrdering::Priority)
    }

    fn transaction(&self, tx_hash: &H256) -> Option<Arc<VerifiedTransaction>> {
        self.importer.miner.transaction(tx_hash)
    }

    fn signing_chain_id(&self) -> Option<u64> {
        self.engine.signing_chain_id(&self.latest_env_info())
    }

    fn block_extra_info(&self, id: BlockId) -> Option<BTreeMap<String, String>> {
        self.block_header_decoded(id)
            .map(|header| self.engine.extra_info(&header))
    }

    fn uncle_extra_info(&self, id: UncleId) -> Option<BTreeMap<String, String>> {
        self.uncle(id).and_then(|h| {
            h.decode(self.engine.params().eip1559_transition)
                .map(|dh| self.engine.extra_info(&dh))
                .ok()
        })
    }

    fn pruning_info(&self) -> PruningInfo {
        PruningInfo {
            earliest_chain: self.chain.read().first_block_number().unwrap_or(1),
            earliest_state: self
                .state_db
                .read()
                .journal_db()
                .earliest_era()
                .unwrap_or(0),
        }
    }

    fn create_transaction(
        &self,
        TransactionRequest {
            action,
            data,
            gas,
            gas_price,
            nonce,
        }: TransactionRequest,
    ) -> Result<SignedTransaction, transaction::Error> {
        let authoring_params = self.importer.miner.authoring_params();
        let service_transaction_checker = self.importer.miner.service_transaction_checker();
        let gas_price = if let Some(checker) = service_transaction_checker {
            match checker.check_address(self, authoring_params.author) {
                Ok(true) => U256::zero(),
                _ => gas_price.unwrap_or_else(|| self.importer.miner.sensible_gas_price()),
            }
        } else {
            self.importer.miner.sensible_gas_price()
        };
        let transaction = TypedTransaction::Legacy(transaction::Transaction {
            nonce: nonce.unwrap_or_else(|| self.latest_nonce(&authoring_params.author)),
            action,
            gas: gas.unwrap_or_else(|| self.importer.miner.sensible_gas_limit()),
            gas_price,
            value: U256::zero(),
            data,
        });
        let chain_id = self.engine.signing_chain_id(&self.latest_env_info());
        let signature = self
            .engine
            .sign(transaction.signature_hash(chain_id))
            .map_err(|e| transaction::Error::InvalidSignature(e.to_string()))?;
        Ok(SignedTransaction::new(
            transaction.with_signature(signature, chain_id),
        )?)
    }

    fn transact(&self, tx_request: TransactionRequest) -> Result<(), transaction::Error> {
        let signed = self.create_transaction(tx_request)?;
        self.importer
            .miner
            .import_own_transaction(self, signed.into())
    }

    fn registrar_address(&self) -> Option<Address> {
        self.registrar_address.clone()
    }

    fn state_data(&self, hash: &H256) -> Option<Bytes> {
        self.state_db.read().journal_db().state(hash)
    }
}

impl IoClient for Client {
    fn queue_transactions(&self, transactions: Vec<Bytes>, peer_id: usize) {
        trace_time!("queue_transactions");
        let len = transactions.len();
        self.queue_transactions
            .queue(&self.io_channel.read(), len, move |client| {
                trace_time!("import_queued_transactions");
                let best_block_number = client.best_block_header().number();
                let txs: Vec<UnverifiedTransaction> = transactions
                    .iter()
                    .filter_map(|bytes| {
                        client
                            .engine
                            .decode_transaction(bytes, best_block_number)
                            .ok()
                    })
                    .collect();

                client.notify(|notify| {
                    notify.transactions_received(&txs, peer_id);
                });

                client
                    .importer
                    .miner
                    .import_external_transactions(client, txs);
            })
            .unwrap_or_else(|e| {
                debug!(target: "client", "Ignoring {} transactions: {}", len, e);
            });
    }

    fn queue_ancient_block(
        &self,
        unverified: Unverified,
        receipts_bytes: Bytes,
    ) -> EthcoreResult<H256> {
        trace_time!("queue_ancient_block");

        let hash = unverified.hash();
        {
            // check block order
            if self.chain.read().is_known(&hash) {
                bail!(EthcoreErrorKind::Import(ImportErrorKind::AlreadyInChain));
            }
            let parent_hash = unverified.parent_hash();
            // NOTE To prevent race condition with import, make sure to check queued blocks first
            // (and attempt to acquire lock)
            let is_parent_pending = self.queued_ancient_blocks.read().contains(&parent_hash);
            if !is_parent_pending && !self.chain.read().is_known(&parent_hash) {
                bail!(EthcoreErrorKind::Block(BlockError::UnknownParent(
                    parent_hash
                )));
            }
        }

        // we queue blocks here and trigger an Executer.
        {
            let mut queued = self.queued_ancient_blocks.write();
            queued.insert(hash);
        }

        // see content of executer in Client::new()
        match self.queued_ancient_blocks_executer.lock().as_ref() {
            Some(queue) => {
                if !queue.enqueue((unverified, receipts_bytes)) {
                    bail!(EthcoreErrorKind::Queue(QueueErrorKind::Full(
                        ANCIENT_BLOCKS_QUEUE_SIZE
                    )));
                }
            }
            None => (),
        }
        Ok(hash)
    }

    fn ancient_block_queue_fullness(&self) -> f32 {
        match self.queued_ancient_blocks_executer.lock().as_ref() {
            Some(queue) => queue.len() as f32 / ANCIENT_BLOCKS_QUEUE_SIZE as f32,
            None => 1.0, //return 1.0 if queue is not set
        }
    }

    fn queue_consensus_message(&self, message: Bytes) {
        match self
            .queue_consensus_message
            .queue(&self.io_channel.read(), 1, move |client| {
                if let Err(e) = client.engine().handle_message(&message) {
                    debug!(target: "poa", "Invalid message received: {}", e);
                }
            }) {
            Ok(_) => (),
            Err(e) => {
                debug!(target: "poa", "Ignoring the message, error queueing: {}", e);
            }
        }
    }
}

impl ReopenBlock for Client {
    fn reopen_block(&self, block: ClosedBlock) -> OpenBlock {
        let engine = &*self.engine;
        let mut block = block.reopen(engine);
        let max_uncles = engine.maximum_uncle_count(block.header.number());
        if block.uncles.len() < max_uncles {
            let chain = self.chain.read();
            let h = chain.best_block_hash();
            // Add new uncles
            let uncles = chain
                .find_uncle_hashes(&h, MAX_UNCLE_AGE)
                .unwrap_or_else(Vec::new);

            for h in uncles {
                if !block.uncles.iter().any(|header| header.hash() == h) {
                    let uncle = chain
                        .block_header_data(&h)
                        .expect("find_uncle_hashes only returns hashes for existing headers; qed");
                    let uncle = uncle
                        .decode(self.engine.params().eip1559_transition)
                        .expect("decoding failure");
                    block.push_uncle(uncle).expect(
                        "pushing up to maximum_uncle_count;
												push_uncle is not ok only if more than maximum_uncle_count is pushed;
												so all push_uncle are Ok;
												qed",
                    );
                    if block.uncles.len() >= max_uncles {
                        break;
                    }
                }
            }
        }
        block
    }
}

impl PrepareOpenBlock for Client {
    fn prepare_open_block(
        &self,
        author: Address,
        gas_range_target: (U256, U256),
        extra_data: Bytes,
    ) -> Result<OpenBlock, EthcoreError> {
        let engine = &*self.engine;
        let chain = self.chain.read();
        let best_header = chain.best_block_header();
        let h = best_header.hash();

        let is_epoch_begin = chain.epoch_transition(best_header.number(), h).is_some();
        let mut open_block = OpenBlock::new(
            engine,
            self.factories.clone(),
            self.tracedb.read().tracing_enabled(),
            self.state_db.read().boxed_clone_canon(&h),
            &best_header,
            self.build_last_hashes(&h),
            author,
            gas_range_target,
            extra_data,
            is_epoch_begin,
            chain.ancestry_with_metadata_iter(best_header.hash()),
        )?;

        // Add uncles
        chain
            .find_uncle_headers(&h, MAX_UNCLE_AGE)
            .unwrap_or_else(Vec::new)
            .into_iter()
            .take(engine.maximum_uncle_count(open_block.header.number()))
            .foreach(|h| {
                open_block
                    .push_uncle(
                        h.decode(engine.params().eip1559_transition)
                            .expect("decoding failure"),
                    )
                    .expect(
                        "pushing maximum_uncle_count;
												open_block was just created;
												push_uncle is not ok only if more than maximum_uncle_count is pushed;
												so all push_uncle are Ok;
												qed",
                    );
            });

        Ok(open_block)
    }
}

impl BlockProducer for Client {}

impl ScheduleInfo for Client {
    fn latest_schedule(&self) -> Schedule {
        self.engine.schedule(self.latest_env_info().number)
    }
}

impl ImportSealedBlock for Client {
    fn import_sealed_block(&self, block: SealedBlock) -> EthcoreResult<H256> {
        let start = Instant::now();
        let raw = block.rlp_bytes();
        let header = block.header.clone();
        let hash = header.hash();
        self.notify(|n| n.block_pre_import(&raw, &hash, header.difficulty()));

        let route = {
            // Do a super duper basic verification to detect potential bugs
            if let Err(e) = self.engine.verify_block_basic(&header) {
                self.importer.bad_blocks.report(
                    block.rlp_bytes(),
                    format!("Detected an issue with locally sealed block: {}", e),
                    self.engine.params().eip1559_transition,
                );
                return Err(e.into());
            }

            // scope for self.import_lock
            let _import_lock = self.importer.import_lock.lock();
            trace_time!("import_sealed_block");

            let block_data = block.rlp_bytes();

            let pending = self.importer.check_epoch_end_signal(
                &header,
                &block_data,
                &block.receipts,
                block.state.db(),
                self,
            )?;
            let route = self.importer.commit_block(
                block,
                &header,
                encoded::Block::new(block_data),
                pending,
                self,
            );
            trace!(target: "client", "Imported sealed block #{} ({})", header.number(), hash);
            self.state_db
                .write()
                .sync_cache(&route.enacted, &route.retracted, false);
            route
        };
        let route = ChainRoute::from([route].as_ref());
        self.importer.miner.chain_new_blocks(
            self,
            &[hash],
            &[],
            route.enacted(),
            route.retracted(),
            self.engine.sealing_state() != SealingState::External,
        );
        self.notify(|notify| {
            notify.new_blocks(NewBlocks::new(
                vec![hash],
                vec![],
                route.clone(),
                vec![hash],
                vec![],
                start.elapsed(),
                false,
            ));
        });
        self.db
            .read()
            .key_value()
            .flush()
            .expect("DB flush failed.");
        Ok(hash)
    }
}

impl BroadcastProposalBlock for Client {
    fn broadcast_proposal_block(&self, block: SealedBlock) {
        const DURATION_ZERO: Duration = Duration::from_millis(0);
        self.notify(|notify| {
            notify.new_blocks(NewBlocks::new(
                vec![],
                vec![],
                ChainRoute::default(),
                vec![],
                vec![block.rlp_bytes()],
                DURATION_ZERO,
                false,
            ));
        });
    }
}

impl SealedBlockImporter for Client {}

impl ::miner::TransactionVerifierClient for Client {}
impl ::miner::BlockChainClient for Client {}

impl super::traits::EngineClient for Client {
    fn update_sealing(&self, force: ForceUpdateSealing) {
        self.importer.miner.update_sealing(self, force)
    }

    fn submit_seal(&self, block_hash: H256, seal: Vec<Bytes>) {
        let import = self
            .importer
            .miner
            .submit_seal(block_hash, seal)
            .and_then(|block| self.import_sealed_block(block));
        if let Err(err) = import {
            warn!(target: "poa", "Wrong internal seal submission! {:?}", err);
        }
    }

    fn broadcast_consensus_message(&self, message: Bytes) {
        self.notify(|notify| notify.broadcast(ChainMessageType::Consensus(message.clone())));
    }

    fn epoch_transition_for(&self, parent_hash: H256) -> Option<::engines::EpochTransition> {
        self.chain.read().epoch_transition_for(parent_hash)
    }

    fn as_full_client(&self) -> Option<&dyn BlockChainClient> {
        Some(self)
    }

    fn block_number(&self, id: BlockId) -> Option<BlockNumber> {
        <dyn BlockChainClient>::block_number(self, id)
    }

    fn block_header(&self, id: BlockId) -> Option<encoded::Header> {
        <dyn BlockChainClient>::block_header(self, id)
    }
}

impl ProvingBlockChainClient for Client {
    fn prove_storage(&self, key1: H256, key2: H256, id: BlockId) -> Option<(Vec<Bytes>, H256)> {
        self.state_at(id)
            .and_then(move |state| state.prove_storage(key1, key2).ok())
    }

    fn prove_account(
        &self,
        key1: H256,
        id: BlockId,
    ) -> Option<(Vec<Bytes>, ::types::basic_account::BasicAccount)> {
        self.state_at(id)
            .and_then(move |state| state.prove_account(key1).ok())
    }

    fn prove_transaction(
        &self,
        transaction: SignedTransaction,
        id: BlockId,
    ) -> Option<(Bytes, Vec<DBValue>)> {
        let (header, mut env_info) = match (self.block_header(id), self.env_info(id)) {
            (Some(s), Some(e)) => (s, e),
            _ => return None,
        };

        env_info.gas_limit = transaction.tx().gas.clone();
        let mut jdb = self.state_db.read().journal_db().boxed_clone();

        state::prove_transaction_virtual(
            jdb.as_hash_db_mut(),
            header.state_root().clone(),
            &transaction,
            self.engine.machine(),
            &env_info,
            self.factories.clone(),
        )
    }

    fn epoch_signal(&self, hash: H256) -> Option<Vec<u8>> {
        // pending transitions are never deleted, and do not contain
        // finality proofs by definition.
        self.chain
            .read()
            .get_pending_transition(hash)
            .map(|pending| pending.proof)
    }
}

impl SnapshotClient for Client {}

impl ImportExportBlocks for Client {
    fn export_blocks<'a>(
        &self,
        mut out: Box<dyn std::io::Write + 'a>,
        from: BlockId,
        to: BlockId,
        format: Option<DataFormat>,
    ) -> Result<(), String> {
        let from = self
            .block_number(from)
            .ok_or("Starting block could not be found")?;
        let to = self
            .block_number(to)
            .ok_or("End block could not be found")?;
        let format = format.unwrap_or_default();

        for i in from..=to {
            if i % 10000 == 0 {
                info!("#{}", i);
            }
            let b = self
                .block(BlockId::Number(i))
                .ok_or("Error exporting incomplete chain")?
                .into_inner();
            match format {
                DataFormat::Binary => {
                    out.write(&b)
                        .map_err(|e| format!("Couldn't write to stream. Cause: {}", e))?;
                }
                DataFormat::Hex => {
                    out.write_fmt(format_args!("{}\n", b.pretty()))
                        .map_err(|e| format!("Couldn't write to stream. Cause: {}", e))?;
                }
            }
        }
        Ok(())
    }

    fn import_blocks<'a>(
        &self,
        mut source: Box<dyn std::io::Read + 'a>,
        format: Option<DataFormat>,
    ) -> Result<(), String> {
        const READAHEAD_BYTES: usize = 8;

        let mut first_bytes: Vec<u8> = vec![0; READAHEAD_BYTES];
        let mut first_read = 0;

        let format = match format {
            Some(format) => format,
            None => {
                first_read = source
                    .read(&mut first_bytes)
                    .map_err(|_| "Error reading from the file/stream.")?;
                match first_bytes[0] {
                    0xf9 => DataFormat::Binary,
                    _ => DataFormat::Hex,
                }
            }
        };

        let do_import = |bytes: Vec<u8>| {
            let block = Unverified::from_rlp(bytes, self.engine.params().eip1559_transition)
                .map_err(|_| "Invalid block rlp")?;
            let number = block.header.number();
            while self.queue_info().is_full() {
                std::thread::sleep(Duration::from_secs(1));
            }
            match self.import_block(block) {
                Err(Error(EthcoreErrorKind::Import(ImportErrorKind::AlreadyInChain), _)) => {
                    trace!("Skipping block #{}: already in chain.", number);
                }
                Err(e) => {
                    return Err(format!("Cannot import block #{}: {:?}", number, e));
                }
                Ok(_) => {}
            }
            Ok(())
        };

        match format {
            DataFormat::Binary => loop {
                let (mut bytes, n) = if first_read > 0 {
                    (first_bytes.clone(), first_read)
                } else {
                    let mut bytes = vec![0; READAHEAD_BYTES];
                    let n = source
                        .read(&mut bytes)
                        .map_err(|err| format!("Error reading from the file/stream: {:?}", err))?;
                    (bytes, n)
                };
                if n == 0 {
                    break;
                }
                first_read = 0;
                let s = PayloadInfo::from(&bytes)
                    .map_err(|e| format!("Invalid RLP in the file/stream: {:?}", e))?
                    .total();
                bytes.resize(s, 0);
                source
                    .read_exact(&mut bytes[n..])
                    .map_err(|err| format!("Error reading from the file/stream: {:?}", err))?;
                do_import(bytes)?;
            },
            DataFormat::Hex => {
                for line in BufReader::new(source).lines() {
                    let s = line
                        .map_err(|err| format!("Error reading from the file/stream: {:?}", err))?;
                    let s = if first_read > 0 {
                        from_utf8(&first_bytes)
                            .map_err(|err| format!("Invalid UTF-8: {:?}", err))?
                            .to_owned()
                            + &(s[..])
                    } else {
                        s
                    };
                    first_read = 0;
                    let bytes = s
                        .from_hex()
                        .map_err(|err| format!("Invalid hex in file/stream: {:?}", err))?;
                    do_import(bytes)?;
                }
            }
        };
        self.flush_queue();
        Ok(())
    }
}

/// Returns `LocalizedReceipt` given `LocalizedTransaction`
/// and a vector of receipts from given block up to transaction index.
fn transaction_receipt(
    machine: &::machine::EthereumMachine,
    mut tx: LocalizedTransaction,
    receipt: TypedReceipt,
    prior_gas_used: U256,
    prior_no_of_logs: usize,
    base_fee: Option<U256>,
) -> LocalizedReceipt {
    let sender = tx.sender();
    let transaction_hash = tx.hash();
    let block_hash = tx.block_hash;
    let block_number = tx.block_number;
    let transaction_index = tx.transaction_index;
    let transaction_type = tx.tx_type();

    let receipt = receipt.receipt().clone();

    LocalizedReceipt {
        from: sender,
        to: match tx.tx().action {
            Action::Create => None,
            Action::Call(ref address) => Some(address.clone().into()),
        },
        transaction_hash: transaction_hash,
        transaction_index: transaction_index,
        transaction_type: transaction_type,
        block_hash: block_hash,
        block_number: block_number,
        cumulative_gas_used: receipt.gas_used,
        gas_used: receipt.gas_used - prior_gas_used,
        contract_address: match tx.tx().action {
            Action::Call(_) => None,
            Action::Create => Some(
                contract_address(
                    machine.create_address_scheme(block_number),
                    &sender,
                    &tx.tx().nonce,
                    &tx.tx().data,
                )
                .0,
            ),
        },
        logs: receipt
            .logs
            .into_iter()
            .enumerate()
            .map(|(i, log)| LocalizedLogEntry {
                entry: log,
                block_hash: block_hash,
                block_number: block_number,
                transaction_hash: transaction_hash,
                transaction_index: transaction_index,
                transaction_log_index: i,
                log_index: prior_no_of_logs + i,
            })
            .collect(),
        log_bloom: receipt.log_bloom,
        outcome: receipt.outcome.clone(),
        effective_gas_price: tx.effective_gas_price(base_fee),
    }
}

/// Queue some items to be processed by IO client.
struct IoChannelQueue {
    /// Using a *signed* integer for counting currently queued messages since the
    /// order in which the counter is incremented and decremented is not defined.
    /// Using an unsigned integer can (and will) result in integer underflow,
    /// incorrectly rejecting messages and returning a FullQueue error.
    currently_queued: Arc<AtomicI64>,
    limit: i64,
}

impl IoChannelQueue {
    pub fn new(limit: usize) -> Self {
        let limit = i64::try_from(limit).unwrap_or(i64::max_value());
        IoChannelQueue {
            currently_queued: Default::default(),
            limit,
        }
    }

    pub fn queue<F>(
        &self,
        channel: &IoChannel<ClientIoMessage>,
        count: usize,
        fun: F,
    ) -> EthcoreResult<()>
    where
        F: Fn(&Client) + Send + Sync + 'static,
    {
        let queue_size = self.currently_queued.load(AtomicOrdering::SeqCst);
        if queue_size >= self.limit {
            let err_limit = usize::try_from(self.limit).unwrap_or(usize::max_value());
            bail!("The queue is full ({})", err_limit);
        };

        let count = i64::try_from(count).unwrap_or(i64::max_value());

        let currently_queued = self.currently_queued.clone();
        let _ok = channel.send(ClientIoMessage::execute(move |client| {
            currently_queued.fetch_sub(count, AtomicOrdering::SeqCst);
            fun(client);
        }))?;

        self.currently_queued
            .fetch_add(count, AtomicOrdering::SeqCst);
        Ok(())
    }
}

impl PrometheusMetrics for Client {
    fn prometheus_metrics(&self, r: &mut PrometheusRegistry) {
        // gas, tx & blocks
        let report = self.report();

        for (key, value) in report.item_sizes.iter() {
            r.register_gauge(
                &key,
                format!("Total item number of {}", key).as_str(),
                *value as i64,
            );
        }

        r.register_counter(
            "import_gas",
            "Gas processed",
            report.gas_processed.as_u64() as i64,
        );
        r.register_counter(
            "import_blocks",
            "Blocks imported",
            report.blocks_imported as i64,
        );
        r.register_counter(
            "import_txs",
            "Transactions applied",
            report.transactions_applied as i64,
        );

        let state_db = self.state_db.read();
        r.register_gauge(
            "statedb_cache_size",
            "State DB cache size",
            state_db.cache_size() as i64,
        );

        // blockchain cache
        let blockchain_cache_info = self.blockchain_cache_info();
        r.register_gauge(
            "blockchaincache_block_details",
            "BlockDetails cache size",
            blockchain_cache_info.block_details as i64,
        );
        r.register_gauge(
            "blockchaincache_block_recipts",
            "Block receipts size",
            blockchain_cache_info.block_receipts as i64,
        );
        r.register_gauge(
            "blockchaincache_blocks",
            "Blocks cache size",
            blockchain_cache_info.blocks as i64,
        );
        r.register_gauge(
            "blockchaincache_txaddrs",
            "Transaction addresses cache size",
            blockchain_cache_info.transaction_addresses as i64,
        );
        r.register_gauge(
            "blockchaincache_size",
            "Total blockchain cache size",
            blockchain_cache_info.total() as i64,
        );

        // chain info
        let chain = self.chain_info();

        let gap = chain
            .ancient_block_number
            .map(|x| U256::from(x + 1))
            .and_then(|first| {
                chain
                    .first_block_number
                    .map(|last| (first, U256::from(last)))
            });
        if let Some((first, last)) = gap {
            r.register_gauge(
                "chain_warpsync_gap_first",
                "Warp sync gap, first block",
                first.as_u64() as i64,
            );
            r.register_gauge(
                "chain_warpsync_gap_last",
                "Warp sync gap, last block",
                last.as_u64() as i64,
            );
        }

        r.register_gauge(
            "chain_block",
            "Best block number",
            chain.best_block_number as i64,
        );

        // prunning info
        let prunning = self.pruning_info();
        r.register_gauge(
            "prunning_earliest_chain",
            "The first block which everything can be served after",
            prunning.earliest_chain as i64,
        );
        r.register_gauge(
            "prunning_earliest_state",
            "The first block where state requests may be served",
            prunning.earliest_state as i64,
        );

        // queue info
        let queue = self.queue_info();
        r.register_gauge(
            "queue_mem_used",
            "Queue heap memory used in bytes",
            queue.mem_used as i64,
        );
        r.register_gauge(
            "queue_size_total",
            "The total size of the queues",
            queue.total_queue_size() as i64,
        );
        r.register_gauge(
            "queue_size_unverified",
            "Number of queued items pending verification",
            queue.unverified_queue_size as i64,
        );
        r.register_gauge(
            "queue_size_verified",
            "Number of verified queued items pending import",
            queue.verified_queue_size as i64,
        );
        r.register_gauge(
            "queue_size_verifying",
            "Number of items being verified",
            queue.verifying_queue_size as i64,
        );

        // database info
        self.db.read().key_value().prometheus_metrics(r);
    }
}

#[cfg(test)]
mod tests {
    use blockchain::{BlockProvider, ExtrasInsert};
    use ethereum_types::{H160, H256};
    use spec::Spec;
    use test_helpers::generate_dummy_client_with_spec_and_data;

    #[test]
    fn should_not_cache_details_before_commit() {
        use client::{BlockChainClient, ChainInfo};
        use test_helpers::{generate_dummy_client, get_good_dummy_block_hash};

        use kvdb::DBTransaction;
        use std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc,
            },
            thread,
            time::Duration,
        };
        use types::encoded;

        let client = generate_dummy_client(0);
        let genesis = client.chain_info().best_block_hash;
        let (new_hash, new_block) = get_good_dummy_block_hash();

        let go = {
            // Separate thread uncommitted transaction
            let go = Arc::new(AtomicBool::new(false));
            let go_thread = go.clone();
            let another_client = client.clone();
            thread::spawn(move || {
                let mut batch = DBTransaction::new();
                another_client.chain.read().insert_block(
                    &mut batch,
                    encoded::Block::new(new_block),
                    Vec::new(),
                    ExtrasInsert {
                        fork_choice: ::engines::ForkChoice::New,
                        is_finalized: false,
                    },
                );
                go_thread.store(true, Ordering::SeqCst);
            });
            go
        };

        while !go.load(Ordering::SeqCst) {
            thread::park_timeout(Duration::from_millis(5));
        }

        assert!(client.tree_route(&genesis, &new_hash).is_none());
    }

    #[test]
    fn should_return_block_receipts() {
        use client::{BlockChainClient, BlockId, TransactionId};
        use test_helpers::generate_dummy_client_with_data;

        let client = generate_dummy_client_with_data(2, 2, &[1.into(), 1.into()]);
        let receipts = client.localized_block_receipts(BlockId::Latest).unwrap();

        assert_eq!(receipts.len(), 2);
        assert_eq!(receipts[0].transaction_index, 0);
        assert_eq!(receipts[0].block_number, 2);
        assert_eq!(receipts[0].cumulative_gas_used, 53_000.into());
        assert_eq!(receipts[0].gas_used, 53_000.into());

        assert_eq!(receipts[1].transaction_index, 1);
        assert_eq!(receipts[1].block_number, 2);
        assert_eq!(receipts[1].cumulative_gas_used, 106_000.into());
        assert_eq!(receipts[1].gas_used, 53_000.into());

        let receipt = client.transaction_receipt(TransactionId::Hash(receipts[0].transaction_hash));
        assert_eq!(receipt, Some(receipts[0].clone()));

        let receipt = client.transaction_receipt(TransactionId::Hash(receipts[1].transaction_hash));
        assert_eq!(receipt, Some(receipts[1].clone()));
    }

    #[test]
    fn should_return_correct_log_index() {
        use super::transaction_receipt;
        use crypto::publickey::KeyPair;
        use hash::keccak;
        use types::{
            log_entry::{LocalizedLogEntry, LogEntry},
            receipt::{LegacyReceipt, LocalizedReceipt, TransactionOutcome, TypedReceipt},
            transaction::{Action, LocalizedTransaction, Transaction, TypedTransaction},
        };

        // given
        let key = KeyPair::from_secret_slice(keccak("test").as_bytes()).unwrap();
        let secret = key.secret();
        let machine = ::ethereum::new_frontier_test_machine();

        let block_number = 1;
        let block_hash = H256::from_low_u64_be(5);
        let state_root = H256::from_low_u64_be(99);
        let gas_used = 10.into();
        let raw_tx = TypedTransaction::Legacy(Transaction {
            nonce: 0.into(),
            gas_price: 0.into(),
            gas: 21000.into(),
            action: Action::Call(H160::from_low_u64_be(10)),
            value: 0.into(),
            data: vec![],
        });
        let tx1 = raw_tx.clone().sign(secret, None);
        let transaction = LocalizedTransaction {
            signed: tx1.clone().into(),
            block_number: block_number,
            block_hash: block_hash,
            transaction_index: 1,
            cached_sender: Some(tx1.sender()),
        };
        let logs = vec![
            LogEntry {
                address: H160::from_low_u64_be(5),
                topics: vec![],
                data: vec![],
            },
            LogEntry {
                address: H160::from_low_u64_be(15),
                topics: vec![],
                data: vec![],
            },
        ];
        let receipt = TypedReceipt::Legacy(LegacyReceipt {
            outcome: TransactionOutcome::StateRoot(state_root),
            gas_used: gas_used,
            log_bloom: Default::default(),
            logs: logs.clone(),
        });

        // when
        let receipt = transaction_receipt(&machine, transaction, receipt, 5.into(), 1, None);

        // then
        assert_eq!(
            receipt,
            LocalizedReceipt {
                from: tx1.sender().into(),
                to: match tx1.tx().action {
                    Action::Create => None,
                    Action::Call(ref address) => Some(address.clone().into()),
                },
                transaction_hash: tx1.hash(),
                transaction_index: 1,
                transaction_type: tx1.tx_type(),
                block_hash: block_hash,
                block_number: block_number,
                cumulative_gas_used: gas_used,
                gas_used: gas_used - 5,
                contract_address: None,
                logs: vec![
                    LocalizedLogEntry {
                        entry: logs[0].clone(),
                        block_hash: block_hash,
                        block_number: block_number,
                        transaction_hash: tx1.hash(),
                        transaction_index: 1,
                        transaction_log_index: 0,
                        log_index: 1,
                    },
                    LocalizedLogEntry {
                        entry: logs[1].clone(),
                        block_hash: block_hash,
                        block_number: block_number,
                        transaction_hash: tx1.hash(),
                        transaction_index: 1,
                        transaction_log_index: 1,
                        log_index: 2,
                    }
                ],
                log_bloom: Default::default(),
                outcome: TransactionOutcome::StateRoot(state_root),
                effective_gas_price: Default::default(),
            }
        );
    }

    #[test]
    fn should_mark_finalization_correctly_for_parent() {
        let client = generate_dummy_client_with_spec_and_data(
            Spec::new_test_with_finality,
            2,
            0,
            &[],
            false,
        );
        let chain = client.chain();

        let block1_details = chain.block_hash(1).and_then(|h| chain.block_details(&h));
        assert!(block1_details.is_some());
        let block1_details = block1_details.unwrap();
        assert_eq!(block1_details.children.len(), 1);
        assert!(block1_details.is_finalized);

        let block2_details = chain.block_hash(2).and_then(|h| chain.block_details(&h));
        assert!(block2_details.is_some());
        let block2_details = block2_details.unwrap();
        assert_eq!(block2_details.children.len(), 0);
        assert!(!block2_details.is_finalized);
    }
}
