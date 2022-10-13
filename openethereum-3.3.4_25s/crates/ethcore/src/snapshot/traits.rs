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

use super::{CreationStatus, ManifestData, RestorationStatus};
use bytes::Bytes;
use ethereum_types::H256;

/// The interface for a snapshot network service.
/// This handles:
///    - restoration of snapshots to temporary databases.
///    - responding to queries for snapshot manifests and chunks
pub trait SnapshotService: Sync + Send {
    /// Query the most recent manifest data.
    fn manifest(&self) -> Option<ManifestData>;

    /// Query the most recent snapshoted block number and hash.
    fn manifest_block(&self) -> Option<(u64, H256)>;

    /// Get the supported range of snapshot version numbers.
    /// `None` indicates warp sync isn't supported by the consensus engine.
    fn supported_versions(&self) -> Option<(u64, u64)>;

    /// Returns a list of the completed chunks
    fn completed_chunks(&self) -> Option<Vec<H256>>;

    /// Get raw chunk for a given hash.
    fn chunk(&self, hash: H256) -> Option<Bytes>;

    /// Ask the snapshot service for the restoration status.
    fn restoration_status(&self) -> RestorationStatus;

    /// Ask the snapshot service for the creation status.
    fn creation_status(&self) -> CreationStatus;

    /// Begin snapshot restoration.
    /// If restoration in-progress, this will reset it.
    /// From this point on, any previous snapshot may become unavailable.
    fn begin_restore(&self, manifest: ManifestData);

    /// Abort an in-progress restoration if there is one.
    fn abort_restore(&self);

    /// Feed a raw state chunk to the service to be processed asynchronously.
    /// no-op if not currently restoring.
    fn restore_state_chunk(&self, hash: H256, chunk: Bytes);

    /// Feed a raw block chunk to the service to be processed asynchronously.
    /// no-op if currently restoring.
    fn restore_block_chunk(&self, hash: H256, chunk: Bytes);

    /// Abort in-progress snapshotting if there is one.
    fn abort_snapshot(&self);

    /// Shutdown the Snapshot Service by aborting any ongoing restore
    fn shutdown(&self);
}
