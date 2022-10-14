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

//! Impls of the `AsHashDB` upcast trait for all different variants of DB
use crate::{AsKeyedHashDB, KeyedHashDB};
use archivedb::ArchiveDB;
use earlymergedb::EarlyMergeDB;
use hash_db::{AsHashDB, HashDB};
use keccak_hasher::KeccakHasher;
use kvdb::DBValue;
use overlaydb::OverlayDB;
use overlayrecentdb::OverlayRecentDB;
use refcounteddb::RefCountedDB;

impl AsHashDB<KeccakHasher, DBValue> for ArchiveDB {
    fn as_hash_db(&self) -> &dyn HashDB<KeccakHasher, DBValue> {
        self
    }
    fn as_hash_db_mut(&mut self) -> &mut dyn HashDB<KeccakHasher, DBValue> {
        self
    }
}

impl AsHashDB<KeccakHasher, DBValue> for EarlyMergeDB {
    fn as_hash_db(&self) -> &dyn HashDB<KeccakHasher, DBValue> {
        self
    }
    fn as_hash_db_mut(&mut self) -> &mut dyn HashDB<KeccakHasher, DBValue> {
        self
    }
}

impl AsHashDB<KeccakHasher, DBValue> for OverlayRecentDB {
    fn as_hash_db(&self) -> &dyn HashDB<KeccakHasher, DBValue> {
        self
    }
    fn as_hash_db_mut(&mut self) -> &mut dyn HashDB<KeccakHasher, DBValue> {
        self
    }
}

impl AsHashDB<KeccakHasher, DBValue> for RefCountedDB {
    fn as_hash_db(&self) -> &dyn HashDB<KeccakHasher, DBValue> {
        self
    }
    fn as_hash_db_mut(&mut self) -> &mut dyn HashDB<KeccakHasher, DBValue> {
        self
    }
}

impl AsHashDB<KeccakHasher, DBValue> for OverlayDB {
    fn as_hash_db(&self) -> &dyn HashDB<KeccakHasher, DBValue> {
        self
    }
    fn as_hash_db_mut(&mut self) -> &mut dyn HashDB<KeccakHasher, DBValue> {
        self
    }
}

impl AsKeyedHashDB for ArchiveDB {
    fn as_keyed_hash_db(&self) -> &dyn KeyedHashDB {
        self
    }
}

impl AsKeyedHashDB for EarlyMergeDB {
    fn as_keyed_hash_db(&self) -> &dyn KeyedHashDB {
        self
    }
}

impl AsKeyedHashDB for OverlayRecentDB {
    fn as_keyed_hash_db(&self) -> &dyn KeyedHashDB {
        self
    }
}

impl AsKeyedHashDB for RefCountedDB {
    fn as_keyed_hash_db(&self) -> &dyn KeyedHashDB {
        self
    }
}

impl AsKeyedHashDB for OverlayDB {
    fn as_keyed_hash_db(&self) -> &dyn KeyedHashDB {
        self
    }
}
