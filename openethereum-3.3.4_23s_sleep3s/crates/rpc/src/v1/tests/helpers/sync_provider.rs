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

//! Test implementation of SyncProvider.

use ethereum_types::{H256, H512};
use network::client_version::ClientVersion;
use parking_lot::RwLock;
use stats::{PrometheusMetrics, PrometheusRegistry};
use std::collections::BTreeMap;
use sync::{EthProtocolInfo, PeerInfo, SyncProvider, SyncState, SyncStatus, TransactionStats};

/// TestSyncProvider config.
pub struct Config {
    /// Protocol version.
    pub network_id: u64,
    /// Number of peers.
    pub num_peers: usize,
}

/// Test sync provider.
pub struct TestSyncProvider {
    /// Sync status.
    pub status: RwLock<SyncStatus>,
}

impl TestSyncProvider {
    /// Creates new sync provider.
    pub fn new(config: Config) -> Self {
        TestSyncProvider {
            status: RwLock::new(SyncStatus {
                state: SyncState::Idle,
                network_id: config.network_id,
                protocol_version: 64,
                start_block_number: 0,
                last_imported_block_number: None,
                highest_block_number: None,
                blocks_total: 0,
                blocks_received: 0,
                num_peers: config.num_peers,
                num_active_peers: 0,
                num_snapshot_chunks: 0,
                snapshot_chunks_done: 0,
                last_imported_old_block_number: None,
                item_sizes: BTreeMap::new(),
            }),
        }
    }

    /// Simulate importing blocks.
    pub fn increase_imported_block_number(&self, count: u64) {
        let mut status = self.status.write();
        let current_number = status.last_imported_block_number.unwrap_or(0);
        status.last_imported_block_number = Some(current_number + count);
    }
}

impl PrometheusMetrics for TestSyncProvider {
    fn prometheus_metrics(&self, _: &mut PrometheusRegistry) {}
}

impl SyncProvider for TestSyncProvider {
    fn status(&self) -> SyncStatus {
        self.status.read().clone()
    }

    fn peers(&self) -> Vec<PeerInfo> {
        vec![
            PeerInfo {
                id: Some("node1".to_owned()),
                client_version: ClientVersion::from("Parity-Ethereum/1/v2.4.0/linux/rustc"),
                capabilities: vec!["eth/63".to_owned(), "eth/64".to_owned()],
                remote_address: "127.0.0.1:7777".to_owned(),
                local_address: "127.0.0.1:8888".to_owned(),
                eth_info: Some(EthProtocolInfo {
                    version: 63,
                    difficulty: Some(40.into()),
                    head: H256::from_low_u64_be(50),
                }),
            },
            PeerInfo {
                id: None,
                client_version: ClientVersion::from("Open-Ethereum/2/v2.4.0/linux/rustc"),
                capabilities: vec!["eth/64".to_owned(), "eth/65".to_owned()],
                remote_address: "Handshake".to_owned(),
                local_address: "127.0.0.1:3333".to_owned(),
                eth_info: Some(EthProtocolInfo {
                    version: 65,
                    difficulty: None,
                    head: H256::from_low_u64_be(60),
                }),
            },
        ]
    }

    fn enode(&self) -> Option<String> {
        None
    }

    fn pending_transactions_stats(&self) -> BTreeMap<H256, TransactionStats> {
        map![
            H256::from_low_u64_be(1) => TransactionStats {
                first_seen: 10,
                propagated_to: map![
                    H512::from_low_u64_be(128) => 16
                ],
            },
            H256::from_low_u64_be(5) => TransactionStats {
                first_seen: 16,
                propagated_to: map![
                    H512::from_low_u64_be(16) => 1
                ],
            }
        ]
    }

    fn new_transactions_stats(&self) -> BTreeMap<H256, TransactionStats> {
        map![
            H256::from_low_u64_be(1) => TransactionStats {
                first_seen: 10,
                propagated_to: map![
                    H512::from_low_u64_be(128) => 2
                ],
            }
        ]
    }
}
