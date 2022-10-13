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

use bytes::Bytes;
use ethcore::verification::queue::kind::blocks::Unverified;
use ethereum_types::H256;
use hash::{keccak, KECCAK_EMPTY_LIST_RLP, KECCAK_NULL_RLP};
use network;
use parity_util_mem::MallocSizeOf;
use rlp::{DecoderError, Rlp, RlpStream};
use std::collections::{hash_map, BTreeMap, HashMap, HashSet};
use triehash_ethereum::ordered_trie_root;
use types::{
    header::Header as BlockHeader,
    transaction::{TypedTransaction, UnverifiedTransaction},
    BlockNumber,
};

malloc_size_of_is_0!(HeaderId);

#[derive(PartialEq, Debug, Clone, MallocSizeOf)]
pub struct SyncHeader {
    pub bytes: Bytes,
    pub header: BlockHeader,
}

impl SyncHeader {
    pub fn from_rlp(bytes: Bytes, eip1559_transition: BlockNumber) -> Result<Self, DecoderError> {
        let rlp = Rlp::new(&bytes);
        let result = SyncHeader {
            header: BlockHeader::decode_rlp(&rlp, eip1559_transition)?,
            bytes,
        };

        Ok(result)
    }
}

#[derive(MallocSizeOf)]
pub struct SyncBody {
    pub transactions_bytes: Bytes,
    pub transactions: Vec<UnverifiedTransaction>,
    pub uncles_bytes: Bytes,
    pub uncles: Vec<BlockHeader>,
}

impl SyncBody {
    pub fn from_rlp(bytes: &[u8], eip1559_transition: BlockNumber) -> Result<Self, DecoderError> {
        let rlp = Rlp::new(bytes);
        let transactions_rlp = rlp.at(0)?;
        let uncles_rlp = rlp.at(1)?;

        let result = SyncBody {
            transactions_bytes: transactions_rlp.as_raw().to_vec(),
            transactions: TypedTransaction::decode_rlp_list(&transactions_rlp)?,
            uncles_bytes: uncles_rlp.as_raw().to_vec(),
            uncles: BlockHeader::decode_rlp_list(&uncles_rlp, eip1559_transition)?,
        };

        Ok(result)
    }

    fn empty_body() -> Self {
        SyncBody {
            transactions_bytes: ::rlp::EMPTY_LIST_RLP.to_vec(),
            transactions: Vec::with_capacity(0),
            uncles_bytes: ::rlp::EMPTY_LIST_RLP.to_vec(),
            uncles: Vec::with_capacity(0),
        }
    }
}

/// Block data with optional body.
#[derive(MallocSizeOf)]
struct SyncBlock {
    header: SyncHeader,
    body: Option<SyncBody>,
    receipts: Option<Bytes>,
    receipts_root: H256,
}

fn unverified_from_sync(header: SyncHeader, body: Option<SyncBody>) -> Unverified {
    let mut stream = RlpStream::new_list(3);
    stream.append_raw(&header.bytes, 1);
    let body = body.unwrap_or_else(SyncBody::empty_body);
    stream.append_raw(&body.transactions_bytes, 1);
    stream.append_raw(&body.uncles_bytes, 1);

    Unverified {
        header: header.header,
        transactions: body.transactions,
        uncles: body.uncles,
        bytes: stream.out().to_vec(),
    }
}

/// Block with optional receipt
pub struct BlockAndReceipts {
    /// Block data.
    pub block: Unverified,
    /// Block receipts RLP list.
    pub receipts: Option<Bytes>,
}

/// Used to identify header by transactions and uncles hashes
#[derive(Eq, PartialEq, Hash)]
struct HeaderId {
    transactions_root: H256,
    uncles: H256,
}

/// A collection of blocks and subchain pointers being downloaded. This keeps track of
/// which headers/bodies need to be downloaded, which are being downloaded and also holds
/// the downloaded blocks.
#[derive(Default)]
pub struct BlockCollection {
    /// Does this collection need block receipts.
    need_receipts: bool,
    /// Heads of subchains to download
    heads: Vec<H256>,
    /// Downloaded blocks.
    blocks: HashMap<H256, SyncBlock>,
    /// Downloaded blocks by parent.
    parents: HashMap<H256, H256>,
    /// Used to map body to header.
    header_ids: HashMap<HeaderId, H256>,
    /// Used to map receipts root to headers.
    receipt_ids: HashMap<H256, Vec<H256>>,
    /// First block in `blocks`.
    head: Option<H256>,
    /// Set of block header hashes being downloaded
    downloading_headers: HashSet<H256>,
    /// Set of block bodies being downloaded identified by block hash.
    downloading_bodies: HashSet<H256>,
    /// Set of block receipts being downloaded identified by receipt root.
    downloading_receipts: HashSet<H256>,
}

impl BlockCollection {
    /// Create a new instance.
    pub fn new(download_receipts: bool) -> BlockCollection {
        BlockCollection {
            need_receipts: download_receipts,
            blocks: HashMap::new(),
            header_ids: HashMap::new(),
            receipt_ids: HashMap::new(),
            heads: Vec::new(),
            parents: HashMap::new(),
            head: None,
            downloading_headers: HashSet::new(),
            downloading_bodies: HashSet::new(),
            downloading_receipts: HashSet::new(),
        }
    }

    /// Clear everything.
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.parents.clear();
        self.header_ids.clear();
        self.receipt_ids.clear();
        self.heads.clear();
        self.head = None;
        self.downloading_headers.clear();
        self.downloading_bodies.clear();
        self.downloading_receipts.clear();
    }

    /// Reset collection for a new sync round with given subchain block hashes.
    pub fn reset_to(&mut self, hashes: Vec<H256>) {
        self.clear();
        self.heads = hashes;
    }

    /// Insert a set of headers into collection and advance subchain head pointers.
    pub fn insert_headers(&mut self, headers: Vec<SyncHeader>) {
        for h in headers {
            if let Err(e) = self.insert_header(h) {
                trace!(target: "sync", "Ignored invalid header: {:?}", e);
            }
        }
        self.update_heads();
    }

    /// Insert a collection of block bodies for previously downloaded headers.
    pub fn insert_bodies(&mut self, bodies: Vec<SyncBody>) -> Vec<H256> {
        bodies
            .into_iter()
            .filter_map(|b| {
                self.insert_body(b)
                    .map_err(|e| trace!(target: "sync", "Ignored invalid body: {:?}", e))
                    .ok()
            })
            .collect()
    }

    /// Insert a collection of block receipts for previously downloaded headers.
    pub fn insert_receipts(&mut self, receipts: Vec<Bytes>) -> Vec<Vec<H256>> {
        if !self.need_receipts {
            return Vec::new();
        }
        receipts
            .into_iter()
            .filter_map(|r| {
                self.insert_receipt(r)
                    .map_err(|e| trace!(target: "sync", "Ignored invalid receipt: {:?}", e))
                    .ok()
            })
            .collect()
    }

    /// Returns a set of block hashes that require a body download. The returned set is marked as being downloaded.
    pub fn needed_bodies(&mut self, count: usize, _ignore_downloading: bool) -> Vec<H256> {
        if self.head.is_none() {
            return Vec::new();
        }
        let mut needed_bodies: Vec<H256> = Vec::new();
        let mut head = self.head;
        while head.is_some() && needed_bodies.len() < count {
            head = self.parents.get(&head.unwrap()).cloned();
            if let Some(head) = head {
                match self.blocks.get(&head) {
                    Some(block)
                        if block.body.is_none() && !self.downloading_bodies.contains(&head) =>
                    {
                        self.downloading_bodies.insert(head.clone());
                        needed_bodies.push(head.clone());
                    }
                    _ => (),
                }
            }
        }
        for h in self.header_ids.values() {
            if needed_bodies.len() >= count {
                break;
            }
            if !self.downloading_bodies.contains(h) {
                needed_bodies.push(h.clone());
                self.downloading_bodies.insert(h.clone());
            }
        }
        needed_bodies
    }

    /// Returns a set of block hashes that require a receipt download. The returned set is marked as being downloaded.
    pub fn needed_receipts(&mut self, count: usize, _ignore_downloading: bool) -> Vec<H256> {
        if self.head.is_none() || !self.need_receipts {
            return Vec::new();
        }
        let mut needed_receipts: Vec<H256> = Vec::new();
        let mut head = self.head;
        while head.is_some() && needed_receipts.len() < count {
            head = self.parents.get(&head.unwrap()).cloned();
            if let Some(head) = head {
                match self.blocks.get(&head) {
                    Some(block) => {
                        if block.receipts.is_none()
                            && !self.downloading_receipts.contains(&block.receipts_root)
                        {
                            self.downloading_receipts.insert(block.receipts_root);
                            needed_receipts.push(head.clone());
                        }
                    }
                    _ => (),
                }
            }
        }
        // If there are multiple blocks per receipt, only request one of them.
        for (root, h) in self
            .receipt_ids
            .iter()
            .map(|(root, hashes)| (root, hashes[0]))
        {
            if needed_receipts.len() >= count {
                break;
            }
            if !self.downloading_receipts.contains(root) {
                needed_receipts.push(h.clone());
                self.downloading_receipts.insert(*root);
            }
        }
        needed_receipts
    }

    /// Returns a set of block hashes that require a header download. The returned set is marked as being downloaded.
    pub fn needed_headers(
        &mut self,
        count: usize,
        ignore_downloading: bool,
    ) -> Option<(H256, usize)> {
        // find subchain to download
        let mut download = None;
        {
            for h in &self.heads {
                if ignore_downloading || !self.downloading_headers.contains(h) {
                    self.downloading_headers.insert(h.clone());
                    download = Some(h.clone());
                    break;
                }
            }
        }
        download.map(|h| (h, count))
    }

    /// Unmark header as being downloaded.
    pub fn clear_header_download(&mut self, hash: &H256) {
        self.downloading_headers.remove(hash);
    }

    /// Unmark block body as being downloaded.
    pub fn clear_body_download(&mut self, hashes: &[H256]) {
        for h in hashes {
            self.downloading_bodies.remove(h);
        }
    }

    /// Unmark block receipt as being downloaded.
    pub fn clear_receipt_download(&mut self, hashes: &[H256]) {
        for h in hashes {
            if let Some(ref block) = self.blocks.get(h) {
                self.downloading_receipts.remove(&block.receipts_root);
            }
        }
    }

    /// Get a valid chain of blocks ordered in ascending order and ready for importing into blockchain.
    pub fn drain(&mut self) -> Vec<BlockAndReceipts> {
        if self.blocks.is_empty() || self.head.is_none() {
            return Vec::new();
        }

        let mut drained = Vec::new();
        let mut hashes = Vec::new();
        {
            let mut blocks = Vec::new();
            let mut head = self.head;
            while let Some(h) = head {
                head = self.parents.get(&h).cloned();
                if let Some(head) = head {
                    match self.blocks.remove(&head) {
                        Some(block) => {
                            if block.body.is_some()
                                && (!self.need_receipts || block.receipts.is_some())
                            {
                                blocks.push(block);
                                hashes.push(head);
                                self.head = Some(head);
                            } else {
                                self.blocks.insert(head, block);
                                break;
                            }
                        }
                        _ => {
                            break;
                        }
                    }
                }
            }

            for block in blocks.into_iter() {
                let unverified = unverified_from_sync(block.header, block.body);
                drained.push(BlockAndReceipts {
                    block: unverified,
                    receipts: block.receipts.clone(),
                });
            }
        }

        trace!(target: "sync", "Drained {} blocks, new head :{:?}", drained.len(), self.head);
        drained
    }

    /// Check if the collection is empty. We consider the syncing round complete once
    /// there is no block data left and only a single or none head pointer remains.
    pub fn is_empty(&self) -> bool {
        self.heads.len() == 0
            || (self.heads.len() == 1 && self.head.map_or(false, |h| h == self.heads[0]))
    }

    /// Check if collection contains a block header.
    pub fn contains(&self, hash: &H256) -> bool {
        self.blocks.contains_key(hash)
    }

    /// Check the number of heads
    pub fn heads_len(&self) -> usize {
        self.heads.len()
    }

    /// Return number of items size.
    pub fn get_sizes(&self, sizes: &mut BTreeMap<String, usize>, insert_prefix: &str) {
        sizes.insert(format!("{}{}", insert_prefix, "heads"), self.heads.len());
        sizes.insert(format!("{}{}", insert_prefix, "blocks"), self.blocks.len());
        sizes.insert(
            format!("{}{}", insert_prefix, "parents_len"),
            self.parents.len(),
        );
        sizes.insert(
            format!("{}{}", insert_prefix, "header_ids_len"),
            self.header_ids.len(),
        );
        sizes.insert(
            format!("{}{}", insert_prefix, "downloading_headers_len"),
            self.downloading_headers.len(),
        );
        sizes.insert(
            format!("{}{}", insert_prefix, "downloading_bodies_len"),
            self.downloading_bodies.len(),
        );

        if self.need_receipts {
            sizes.insert(
                format!("{}{}", insert_prefix, "downloading_receipts_len"),
                self.downloading_receipts.len(),
            );
            sizes.insert(
                format!("{}{}", insert_prefix, "receipt_ids_len"),
                self.receipt_ids.len(),
            );
        }
    }

    /// Check if given block hash is marked as being downloaded.
    pub fn is_downloading(&self, hash: &H256) -> bool {
        self.downloading_headers.contains(hash) || self.downloading_bodies.contains(hash)
    }

    fn insert_body(&mut self, body: SyncBody) -> Result<H256, network::Error> {
        let header_id = {
            let tx_root = ordered_trie_root(Rlp::new(&body.transactions_bytes).iter().map(|r| {
                if r.is_list() {
                    r.as_raw()
                } else {
                    // this list is already decoded and passed validation, for this we are okay to expect proper data
                    r.data().expect("Expect raw transaction list to be valid")
                }
            }));
            let uncles = keccak(&body.uncles_bytes);
            HeaderId {
                transactions_root: tx_root,
                uncles: uncles,
            }
        };

        match self.header_ids.remove(&header_id) {
            Some(h) => {
                self.downloading_bodies.remove(&h);
                match self.blocks.get_mut(&h) {
                    Some(ref mut block) => {
                        trace!(target: "sync", "Got body {}", h);
                        block.body = Some(body);
                        Ok(h)
                    }
                    None => {
                        warn!("Got body with no header {}", h);
                        Err(network::ErrorKind::BadProtocol.into())
                    }
                }
            }
            None => {
                trace!(target: "sync", "Ignored unknown/stale block body. tx_root = {:?}, uncles = {:?}", header_id.transactions_root, header_id.uncles);
                Err(network::ErrorKind::BadProtocol.into())
            }
        }
    }

    fn insert_receipt(&mut self, r: Bytes) -> Result<Vec<H256>, network::Error> {
        let receipt_root = {
            let receipts = Rlp::new(&r);
            //check receipts data before calculating trie root
            let mut temp_receipts: Vec<&[u8]> = Vec::new();
            for receipt_byte in receipts.iter() {
                if receipt_byte.is_list() {
                    temp_receipts.push(receipt_byte.as_raw())
                } else {
                    temp_receipts.push(
                        receipt_byte
                            .data()
                            .map_err(|e| network::ErrorKind::Rlp(e))?,
                    );
                }
            }

            // calculate trie root and use it as hash
            ordered_trie_root(temp_receipts.iter())
        };
        self.downloading_receipts.remove(&receipt_root);
        match self.receipt_ids.entry(receipt_root) {
            hash_map::Entry::Occupied(entry) => {
                let block_hashes = entry.remove();
                for h in block_hashes.iter() {
                    match self.blocks.get_mut(&h) {
                        Some(ref mut block) => {
                            trace!(target: "sync", "Got receipt {}", h);
                            block.receipts = Some(r.clone());
                        }
                        None => {
                            warn!("Got receipt with no header {}", h);
                            return Err(network::ErrorKind::BadProtocol.into());
                        }
                    }
                }
                Ok(block_hashes)
            }
            hash_map::Entry::Vacant(_) => {
                trace!(target: "sync", "Ignored unknown/stale block receipt {:?}", receipt_root);
                Err(network::ErrorKind::BadProtocol.into())
            }
        }
    }

    fn insert_header(&mut self, info: SyncHeader) -> Result<H256, DecoderError> {
        let hash = info.header.hash();
        if self.blocks.contains_key(&hash) {
            return Ok(hash);
        }

        match self.head {
            None if hash == self.heads[0] => {
                trace!(target: "sync", "New head {}", hash);
                self.head = Some(info.header.parent_hash().clone());
            }
            _ => (),
        }

        let header_id = HeaderId {
            transactions_root: *info.header.transactions_root(),
            uncles: *info.header.uncles_hash(),
        };

        let body = if header_id.transactions_root == KECCAK_NULL_RLP
            && header_id.uncles == KECCAK_EMPTY_LIST_RLP
        {
            // empty body, just mark as downloaded
            Some(SyncBody::empty_body())
        } else {
            trace!(
                "Queueing body tx_root = {:?}, uncles = {:?}, block = {:?}, number = {}",
                header_id.transactions_root,
                header_id.uncles,
                hash,
                info.header.number()
            );
            self.header_ids.insert(header_id, hash);
            None
        };

        let (receipts, receipts_root) = if self.need_receipts {
            let receipt_root = *info.header.receipts_root();
            if receipt_root == KECCAK_NULL_RLP {
                let receipts_stream = RlpStream::new_list(0);
                (Some(receipts_stream.out()), receipt_root)
            } else {
                self.receipt_ids
                    .entry(receipt_root)
                    .or_insert_with(Vec::new)
                    .push(hash);
                (None, receipt_root)
            }
        } else {
            (None, H256::default())
        };

        self.parents.insert(*info.header.parent_hash(), hash);

        let block = SyncBlock {
            header: info,
            body,
            receipts,
            receipts_root,
        };

        self.blocks.insert(hash, block);
        trace!(target: "sync", "New header: {:x}", hash);
        Ok(hash)
    }

    // update subchain headers
    fn update_heads(&mut self) {
        let mut new_heads = Vec::new();
        let old_subchains: HashSet<_> = { self.heads.iter().cloned().collect() };
        for s in self.heads.drain(..) {
            let mut h = s.clone();
            if !self.blocks.contains_key(&h) {
                new_heads.push(h);
                continue;
            }
            loop {
                match self.parents.get(&h) {
                    Some(next) => {
                        h = next.clone();
                        if old_subchains.contains(&h) {
                            trace!(target: "sync", "Completed subchain {:?}", s);
                            break; // reached head of the other subchain, merge by not adding
                        }
                    }
                    _ => {
                        new_heads.push(h);
                        break;
                    }
                }
            }
        }
        self.heads = new_heads;
    }
}

#[cfg(test)]
mod test {
    use super::{BlockCollection, SyncHeader};
    use ethcore::{
        client::{BlockChainClient, BlockId, EachBlockWith, TestBlockChainClient},
        verification::queue::kind::blocks::Unverified,
    };
    use rlp::*;
    use types::BlockNumber;

    fn is_empty(bc: &BlockCollection) -> bool {
        bc.heads.is_empty()
            && bc.blocks.is_empty()
            && bc.parents.is_empty()
            && bc.header_ids.is_empty()
            && bc.head.is_none()
            && bc.downloading_headers.is_empty()
            && bc.downloading_bodies.is_empty()
    }

    #[test]
    fn create_clear() {
        let mut bc = BlockCollection::new(false);
        assert!(is_empty(&bc));
        let client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Nothing);
        let hashes = (0..100)
            .map(|i| {
                (&client as &dyn BlockChainClient)
                    .block_hash(BlockId::Number(i))
                    .unwrap()
            })
            .collect();
        bc.reset_to(hashes);
        assert!(!is_empty(&bc));
        bc.clear();
        assert!(is_empty(&bc));
    }

    #[test]
    fn insert_headers() {
        let mut bc = BlockCollection::new(false);
        assert!(is_empty(&bc));
        let client = TestBlockChainClient::new();
        let nblocks = 200;
        client.add_blocks(nblocks, EachBlockWith::Nothing);
        let blocks: Vec<_> = (0..nblocks)
            .map(|i| {
                (&client as &dyn BlockChainClient)
                    .block(BlockId::Number(i as BlockNumber))
                    .unwrap()
                    .into_inner()
            })
            .collect();
        let headers: Vec<_> = blocks
            .iter()
            .map(|b| {
                SyncHeader::from_rlp(
                    Rlp::new(b).at(0).unwrap().as_raw().to_vec(),
                    client.spec.params().eip1559_transition,
                )
                .unwrap()
            })
            .collect();
        let hashes: Vec<_> = headers.iter().map(|h| h.header.hash()).collect();
        let heads: Vec<_> = hashes
            .iter()
            .enumerate()
            .filter_map(|(i, h)| if i % 20 == 0 { Some(*h) } else { None })
            .collect();
        bc.reset_to(heads);
        assert!(!bc.is_empty());
        assert_eq!(hashes[0], bc.heads[0]);
        assert!(bc.needed_bodies(1, false).is_empty());
        assert!(!bc.contains(&hashes[0]));
        assert!(!bc.is_downloading(&hashes[0]));

        let (h, n) = bc.needed_headers(6, false).unwrap();
        assert!(bc.is_downloading(&hashes[0]));
        assert_eq!(hashes[0], h);
        assert_eq!(n, 6);
        assert_eq!(bc.downloading_headers.len(), 1);
        assert!(bc.drain().is_empty());

        bc.insert_headers(headers[0..6].into_iter().map(Clone::clone).collect());
        assert_eq!(hashes[5], bc.heads[0]);
        for h in &hashes[0..6] {
            bc.clear_header_download(h)
        }
        assert_eq!(bc.downloading_headers.len(), 0);
        assert!(!bc.is_downloading(&hashes[0]));
        assert!(bc.contains(&hashes[0]));

        assert_eq!(
            bc.drain().into_iter().map(|b| b.block).collect::<Vec<_>>(),
            blocks[0..6]
                .iter()
                .map(
                    |b| Unverified::from_rlp(b.to_vec(), client.spec.params().eip1559_transition)
                        .unwrap()
                )
                .collect::<Vec<_>>()
        );
        assert!(!bc.contains(&hashes[0]));
        assert_eq!(hashes[5], bc.head.unwrap());

        let (h, _) = bc.needed_headers(6, false).unwrap();
        assert_eq!(hashes[5], h);
        let (h, _) = bc.needed_headers(6, false).unwrap();
        assert_eq!(hashes[20], h);
        bc.insert_headers(headers[10..16].into_iter().map(Clone::clone).collect());
        assert!(bc.drain().is_empty());
        bc.insert_headers(headers[5..10].into_iter().map(Clone::clone).collect());
        assert_eq!(
            bc.drain().into_iter().map(|b| b.block).collect::<Vec<_>>(),
            blocks[6..16]
                .iter()
                .map(
                    |b| Unverified::from_rlp(b.to_vec(), client.spec.params().eip1559_transition)
                        .unwrap()
                )
                .collect::<Vec<_>>()
        );

        assert_eq!(hashes[15], bc.heads[0]);

        bc.insert_headers(headers[15..].into_iter().map(Clone::clone).collect());
        bc.drain();
        assert!(bc.is_empty());
    }

    #[test]
    fn insert_headers_with_gap() {
        let mut bc = BlockCollection::new(false);
        assert!(is_empty(&bc));
        let client = TestBlockChainClient::new();
        let nblocks = 200;
        client.add_blocks(nblocks, EachBlockWith::Nothing);
        let blocks: Vec<_> = (0..nblocks)
            .map(|i| {
                (&client as &dyn BlockChainClient)
                    .block(BlockId::Number(i as BlockNumber))
                    .unwrap()
                    .into_inner()
            })
            .collect();
        let headers: Vec<_> = blocks
            .iter()
            .map(|b| {
                SyncHeader::from_rlp(
                    Rlp::new(b).at(0).unwrap().as_raw().to_vec(),
                    client.spec.params().eip1559_transition,
                )
                .unwrap()
            })
            .collect();
        let hashes: Vec<_> = headers.iter().map(|h| h.header.hash()).collect();
        let heads: Vec<_> = hashes
            .iter()
            .enumerate()
            .filter_map(|(i, h)| if i % 20 == 0 { Some(*h) } else { None })
            .collect();
        bc.reset_to(heads);

        bc.insert_headers(headers[2..22].into_iter().map(Clone::clone).collect());
        assert_eq!(hashes[0], bc.heads[0]);
        assert_eq!(hashes[21], bc.heads[1]);
        assert!(bc.head.is_none());
        bc.insert_headers(headers[0..2].into_iter().map(Clone::clone).collect());
        assert!(bc.head.is_some());
        assert_eq!(hashes[21], bc.heads[0]);
    }

    #[test]
    fn insert_headers_no_gap() {
        let mut bc = BlockCollection::new(false);
        assert!(is_empty(&bc));
        let client = TestBlockChainClient::new();
        let nblocks = 200;
        client.add_blocks(nblocks, EachBlockWith::Nothing);
        let blocks: Vec<_> = (0..nblocks)
            .map(|i| {
                (&client as &dyn BlockChainClient)
                    .block(BlockId::Number(i as BlockNumber))
                    .unwrap()
                    .into_inner()
            })
            .collect();
        let headers: Vec<_> = blocks
            .iter()
            .map(|b| {
                SyncHeader::from_rlp(
                    Rlp::new(b).at(0).unwrap().as_raw().to_vec(),
                    client.spec.params().eip1559_transition,
                )
                .unwrap()
            })
            .collect();
        let hashes: Vec<_> = headers.iter().map(|h| h.header.hash()).collect();
        let heads: Vec<_> = hashes
            .iter()
            .enumerate()
            .filter_map(|(i, h)| if i % 20 == 0 { Some(*h) } else { None })
            .collect();
        bc.reset_to(heads);

        bc.insert_headers(headers[1..2].into_iter().map(Clone::clone).collect());
        assert!(bc.drain().is_empty());
        bc.insert_headers(headers[0..1].into_iter().map(Clone::clone).collect());
        assert_eq!(bc.drain().len(), 2);
    }
}
