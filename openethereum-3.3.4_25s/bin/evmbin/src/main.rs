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

//! OpenEthereum EVM interpreter binary.

#![warn(missing_docs)]

extern crate common_types as types;
extern crate ethcore;
extern crate ethjson;
extern crate rustc_hex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate docopt;
extern crate env_logger;
extern crate ethereum_types;
extern crate evm;
extern crate panic_hook;
extern crate parity_bytes as bytes;
extern crate vm;

#[cfg(test)]
#[macro_use]
extern crate pretty_assertions;

#[cfg(test)]
extern crate tempdir;

use bytes::Bytes;
use docopt::Docopt;
use ethcore::{json_tests, spec, TrieSpec};
use ethereum_types::{Address, U256};
use ethjson::spec::ForkSpec;
use evm::EnvInfo;
use rustc_hex::FromHex;
use std::{fmt, fs, path::PathBuf, sync::Arc};
use vm::{ActionParams, CallType};

mod display;
mod info;

use info::Informant;

const USAGE: &'static str = r#"
EVM implementation for Parity.
  Copyright 2015-2020 Parity Technologies (UK) Ltd.

Usage:
    openethereum-evm state-test <file> [--json --std-json --std-dump-json --only NAME --chain CHAIN --std-out-only --std-err-only --omit-storage-output --omit-memory-output]
    openethereum-evm stats [options]
    openethereum-evm stats-jsontests-vm <file>
    openethereum-evm [options]
    openethereum-evm [-h | --help]

Commands:
    state-test         Run a state test from a json file.
    stats              Execute EVM runtime code and return the statistics.
    stats-jsontests-vm Execute standard json-tests format VMTests and return
                       timing statistics in tsv format.

Transaction options:
    --code CODE        Contract code as hex (without 0x).
    --to ADDRESS       Recipient address (without 0x).
    --from ADDRESS     Sender address (without 0x).
    --input DATA       Input data as hex (without 0x).
    --gas GAS          Supplied gas as hex (without 0x).
    --gas-price WEI    Supplied gas price as hex (without 0x).

State test options:
    --chain CHAIN      Run only from specific chain name (i.e. one of EIP150, EIP158,
                       Frontier, Homestead, Byzantium, Constantinople,
                       ConstantinopleFix, Istanbul, EIP158ToByzantiumAt5, FrontierToHomesteadAt5,
                       HomesteadToDaoAt5, HomesteadToEIP150At5, Berlin, Yolo3).
    --only NAME        Runs only a single test matching the name.

General options:
    --json                    Display verbose results in JSON.
    --std-json                Display results in standardized JSON format.
    --std-err-only            With --std-json redirect to err output only.
    --std-out-only            With --std-json redirect to out output only.
    --omit-storage-output     With --std-json omit storage output.
    --omit-memory-output      With --std-json omit memory output.
    --std-dump-json           Display results in standardized JSON format
                              with additional state dump.

Display result state dump in standardized JSON format.
    --chain CHAIN      Chain spec file path.
    -h, --help         Display this message and exit.
"#;

fn main() {
    panic_hook::set_abort();
    env_logger::init();

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());

    let config = args.config();

    if args.cmd_state_test {
        run_state_test(args)
    } else if args.cmd_stats_jsontests_vm {
        run_stats_jsontests_vm(args)
    } else if args.flag_json {
        run_call(args, display::json::Informant::new(config))
    } else if args.flag_std_dump_json || args.flag_std_json {
        if args.flag_std_err_only {
            run_call(args, display::std_json::Informant::err_only(config))
        } else if args.flag_std_out_only {
            run_call(args, display::std_json::Informant::out_only(config))
        } else {
            run_call(args, display::std_json::Informant::new_default(config))
        };
    } else {
        run_call(args, display::simple::Informant::new(config))
    }
}

fn run_stats_jsontests_vm(args: Args) {
    use json_tests::HookType;
    use std::{
        collections::HashMap,
        time::{Duration, Instant},
    };

    let file = args.arg_file.expect("FILE (or PATH) is required");

    let mut timings: HashMap<String, (Instant, Option<Duration>)> = HashMap::new();

    {
        let mut record_time = |name: &str, typ: HookType| match typ {
            HookType::OnStart => {
                timings.insert(name.to_string(), (Instant::now(), None));
            }
            HookType::OnStop => {
                timings.entry(name.to_string()).and_modify(|v| {
                    v.1 = Some(v.0.elapsed());
                });
            }
        };
        for file_path in json_tests::find_json_files_recursive(&file) {
            let json_data = std::fs::read(&file_path).unwrap();
            json_tests::json_executive_test(&file_path, &json_data, &mut record_time);
        }
    }

    for (name, v) in timings {
        println!(
            "{}\t{}",
            name,
            display::as_micros(&v.1.expect("All hooks are called with OnStop; qed"))
        );
    }
}

fn run_state_test(args: Args) {
    use ethjson::state::test::Test;
    let config = args.config();
    let file = args.arg_file.expect("FILE is required");
    let mut file = match fs::File::open(&file) {
        Err(err) => die(format!("Unable to open: {:?}: {}", file, err)),
        Ok(file) => file,
    };
    let state_test = match Test::load(&mut file) {
        Err(err) => die(format!("Unable to load the test file: {}", err)),
        Ok(test) => test,
    };
    let only_test = args.flag_only.map(|s| s.to_lowercase());
    let only_chain = args.flag_chain.map(|s| s.to_lowercase());

    for (name, test) in state_test {
        if let Some(false) = only_test
            .as_ref()
            .map(|only_test| &name.to_lowercase() == only_test)
        {
            continue;
        }

        let multitransaction = test.transaction;
        let env_info: EnvInfo = test.env.into();
        let pre = test.pre_state.into();

        for (spec, states) in test.post_states {
            //hardcode base fee for part of the london tests, that miss base fee field in env
            let mut test_env = env_info.clone();
            if spec >= ForkSpec::London {
                if test_env.base_fee.is_none() {
                    test_env.base_fee = Some(0x0a.into());
                }
            }

            if let Some(false) = only_chain
                .as_ref()
                .map(|only_chain| &format!("{:?}", spec).to_lowercase() == only_chain)
            {
                continue;
            }
            for (idx, state) in states.into_iter().enumerate() {
                let post_root = state.hash.into();
                let transaction = multitransaction.select(&state.indexes);

                let trie_spec = if args.flag_std_dump_json {
                    TrieSpec::Fat
                } else {
                    TrieSpec::Secure
                };
                if args.flag_json {
                    info::run_transaction(
                        &name,
                        idx,
                        &spec,
                        &pre,
                        post_root,
                        &test_env,
                        transaction,
                        display::json::Informant::new(config),
                        trie_spec,
                    )
                } else if args.flag_std_dump_json || args.flag_std_json {
                    if args.flag_std_err_only {
                        info::run_transaction(
                            &name,
                            idx,
                            &spec,
                            &pre,
                            post_root,
                            &test_env,
                            transaction,
                            display::std_json::Informant::err_only(config),
                            trie_spec,
                        )
                    } else if args.flag_std_out_only {
                        info::run_transaction(
                            &name,
                            idx,
                            &spec,
                            &pre,
                            post_root,
                            &test_env,
                            transaction,
                            display::std_json::Informant::out_only(config),
                            trie_spec,
                        )
                    } else {
                        info::run_transaction(
                            &name,
                            idx,
                            &spec,
                            &pre,
                            post_root,
                            &test_env,
                            transaction,
                            display::std_json::Informant::new_default(config),
                            trie_spec,
                        )
                    }
                } else {
                    info::run_transaction(
                        &name,
                        idx,
                        &spec,
                        &pre,
                        post_root,
                        &test_env,
                        transaction,
                        display::simple::Informant::new(config),
                        trie_spec,
                    )
                }
            }
        }
    }
}

fn run_call<T: Informant>(args: Args, informant: T) {
    let from = arg(args.from(), "--from");
    let to = arg(args.to(), "--to");
    let code = arg(args.code(), "--code");
    let spec = arg(args.spec(), "--chain");
    let gas = arg(args.gas(), "--gas");
    let gas_price = arg(args.gas_price(), "--gas-price");
    let data = arg(args.data(), "--input");

    if code.is_none() && to == Address::default() {
        die("Either --code or --to is required.");
    }
    let mut params = ActionParams::default();
    if spec.engine.params().eip2929_transition == 0 {
        params.access_list.enable();
        params.access_list.insert_address(from);
        params.access_list.insert_address(to);
        for (builtin, _) in spec.engine.builtins() {
            params.access_list.insert_address(*builtin);
        }
    }

    params.call_type = if code.is_none() {
        CallType::Call
    } else {
        CallType::None
    };
    params.code_address = to;
    params.address = to;
    params.sender = from;
    params.origin = from;
    params.gas = gas;
    params.gas_price = gas_price;
    params.code = code.map(Arc::new);
    params.data = data;

    let mut sink = informant.clone_sink();
    let result = if args.flag_std_dump_json {
        info::run_action(&spec, params, informant, TrieSpec::Fat)
    } else {
        info::run_action(&spec, params, informant, TrieSpec::Secure)
    };
    T::finish(result, &mut sink);
}

#[derive(Debug, Deserialize)]
struct Args {
    cmd_stats: bool,
    cmd_state_test: bool,
    cmd_stats_jsontests_vm: bool,
    arg_file: Option<PathBuf>,
    flag_only: Option<String>,
    flag_from: Option<String>,
    flag_to: Option<String>,
    flag_code: Option<String>,
    flag_gas: Option<String>,
    flag_gas_price: Option<String>,
    flag_input: Option<String>,
    flag_chain: Option<String>,
    flag_json: bool,
    flag_std_json: bool,
    flag_std_dump_json: bool,
    flag_std_err_only: bool,
    flag_std_out_only: bool,
    flag_omit_storage_output: bool,
    flag_omit_memory_output: bool,
}

impl Args {
    pub fn gas(&self) -> Result<U256, String> {
        match self.flag_gas {
            Some(ref gas) => gas.parse().map_err(to_string),
            None => Ok(U256::from(u64::max_value())),
        }
    }

    pub fn gas_price(&self) -> Result<U256, String> {
        match self.flag_gas_price {
            Some(ref gas_price) => gas_price.parse().map_err(to_string),
            None => Ok(U256::zero()),
        }
    }

    pub fn from(&self) -> Result<Address, String> {
        match self.flag_from {
            Some(ref from) => from.parse().map_err(to_string),
            None => Ok(Address::default()),
        }
    }

    pub fn to(&self) -> Result<Address, String> {
        match self.flag_to {
            Some(ref to) => to.parse().map_err(to_string),
            None => Ok(Address::default()),
        }
    }

    pub fn code(&self) -> Result<Option<Bytes>, String> {
        match self.flag_code {
            Some(ref code) => code.from_hex().map(Some).map_err(to_string),
            None => Ok(None),
        }
    }

    pub fn data(&self) -> Result<Option<Bytes>, String> {
        match self.flag_input {
            Some(ref input) => input.from_hex().map_err(to_string).map(Some),
            None => Ok(None),
        }
    }

    pub fn spec(&self) -> Result<spec::Spec, String> {
        Ok(match self.flag_chain {
            Some(ref spec_name) => {
                let fork_spec: Result<ethjson::spec::ForkSpec, _> =
                    serde_json::from_str(&format!("{:?}", spec_name));
                if let Ok(fork_spec) = fork_spec {
                    ethcore::client::EvmTestClient::spec_from_json(&fork_spec)
                        .expect("this forkspec is not defined")
                } else {
                    let file = fs::File::open(spec_name).map_err(|e| format!("{}", e))?;
                    spec::Spec::load(&::std::env::temp_dir(), file)?
                }
            }
            None => ethcore::ethereum::new_foundation(&::std::env::temp_dir()),
        })
    }

    pub fn config(&self) -> display::config::Config {
        display::config::Config::new(self.flag_omit_storage_output, self.flag_omit_memory_output)
    }
}

fn arg<T>(v: Result<T, String>, param: &str) -> T {
    v.unwrap_or_else(|e| die(format!("Invalid {}: {}", param, e)))
}

fn to_string<T: fmt::Display>(msg: T) -> String {
    format!("{}", msg)
}

fn die<T: fmt::Display>(msg: T) -> ! {
    println!("{}", msg);
    ::std::process::exit(-1)
}

#[cfg(test)]
mod tests {
    use super::{Args, USAGE};
    use docopt::Docopt;
    use ethereum_types::Address;

    fn run<T: AsRef<str>>(args: &[T]) -> Args {
        Docopt::new(USAGE)
            .and_then(|d| d.argv(args.into_iter()).deserialize())
            .unwrap()
    }

    #[test]
    fn should_parse_all_the_options() {
        let args = run(&[
            "openethereum-evm",
            "--json",
            "--std-json",
            "--std-dump-json",
            "--gas",
            "1",
            "--gas-price",
            "2",
            "--from",
            "0000000000000000000000000000000000000003",
            "--to",
            "0000000000000000000000000000000000000004",
            "--code",
            "05",
            "--input",
            "06",
            "--chain",
            "./testfile",
            "--std-err-only",
            "--std-out-only",
        ]);

        assert_eq!(args.flag_json, true);
        assert_eq!(args.flag_std_json, true);
        assert_eq!(args.flag_std_dump_json, true);
        assert_eq!(args.flag_std_err_only, true);
        assert_eq!(args.flag_std_out_only, true);
        assert_eq!(args.gas(), Ok(1.into()));
        assert_eq!(args.gas_price(), Ok(2.into()));
        assert_eq!(args.from(), Ok(Address::from_low_u64_be(3)));
        assert_eq!(args.to(), Ok(Address::from_low_u64_be(4)));
        assert_eq!(args.code(), Ok(Some(vec![05])));
        assert_eq!(args.data(), Ok(Some(vec![06])));
        assert_eq!(args.flag_chain, Some("./testfile".to_owned()));
    }

    #[test]
    fn should_parse_state_test_command() {
        let args = run(&[
            "openethereum-evm",
            "state-test",
            "./file.json",
            "--chain",
            "homestead",
            "--only=add11",
            "--json",
            "--std-json",
            "--std-dump-json",
        ]);

        assert_eq!(args.cmd_state_test, true);
        assert!(args.arg_file.is_some());
        assert_eq!(args.flag_json, true);
        assert_eq!(args.flag_std_json, true);
        assert_eq!(args.flag_std_dump_json, true);
        assert_eq!(args.flag_chain, Some("homestead".to_owned()));
        assert_eq!(args.flag_only, Some("add11".to_owned()));
    }
}
