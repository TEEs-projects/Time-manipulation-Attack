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

//! Lazily-decoded owning views of RLP-encoded blockchain objects.
//! These views are meant to contain _trusted_ data -- without encoding
//! errors or inconsistencies.
//!
//! In general these views are useful when only a few fields of an object
//! are relevant. In these cases it's more efficient to decode the object piecemeal.
//! When the entirety of the object is needed, it's better to upgrade it to a fully
//! decoded object where parts like the hash can be saved.

use crate::{
    block::Block as FullBlock,
    hash::keccak,
    header::Header as FullHeader,
    transaction::UnverifiedTransaction,
    views::{self, BlockView, BodyView, HeaderView},
    BlockNumber,
};

use ethereum_types::{Address, Bloom, H256, U256};
use parity_util_mem::MallocSizeOf;
use rlp::{self, Rlp, RlpStream};

/// Owning header view.
#[derive(Debug, Clone, PartialEq, Eq, MallocSizeOf)]
pub struct Header(Vec<u8>);

impl Header {
    /// Create a new owning header view.
    /// Expects the data to be an RLP-encoded header -- any other case will likely lead to
    /// panics further down the line.
    pub fn new(encoded: Vec<u8>) -> Self {
        Header(encoded)
    }

    /// Upgrade this encoded view to a fully owned `Header` object.
    pub fn decode(&self, eip1559_transition: BlockNumber) -> Result<FullHeader, rlp::DecoderError> {
        FullHeader::decode_rlp(&self.rlp(), eip1559_transition)
    }

    /// Get a borrowed header view onto the data.
    #[inline]
    pub fn view(&self) -> HeaderView {
        view!(HeaderView, &self.0)
    }

    /// Get the rlp of the header.
    #[inline]
    pub fn rlp(&self) -> Rlp {
        Rlp::new(&self.0)
    }

    /// Consume the view and return the raw bytes.
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

// forwarders to borrowed view.
impl Header {
    /// Returns the header hash.
    pub fn hash(&self) -> H256 {
        keccak(&self.0)
    }

    /// Returns the parent hash.
    pub fn parent_hash(&self) -> H256 {
        self.view().parent_hash()
    }

    /// Returns the uncles hash.
    pub fn uncles_hash(&self) -> H256 {
        self.view().uncles_hash()
    }

    /// Returns the author.
    pub fn author(&self) -> Address {
        self.view().author()
    }

    /// Returns the state root.
    pub fn state_root(&self) -> H256 {
        self.view().state_root()
    }

    /// Returns the transaction trie root.
    pub fn transactions_root(&self) -> H256 {
        self.view().transactions_root()
    }

    /// Returns the receipts trie root
    pub fn receipts_root(&self) -> H256 {
        self.view().receipts_root()
    }

    /// Returns the block log bloom
    pub fn log_bloom(&self) -> Bloom {
        self.view().log_bloom()
    }

    /// Difficulty of this block
    pub fn difficulty(&self) -> U256 {
        self.view().difficulty()
    }

    /// Number of this block.
    pub fn number(&self) -> BlockNumber {
        self.view().number()
    }

    /// Time this block was produced.
    pub fn timestamp(&self) -> u64 {
        self.view().timestamp()
    }

    /// Gas limit of this block.
    pub fn gas_limit(&self) -> U256 {
        self.view().gas_limit()
    }

    /// Total gas used in this block.
    pub fn gas_used(&self) -> U256 {
        self.view().gas_used()
    }

    /// Block extra data.
    pub fn extra_data(&self) -> Vec<u8> {
        self.view().extra_data()
    }

    /// Engine-specific seal fields.
    pub fn seal(&self, eip1559: bool) -> Vec<Vec<u8>> {
        self.view().seal(eip1559)
    }

    /// Base fee.
    pub fn base_fee(&self) -> U256 {
        self.view().base_fee()
    }
}

/// Owning block body view.
#[derive(Debug, Clone, PartialEq, Eq, MallocSizeOf)]
pub struct Body(Vec<u8>);

impl Body {
    /// Create a new owning block body view. The raw bytes passed in must be an rlp-encoded block
    /// body.
    pub fn new(raw: Vec<u8>) -> Self {
        Body(raw)
    }

    /// Get a borrowed view of the data within.
    #[inline]
    pub fn view(&self) -> BodyView {
        view!(BodyView, &self.0)
    }

    /// Fully decode this block body.
    pub fn decode(
        &self,
        eip1559_transition: BlockNumber,
    ) -> (Vec<UnverifiedTransaction>, Vec<FullHeader>) {
        (
            self.view().transactions(),
            self.view().uncles(eip1559_transition),
        )
    }

    /// Get the RLP of this block body.
    #[inline]
    pub fn rlp(&self) -> Rlp {
        Rlp::new(&self.0)
    }

    /// Consume the view and return the raw bytes.
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

// forwarders to borrowed view.
impl Body {
    /// Get raw rlp of transactions
    pub fn transactions_rlp(&self) -> Rlp {
        self.view().transactions_rlp().rlp
    }

    /// Get a vector of all transactions.
    pub fn transactions(&self) -> Vec<UnverifiedTransaction> {
        self.view().transactions()
    }

    /// Number of transactions in the block.
    pub fn transactions_count(&self) -> usize {
        self.view().transactions_count()
    }

    /// A view over each transaction in the block.
    pub fn transaction_views(&self) -> Vec<views::TypedTransactionView> {
        self.view().transaction_views()
    }

    /// The hash of each transaction in the block.
    pub fn transaction_hashes(&self) -> Vec<H256> {
        self.view().transaction_hashes()
    }

    /// Get raw rlp of uncle headers
    pub fn uncles_rlp(&self) -> Rlp {
        self.view().uncles_rlp().rlp
    }

    /// Decode uncle headers.
    pub fn uncles(&self, eip1559_transition: BlockNumber) -> Vec<FullHeader> {
        self.view().uncles(eip1559_transition)
    }

    /// Number of uncles.
    pub fn uncles_count(&self) -> usize {
        self.view().uncles_count()
    }

    /// Borrowed view over each uncle.
    pub fn uncle_views(&self) -> Vec<views::HeaderView> {
        self.view().uncle_views()
    }

    /// Hash of each uncle.
    pub fn uncle_hashes(&self) -> Vec<H256> {
        self.view().uncle_hashes()
    }
}

/// Owning block view.
#[derive(Debug, Clone, PartialEq, Eq, MallocSizeOf)]
pub struct Block(Vec<u8>);

impl Block {
    /// Create a new owning block view. The raw bytes passed in must be an rlp-encoded block.
    pub fn new(raw: Vec<u8>) -> Self {
        Block(raw)
    }

    /// Create a new owning block view by concatenating the encoded header and body
    pub fn new_from_header_and_body(header: &views::HeaderView, body: &views::BodyView) -> Self {
        let mut stream = RlpStream::new_list(3);
        stream.append_raw(header.rlp().as_raw(), 1);
        stream.append_raw(body.transactions_rlp().as_raw(), 1);
        stream.append_raw(body.uncles_rlp().as_raw(), 1);
        Block::new(stream.out())
    }

    /// Get a borrowed view of the whole block.
    #[inline]
    pub fn view(&self) -> BlockView {
        view!(BlockView, &self.0)
    }

    /// Get a borrowed view of the block header.
    #[inline]
    pub fn header_view(&self) -> HeaderView {
        self.view().header_view()
    }

    /// Decode to a full block.
    pub fn decode(&self, eip1559_transition: BlockNumber) -> Result<FullBlock, rlp::DecoderError> {
        FullBlock::decode_rlp(&self.rlp(), eip1559_transition)
    }

    /// Decode the header.
    pub fn decode_header(&self, eip1559_transition: BlockNumber) -> FullHeader {
        FullHeader::decode_rlp(&self.view().rlp().at(0).rlp, eip1559_transition).unwrap_or_else(
            |e| {
                panic!(
                    "block header, view rlp is trusted and should be valid: {:?}",
                    e
                )
            },
        )
    }

    /// Clone the encoded header.
    pub fn header(&self) -> Header {
        Header(self.view().rlp().at(0).as_raw().to_vec())
    }

    /// Get the rlp of this block.
    #[inline]
    pub fn rlp(&self) -> Rlp {
        Rlp::new(&self.0)
    }

    /// Consume the view and return the raw bytes.
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }

    /// Returns the reference to slice of bytes
    pub fn raw(&self) -> &[u8] {
        &self.0
    }
}

// forwarders to borrowed header view.
impl Block {
    /// Returns the header hash.
    pub fn hash(&self) -> H256 {
        self.header_view().hash()
    }

    /// Returns the parent hash.
    pub fn parent_hash(&self) -> H256 {
        self.header_view().parent_hash()
    }

    /// Returns the uncles hash.
    pub fn uncles_hash(&self) -> H256 {
        self.header_view().uncles_hash()
    }

    /// Returns the author.
    pub fn author(&self) -> Address {
        self.header_view().author()
    }

    /// Returns the state root.
    pub fn state_root(&self) -> H256 {
        self.header_view().state_root()
    }

    /// Returns the transaction trie root.
    pub fn transactions_root(&self) -> H256 {
        self.header_view().transactions_root()
    }

    /// Returns the receipts trie root
    pub fn receipts_root(&self) -> H256 {
        self.header_view().receipts_root()
    }

    /// Returns the block log bloom
    pub fn log_bloom(&self) -> Bloom {
        self.header_view().log_bloom()
    }

    /// Difficulty of this block
    pub fn difficulty(&self) -> U256 {
        self.header_view().difficulty()
    }

    /// Number of this block.
    pub fn number(&self) -> BlockNumber {
        self.header_view().number()
    }

    /// Time this block was produced.
    pub fn timestamp(&self) -> u64 {
        self.header_view().timestamp()
    }

    /// Gas limit of this block.
    pub fn gas_limit(&self) -> U256 {
        self.header_view().gas_limit()
    }

    /// Total gas used in this block.
    pub fn gas_used(&self) -> U256 {
        self.header_view().gas_used()
    }

    /// Block extra data.
    pub fn extra_data(&self) -> Vec<u8> {
        self.header_view().extra_data()
    }

    /// Engine-specific seal fields.
    pub fn seal(&self, eip1559: bool) -> Vec<Vec<u8>> {
        self.header_view().seal(eip1559)
    }
}

// forwarders to body view.
impl Block {
    /// Get a vector of all transactions.
    pub fn transactions(&self) -> Vec<UnverifiedTransaction> {
        self.view().transactions()
    }

    /// Number of transactions in the block.
    pub fn transactions_count(&self) -> usize {
        self.view().transactions_count()
    }

    /// A view over each transaction in the block.
    pub fn transaction_views(&self) -> Vec<views::TypedTransactionView> {
        self.view().transaction_views()
    }

    /// The hash of each transaction in the block.
    pub fn transaction_hashes(&self) -> Vec<H256> {
        self.view().transaction_hashes()
    }

    /// Decode uncle headers.
    pub fn uncles(&self, eip1559_transition: BlockNumber) -> Vec<FullHeader> {
        self.view().uncles(eip1559_transition)
    }

    /// Number of uncles.
    pub fn uncles_count(&self) -> usize {
        self.view().uncles_count()
    }

    /// Borrowed view over each uncle.
    pub fn uncle_views(&self) -> Vec<views::HeaderView> {
        self.view().uncle_views()
    }

    /// Hash of each uncle.
    pub fn uncle_hashes(&self) -> Vec<H256> {
        self.view().uncle_hashes()
    }
}
