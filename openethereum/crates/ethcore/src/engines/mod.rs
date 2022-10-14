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

//! Consensus engine specification and basic implementations.

mod authority_round;
mod basic_authority;
mod clique;
mod instant_seal;
mod null_engine;
mod validator_set;

pub mod block_reward;
pub mod signer;

pub use self::{
    authority_round::AuthorityRound,
    basic_authority::BasicAuthority,
    clique::Clique,
    instant_seal::{InstantSeal, InstantSealParams},
    null_engine::NullEngine,
    signer::EngineSigner,
};

// TODO [ToDr] Remove re-export (#10130)
pub use types::engines::{
    epoch::{self, Transition as EpochTransition},
    ForkChoice,
};

use std::{
    collections::{BTreeMap, HashMap},
    error, fmt,
    sync::{Arc, Weak},
};

use builtin::Builtin;
use error::Error;
use snapshot::SnapshotComponents;
use spec::CommonParams;
use types::{
    header::{ExtendedHeader, Header},
    transaction::{self, SignedTransaction, UnverifiedTransaction},
    BlockNumber,
};
use vm::{ActionValue, CallType, CreateContractAddress, EnvInfo, Schedule};

use block::ExecutedBlock;
use bytes::Bytes;
use crypto::publickey::Signature;
use ethereum_types::{Address, H256, H64, U256};
use machine::{self, AuxiliaryData, AuxiliaryRequest, Machine};
use types::ancestry_action::AncestryAction;
use unexpected::{Mismatch, OutOfBounds};

/// Default EIP-210 contract code.
/// As defined in https://github.com/ethereum/EIPs/pull/210
pub const DEFAULT_BLOCKHASH_CONTRACT: &'static str = "73fffffffffffffffffffffffffffffffffffffffe33141561006a5760014303600035610100820755610100810715156100455760003561010061010083050761010001555b6201000081071515610064576000356101006201000083050761020001555b5061013e565b4360003512151561008457600060405260206040f361013d565b61010060003543031315156100a857610100600035075460605260206060f361013c565b6101006000350715156100c55762010000600035430313156100c8565b60005b156100ea576101006101006000350507610100015460805260206080f361013b565b620100006000350715156101095763010000006000354303131561010c565b60005b1561012f57610100620100006000350507610200015460a052602060a0f361013a565b600060c052602060c0f35b5b5b5b5b";
/// The number of generations back that uncles can be.
pub const MAX_UNCLE_AGE: usize = 6;

/// Voting errors.
#[derive(Debug)]
pub enum EngineError {
    /// Signature or author field does not belong to an authority.
    NotAuthorized(Address),
    /// The same author issued different votes at the same step.
    DoubleVote(Address),
    /// The received block is from an incorrect proposer.
    NotProposer(Mismatch<Address>),
    /// Message was not expected.
    UnexpectedMessage,
    /// Seal field has an unexpected size.
    BadSealFieldSize(OutOfBounds<usize>),
    /// Validation proof insufficient.
    InsufficientProof(String),
    /// Failed system call.
    FailedSystemCall(String),
    /// Failed to decode the result of a system call.
    SystemCallResultDecoding(String),
    /// The result of a system call is invalid.
    SystemCallResultInvalid(String),
    /// Malformed consensus message.
    MalformedMessage(String),
    /// Requires client ref, but none registered.
    RequiresClient,
    /// Invalid engine specification or implementation.
    InvalidEngine,
    /// Requires signer ref, but none registered.
    RequiresSigner,
    /// Checkpoint is missing
    CliqueMissingCheckpoint(H256),
    /// Missing vanity data
    CliqueMissingVanity,
    /// Missing signature
    CliqueMissingSignature,
    /// Missing signers
    CliqueCheckpointNoSigner,
    /// List of signers is invalid
    CliqueCheckpointInvalidSigners(usize),
    /// Wrong author on a checkpoint
    CliqueWrongAuthorCheckpoint(Mismatch<Address>),
    /// Wrong checkpoint authors recovered
    CliqueFaultyRecoveredSigners(Vec<String>),
    /// Invalid nonce (should contain vote)
    CliqueInvalidNonce(H64),
    /// The signer signed a block to recently
    CliqueTooRecentlySigned(Address),
    /// Custom
    Custom(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::EngineError::*;
        let msg = match *self {
            CliqueMissingCheckpoint(ref hash) => format!("Missing checkpoint block: {}", hash),
            CliqueMissingVanity => format!("Extra data is missing vanity data"),
            CliqueMissingSignature => format!("Extra data is missing signature"),
            CliqueCheckpointInvalidSigners(len) => format!(
                "Checkpoint block list was of length: {} of checkpoint but
															it needs to be bigger than zero and a divisible by 20",
                len
            ),
            CliqueCheckpointNoSigner => format!("Checkpoint block list of signers was empty"),
            CliqueInvalidNonce(ref mis) => format!(
                "Unexpected nonce {} expected {} or {}",
                mis,
                0_u64,
                u64::max_value()
            ),
            CliqueWrongAuthorCheckpoint(ref oob) => {
                format!("Unexpected checkpoint author: {}", oob)
            }
            CliqueFaultyRecoveredSigners(ref mis) => format!("Faulty recovered signers {:?}", mis),
            CliqueTooRecentlySigned(ref address) => {
                format!("The signer: {} has signed a block too recently", address)
            }
            Custom(ref s) => s.clone(),
            DoubleVote(ref address) => format!("Author {} issued too many blocks.", address),
            NotProposer(ref mis) => format!("Author is not a current proposer: {}", mis),
            NotAuthorized(ref address) => format!("Signer {} is not authorized.", address),
            UnexpectedMessage => "This Engine should not be fed messages.".into(),
            BadSealFieldSize(ref oob) => format!("Seal field has an unexpected length: {}", oob),
            InsufficientProof(ref msg) => format!("Insufficient validation proof: {}", msg),
            FailedSystemCall(ref msg) => format!("Failed to make system call: {}", msg),
            SystemCallResultDecoding(ref msg) => {
                format!("Failed to decode the result of a system call: {}", msg)
            }
            SystemCallResultInvalid(ref msg) => {
                format!("The result of a system call is invalid: {}", msg)
            }
            MalformedMessage(ref msg) => format!("Received malformed consensus message: {}", msg),
            RequiresClient => format!("Call requires client but none registered"),
            RequiresSigner => format!("Call requires signer but none registered"),
            InvalidEngine => format!("Invalid engine specification or implementation"),
        };

        f.write_fmt(format_args!("Engine error ({})", msg))
    }
}

impl error::Error for EngineError {
    fn description(&self) -> &str {
        "Engine error"
    }
}

/// Seal type.
#[derive(Debug, PartialEq, Eq)]
pub enum Seal {
    /// Proposal seal; should be broadcasted, but not inserted into blockchain.
    Proposal(Vec<Bytes>),
    /// Regular block seal; should be part of the blockchain.
    Regular(Vec<Bytes>),
    /// Engine does not generate seal for this block right now.
    None,
}

/// The type of sealing the engine is currently able to perform.
#[derive(Debug, PartialEq, Eq)]
pub enum SealingState {
    /// The engine is ready to seal a block.
    Ready,
    /// The engine can't seal at the moment, and no block should be prepared and queued.
    NotReady,
    /// The engine does not seal internally.
    External,
}

/// A system-calling closure. Enacts calls on a block's state from the system address.
pub type SystemCall<'a> = dyn FnMut(Address, Vec<u8>) -> Result<Vec<u8>, String> + 'a;

/// A system-calling closure. Enacts calls on a block's state with code either from an on-chain contract, or hard-coded EVM or WASM (if enabled on-chain) codes.
pub type SystemOrCodeCall<'a> =
    dyn FnMut(SystemOrCodeCallKind, Vec<u8>) -> Result<Vec<u8>, String> + 'a;

/// Kind of SystemOrCodeCall, this is either an on-chain address, or code.
#[derive(PartialEq, Debug, Clone)]
pub enum SystemOrCodeCallKind {
    /// On-chain address.
    Address(Address),
    /// Hard-coded code.
    Code(Arc<Vec<u8>>, H256),
}

/// Default SystemOrCodeCall implementation.
pub fn default_system_or_code_call<'a>(
    machine: &'a ::machine::EthereumMachine,
    block: &'a mut ::block::ExecutedBlock,
) -> impl FnMut(SystemOrCodeCallKind, Vec<u8>) -> Result<Vec<u8>, String> + 'a {
    move |to, data| {
        let result = match to {
            SystemOrCodeCallKind::Address(address) => {
                machine.execute_as_system(block, address, U256::max_value(), Some(data))
            }
            SystemOrCodeCallKind::Code(code, code_hash) => machine.execute_code_as_system(
                block,
                None,
                Some(code),
                Some(code_hash),
                Some(ActionValue::Apparent(U256::zero())),
                U256::max_value(),
                Some(data),
                Some(CallType::StaticCall),
            ),
        };

        result.map_err(|e| format!("{}", e))
    }
}

/// Type alias for a function we can get headers by hash through.
pub type Headers<'a, H> = dyn Fn(H256) -> Option<H> + 'a;

/// Type alias for a function we can query pending transitions by block hash through.
pub type PendingTransitionStore<'a> = dyn Fn(H256) -> Option<epoch::PendingTransition> + 'a;

/// Proof dependent on state.
pub trait StateDependentProof<M: Machine>: Send + Sync {
    /// Generate a proof, given the state.
    fn generate_proof<'a>(&self, state: &machine::Call) -> Result<Vec<u8>, String>;
    /// Check a proof generated elsewhere (potentially by a peer).
    // `engine` needed to check state proofs, while really this should
    // just be state machine params.
    fn check_proof(&self, machine: &M, proof: &[u8]) -> Result<(), String>;
}

/// Proof generated on epoch change.
pub enum Proof<M: Machine> {
    /// Known proof (extracted from signal)
    Known(Vec<u8>),
    /// State dependent proof.
    WithState(Arc<dyn StateDependentProof<M>>),
}

/// Generated epoch verifier.
pub enum ConstructedVerifier<'a, M: Machine> {
    /// Fully trusted verifier.
    Trusted(Box<dyn EpochVerifier<M>>),
    /// Verifier unconfirmed. Check whether given finality proof finalizes given hash
    /// under previous epoch.
    Unconfirmed(Box<dyn EpochVerifier<M>>, &'a [u8], H256),
    /// Error constructing verifier.
    Err(Error),
}

impl<'a, M: Machine> ConstructedVerifier<'a, M> {
    /// Convert to a result, indicating that any necessary confirmation has been done
    /// already.
    pub fn known_confirmed(self) -> Result<Box<dyn EpochVerifier<M>>, Error> {
        match self {
            ConstructedVerifier::Trusted(v) | ConstructedVerifier::Unconfirmed(v, _, _) => Ok(v),
            ConstructedVerifier::Err(e) => Err(e),
        }
    }
}

/// Results of a query of whether an epoch change occurred at the given block.
pub enum EpochChange<M: Machine> {
    /// Cannot determine until more data is passed.
    Unsure(AuxiliaryRequest),
    /// No epoch change.
    No,
    /// The epoch will change, with proof.
    Yes(Proof<M>),
}

/// A consensus mechanism for the chain. Generally either proof-of-work or proof-of-stake-based.
/// Provides hooks into each of the major parts of block import.
pub trait Engine<M: Machine>: Sync + Send {
    /// The name of this engine.
    fn name(&self) -> &str;

    /// Get access to the underlying state machine.
    // TODO: decouple.
    fn machine(&self) -> &M;

    /// The number of additional header fields required for this engine.
    fn seal_fields(&self, _header: &Header) -> usize {
        0
    }

    /// Additional engine-specific information for the user/developer concerning `header`.
    fn extra_info(&self, _header: &Header) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    /// Maximum number of uncles a block is allowed to declare.
    fn maximum_uncle_count(&self, _block: BlockNumber) -> usize {
        0
    }

    /// Optional maximum gas limit.
    fn maximum_gas_limit(&self) -> Option<U256> {
        None
    }

    /// Block transformation functions, before the transactions.
    /// `epoch_begin` set to true if this block kicks off an epoch.
    fn on_new_block(
        &self,
        _block: &mut ExecutedBlock,
        _epoch_begin: bool,
        _ancestry: &mut dyn Iterator<Item = ExtendedHeader>,
    ) -> Result<(), M::Error> {
        Ok(())
    }

    /// Block transformation functions, after the transactions.
    fn on_close_block(&self, _block: &mut ExecutedBlock) -> Result<(), M::Error> {
        Ok(())
    }

    /// Allow mutating the header during seal generation. Currently only used by Clique.
    fn on_seal_block(&self, _block: &mut ExecutedBlock) -> Result<(), Error> {
        Ok(())
    }

    /// Returns the engine's current sealing state.
    fn sealing_state(&self) -> SealingState {
        SealingState::External
    }

    /// Called in `miner.chain_new_blocks` if the engine wishes to `update_sealing`
    /// after a block was recently sealed.
    ///
    /// returns false by default
    fn should_reseal_on_update(&self) -> bool {
        false
    }

    /// Attempt to seal the block internally.
    ///
    /// If `Some` is returned, then you get a valid seal.
    ///
    /// This operation is synchronous and may (quite reasonably) not be available, in which None will
    /// be returned.
    ///
    /// It is fine to require access to state or a full client for this function, since
    /// light clients do not generate seals.
    fn generate_seal(&self, _block: &ExecutedBlock, _parent: &Header) -> Seal {
        Seal::None
    }

    /// Verify a locally-generated seal of a header.
    ///
    /// If this engine seals internally,
    /// no checks have to be done here, since all internally generated seals
    /// should be valid.
    ///
    /// Externally-generated seals (e.g. PoW) will need to be checked for validity.
    ///
    /// It is fine to require access to state or a full client for this function, since
    /// light clients do not generate seals.
    fn verify_local_seal(&self, header: &Header) -> Result<(), M::Error>;

    /// Phase 1 quick block verification. Only does checks that are cheap. Returns either a null `Ok` or a general error detailing the problem with import.
    /// The verification module can optionally avoid checking the seal (`check_seal`), if seal verification is disabled this method won't be called.
    fn verify_block_basic(&self, _header: &Header) -> Result<(), M::Error> {
        Ok(())
    }

    /// Phase 2 verification. Perform costly checks such as transaction signatures. Returns either a null `Ok` or a general error detailing the problem with import.
    /// The verification module can optionally avoid checking the seal (`check_seal`), if seal verification is disabled this method won't be called.
    fn verify_block_unordered(&self, _header: &Header) -> Result<(), M::Error> {
        Ok(())
    }

    /// Phase 3 verification. Check block information against parent. Returns either a null `Ok` or a general error detailing the problem with import.
    fn verify_block_family(&self, _header: &Header, _parent: &Header) -> Result<(), M::Error> {
        Ok(())
    }

    /// Phase 4 verification. Verify block header against potentially external data.
    /// Should only be called when `register_client` has been called previously.
    fn verify_block_external(&self, _header: &Header) -> Result<(), M::Error> {
        Ok(())
    }

    /// Genesis epoch data.
    fn genesis_epoch_data<'a>(
        &self,
        _header: &Header,
        _state: &machine::Call,
    ) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }

    /// Whether an epoch change is signalled at the given header but will require finality.
    /// If a change can be enacted immediately then return `No` from this function but
    /// `Yes` from `is_epoch_end`.
    ///
    /// If auxiliary data of the block is required, return an auxiliary request and the function will be
    /// called again with them.
    /// Return `Yes` or `No` when the answer is definitively known.
    ///
    /// Should not interact with state.
    fn signals_epoch_end<'a>(&self, _header: &Header, _aux: AuxiliaryData<'a>) -> EpochChange<M> {
        EpochChange::No
    }

    /// Whether a block is the end of an epoch.
    ///
    /// This either means that an immediate transition occurs or a block signalling transition
    /// has reached finality. The `Headers` given are not guaranteed to return any blocks
    /// from any epoch other than the current. The client must keep track of finality and provide
    /// the latest finalized headers to check against the transition store.
    ///
    /// Return optional transition proof.
    fn is_epoch_end(
        &self,
        _chain_head: &Header,
        _finalized: &[H256],
        _chain: &Headers<Header>,
        _transition_store: &PendingTransitionStore,
    ) -> Option<Vec<u8>> {
        None
    }

    /// Create an epoch verifier from validation proof and a flag indicating
    /// whether finality is required.
    fn epoch_verifier<'a>(&self, _header: &Header, _proof: &'a [u8]) -> ConstructedVerifier<'a, M> {
        ConstructedVerifier::Trusted(Box::new(NoOp))
    }

    /// Populate a header's fields based on its parent's header.
    /// Usually implements the chain scoring rule based on weight.
    fn populate_from_parent(&self, _header: &mut Header, _parent: &Header) {}

    /// Handle any potential consensus messages;
    /// updating consensus state and potentially issuing a new one.
    fn handle_message(&self, _message: &[u8]) -> Result<(), EngineError> {
        Err(EngineError::UnexpectedMessage)
    }

    /// Register a component which signs consensus messages.
    fn set_signer(&self, _signer: Option<Box<dyn EngineSigner>>) {}

    /// Returns whether the current node is a validator and
    /// actually may seal a block if AuRa engine is used.
    ///
    /// Used by `eth_mining` rpc call.
    fn is_allowed_to_seal(&self) -> bool {
        true
    }

    /// Sign using the EngineSigner, to be used for consensus tx signing.
    fn sign(&self, _hash: H256) -> Result<Signature, M::Error> {
        unimplemented!()
    }

    /// Add Client which can be used for sealing, potentially querying the state and sending messages.
    fn register_client(&self, _client: Weak<M::EngineClient>) {}

    /// Trigger next step of the consensus engine.
    fn step(&self) {}

    /// Create a factory for building snapshot chunks and restoring from them.
    /// Returning `None` indicates that this engine doesn't support snapshot creation.
    fn snapshot_components(&self) -> Option<Box<dyn SnapshotComponents>> {
        None
    }

    /// Whether this engine supports warp sync.
    fn supports_warp(&self) -> bool {
        self.snapshot_components().is_some()
    }

    /// Return a new open block header timestamp based on the parent timestamp.
    fn open_block_header_timestamp(&self, parent_timestamp: u64) -> u64 {
        use std::{cmp, time};

        let now = time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap_or_default();
        cmp::max(now.as_secs() as u64, parent_timestamp + 1)
    }

    /// Check whether the parent timestamp is valid.
    fn is_timestamp_valid(&self, header_timestamp: u64, parent_timestamp: u64) -> bool {
        header_timestamp > parent_timestamp
    }

    // t_nb 9.1 Gather all ancestry actions. Called at the last stage when a block is committed. The Engine must guarantee that
    /// the ancestry exists.
    fn ancestry_actions(
        &self,
        _header: &Header,
        _ancestry: &mut dyn Iterator<Item = ExtendedHeader>,
    ) -> Vec<AncestryAction> {
        Vec::new()
    }

    /// Check whether the given new block is the best block, after finalization check.
    fn fork_choice(&self, new: &ExtendedHeader, best: &ExtendedHeader) -> ForkChoice;

    /// Returns author should used when executing tx's for this block.
    fn executive_author(&self, header: &Header) -> Result<Address, Error> {
        Ok(*header.author())
    }

    /// Returns a list of transactions for a new block if we are the author.
    ///
    /// This is called when the miner prepares a new block that this node will author and seal. It returns a list of
    /// transactions that will be added to the block before any other transactions from the queue.
    /// Added for AuRa needs.
    fn generate_engine_transactions(
        &self,
        _block: &ExecutedBlock,
    ) -> Result<Vec<SignedTransaction>, Error> {
        Ok(Vec::new())
    }

    /// Overrides the block gas limit. Whenever this returns `Some` for a header, the next block's gas limit must be
    /// exactly that value. used by AuRa engine.
    fn gas_limit_override(&self, _header: &Header) -> Option<U256> {
        None
    }
}

/// t_nb 9.3 Check whether a given block is the best block based on the default total difficulty rule.
pub fn total_difficulty_fork_choice(new: &ExtendedHeader, best: &ExtendedHeader) -> ForkChoice {
    if new.total_score() > best.total_score() {
        ForkChoice::New
    } else {
        ForkChoice::Old
    }
}

/// Common type alias for an engine coupled with an Ethereum-like state machine.
// TODO: make this a _trait_ alias when those exist.
// fortunately the effect is largely the same since engines are mostly used
// via trait objects.
pub trait EthEngine: Engine<::machine::EthereumMachine> {
    /// Get the general parameters of the chain.
    fn params(&self) -> &CommonParams {
        self.machine().params()
    }

    /// Get the EVM schedule for the given block number.
    fn schedule(&self, block_number: BlockNumber) -> Schedule {
        self.machine().schedule(block_number)
    }

    /// Builtin-contracts for the chain..
    fn builtins(&self) -> &BTreeMap<Address, Builtin> {
        self.machine().builtins()
    }

    /// Attempt to get a handle to a built-in contract.
    /// Only returns references to activated built-ins.
    fn builtin(&self, a: &Address, block_number: BlockNumber) -> Option<&Builtin> {
        self.machine().builtin(a, block_number)
    }

    /// Some intrinsic operation parameters; by default they take their value from the `spec()`'s `engine_params`.
    fn maximum_extra_data_size(&self) -> usize {
        self.machine().maximum_extra_data_size()
    }

    /// The nonce with which accounts begin at given block.
    fn account_start_nonce(&self, block: BlockNumber) -> U256 {
        self.machine().account_start_nonce(block)
    }

    /// The network ID that transactions should be signed with.
    fn signing_chain_id(&self, env_info: &EnvInfo) -> Option<u64> {
        self.machine().signing_chain_id(env_info)
    }

    /// Returns new contract address generation scheme at given block number.
    fn create_address_scheme(&self, number: BlockNumber) -> CreateContractAddress {
        self.machine().create_address_scheme(number)
    }

    // t_nb 5.3.1 Verify a particular transaction is valid.
    ///
    /// Unordered verification doesn't rely on the transaction execution order,
    /// i.e. it should only verify stuff that doesn't assume any previous transactions
    /// has already been verified and executed.
    ///
    /// NOTE This function consumes an `UnverifiedTransaction` and produces `SignedTransaction`
    /// which implies that a heavy check of the signature is performed here.
    fn verify_transaction_unordered(
        &self,
        t: UnverifiedTransaction,
        header: &Header,
    ) -> Result<SignedTransaction, transaction::Error> {
        self.machine().verify_transaction_unordered(t, header)
    }

    /// Perform basic/cheap transaction verification.
    ///
    /// This should include all cheap checks that can be done before
    /// actually checking the signature, like chain-replay protection.
    ///
    /// NOTE This is done before the signature is recovered so avoid
    /// doing any state-touching checks that might be expensive.
    ///
    /// TODO: Add flags for which bits of the transaction to check.
    /// TODO: consider including State in the params.
    fn verify_transaction_basic(
        &self,
        t: &UnverifiedTransaction,
        header: &Header,
    ) -> Result<(), transaction::Error> {
        self.machine().verify_transaction_basic(t, header)
    }

    /// Additional information.
    fn additional_params(&self) -> HashMap<String, String> {
        self.machine().additional_params()
    }

    /// Performs pre-validation of RLP decoded transaction before other processing
    fn decode_transaction(
        &self,
        transaction: &[u8],
        best_block_number: BlockNumber,
    ) -> Result<UnverifiedTransaction, transaction::Error> {
        let schedule = self.schedule(best_block_number);
        self.machine().decode_transaction(transaction, &schedule)
    }

    /// Calculates base fee for the block that should be mined next.
    /// This base fee is calculated based on the parent header (last block in blockchain / best block).
    ///
    /// Introduced by EIP1559 to support new market fee mechanism.
    fn calculate_base_fee(&self, parent: &Header) -> Option<U256> {
        self.machine().calc_base_fee(parent)
    }

    /// The configured minimum gas limit. Used by AuRa Engine.
    fn min_gas_limit(&self) -> U256 {
        self.params().min_gas_limit
    }

    /// Returns whether transactions from non externally owned accounts (EOA)
    /// are allowed in the given block number (see EIP-3607).
    ///
    /// That is only possible if EIP-3607 is still not activated.
    fn allow_non_eoa_sender(&self, best_block_number: BlockNumber) -> bool {
        self.params().eip3607_transition > best_block_number
    }
}

// convenience wrappers for existing functions.
impl<T> EthEngine for T where T: Engine<::machine::EthereumMachine> {}

/// Verifier for all blocks within an epoch with self-contained state.
pub trait EpochVerifier<M: machine::Machine>: Send + Sync {
    /// Lightly verify the next block header.
    /// This may not be a header belonging to a different epoch.
    fn verify_light(&self, header: &Header) -> Result<(), M::Error>;

    /// Perform potentially heavier checks on the next block header.
    fn verify_heavy(&self, header: &Header) -> Result<(), M::Error> {
        self.verify_light(header)
    }

    /// Check a finality proof against this epoch verifier.
    /// Returns `Some(hashes)` if the proof proves finality of these hashes.
    /// Returns `None` if the proof doesn't prove anything.
    fn check_finality_proof(&self, _proof: &[u8]) -> Option<Vec<H256>> {
        None
    }
}

/// Special "no-op" verifier for stateless, epoch-less engines.
pub struct NoOp;

impl<M: machine::Machine> EpochVerifier<M> for NoOp {
    fn verify_light(&self, _header: &Header) -> Result<(), M::Error> {
        Ok(())
    }
}
