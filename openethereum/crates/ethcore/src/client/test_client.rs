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

//! Test client.

use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrder},
        Arc,
    },
};

use blockchain::{BlockReceipts, TreeRoute};
use bytes::Bytes;
use crypto::publickey::{Generator, Random};
use db::{COL_STATE, NUM_COLUMNS};
use ethcore_miner::pool::VerifiedTransaction;
use ethereum_types::{Address, H256, U256};
use ethtrie;
use hash::keccak;
use itertools::Itertools;
use kvdb::DBValue;
use parking_lot::RwLock;
use rlp::RlpStream;
use rustc_hex::FromHex;
use types::{
    basic_account::BasicAccount,
    encoded,
    filter::Filter,
    header::Header,
    log_entry::LocalizedLogEntry,
    pruning_info::PruningInfo,
    receipt::{LegacyReceipt, LocalizedReceipt, TransactionOutcome, TypedReceipt},
    transaction::{
        self, Action, LocalizedTransaction, SignedTransaction, Transaction, TypedTransaction,
        TypedTxId,
    },
    view,
    views::BlockView,
    BlockNumber,
};
use vm::Schedule;

use block::{ClosedBlock, OpenBlock, SealedBlock};
use call_contract::{CallContract, RegistryInfo};
use client::{
    traits::{ForceUpdateSealing, TransactionRequest},
    AccountData, BadBlocks, Balance, BlockChain, BlockChainClient, BlockChainInfo, BlockId,
    BlockInfo, BlockProducer, BlockStatus, BroadcastProposalBlock, Call, CallAnalytics, ChainInfo,
    EngineInfo, ImportBlock, ImportSealedBlock, IoClient, LastHashes, Mode, Nonce,
    PrepareOpenBlock, ProvingBlockChainClient, ReopenBlock, ScheduleInfo, SealedBlockImporter,
    StateClient, StateOrBlock, TraceFilter, TraceId, TransactionId, TransactionInfo, UncleId,
};
use engines::EthEngine;
use error::{Error, EthcoreResult};
use executed::CallError;
use executive::Executed;
use journaldb;
use miner::{self, Miner, MinerService};
use spec::Spec;
use state::StateInfo;
use state_db::StateDB;
use stats::{PrometheusMetrics, PrometheusRegistry};
use trace::LocalizedTrace;
use verification::queue::{kind::blocks::Unverified, QueueInfo};

/// Test client.
pub struct TestBlockChainClient {
    /// Blocks.
    pub blocks: RwLock<HashMap<H256, Bytes>>,
    /// Mapping of numbers to hashes.
    pub numbers: RwLock<HashMap<usize, H256>>,
    /// Genesis block hash.
    pub genesis_hash: H256,
    /// Last block hash.
    pub last_hash: RwLock<H256>,
    /// Extra data do set for each block
    pub extra_data: Bytes,
    /// Difficulty.
    pub difficulty: RwLock<U256>,
    /// Balances.
    pub balances: RwLock<HashMap<Address, U256>>,
    /// Nonces.
    pub nonces: RwLock<HashMap<Address, U256>>,
    /// Storage.
    pub storage: RwLock<HashMap<(Address, H256), H256>>,
    /// Code.
    pub code: RwLock<HashMap<Address, Bytes>>,
    /// Execution result.
    pub execution_result: RwLock<Option<Result<Executed, CallError>>>,
    /// Transaction receipts.
    pub receipts: RwLock<HashMap<TransactionId, LocalizedReceipt>>,
    /// Logs
    pub logs: RwLock<Vec<LocalizedLogEntry>>,
    /// Should return errors on logs.
    pub error_on_logs: RwLock<Option<BlockId>>,
    /// Block queue size.
    pub queue_size: AtomicUsize,
    /// Miner
    pub miner: Arc<Miner>,
    /// Spec
    pub spec: Spec,
    /// Timestamp assigned to latest sealed block
    pub latest_block_timestamp: RwLock<u64>,
    /// Ancient block info.
    pub ancient_block: RwLock<Option<(H256, u64)>>,
    /// First block info.
    pub first_block: RwLock<Option<(H256, u64)>>,
    /// Traces to return
    pub traces: RwLock<Option<Vec<LocalizedTrace>>>,
    /// Pruning history size to report.
    pub history: RwLock<Option<u64>>,
    /// Is disabled
    pub disabled: AtomicBool,
    /// Transaction hashes producer
    pub new_transaction_hashes: RwLock<Option<crossbeam_channel::Sender<H256>>>,
}

/// Used for generating test client blocks.
#[derive(Clone, Copy)]
pub enum EachBlockWith {
    /// Plain block.
    Nothing,
    /// Block with an uncle.
    Uncle,
    /// Block with a transaction.
    Transaction,
    /// Block with multiple transactions.
    Transactions(usize),
    /// Block with an uncle and transaction.
    UncleAndTransaction,
}

impl Default for TestBlockChainClient {
    fn default() -> Self {
        TestBlockChainClient::new()
    }
}

impl TestBlockChainClient {
    /// Creates new test client.
    pub fn new() -> Self {
        Self::new_with_extra_data(Bytes::new())
    }

    /// Creates new test client with specified extra data for each block
    pub fn new_with_extra_data(extra_data: Bytes) -> Self {
        let spec = Spec::new_test();
        TestBlockChainClient::new_with_spec_and_extra(spec, extra_data)
    }

    /// Create test client with custom spec.
    pub fn new_with_spec(spec: Spec) -> Self {
        TestBlockChainClient::new_with_spec_and_extra(spec, Bytes::new())
    }

    /// Create test client with custom spec and extra data.
    pub fn new_with_spec_and_extra(spec: Spec, extra_data: Bytes) -> Self {
        let genesis_block = spec.genesis_block();
        let genesis_hash = spec.genesis_header().hash();

        let mut client = TestBlockChainClient {
            blocks: RwLock::new(HashMap::new()),
            numbers: RwLock::new(HashMap::new()),
            genesis_hash: H256::default(),
            extra_data: extra_data,
            last_hash: RwLock::new(H256::default()),
            difficulty: RwLock::new(spec.genesis_header().difficulty().clone()),
            balances: RwLock::new(HashMap::new()),
            nonces: RwLock::new(HashMap::new()),
            storage: RwLock::new(HashMap::new()),
            code: RwLock::new(HashMap::new()),
            execution_result: RwLock::new(None),
            receipts: RwLock::new(HashMap::new()),
            logs: RwLock::new(Vec::new()),
            queue_size: AtomicUsize::new(0),
            miner: Arc::new(Miner::new_for_tests(&spec, None)),
            spec: spec,
            latest_block_timestamp: RwLock::new(10_000_000),
            ancient_block: RwLock::new(None),
            first_block: RwLock::new(None),
            traces: RwLock::new(None),
            history: RwLock::new(None),
            disabled: AtomicBool::new(false),
            error_on_logs: RwLock::new(None),
            new_transaction_hashes: RwLock::new(None),
        };

        // insert genesis hash.
        client.blocks.get_mut().insert(genesis_hash, genesis_block);
        client.numbers.get_mut().insert(0, genesis_hash);
        *client.last_hash.get_mut() = genesis_hash;
        client.genesis_hash = genesis_hash;
        client
    }

    /// Set the transaction receipt result
    pub fn set_transaction_receipt(&self, id: TransactionId, receipt: LocalizedReceipt) {
        self.receipts.write().insert(id, receipt);
    }

    /// Set the execution result.
    pub fn set_execution_result(&self, result: Result<Executed, CallError>) {
        *self.execution_result.write() = Some(result);
    }

    /// Set the balance of account `address` to `balance`.
    pub fn set_balance(&self, address: Address, balance: U256) {
        self.balances.write().insert(address, balance);
    }

    /// Set nonce of account `address` to `nonce`.
    pub fn set_nonce(&self, address: Address, nonce: U256) {
        self.nonces.write().insert(address, nonce);
    }

    /// Set `code` at `address`.
    pub fn set_code(&self, address: Address, code: Bytes) {
        self.code.write().insert(address, code);
    }

    /// Set storage `position` to `value` for account `address`.
    pub fn set_storage(&self, address: Address, position: H256, value: H256) {
        self.storage.write().insert((address, position), value);
    }

    /// Set block queue size for testing
    pub fn set_queue_size(&self, size: usize) {
        self.queue_size.store(size, AtomicOrder::SeqCst);
    }

    /// Set timestamp assigned to latest sealed block
    pub fn set_latest_block_timestamp(&self, ts: u64) {
        *self.latest_block_timestamp.write() = ts;
    }

    /// Set logs to return for each logs call.
    pub fn set_logs(&self, logs: Vec<LocalizedLogEntry>) {
        *self.logs.write() = logs;
    }

    /// Set return errors on logs.
    pub fn set_error_on_logs(&self, val: Option<BlockId>) {
        *self.error_on_logs.write() = val;
    }

    /// Add a block to test client.
    pub fn add_block<F>(&self, with: EachBlockWith, hook: F)
    where
        F: Fn(Header) -> Header,
    {
        let n = self.numbers.read().len();

        let mut header = Header::new();
        header.set_difficulty(From::from(n));
        header.set_parent_hash(self.last_hash.read().clone());
        header.set_number(n as BlockNumber);
        header.set_gas_limit(U256::from(1_000_000));
        header.set_extra_data(self.extra_data.clone());

        header = hook(header);

        let uncles = match with {
            EachBlockWith::Uncle | EachBlockWith::UncleAndTransaction => {
                let mut uncles = RlpStream::new_list(1);
                let mut uncle_header = Header::new();
                uncle_header.set_difficulty(From::from(n));
                uncle_header.set_parent_hash(self.last_hash.read().clone());
                uncle_header.set_number(n as BlockNumber);
                uncles.append(&uncle_header);
                header.set_uncles_hash(keccak(uncles.as_raw()));
                uncles
            }
            _ => RlpStream::new_list(0),
        };
        let txs = match with {
            EachBlockWith::Transaction
            | EachBlockWith::UncleAndTransaction
            | EachBlockWith::Transactions(_) => {
                let num_transactions = match with {
                    EachBlockWith::Transactions(num) => num,
                    _ => 1,
                };
                let mut txs = RlpStream::new_list(num_transactions);
                let keypair = Random.generate();
                let mut nonce = U256::zero();

                for _ in 0..num_transactions {
                    // Update nonces value
                    let tx = TypedTransaction::Legacy(Transaction {
                        action: Action::Create,
                        value: U256::from(100),
                        data: "3331600055".from_hex().unwrap(),
                        gas: U256::from(100_000),
                        gas_price: U256::from(200_000_000_000u64),
                        nonce: nonce,
                    });
                    let signed_tx = tx.sign(keypair.secret(), None);
                    signed_tx.rlp_append(&mut txs);
                    nonce += U256::one();
                }

                self.nonces.write().insert(keypair.address(), nonce);
                txs.out()
            }
            _ => ::rlp::EMPTY_LIST_RLP.to_vec(),
        };

        let mut rlp = RlpStream::new_list(3);
        rlp.append(&header);
        rlp.append_raw(&txs, 1);
        rlp.append_raw(uncles.as_raw(), 1);
        let unverified = Unverified::from_rlp(rlp.out(), BlockNumber::max_value()).unwrap();
        self.import_block(unverified).unwrap();
    }

    /// Add a sequence of blocks to test client.
    pub fn add_blocks(&self, count: usize, with: EachBlockWith) {
        for _ in 0..count {
            self.add_block(with, |header| header);
        }
    }

    /// Make a bad block by setting invalid parent hash.
    pub fn corrupt_block_parent(&self, n: BlockNumber) {
        let hash = self.block_hash(BlockId::Number(n)).unwrap();
        let mut header: Header = self
            .block_header(BlockId::Number(n))
            .unwrap()
            .decode(BlockNumber::max_value())
            .expect("decoding failed");
        header.set_parent_hash(H256::from_low_u64_be(42));
        let mut rlp = RlpStream::new_list(3);
        rlp.append(&header);
        rlp.append_raw(&::rlp::NULL_RLP, 1);
        rlp.append_raw(&::rlp::NULL_RLP, 1);
        self.blocks.write().insert(hash, rlp.out());
    }

    /// Get block hash with `delta` as offset from the most recent blocks.
    pub fn block_hash_delta_minus(&mut self, delta: usize) -> H256 {
        let blocks_read = self.numbers.read();
        let index = blocks_read.len() - delta;
        blocks_read[&index].clone()
    }

    fn block_hash(&self, id: BlockId) -> Option<H256> {
        match id {
            BlockId::Hash(hash) => Some(hash),
            BlockId::Number(n) => self.numbers.read().get(&(n as usize)).cloned(),
            BlockId::Earliest => self.numbers.read().get(&0).cloned(),
            BlockId::Latest => self
                .numbers
                .read()
                .get(&(self.numbers.read().len() - 1))
                .cloned(),
        }
    }

    /// Inserts a transaction with given gas price to miners transactions queue.
    pub fn insert_transaction_with_gas_price_to_queue(&self, gas_price: U256) -> H256 {
        let keypair = Random.generate();
        let tx = TypedTransaction::Legacy(Transaction {
            action: Action::Create,
            value: U256::from(100),
            data: "3331600055".from_hex().unwrap(),
            gas: U256::from(100_000),
            gas_price: gas_price,
            nonce: U256::zero(),
        });
        let signed_tx = tx.sign(keypair.secret(), None);
        self.set_balance(signed_tx.sender(), 10_000_000_000_000_000_000u64.into());
        let hash = signed_tx.hash();
        let res = self
            .miner
            .import_external_transactions(self, vec![signed_tx.into()]);
        let res = res.into_iter().next().unwrap();
        assert!(res.is_ok());

        // if new_transaction_hashes producer channel exists, send the transaction hash
        let _ = self
            .new_transaction_hashes
            .write()
            .as_ref()
            .and_then(|tx| Some(tx.send(hash)));

        hash
    }

    /// Inserts a transaction to miners transactions queue.
    pub fn insert_transaction_to_queue(&self) -> H256 {
        self.insert_transaction_with_gas_price_to_queue(U256::from(20_000_000_000u64))
    }

    /// Set reported history size.
    pub fn set_history(&self, h: Option<u64>) {
        *self.history.write() = h;
    }

    /// Returns true if the client has been disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled.load(AtomicOrder::SeqCst)
    }

    pub fn set_new_transaction_hashes_producer(
        &self,
        new_transaction_hashes: crossbeam_channel::Sender<H256>,
    ) {
        *self.new_transaction_hashes.write() = Some(new_transaction_hashes);
    }
}

/// Get temporary db state1
pub fn get_temp_state_db() -> StateDB {
    let db = ethcore_db::InMemoryWithMetrics::create(NUM_COLUMNS.unwrap_or(0));
    let journal_db = journaldb::new(Arc::new(db), journaldb::Algorithm::EarlyMerge, COL_STATE);
    StateDB::new(journal_db, 1024 * 1024)
}

impl ReopenBlock for TestBlockChainClient {
    fn reopen_block(&self, block: ClosedBlock) -> OpenBlock {
        block.reopen(&*self.spec.engine)
    }
}

impl PrepareOpenBlock for TestBlockChainClient {
    fn prepare_open_block(
        &self,
        author: Address,
        gas_range_target: (U256, U256),
        extra_data: Bytes,
    ) -> Result<OpenBlock, Error> {
        let engine = &*self.spec.engine;
        let genesis_header = self.spec.genesis_header();
        let db = self
            .spec
            .ensure_db_good(get_temp_state_db(), &Default::default())
            .unwrap();

        let last_hashes = vec![genesis_header.hash()];
        let mut open_block = OpenBlock::new(
            engine,
            Default::default(),
            false,
            db,
            &genesis_header,
            Arc::new(last_hashes),
            author,
            gas_range_target,
            extra_data,
            false,
            None,
        )?;
        // TODO [todr] Override timestamp for predictability
        open_block.set_timestamp(*self.latest_block_timestamp.read());
        Ok(open_block)
    }
}

impl ScheduleInfo for TestBlockChainClient {
    fn latest_schedule(&self) -> Schedule {
        Schedule::new_post_eip150(24576, true, true, true)
    }
}

impl ImportSealedBlock for TestBlockChainClient {
    fn import_sealed_block(&self, _block: SealedBlock) -> EthcoreResult<H256> {
        Ok(H256::default())
    }
}

impl BlockProducer for TestBlockChainClient {}

impl BroadcastProposalBlock for TestBlockChainClient {
    fn broadcast_proposal_block(&self, _block: SealedBlock) {}
}

impl SealedBlockImporter for TestBlockChainClient {}

impl ::miner::TransactionVerifierClient for TestBlockChainClient {}
impl ::miner::BlockChainClient for TestBlockChainClient {}

impl Nonce for TestBlockChainClient {
    fn nonce(&self, address: &Address, id: BlockId) -> Option<U256> {
        match id {
            BlockId::Latest => Some(
                self.nonces
                    .read()
                    .get(address)
                    .cloned()
                    .unwrap_or(self.spec.params().account_start_nonce),
            ),
            _ => None,
        }
    }

    fn latest_nonce(&self, address: &Address) -> U256 {
        self.nonce(address, BlockId::Latest).unwrap()
    }
}

impl Balance for TestBlockChainClient {
    fn balance(&self, address: &Address, state: StateOrBlock) -> Option<U256> {
        match state {
            StateOrBlock::Block(BlockId::Latest) | StateOrBlock::State(_) => Some(
                self.balances
                    .read()
                    .get(address)
                    .cloned()
                    .unwrap_or_else(U256::zero),
            ),
            _ => None,
        }
    }

    fn latest_balance(&self, address: &Address) -> U256 {
        self.balance(address, BlockId::Latest.into()).unwrap()
    }
}

impl AccountData for TestBlockChainClient {}

impl ChainInfo for TestBlockChainClient {
    fn chain_info(&self) -> BlockChainInfo {
        let number = self.blocks.read().len() as BlockNumber - 1;
        BlockChainInfo {
            total_difficulty: *self.difficulty.read(),
            pending_total_difficulty: *self.difficulty.read(),
            genesis_hash: self.genesis_hash.clone(),
            best_block_hash: self.last_hash.read().clone(),
            best_block_number: number,
            best_block_timestamp: number,
            first_block_hash: self.first_block.read().as_ref().map(|x| x.0),
            first_block_number: self.first_block.read().as_ref().map(|x| x.1),
            ancient_block_hash: self.ancient_block.read().as_ref().map(|x| x.0),
            ancient_block_number: self.ancient_block.read().as_ref().map(|x| x.1),
        }
    }
}

impl BlockInfo for TestBlockChainClient {
    fn block_header(&self, id: BlockId) -> Option<encoded::Header> {
        self.block_hash(id)
            .and_then(|hash| {
                self.blocks
                    .read()
                    .get(&hash)
                    .map(|r| view!(BlockView, r).header_rlp().as_raw().to_vec())
            })
            .map(encoded::Header::new)
    }

    fn best_block_header(&self) -> Header {
        self.block_header(BlockId::Hash(self.chain_info().best_block_hash))
            .expect("Best block always has header.")
            .decode(BlockNumber::max_value())
            .expect("decoding failed")
    }

    fn block(&self, id: BlockId) -> Option<encoded::Block> {
        self.block_hash(id)
            .and_then(|hash| self.blocks.read().get(&hash).cloned())
            .map(encoded::Block::new)
    }

    fn code_hash(&self, address: &Address, id: BlockId) -> Option<H256> {
        match id {
            BlockId::Latest => self.code.read().get(address).map(|c| keccak(&c)),
            _ => None,
        }
    }
}

impl CallContract for TestBlockChainClient {
    fn call_contract(
        &self,
        _id: BlockId,
        _address: Address,
        _data: Bytes,
    ) -> Result<Bytes, String> {
        Ok(vec![])
    }
}

impl TransactionInfo for TestBlockChainClient {
    fn transaction_block(&self, _id: TransactionId) -> Option<H256> {
        None // Simple default.
    }
}

impl BlockChain for TestBlockChainClient {}

impl RegistryInfo for TestBlockChainClient {
    fn registry_address(&self, _name: String, _block: BlockId) -> Option<Address> {
        None
    }
}

impl ImportBlock for TestBlockChainClient {
    fn import_block(&self, unverified: Unverified) -> EthcoreResult<H256> {
        let header = unverified.header;
        let h = header.hash();
        let number: usize = header.number() as usize;
        if number > self.blocks.read().len() {
            panic!(
                "Unexpected block number. Expected {}, got {}",
                self.blocks.read().len(),
                number
            );
        }
        if number > 0 {
            match self.blocks.read().get(header.parent_hash()) {
                Some(parent) => {
                    let parent = view!(BlockView, parent).header(BlockNumber::max_value());
                    if parent.number() != (header.number() - 1) {
                        panic!("Unexpected block parent");
                    }
                }
                None => {
                    panic!(
                        "Unknown block parent {:?} for block {}",
                        header.parent_hash(),
                        number
                    );
                }
            }
        }
        let len = self.numbers.read().len();
        if number == len {
            {
                let mut difficulty = self.difficulty.write();
                *difficulty = *difficulty + header.difficulty().clone();
            }
            *self.last_hash.write() = h.clone();
            self.blocks.write().insert(h.clone(), unverified.bytes);
            self.numbers.write().insert(number, h.clone());
            let mut parent_hash = header.parent_hash().clone();
            if number > 0 {
                let mut n = number - 1;
                while n > 0 && self.numbers.read()[&n] != parent_hash {
                    *self.numbers.write().get_mut(&n).unwrap() = parent_hash.clone();
                    n -= 1;
                    parent_hash = view!(BlockView, &self.blocks.read()[&parent_hash])
                        .header(BlockNumber::max_value())
                        .parent_hash()
                        .clone();
                }
            }
        } else {
            self.blocks.write().insert(h.clone(), unverified.bytes);
        }
        Ok(h)
    }
}

impl Call for TestBlockChainClient {
    // State will not be used by test client anyway, since all methods that accept state are mocked
    type State = TestState;

    fn call(
        &self,
        _t: &SignedTransaction,
        _analytics: CallAnalytics,
        _state: &mut Self::State,
        _header: &Header,
    ) -> Result<Executed, CallError> {
        self.execution_result.read().clone().unwrap()
    }

    fn call_many(
        &self,
        txs: &[(SignedTransaction, CallAnalytics)],
        state: &mut Self::State,
        header: &Header,
    ) -> Result<Vec<Executed>, CallError> {
        let mut res = Vec::with_capacity(txs.len());
        for &(ref tx, analytics) in txs {
            res.push(self.call(tx, analytics, state, header)?);
        }
        Ok(res)
    }

    fn estimate_gas(
        &self,
        _t: &SignedTransaction,
        _state: &Self::State,
        _header: &Header,
    ) -> Result<U256, CallError> {
        Ok(21000.into())
    }
}

/// NewType wrapper around `()` to impersonate `State` in trait impls. State will not be used by
/// test client, since all methods that accept state are mocked.
pub struct TestState;
impl StateInfo for TestState {
    fn nonce(&self, _address: &Address) -> ethtrie::Result<U256> {
        unimplemented!()
    }
    fn balance(&self, _address: &Address) -> ethtrie::Result<U256> {
        unimplemented!()
    }
    fn storage_at(&self, _address: &Address, _key: &H256) -> ethtrie::Result<H256> {
        unimplemented!()
    }
    fn code(&self, _address: &Address) -> ethtrie::Result<Option<Arc<Bytes>>> {
        unimplemented!()
    }
}

impl StateClient for TestBlockChainClient {
    // State will not be used by test client anyway, since all methods that accept state are mocked
    type State = TestState;

    fn latest_state_and_header(&self) -> (Self::State, Header) {
        (TestState, self.best_block_header())
    }

    fn state_at(&self, _id: BlockId) -> Option<Self::State> {
        Some(TestState)
    }
}

impl EngineInfo for TestBlockChainClient {
    fn engine(&self) -> &dyn EthEngine {
        &*self.spec.engine
    }
}

impl BadBlocks for TestBlockChainClient {
    fn bad_blocks(&self) -> Vec<(Unverified, String)> {
        vec![(
            Unverified {
                header: Default::default(),
                transactions: vec![],
                uncles: vec![],
                bytes: vec![1, 2, 3],
            },
            "Invalid block".into(),
        )]
    }
}

impl BlockChainClient for TestBlockChainClient {
    fn replay(&self, _id: TransactionId, _analytics: CallAnalytics) -> Result<Executed, CallError> {
        self.execution_result.read().clone().unwrap()
    }

    fn replay_block_transactions(
        &self,
        _block: BlockId,
        _analytics: CallAnalytics,
    ) -> Result<Box<dyn Iterator<Item = (H256, Executed)>>, CallError> {
        Ok(Box::new(
            self.traces
                .read()
                .clone()
                .unwrap()
                .into_iter()
                .map(|t| t.transaction_hash.unwrap_or(H256::default()))
                .zip(self.execution_result.read().clone().unwrap().into_iter()),
        ))
    }

    fn block_total_difficulty(&self, _id: BlockId) -> Option<U256> {
        Some(U256::zero())
    }

    fn block_hash(&self, id: BlockId) -> Option<H256> {
        Self::block_hash(self, id)
    }

    fn storage_root(&self, _address: &Address, _id: BlockId) -> Option<H256> {
        None
    }

    fn code(&self, address: &Address, state: StateOrBlock) -> Option<Option<Bytes>> {
        match state {
            StateOrBlock::Block(BlockId::Latest) => Some(self.code.read().get(address).cloned()),
            _ => None,
        }
    }

    fn storage_at(&self, address: &Address, position: &H256, state: StateOrBlock) -> Option<H256> {
        match state {
            StateOrBlock::Block(BlockId::Latest) => Some(
                self.storage
                    .read()
                    .get(&(address.clone(), position.clone()))
                    .cloned()
                    .unwrap_or_else(H256::default),
            ),
            _ => None,
        }
    }

    fn list_accounts(
        &self,
        _id: BlockId,
        _after: Option<&Address>,
        _count: u64,
    ) -> Option<Vec<Address>> {
        None
    }

    fn list_storage(
        &self,
        _id: BlockId,
        _account: &Address,
        _after: Option<&H256>,
        _count: u64,
    ) -> Option<Vec<H256>> {
        None
    }
    fn block_transaction(&self, _id: TransactionId) -> Option<LocalizedTransaction> {
        None // Simple default.
    }
    fn queued_transaction(&self, _hash: H256) -> Option<Arc<VerifiedTransaction>> {
        None
    }

    fn uncle(&self, _id: UncleId) -> Option<encoded::Header> {
        None // Simple default.
    }

    fn uncle_extra_info(&self, _id: UncleId) -> Option<BTreeMap<String, String>> {
        None
    }

    fn transaction_receipt(&self, id: TransactionId) -> Option<LocalizedReceipt> {
        self.receipts.read().get(&id).cloned()
    }

    fn localized_block_receipts(&self, _id: BlockId) -> Option<Vec<LocalizedReceipt>> {
        Some(self.receipts.read().values().cloned().collect())
    }

    fn logs(&self, filter: Filter) -> Result<Vec<LocalizedLogEntry>, BlockId> {
        match self.error_on_logs.read().as_ref() {
            Some(id) => return Err(id.clone()),
            None => (),
        }

        let mut logs = self.logs.read().clone();
        let len = logs.len();
        Ok(match filter.limit {
            Some(limit) if limit <= len => logs.split_off(len - limit),
            _ => logs,
        })
    }

    fn last_hashes(&self) -> LastHashes {
        unimplemented!();
    }

    fn is_canon(&self, _hash: &H256) -> bool {
        unimplemented!()
    }

    fn block_number(&self, id: BlockId) -> Option<BlockNumber> {
        match id {
            BlockId::Number(number) => Some(number),
            BlockId::Earliest => Some(0),
            BlockId::Latest => Some(self.chain_info().best_block_number),
            BlockId::Hash(ref h) => self
                .numbers
                .read()
                .iter()
                .find(|&(_, hash)| hash == h)
                .map(|e| *e.0 as u64),
        }
    }

    fn block_body(&self, id: BlockId) -> Option<encoded::Body> {
        self.block_hash(id).and_then(|hash| {
            self.blocks.read().get(&hash).map(|r| {
                let block = view!(BlockView, r);
                let mut stream = RlpStream::new_list(2);
                stream.append_raw(block.transactions_rlp().as_raw(), 1);
                stream.append_raw(block.uncles_rlp().as_raw(), 1);
                encoded::Body::new(stream.out())
            })
        })
    }

    fn block_extra_info(&self, id: BlockId) -> Option<BTreeMap<String, String>> {
        self.block(id)
            .map(|block| block.view().header(BlockNumber::max_value()))
            .map(|header| self.spec.engine.extra_info(&header))
    }

    fn block_status(&self, id: BlockId) -> BlockStatus {
        match id {
            BlockId::Number(number) if (number as usize) < self.blocks.read().len() => {
                BlockStatus::InChain
            }
            BlockId::Hash(ref hash) if self.blocks.read().get(hash).is_some() => {
                BlockStatus::InChain
            }
            BlockId::Latest | BlockId::Earliest => BlockStatus::InChain,
            _ => BlockStatus::Unknown,
        }
    }

    fn is_processing_fork(&self) -> bool {
        false
    }

    // works only if blocks are one after another 1 -> 2 -> 3
    fn tree_route(&self, from: &H256, to: &H256) -> Option<TreeRoute> {
        Some(TreeRoute {
            ancestor: H256::default(),
            index: 0,
            blocks: {
                let numbers_read = self.numbers.read();
                let mut adding = false;

                let mut blocks = Vec::new();
                for (_, hash) in numbers_read
                    .iter()
                    .sorted_by(|tuple1, tuple2| tuple1.0.cmp(tuple2.0))
                {
                    if hash == to {
                        if adding {
                            blocks.push(hash.clone());
                        }
                        adding = false;
                        break;
                    }
                    if hash == from {
                        adding = true;
                    }
                    if adding {
                        blocks.push(hash.clone());
                    }
                }
                if adding {
                    Vec::new()
                } else {
                    blocks
                }
            },
            is_from_route_finalized: false,
        })
    }

    fn find_uncles(&self, _hash: &H256) -> Option<Vec<H256>> {
        None
    }

    fn block_receipts(&self, hash: &H256) -> Option<BlockReceipts> {
        // starts with 'f' ?
        if *hash
            > H256::from_str("f000000000000000000000000000000000000000000000000000000000000000")
                .unwrap()
        {
            let receipt = BlockReceipts::new(vec![TypedReceipt::new(
                TypedTxId::Legacy,
                LegacyReceipt::new(
                    TransactionOutcome::StateRoot(H256::zero()),
                    U256::zero(),
                    vec![],
                ),
            )]);
            return Some(receipt);
        }
        None
    }

    fn queue_info(&self) -> QueueInfo {
        QueueInfo {
            verified_queue_size: self.queue_size.load(AtomicOrder::SeqCst),
            unverified_queue_size: 0,
            verifying_queue_size: 0,
            max_queue_size: 0,
            max_mem_use: 0,
            mem_used: 0,
        }
    }

    fn clear_queue(&self) {}

    fn additional_params(&self) -> BTreeMap<String, String> {
        Default::default()
    }

    fn filter_traces(&self, _filter: TraceFilter) -> Option<Vec<LocalizedTrace>> {
        self.traces.read().clone()
    }

    fn trace(&self, _trace: TraceId) -> Option<LocalizedTrace> {
        self.traces
            .read()
            .clone()
            .and_then(|vec| vec.into_iter().next())
    }

    fn transaction_traces(&self, _trace: TransactionId) -> Option<Vec<LocalizedTrace>> {
        self.traces.read().clone()
    }

    fn block_traces(&self, _trace: BlockId) -> Option<Vec<LocalizedTrace>> {
        self.traces.read().clone()
    }

    fn transactions_to_propagate(&self) -> Vec<Arc<VerifiedTransaction>> {
        self.miner
            .ready_transactions(self, 4096, miner::PendingOrdering::Priority)
    }

    fn signing_chain_id(&self) -> Option<u64> {
        None
    }

    fn mode(&self) -> Mode {
        Mode::Active
    }

    fn set_mode(&self, _: Mode) {
        unimplemented!();
    }

    fn spec_name(&self) -> String {
        "foundation".into()
    }

    fn set_spec_name(&self, _: String) -> Result<(), ()> {
        unimplemented!();
    }

    fn disable(&self) {
        self.disabled.store(true, AtomicOrder::SeqCst);
    }

    fn pruning_info(&self) -> PruningInfo {
        let best_num = self.chain_info().best_block_number;
        PruningInfo {
            earliest_chain: 1,
            earliest_state: self
                .history
                .read()
                .as_ref()
                .map(|x| best_num - x)
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
        let transaction = TypedTransaction::Legacy(Transaction {
            nonce: nonce
                .unwrap_or_else(|| self.latest_nonce(&self.miner.authoring_params().author)),
            action,
            gas: gas.unwrap_or(self.spec.gas_limit),
            gas_price: gas_price.unwrap_or_else(U256::zero),
            value: U256::default(),
            data: data,
        });
        let chain_id = Some(self.spec.chain_id());
        let sig = self
            .spec
            .engine
            .sign(transaction.signature_hash(chain_id))
            .unwrap();
        Ok(SignedTransaction::new(transaction.with_signature(sig, chain_id)).unwrap())
    }

    fn transact(&self, tx_request: TransactionRequest) -> Result<(), transaction::Error> {
        let signed = self.create_transaction(tx_request)?;
        self.miner.import_own_transaction(self, signed.into())
    }

    fn registrar_address(&self) -> Option<Address> {
        None
    }

    fn state_data(&self, hash: &H256) -> Option<Bytes> {
        let begins_with_f =
            H256::from_str("f000000000000000000000000000000000000000000000000000000000000000")
                .unwrap();
        if *hash > begins_with_f {
            let mut rlp = RlpStream::new();
            rlp.append(&hash.clone());
            return Some(rlp.out());
        } else if *hash
            == H256::from_str("000000000000000000000000000000000000000000000000000000000000000a")
                .unwrap()
        {
            // for basic `return_node_data` tests
            return Some(vec![0xaa, 0xaa]);
        } else if *hash
            == H256::from_str("000000000000000000000000000000000000000000000000000000000000000c")
                .unwrap()
        {
            return Some(vec![0xcc]);
        }
        None
    }

    fn transaction(&self, tx_hash: &H256) -> Option<Arc<VerifiedTransaction>> {
        self.miner.transaction(tx_hash)
    }
}

impl IoClient for TestBlockChainClient {
    fn queue_transactions(&self, transactions: Vec<Bytes>, _peer_id: usize) {
        // import right here
        let txs = transactions
            .into_iter()
            .filter_map(|bytes| TypedTransaction::decode(&bytes).ok())
            .collect();
        self.miner.import_external_transactions(self, txs);
    }

    fn ancient_block_queue_fullness(&self) -> f32 {
        0.0
    }

    fn queue_ancient_block(&self, unverified: Unverified, _r: Bytes) -> EthcoreResult<H256> {
        self.import_block(unverified)
    }

    fn queue_consensus_message(&self, message: Bytes) {
        self.spec.engine.handle_message(&message).unwrap();
    }
}

impl ProvingBlockChainClient for TestBlockChainClient {
    fn prove_storage(&self, _: H256, _: H256, _: BlockId) -> Option<(Vec<Bytes>, H256)> {
        None
    }

    fn prove_account(&self, _: H256, _: BlockId) -> Option<(Vec<Bytes>, BasicAccount)> {
        None
    }

    fn prove_transaction(&self, _: SignedTransaction, _: BlockId) -> Option<(Bytes, Vec<DBValue>)> {
        None
    }

    fn epoch_signal(&self, _: H256) -> Option<Vec<u8>> {
        None
    }
}

impl super::traits::EngineClient for TestBlockChainClient {
    fn update_sealing(&self, force: ForceUpdateSealing) {
        self.miner.update_sealing(self, force)
    }

    fn submit_seal(&self, block_hash: H256, seal: Vec<Bytes>) {
        let import = self
            .miner
            .submit_seal(block_hash, seal)
            .and_then(|block| self.import_sealed_block(block));
        if let Err(err) = import {
            warn!(target: "poa", "Wrong internal seal submission! {:?}", err);
        }
    }

    fn broadcast_consensus_message(&self, _message: Bytes) {}

    fn epoch_transition_for(&self, _block_hash: H256) -> Option<::engines::EpochTransition> {
        None
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

impl PrometheusMetrics for TestBlockChainClient {
    fn prometheus_metrics(&self, _r: &mut PrometheusRegistry) {}
}
