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

//! ethkey reexport to make documentation look pretty.
pub use _ethkey::*;
pub use crypto::publickey::Address;
use json;

impl Into<json::H160> for Address {
    fn into(self) -> json::H160 {
        let a: [u8; 20] = self.into();
        From::from(a)
    }
}

impl From<json::H160> for Address {
    fn from(json: json::H160) -> Self {
        let a: [u8; 20] = json.into();
        From::from(a)
    }
}

impl<'a> From<&'a json::H160> for Address {
    fn from(json: &'a json::H160) -> Self {
        let mut a = [0u8; 20];
        a.copy_from_slice(json);
        From::from(a)
    }
}
