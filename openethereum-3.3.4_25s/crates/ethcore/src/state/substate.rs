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

//! Execution environment substate.
use super::CleanupMode;
use ethereum_types::Address;
use evm::{CleanDustMode, Schedule};
use std::collections::HashSet;
use types::log_entry::LogEntry;
use vm::access_list::AccessList;

/// State changes which should be applied in finalize,
/// after transaction is fully executed.
#[derive(Debug, Default)]
pub struct Substate {
    /// Any accounts that have suicided.
    pub suicides: HashSet<Address>,

    /// Any accounts that are touched.
    pub touched: HashSet<Address>,

    /// Any logs.
    pub logs: Vec<LogEntry>,

    /// Refund counter of SSTORE.
    pub sstore_clears_refund: i128,

    /// Created contracts.
    pub contracts_created: Vec<Address>,

    /// List of accesses addresses and slots
    pub access_list: AccessList,
}

impl Substate {
    /// Creates new substate.
    pub fn new() -> Self {
        Substate::default()
    }
    /// Creates a new substate from an access list
    pub fn from_access_list(access_list: &AccessList) -> Self {
        Self {
            suicides: HashSet::default(),
            touched: HashSet::default(),
            logs: Vec::default(),
            sstore_clears_refund: 0,
            contracts_created: Vec::default(),
            access_list: access_list.clone(),
        }
    }

    /// Merge secondary substate `s` into self, accruing each element correspondingly.
    pub fn accrue(&mut self, s: Substate) {
        self.suicides.extend(s.suicides);
        self.touched.extend(s.touched);
        self.logs.extend(s.logs);
        self.sstore_clears_refund += s.sstore_clears_refund;
        self.contracts_created.extend(s.contracts_created);
    }

    /// Get the cleanup mode object from this.
    pub fn to_cleanup_mode(&mut self, schedule: &Schedule) -> CleanupMode {
        match (
            schedule.kill_dust != CleanDustMode::Off,
            schedule.no_empty,
            schedule.kill_empty,
        ) {
            (false, false, _) => CleanupMode::ForceCreate,
            (false, true, false) => CleanupMode::NoEmpty,
            (false, true, true) | (true, _, _) => CleanupMode::TrackTouched(&mut self.touched),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Substate;
    use ethereum_types::Address;
    use types::log_entry::LogEntry;

    #[test]
    fn created() {
        let sub_state = Substate::new();
        assert_eq!(sub_state.suicides.len(), 0);
    }

    #[test]
    fn accrue() {
        let mut sub_state = Substate::new();
        sub_state
            .contracts_created
            .push(Address::from_low_u64_be(1u64));
        sub_state.logs.push(LogEntry {
            address: Address::from_low_u64_be(1u64),
            topics: vec![],
            data: vec![],
        });
        sub_state.sstore_clears_refund = (15000 * 5).into();
        sub_state.suicides.insert(Address::from_low_u64_be(10u64));

        let mut sub_state_2 = Substate::new();
        sub_state_2
            .contracts_created
            .push(Address::from_low_u64_be(2u64));
        sub_state_2.logs.push(LogEntry {
            address: Address::from_low_u64_be(1u64),
            topics: vec![],
            data: vec![],
        });
        sub_state_2.sstore_clears_refund = (15000 * 7).into();

        sub_state.accrue(sub_state_2);
        assert_eq!(sub_state.contracts_created.len(), 2);
        assert_eq!(sub_state.sstore_clears_refund, (15000 * 12).into());
        assert_eq!(sub_state.suicides.len(), 1);
    }
}
