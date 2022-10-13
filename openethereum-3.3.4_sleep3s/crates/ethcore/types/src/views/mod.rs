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

//! Block oriented views onto rlp.

#[macro_use]
mod view_rlp;
mod block;
mod body;
mod header;
mod typed_transaction;

pub use self::{
    block::BlockView, body::BodyView, header::HeaderView, typed_transaction::TypedTransactionView,
    view_rlp::ViewRlp,
};

#[cfg(test)]
mod tests {
    use super::HeaderView;

    #[test]
    #[should_panic]
    fn should_include_file_line_number_in_panic_for_invalid_rlp() {
        let _ = view!(HeaderView, &[]).parent_hash();
    }
}
