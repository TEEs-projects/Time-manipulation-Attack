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

//! Smart contract based transaction filter.

use ethabi::FunctionOutputDecoder;
use ethereum_types::{Address, H256, U256};
use lru_cache::LruCache;

use call_contract::CallContract;
use client::{BlockId, BlockInfo};
use hash::KECCAK_EMPTY;
use parking_lot::Mutex;
use spec::CommonParams;
use types::{
    transaction::{Action, SignedTransaction},
    BlockNumber,
};

use_contract!(
    transact_acl_deprecated,
    "res/contracts/tx_acl_deprecated.json"
);
use_contract!(transact_acl, "res/contracts/tx_acl.json");
use_contract!(
    transact_acl_gas_price,
    "res/contracts/tx_acl_gas_price.json"
);
use_contract!(transact_acl_1559, "res/contracts/tx_acl_1559.json");

const MAX_CACHE_SIZE: usize = 4096;

mod tx_permissions {
    pub const _ALL: u32 = 0xffffffff;
    pub const NONE: u32 = 0x0;
    pub const BASIC: u32 = 0b00000001;
    pub const CALL: u32 = 0b00000010;
    pub const CREATE: u32 = 0b00000100;
    pub const _PRIVATE: u32 = 0b00001000;
}

/// Connection filter that uses a contract to manage permissions.
pub struct TransactionFilter {
    contract_address: Address,
    transition_block: BlockNumber,
    permission_cache: Mutex<LruCache<(H256, Address), u32>>,
    contract_version_cache: Mutex<LruCache<H256, Option<U256>>>,
}

impl TransactionFilter {
    /// Create a new instance if address is specified in params.
    pub fn from_params(params: &CommonParams) -> Option<TransactionFilter> {
        params
            .transaction_permission_contract
            .map(|address| TransactionFilter {
                contract_address: address,
                transition_block: params.transaction_permission_contract_transition,
                permission_cache: Mutex::new(LruCache::new(MAX_CACHE_SIZE)),
                contract_version_cache: Mutex::new(LruCache::new(MAX_CACHE_SIZE)),
            })
    }

    /// Check if transaction is allowed at given block.
    pub fn transaction_allowed<C: BlockInfo + CallContract>(
        &self,
        parent_hash: &H256,
        block_number: BlockNumber,
        transaction: &SignedTransaction,
        client: &C,
    ) -> bool {
        if block_number < self.transition_block {
            return true;
        }

        let mut permission_cache = self.permission_cache.lock();
        let mut contract_version_cache = self.contract_version_cache.lock();

        let (tx_type, to) = match transaction.tx().action {
            Action::Create => (tx_permissions::CREATE, Address::default()),
            Action::Call(address) => {
                if client
                    .code_hash(&address, BlockId::Hash(*parent_hash))
                    .map_or(false, |c| c != KECCAK_EMPTY)
                {
                    (tx_permissions::CALL, address)
                } else {
                    (tx_permissions::BASIC, address)
                }
            }
        };

        let sender = transaction.sender();
        let value = transaction.tx().value;
        let gas_price = transaction.tx().gas_price;
        let max_priority_fee_per_gas = transaction.max_priority_fee_per_gas();
        let gas_limit = transaction.tx().gas;
        let key = (*parent_hash, sender);

        if let Some(permissions) = permission_cache.get_mut(&key) {
            return *permissions & tx_type != 0;
        }

        let contract_address = self.contract_address;
        let contract_version = contract_version_cache
            .get_mut(parent_hash)
            .and_then(|v| *v)
            .or_else(|| {
                let (data, decoder) = transact_acl::functions::contract_version::call();
                decoder
                    .decode(
                        &client
                            .call_contract(BlockId::Hash(*parent_hash), contract_address, data)
                            .ok()?,
                    )
                    .ok()
            });
        contract_version_cache.insert(*parent_hash, contract_version);

        // Check permissions in smart contract based on its version
        let (permissions, filter_only_sender) = match contract_version {
            Some(version) => {
                let version_u64 = version.low_u64();
                trace!(target: "tx_filter", "Version of tx permission contract: {}", version);
                match version_u64 {
                    2 => {
                        let (data, decoder) =
                            transact_acl::functions::allowed_tx_types::call(sender, to, value);
                        client.call_contract(BlockId::Hash(*parent_hash), contract_address, data)
							.and_then(|value| decoder.decode(&value).map_err(|e| e.to_string()))
							.map(|(p, f)| (p.low_u32(), f))
							.unwrap_or_else(|e| {
								error!(target: "tx_filter", "Error calling tx permissions contract: {:?}", e);
								(tx_permissions::NONE, true)
							})
                    }
                    3 => {
                        trace!(target: "tx_filter", "Using filter with gas price and data");
                        let (data, decoder) =
                            transact_acl_gas_price::functions::allowed_tx_types::call(
                                sender,
                                to,
                                value,
                                gas_price,
                                transaction.tx().data.clone(),
                            );
                        client.call_contract(BlockId::Hash(*parent_hash), contract_address, data)
							.and_then(|value| decoder.decode(&value).map_err(|e| e.to_string()))
							.map(|(p, f)| (p.low_u32(), f))
							.unwrap_or_else(|e| {
								error!(target: "tx_filter", "Error calling tx permissions contract: {:?}", e);
								(tx_permissions::NONE, true)
							})
                    }
                    4 => {
                        trace!(target: "tx_filter", "Using filter with maxFeePerGas and maxPriorityFeePerGas and data");
                        let (data, decoder) = transact_acl_1559::functions::allowed_tx_types::call(
                            sender,
                            to,
                            value,
                            gas_price,
                            max_priority_fee_per_gas,
                            gas_limit,
                            transaction.tx().data.clone(),
                        );
                        client.call_contract(BlockId::Hash(*parent_hash), contract_address, data)
							.and_then(|value| decoder.decode(&value).map_err(|e| e.to_string()))
							.map(|(p, f)| (p.low_u32(), f))
							.unwrap_or_else(|e| {
								error!(target: "tx_filter", "Error calling tx permissions contract: {:?}", e);
								(tx_permissions::NONE, true)
							})
                    }
                    _ => {
                        error!(target: "tx_filter", "Unknown version of tx permissions contract is used");
                        (tx_permissions::NONE, true)
                    }
                }
            }
            None => {
                trace!(target: "tx_filter", "Fallback to the deprecated version of tx permission contract");
                let (data, decoder) =
                    transact_acl_deprecated::functions::allowed_tx_types::call(sender);
                (client.call_contract(BlockId::Hash(*parent_hash), contract_address, data)
					.and_then(|value| decoder.decode(&value).map_err(|e| e.to_string()))
					.map(|p| p.low_u32())
					.unwrap_or_else(|e| {
						error!(target: "tx_filter", "Error calling tx permissions contract: {:?}", e);
						tx_permissions::NONE
					}), true)
            }
        };

        if filter_only_sender {
            permission_cache.insert((*parent_hash, sender), permissions);
        }
        trace!(target: "tx_filter", "Given transaction data: sender: {:?} to: {:?} value: {}, gas_price: {}. Permissions required: {:X}, got: {:X}", sender, to, value, gas_price, tx_type, permissions);
        permissions & tx_type != 0
    }
}

#[cfg(test)]
mod test {
    use super::TransactionFilter;
    use client::{BlockChainClient, BlockId, Client, ClientConfig};
    use crypto::publickey::{KeyPair, Secret};
    use ethereum_types::{Address, U256};
    use io::IoChannel;
    use miner::Miner;
    use spec::Spec;
    use std::{str::FromStr, sync::Arc};
    use tempdir::TempDir;
    use test_helpers;
    use types::transaction::{
        AccessListTx, Action, EIP1559TransactionTx, Transaction, TypedTransaction,
    };

    /// Contract code: https://gist.github.com/VladLupashevskyi/84f18eabb1e4afadf572cf92af3e7e7f
    #[test]
    fn transaction_filter_ver_2() {
        let spec_data = include_str!("../res/chainspec/test/contract_ver_2_genesis.json");

        let db = test_helpers::new_db();
        let tempdir = TempDir::new("").unwrap();
        let spec = Spec::load(&tempdir.path(), spec_data.as_bytes()).unwrap();

        let client = Client::new(
            ClientConfig::default(),
            &spec,
            db,
            Arc::new(Miner::new_for_tests(&spec, None)),
            IoChannel::disconnected(),
        )
        .unwrap();
        let key1 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        )
        .unwrap();
        let key2 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000002")
                .unwrap(),
        )
        .unwrap();
        let key3 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000003")
                .unwrap(),
        )
        .unwrap();
        let key4 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000004")
                .unwrap(),
        )
        .unwrap();
        let key5 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000005")
                .unwrap(),
        )
        .unwrap();
        let key6 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000006")
                .unwrap(),
        )
        .unwrap();
        let key7 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000007")
                .unwrap(),
        )
        .unwrap();

        let filter = TransactionFilter::from_params(spec.params()).unwrap();
        let mut basic_tx = TypedTransaction::Legacy(Transaction::default());
        basic_tx.tx_mut().action =
            Action::Call(Address::from_str("d41c057fd1c78805aac12b0a94a405c0461a6fbb").unwrap());
        let create_tx = TypedTransaction::Legacy(Transaction::default());
        let mut call_tx = TypedTransaction::Legacy(Transaction::default());
        call_tx.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000005").unwrap());

        let mut basic_tx_with_ether_and_to_key7 = TypedTransaction::Legacy(Transaction::default());
        basic_tx_with_ether_and_to_key7.tx_mut().action =
            Action::Call(Address::from_str("d41c057fd1c78805aac12b0a94a405c0461a6fbb").unwrap());
        basic_tx_with_ether_and_to_key7.tx_mut().value = U256::from(123123);
        let mut call_tx_with_ether = TypedTransaction::Legacy(Transaction::default());
        call_tx_with_ether.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000005").unwrap());
        call_tx_with_ether.tx_mut().value = U256::from(123123);

        let mut basic_tx_to_key6 = TypedTransaction::Legacy(Transaction::default());
        basic_tx_to_key6.tx_mut().action =
            Action::Call(Address::from_str("e57bfe9f44b819898f47bf37e5af72a0783e1141").unwrap());
        let mut basic_tx_with_ether_and_to_key6 = TypedTransaction::Legacy(Transaction::default());
        basic_tx_with_ether_and_to_key6.tx_mut().action =
            Action::Call(Address::from_str("e57bfe9f44b819898f47bf37e5af72a0783e1141").unwrap());
        basic_tx_with_ether_and_to_key6.tx_mut().value = U256::from(123123);

        let genesis = client.block_hash(BlockId::Latest).unwrap();
        let block_number = 1;

        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key2.secret(), None),
            &*client
        ));
        // same tx but request is allowed because the contract only enables at block #1
        assert!(filter.transaction_allowed(
            &genesis,
            0,
            &create_tx.clone().sign(key2.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key1.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key1.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key1.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key2.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key2.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key2.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key3.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key3.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key3.secret(), None),
            &*client
        ));

        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key4.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key4.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key4.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key1.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key1.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key1.secret(), None),
            &*client
        ));

        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx_with_ether_and_to_key7
                .clone()
                .sign(key5.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx_with_ether.clone().sign(key5.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key6.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx_with_ether_and_to_key7
                .clone()
                .sign(key6.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx_to_key6.clone().sign(key7.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx_with_ether_and_to_key6
                .clone()
                .sign(key7.secret(), None),
            &*client
        ));
    }

    /// Contract code: res/chainspec/test/contract_ver_3.sol
    #[test]
    fn transaction_filter_ver_3() {
        let spec_data = include_str!("../res/chainspec/test/contract_ver_3_genesis.json");

        let db = test_helpers::new_db();
        let tempdir = TempDir::new("").unwrap();
        let spec = Spec::load(&tempdir.path(), spec_data.as_bytes()).unwrap();

        let client = Client::new(
            ClientConfig::default(),
            &spec,
            db,
            Arc::new(Miner::new_for_tests(&spec, None)),
            IoChannel::disconnected(),
        )
        .unwrap();
        let key1 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        )
        .unwrap();

        // The only difference to version 2 is that the contract now knows the transaction's gas price and data.
        // So we only test those: The contract allows only transactions with either nonzero gas price or short data.

        let filter = TransactionFilter::from_params(spec.params()).unwrap();
        let mut tx = TypedTransaction::Legacy(Transaction::default());
        tx.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000042").unwrap());
        tx.tx_mut().data = b"01234567".to_vec();
        tx.tx_mut().gas_price = 0.into();

        let genesis = client.block_hash(BlockId::Latest).unwrap();
        let block_number = 1;

        // Data too long and gas price zero. This transaction is not allowed.
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));

        // But if we either set a nonzero gas price or short data or both, it is allowed.
        tx.tx_mut().gas_price = 1.into();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
        tx.tx_mut().data = b"01".to_vec();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
        tx.tx_mut().gas_price = 0.into();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
    }

    /// Contract code: res/chainspec/test/contract_ver_4.sol
    #[test]
    fn transaction_filter_ver_4_legacy() {
        let spec_data = include_str!("../res/chainspec/test/contract_ver_4_genesis.json");

        let db = test_helpers::new_db();
        let tempdir = TempDir::new("").unwrap();
        let spec = Spec::load(&tempdir.path(), spec_data.as_bytes()).unwrap();

        let client = Client::new(
            ClientConfig::default(),
            &spec,
            db,
            Arc::new(Miner::new_for_tests(&spec, None)),
            IoChannel::disconnected(),
        )
        .unwrap();
        let key1 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        )
        .unwrap();

        // The only difference to version 2 is that the contract now knows the transaction's gas price and data.
        // So we only test those: The contract allows only transactions with either nonzero gas price or short data.

        let filter = TransactionFilter::from_params(spec.params()).unwrap();
        let mut tx = TypedTransaction::Legacy(Transaction::default());
        tx.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000042").unwrap());
        tx.tx_mut().data = b"01234567".to_vec();
        tx.tx_mut().gas_price = 0.into();

        let genesis = client.block_hash(BlockId::Latest).unwrap();
        let block_number = 1;

        // Data too long and gas price zero. This transaction is not allowed.
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));

        // But if we either set a nonzero gas price or short data or both, it is allowed.
        tx.tx_mut().gas_price = 1.into();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
        tx.tx_mut().data = b"01".to_vec();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
        tx.tx_mut().gas_price = 0.into();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
    }

    /// Contract code: res/chainspec/test/contract_ver_4.sol
    #[test]
    fn transaction_filter_ver_4_1559() {
        let spec_data = include_str!("../res/chainspec/test/contract_ver_4_genesis.json");

        let db = test_helpers::new_db();
        let tempdir = TempDir::new("").unwrap();
        let spec = Spec::load(&tempdir.path(), spec_data.as_bytes()).unwrap();

        let client = Client::new(
            ClientConfig::default(),
            &spec,
            db,
            Arc::new(Miner::new_for_tests(&spec, None)),
            IoChannel::disconnected(),
        )
        .unwrap();
        let key1 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        )
        .unwrap();

        // The only difference to version 2 is that the contract now knows the transaction's gas price and data.
        // So we only test those: The contract allows only transactions with either nonzero gas price or short data.

        let filter = TransactionFilter::from_params(spec.params()).unwrap();
        let mut tx = TypedTransaction::EIP1559Transaction(EIP1559TransactionTx {
            transaction: AccessListTx::new(Transaction::default(), vec![]),
            max_priority_fee_per_gas: U256::from(0),
        });
        tx.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000042").unwrap());
        tx.tx_mut().data = b"01234567".to_vec();
        tx.tx_mut().gas_price = 0.into();

        let genesis = client.block_hash(BlockId::Latest).unwrap();
        let block_number = 1;

        // Data too long and gas price zero. This transaction is not allowed.
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));

        // But if we either set a nonzero gas price or short data or both, it is allowed.
        tx.tx_mut().gas_price = 1.into();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
        tx.tx_mut().data = b"01".to_vec();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
        tx.tx_mut().gas_price = 0.into();
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &tx.clone().sign(key1.secret(), None),
            &*client
        ));
    }

    /// Contract code: https://gist.github.com/arkpar/38a87cb50165b7e683585eec71acb05a
    #[test]
    fn transaction_filter_deprecated() {
        let spec_data = include_str!("../res/chainspec/test/deprecated_contract_genesis.json");

        let db = test_helpers::new_db();
        let tempdir = TempDir::new("").unwrap();
        let spec = Spec::load(&tempdir.path(), spec_data.as_bytes()).unwrap();

        let client = Client::new(
            ClientConfig::default(),
            &spec,
            db,
            Arc::new(Miner::new_for_tests(&spec, None)),
            IoChannel::disconnected(),
        )
        .unwrap();
        let key1 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap(),
        )
        .unwrap();
        let key2 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000002")
                .unwrap(),
        )
        .unwrap();
        let key3 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000003")
                .unwrap(),
        )
        .unwrap();
        let key4 = KeyPair::from_secret(
            Secret::from_str("0000000000000000000000000000000000000000000000000000000000000004")
                .unwrap(),
        )
        .unwrap();

        let filter = TransactionFilter::from_params(spec.params()).unwrap();
        let mut basic_tx = TypedTransaction::Legacy(Transaction::default());
        basic_tx.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000032").unwrap());
        let create_tx = TypedTransaction::Legacy(Transaction::default());
        let mut call_tx = TypedTransaction::Legacy(Transaction::default());
        call_tx.tx_mut().action =
            Action::Call(Address::from_str("0000000000000000000000000000000000000005").unwrap());

        let genesis = client.block_hash(BlockId::Latest).unwrap();
        let block_number = 1;

        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key2.secret(), None),
            &*client
        ));
        // same tx but request is allowed because the contract only enables at block #1
        assert!(filter.transaction_allowed(
            &genesis,
            0,
            &create_tx.clone().sign(key2.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key1.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key1.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key1.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key2.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key2.secret(), None),
            &*client
        ));
        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key2.secret(), None),
            &*client
        ));

        assert!(filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key3.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key3.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key3.secret(), None),
            &*client
        ));

        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &basic_tx.clone().sign(key4.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &create_tx.clone().sign(key4.secret(), None),
            &*client
        ));
        assert!(!filter.transaction_allowed(
            &genesis,
            block_number,
            &call_tx.clone().sign(key4.secret(), None),
            &*client
        ));
    }
}
