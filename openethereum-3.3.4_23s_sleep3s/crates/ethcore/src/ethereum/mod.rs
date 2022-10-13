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

//! Ethereum protocol module.
//!
//! Contains all Ethereum network specific stuff, such as denominations and
//! consensus specifications.

/// Export the denominations module.
pub mod denominations;
/// Export the ethash module.
pub mod ethash;

pub use self::{denominations::*, ethash::Ethash};

use super::spec::*;
use machine::EthereumMachine;

/// Load chain spec from `SpecParams` and JSON.
pub fn load<'a, T: Into<Option<SpecParams<'a>>>>(params: T, b: &[u8]) -> Spec {
    match params.into() {
        Some(params) => Spec::load(params, b),
        None => Spec::load(&::std::env::temp_dir(), b),
    }
    .expect("chain spec is invalid")
}

fn load_machine(b: &[u8]) -> EthereumMachine {
    Spec::load_machine(b).expect("chain spec is invalid")
}

/// Create a new Foundation mainnet chain spec.
pub fn new_foundation<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/foundation.json"),
    )
}

/// Create a new POA Network mainnet chain spec.
pub fn new_poanet<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/poacore.json"),
    )
}

/// Create a new xDai chain spec.
pub fn new_xdai<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/xdai.json"),
    )
}

/// Create a new Volta mainnet chain spec.
pub fn new_volta<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/volta.json"),
    )
}

/// Create a new EWC mainnet chain spec.
pub fn new_ewc<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/ewc.json"),
    )
}

/// Create a new Musicoin mainnet chain spec.
pub fn new_musicoin<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    // The musicoin chain spec uses a block reward contract which can be found at
    // https://gist.github.com/andresilva/6f2afaf9486732a0797f4bdeae018ee9
    load(
        params.into(),
        include_bytes!("../../res/chainspec/musicoin.json"),
    )
}

/// Create a new Ellaism mainnet chain spec.
pub fn new_ellaism<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/ellaism.json"),
    )
}

/// Create a new MIX mainnet chain spec.
pub fn new_mix<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/mix.json"),
    )
}

/// Create a new Callisto chain spec
pub fn new_callisto<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/callisto.json"),
    )
}

/// Create a new Morden testnet chain spec.
pub fn new_morden<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/morden.json"),
    )
}

/// Create a new Ropsten testnet chain spec.
pub fn new_ropsten<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/ropsten.json"),
    )
}

/// Create a new Kovan testnet chain spec.
pub fn new_kovan<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/kovan.json"),
    )
}

/// Create a new Rinkeby testnet chain spec.
pub fn new_rinkeby<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/rinkeby.json"),
    )
}

/// Create a new Görli testnet chain spec.
pub fn new_goerli<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/goerli.json"),
    )
}

/// Create a new POA Sokol testnet chain spec.
pub fn new_sokol<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/poasokol.json"),
    )
}
/// Create a new YOLO spec
pub fn new_yolo3<'a, T: Into<SpecParams<'a>>>(params: T) -> Spec {
    load(
        params.into(),
        include_bytes!("../../res/chainspec/yolo3.json"),
    )
}

// For tests

/// Create a new Foundation Frontier-era chain spec as though it never changes to Homestead.
pub fn new_frontier_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/frontier_test.json"),
    )
}

/// Create a new Ropsten chain spec.
pub fn new_ropsten_test() -> Spec {
    load(None, include_bytes!("../../res/chainspec/ropsten.json"))
}

/// Create a new Foundation Homestead-era chain spec as though it never changed from Frontier.
pub fn new_homestead_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/homestead_test.json"),
    )
}

/// Create a new Foundation Homestead-EIP150-era chain spec as though it never changed from Homestead/Frontier.
pub fn new_eip150_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/eip150_test.json"),
    )
}

/// Create a new Foundation Homestead-EIP161-era chain spec as though it never changed from Homestead/Frontier.
pub fn new_eip161_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/eip161_test.json"),
    )
}

/// Create a new Foundation Frontier/Homestead/DAO chain spec with transition points at #5 and #8.
pub fn new_transition_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/transition_test.json"),
    )
}

/// Create a new Foundation Mainnet chain spec without genesis accounts.
pub fn new_mainnet_like() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/frontier_like_test.json"),
    )
}

/// Create a new Foundation Byzantium era spec.
pub fn new_byzantium_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/byzantium_test.json"),
    )
}

/// Create a new Foundation Constantinople era spec.
pub fn new_constantinople_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/constantinople_test.json"),
    )
}

/// Create a new Foundation St. Peter's (Contantinople Fix) era spec.
pub fn new_constantinople_fix_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/st_peters_test.json"),
    )
}

/// Create a new Foundation Istanbul era spec.
pub fn new_istanbul_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/istanbul_test.json"),
    )
}

/// Create a new BizantiumToConstaninopleFixAt5 era spec.
pub fn new_byzantium_to_constantinoplefixat5_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/byzantium_to_constantinoplefixat5_test.json"),
    )
}

/// Create a new Foundation Berlin era spec.
pub fn new_berlin_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/berlin_test.json"),
    )
}

/// Create a new Foundation London era spec.
pub fn new_london_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/london_test.json"),
    )
}

/// Create a new BerlinToLondonAt5 era spec.
pub fn new_berlin_to_london_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/berlin_to_londonat5_test.json"),
    )
}

/// Create a new Musicoin-MCIP3-era spec.
pub fn new_mcip3_test() -> Spec {
    load(
        None,
        include_bytes!("../../res/chainspec/test/mcip3_test.json"),
    )
}

// For tests

/// Create a new Foundation Frontier-era chain spec as though it never changes to Homestead.
pub fn new_frontier_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/frontier_test.json"
    ))
}

/// Create a new Foundation Homestead-era chain spec as though it never changed from Frontier.
pub fn new_homestead_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/homestead_test.json"
    ))
}

/// Create a new Foundation London era chain spec.
pub fn new_london_test_machine() -> EthereumMachine {
    load_machine(include_bytes!("../../res/chainspec/test/london_test.json"))
}

/// Create a new Foundation Homestead-EIP210-era chain spec as though it never changed from Homestead/Frontier.
pub fn new_eip210_test_machine() -> EthereumMachine {
    load_machine(include_bytes!("../../res/chainspec/test/eip210_test.json"))
}

/// Create a new Foundation Byzantium era spec.
pub fn new_byzantium_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/byzantium_test.json"
    ))
}

/// Create a new Foundation Constantinople era spec.
pub fn new_constantinople_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/constantinople_test.json"
    ))
}

/// Create a new Foundation St. Peter's (Contantinople Fix) era spec.
pub fn new_constantinople_fix_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/st_peters_test.json"
    ))
}

/// Create a new Foundation Istanbul era spec.
pub fn new_istanbul_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/istanbul_test.json"
    ))
}

/// Create a new Musicoin-MCIP3-era spec.
pub fn new_mcip3_test_machine() -> EthereumMachine {
    load_machine(include_bytes!("../../res/chainspec/test/mcip3_test.json"))
}

/// Create new Kovan spec with wasm activated at certain block
pub fn new_kovan_wasm_test_machine() -> EthereumMachine {
    load_machine(include_bytes!(
        "../../res/chainspec/test/kovan_wasm_test.json"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethereum_types::{H160, H256, U256};
    use state::*;
    use std::str::FromStr;
    use test_helpers::get_temp_state_db;
    use types::{view, views::BlockView};

    #[test]
    fn ensure_db_good() {
        let spec = new_morden(&::std::env::temp_dir());
        let engine = &spec.engine;
        let genesis_header = spec.genesis_header();
        let db = spec
            .ensure_db_good(get_temp_state_db(), &Default::default())
            .unwrap();
        let s = State::from_existing(
            db,
            genesis_header.state_root().clone(),
            engine.account_start_nonce(0),
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            s.balance(&H160::from_str("0000000000000000000000000000000000000001").unwrap())
                .unwrap(),
            1u64.into()
        );
        assert_eq!(
            s.balance(&H160::from_str("0000000000000000000000000000000000000002").unwrap())
                .unwrap(),
            1u64.into()
        );
        assert_eq!(
            s.balance(&H160::from_str("0000000000000000000000000000000000000003").unwrap())
                .unwrap(),
            1u64.into()
        );
        assert_eq!(
            s.balance(&H160::from_str("0000000000000000000000000000000000000004").unwrap())
                .unwrap(),
            1u64.into()
        );
        assert_eq!(
            s.balance(&H160::from_str("102e61f5d8f9bc71d0ad4a084df4e65e05ce0e1c").unwrap())
                .unwrap(),
            U256::from(1u64) << 200
        );
        assert_eq!(
            s.balance(&H160::from_str("0000000000000000000000000000000000000000").unwrap())
                .unwrap(),
            0u64.into()
        );
    }

    #[test]
    fn morden() {
        let morden = new_morden(&::std::env::temp_dir());

        assert_eq!(
            morden.state_root(),
            H256::from_str("f3f4696bbf3b3b07775128eb7a3763279a394e382130f27c21e70233e04946a9")
                .unwrap()
        );
        let genesis = morden.genesis_block();
        assert_eq!(
            view!(BlockView, &genesis).header_view().hash(),
            H256::from_str("0cd786a2425d16f152c658316c423e6ce1181e15c3295826d7c9904cba9ce303")
                .unwrap()
        );

        let _ = morden.engine;
    }

    #[test]
    fn frontier() {
        let frontier = new_foundation(&::std::env::temp_dir());

        assert_eq!(
            frontier.state_root(),
            H256::from_str("d7f8974fb5ac78d9ac099b9ad5018bedc2ce0a72dad1827a1709da30580f0544")
                .unwrap()
        );
        let genesis = frontier.genesis_block();
        assert_eq!(
            view!(BlockView, &genesis).header_view().hash(),
            H256::from_str("d4e56740f876aef8c010b86a40d5f56745a118d0906a34e69aec8c0db1cb8fa3")
                .unwrap()
        );

        let _ = frontier.engine;
    }
}
