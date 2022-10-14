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

//! Tests for the snapshot service.

use std::{fs, sync::Arc};

use blockchain::BlockProvider;
use client::{BlockInfo, Client, ClientConfig, ImportBlock};
use snapshot::{
    chunk_secondary, chunk_state,
    io::{PackedReader, PackedWriter, SnapshotReader, SnapshotWriter},
    service::{Service, ServiceParams},
    ManifestData, Progress, RestorationStatus, SnapshotService,
};
use spec::Spec;
use tempdir::TempDir;
use test_helpers::{
    generate_dummy_client_with_spec_and_data, new_db, new_temp_db, restoration_db_handler,
};
use types::ids::BlockId;

use io::IoChannel;
use kvdb_rocksdb::DatabaseConfig;
use parking_lot::Mutex;
use verification::queue::kind::blocks::Unverified;

#[test]
fn restored_is_equivalent() {
    let _ = ::env_logger::try_init();

    const NUM_BLOCKS: u32 = 400;
    const TX_PER: usize = 5;

    let gas_prices = vec![1.into(), 2.into(), 3.into(), 999.into()];
    let client = generate_dummy_client_with_spec_and_data(
        Spec::new_null,
        NUM_BLOCKS,
        TX_PER,
        &gas_prices,
        false,
    );

    let tempdir = TempDir::new("").unwrap();
    let client_db = tempdir.path().join("client_db");
    let path = tempdir.path().join("snapshot");

    let db_config = DatabaseConfig::with_columns(::db::NUM_COLUMNS);
    let restoration = restoration_db_handler(db_config);
    let blockchain_db = restoration.open(&client_db).unwrap();

    let spec = Spec::new_null();
    let client2 = Client::new(
        Default::default(),
        &spec,
        blockchain_db,
        Arc::new(::miner::Miner::new_for_tests(&spec, None)),
        IoChannel::disconnected(),
    )
    .unwrap();

    let service_params = ServiceParams {
        engine: spec.engine.clone(),
        genesis_block: spec.genesis_block(),
        restoration_db_handler: restoration,
        pruning: ::journaldb::Algorithm::Archive,
        channel: IoChannel::disconnected(),
        snapshot_root: path,
        client: client2.clone(),
    };

    let service = Service::new(service_params).unwrap();
    service.take_snapshot(&client, NUM_BLOCKS as u64).unwrap();

    let manifest = service.manifest().unwrap();

    service.init_restore(manifest.clone(), true).unwrap();
    assert!(service.init_restore(manifest.clone(), true).is_ok());

    for hash in manifest.state_hashes {
        let chunk = service.chunk(hash).unwrap();
        service.feed_state_chunk(hash, &chunk);
    }

    for hash in manifest.block_hashes {
        let chunk = service.chunk(hash).unwrap();
        service.feed_block_chunk(hash, &chunk);
    }

    assert_eq!(service.restoration_status(), RestorationStatus::Inactive);

    for x in 0..NUM_BLOCKS {
        let block1 = client.block(BlockId::Number(x as u64)).unwrap();
        let block2 = client2.block(BlockId::Number(x as u64)).unwrap();

        assert_eq!(block1, block2);
    }
}

// on windows the guards deletion (remove_dir_all)
// is not happening (error directory is not empty).
// So the test is disabled until windows api behave.
#[cfg(not(target_os = "windows"))]
#[test]
fn guards_delete_folders() {
    let gas_prices = vec![1.into(), 2.into(), 3.into(), 999.into()];
    let client =
        generate_dummy_client_with_spec_and_data(Spec::new_null, 400, 5, &gas_prices, false);

    let spec = Spec::new_null();
    let tempdir = TempDir::new("").unwrap();
    let service_params = ServiceParams {
        engine: spec.engine.clone(),
        genesis_block: spec.genesis_block(),
        restoration_db_handler: restoration_db_handler(DatabaseConfig::with_columns(
            ::db::NUM_COLUMNS,
        )),
        pruning: ::journaldb::Algorithm::Archive,
        channel: IoChannel::disconnected(),
        snapshot_root: tempdir.path().to_owned(),
        client: client,
    };

    let service = Service::new(service_params).unwrap();
    let path = tempdir.path().join("restoration");

    let manifest = ManifestData {
        version: 2,
        state_hashes: vec![],
        block_hashes: vec![],
        block_number: 0,
        block_hash: Default::default(),
        state_root: Default::default(),
    };

    service.init_restore(manifest.clone(), true).unwrap();
    assert!(path.exists());

    // The `db` folder should have been deleted,
    // while the `temp` one kept
    service.abort_restore();
    assert!(!path.join("db").exists());
    assert!(path.join("temp").exists());

    service.init_restore(manifest.clone(), true).unwrap();
    assert!(path.exists());

    drop(service);
    assert!(!path.join("db").exists());
    assert!(path.join("temp").exists());
}

#[test]
fn keep_ancient_blocks() {
    let _ = ::env_logger::try_init();

    // Test variables
    const NUM_BLOCKS: u64 = 500;
    const NUM_SNAPSHOT_BLOCKS: u64 = 300;
    const SNAPSHOT_MODE: ::snapshot::PowSnapshot = ::snapshot::PowSnapshot {
        blocks: NUM_SNAPSHOT_BLOCKS,
        max_restore_blocks: NUM_SNAPSHOT_BLOCKS,
    };

    // Temporary folders
    let tempdir = TempDir::new("").unwrap();
    let snapshot_path = tempdir.path().join("SNAP");

    // Generate blocks
    let gas_prices = vec![1.into(), 2.into(), 3.into(), 999.into()];
    let spec_f = Spec::new_null;
    let spec = spec_f();
    let client =
        generate_dummy_client_with_spec_and_data(spec_f, NUM_BLOCKS as u32, 5, &gas_prices, false);

    let bc = client.chain();

    // Create the Snapshot
    let best_hash = bc.best_block_hash();
    let writer = Mutex::new(PackedWriter::new(&snapshot_path).unwrap());
    let block_hashes = chunk_secondary(
        Box::new(SNAPSHOT_MODE),
        &bc,
        best_hash,
        &writer,
        &Progress::default(),
    )
    .unwrap();
    let state_db = client.state_db().journal_db().boxed_clone();
    let start_header = bc.block_header_data(&best_hash).unwrap();
    let state_root = start_header.state_root();
    let state_hashes = chunk_state(
        state_db.as_hash_db(),
        &state_root,
        &writer,
        &Progress::default(),
        None,
        0,
    )
    .unwrap();

    let manifest = ManifestData {
        version: 2,
        state_hashes,
        state_root,
        block_hashes,
        block_number: NUM_BLOCKS,
        block_hash: best_hash,
    };

    writer.into_inner().finish(manifest.clone()).unwrap();

    // Initialize the Client
    let db_config = DatabaseConfig::with_columns(::db::NUM_COLUMNS);
    let client_db = new_temp_db(&tempdir.path());
    let client2 = Client::new(
        ClientConfig::default(),
        &spec,
        client_db,
        Arc::new(::miner::Miner::new_for_tests(&spec, None)),
        IoChannel::disconnected(),
    )
    .unwrap();

    // Add some ancient blocks
    for block_number in 1..50 {
        let block_hash = bc.block_hash(block_number).unwrap();
        let block = bc.block(&block_hash).unwrap();
        client2
            .import_block(
                Unverified::from_rlp(block.into_inner(), spec.params().eip1559_transition).unwrap(),
            )
            .unwrap();
    }

    client2.import_verified_blocks();
    client2.flush_queue();

    // Restore the Snapshot
    let reader = PackedReader::new(&snapshot_path).unwrap().unwrap();
    let service_params = ServiceParams {
        engine: spec.engine.clone(),
        genesis_block: spec.genesis_block(),
        restoration_db_handler: restoration_db_handler(db_config),
        pruning: ::journaldb::Algorithm::Archive,
        channel: IoChannel::disconnected(),
        snapshot_root: tempdir.path().to_owned(),
        client: client2.clone(),
    };
    let service = Service::new(service_params).unwrap();
    service.init_restore(manifest.clone(), false).unwrap();

    for hash in &manifest.block_hashes {
        let chunk = reader.chunk(*hash).unwrap();
        service.feed_block_chunk(*hash, &chunk);
    }

    for hash in &manifest.state_hashes {
        let chunk = reader.chunk(*hash).unwrap();
        service.feed_state_chunk(*hash, &chunk);
    }

    match service.restoration_status() {
        RestorationStatus::Inactive => (),
        RestorationStatus::Failed => panic!("Snapshot Restoration has failed."),
        RestorationStatus::Ongoing { .. } => panic!("Snapshot Restoration should be done."),
        _ => panic!("Invalid Snapshot Service status."),
    }

    // Check that the latest block number is the right one
    assert_eq!(
        client2.block(BlockId::Latest).unwrap().number(),
        NUM_BLOCKS as u64
    );

    // Check that we have blocks in [NUM_BLOCKS - NUM_SNAPSHOT_BLOCKS + 1 ; NUM_BLOCKS]
    // but none before
    assert!(client2
        .block(BlockId::Number(NUM_BLOCKS - NUM_SNAPSHOT_BLOCKS + 1))
        .is_some());
    assert!(client2.block(BlockId::Number(100)).is_none());

    // Check that the first 50 blocks have been migrated
    for block_number in 1..49 {
        assert!(client2.block(BlockId::Number(block_number)).is_some());
    }
}

#[test]
fn recover_aborted_recovery() {
    let _ = ::env_logger::try_init();

    const NUM_BLOCKS: u32 = 400;
    let gas_prices = vec![1.into(), 2.into(), 3.into(), 999.into()];
    let client =
        generate_dummy_client_with_spec_and_data(Spec::new_null, NUM_BLOCKS, 5, &gas_prices, false);

    let spec = Spec::new_null();
    let tempdir = TempDir::new("oe_snapshot").unwrap();
    let db_config = DatabaseConfig::with_columns(::db::NUM_COLUMNS);
    let client_db = new_db();
    let client2 = Client::new(
        Default::default(),
        &spec,
        client_db,
        Arc::new(::miner::Miner::new_for_tests(&spec, None)),
        IoChannel::disconnected(),
    )
    .unwrap();
    let service_params = ServiceParams {
        engine: spec.engine.clone(),
        genesis_block: spec.genesis_block(),
        restoration_db_handler: restoration_db_handler(db_config),
        pruning: ::journaldb::Algorithm::Archive,
        channel: IoChannel::disconnected(),
        snapshot_root: tempdir.path().to_owned(),
        client: client2.clone(),
    };

    let service = Service::new(service_params).unwrap();
    service.take_snapshot(&client, NUM_BLOCKS as u64).unwrap();

    let manifest = service.manifest().unwrap();
    service.init_restore(manifest.clone(), true).unwrap();

    // Restore only the state chunks
    for hash in &manifest.state_hashes {
        let chunk = service.chunk(*hash).unwrap();
        service.feed_state_chunk(*hash, &chunk);
    }

    match service.restoration_status() {
        RestorationStatus::Ongoing {
            block_chunks_done,
            state_chunks_done,
            ..
        } => {
            assert_eq!(state_chunks_done, manifest.state_hashes.len() as u32);
            assert_eq!(block_chunks_done, 0);
        }
        e => panic!("Snapshot restoration must be ongoing ; {:?}", e),
    }

    // Abort the restore...
    service.abort_restore();

    // And try again!
    service.init_restore(manifest.clone(), true).unwrap();

    match service.restoration_status() {
        RestorationStatus::Ongoing {
            block_chunks_done,
            state_chunks_done,
            ..
        } => {
            assert_eq!(state_chunks_done, manifest.state_hashes.len() as u32);
            assert_eq!(block_chunks_done, 0);
        }
        e => panic!("Snapshot restoration must be ongoing ; {:?}", e),
    }
    // abort restoration so that we can delete snapshot root folder
    service.abort_restore();

    // Remove the snapshot directory, and restart the restoration
    // It shouldn't have restored any previous blocks
    fs::remove_dir_all(tempdir.path()).unwrap();

    // And try again!
    service.init_restore(manifest.clone(), true).unwrap();

    match service.restoration_status() {
        RestorationStatus::Ongoing {
            block_chunks_done,
            state_chunks_done,
            ..
        } => {
            assert_eq!(block_chunks_done, 0);
            assert_eq!(state_chunks_done, 0);
        }
        _ => panic!("Snapshot restoration must be ongoing"),
    }
}
