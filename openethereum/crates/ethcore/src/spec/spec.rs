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

//! Parameters for a block chain.

use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    io::Read,
    path::Path,
    sync::Arc,
};

use bytes::Bytes;
use ethereum_types::{Address, Bloom, H160, H256, U256};
use ethjson;
use hash::{keccak, KECCAK_NULL_RLP};
use parking_lot::RwLock;
use rlp::{Rlp, RlpStream};
use rustc_hex::FromHex;
use types::{header::Header, BlockNumber};
use vm::{AccessList, ActionParams, ActionValue, CallType, EnvInfo, ParamsType};

use builtin::Builtin;
use engines::{
    AuthorityRound, BasicAuthority, Clique, EthEngine, InstantSeal, InstantSealParams, NullEngine,
    DEFAULT_BLOCKHASH_CONTRACT,
};
use error::Error;
use executive::Executive;
use factory::Factories;
use machine::EthereumMachine;
use maplit::btreeset;
use pod_state::PodState;
use spec::{seal::Generic as GenericSeal, Genesis};
use state::{backend::Basic as BasicBackend, Backend, State, Substate};
use trace::{NoopTracer, NoopVMTracer};

pub use ethash::OptimizeFor;

const MAX_TRANSACTION_SIZE: usize = 300 * 1024;

// helper for formatting errors.
fn fmt_err<F: ::std::fmt::Display>(f: F) -> String {
    format!("Spec json is invalid: {}", f)
}

/// Parameters common to ethereum-like blockchains.
/// NOTE: when adding bugfix hard-fork parameters,
/// add to `nonzero_bugfix_hard_fork`
///
/// we define a "bugfix" hard fork as any hard fork which
/// you would put on-by-default in a new chain.
#[derive(Debug, PartialEq, Default)]
#[cfg_attr(any(test, feature = "test-helpers"), derive(Clone))]
pub struct CommonParams {
    /// Account start nonce.
    pub account_start_nonce: U256,
    /// Maximum size of extra data.
    pub maximum_extra_data_size: usize,
    /// Network id.
    pub network_id: u64,
    /// Chain id.
    pub chain_id: u64,
    /// Main subprotocol name.
    pub subprotocol_name: String,
    /// Minimum gas limit.
    pub min_gas_limit: U256,
    /// Fork block to check.
    pub fork_block: Option<(BlockNumber, H256)>,
    /// EIP150 transition block number.
    pub eip150_transition: BlockNumber,
    /// Number of first block where EIP-160 rules begin.
    pub eip160_transition: BlockNumber,
    /// Number of first block where EIP-161.abc begin.
    pub eip161abc_transition: BlockNumber,
    /// Number of first block where EIP-161.d begins.
    pub eip161d_transition: BlockNumber,
    /// Number of first block where EIP-98 rules begin.
    pub eip98_transition: BlockNumber,
    /// Number of first block where EIP-658 rules begin.
    pub eip658_transition: BlockNumber,
    /// Number of first block where EIP-155 rules begin.
    pub eip155_transition: BlockNumber,
    /// Validate block receipts root.
    pub validate_receipts_transition: BlockNumber,
    /// Validate transaction chain id.
    pub validate_chain_id_transition: BlockNumber,
    /// Number of first block where EIP-140 rules begin.
    pub eip140_transition: BlockNumber,
    /// Number of first block where EIP-210 rules begin.
    pub eip210_transition: BlockNumber,
    /// EIP-210 Blockhash contract address.
    pub eip210_contract_address: Address,
    /// EIP-210 Blockhash contract code.
    pub eip210_contract_code: Bytes,
    /// Gas allocated for EIP-210 blockhash update.
    pub eip210_contract_gas: U256,
    /// Number of first block where EIP-211 rules begin.
    pub eip211_transition: BlockNumber,
    /// Number of first block where EIP-214 rules begin.
    pub eip214_transition: BlockNumber,
    /// Number of first block where EIP-145 rules begin.
    pub eip145_transition: BlockNumber,
    /// Number of first block where EIP-1052 rules begin.
    pub eip1052_transition: BlockNumber,
    /// Number of first block where EIP-1283 rules begin.
    pub eip1283_transition: BlockNumber,
    /// Number of first block where EIP-1283 rules end.
    pub eip1283_disable_transition: BlockNumber,
    /// Number of first block where EIP-1283 rules re-enabled.
    pub eip1283_reenable_transition: BlockNumber,
    /// Number of first block where EIP-1014 rules begin.
    pub eip1014_transition: BlockNumber,
    /// Number of first block where EIP-1706 rules begin.
    pub eip1706_transition: BlockNumber,
    /// Number of first block where EIP-1344 rules begin: https://github.com/ethereum/EIPs/blob/master/EIPS/eip-1344.md
    pub eip1344_transition: BlockNumber,
    /// Number of first block where EIP-1884 rules begin:https://github.com/ethereum/EIPs/blob/master/EIPS/eip-1884.md
    pub eip1884_transition: BlockNumber,
    /// Number of first block where EIP-2028 rules begin.
    pub eip2028_transition: BlockNumber,
    /// Number of first block where EIP-2315 rules begin.
    pub eip2315_transition: BlockNumber,
    /// Number of first block where EIP-2929 rules begin.
    pub eip2929_transition: BlockNumber,
    /// Number of first block where EIP-2930 rules begin.
    pub eip2930_transition: BlockNumber,
    /// Number of first block where EIP-1559 rules begin.
    pub eip1559_transition: BlockNumber,
    /// Number of first block where EIP-3198 rules begin. Basefee opcode.
    pub eip3198_transition: BlockNumber,
    /// Number of first block where EIP-3529 rules begin.
    pub eip3529_transition: BlockNumber,
    /// Number of first block where EIP-3541 rule begins.
    pub eip3541_transition: BlockNumber,
    /// Number of first block where EIP-3607 rule begins.
    pub eip3607_transition: BlockNumber,
    /// Number of first block where dust cleanup rules (EIP-168 and EIP169) begin.
    pub dust_protection_transition: BlockNumber,
    /// Nonce cap increase per block. Nonce cap is only checked if dust protection is enabled.
    pub nonce_cap_increment: u64,
    /// Enable dust cleanup for contracts.
    pub remove_dust_contracts: bool,
    /// Wasm activation blocknumber, if any disabled initially.
    pub wasm_activation_transition: BlockNumber,
    /// Wasm deactivation blocknumber, if enabled.
    pub wasm_disable_transition: BlockNumber,
    /// Number of first block where KIP-4 rules begin. Only has effect if Wasm is activated.
    pub kip4_transition: BlockNumber,
    /// Number of first block where KIP-6 rules begin. Only has effect if Wasm is activated.
    pub kip6_transition: BlockNumber,
    /// Gas limit bound divisor (how much gas limit can change per block)
    pub gas_limit_bound_divisor: U256,
    /// Registrar contract address.
    pub registrar: Address,
    /// Node permission managing contract address.
    pub node_permission_contract: Option<Address>,
    /// Maximum contract code size that can be deployed.
    pub max_code_size: u64,
    /// Number of first block where max code size limit is active.
    pub max_code_size_transition: BlockNumber,
    /// Transaction permission managing contract address.
    pub transaction_permission_contract: Option<Address>,
    /// Block at which the transaction permission contract should start being used.
    pub transaction_permission_contract_transition: BlockNumber,
    /// Maximum size of transaction's RLP payload
    pub max_transaction_size: usize,
    /// Base fee max change denominator
    pub eip1559_base_fee_max_change_denominator: Option<U256>,
    /// Elasticity multiplier
    pub eip1559_elasticity_multiplier: U256,
    /// Default value for the block base fee
    pub eip1559_base_fee_initial_value: U256,
    /// Min value for the block base fee.
    pub eip1559_base_fee_min_value: Option<U256>,
    /// Block at which the min value for the base fee starts to be used.
    pub eip1559_base_fee_min_value_transition: BlockNumber,
    /// Address where EIP-1559 burnt fee will be accrued to.
    pub eip1559_fee_collector: Option<Address>,
    /// Block at which the fee collector should start being used.
    pub eip1559_fee_collector_transition: BlockNumber,
    /// Block at which zero gas price transactions start being checked with Certifier contract.
    pub validate_service_transactions_transition: BlockNumber,
}

impl CommonParams {
    /// Schedule for an EVM in the post-EIP-150-era of the Ethereum main net.
    pub fn schedule(&self, block_number: u64) -> ::vm::Schedule {
        if block_number < self.eip150_transition {
            ::vm::Schedule::new_homestead()
        } else {
            let max_code_size = self.max_code_size(block_number);
            let mut schedule = ::vm::Schedule::new_post_eip150(
                max_code_size as _,
                block_number >= self.eip160_transition,
                block_number >= self.eip161abc_transition,
                block_number >= self.eip161d_transition,
            );

            self.update_schedule(block_number, &mut schedule);
            schedule
        }
    }

    /// Returns max code size at given block.
    pub fn max_code_size(&self, block_number: u64) -> u64 {
        if block_number >= self.max_code_size_transition {
            self.max_code_size
        } else {
            u64::max_value()
        }
    }

    /// Apply common spec config parameters to the schedule.
    pub fn update_schedule(&self, block_number: u64, schedule: &mut ::vm::Schedule) {
        schedule.have_create2 = block_number >= self.eip1014_transition;
        schedule.have_revert = block_number >= self.eip140_transition;
        schedule.have_static_call = block_number >= self.eip214_transition;
        schedule.have_return_data = block_number >= self.eip211_transition;
        schedule.have_bitwise_shifting = block_number >= self.eip145_transition;
        schedule.have_extcodehash = block_number >= self.eip1052_transition;
        schedule.have_chain_id = block_number >= self.eip1344_transition;
        schedule.eip1283 = (block_number >= self.eip1283_transition
            && !(block_number >= self.eip1283_disable_transition))
            || block_number >= self.eip1283_reenable_transition;
        schedule.eip1706 = block_number >= self.eip1706_transition;
        schedule.have_subs = block_number >= self.eip2315_transition;
        schedule.eip2929 = block_number >= self.eip2929_transition;
        schedule.eip2930 = block_number >= self.eip2930_transition;
        schedule.eip3541 = block_number >= self.eip3541_transition;
        schedule.eip1559 = block_number >= self.eip1559_transition;
        schedule.eip3198 = block_number >= self.eip3198_transition;
        if schedule.eip1559 {
            schedule.eip1559_elasticity_multiplier = self.eip1559_elasticity_multiplier.as_usize();

            schedule.eip1559_gas_limit_bump = if block_number == self.eip1559_transition {
                schedule.eip1559_elasticity_multiplier
            } else {
                1
            };
        }

        if block_number >= self.eip1884_transition {
            schedule.have_selfbalance = true;
            schedule.sload_gas = 800;
            schedule.balance_gas = 700;
            schedule.extcodehash_gas = 700;
        }
        if block_number >= self.eip2028_transition {
            schedule.tx_data_non_zero_gas = 16;
        }
        if block_number >= self.eip210_transition {
            schedule.blockhash_gas = 800;
        }
        if block_number >= self.eip2929_transition {
            schedule.eip2929 = true;
            schedule.eip1283 = true;

            schedule.call_gas = ::vm::schedule::EIP2929_COLD_ACCOUNT_ACCESS_COST;
            schedule.balance_gas = ::vm::schedule::EIP2929_COLD_ACCOUNT_ACCESS_COST;
            schedule.extcodecopy_base_gas = ::vm::schedule::EIP2929_COLD_ACCOUNT_ACCESS_COST;
            schedule.extcodehash_gas = ::vm::schedule::EIP2929_COLD_ACCOUNT_ACCESS_COST;
            schedule.extcodesize_gas = ::vm::schedule::EIP2929_COLD_ACCOUNT_ACCESS_COST;

            schedule.cold_sload_cost = ::vm::schedule::EIP2929_COLD_SLOAD_COST;
            schedule.cold_account_access_cost = ::vm::schedule::EIP2929_COLD_ACCOUNT_ACCESS_COST;
            schedule.warm_storage_read_cost = ::vm::schedule::EIP2929_WARM_STORAGE_READ_COST;

            schedule.sload_gas = ::vm::schedule::EIP2929_WARM_STORAGE_READ_COST;
            schedule.sstore_reset_gas = ::vm::schedule::EIP2929_SSTORE_RESET_GAS;
        }
        if block_number >= self.eip3529_transition {
            schedule.suicide_refund_gas = 0;
            schedule.sstore_refund_gas = ::vm::schedule::EIP3529_SSTORE_CLEARS_SCHEDULE;
            schedule.max_refund_quotient = ::vm::schedule::EIP3529_MAX_REFUND_QUOTIENT;
        }

        if block_number >= self.dust_protection_transition {
            schedule.kill_dust = match self.remove_dust_contracts {
                true => ::vm::CleanDustMode::WithCodeAndStorage,
                false => ::vm::CleanDustMode::BasicOnly,
            };
        }
        if block_number >= self.wasm_activation_transition
            && block_number < self.wasm_disable_transition
        {
            let mut wasm = ::vm::WasmCosts::default();
            if block_number >= self.kip4_transition {
                wasm.have_create2 = true;
            }
            if block_number >= self.kip6_transition {
                wasm.have_gasleft = true;
            }
            schedule.wasm = Some(wasm);
        }
    }

    /// Return Some if the current parameters contain a bugfix hard fork not on block 0.
    pub fn nonzero_bugfix_hard_fork(&self) -> Option<&str> {
        if self.eip155_transition != 0 {
            return Some("eip155Transition");
        }

        if self.validate_receipts_transition != 0 {
            return Some("validateReceiptsTransition");
        }

        if self.validate_chain_id_transition != 0 {
            return Some("validateChainIdTransition");
        }

        None
    }
}

impl From<ethjson::spec::Params> for CommonParams {
    fn from(p: ethjson::spec::Params) -> Self {
        CommonParams {
            account_start_nonce: p.account_start_nonce.map_or_else(U256::zero, Into::into),
            maximum_extra_data_size: p.maximum_extra_data_size.into(),
            network_id: p.network_id.into(),
            chain_id: if let Some(n) = p.chain_id {
                n.into()
            } else {
                p.network_id.into()
            },
            subprotocol_name: p.subprotocol_name.unwrap_or_else(|| "eth".to_owned()),
            min_gas_limit: p.min_gas_limit.into(),
            fork_block: if let (Some(n), Some(h)) = (p.fork_block, p.fork_hash) {
                Some((n.into(), h.into()))
            } else {
                None
            },
            eip150_transition: p.eip150_transition.map_or(0, Into::into),
            eip160_transition: p.eip160_transition.map_or(0, Into::into),
            eip161abc_transition: p.eip161abc_transition.map_or(0, Into::into),
            eip161d_transition: p.eip161d_transition.map_or(0, Into::into),
            eip98_transition: p
                .eip98_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip155_transition: p.eip155_transition.map_or(0, Into::into),
            validate_receipts_transition: p.validate_receipts_transition.map_or(0, Into::into),
            validate_chain_id_transition: p.validate_chain_id_transition.map_or(0, Into::into),
            eip140_transition: p
                .eip140_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip210_transition: p
                .eip210_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip210_contract_address: p
                .eip210_contract_address
                .map_or(H160::from_low_u64_be(0xf0), Into::into),
            eip210_contract_code: p.eip210_contract_code.map_or_else(
                || {
                    DEFAULT_BLOCKHASH_CONTRACT
                        .from_hex()
                        .expect("Default BLOCKHASH contract is valid")
                },
                Into::into,
            ),
            eip210_contract_gas: p.eip210_contract_gas.map_or(1000000.into(), Into::into),
            eip211_transition: p
                .eip211_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip145_transition: p
                .eip145_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip214_transition: p
                .eip214_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip658_transition: p
                .eip658_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1052_transition: p
                .eip1052_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1283_transition: p
                .eip1283_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1283_disable_transition: p
                .eip1283_disable_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1283_reenable_transition: p
                .eip1283_reenable_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1706_transition: p
                .eip1706_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1014_transition: p
                .eip1014_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1344_transition: p
                .eip1344_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1884_transition: p
                .eip1884_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip2028_transition: p
                .eip2028_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip2315_transition: p
                .eip2315_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip2929_transition: p
                .eip2929_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip2930_transition: p
                .eip2930_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1559_transition: p
                .eip1559_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip3198_transition: p
                .eip3198_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip3529_transition: p
                .eip3529_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip3541_transition: p
                .eip3541_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            dust_protection_transition: p
                .dust_protection_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip3607_transition: p.eip3607_transition.map_or(0, Into::into),
            nonce_cap_increment: p.nonce_cap_increment.map_or(64, Into::into),
            remove_dust_contracts: p.remove_dust_contracts.unwrap_or(false),
            gas_limit_bound_divisor: p.gas_limit_bound_divisor.into(),
            registrar: p.registrar.map_or_else(Address::default, Into::into),
            node_permission_contract: p.node_permission_contract.map(Into::into),
            max_code_size: p.max_code_size.map_or(u64::max_value(), Into::into),
            max_transaction_size: p
                .max_transaction_size
                .map_or(MAX_TRANSACTION_SIZE, Into::into),
            max_code_size_transition: p.max_code_size_transition.map_or(0, Into::into),
            transaction_permission_contract: p.transaction_permission_contract.map(Into::into),
            transaction_permission_contract_transition: p
                .transaction_permission_contract_transition
                .map_or(0, Into::into),
            wasm_activation_transition: p
                .wasm_activation_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            wasm_disable_transition: p
                .wasm_disable_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            kip4_transition: p
                .kip4_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            kip6_transition: p
                .kip6_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1559_base_fee_max_change_denominator: p
                .eip1559_base_fee_max_change_denominator
                .map(Into::into),
            eip1559_elasticity_multiplier: p
                .eip1559_elasticity_multiplier
                .map_or_else(U256::zero, Into::into),
            eip1559_base_fee_initial_value: p
                .eip1559_base_fee_initial_value
                .map_or_else(U256::zero, Into::into),
            eip1559_base_fee_min_value: p.eip1559_base_fee_min_value.map(Into::into),
            eip1559_base_fee_min_value_transition: p
                .eip1559_base_fee_min_value_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            eip1559_fee_collector: p.eip1559_fee_collector.map(Into::into),
            eip1559_fee_collector_transition: p
                .eip1559_fee_collector_transition
                .map_or_else(BlockNumber::max_value, Into::into),
            validate_service_transactions_transition: p
                .validate_service_transactions_transition
                .map_or_else(BlockNumber::max_value, Into::into),
        }
    }
}

/// Runtime parameters for the spec that are related to how the software should run the chain,
/// rather than integral properties of the chain itself.
#[derive(Debug, Clone, Copy)]
pub struct SpecParams<'a> {
    /// The path to the folder used to cache nodes. This is typically /tmp/ on Unix-like systems
    pub cache_dir: &'a Path,
    /// Whether to run slower at the expense of better memory usage, or run faster while using
    /// more
    /// memory. This may get more fine-grained in the future but for now is simply a binary
    /// option.
    pub optimization_setting: Option<OptimizeFor>,
}

impl<'a> SpecParams<'a> {
    /// Create from a cache path, with null values for the other fields
    pub fn from_path(path: &'a Path) -> Self {
        SpecParams {
            cache_dir: path,
            optimization_setting: None,
        }
    }

    /// Create from a cache path and an optimization setting
    pub fn new(path: &'a Path, optimization: OptimizeFor) -> Self {
        SpecParams {
            cache_dir: path,
            optimization_setting: Some(optimization),
        }
    }
}

impl<'a, T: AsRef<Path>> From<&'a T> for SpecParams<'a> {
    fn from(path: &'a T) -> Self {
        Self::from_path(path.as_ref())
    }
}

/// Parameters for a block chain; includes both those intrinsic to the design of the
/// chain and those to be interpreted by the active chain engine.
pub struct Spec {
    /// User friendly spec name
    pub name: String,
    /// What engine are we using for this?
    pub engine: Arc<dyn EthEngine>,
    /// Name of the subdir inside the main data dir to use for chain data and settings.
    pub data_dir: String,

    /// Known nodes on the network in enode format.
    pub nodes: Vec<String>,

    /// The genesis block's parent hash field.
    pub parent_hash: H256,
    /// The genesis block's author field.
    pub author: Address,
    /// The genesis block's difficulty field.
    pub difficulty: U256,
    /// The genesis block's gas limit field.
    pub gas_limit: U256,
    /// The genesis block's gas used field.
    pub gas_used: U256,
    /// The genesis block's timestamp field.
    pub timestamp: u64,
    /// Transactions root of the genesis block. Should be KECCAK_NULL_RLP.
    pub transactions_root: H256,
    /// Receipts root of the genesis block. Should be KECCAK_NULL_RLP.
    pub receipts_root: H256,
    /// The genesis block's extra data field.
    pub extra_data: Bytes,
    /// Each seal field, expressed as RLP, concatenated.
    pub seal_rlp: Bytes,
    /// Base fee,
    pub base_fee: Option<U256>,

    /// List of hard forks in the network.
    pub hard_forks: BTreeSet<BlockNumber>,

    /// Contract constructors to be executed on genesis.
    constructors: Vec<(Address, Bytes)>,

    /// May be prepopulated if we know this in advance.
    state_root_memo: RwLock<H256>,

    /// Genesis state as plain old data.
    genesis_state: PodState,
}

#[cfg(test)]
impl Clone for Spec {
    fn clone(&self) -> Spec {
        Spec {
            name: self.name.clone(),
            engine: self.engine.clone(),
            data_dir: self.data_dir.clone(),
            nodes: self.nodes.clone(),
            parent_hash: self.parent_hash.clone(),
            transactions_root: self.transactions_root.clone(),
            receipts_root: self.receipts_root.clone(),
            author: self.author.clone(),
            difficulty: self.difficulty.clone(),
            gas_limit: self.gas_limit.clone(),
            gas_used: self.gas_used.clone(),
            timestamp: self.timestamp.clone(),
            extra_data: self.extra_data.clone(),
            seal_rlp: self.seal_rlp.clone(),
            hard_forks: self.hard_forks.clone(),
            constructors: self.constructors.clone(),
            state_root_memo: RwLock::new(*self.state_root_memo.read()),
            genesis_state: self.genesis_state.clone(),
            base_fee: self.base_fee.clone(),
        }
    }
}

fn load_machine_from(s: ethjson::spec::Spec) -> EthereumMachine {
    let builtins = s
        .accounts
        .builtins()
        .into_iter()
        .map(|p| {
            (
                p.0.into(),
                Builtin::try_from(p.1).expect("chain spec is invalid"),
            )
        })
        .collect();
    let params = CommonParams::from(s.params);

    Spec::machine(&s.engine, params, builtins)
}

fn convert_json_to_spec(
    (address, builtin): (ethjson::hash::Address, ethjson::spec::builtin::Builtin),
) -> Result<(Address, Builtin), Error> {
    let builtin = Builtin::try_from(builtin)?;
    Ok((address.into(), builtin))
}

/// Load from JSON object.
fn load_from(spec_params: SpecParams, s: ethjson::spec::Spec) -> Result<Spec, Error> {
    let builtins: Result<BTreeMap<Address, Builtin>, _> = s
        .accounts
        .builtins()
        .into_iter()
        .map(convert_json_to_spec)
        .collect();
    let builtins = builtins?;
    let g = Genesis::from(s.genesis);
    let GenericSeal(seal_rlp) = g.seal.into();
    let params = CommonParams::from(s.params);

    let (engine, hard_forks) = Spec::engine(spec_params, s.engine, params, builtins);

    let mut s = Spec {
        name: s.name.clone().into(),
        engine,
        data_dir: s.data_dir.unwrap_or(s.name).into(),
        nodes: s.nodes.unwrap_or_else(Vec::new),
        parent_hash: g.parent_hash,
        transactions_root: g.transactions_root,
        receipts_root: g.receipts_root,
        author: g.author,
        difficulty: g.difficulty,
        gas_limit: g.gas_limit,
        gas_used: g.gas_used,
        timestamp: g.timestamp,
        extra_data: g.extra_data,
        seal_rlp: seal_rlp,
        base_fee: g.base_fee,
        hard_forks,
        constructors: s
            .accounts
            .constructors()
            .into_iter()
            .map(|(a, c)| (a.into(), c.into()))
            .collect(),
        state_root_memo: RwLock::new(Default::default()), // will be overwritten right after.
        genesis_state: s.accounts.into(),
    };

    // use memoized state root if provided.
    match g.state_root {
        Some(root) => *s.state_root_memo.get_mut() = root,
        None => {
            let _ = s.run_constructors(
                &Default::default(),
                BasicBackend(journaldb::new_memory_db()),
            )?;
        }
    }

    Ok(s)
}

macro_rules! load_bundled {
    ($e:expr) => {
        Spec::load(
            &::std::env::temp_dir(),
            include_bytes!(concat!("../../res/chainspec/", $e, ".json")) as &[u8],
        )
        .expect(concat!("Chain spec ", $e, " is invalid."))
    };
}

#[cfg(any(test, feature = "test-helpers"))]
macro_rules! load_machine_bundled {
    ($e:expr) => {
        Spec::load_machine(include_bytes!(concat!("../../res/chainspec/", $e, ".json")) as &[u8])
            .expect(concat!("Chain spec ", $e, " is invalid."))
    };
}

impl Spec {
    // create an instance of an Ethereum state machine, minus consensus logic.
    fn machine(
        engine_spec: &ethjson::spec::Engine,
        params: CommonParams,
        builtins: BTreeMap<Address, Builtin>,
    ) -> EthereumMachine {
        if let ethjson::spec::Engine::Ethash(ref ethash) = *engine_spec {
            EthereumMachine::with_ethash_extensions(params, builtins, ethash.params.clone().into())
        } else {
            EthereumMachine::regular(params, builtins)
        }
    }

    /// Convert engine spec into a arc'd Engine of the right underlying type.
    /// TODO avoid this hard-coded nastiness - use dynamic-linked plugin framework instead.
    fn engine(
        spec_params: SpecParams,
        engine_spec: ethjson::spec::Engine,
        params: CommonParams,
        builtins: BTreeMap<Address, Builtin>,
    ) -> (Arc<dyn EthEngine>, BTreeSet<BlockNumber>) {
        let mut hard_forks = btreeset![
            params.eip150_transition,
            params.eip160_transition,
            params.eip161abc_transition,
            params.eip161d_transition,
            params.eip98_transition,
            params.eip658_transition,
            params.eip155_transition,
            params.validate_receipts_transition,
            params.validate_chain_id_transition,
            params.eip140_transition,
            params.eip210_transition,
            params.eip211_transition,
            params.eip214_transition,
            params.eip145_transition,
            params.eip1052_transition,
            params.eip1283_transition,
            params.eip1283_disable_transition,
            params.eip1283_reenable_transition,
            params.eip1014_transition,
            params.eip1706_transition,
            params.eip1344_transition,
            params.eip1884_transition,
            params.eip2028_transition,
            params.eip2315_transition,
            params.eip2929_transition,
            params.eip2930_transition,
            params.eip1559_transition,
            params.eip3198_transition,
            params.eip3529_transition,
            params.eip3541_transition,
            params.dust_protection_transition,
            params.wasm_activation_transition,
            params.wasm_disable_transition,
            params.kip4_transition,
            params.kip6_transition,
            params.max_code_size_transition,
            params.transaction_permission_contract_transition,
            params.eip1559_fee_collector_transition,
            params.eip1559_base_fee_min_value_transition,
            params.validate_service_transactions_transition,
        ];
        // BUG: Rinkeby has homestead transition at block 1 but we can't reflect that in specs for non-Ethash networks
        if params.network_id == 0x4 {
            hard_forks.insert(1);
        }

        let machine = Self::machine(&engine_spec, params, builtins);

        let engine: Arc<dyn EthEngine> = match engine_spec {
            ethjson::spec::Engine::Null(null) => {
                Arc::new(NullEngine::new(null.params.into(), machine))
            }
            ethjson::spec::Engine::Ethash(ethash) => {
                // Specific transitions for Ethash-based networks
                for block in &[
                    ethash.params.homestead_transition,
                    ethash.params.dao_hardfork_transition,
                ] {
                    if let Some(block) = *block {
                        hard_forks.insert(block.into());
                    }
                }

                // Ethereum's difficulty bomb delay is a fork too
                if let Some(delays) = &ethash.params.difficulty_bomb_delays {
                    for delay in delays.keys().copied() {
                        hard_forks.insert(delay.into());
                    }
                }

                Arc::new(::ethereum::Ethash::new(
                    spec_params.cache_dir,
                    ethash.params.into(),
                    machine,
                    spec_params.optimization_setting,
                ))
            }
            ethjson::spec::Engine::InstantSeal(Some(instant_seal)) => {
                Arc::new(InstantSeal::new(instant_seal.params.into(), machine))
            }
            ethjson::spec::Engine::InstantSeal(None) => {
                Arc::new(InstantSeal::new(InstantSealParams::default(), machine))
            }
            ethjson::spec::Engine::BasicAuthority(basic_authority) => {
                Arc::new(BasicAuthority::new(basic_authority.params.into(), machine))
            }
            ethjson::spec::Engine::Clique(clique) => Clique::new(clique.params.into(), machine)
                .expect("Failed to start Clique consensus engine."),
            ethjson::spec::Engine::AuthorityRound(authority_round) => {
                AuthorityRound::new(authority_round.params.into(), machine)
                    .expect("Failed to start AuthorityRound consensus engine.")
            }
        };

        // Dummy value is a filler for non-existent transitions
        hard_forks.remove(&BlockNumber::max_value());

        (engine, hard_forks)
    }

    // given a pre-constructor state, run all the given constructors and produce a new state and
    // state root.
    fn run_constructors<T: Backend>(&self, factories: &Factories, mut db: T) -> Result<T, Error> {
        let mut root = KECCAK_NULL_RLP;

        // basic accounts in spec.
        {
            let mut t = factories.trie.create(db.as_hash_db_mut(), &mut root);

            for (address, account) in self.genesis_state.get().iter() {
                t.insert(address.as_bytes(), &account.rlp())?;
            }
        }

        for (address, account) in self.genesis_state.get().iter() {
            account.insert_additional(
                &mut *factories
                    .accountdb
                    .create(db.as_hash_db_mut(), keccak(address)),
                &factories.trie,
            );
        }

        let start_nonce = self.engine.account_start_nonce(0);

        let (root, db) = {
            let mut state = State::from_existing(db, root, start_nonce, factories.clone())?;

            // Execute contract constructors.
            let env_info = EnvInfo {
                number: 0,
                author: self.author,
                timestamp: self.timestamp,
                difficulty: self.difficulty,
                last_hashes: Default::default(),
                gas_used: U256::zero(),
                gas_limit: U256::max_value(),
                base_fee: None,
            };

            if !self.constructors.is_empty() {
                let from = Address::default();
                for &(ref address, ref constructor) in self.constructors.iter() {
                    trace!(target: "spec", "run_constructors: Creating a contract at {}.", address);
                    trace!(target: "spec", "  .. root before = {}", state.root());
                    let params = ActionParams {
                        code_address: address.clone(),
                        code_hash: Some(keccak(constructor)),
                        address: address.clone(),
                        sender: from.clone(),
                        origin: from.clone(),
                        gas: U256::max_value(),
                        gas_price: Default::default(),
                        value: ActionValue::Transfer(Default::default()),
                        code: Some(Arc::new(constructor.clone())),
                        data: None,
                        call_type: CallType::None,
                        params_type: ParamsType::Embedded,
                        access_list: AccessList::default(),
                    };

                    let mut substate = Substate::new();

                    {
                        let machine = self.engine.machine();
                        let schedule = machine.schedule(env_info.number);
                        let mut exec = Executive::new(&mut state, &env_info, &machine, &schedule);
                        if let Err(e) =
                            exec.create(params, &mut substate, &mut NoopTracer, &mut NoopVMTracer)
                        {
                            warn!(target: "spec", "Genesis constructor execution at {} failed: {}.", address, e);
                        }
                    }

                    if let Err(e) = state.commit() {
                        warn!(target: "spec", "Genesis constructor trie commit at {} failed: {}.", address, e);
                    }

                    trace!(target: "spec", "  .. root after = {}", state.root());
                }
            } else {
                state.populate_from(self.genesis_state().to_owned());
                state.commit()?;
            }
            state.drop()
        };

        *self.state_root_memo.write() = root;
        Ok(db)
    }

    /// Return the state root for the genesis state, memoising accordingly.
    pub fn state_root(&self) -> H256 {
        self.state_root_memo.read().clone()
    }

    /// Get common blockchain parameters.
    pub fn params(&self) -> &CommonParams {
        &self.engine.params()
    }

    /// Get the known knodes of the network in enode format.
    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    /// Get the configured Network ID.
    pub fn network_id(&self) -> u64 {
        self.params().network_id
    }

    /// Get the chain ID used for signing.
    pub fn chain_id(&self) -> u64 {
        self.params().chain_id
    }

    /// Get the configured subprotocol name.
    pub fn subprotocol_name(&self) -> String {
        self.params().subprotocol_name.clone()
    }

    /// Get the configured network fork block.
    pub fn fork_block(&self) -> Option<(BlockNumber, H256)> {
        self.params().fork_block
    }

    /// Get the header of the genesis block.
    pub fn genesis_header(&self) -> Header {
        let mut header: Header = Default::default();
        header.set_parent_hash(self.parent_hash.clone());
        header.set_timestamp(self.timestamp);
        header.set_number(0);
        header.set_author(self.author.clone());
        header.set_transactions_root(self.transactions_root.clone());
        header.set_uncles_hash(keccak(RlpStream::new_list(0).out()));
        header.set_extra_data(self.extra_data.clone());
        header.set_state_root(self.state_root());
        header.set_receipts_root(self.receipts_root.clone());
        header.set_log_bloom(Bloom::default());
        header.set_gas_used(self.gas_used.clone());
        header.set_gas_limit(self.gas_limit.clone());
        header.set_difficulty(self.difficulty.clone());
        header.set_seal({
            let r = Rlp::new(&self.seal_rlp);
            r.iter().map(|f| f.as_raw().to_vec()).collect()
        });
        header.set_base_fee(self.base_fee.clone());
        trace!(target: "spec", "Header hash is {}", header.hash());
        header
    }

    /// Compose the genesis block for this chain.
    pub fn genesis_block(&self) -> Bytes {
        let empty_list = RlpStream::new_list(0).out();
        let header = self.genesis_header();
        let mut ret = RlpStream::new_list(3);
        ret.append(&header);
        ret.append_raw(&empty_list, 1);
        ret.append_raw(&empty_list, 1);
        ret.out()
    }

    /// Overwrite the genesis components.
    pub fn overwrite_genesis_params(&mut self, g: Genesis) {
        let GenericSeal(seal_rlp) = g.seal.into();
        self.parent_hash = g.parent_hash;
        self.transactions_root = g.transactions_root;
        self.receipts_root = g.receipts_root;
        self.author = g.author;
        self.difficulty = g.difficulty;
        self.gas_limit = g.gas_limit;
        self.gas_used = g.gas_used;
        self.timestamp = g.timestamp;
        self.extra_data = g.extra_data;
        self.seal_rlp = seal_rlp;
        self.base_fee = g.base_fee;
    }

    /// Alter the value of the genesis state.
    pub fn set_genesis_state(&mut self, s: PodState) -> Result<(), Error> {
        self.genesis_state = s;
        let _ = self.run_constructors(
            &Default::default(),
            BasicBackend(journaldb::new_memory_db()),
        )?;

        Ok(())
    }

    /// Return genesis state as Plain old data.
    pub fn genesis_state(&self) -> &PodState {
        &self.genesis_state
    }

    /// Returns `false` if the memoized state root is invalid. `true` otherwise.
    pub fn is_state_root_valid(&self) -> bool {
        // TODO: get rid of this function and ensure state root always is valid.
        // we're mostly there, but `self.genesis_state.root()` doesn't encompass
        // post-constructor state.
        *self.state_root_memo.read() == self.genesis_state.root()
    }

    /// Ensure that the given state DB has the trie nodes in for the genesis state.
    pub fn ensure_db_good<T: Backend>(&self, db: T, factories: &Factories) -> Result<T, Error> {
        if db.as_hash_db().contains(&self.state_root()) {
            return Ok(db);
        }

        // TODO: could optimize so we don't re-run, but `ensure_db_good` is barely ever
        // called anyway.
        let db = self.run_constructors(factories, db)?;
        Ok(db)
    }

    /// Loads just the state machine from a json file.
    pub fn load_machine<R: Read>(reader: R) -> Result<EthereumMachine, String> {
        ethjson::spec::Spec::load(reader)
            .map_err(fmt_err)
            .map(load_machine_from)
    }

    /// Loads spec from json file. Provide factories for executing contracts and ensuring
    /// storage goes to the right place.
    pub fn load<'a, T: Into<SpecParams<'a>>, R>(params: T, reader: R) -> Result<Self, String>
    where
        R: Read,
    {
        ethjson::spec::Spec::load(reader)
            .map_err(fmt_err)
            .and_then(|x| load_from(params.into(), x).map_err(fmt_err))
    }

    /// initialize genesis epoch data, using in-memory database for
    /// constructor.
    pub fn genesis_epoch_data(&self) -> Result<Vec<u8>, String> {
        use types::transaction::{Action, Transaction, TypedTransaction};

        let genesis = self.genesis_header();

        let factories = Default::default();
        let mut db = journaldb::new(
            Arc::new(db::InMemoryWithMetrics::create(0)),
            journaldb::Algorithm::Archive,
            None,
        );

        self.ensure_db_good(BasicBackend(db.as_hash_db_mut()), &factories)
            .map_err(|e| format!("Unable to initialize genesis state: {}", e))?;

        let call = |a, d| {
            let mut db = db.boxed_clone();
            let env_info = ::evm::EnvInfo {
                number: 0,
                author: *genesis.author(),
                timestamp: genesis.timestamp(),
                difficulty: *genesis.difficulty(),
                gas_limit: U256::max_value(),
                last_hashes: Arc::new(Vec::new()),
                gas_used: 0.into(),
                base_fee: genesis.base_fee(),
            };

            let from = Address::default();
            let tx = TypedTransaction::Legacy(Transaction {
                nonce: self.engine.account_start_nonce(0),
                action: Action::Call(a),
                gas: U256::max_value(),
                gas_price: U256::default(),
                value: U256::default(),
                data: d,
            })
            .fake_sign(from);

            let res = ::state::prove_transaction_virtual(
                db.as_hash_db_mut(),
                *genesis.state_root(),
                &tx,
                self.engine.machine(),
                &env_info,
                factories.clone(),
            );

            res.map(|(out, proof)| (out, proof.into_iter().map(|x| x.into_vec()).collect()))
                .ok_or_else(|| "Failed to prove call: insufficient state".into())
        };

        self.engine.genesis_epoch_data(&genesis, &call)
    }

    /// Create a new Spec with InstantSeal consensus which does internal sealing (not requiring
    /// work).
    pub fn new_instant() -> Spec {
        load_bundled!("instant_seal")
    }

    /// Create a new Spec which conforms to the Frontier-era Morden chain except that it's a
    /// NullEngine consensus.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test() -> Spec {
        load_bundled!("test/null_morden")
    }

    /// Create the EthereumMachine corresponding to Spec::new_test.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_machine() -> EthereumMachine {
        load_machine_bundled!("test/null_morden")
    }

    /// Create a new Spec which conforms to the Frontier-era Morden chain except that it's a NullEngine consensus with applying reward on block close.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_with_reward() -> Spec {
        load_bundled!("test/null_morden_with_reward")
    }

    /// Create a new Spec which conforms to the Frontier-era Morden chain except that it's a NullEngine consensus with finality.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_with_finality() -> Spec {
        load_bundled!("test/null_morden_with_finality")
    }

    /// Create a new Spec which is a NullEngine consensus with a premine of address whose
    /// secret is keccak('').
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_null() -> Spec {
        load_bundled!("test/null")
    }

    /// Create a new Spec which constructs a contract at address 5 with storage at 0 equal to 1.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_constructor() -> Spec {
        load_bundled!("test/constructor")
    }

    /// Create a new Spec which is a NullEngine consensus with EIP3607 transition equal to 2,
    /// and with a contract at address '0x71562b71999873DB5b286dF957af199Ec94617F7'.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_eip3607() -> Self {
        load_bundled!("test/eip3607_test")
    }

    /// Create a new Spec with Autority Round randomness contract
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_round_randomness_contract() -> Spec {
        load_bundled!("test/authority_round_randomness_contract")
    }

    /// Create a new Spec with AuthorityRound consensus which does internal sealing (not
    /// requiring work).
    /// Accounts with secrets keccak("0") and keccak("1") are the validators.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_round() -> Self {
        load_bundled!("test/authority_round")
    }

    /// Create a new Spec with AuthorityRound consensus which does internal sealing (not
    /// requiring work) with empty step messages enabled.
    /// Accounts with secrets keccak("0") and keccak("1") are the validators.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_round_empty_steps() -> Self {
        load_bundled!("test/authority_round_empty_steps")
    }

    /// Create a new Spec with AuthorityRound consensus (with empty steps) using a block reward
    /// contract. The contract source code can be found at:
    /// https://github.com/openethereum/block-reward/blob/daf7d44383b6cdb11cb6b953b018648e2b027cfb/contracts/ExampleBlockReward.sol
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_test_round_block_reward_contract() -> Self {
        load_bundled!("test/authority_round_block_reward_contract")
    }

    pub fn new_test_round_rewrite_bytecode_transitions() -> Self {
        load_bundled!("test/authority_round_rewrite_bytecode_transitions")
    }

    /// TestList.sol used in both specs: https://github.com/paritytech/contracts/pull/30/files (link not valid)
    /// Accounts with secrets keccak("0") and keccak("1") are initially the validators.
    /// Create a new Spec with BasicAuthority which uses a contract at address 5 to determine
    /// the current validators using `getValidators`.
    /// Second validator can be removed with
    /// "0xbfc708a000000000000000000000000082a978b3f5962a5b0957d9ee9eef472ee55b42f1" and added
    /// back in using
    /// "0x4d238c8e00000000000000000000000082a978b3f5962a5b0957d9ee9eef472ee55b42f1".
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_validator_safe_contract() -> Self {
        load_bundled!("test/validator_safe_contract")
    }

    /// The same as the `safeContract`, but allows reporting and uses AuthorityRound.
    /// Account is marked with `reportBenign` it can be checked as disliked with "0xd8f2e0bf".
    /// Validator can be removed with `reportMalicious`.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_validator_contract() -> Self {
        load_bundled!("test/validator_contract")
    }

    /// Create a new Spec with BasicAuthority which uses multiple validator sets changing with
    /// height.
    /// Account with secrets keccak("0") is the validator for block 1 and with keccak("1")
    /// onwards.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn new_validator_multi() -> Self {
        load_bundled!("test/validator_multi")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethereum_types::{H160, H256};
    use state::State;
    use std::str::FromStr;
    use tempdir::TempDir;
    use test_helpers::get_temp_state_db;
    use types::{view, views::BlockView};

    #[test]
    fn test_load_empty() {
        let tempdir = TempDir::new("").unwrap();
        assert!(Spec::load(&tempdir.path(), &[] as &[u8]).is_err());
    }

    #[test]
    fn test_chain() {
        let test_spec = Spec::new_test();

        assert_eq!(
            test_spec.state_root(),
            H256::from_str("f3f4696bbf3b3b07775128eb7a3763279a394e382130f27c21e70233e04946a9")
                .unwrap()
        );
        let genesis = test_spec.genesis_block();
        assert_eq!(
            view!(BlockView, &genesis).header_view().hash(),
            H256::from_str("0cd786a2425d16f152c658316c423e6ce1181e15c3295826d7c9904cba9ce303")
                .unwrap()
        );
    }

    #[test]
    fn genesis_constructor() {
        let _ = ::env_logger::try_init();
        let spec = Spec::new_test_constructor();
        let db = spec
            .ensure_db_good(get_temp_state_db(), &Default::default())
            .unwrap();
        let state = State::from_existing(
            db.boxed_clone(),
            spec.state_root(),
            spec.engine.account_start_nonce(0),
            Default::default(),
        )
        .unwrap();
        let expected =
            H256::from_str("0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();
        let address = H160::from_str("0000000000000000000000000000000000001337").unwrap();

        assert_eq!(state.storage_at(&address, &H256::zero()).unwrap(), expected);
        assert_eq!(state.balance(&address).unwrap(), 1.into());
    }
}
