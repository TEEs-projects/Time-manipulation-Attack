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

use std::{cmp, collections::HashSet};

use bytes::Bytes;
use ethereum_types::H256;
use fastmap::H256FastSet;
use network::{client_version::ClientCapabilities, PeerId};
use rand::RngCore;
use rlp::RlpStream;
use sync_io::SyncIo;
use types::{blockchain_info::BlockChainInfo, transaction::SignedTransaction, BlockNumber};

use super::sync_packet::SyncPacket::{self, *};

use super::{
    random, ChainSync, ETH_PROTOCOL_VERSION_65, MAX_PEERS_PROPAGATION, MAX_PEER_LAG_PROPAGATION,
    MAX_TRANSACTION_PACKET_SIZE, MIN_PEERS_PROPAGATION,
};
use ethcore_miner::pool::VerifiedTransaction;
use std::sync::Arc;

const NEW_POOLED_HASHES_LIMIT: usize = 4096;

/// The Chain Sync Propagator: propagates data to peers
pub struct SyncPropagator;

impl SyncPropagator {
    // t_nb 11.4.3 propagates latest block to a set of peers
    pub fn propagate_blocks(
        sync: &mut ChainSync,
        chain_info: &BlockChainInfo,
        io: &mut dyn SyncIo,
        blocks: &[H256],
        peers: &[PeerId],
    ) -> usize {
        trace!(target: "sync", "Sending NewBlocks to {:?}", peers);
        let sent = peers.len();
        let mut send_packet = |io: &mut dyn SyncIo, rlp: Bytes| {
            for peer_id in peers {
                SyncPropagator::send_packet(io, *peer_id, NewBlockPacket, rlp.clone());

                if let Some(ref mut peer) = sync.peers.get_mut(peer_id) {
                    peer.latest_hash = chain_info.best_block_hash.clone();
                }
            }
        };

        if blocks.is_empty() {
            let rlp = ChainSync::create_latest_block_rlp(io.chain());
            send_packet(io, rlp);
        } else {
            for h in blocks {
                let rlp = ChainSync::create_new_block_rlp(io.chain(), h);
                send_packet(io, rlp);
            }
        }

        sent
    }

    // t_nb 11.4.2 propagates new known hashes to all peers
    pub fn propagate_new_hashes(
        sync: &mut ChainSync,
        chain_info: &BlockChainInfo,
        io: &mut dyn SyncIo,
        peers: &[PeerId],
    ) -> usize {
        trace!(target: "sync", "Sending NewHashes to {:?}", peers);
        let last_parent = *io.chain().best_block_header().parent_hash();
        let best_block_hash = chain_info.best_block_hash;
        let rlp = match ChainSync::create_new_hashes_rlp(io.chain(), &last_parent, &best_block_hash)
        {
            Some(rlp) => rlp,
            None => return 0,
        };

        let sent = peers.len();
        for peer_id in peers {
            if let Some(ref mut peer) = sync.peers.get_mut(peer_id) {
                peer.latest_hash = best_block_hash;
            }
            SyncPropagator::send_packet(io, *peer_id, NewBlockHashesPacket, rlp.clone());
        }
        sent
    }

    /// propagates new transactions to all peers
    pub fn propagate_new_transactions<F: FnMut() -> bool>(
        sync: &mut ChainSync,
        io: &mut dyn SyncIo,
        tx_hashes: Vec<H256>,
        should_continue: F,
    ) -> usize {
        let transactions = move |io: &dyn SyncIo| {
            tx_hashes
                .iter()
                .filter_map(|hash| io.chain().transaction(hash))
                .collect()
        };
        SyncPropagator::propagate_transactions(sync, io, transactions, true, should_continue)
    }

    pub fn propagate_ready_transactions<F: FnMut() -> bool>(
        sync: &mut ChainSync,
        io: &mut dyn SyncIo,
        should_continue: F,
    ) -> usize {
        let transactions = |io: &dyn SyncIo| io.chain().transactions_to_propagate();
        SyncPropagator::propagate_transactions(sync, io, transactions, false, should_continue)
    }

    fn propagate_transactions_to_peers<F: FnMut() -> bool>(
        sync: &mut ChainSync,
        io: &mut dyn SyncIo,
        peers: Vec<PeerId>,
        transactions: Vec<&SignedTransaction>,
        are_new: bool,
        mut should_continue: F,
    ) -> HashSet<PeerId> {
        let all_transactions_hashes = transactions
            .iter()
            .map(|tx| tx.hash())
            .collect::<H256FastSet>();
        let all_transactions_rlp = {
            let mut packet = RlpStream::new_list(transactions.len());
            for tx in &transactions {
                tx.rlp_append(&mut packet);
            }
            packet.out()
        };
        let all_transactions_hashes_rlp =
            rlp::encode_list(&all_transactions_hashes.iter().copied().collect::<Vec<_>>());

        let block_number = io.chain().chain_info().best_block_number;

        if are_new {
            sync.transactions_stats
                .retain_new(block_number, sync.new_transactions_stats_period);
        } else {
            sync.transactions_stats
                .retain_pending(&all_transactions_hashes);
        }

        let send_packet = |io: &mut dyn SyncIo,
                           peer_id: PeerId,
                           is_hashes: bool,
                           sent: usize,
                           rlp: Bytes| {
            let size = rlp.len();
            SyncPropagator::send_packet(
                io,
                peer_id,
                if is_hashes {
                    NewPooledTransactionHashesPacket
                } else {
                    TransactionsPacket
                },
                rlp,
            );
            trace!(target: "sync", "{:02} <- {} ({} entries; {} bytes)", peer_id, if is_hashes { "NewPooledTransactionHashes" } else { "Transactions" }, sent, size);
        };

        let mut sent_to_peers = HashSet::new();
        let mut max_sent = 0;

        // for every peer construct and send transactions packet
        for peer_id in peers {
            if !should_continue() {
                debug!(target: "sync", "Sent up to {} transactions to {} peers.", max_sent, sent_to_peers.len());
                return sent_to_peers;
            }

            let stats = &mut sync.transactions_stats;
            let peer_info = sync.peers.get_mut(&peer_id)
				.expect("peer_id is form peers; peers is result of select_peers_for_transactions; select_peers_for_transactions selects peers from self.peers; qed");

            let is_hashes = peer_info.protocol_version >= ETH_PROTOCOL_VERSION_65.0;

            // Send all transactions, if the peer doesn't know about anything
            if peer_info.last_sent_transactions.is_empty() {
                // update stats
                for hash in &all_transactions_hashes {
                    let id = io.peer_session_info(peer_id).and_then(|info| info.id);
                    stats.propagated(hash, are_new, id, block_number);
                }
                peer_info.last_sent_transactions = all_transactions_hashes.clone();

                let rlp = {
                    if is_hashes {
                        all_transactions_hashes_rlp.clone()
                    } else {
                        all_transactions_rlp.clone()
                    }
                };
                send_packet(io, peer_id, is_hashes, all_transactions_hashes.len(), rlp);
                sent_to_peers.insert(peer_id);
                max_sent = cmp::max(max_sent, all_transactions_hashes.len());
                continue;
            }

            // Get hashes of all transactions to send to this peer
            let to_send = all_transactions_hashes
                .difference(&peer_info.last_sent_transactions)
                .cloned()
                .collect::<HashSet<_>>();
            if to_send.is_empty() {
                continue;
            }

            // Construct RLP
            let (packet, to_send) = {
                let mut to_send_new = HashSet::new();
                let mut packet = RlpStream::new();
                packet.begin_unbounded_list();
                for tx in &transactions {
                    let hash = tx.hash();
                    if to_send.contains(&hash) {
                        if is_hashes {
                            if to_send_new.len() >= NEW_POOLED_HASHES_LIMIT {
                                debug!(target: "sync", "NewPooledTransactionHashes length limit reached. Sending incomplete list of {}/{} transactions.", to_send_new.len(), to_send.len());
                                break;
                            }
                            packet.append(&hash);
                            to_send_new.insert(hash);
                        } else {
                            tx.rlp_append(&mut packet);
                            to_send_new.insert(hash);
                            // this is not hard limit and we are okay with it. Max default tx size is 300k.
                            if packet.as_raw().len() >= MAX_TRANSACTION_PACKET_SIZE {
                                // Maximal packet size reached just proceed with sending
                                debug!(target: "sync", "Transaction packet size limit reached. Sending incomplete set of {}/{} transactions.", to_send_new.len(), to_send.len());
                                break;
                            }
                        }
                    }
                }
                packet.finalize_unbounded_list();
                (packet, to_send_new)
            };

            // Update stats.
            let id = io.peer_session_info(peer_id).and_then(|info| info.id);
            for hash in &to_send {
                stats.propagated(hash, are_new, id, block_number);
            }

            peer_info.last_sent_transactions = all_transactions_hashes
                .intersection(&peer_info.last_sent_transactions)
                .chain(&to_send)
                .cloned()
                .collect();
            send_packet(io, peer_id, is_hashes, to_send.len(), packet.out());
            sent_to_peers.insert(peer_id);
            max_sent = cmp::max(max_sent, to_send.len());
        }

        debug!(target: "sync", "Sent up to {} transactions to {} peers.", max_sent, sent_to_peers.len());
        sent_to_peers
    }

    // t_nb 11.4.1 propagate latest blocks to peers
    pub fn propagate_latest_blocks(sync: &mut ChainSync, io: &mut dyn SyncIo, sealed: &[H256]) {
        let chain_info = io.chain().chain_info();
        if (((chain_info.best_block_number as i64) - (sync.last_sent_block_number as i64)).abs()
            as BlockNumber)
            < MAX_PEER_LAG_PROPAGATION
        {
            let peers = sync.get_lagging_peers(&chain_info);
            if sealed.is_empty() {
                // t_nb 11.4.2
                let hashes = SyncPropagator::propagate_new_hashes(sync, &chain_info, io, &peers);
                let peers = ChainSync::select_random_peers(&peers);
                // t_nb 11.4.3
                let blocks =
                    SyncPropagator::propagate_blocks(sync, &chain_info, io, sealed, &peers);
                if blocks != 0 || hashes != 0 {
                    trace!(target: "sync", "Sent latest {} blocks and {} hashes to peers.", blocks, hashes);
                }
            } else {
                // t_nb 11.4.3
                SyncPropagator::propagate_blocks(sync, &chain_info, io, sealed, &peers);
                // t_nb 11.4.2
                SyncPropagator::propagate_new_hashes(sync, &chain_info, io, &peers);
                trace!(target: "sync", "Sent sealed block to all peers");
            };
        }
        sync.last_sent_block_number = chain_info.best_block_number;
    }

    // t_nb 11.4.4 Distribute valid proposed blocks to subset of current peers. (if there is any proposed)
    pub fn propagate_proposed_blocks(
        sync: &mut ChainSync,
        io: &mut dyn SyncIo,
        proposed: &[Bytes],
    ) {
        let peers = sync.get_consensus_peers();
        trace!(target: "sync", "Sending proposed blocks to {:?}", peers);
        for block in proposed {
            let rlp = ChainSync::create_block_rlp(block, io.chain().chain_info().total_difficulty);
            for peer_id in &peers {
                SyncPropagator::send_packet(io, *peer_id, NewBlockPacket, rlp.clone());
            }
        }
    }

    /// Broadcast consensus message to peers.
    pub fn propagate_consensus_packet(sync: &mut ChainSync, io: &mut dyn SyncIo, packet: Bytes) {
        let lucky_peers = ChainSync::select_random_peers(&sync.get_consensus_peers());
        trace!(target: "sync", "Sending consensus packet to {:?}", lucky_peers);
        for peer_id in lucky_peers {
            SyncPropagator::send_packet(io, peer_id, ConsensusDataPacket, packet.clone());
        }
    }

    fn select_peers_for_transactions<F>(sync: &ChainSync, filter: F, are_new: bool) -> Vec<PeerId>
    where
        F: Fn(&PeerId) -> bool,
    {
        let fraction_filter: Box<dyn FnMut(&PeerId) -> bool> = if are_new {
            // We propagate new transactions to all peers initially.
            Box::new(|_| true)
        } else {
            // Otherwise, we propagate transaction only to squire root of all peers.
            let mut random = random::new();
            // sqrt(x)/x scaled to max u32
            let fraction =
                ((sync.peers.len() as f64).powf(-0.5) * (u32::max_value() as f64).round()) as u32;
            let small = sync.peers.len() < MIN_PEERS_PROPAGATION;
            Box::new(move |_| small || random.next_u32() < fraction)
        };

        sync.peers
            .keys()
            .cloned()
            .filter(filter)
            .filter(fraction_filter)
            .take(MAX_PEERS_PROPAGATION)
            .collect()
    }

    /// Generic packet sender
    pub fn send_packet(
        sync: &mut dyn SyncIo,
        peer_id: PeerId,
        packet_id: SyncPacket,
        packet: Bytes,
    ) {
        if let Err(e) = sync.send(peer_id, packet_id, packet) {
            debug!(target:"sync", "Error sending packet: {:?}", e);
            sync.disconnect_peer(peer_id);
        }
    }

    /// propagates new transactions to all peers
    fn propagate_transactions<'a, F, G>(
        sync: &mut ChainSync,
        io: &mut dyn SyncIo,
        get_transactions: G,
        are_new: bool,
        mut should_continue: F,
    ) -> usize
    where
        F: FnMut() -> bool,
        G: Fn(&dyn SyncIo) -> Vec<Arc<VerifiedTransaction>>,
    {
        // Early out if nobody to send to.
        if sync.peers.is_empty() {
            return 0;
        }

        let transactions = get_transactions(io);
        if transactions.is_empty() {
            return 0;
        }

        if !should_continue() {
            return 0;
        }

        let (transactions, service_transactions): (Vec<_>, Vec<_>) = transactions
            .iter()
            .map(|tx| tx.signed())
            .partition(|tx| !tx.tx().gas_price.is_zero());

        // usual transactions could be propagated to all peers
        let mut affected_peers = HashSet::new();
        if !transactions.is_empty() {
            let peers = SyncPropagator::select_peers_for_transactions(sync, |_| true, are_new);
            affected_peers = SyncPropagator::propagate_transactions_to_peers(
                sync,
                io,
                peers,
                transactions,
                are_new,
                &mut should_continue,
            );
        }

        // most of times service_transactions will be empty
        // => there's no need to merge packets
        if !service_transactions.is_empty() {
            let service_transactions_peers = SyncPropagator::select_peers_for_transactions(
                sync,
                |peer_id| io.peer_version(*peer_id).accepts_service_transaction(),
                are_new,
            );
            let service_transactions_affected_peers =
                SyncPropagator::propagate_transactions_to_peers(
                    sync,
                    io,
                    service_transactions_peers,
                    service_transactions,
                    are_new,
                    &mut should_continue,
                );
            affected_peers.extend(&service_transactions_affected_peers);
        }

        affected_peers.len()
    }
}

#[cfg(test)]
mod tests {
    use ethcore::client::{BlockInfo, ChainInfo, EachBlockWith, TestBlockChainClient};
    use parking_lot::RwLock;
    use rlp::Rlp;
    use std::collections::VecDeque;
    use tests::{helpers::TestIo, snapshot::TestSnapshotService};
    use types::transaction::TypedTransaction;

    use super::{
        super::{tests::*, *},
        *,
    };
    use ethcore::ethereum::new_london_test;

    #[test]
    fn sends_new_hashes_to_lagging_peer() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        let queue = RwLock::new(VecDeque::new());
        let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(5), &client);
        let chain_info = client.chain_info();
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);

        let peers = sync.get_lagging_peers(&chain_info);
        let peer_count =
            SyncPropagator::propagate_new_hashes(&mut sync, &chain_info, &mut io, &peers);

        // 1 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should be updated
        assert_eq!(1, peer_count);
        // NEW_BLOCK_HASHES_PACKET
        assert_eq!(0x01, io.packets[0].packet_id);
    }

    #[test]
    fn sends_latest_block_to_lagging_peer() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        let queue = RwLock::new(VecDeque::new());
        let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(5), &client);
        let chain_info = client.chain_info();
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peers = sync.get_lagging_peers(&chain_info);
        let peer_count =
            SyncPropagator::propagate_blocks(&mut sync, &chain_info, &mut io, &[], &peers);

        // 1 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should be updated
        assert_eq!(1, peer_count);
        // NEW_BLOCK_PACKET
        assert_eq!(0x07, io.packets[0].packet_id);
    }

    #[test]
    fn sends_sealed_block() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        let queue = RwLock::new(VecDeque::new());
        let hash = client.block_hash(BlockId::Number(99)).unwrap();
        let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(5), &client);
        let chain_info = client.chain_info();
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peers = sync.get_lagging_peers(&chain_info);
        let peer_count = SyncPropagator::propagate_blocks(
            &mut sync,
            &chain_info,
            &mut io,
            &[hash.clone()],
            &peers,
        );

        // 1 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should be updated
        assert_eq!(1, peer_count);
        // NEW_BLOCK_PACKET
        assert_eq!(0x07, io.packets[0].packet_id);
    }

    #[test]
    fn sends_proposed_block() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(2, EachBlockWith::Uncle);
        let queue = RwLock::new(VecDeque::new());
        let block = client.block(BlockId::Latest).unwrap().into_inner();
        let mut sync = dummy_sync(&client);
        sync.peers.insert(
            0,
            PeerInfo {
                // Messaging protocol
                protocol_version: 2,
                genesis: H256::zero(),
                network_id: 0,
                latest_hash: client.block_hash_delta_minus(1),
                difficulty: None,
                asking: PeerAsking::Nothing,
                asking_blocks: Vec::new(),
                asking_hash: None,
                unfetched_pooled_transactions: Default::default(),
                asking_pooled_transactions: Default::default(),
                ask_time: Instant::now(),
                last_sent_transactions: Default::default(),
                expired: false,
                confirmation: ForkConfirmation::Confirmed,
                snapshot_number: None,
                snapshot_hash: None,
                asking_snapshot_data: None,
                block_set: None,
                client_version: ClientVersion::from(""),
            },
        );
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        SyncPropagator::propagate_proposed_blocks(&mut sync, &mut io, &[block]);

        // 1 message should be sent
        assert_eq!(1, io.packets.len());
        // NEW_BLOCK_PACKET
        assert_eq!(0x07, io.packets[0].packet_id);
    }

    #[test]
    fn propagates_ready_transactions() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
        // Try to propagate same transactions for the second time
        let peer_count2 = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
        // Even after new block transactions should not be propagated twice
        sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);
        // Try to propagate same transactions for the third time
        let peer_count3 = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);

        // 1 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should be updated but only once
        assert_eq!(1, peer_count);
        assert_eq!(0, peer_count2);
        assert_eq!(0, peer_count3);
        // TRANSACTIONS_PACKET
        assert_eq!(0x02, io.packets[0].packet_id);
    }

    #[test]
    fn propagates_ready_transactions_to_subset_of_peers() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        client.insert_transaction_to_queue();
        let mut sync = dummy_sync(&client);
        for id in 0..25 {
            insert_dummy_peer(&mut sync, id, client.block_hash_delta_minus(1))
        }
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);

        // Currently random implementation for test returns 8 peers as result of peers selection.
        assert_eq!(8, peer_count);
    }

    #[test]
    fn propagates_new_transactions_to_all_peers() {
        let (new_transaction_hashes_tx, new_transaction_hashes_rx) = crossbeam_channel::unbounded();

        let mut client = TestBlockChainClient::new();
        client.set_new_transaction_hashes_producer(new_transaction_hashes_tx);
        client.add_blocks(100, EachBlockWith::Uncle);
        let tx_hash = client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_tx_hashes_rx(&client, new_transaction_hashes_rx);
        for id in 0..25 {
            insert_dummy_peer(&mut sync, id, client.block_hash_delta_minus(1))
        }
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);

        assert_eq!(25, peer_count);
    }

    #[test]
    fn propagates_new_transactions() {
        let (new_transaction_hashes_tx, new_transaction_hashes_rx) = crossbeam_channel::unbounded();

        let mut client = TestBlockChainClient::new();
        client.set_new_transaction_hashes_producer(new_transaction_hashes_tx);
        client.add_blocks(100, EachBlockWith::Uncle);
        let tx_hash = client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer_and_tx_hashes_rx(
            client.block_hash_delta_minus(1),
            &client,
            new_transaction_hashes_rx,
        );
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);
        // Try to propagate same transactions for the second time
        let peer_count2 =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);
        // Even after new block transactions should not be propagated twice
        sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);
        // Try to propagate same transactions for the third time
        let peer_count3 =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);

        // 1 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should be updated but only once
        assert_eq!(1, peer_count);
        assert_eq!(0, peer_count2);
        assert_eq!(0, peer_count3);
        // TRANSACTIONS_PACKET
        assert_eq!(0x02, io.packets[0].packet_id);
    }

    #[test]
    fn does_not_propagate_ready_transactions_after_new_block() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
        io.chain.insert_transaction_to_queue();
        // New block import should not trigger propagation.
        // (we only propagate on timeout)
        sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);

        // 2 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should receive the message
        assert_eq!(1, peer_count);
        // TRANSACTIONS_PACKET
        assert_eq!(0x02, io.packets[0].packet_id);
    }

    #[test]
    fn does_not_propagate_new_transactions_after_new_block() {
        let (new_transaction_hashes_tx, new_transaction_hashes_rx) = crossbeam_channel::unbounded();

        let mut client = TestBlockChainClient::new();
        client.set_new_transaction_hashes_producer(new_transaction_hashes_tx);
        client.add_blocks(100, EachBlockWith::Uncle);
        let tx_hash = client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer_and_tx_hashes_rx(
            client.block_hash_delta_minus(1),
            &client,
            new_transaction_hashes_rx,
        );
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);
        io.chain.insert_transaction_to_queue();
        // New block import should not trigger propagation.
        // (we only propagate on timeout)
        sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);

        // 2 message should be send
        assert_eq!(1, io.packets.len());
        // 1 peer should receive the message
        assert_eq!(1, peer_count);
        // TRANSACTIONS_PACKET
        assert_eq!(0x02, io.packets[0].packet_id);
    }

    #[test]
    fn does_not_fail_for_no_peers() {
        let (new_transaction_hashes_tx, new_transaction_hashes_rx) = crossbeam_channel::unbounded();

        let mut client = TestBlockChainClient::new();
        client.set_new_transaction_hashes_producer(new_transaction_hashes_tx);
        client.add_blocks(100, EachBlockWith::Uncle);
        let tx_hash = client.insert_transaction_to_queue();
        // Sync with no peers
        let mut sync = dummy_sync_with_tx_hashes_rx(&client, new_transaction_hashes_rx);
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
        let peer_count_new =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);
        sync.chain_new_blocks(&mut io, &[], &[], &[], &[], &[], &[]);
        // Try to propagate same transactions for the second time
        let peer_count2 = SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
        let peer_count_new2 =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);

        assert_eq!(0, io.packets.len());
        assert_eq!(0, peer_count);
        assert_eq!(0, peer_count2);
        assert_eq!(0, peer_count_new);
        assert_eq!(0, peer_count_new2);
    }

    #[test]
    fn propagates_transactions_without_alternating() {
        let mut client = TestBlockChainClient::new();
        client.add_blocks(100, EachBlockWith::Uncle);
        client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer(client.block_hash_delta_minus(1), &client);
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        // should sent some
        {
            let mut io = TestIo::new(&mut client, &ss, &queue, None);
            let peer_count =
                SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
            assert_eq!(1, io.packets.len());
            assert_eq!(1, peer_count);
        }
        // Insert some more
        client.insert_transaction_to_queue();
        let (peer_count2, peer_count3) = {
            let mut io = TestIo::new(&mut client, &ss, &queue, None);
            // Propagate new transactions
            let peer_count2 =
                SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
            // And now the peer should have all transactions
            let peer_count3 =
                SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
            (peer_count2, peer_count3)
        };

        // 2 message should be send (in total)
        assert_eq!(2, queue.read().len());
        // 1 peer should be updated but only once after inserting new transaction
        assert_eq!(1, peer_count2);
        assert_eq!(0, peer_count3);
        // TRANSACTIONS_PACKET
        assert_eq!(0x02, queue.read()[0].packet_id);
        assert_eq!(0x02, queue.read()[1].packet_id);
    }

    #[test]
    fn should_maintain_transactions_propagation_stats() {
        let (new_transaction_hashes_tx, new_transaction_hashes_rx) = crossbeam_channel::unbounded();

        let mut client = TestBlockChainClient::new();
        client.set_new_transaction_hashes_producer(new_transaction_hashes_tx);
        client.add_blocks(100, EachBlockWith::Uncle);
        let tx_hash1 = client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer_and_tx_hashes_rx(
            client.block_hash_delta_minus(1),
            &client,
            new_transaction_hashes_rx,
        );
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();

        {
            let mut io = TestIo::new(&mut client, &ss, &queue, None);
            SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);
        }

        let tx_hash2 = client.insert_transaction_to_queue();
        {
            let mut io = TestIo::new(&mut client, &ss, &queue, None);
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash2], || true);
        }

        let stats = sync.pending_transactions_stats();
        assert_eq!(
            stats.len(),
            1,
            "Should maintain stats for single ready transaction."
        );
        assert!(
            stats.contains_key(&tx_hash1),
            "Should maintain stats for propagated ready transaction."
        );

        let stats = sync.new_transactions_stats();
        assert_eq!(
            stats.len(),
            1,
            "Should maintain stats for single new transaction."
        );
        assert!(
            stats.contains_key(&tx_hash2),
            "Should maintain stats for propagated new transaction."
        );
    }

    #[test]
    fn should_propagate_service_transaction_to_selected_peers_only() {
        let mut client = TestBlockChainClient::new();
        client.insert_transaction_with_gas_price_to_queue(U256::zero());
        let block_hash = client.block_hash_delta_minus(1);
        let mut sync = dummy_sync(&client);
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);

        // when peer#1 is Geth
        insert_dummy_peer(&mut sync, 1, block_hash);
        io.peers_info.insert(1, "Geth".to_owned());
        // and peer#2 is OpenEthereum, accepting service transactions
        insert_dummy_peer(&mut sync, 2, block_hash);
        io.peers_info
            .insert(2, "OpenEthereum/v2.6.0/linux/rustc".to_owned());
        // and peer#3 is OpenEthereum, accepting service transactions
        insert_dummy_peer(&mut sync, 3, block_hash);
        io.peers_info
            .insert(3, "OpenEthereum/ABCDEFGH/v2.7.3/linux/rustc".to_owned());

        // and new service transaction is propagated to peers
        SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);

        // peer#2 && peer#3 are receiving service transaction
        assert!(io
            .packets
            .iter()
            .any(|p| p.packet_id == 0x02 && p.recipient == 2)); // TRANSACTIONS_PACKET
        assert!(io
            .packets
            .iter()
            .any(|p| p.packet_id == 0x02 && p.recipient == 3)); // TRANSACTIONS_PACKET
        assert_eq!(io.packets.len(), 2);
    }

    #[test]
    fn should_propagate_service_transaction_is_sent_as_separate_message() {
        let mut client = TestBlockChainClient::new();
        let tx1_hash = client.insert_transaction_to_queue();
        let tx2_hash = client.insert_transaction_with_gas_price_to_queue(U256::zero());
        let block_hash = client.block_hash_delta_minus(1);
        let mut sync = dummy_sync(&client);
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();
        let mut io = TestIo::new(&mut client, &ss, &queue, None);

        // when peer#1 is OpenEthereum, accepting service transactions
        insert_dummy_peer(&mut sync, 1, block_hash);
        io.peers_info
            .insert(1, "OpenEthereum/v2.6.0/linux/rustc".to_owned());

        // and service + non-service transactions are propagated to peers
        SyncPropagator::propagate_ready_transactions(&mut sync, &mut io, || true);

        // two separate packets for peer are queued:
        // 1) with non-service-transaction
        // 2) with service transaction
        let sent_transactions: Vec<UnverifiedTransaction> = io
            .packets
            .iter()
            .filter_map(|p| {
                if p.packet_id != 0x02 || p.recipient != 1 {
                    // TRANSACTIONS_PACKET
                    return None;
                }

                let rlp = Rlp::new(&*p.data);
                let item_count = rlp.item_count().unwrap_or(0);
                if item_count != 1 {
                    return None;
                }

                rlp.at(0)
                    .ok()
                    .and_then(|r| TypedTransaction::decode_rlp(&r).ok())
            })
            .collect();
        assert_eq!(sent_transactions.len(), 2);
        assert!(sent_transactions.iter().any(|tx| tx.hash() == tx1_hash));
        assert!(sent_transactions.iter().any(|tx| tx.hash() == tx2_hash));
    }

    #[test]
    fn should_propagate_transactions_with_max_fee_per_gas_lower_than_base_fee() {
        let (new_transaction_hashes_tx, new_transaction_hashes_rx) = crossbeam_channel::unbounded();

        let spec = new_london_test();
        let mut client = TestBlockChainClient::new_with_spec(spec);
        client.set_new_transaction_hashes_producer(new_transaction_hashes_tx);
        client.add_blocks(100, EachBlockWith::Uncle);
        let tx_hash = client.insert_transaction_to_queue();
        let mut sync = dummy_sync_with_peer_and_tx_hashes_rx(
            client.block_hash_delta_minus(1),
            &client,
            new_transaction_hashes_rx,
        );
        let queue = RwLock::new(VecDeque::new());
        let ss = TestSnapshotService::new();

        let mut io = TestIo::new(&mut client, &ss, &queue, None);
        let peer_count =
            SyncPropagator::propagate_new_transactions(&mut sync, &mut io, vec![tx_hash], || true);

        assert_eq!(1, io.packets.len());
        assert_eq!(1, peer_count);
    }
}
