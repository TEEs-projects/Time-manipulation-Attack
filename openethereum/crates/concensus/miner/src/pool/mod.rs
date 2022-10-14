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

//! Transaction Pool

use ethereum_types::{Address, H256, U256};
use parity_util_mem::MallocSizeOfExt;
use txpool;
use types::transaction;

mod listener;
mod queue;
mod ready;

pub mod client;
pub mod local_transactions;
pub mod replace;
pub mod scoring;
pub mod transaction_filter;
pub mod verifier;

#[cfg(test)]
mod tests;

pub use self::{
    queue::{Status as QueueStatus, TransactionQueue},
    txpool::{Options, VerifiedTransaction as PoolVerifiedTransaction},
};

/// How to prioritize transactions in the pool
///
/// TODO [ToDr] Implement more strategies.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PrioritizationStrategy {
    /// Simple gas-price based prioritization.
    GasPriceOnly,
}

/// Transaction ordering when requesting pending set.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PendingOrdering {
    /// Get pending transactions ordered by their priority (potentially expensive)
    Priority,
    /// Get pending transactions without any care of particular ordering (cheaper).
    Unordered,
}

/// Pending set query settings
#[derive(Debug, Clone)]
pub struct PendingSettings {
    /// Current block number (affects readiness of some transactions).
    pub block_number: u64,
    /// Current timestamp (affects readiness of some transactions).
    pub current_timestamp: u64,
    /// Nonce cap (for dust protection; EIP-168)
    pub nonce_cap: Option<U256>,
    /// Maximal number of transactions in pending the set.
    pub max_len: usize,
    /// Ordering of transactions.
    pub ordering: PendingOrdering,
    /// Value of score that is a boundary between includable and non-includable transactions
    /// Before EIP1559 it should be equal to zero, after EIP1559 it should be equal to block_base_fee
    pub includable_boundary: U256,
    /// If `true` all non-local transactions in the pending set should have
    /// `effective_priority_fee` to be at least `min_gas_price`.
    pub enforce_priority_fees: bool,
}

impl PendingSettings {
    /// Get all transactions (no cap or len limit) prioritized.
    pub fn all_prioritized(block_number: u64, current_timestamp: u64) -> Self {
        PendingSettings {
            block_number,
            current_timestamp,
            nonce_cap: None,
            max_len: usize::max_value(),
            ordering: PendingOrdering::Priority,
            includable_boundary: Default::default(),
            enforce_priority_fees: false,
        }
    }
}

/// Transaction priority.
#[derive(Debug, PartialEq, Eq, PartialOrd, Clone, Copy)]
pub enum Priority {
    /// Regular transactions received over the network. (no priority boost)
    Regular,
    /// Transactions from retracted blocks (medium priority)
    ///
    /// When block becomes non-canonical we re-import the transactions it contains
    /// to the queue and boost their priority.
    Retracted,
    /// Local transactions (high priority)
    ///
    /// Transactions either from a local account or
    /// submitted over local RPC connection via `eth_sendRawTransaction`
    Local,
}

impl Priority {
    fn is_local(&self) -> bool {
        match *self {
            Priority::Local => true,
            _ => false,
        }
    }
}

/// Scoring properties for verified transaction.
pub trait ScoredTransaction {
    /// Gets transaction priority.
    fn priority(&self) -> Priority;

    /// Gets transaction gas price.
    fn effective_gas_price(&self, block_base_fee: Option<U256>) -> U256;

    /// Gets the actual reward miner will get if the transaction is added into the current block.
    fn effective_priority_fee(&self, block_base_fee: Option<U256>) -> U256;

    /// Gets transaction nonce.
    fn nonce(&self) -> U256;

    /// Calculates maximal transaction cost
    /// (`gas_price` * `gas_limit` + `value`).
    fn cost(&self) -> U256;
}

/// Verified transaction stored in the pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedTransaction {
    transaction: transaction::PendingTransaction,
    // TODO [ToDr] hash and sender should go directly from the transaction
    hash: H256,
    sender: Address,
    priority: Priority,
    insertion_id: usize,
}

impl VerifiedTransaction {
    /// Create `VerifiedTransaction` directly from `SignedTransaction`.
    ///
    /// This method should be used only:
    /// 1. for tests
    /// 2. In case we are converting pending block transactions that are already in the queue to match the function signature.
    pub fn from_pending_block_transaction(tx: transaction::SignedTransaction) -> Self {
        let hash = tx.hash();
        let sender = tx.sender();
        VerifiedTransaction {
            transaction: tx.into(),
            hash,
            sender,
            priority: Priority::Retracted,
            insertion_id: 0,
        }
    }

    /// Gets transaction insertion id.
    pub(crate) fn insertion_id(&self) -> usize {
        self.insertion_id
    }

    /// Gets wrapped `SignedTransaction`
    pub fn signed(&self) -> &transaction::SignedTransaction {
        &self.transaction
    }

    /// Gets wrapped `PendingTransaction`
    pub fn pending(&self) -> &transaction::PendingTransaction {
        &self.transaction
    }
}

impl txpool::VerifiedTransaction for VerifiedTransaction {
    type Hash = H256;
    type Sender = Address;

    fn hash(&self) -> &H256 {
        &self.hash
    }

    fn mem_usage(&self) -> usize {
        self.transaction.malloc_size_of()
    }

    fn sender(&self) -> &Address {
        &self.sender
    }

    fn has_zero_gas_price(&self) -> bool {
        self.transaction.has_zero_gas_price()
    }
}

impl ScoredTransaction for VerifiedTransaction {
    fn priority(&self) -> Priority {
        self.priority
    }

    fn effective_gas_price(&self, block_base_fee: Option<U256>) -> U256 {
        self.transaction.effective_gas_price(block_base_fee)
    }

    fn effective_priority_fee(&self, block_base_fee: Option<U256>) -> U256 {
        self.transaction.effective_priority_fee(block_base_fee)
    }

    /// Gets transaction nonce.
    fn nonce(&self) -> U256 {
        self.transaction.tx().nonce
    }

    /// Gets maximum potential cost of the transaction.
    fn cost(&self) -> U256 {
        let tx = self.transaction.tx();
        // As the transaction is verified, arithmetic
        // expression below could not overflow.
        tx.gas_price * tx.gas + tx.value
    }
}
