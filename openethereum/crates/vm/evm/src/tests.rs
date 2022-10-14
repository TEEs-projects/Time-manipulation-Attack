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

use super::interpreter::MAX_SUB_STACK_SIZE;
use ethereum_types::{Address, H256, U256};
use factory::Factory;
use hex_literal::hex;
use rustc_hex::FromHex;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
    str::FromStr,
    sync::Arc,
};
use vm::{
    self,
    tests::{test_finalize, FakeCall, FakeCallType, FakeExt},
    ActionParams, ActionValue, Ext,
};
use vmtype::VMType;

evm_test! {test_add: test_add_int}
fn test_add(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff01600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_988));
    assert_store(
        &ext,
        0,
        "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
    );
}

evm_test! {test_sha3: test_sha3_int}
fn test_sha3(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "6000600020600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_961));
    assert_store(
        &ext,
        0,
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
    );
}

evm_test! {test_address: test_address_int}
fn test_address(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "30600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000f572e5295c57f15886f9b263e2f6d2d6c7b5ec6",
    );
}

evm_test! {test_origin: test_origin_int}
fn test_origin(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let origin = Address::from_str("cd1722f2947def4cf144679da39c4c32bdc35681").unwrap();
    let code = "32600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.origin = origin.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "000000000000000000000000cd1722f2947def4cf144679da39c4c32bdc35681",
    );
}

evm_test! {test_selfbalance: test_selfbalance_int}
fn test_selfbalance(factory: super::Factory) {
    let own_addr = Address::from_str("1337000000000000000000000000000000000000").unwrap();
    // 47       SELFBALANCE
    // 60 ff    PUSH ff
    // 55       SSTORE
    let code = hex!("47 60 ff 55").to_vec();

    let mut params = ActionParams::default();
    params.address = own_addr.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_istanbul();
    ext.balances = {
        let mut x = HashMap::new();
        x.insert(own_addr, U256::from(1_025)); // 0x401
        x
    };
    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };
    assert_eq!(gas_left, U256::from(79_992)); // TODO[dvdplm]: do the sums here, SELFBALANCE-5 + PUSH1-3 + ONEBYTE-4 + SSTORE-?? = 100_000 - 79_992
    assert_store(
        &ext,
        0xff,
        "0000000000000000000000000000000000000000000000000000000000000401",
    );
}

evm_test! {test_sender: test_sender_int}
fn test_sender(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let sender = Address::from_str("cd1722f2947def4cf144679da39c4c32bdc35681").unwrap();
    let code = "33600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.sender = sender.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "000000000000000000000000cd1722f2947def4cf144679da39c4c32bdc35681",
    );
}

evm_test! {test_chain_id: test_chain_id_int}
fn test_chain_id(factory: super::Factory) {
    // 46       CHAINID
    // 60 00    PUSH 0
    // 55       SSTORE
    let code = hex!("46 60 00 55").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_istanbul().with_chain_id(9);

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000009",
    );
}

evm_test! {test_extcodecopy: test_extcodecopy_int}
fn test_extcodecopy(factory: super::Factory) {
    // 33 - sender
    // 3b - extcodesize
    // 60 00 - push 0
    // 60 00 - push 0
    // 33 - sender
    // 3c - extcodecopy
    // 60 00 - push 0
    // 51 - load word from memory
    // 60 00 - push 0
    // 55 - sstore

    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let sender = Address::from_str("cd1722f2947def4cf144679da39c4c32bdc35681").unwrap();
    let code = "333b60006000333c600051600055".from_hex().unwrap();
    let sender_code = "6005600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.sender = sender.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.codes.insert(sender, Arc::new(sender_code));

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_935));
    assert_store(
        &ext,
        0,
        "6005600055000000000000000000000000000000000000000000000000000000",
    );
}

evm_test! {test_log_empty: test_log_empty_int}
fn test_log_empty(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "60006000a0".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(99_619));
    assert_eq!(ext.logs.len(), 1);
    assert_eq!(ext.logs[0].topics.len(), 0);
    assert!(ext.logs[0].data.is_empty());
}

evm_test! {test_log_sender: test_log_sender_int}
fn test_log_sender(factory: super::Factory) {
    // 60 ff - push ff
    // 60 00 - push 00
    // 53 - mstore
    // 33 - sender
    // 60 20 - push 20
    // 60 00 - push 0
    // a1 - log with 1 topic

    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let sender = Address::from_str("cd1722f3947def4cf144679da39c4c32bdc35681").unwrap();
    let code = "60ff6000533360206000a1".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.sender = sender.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(98_974));
    assert_eq!(ext.logs.len(), 1);
    assert_eq!(ext.logs[0].topics.len(), 1);
    assert_eq!(
        ext.logs[0].topics[0],
        H256::from_str("000000000000000000000000cd1722f3947def4cf144679da39c4c32bdc35681").unwrap()
    );
    assert_eq!(
        ext.logs[0].data,
        "ff00000000000000000000000000000000000000000000000000000000000000"
            .from_hex()
            .unwrap()
    );
}

evm_test! {test_blockhash: test_blockhash_int}
fn test_blockhash(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "600040600055".from_hex().unwrap();
    let blockhash =
        H256::from_str("123400000000000000000000cd1722f2947def4cf144679da39c4c32bdc35681").unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.blockhashes.insert(U256::zero(), blockhash.clone());

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_974));
    assert_eq!(ext.store.get(&H256::default()).unwrap(), &blockhash);
}

evm_test! {test_calldataload: test_calldataload_int}
fn test_calldataload(factory: super::Factory) {
    let address = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "600135600055".from_hex().unwrap();
    let data = "0123ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff23"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.address = address.clone();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    params.data = Some(data);
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_991));
    assert_store(
        &ext,
        0,
        "23ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff23",
    );
}

evm_test! {test_author: test_author_int}
fn test_author(factory: super::Factory) {
    let author = Address::from_str("0f572e5295c57f15886f9b263e2f6d2d6c7b5ec6").unwrap();
    let code = "41600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.info.author = author;

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000f572e5295c57f15886f9b263e2f6d2d6c7b5ec6",
    );
}

evm_test! {test_timestamp: test_timestamp_int}
fn test_timestamp(factory: super::Factory) {
    let timestamp = 0x1234;
    let code = "42600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.info.timestamp = timestamp;

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000001234",
    );
}

evm_test! {test_number: test_number_int}
fn test_number(factory: super::Factory) {
    let number = 0x1234;
    let code = "43600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.info.number = number;

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000001234",
    );
}

evm_test! {test_difficulty: test_difficulty_int}
fn test_difficulty(factory: super::Factory) {
    let difficulty = U256::from(0x1234);
    let code = "44600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.info.difficulty = difficulty;

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000001234",
    );
}

evm_test! {test_base_fee: test_base_fee_int}
fn test_base_fee(factory: super::Factory) {
    let base_fee = Some(U256::from(0x07));
    let code = "48600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_london(
        Address::from_str("0000000000000000000000000000000000000000").unwrap(),
        Address::from_str("000000000000000000000000636F6E7472616374").unwrap(),
        &[],
    );
    ext.info.base_fee = base_fee;

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(77_895));
    println!("elements {}", ext.store.len());
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000007",
    );
}

evm_test! {test_gas_limit: test_gas_limit_int}
fn test_gas_limit(factory: super::Factory) {
    let gas_limit = U256::from(0x1234);
    let code = "45600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();
    ext.info.gas_limit = gas_limit;

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(79_995));
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000001234",
    );
}

evm_test! {test_mul: test_mul_int}
fn test_mul(factory: super::Factory) {
    let code = "65012365124623626543219002600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "000000000000000000000000000000000000000000000000734349397b853383",
    );
    assert_eq!(gas_left, U256::from(79_983));
}

evm_test! {test_sub: test_sub_int}
fn test_sub(factory: super::Factory) {
    let code = "65012365124623626543219003600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000012364ad0302",
    );
    assert_eq!(gas_left, U256::from(79_985));
}

evm_test! {test_div: test_div_int}
fn test_div(factory: super::Factory) {
    let code = "65012365124623626543219004600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "000000000000000000000000000000000000000000000000000000000002e0ac",
    );
    assert_eq!(gas_left, U256::from(79_983));
}

evm_test! {test_div_zero: test_div_zero_int}
fn test_div_zero(factory: super::Factory) {
    let code = "6501236512462360009004600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(gas_left, U256::from(94_983));
}

evm_test! {test_mod: test_mod_int}
fn test_mod(factory: super::Factory) {
    let code = "650123651246236265432290066000556501236512462360009006600155"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000076b4b",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(gas_left, U256::from(74_966));
}

evm_test! {test_smod: test_smod_int}
fn test_smod(factory: super::Factory) {
    let code = "650123651246236265432290076000556501236512462360009007600155"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000076b4b",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(gas_left, U256::from(74_966));
}

evm_test! {test_sdiv: test_sdiv_int}
fn test_sdiv(factory: super::Factory) {
    let code = "650123651246236265432290056000556501236512462360009005600155"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "000000000000000000000000000000000000000000000000000000000002e0ac",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(gas_left, U256::from(74_966));
}

evm_test! {test_exp: test_exp_int}
fn test_exp(factory: super::Factory) {
    let code = "6016650123651246230a6000556001650123651246230a6001556000650123651246230a600255"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "90fd23767b60204c3d6fc8aec9e70a42a3f127140879c133a20129a597ed0c59",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000012365124623",
    );
    assert_store(
        &ext,
        2,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_eq!(gas_left, U256::from(39_923));
}

evm_test! {test_comparison: test_comparison_int}
fn test_comparison(factory: super::Factory) {
    let code = "601665012365124623818181811060005511600155146002556415235412358014600355"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_store(
        &ext,
        2,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_store(
        &ext,
        3,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_eq!(gas_left, U256::from(49_952));
}

evm_test! {test_signed_comparison: test_signed_comparison_int}
fn test_signed_comparison(factory: super::Factory) {
    let code = "60106000036010818112600055136001556010601060000381811260025513600355"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_store(
        &ext,
        2,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_store(
        &ext,
        3,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(gas_left, U256::from(49_940));
}

evm_test! {test_bitops: test_bitops_int}
fn test_bitops(factory: super::Factory) {
    let code = "60ff610ff08181818116600055176001551860025560008015600355198015600455600555"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(150_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "00000000000000000000000000000000000000000000000000000000000000f0",
    );
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000fff",
    );
    assert_store(
        &ext,
        2,
        "0000000000000000000000000000000000000000000000000000000000000f0f",
    );
    assert_store(
        &ext,
        3,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_store(
        &ext,
        4,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_store(
        &ext,
        5,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    assert_eq!(gas_left, U256::from(44_937));
}

evm_test! {test_addmod_mulmod: test_addmod_mulmod_int}
fn test_addmod_mulmod(factory: super::Factory) {
    let code = "60ff60f060108282820860005509600155600060f0601082828208196002550919600355"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    assert_store(
        &ext,
        1,
        "000000000000000000000000000000000000000000000000000000000000000f",
    );
    assert_store(
        &ext,
        2,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    assert_store(
        &ext,
        3,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    assert_eq!(gas_left, U256::from(19_914));
}

evm_test! {test_byte: test_byte_int}
fn test_byte(factory: super::Factory) {
    let code = "60f061ffff1a600055610fff601f1a600155".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_store(
        &ext,
        1,
        "00000000000000000000000000000000000000000000000000000000000000ff",
    );
    assert_eq!(gas_left, U256::from(74_976));
}

evm_test! {test_signextend: test_signextend_int}
fn test_signextend(factory: super::Factory) {
    let code = "610fff60020b60005560ff60200b600155".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000fff",
    );
    assert_store(
        &ext,
        1,
        "00000000000000000000000000000000000000000000000000000000000000ff",
    );
    assert_eq!(gas_left, U256::from(59_972));
}

#[test] // JIT just returns out of gas
fn test_badinstruction_int() {
    let factory = super::Factory::new(VMType::Interpreter, 1024 * 32);
    let code = "af".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let err = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap_err()
    };

    match err {
        vm::Error::BadInstruction { instruction: 0xaf } => (),
        _ => assert!(false, "Expected bad instruction"),
    }
}

evm_test! {test_pop: test_pop_int}
fn test_pop(factory: super::Factory) {
    let code = "60f060aa50600055".from_hex().unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "00000000000000000000000000000000000000000000000000000000000000f0",
    );
    assert_eq!(gas_left, U256::from(79_989));
}

evm_test! {test_extops: test_extops_int}
fn test_extops(factory: super::Factory) {
    let code = "5a6001555836553a600255386003553460045560016001526016590454600555"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(150_000);
    params.gas_price = U256::from(0x32);
    params.value = ActionValue::Transfer(U256::from(0x99));
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000004",
    ); // PC / CALLDATASIZE
    assert_store(
        &ext,
        1,
        "00000000000000000000000000000000000000000000000000000000000249ee",
    ); // GAS
    assert_store(
        &ext,
        2,
        "0000000000000000000000000000000000000000000000000000000000000032",
    ); // GASPRICE
    assert_store(
        &ext,
        3,
        "0000000000000000000000000000000000000000000000000000000000000020",
    ); // CODESIZE
    assert_store(
        &ext,
        4,
        "0000000000000000000000000000000000000000000000000000000000000099",
    ); // CALLVALUE
    assert_store(
        &ext,
        5,
        "0000000000000000000000000000000000000000000000000000000000000032",
    );
    assert_eq!(gas_left, U256::from(29_898));
}

evm_test! {test_jumps: test_jumps_int}
fn test_jumps(factory: super::Factory) {
    let code = "600160015560066000555b60016000540380806000551560245760015402600155600a565b"
        .from_hex()
        .unwrap();

    let mut params = ActionParams::default();
    params.gas = U256::from(150_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new();

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(ext.sstore_clears, ext.schedule().sstore_refund_gas as i128);
    assert_store(
        &ext,
        0,
        "0000000000000000000000000000000000000000000000000000000000000000",
    ); // 5!
    assert_store(
        &ext,
        1,
        "0000000000000000000000000000000000000000000000000000000000000078",
    ); // 5!
    assert_eq!(gas_left, U256::from(54_117));
}

evm_test! {test_subs_simple: test_subs_simple_int}
fn test_subs_simple(factory: super::Factory) {
    // as defined in https://eips.ethereum.org/EIPS/eip-2315
    let code = hex!("60045e005c5d").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(18);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_subs_two_levels: test_subs_two_levels_int}
fn test_subs_two_levels(factory: super::Factory) {
    // as defined in https://eips.ethereum.org/EIPS/eip-2315
    let code = hex!("6800000000000000000c5e005c60115e5d5c5d").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(36);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_subs_invalid_jump: test_subs_invalid_jump_int}
fn test_subs_invalid_jump(factory: super::Factory) {
    // as defined in https://eips.ethereum.org/EIPS/eip-2315
    let code = hex!("6801000000000000000c5e005c60115e5d5c5d").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(24);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let current = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap())
    };

    let expected = Result::Err(vm::Error::BadJumpDestination { destination: 0xc });
    assert_eq!(current, expected);
}

evm_test! {test_subs_shallow_return_stack: test_subs_shallow_return_stack_int}
fn test_subs_shallow_return_stack(factory: super::Factory) {
    // as defined in https://eips.ethereum.org/EIPS/eip-2315
    let code = hex!("5d5858").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(24);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let current = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap())
    };

    let expected = Result::Err(vm::Error::SubStackUnderflow {
        wanted: 1,
        on_stack: 0,
    });
    assert_eq!(current, expected);
}

evm_test! {test_subs_substack_limit: test_subs_substack_limit_int}
fn test_subs_substack_limit(factory: super::Factory) {
    //    PUSH <recursion_limit>
    //    JUMP a
    // s: BEGINSUB
    // a: JUMPDEST
    //    DUP1
    //    JUMPI c
    //    STOP
    // c: JUMPDEST
    //    PUSH1 1
    //    SWAP
    //    SUB
    //    JUMPSUB s

    let mut code = hex!("6104006007565c5b80600d57005b6001900360065e").to_vec();
    code[1..3].copy_from_slice(&(MAX_SUB_STACK_SIZE as u16).to_be_bytes()[..]);

    let mut params = ActionParams::default();
    params.gas = U256::from(1_000_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(959_049));
}

evm_test! {test_subs_substack_out: test_subs_substack_out_int}
fn test_subs_substack_out(factory: super::Factory) {
    let mut code = hex!("6104006007565c5b80600d57005b6001900360065e").to_vec();
    code[1..3].copy_from_slice(&((MAX_SUB_STACK_SIZE + 1) as u16).to_be_bytes()[..]);

    let mut params = ActionParams::default();
    params.gas = U256::from(1_000_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let current = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap())
    };

    let expected = Result::Err(vm::Error::OutOfSubStack {
        wanted: 1,
        limit: MAX_SUB_STACK_SIZE,
    });
    assert_eq!(current, expected);
}

evm_test! {test_subs_sub_at_end: test_subs_sub_at_end_int}
fn test_subs_sub_at_end(factory: super::Factory) {
    let code = hex!("6005565c5d5b60035e").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(30);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_subs_walk_into_subroutine: test_subs_walk_into_subroutine_int}
fn test_subs_walk_into_subroutine(factory: super::Factory) {
    let code = hex!("5c5d00").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(100);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(Address::zero(), Address::zero(), &[]);

    let current = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap())
    };

    let expected = Result::Err(vm::Error::InvalidSubEntry);
    assert_eq!(current, expected);
}

evm_test! {test_calls: test_calls_int}
fn test_calls(factory: super::Factory) {
    let code = "600054602d57600160005560006000600060006050610998610100f160006000600060006050610998610100f25b".from_hex().unwrap();

    let address = Address::from_low_u64_be(0x155);
    let code_address = Address::from_low_u64_be(0x998);
    let mut params = ActionParams::default();
    params.gas = U256::from(150_000);
    params.code = Some(Arc::new(code));
    params.address = address.clone();
    let mut ext = FakeExt::new();
    ext.balances = {
        let mut s = HashMap::new();
        s.insert(params.address.clone(), params.gas);
        s
    };

    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_set_contains(
        &ext.calls,
        &FakeCall {
            call_type: FakeCallType::Call,
            create_scheme: None,
            gas: U256::from(2556),
            sender_address: Some(address.clone()),
            receive_address: Some(code_address.clone()),
            value: Some(U256::from(0x50)),
            data: vec![],
            code_address: Some(code_address.clone()),
        },
    );
    assert_set_contains(
        &ext.calls,
        &FakeCall {
            call_type: FakeCallType::Call,
            create_scheme: None,
            gas: U256::from(2556),
            sender_address: Some(address.clone()),
            receive_address: Some(address.clone()),
            value: Some(U256::from(0x50)),
            data: vec![],
            code_address: Some(code_address.clone()),
        },
    );
    assert_eq!(gas_left, U256::from(91_405));
    assert_eq!(ext.calls.len(), 2);
}

evm_test! {test_create_in_staticcall: test_create_in_staticcall_int}
fn test_create_in_staticcall(factory: super::Factory) {
    let code = "600060006064f000".from_hex().unwrap();

    let address = Address::from_low_u64_be(0x155);
    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    params.address = address.clone();
    let mut ext = FakeExt::new_byzantium();
    ext.is_static = true;

    let err = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap_err()
    };

    assert_eq!(err, vm::Error::MutableCallInStaticContext);
    assert_eq!(ext.calls.len(), 0);
}

evm_test! {test_shl: test_shl_int}
fn test_shl(factory: super::Factory) {
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "00",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "01",
        "0000000000000000000000000000000000000000000000000000000000000002",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "ff",
        "8000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0100",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0101",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "00",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "01",
        "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ff",
        "8000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "0100",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "0000000000000000000000000000000000000000000000000000000000000000",
        "01",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1b,
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "01",
        "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
    );
}

evm_test! {test_shr: test_shr_int}
fn test_shr(factory: super::Factory) {
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "00",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "01",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "01",
        "4000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "ff",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "0100",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "0101",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "00",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "01",
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ff",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "0100",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1c,
        "0000000000000000000000000000000000000000000000000000000000000000",
        "01",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
}

evm_test! {test_sar: test_sar_int}
fn test_sar(factory: super::Factory) {
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "00",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "0000000000000000000000000000000000000000000000000000000000000001",
        "01",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "01",
        "c000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "ff",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "0100",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "8000000000000000000000000000000000000000000000000000000000000000",
        "0101",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "00",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "01",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ff",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "0100",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "0000000000000000000000000000000000000000000000000000000000000000",
        "01",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "4000000000000000000000000000000000000000000000000000000000000000",
        "fe",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "f8",
        "000000000000000000000000000000000000000000000000000000000000007f",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "fe",
        "0000000000000000000000000000000000000000000000000000000000000001",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "ff",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    push_two_pop_one_constantinople_test(
        &factory,
        0x1d,
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "0100",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
}

// from https://gist.github.com/holiman/174548cad102096858583c6fbbb0649a
evm_test! {test_access_list_ext_at_precompiles: test_access_list_ext_at_precompiles_int}
fn test_access_list_ext_at_precompiles(factory: super::Factory) {
    // 6001 3f 50
    // 6002 3b 50
    // 6003 31 50
    // 60f1 3f 50
    // 60f2 3b 50
    // 60f3 31 50
    // 60f2 3f 50
    // 60f3 3b 50
    // 60f1 31 50
    // 32 31 50
    // 30 31 50
    // 00

    let code = hex!(
        "60013f5060023b506003315060f13f5060f23b5060f3315060f23f5060f33b5060f1315032315030315000"
    )
    .to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(8653);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(
        Address::from_str("0000000000000000000000000000000000000000").unwrap(),
        Address::from_str("000000000000000000000000636F6E7472616374").unwrap(),
        &[
            Address::from_str("0000000000000000000000000000000000000001").unwrap(),
            Address::from_str("0000000000000000000000000000000000000002").unwrap(),
            Address::from_str("0000000000000000000000000000000000000003").unwrap(),
        ],
    );
    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_access_list_extcodecopy_twice: test_access_list_extcodecopy_twice_int}
fn test_access_list_extcodecopy_twice(factory: super::Factory) {
    let code = hex!("60006000600060ff3c60006000600060ff3c600060006000303c").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(2835);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(
        Address::from_str("0000000000000000000000000000000000000000").unwrap(),
        Address::from_str("000000000000000000000000636F6E7472616374").unwrap(),
        &[],
    );
    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_access_list_sload_sstore: test_access_list_sload_sstore_int}
fn test_access_list_sload_sstore(factory: super::Factory) {
    // 6001 54 50    sload( 0x1) pop
    // 6011 6001 55  sstore(loc: 0x01, val:0x11) 20000
    // 6011 6002 55  sstore(loc: 0x02, val:0x11) 20000 + 2100
    // 6011 6002 55  sstore(loc: 0x02, val:0x11) 100
    // 6002 54       sload(0x2)
    // 6001 54       sload(0x1)
    let code = hex!("60015450 6011600155 6011600255 6011600255 600254 600154").to_vec();

    let mut params = ActionParams::default();
    params.gas = U256::from(44529);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(
        Address::from_str("0000000000000000000000000000000000000000").unwrap(),
        Address::from_str("000000000000000000000000636F6E7472616374").unwrap(),
        &[],
    );
    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_access_list_cheap_expensive_cheap: test_access_list_cheap_expensive_cheap_int}
fn test_access_list_cheap_expensive_cheap(factory: super::Factory) {
    let code =
        hex!("60008080808060046000f15060008080808060ff6000f15060008080808060ff6000fa50").to_vec();
    let mut params = ActionParams::default();
    params.gas = U256::from(2869);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_berlin(
        Address::from_str("0000000000000000000000000000000000000000").unwrap(),
        Address::from_str("000000000000000000000000636F6E7472616374").unwrap(),
        &[Address::from_str("0000000000000000000000000000000000000004").unwrap()],
    );
    let gas_left = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_eq!(gas_left, U256::from(0));
}

evm_test! {test_refund_post_london: test_refund_post_london_int}
fn test_refund_post_london(factory: super::Factory) {
    // Compare EIP-3529 for the test cases

    let code = hex!("60006000556000600055").to_vec();
    london_refund_test(&factory, code, &[], 0);

    let code = hex!("60006000556001600055").to_vec();
    london_refund_test(&factory, code, &[], 0);

    let code = hex!("60016000556000600055").to_vec();
    london_refund_test(&factory, code, &[], 19900);

    let code = hex!("60006000556000600055").to_vec();
    london_refund_test(&factory, code, &[1], 4800);

    let code = hex!("60006000556001600055").to_vec();
    london_refund_test(&factory, code, &[1], 2800);

    let code = hex!("60006000556002600055").to_vec();
    london_refund_test(&factory, code, &[1], 0);

    let code = hex!("60026000556000600055").to_vec();
    london_refund_test(&factory, code, &[1], 4800);

    let code = hex!("60026000556001600055").to_vec();
    london_refund_test(&factory, code, &[1], 2800);

    let code = hex!("60016000556000600055").to_vec();
    london_refund_test(&factory, code, &[], 19900);

    let code = hex!("600060005560016000556000600055").to_vec();
    london_refund_test(&factory, code, &[1], 7600);
}

fn london_refund_test(
    factory: &super::Factory,
    code: Vec<u8>,
    fill: &[u64],
    expected_refund: i128,
) {
    let mut params = ActionParams::default();
    params.gas = U256::from(22318);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_london(
        Address::from_str("0000000000000000000000000000000000000000").unwrap(),
        Address::from_str("000000000000000000000000636F6E7472616374").unwrap(),
        &[],
    );
    ext.prefill(fill);
    let vm = factory.create(params, ext.schedule(), ext.depth());
    vm.exec(&mut ext).ok().unwrap().unwrap();
    assert_eq!(ext.sstore_clears, expected_refund);
}

fn push_two_pop_one_constantinople_test(
    factory: &super::Factory,
    opcode: u8,
    push1: &str,
    push2: &str,
    result: &str,
) {
    let mut push1 = push1.from_hex().unwrap();
    let mut push2 = push2.from_hex().unwrap();
    assert!(push1.len() <= 32 && push1.len() != 0);
    assert!(push2.len() <= 32 && push2.len() != 0);

    let mut code = Vec::new();
    code.push(0x60 + ((push1.len() - 1) as u8));
    code.append(&mut push1);
    code.push(0x60 + ((push2.len() - 1) as u8));
    code.append(&mut push2);
    code.push(opcode);
    code.append(&mut vec![0x60, 0x00, 0x55]);

    let mut params = ActionParams::default();
    params.gas = U256::from(100_000);
    params.code = Some(Arc::new(code));
    let mut ext = FakeExt::new_constantinople();

    let _ = {
        let vm = factory.create(params, ext.schedule(), ext.depth());
        test_finalize(vm.exec(&mut ext).ok().unwrap()).unwrap()
    };

    assert_store(&ext, 0, result);
}

fn assert_set_contains<T: Debug + Eq + PartialEq + Hash>(set: &HashSet<T>, val: &T) {
    let contains = set.contains(val);
    if !contains {
        println!("Set: {:?}", set);
        println!("Elem: {:?}", val);
    }
    assert!(contains, "Element not found in HashSet");
}

fn assert_store(ext: &FakeExt, pos: u64, val: &str) {
    assert_eq!(
        ext.store.get(&H256::from_low_u64_be(pos)).unwrap(),
        &H256::from_str(val).unwrap()
    );
}
