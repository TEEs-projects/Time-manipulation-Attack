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

use super::test_common::*;
use bytes::Bytes;
use ethereum_types::BigEndianHash;
use ethjson;
use ethtrie;
use evm::Finalize;
use executive::*;
use externalities::*;
use hash::keccak;
use machine::EthereumMachine as Machine;
use rlp::RlpStream;
use state::{Backend as StateBackend, State, Substate};
use std::{path::Path, sync::Arc};
use test_helpers::get_temp_state;
use trace::{NoopTracer, NoopVMTracer, Tracer, VMTracer};
use vm::{
    self, ActionParams, CallType, ContractCreateResult, CreateContractAddress, EnvInfo, Ext,
    MessageCallResult, ReturnData, Schedule,
};

use super::HookType;

#[derive(Debug, PartialEq, Clone)]
struct CallCreate {
    data: Bytes,
    destination: Option<Address>,
    gas_limit: U256,
    value: U256,
}

impl From<ethjson::vm::Call> for CallCreate {
    fn from(c: ethjson::vm::Call) -> Self {
        let dst: Option<ethjson::hash::Address> = c.destination.into();
        CallCreate {
            data: c.data.into(),
            destination: dst.map(Into::into),
            gas_limit: c.gas_limit.into(),
            value: c.value.into(),
        }
    }
}

/// Tiny wrapper around executive externalities.
/// Stores callcreates.
struct TestExt<'a, T: 'a, V: 'a, B: 'a>
where
    T: Tracer,
    V: VMTracer,
    B: StateBackend,
{
    ext: Externalities<'a, T, V, B>,
    callcreates: Vec<CallCreate>,
    nonce: U256,
    sender: Address,
}

impl<'a, T: 'a, V: 'a, B: 'a> TestExt<'a, T, V, B>
where
    T: Tracer,
    V: VMTracer,
    B: StateBackend,
{
    fn new(
        state: &'a mut State<B>,
        info: &'a EnvInfo,
        machine: &'a Machine,
        schedule: &'a Schedule,
        depth: usize,
        origin_info: &'a OriginInfo,
        substate: &'a mut Substate,
        output: OutputPolicy,
        address: Address,
        tracer: &'a mut T,
        vm_tracer: &'a mut V,
    ) -> ethtrie::Result<Self> {
        let static_call = false;
        Ok(TestExt {
            nonce: state.nonce(&address)?,
            ext: Externalities::new(
                state,
                info,
                machine,
                schedule,
                depth,
                0,
                origin_info,
                substate,
                output,
                tracer,
                vm_tracer,
                static_call,
            ),
            callcreates: vec![],
            sender: address,
        })
    }
}

impl<'a, T: 'a, V: 'a, B: 'a> Ext for TestExt<'a, T, V, B>
where
    T: Tracer,
    V: VMTracer,
    B: StateBackend,
{
    fn storage_at(&self, key: &H256) -> vm::Result<H256> {
        self.ext.storage_at(key)
    }

    fn initial_storage_at(&self, key: &H256) -> vm::Result<H256> {
        self.ext.initial_storage_at(key)
    }

    fn set_storage(&mut self, key: H256, value: H256) -> vm::Result<()> {
        self.ext.set_storage(key, value)
    }

    fn exists(&self, address: &Address) -> vm::Result<bool> {
        self.ext.exists(address)
    }

    fn exists_and_not_null(&self, address: &Address) -> vm::Result<bool> {
        self.ext.exists_and_not_null(address)
    }

    fn balance(&self, address: &Address) -> vm::Result<U256> {
        self.ext.balance(address)
    }

    fn origin_balance(&self) -> vm::Result<U256> {
        self.ext.origin_balance()
    }

    fn blockhash(&mut self, number: &U256) -> H256 {
        self.ext.blockhash(number)
    }

    fn create(
        &mut self,
        gas: &U256,
        value: &U256,
        code: &[u8],
        address: CreateContractAddress,
        _trap: bool,
    ) -> Result<ContractCreateResult, vm::TrapKind> {
        self.callcreates.push(CallCreate {
            data: code.to_vec(),
            destination: None,
            gas_limit: *gas,
            value: *value,
        });
        let contract_address = contract_address(address, &self.sender, &self.nonce, &code).0;
        Ok(ContractCreateResult::Created(contract_address, *gas))
    }

    fn calc_address(&self, code: &[u8], address: CreateContractAddress) -> Option<Address> {
        Some(contract_address(address, &self.sender, &self.nonce, &code).0)
    }

    fn call(
        &mut self,
        gas: &U256,
        _sender_address: &Address,
        receive_address: &Address,
        value: Option<U256>,
        data: &[u8],
        _code_address: &Address,
        _call_type: CallType,
        _trap: bool,
    ) -> Result<MessageCallResult, vm::TrapKind> {
        self.callcreates.push(CallCreate {
            data: data.to_vec(),
            destination: Some(receive_address.clone()),
            gas_limit: *gas,
            value: value.unwrap(),
        });
        Ok(MessageCallResult::Success(*gas, ReturnData::empty()))
    }

    fn extcode(&self, address: &Address) -> vm::Result<Option<Arc<Bytes>>> {
        self.ext.extcode(address)
    }

    fn extcodesize(&self, address: &Address) -> vm::Result<Option<usize>> {
        self.ext.extcodesize(address)
    }

    fn extcodehash(&self, address: &Address) -> vm::Result<Option<H256>> {
        self.ext.extcodehash(address)
    }

    fn log(&mut self, topics: Vec<H256>, data: &[u8]) -> vm::Result<()> {
        self.ext.log(topics, data)
    }

    fn ret(self, gas: &U256, data: &ReturnData, apply_state: bool) -> Result<U256, vm::Error> {
        self.ext.ret(gas, data, apply_state)
    }

    fn suicide(&mut self, refund_address: &Address) -> vm::Result<()> {
        self.ext.suicide(refund_address)
    }

    fn schedule(&self) -> &Schedule {
        self.ext.schedule()
    }

    fn env_info(&self) -> &EnvInfo {
        self.ext.env_info()
    }

    fn chain_id(&self) -> u64 {
        0
    }

    fn depth(&self) -> usize {
        0
    }

    fn is_static(&self) -> bool {
        false
    }

    fn add_sstore_refund(&mut self, value: usize) {
        self.ext.add_sstore_refund(value)
    }

    fn sub_sstore_refund(&mut self, value: usize) {
        self.ext.sub_sstore_refund(value)
    }

    fn al_is_enabled(&self) -> bool {
        self.ext.al_is_enabled()
    }

    fn al_contains_storage_key(&self, address: &Address, key: &H256) -> bool {
        self.ext.al_contains_storage_key(address, key)
    }

    fn al_insert_storage_key(&mut self, address: Address, key: H256) {
        self.ext.al_insert_storage_key(address, key)
    }

    fn al_contains_address(&self, address: &Address) -> bool {
        self.ext.al_contains_address(address)
    }

    fn al_insert_address(&mut self, address: Address) {
        self.ext.al_insert_address(address)
    }
}

/// run an json executive test
pub fn json_executive_test<H: FnMut(&str, HookType)>(
    path: &Path,
    json_data: &[u8],
    start_stop_hook: &mut H,
) -> Vec<String> {
    let tests = ethjson::vm::Test::load(json_data).expect(&format!(
        "Could not parse JSON executive test data from {}",
        path.display()
    ));
    let mut failed = Vec::new();

    for (name, vm) in tests.into_iter() {
        if !super::debug_include_test(&name) {
            continue;
        }

        start_stop_hook(&format!("{}", name), HookType::OnStart);

        let mut fail = false;

        let mut fail_unless = |cond: bool, s: &str| {
            if !cond && !fail {
                failed.push(format!("{}: {}", name, s));
                fail = true
            }
        };

        macro_rules! try_fail {
            ($e: expr) => {
                match $e {
                    Ok(x) => x,
                    Err(e) => {
                        let msg = format!("Internal error: {}", e);
                        fail_unless(false, &msg);
                        continue;
                    }
                }
            };
        }

        let out_of_gas = vm.out_of_gas();
        let mut state = get_temp_state();
        state.populate_from(From::from(vm.pre_state.clone()));
        let info: EnvInfo = From::from(vm.env);
        let machine = {
            let mut machine = ::ethereum::new_frontier_test_machine();
            machine.set_schedule_creation_rules(Box::new(move |s, _| s.max_depth = 1));
            machine
        };

        let params = ActionParams::from(vm.transaction);

        let mut substate = Substate::new();
        let mut tracer = NoopTracer;
        let mut vm_tracer = NoopVMTracer;
        let vm_factory = state.vm_factory();
        let origin_info = OriginInfo::from(&params);

        // execute
        let (res, callcreates) = {
            let schedule = machine.schedule(info.number);
            let mut ex = try_fail!(TestExt::new(
                &mut state,
                &info,
                &machine,
                &schedule,
                0,
                &origin_info,
                &mut substate,
                OutputPolicy::Return,
                params.address.clone(),
                &mut tracer,
                &mut vm_tracer,
            ));
            let evm = vm_factory.create(params, &schedule, 0);
            let res = evm
                .exec(&mut ex)
                .ok()
                .expect("TestExt never trap; resume error never happens; qed");
            // a return in finalize will not alter callcreates
            let callcreates = ex.callcreates.clone();
            (res.finalize(ex), callcreates)
        };

        let output = match &res {
            Ok(res) => res.return_data.to_vec(),
            Err(_) => Vec::new(),
        };

        let log_hash = {
            let mut rlp = RlpStream::new_list(substate.logs.len());
            for l in &substate.logs {
                rlp.append(l);
            }
            keccak(&rlp.drain())
        };

        match res {
            Err(_) => fail_unless(out_of_gas, "didn't expect to run out of gas."),
            Ok(res) => {
                fail_unless(!out_of_gas, "expected to run out of gas.");
                fail_unless(
                    Some(res.gas_left) == vm.gas_left.map(Into::into),
                    "gas_left is incorrect",
                );
                let vm_output: Option<Vec<u8>> = vm.output.map(Into::into);
                fail_unless(Some(output) == vm_output, "output is incorrect");
                fail_unless(Some(log_hash) == vm.logs.map(|h| h.0), "logs are incorrect");

                for (address, account) in vm.post_state.unwrap().into_iter() {
                    let address = address.into();
                    if let Some(code) = account.code {
                        let code: Vec<u8> = code.into();
                        let found_code = try_fail!(state.code(&address));
                        fail_unless(
                            found_code
                                .as_ref()
                                .map_or_else(|| code.is_empty(), |c| &**c == &code),
                            "code is incorrect",
                        );
                    }
                    let found_balance = try_fail!(state.balance(&address));
                    let found_nonce = try_fail!(state.nonce(&address));
                    if let Some(balance) = account.balance {
                        fail_unless(found_balance == balance.into(), "balance is incorrect");
                    }
                    if let Some(nonce) = account.nonce {
                        fail_unless(found_nonce == nonce.into(), "nonce is incorrect");
                    }
                    if let Some(storage) = account.storage {
                        for (k, v) in storage {
                            let key: U256 = k.into();
                            let value: U256 = v.into();
                            let found_storage = try_fail!(
                                state.storage_at(&address, &BigEndianHash::from_uint(&key))
                            );
                            fail_unless(
                                found_storage == BigEndianHash::from_uint(&value),
                                "storage is incorrect",
                            );
                        }
                    }
                }

                let calls: Option<Vec<CallCreate>> =
                    vm.calls.map(|c| c.into_iter().map(From::from).collect());
                fail_unless(Some(callcreates) == calls, "callcreates does not match");
            }
        };

        if fail {
            println!("   - vm: {:?}...FAILED", name);
        } else {
            println!("   - vm: {:?}...OK", name);
        }

        start_stop_hook(&format!("{}", name), HookType::OnStop);
    }

    failed
}
