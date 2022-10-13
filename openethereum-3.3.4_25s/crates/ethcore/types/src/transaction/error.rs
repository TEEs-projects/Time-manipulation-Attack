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

use std::{error, fmt};

use crate::crypto;
use ethereum_types::U256;
use rlp;
use unexpected::OutOfBounds;

#[derive(Debug, PartialEq, Clone)]
/// Errors concerning transaction processing.
pub enum Error {
    /// Transaction is already imported to the queue
    AlreadyImported,
    /// Transaction is not valid anymore (state already has higher nonce)
    Old,
    /// Transaction was not imported to the queue because limit has been reached.
    LimitReached,
    /// Transaction's gas price is below threshold.
    InsufficientGasPrice {
        /// Minimal expected gas price
        minimal: U256,
        /// Transaction gas price
        got: U256,
    },
    /// Transaction's max gas price is lower then block base fee.
    GasPriceLowerThanBaseFee {
        /// Transaction max gas price
        gas_price: U256,
        /// Current block base fee
        base_fee: U256,
    },
    /// Transaction has too low fee
    /// (there is already a transaction with the same sender-nonce but higher gas price)
    TooCheapToReplace {
        /// previous transaction's gas price
        prev: Option<U256>,
        /// new transaction's gas price
        new: Option<U256>,
    },
    /// Transaction's gas is below currently set minimal gas requirement.
    InsufficientGas {
        /// Minimal expected gas
        minimal: U256,
        /// Transaction gas
        got: U256,
    },
    /// Sender doesn't have enough funds to pay for this transaction
    InsufficientBalance {
        /// Senders balance
        balance: U256,
        /// Transaction cost
        cost: U256,
    },
    /// Transactions gas is higher then current gas limit
    GasLimitExceeded {
        /// Current gas limit
        limit: U256,
        /// Declared transaction gas
        got: U256,
    },
    /// Transaction's gas limit (aka gas) is invalid.
    InvalidGasLimit(OutOfBounds<U256>),
    /// Transaction sender is banned.
    SenderBanned,
    /// Transaction receipient is banned.
    RecipientBanned,
    /// Contract creation code is banned.
    CodeBanned,
    /// Invalid chain ID given.
    InvalidChainId,
    /// Not enough permissions given by permission contract.
    NotAllowed,
    /// Signature error
    InvalidSignature(String),
    /// Transaction too big
    TooBig,
    /// Invalid RLP encoding
    InvalidRlp(String),
    /// Transaciton is still not enabled.
    TransactionTypeNotEnabled,
    /// Transaction sender is not an EOA (see EIP-3607)
    SenderIsNotEOA,
}

impl From<crypto::publickey::Error> for Error {
    fn from(err: crypto::publickey::Error) -> Self {
        Error::InvalidSignature(format!("{}", err))
    }
}

impl From<rlp::DecoderError> for Error {
    fn from(err: rlp::DecoderError) -> Self {
        Error::InvalidRlp(format!("{}", err))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;
        let msg = match *self {
            AlreadyImported => "Already imported".into(),
            Old => "No longer valid".into(),
            TooCheapToReplace { prev, new } => format!(
                "Gas price too low to replace, previous tx gas: {:?}, new tx gas: {:?}",
                prev, new
            ),
            LimitReached => "Transaction limit reached".into(),
            InsufficientGasPrice { minimal, got } => {
                format!("Insufficient gas price. Min={}, Given={}", minimal, got)
            }
            GasPriceLowerThanBaseFee {
                gas_price,
                base_fee,
            } => {
                format!(
                    "Max gas price is lower then required base fee. Gas price={}, Base fee={}",
                    gas_price, base_fee
                )
            }
            InsufficientGas { minimal, got } => {
                format!("Insufficient gas. Min={}, Given={}", minimal, got)
            }
            InsufficientBalance { balance, cost } => format!(
                "Insufficient balance for transaction. Balance={}, Cost={}",
                balance, cost
            ),
            GasLimitExceeded { limit, got } => {
                format!("Gas limit exceeded. Limit={}, Given={}", limit, got)
            }
            InvalidGasLimit(ref err) => format!("Invalid gas limit. {}", err),
            SenderBanned => "Sender is temporarily banned.".into(),
            RecipientBanned => "Recipient is temporarily banned.".into(),
            CodeBanned => "Contract code is temporarily banned.".into(),
            InvalidChainId => "Transaction of this chain ID is not allowed on this chain.".into(),
            InvalidSignature(ref err) => format!("Transaction has invalid signature: {}.", err),
            NotAllowed => {
                "Sender does not have permissions to execute this type of transaction".into()
            }
            TooBig => "Transaction too big".into(),
            InvalidRlp(ref err) => format!("Transaction has invalid RLP structure: {}.", err),
            TransactionTypeNotEnabled => {
                format!("Transaction type is not enabled for current block")
            }
            SenderIsNotEOA => "Transaction sender is not an EOA (see EIP-3607)".into(),
        };

        f.write_fmt(format_args!("Transaction error ({})", msg))
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        "Transaction error"
    }
}
