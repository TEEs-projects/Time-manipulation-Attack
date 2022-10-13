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
use client::EvmTestClient;
use ethjson;
use std::path::Path;
use transaction_ext::Transaction;
use types::{
    header::Header,
    transaction::{TypedTransaction, UnverifiedTransaction},
};

pub fn json_transaction_test<H: FnMut(&str, HookType)>(
    path: &Path,
    json_data: &[u8],
    start_stop_hook: &mut H,
) -> Vec<String> {
    // Block number used to run the tests.
    // Make sure that all the specified features are activated.
    const BLOCK_NUMBER: u64 = 0x6ffffffffffffe;

    let tests = ethjson::transaction::Test::load(json_data).expect(&format!(
        "Could not parse JSON transaction test data from {}",
        path.display()
    ));
    let mut failed = Vec::new();
    for (name, test) in tests.into_iter() {
        if !super::debug_include_test(&name) {
            continue;
        }

        start_stop_hook(&name, HookType::OnStart);

        println!("   - tx: {} ", name);

        for (spec_name, result) in test.post_state {
            let spec = match EvmTestClient::spec_from_json(&spec_name) {
                Some(spec) => spec,
                None => {
                    failed.push(format!("{}-{:?} (missing spec)", name, spec_name));
                    continue;
                }
            };

            let mut fail_unless = |cond: bool, title: &str| {
                if !cond {
                    failed.push(format!("{}-{:?}", name, spec_name));
                    println!(
                        "Transaction failed: {:?}-{:?}: {:?}",
                        name, spec_name, title
                    );
                }
            };

            let rlp: Vec<u8> = test.rlp.clone().into();
            let res = TypedTransaction::decode(&rlp)
                .map_err(::error::Error::from)
                .and_then(|t: UnverifiedTransaction| {
                    let mut header: Header = Default::default();
                    // Use high enough number to activate all required features.
                    header.set_number(BLOCK_NUMBER);

                    let minimal = t
                        .tx()
                        .gas_required(&spec.engine.schedule(header.number()))
                        .into();
                    if t.tx().gas < minimal {
                        return Err(::types::transaction::Error::InsufficientGas {
                            minimal,
                            got: t.tx().gas,
                        }
                        .into());
                    }
                    spec.engine.verify_transaction_basic(&t, &header)?;
                    Ok(spec.engine.verify_transaction_unordered(t, &header)?)
                });

            match (res, result.hash, result.sender) {
                (Ok(t), Some(hash), Some(sender)) => {
                    fail_unless(t.sender() == sender.into(), "sender mismatch");
                    fail_unless(t.hash() == hash.into(), "hash mismatch");
                }
                (Err(_), None, None) => {}
                data => {
                    fail_unless(false, &format!("Validity different: {:?}", data));
                }
            }
        }

        start_stop_hook(&name, HookType::OnStop);
    }

    for f in &failed {
        println!("FAILED: {:?}", f);
    }
    failed
}
