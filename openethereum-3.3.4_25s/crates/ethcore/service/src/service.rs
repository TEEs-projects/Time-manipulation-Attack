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

//! Creates and registers client and network services.

use std::{path::Path, sync::Arc, time::Duration};

use ansi_term::Colour;
use io::{IoContext, IoError, IoHandler, IoService, TimerToken};
use stop_guard::StopGuard;

use blockchain::{BlockChainDB, BlockChainDBHandler};
use ethcore::{
    client::{ChainNotify, Client, ClientConfig, ClientIoMessage},
    error::{Error as EthcoreError, ErrorKind},
    miner::Miner,
    snapshot::{
        service::{Service as SnapshotService, ServiceParams as SnapServiceParams},
        Error as SnapshotError, RestorationStatus, SnapshotService as _SnapshotService,
    },
    spec::Spec,
};

use Error;

/// Client service setup. Creates and registers client and network services with the IO subsystem.
pub struct ClientService {
    io_service: Arc<IoService<ClientIoMessage>>,
    client: Arc<Client>,
    snapshot: Arc<SnapshotService>,
    database: Arc<dyn BlockChainDB>,
    _stop_guard: StopGuard,
}

impl ClientService {
    /// Start the `ClientService`.
    pub fn start(
        config: ClientConfig,
        spec: &Spec,
        blockchain_db: Arc<dyn BlockChainDB>,
        snapshot_path: &Path,
        restoration_db_handler: Box<dyn BlockChainDBHandler>,
        _ipc_path: &Path,
        miner: Arc<Miner>,
    ) -> Result<ClientService, Error> {
        let io_service = IoService::<ClientIoMessage>::start("Client")?;

        info!(
            "Configured for {} using {} engine",
            Colour::White.bold().paint(spec.name.clone()),
            Colour::Yellow.bold().paint(spec.engine.name())
        );

        let pruning = config.pruning;
        let client = Client::new(
            config,
            &spec,
            blockchain_db.clone(),
            miner.clone(),
            io_service.channel(),
        )?;
        miner.set_io_channel(io_service.channel());
        miner.set_in_chain_checker(&client.clone());

        let snapshot_params = SnapServiceParams {
            engine: spec.engine.clone(),
            genesis_block: spec.genesis_block(),
            restoration_db_handler: restoration_db_handler,
            pruning: pruning,
            channel: io_service.channel(),
            snapshot_root: snapshot_path.into(),
            client: client.clone(),
        };
        let snapshot = Arc::new(SnapshotService::new(snapshot_params)?);

        let client_io = Arc::new(ClientIoHandler {
            client: client.clone(),
            snapshot: snapshot.clone(),
        });
        io_service.register_handler(client_io)?;

        spec.engine.register_client(Arc::downgrade(&client) as _);

        let stop_guard = StopGuard::new();

        Ok(ClientService {
            io_service: Arc::new(io_service),
            client: client,
            snapshot: snapshot,
            database: blockchain_db,
            _stop_guard: stop_guard,
        })
    }

    /// Get general IO interface
    pub fn register_io_handler(
        &self,
        handler: Arc<dyn IoHandler<ClientIoMessage> + Send>,
    ) -> Result<(), IoError> {
        self.io_service.register_handler(handler)
    }

    /// Get client interface
    pub fn client(&self) -> Arc<Client> {
        self.client.clone()
    }

    /// Get snapshot interface.
    pub fn snapshot_service(&self) -> Arc<SnapshotService> {
        self.snapshot.clone()
    }

    /// Get network service component
    pub fn io(&self) -> Arc<IoService<ClientIoMessage>> {
        self.io_service.clone()
    }

    /// Set the actor to be notified on certain chain events
    pub fn add_notify(&self, notify: Arc<dyn ChainNotify>) {
        self.client.add_notify(notify);
    }

    /// Get a handle to the database.
    pub fn db(&self) -> Arc<dyn BlockChainDB> {
        self.database.clone()
    }

    /// Shutdown the Client Service
    pub fn shutdown(&self) {
        trace!(target: "shutdown", "Shutting down Client Service");
        self.snapshot.shutdown();
        self.client.shutdown();
    }
}

/// IO interface for the Client handler
struct ClientIoHandler {
    client: Arc<Client>,
    snapshot: Arc<SnapshotService>,
}

const CLIENT_TICK_TIMER: TimerToken = 0;
const SNAPSHOT_TICK_TIMER: TimerToken = 1;

const CLIENT_TICK: Duration = Duration::from_secs(5);
const SNAPSHOT_TICK: Duration = Duration::from_secs(10);

impl IoHandler<ClientIoMessage> for ClientIoHandler {
    fn initialize(&self, io: &IoContext<ClientIoMessage>) {
        io.register_timer(CLIENT_TICK_TIMER, CLIENT_TICK)
            .expect("Error registering client timer");
        io.register_timer(SNAPSHOT_TICK_TIMER, SNAPSHOT_TICK)
            .expect("Error registering snapshot timer");
    }

    fn timeout(&self, _io: &IoContext<ClientIoMessage>, timer: TimerToken) {
        trace_time!("service::read");
        match timer {
            CLIENT_TICK_TIMER => {
                use ethcore::snapshot::SnapshotService;
                let snapshot_restoration =
                    if let RestorationStatus::Ongoing { .. } = self.snapshot.restoration_status() {
                        true
                    } else {
                        false
                    };
                self.client.tick(snapshot_restoration)
            }
            SNAPSHOT_TICK_TIMER => self.snapshot.tick(),
            _ => warn!("IO service triggered unregistered timer '{}'", timer),
        }
    }

    fn message(&self, _io: &IoContext<ClientIoMessage>, net_message: &ClientIoMessage) {
        trace_time!("service::message");
        use std::thread;

        match *net_message {
            ClientIoMessage::BlockVerified => {
                self.client.import_verified_blocks();
            }
            ClientIoMessage::BeginRestoration(ref manifest) => {
                if let Err(e) = self.snapshot.init_restore(manifest.clone(), true) {
                    warn!("Failed to initialize snapshot restoration: {}", e);
                }
            }
            ClientIoMessage::FeedStateChunk(ref hash, ref chunk) => {
                self.snapshot.feed_state_chunk(*hash, chunk)
            }
            ClientIoMessage::FeedBlockChunk(ref hash, ref chunk) => {
                self.snapshot.feed_block_chunk(*hash, chunk)
            }
            ClientIoMessage::TakeSnapshot(num) => {
                let client = self.client.clone();
                let snapshot = self.snapshot.clone();

                let res = thread::Builder::new()
                    .name("Periodic Snapshot".into())
                    .spawn(move || {
                        if let Err(e) = snapshot.take_snapshot(&*client, num) {
                            match e {
                                EthcoreError(
                                    ErrorKind::Snapshot(SnapshotError::SnapshotAborted),
                                    _,
                                ) => info!("Snapshot aborted"),
                                _ => warn!("Failed to take snapshot at block #{}: {}", num, e),
                            }
                        }
                    });

                if let Err(e) = res {
                    debug!(target: "snapshot", "Failed to initialize periodic snapshot thread: {:?}", e);
                }
            }
            ClientIoMessage::Execute(ref exec) => {
                (*exec.0)(&self.client);
            }
            _ => {} // ignore other messages
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread, time};

    use tempdir::TempDir;

    use super::*;
    use ethcore::{client::ClientConfig, miner::Miner, spec::Spec, test_helpers};
    use ethcore_db::NUM_COLUMNS;
    use kvdb_rocksdb::{CompactionProfile, DatabaseConfig};

    #[test]
    fn it_can_be_started() {
        let tempdir = TempDir::new("").unwrap();
        let client_path = tempdir.path().join("client");
        let snapshot_path = tempdir.path().join("snapshot");

        let client_config = ClientConfig::default();
        let mut client_db_config = DatabaseConfig::with_columns(NUM_COLUMNS);

        client_db_config.memory_budget = client_config.db_cache_size;
        client_db_config.compaction = CompactionProfile::auto(&client_path);

        let client_db_handler = test_helpers::restoration_db_handler(client_db_config.clone());
        let client_db = client_db_handler.open(&client_path).unwrap();
        let restoration_db_handler = test_helpers::restoration_db_handler(client_db_config);

        let spec = Spec::new_test();
        let service = ClientService::start(
            ClientConfig::default(),
            &spec,
            client_db,
            &snapshot_path,
            restoration_db_handler,
            tempdir.path(),
            Arc::new(Miner::new_for_tests(&spec, None)),
        );
        assert!(service.is_ok());
        drop(service.unwrap());
        thread::park_timeout(time::Duration::from_millis(100));
    }
}
