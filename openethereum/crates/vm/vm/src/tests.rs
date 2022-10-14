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

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::access_list::AccessList;
use bytes::Bytes;
use error::TrapKind;
use ethereum_types::{Address, H256, U256};
use hash::keccak;
use CallType;
use ContractCreateResult;
use CreateContractAddress;
use EnvInfo;
use Ext;
use GasLeft;
use MessageCallResult;
use Result;
use ReturnData;
use Schedule;

pub struct FakeLogEntry {
    pub topics: Vec<H256>,
    pub data: Bytes,
}

#[derive(PartialEq, Eq, Hash, Debug)]
pub enum FakeCallType {
    Call,
    Create,
}

#[derive(PartialEq, Eq, Hash, Debug)]
pub struct FakeCall {
    pub call_type: FakeCallType,
    pub create_scheme: Option<CreateContractAddress>,
    pub gas: U256,
    pub sender_address: Option<Address>,
    pub receive_address: Option<Address>,
    pub value: Option<U256>,
    pub data: Bytes,
    pub code_address: Option<Address>,
}

/// Fake externalities test structure.
///
/// Can't do recursive calls.
#[derive(Default)]
pub struct FakeExt {
    pub initial_store: HashMap<H256, H256>,
    pub store: HashMap<H256, H256>,
    pub suicides: HashSet<Address>,
    pub calls: HashSet<FakeCall>,
    pub sstore_clears: i128,
    pub depth: usize,
    pub blockhashes: HashMap<U256, H256>,
    pub codes: HashMap<Address, Arc<Bytes>>,
    pub logs: Vec<FakeLogEntry>,
    pub info: EnvInfo,
    pub schedule: Schedule,
    pub balances: HashMap<Address, U256>,
    pub tracing: bool,
    pub is_static: bool,
    pub access_list: AccessList,

    chain_id: u64,
}

// similar to the normal `finalize` function, but ignoring NeedsReturn.
pub fn test_finalize(res: Result<GasLeft>) -> Result<U256> {
    match res {
        Ok(GasLeft::Known(gas)) => Ok(gas),
        Ok(GasLeft::NeedsReturn { .. }) => unimplemented!(), // since ret is unimplemented.
        Err(e) => Err(e),
    }
}

impl FakeExt {
    /// New fake externalities
    pub fn new() -> Self {
        FakeExt::default()
    }

    /// New fake externalities with byzantium schedule rules
    pub fn new_byzantium() -> Self {
        let mut ext = FakeExt::default();
        ext.schedule = Schedule::new_byzantium();
        ext
    }

    /// New fake externalities with constantinople schedule rules
    pub fn new_constantinople() -> Self {
        let mut ext = FakeExt::default();
        ext.schedule = Schedule::new_constantinople();
        ext
    }

    /// New fake externalities with Istanbul schedule rules
    pub fn new_istanbul() -> Self {
        let mut ext = FakeExt::default();
        ext.schedule = Schedule::new_istanbul();
        ext
    }

    /// New fake externalities with Berlin schedule rules
    pub fn new_berlin(from: Address, to: Address, builtins: &[Address]) -> Self {
        let mut ext = FakeExt::default();
        ext.schedule = Schedule::new_berlin();
        ext.access_list.enable();
        ext.access_list.insert_address(from);
        ext.access_list.insert_address(to);
        for builtin in builtins {
            ext.access_list.insert_address(*builtin);
        }
        ext
    }

    /// New fake externalities with London schedule rules
    pub fn new_london(from: Address, to: Address, builtins: &[Address]) -> Self {
        let mut ext = FakeExt::new_berlin(from, to, builtins);
        ext.schedule = Schedule::new_london();
        ext
    }

    /// Alter fake externalities to allow wasm
    pub fn with_wasm(mut self) -> Self {
        self.schedule.wasm = Some(Default::default());
        self
    }

    /// Set chain ID
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    pub fn set_initial_storage(&mut self, key: H256, value: H256) {
        self.initial_store.insert(key, value);
    }

    /// Fill the storage before the transaction with `data`, i. e. set
    /// both original and current values, beginning from address 0.
    pub fn prefill(&mut self, data: &[u64]) {
        for (k, v) in data.iter().enumerate() {
            let key = H256::from_low_u64_be(k as u64);
            let value = H256::from_low_u64_be(*v);
            self.set_initial_storage(key, value);
            self.set_storage(key, value)
                .expect("FakeExt::set_storage() never returns an Err.");
        }
    }
}

impl Ext for FakeExt {
    fn initial_storage_at(&self, key: &H256) -> Result<H256> {
        match self.initial_store.get(key) {
            Some(value) => Ok(*value),
            None => Ok(H256::default()),
        }
    }

    fn storage_at(&self, key: &H256) -> Result<H256> {
        Ok(self.store.get(key).unwrap_or(&H256::default()).clone())
    }

    fn set_storage(&mut self, key: H256, value: H256) -> Result<()> {
        self.store.insert(key, value);
        Ok(())
    }

    fn exists(&self, address: &Address) -> Result<bool> {
        Ok(self.balances.contains_key(address))
    }

    fn exists_and_not_null(&self, address: &Address) -> Result<bool> {
        Ok(self.balances.get(address).map_or(false, |b| !b.is_zero()))
    }

    fn origin_balance(&self) -> Result<U256> {
        unimplemented!()
    }

    fn balance(&self, address: &Address) -> Result<U256> {
        Ok(self.balances.get(address).cloned().unwrap_or(U256::zero()))
    }

    fn blockhash(&mut self, number: &U256) -> H256 {
        self.blockhashes
            .get(number)
            .unwrap_or(&H256::default())
            .clone()
    }

    fn create(
        &mut self,
        gas: &U256,
        value: &U256,
        code: &[u8],
        address: CreateContractAddress,
        _trap: bool,
    ) -> ::std::result::Result<ContractCreateResult, TrapKind> {
        self.calls.insert(FakeCall {
            call_type: FakeCallType::Create,
            create_scheme: Some(address),
            gas: *gas,
            sender_address: None,
            receive_address: None,
            value: Some(*value),
            data: code.to_vec(),
            code_address: None,
        });
        // TODO: support traps in testing.
        Ok(ContractCreateResult::Failed)
    }

    fn calc_address(&self, _code: &[u8], _address: CreateContractAddress) -> Option<Address> {
        None
    }

    fn call(
        &mut self,
        gas: &U256,
        sender_address: &Address,
        receive_address: &Address,
        value: Option<U256>,
        data: &[u8],
        code_address: &Address,
        _call_type: CallType,
        _trap: bool,
    ) -> ::std::result::Result<MessageCallResult, TrapKind> {
        self.calls.insert(FakeCall {
            call_type: FakeCallType::Call,
            create_scheme: None,
            gas: *gas,
            sender_address: Some(sender_address.clone()),
            receive_address: Some(receive_address.clone()),
            value: value,
            data: data.to_vec(),
            code_address: Some(code_address.clone()),
        });
        // TODO: support traps in testing.
        Ok(MessageCallResult::Success(*gas, ReturnData::empty()))
    }

    fn extcode(&self, address: &Address) -> Result<Option<Arc<Bytes>>> {
        Ok(self.codes.get(address).cloned())
    }

    fn extcodesize(&self, address: &Address) -> Result<Option<usize>> {
        Ok(self.codes.get(address).map(|c| c.len()))
    }

    fn extcodehash(&self, address: &Address) -> Result<Option<H256>> {
        Ok(self.codes.get(address).map(|c| keccak(c.as_ref())))
    }

    fn log(&mut self, topics: Vec<H256>, data: &[u8]) -> Result<()> {
        self.logs.push(FakeLogEntry {
            topics,
            data: data.to_vec(),
        });
        Ok(())
    }

    fn ret(self, _gas: &U256, _data: &ReturnData, _apply_state: bool) -> Result<U256> {
        unimplemented!();
    }

    fn suicide(&mut self, refund_address: &Address) -> Result<()> {
        self.suicides.insert(refund_address.clone());
        Ok(())
    }

    fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    fn env_info(&self) -> &EnvInfo {
        &self.info
    }

    fn chain_id(&self) -> u64 {
        self.chain_id
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn is_static(&self) -> bool {
        self.is_static
    }

    fn add_sstore_refund(&mut self, value: usize) {
        self.sstore_clears += value as i128;
    }

    fn sub_sstore_refund(&mut self, value: usize) {
        self.sstore_clears -= value as i128;
    }

    fn trace_next_instruction(&mut self, _pc: usize, _instruction: u8, _gas: U256) -> bool {
        self.tracing
    }

    fn al_is_enabled(&self) -> bool {
        self.access_list.is_enabled()
    }

    fn al_contains_storage_key(&self, address: &Address, key: &H256) -> bool {
        self.access_list.contains_storage_key(address, key)
    }

    fn al_insert_storage_key(&mut self, address: Address, key: H256) {
        self.access_list.insert_storage_key(address, key)
    }

    fn al_contains_address(&self, address: &Address) -> bool {
        self.access_list.contains_address(address)
    }

    fn al_insert_address(&mut self, address: Address) {
        self.access_list.insert_address(address)
    }
}
