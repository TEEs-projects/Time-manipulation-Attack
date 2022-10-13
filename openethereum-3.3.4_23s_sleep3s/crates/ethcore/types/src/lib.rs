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

//! Types used in the public API
//!
//! This crate stores Open Etherem specific types that are
//! COMMONLY used across different separate modules of the codebase.
//! It should only focus on data structures, not any logic that relates to them.
//!
//! The interaction between modules should be possible by
//! implementing a required trait that potentially uses some of the data
//! structures from that crate.
//!
//! NOTE If you can specify your data type in the same crate as your trait, please do that.
//! Don't treat this crate as a bag for any types that we use in OpenEthereum.
//! This one is reserved for types that are shared heavily (like transactions),
//! historically this contains types extracted from `ethcore` crate, if possible
//! we should try to dissolve that crate in favour of more fine-grained crates,
//! by moving the types closer to where they are actually required.

#![allow(missing_docs)]
#![warn(unused_extern_crates)]

pub use keccak_hash as hash;
pub use parity_bytes as bytes;
pub use parity_crypto as crypto;

#[macro_use]
extern crate rlp_derive;

#[cfg(test)]
pub use rustc_hex;

#[macro_use]
pub mod views;

pub mod account_diff;
pub mod ancestry_action;
pub mod basic_account;
pub mod block;
pub mod block_status;
pub mod blockchain_info;
pub mod call_analytics;
pub mod creation_status;
pub mod data_format;
pub mod encoded;
pub mod engines;
pub mod filter;
pub mod header;
pub mod ids;
pub mod log_entry;
pub mod pruning_info;
pub mod receipt;
pub mod restoration_status;
pub mod security_level;
pub mod snapshot_manifest;
pub mod state_diff;
pub mod trace_filter;
pub mod transaction;
pub mod tree_route;
pub mod verification_queue_info;

/// Type for block number.
pub type BlockNumber = u64;
