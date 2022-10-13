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

//! Trace errors.

use parity_util_mem::MallocSizeOf;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::fmt;
use vm::Error as VmError;

/// Trace evm errors.
#[derive(Debug, PartialEq, Clone, MallocSizeOf)]
pub enum Error {
    /// `OutOfGas` is returned when transaction execution runs out of gas.
    OutOfGas,
    /// `BadJumpDestination` is returned when execution tried to move
    /// to position that wasn't marked with JUMPDEST instruction
    BadJumpDestination,
    /// `BadInstructions` is returned when given instruction is not supported
    BadInstruction,
    /// `StackUnderflow` when there is not enough stack elements to execute instruction
    StackUnderflow,
    /// When execution would exceed defined Stack Limit
    OutOfStack,
    /// When there is not enough subroutine stack elements to return from
    SubStackUnderflow,
    /// When execution would exceed defined subroutine Stack Limit
    OutOfSubStack,
    /// When the code walks into a subroutine, that is not allowed
    InvalidSubEntry,
    /// When builtin contract failed on input data
    BuiltIn,
    /// Returned on evm internal error. Should never be ignored during development.
    /// Likely to cause consensus issues.
    Internal,
    /// When execution tries to modify the state in static context
    MutableCallInStaticContext,
    /// When invalid code was attempted to deploy
    InvalidCode,
    /// Wasm error
    Wasm,
    /// Contract tried to access past the return data buffer.
    OutOfBounds,
    /// Execution has been reverted with REVERT instruction.
    Reverted,
}

impl<'a> From<&'a VmError> for Error {
    fn from(e: &'a VmError) -> Self {
        match *e {
            VmError::OutOfGas => Error::OutOfGas,
            VmError::BadJumpDestination { .. } => Error::BadJumpDestination,
            VmError::BadInstruction { .. } => Error::BadInstruction,
            VmError::StackUnderflow { .. } => Error::StackUnderflow,
            VmError::OutOfStack { .. } => Error::OutOfStack,
            VmError::SubStackUnderflow { .. } => Error::SubStackUnderflow,
            VmError::OutOfSubStack { .. } => Error::OutOfSubStack,
            VmError::InvalidSubEntry { .. } => Error::InvalidSubEntry,
            VmError::BuiltIn { .. } => Error::BuiltIn,
            VmError::InvalidCode => Error::InvalidCode,
            VmError::Wasm { .. } => Error::Wasm,
            VmError::Internal(_) => Error::Internal,
            VmError::MutableCallInStaticContext => Error::MutableCallInStaticContext,
            VmError::OutOfBounds => Error::OutOfBounds,
            VmError::Reverted => Error::Reverted,
        }
    }
}

impl From<VmError> for Error {
    fn from(e: VmError) -> Self {
        Error::from(&e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;
        let message = match *self {
            OutOfGas => "Out of gas",
            BadJumpDestination => "Bad jump destination",
            BadInstruction => "Bad instruction",
            StackUnderflow => "Stack underflow",
            OutOfStack => "Out of stack",
            SubStackUnderflow => "Subroutine stack underflow",
            OutOfSubStack => "Subroutine stack overflow",
            BuiltIn => "Built-in failed",
            InvalidSubEntry => "Invalid subroutine entry",
            InvalidCode => "Invalid code",
            Wasm => "Wasm runtime error",
            Internal => "Internal error",
            MutableCallInStaticContext => "Mutable Call In Static Context",
            OutOfBounds => "Out of bounds",
            Reverted => "Reverted",
        };
        message.fmt(f)
    }
}

impl Encodable for Error {
    fn rlp_append(&self, s: &mut RlpStream) {
        use self::Error::*;
        let value = match *self {
            OutOfGas => 0u8,
            BadJumpDestination => 1,
            BadInstruction => 2,
            StackUnderflow => 3,
            OutOfStack => 4,
            Internal => 5,
            BuiltIn => 6,
            MutableCallInStaticContext => 7,
            Wasm => 8,
            OutOfBounds => 9,
            Reverted => 10,
            SubStackUnderflow => 11,
            OutOfSubStack => 12,
            InvalidSubEntry => 13,
            InvalidCode => 14,
        };

        s.append_internal(&value);
    }
}

impl Decodable for Error {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        use self::Error::*;
        let value: u8 = rlp.as_val()?;
        match value {
            0 => Ok(OutOfGas),
            1 => Ok(BadJumpDestination),
            2 => Ok(BadInstruction),
            3 => Ok(StackUnderflow),
            4 => Ok(OutOfStack),
            5 => Ok(Internal),
            6 => Ok(BuiltIn),
            7 => Ok(MutableCallInStaticContext),
            8 => Ok(Wasm),
            9 => Ok(OutOfBounds),
            10 => Ok(Reverted),
            11 => Ok(SubStackUnderflow),
            12 => Ok(OutOfSubStack),
            13 => Ok(InvalidSubEntry),
            14 => Ok(InvalidCode),
            _ => Err(DecoderError::Custom("Invalid error type")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use rlp::*;

    #[test]
    fn encode_error() {
        let err = Error::BadJumpDestination;

        let mut s = RlpStream::new_list(2);
        s.append(&err);
        assert!(!s.is_finished(), "List shouldn't finished yet");
        s.append(&err);
        assert!(s.is_finished(), "List should be finished now");
        s.out();
    }
}
