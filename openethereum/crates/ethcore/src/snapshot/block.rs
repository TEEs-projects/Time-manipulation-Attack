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

//! Block RLP compression.

use bytes::Bytes;
use ethereum_types::{H256, U256};
use hash::keccak;
use rlp::{DecoderError, Rlp, RlpStream};
use triehash::ordered_trie_root;
use types::{
    block::Block, header::Header, transaction::TypedTransaction, views::BlockView, BlockNumber,
};

const HEADER_FIELDS: usize = 8;
const BLOCK_FIELDS: usize = 2;
#[derive(Debug)]
pub struct AbridgedBlock {
    rlp: Bytes,
}

impl AbridgedBlock {
    /// Create from rlp-compressed bytes. Does no verification.
    pub fn from_raw(compressed: Bytes) -> Self {
        AbridgedBlock { rlp: compressed }
    }

    /// Return the inner bytes.
    pub fn into_inner(self) -> Bytes {
        self.rlp
    }

    /// Given a full block view, trim out the parent hash and block number,
    /// producing new rlp.
    pub fn from_block_view(block_view: &BlockView, eip1559_transition: BlockNumber) -> Self {
        let header = block_view.header_view();
        let eip1559 = header.number() >= eip1559_transition;
        let seal_fields = header.seal(eip1559);

        let nmb_of_elements = if eip1559 {
            HEADER_FIELDS + seal_fields.len() + BLOCK_FIELDS + 1
        } else {
            HEADER_FIELDS + seal_fields.len() + BLOCK_FIELDS
        };

        // 10 header fields, unknown number of seal fields, and 2 block fields.
        let mut stream = RlpStream::new_list(nmb_of_elements);

        // write header values.
        stream
            .append(&header.author())
            .append(&header.state_root())
            .append(&header.log_bloom())
            .append(&header.difficulty())
            .append(&header.gas_limit())
            .append(&header.gas_used())
            .append(&header.timestamp())
            .append(&header.extra_data());

        // write block values.

        TypedTransaction::rlp_append_list(&mut stream, &block_view.transactions());
        stream.append_list(&block_view.uncles(eip1559_transition));

        // write seal fields.
        for field in seal_fields {
            stream.append_raw(&field, 1);
        }

        if eip1559 {
            stream.append(&header.base_fee());
        }

        AbridgedBlock { rlp: stream.out() }
    }

    /// Flesh out an abridged block view with the provided parent hash and block number.
    ///
    /// Will fail if contains invalid rlp.
    pub fn to_block(
        &self,
        parent_hash: H256,
        number: u64,
        receipts_root: H256,
        eip1559_transition: BlockNumber,
    ) -> Result<Block, DecoderError> {
        let rlp = Rlp::new(&self.rlp);

        let mut header: Header = Default::default();
        header.set_parent_hash(parent_hash);
        header.set_author(rlp.val_at(0)?);
        header.set_state_root(rlp.val_at(1)?);
        header.set_log_bloom(rlp.val_at(2)?);
        header.set_difficulty(rlp.val_at(3)?);
        header.set_number(number);
        header.set_gas_limit(rlp.val_at(4)?);
        header.set_gas_used(rlp.val_at(5)?);
        header.set_timestamp(rlp.val_at(6)?);
        header.set_extra_data(rlp.val_at(7)?);

        let transactions = TypedTransaction::decode_rlp_list(&rlp.at(8)?)?;
        let uncles = Header::decode_rlp_list(&rlp.at(9)?, eip1559_transition)?;

        header.set_transactions_root(ordered_trie_root(rlp.at(8)?.iter().map(|r| {
            if r.is_list() {
                r.as_raw()
            } else {
                // We already checked if list is valid with decode_rlp_list above
                r.data().expect("To raw rlp list to be valid")
            }
        })));
        header.set_receipts_root(receipts_root);

        let mut uncles_rlp = RlpStream::new();
        uncles_rlp.append_list(&uncles);
        header.set_uncles_hash(keccak(uncles_rlp.as_raw()));

        let mut seal_fields = Vec::new();
        let last_seal_index = if number >= eip1559_transition {
            rlp.item_count()? - 1
        } else {
            rlp.item_count()?
        };
        for i in (HEADER_FIELDS + BLOCK_FIELDS)..last_seal_index {
            let seal_rlp = rlp.at(i)?;
            seal_fields.push(seal_rlp.as_raw().to_owned());
        }
        header.set_seal(seal_fields);

        if number >= eip1559_transition {
            header.set_base_fee(Some(rlp.val_at::<U256>(rlp.item_count()? - 1)?));
        }

        Ok(Block {
            header: header,
            transactions: transactions,
            uncles: uncles,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AbridgedBlock;

    use bytes::Bytes;
    use ethereum_types::{Address, H256, U256};
    use types::{
        block::Block,
        transaction::{Action, Transaction, TypedTransaction},
        view,
        views::BlockView,
        BlockNumber,
    };

    fn encode_block(b: &Block) -> Bytes {
        b.rlp_bytes()
    }

    #[test]
    fn empty_block_abridging() {
        let b = Block::default();
        let receipts_root = b.header.receipts_root().clone();
        let encoded = encode_block(&b);

        let abridged =
            AbridgedBlock::from_block_view(&view!(BlockView, &encoded), BlockNumber::max_value());
        assert_eq!(
            abridged
                .to_block(H256::default(), 0, receipts_root, BlockNumber::max_value())
                .unwrap(),
            b
        );
    }

    #[test]
    fn eip1559_block_abridging() {
        let mut b = Block::default();
        b.header.set_base_fee(Some(U256::from(100)));
        b.header.set_seal(vec![vec![50u8], vec![60u8]]);
        let receipts_root = b.header.receipts_root().clone();
        let encoded = encode_block(&b);

        let abridged =
            AbridgedBlock::from_block_view(&view!(BlockView, &encoded), BlockNumber::default());
        assert_eq!(
            abridged
                .to_block(H256::default(), 0, receipts_root, BlockNumber::default())
                .unwrap(),
            b
        );
    }

    #[test]
    #[should_panic]
    fn wrong_number() {
        let b = Block::default();
        let receipts_root = b.header.receipts_root().clone();
        let encoded = encode_block(&b);

        let abridged =
            AbridgedBlock::from_block_view(&view!(BlockView, &encoded), BlockNumber::max_value());
        assert_eq!(
            abridged
                .to_block(H256::default(), 2, receipts_root, BlockNumber::max_value())
                .unwrap(),
            b
        );
    }

    #[test]
    fn with_transactions() {
        let mut b = Block::default();

        let t1 = TypedTransaction::Legacy(Transaction {
            action: Action::Create,
            nonce: U256::from(42),
            gas_price: U256::from(3000),
            gas: U256::from(50_000),
            value: U256::from(1),
            data: b"Hello!".to_vec(),
        })
        .fake_sign(Address::from_low_u64_be(0x69));

        let t2 = TypedTransaction::Legacy(Transaction {
            action: Action::Create,
            nonce: U256::from(88),
            gas_price: U256::from(12345),
            gas: U256::from(300000),
            value: U256::from(1000000000),
            data: "Eep!".into(),
        })
        .fake_sign(Address::from_low_u64_be(0x55));

        b.transactions.push(t1.into());
        b.transactions.push(t2.into());

        let receipts_root = b.header.receipts_root().clone();
        b.header
            .set_transactions_root(::triehash::ordered_trie_root(
                b.transactions.iter().map(|tx| tx.encode()),
            ));

        let encoded = encode_block(&b);

        let abridged = AbridgedBlock::from_block_view(
            &view!(BlockView, &encoded[..]),
            BlockNumber::max_value(),
        );
        assert_eq!(
            abridged
                .to_block(H256::default(), 0, receipts_root, BlockNumber::max_value())
                .unwrap(),
            b
        );
    }
}
