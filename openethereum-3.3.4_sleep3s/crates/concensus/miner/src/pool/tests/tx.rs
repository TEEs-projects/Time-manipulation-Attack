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

use crypto::publickey::{Generator, Random};
use ethereum_types::{H256, U256};
use rustc_hex::FromHex;
use types::transaction::{
    self, AccessListTx, EIP1559TransactionTx, SignedTransaction, Transaction, TypedTransaction,
    UnverifiedTransaction,
};

use pool::{verifier, VerifiedTransaction};

#[derive(Clone)]
pub struct Tx {
    pub nonce: u64,
    pub gas: u64,
    pub gas_price: u64,
    pub value: u64,
}

impl Default for Tx {
    fn default() -> Self {
        Tx {
            nonce: 123,
            gas: 21_000,
            gas_price: 1,
            value: 100,
        }
    }
}

impl Tx {
    pub fn gas_price(gas_price: u64) -> Self {
        Tx {
            gas_price,
            ..Default::default()
        }
    }

    pub fn with_value(mut self, value: u64) -> Self {
        self.value = value;
        self
    }

    pub fn signed(self) -> SignedTransaction {
        let keypair = Random.generate();
        self.unsigned().sign(keypair.secret(), None)
    }

    pub fn signed_pair(self) -> (SignedTransaction, SignedTransaction) {
        let (tx1, tx2, _) = self.signed_triple();
        (tx1, tx2)
    }

    pub fn signed_triple(mut self) -> (SignedTransaction, SignedTransaction, SignedTransaction) {
        let keypair = Random.generate();
        let tx1 = self.clone().unsigned().sign(keypair.secret(), None);
        self.nonce += 1;
        let tx2 = self.clone().unsigned().sign(keypair.secret(), None);
        self.nonce += 1;
        let tx3 = self.unsigned().sign(keypair.secret(), None);

        (tx1, tx2, tx3)
    }

    pub fn signed_replacement(mut self) -> (SignedTransaction, SignedTransaction) {
        let keypair = Random.generate();
        let tx1 = self.clone().unsigned().sign(keypair.secret(), None);
        self.gas_price += 1;
        let tx2 = self.unsigned().sign(keypair.secret(), None);

        (tx1, tx2)
    }

    pub fn unsigned(self) -> TypedTransaction {
        TypedTransaction::Legacy(Transaction {
            action: transaction::Action::Create,
            value: self.value.into(),
            data: "3331600055".from_hex().unwrap(),
            gas: self.gas.into(),
            gas_price: self.gas_price.into(),
            nonce: self.nonce.into(),
        })
    }

    pub fn big_one(self) -> SignedTransaction {
        let keypair = Random.generate();
        let tx = TypedTransaction::Legacy(Transaction {
            action: transaction::Action::Create,
            value: self.value.into(),
            data: include_str!("../res/big_transaction.data")
                .from_hex()
                .unwrap(),
            gas: self.gas.into(),
            gas_price: self.gas_price.into(),
            nonce: self.nonce.into(),
        });
        tx.sign(keypair.secret(), None)
    }

    pub fn eip1559_one(self, max_priority_fee_per_gas: u64) -> SignedTransaction {
        let keypair = Random.generate();
        let tx = TypedTransaction::EIP1559Transaction(EIP1559TransactionTx {
            transaction: AccessListTx {
                transaction: Transaction {
                    action: transaction::Action::Create,
                    value: U256::from(100),
                    data: "3331600055".from_hex().unwrap(),
                    gas: self.gas.into(),
                    gas_price: self.gas_price.into(),
                    nonce: self.nonce.into(),
                },
                access_list: vec![],
            },
            max_priority_fee_per_gas: max_priority_fee_per_gas.into(),
        });
        tx.sign(keypair.secret(), None)
    }
}
pub trait TxExt: Sized {
    type Out;
    type Verified;
    type Hash;

    fn hash(&self) -> Self::Hash;

    fn local(self) -> Self::Out;

    fn retracted(self) -> Self::Out;

    fn unverified(self) -> Self::Out;

    fn verified(self) -> Self::Verified;
}

impl<A, B, O, V, H> TxExt for (A, B)
where
    A: TxExt<Out = O, Verified = V, Hash = H>,
    B: TxExt<Out = O, Verified = V, Hash = H>,
{
    type Out = (O, O);
    type Verified = (V, V);
    type Hash = (H, H);

    fn hash(&self) -> Self::Hash {
        (self.0.hash(), self.1.hash())
    }
    fn local(self) -> Self::Out {
        (self.0.local(), self.1.local())
    }
    fn retracted(self) -> Self::Out {
        (self.0.retracted(), self.1.retracted())
    }
    fn unverified(self) -> Self::Out {
        (self.0.unverified(), self.1.unverified())
    }
    fn verified(self) -> Self::Verified {
        (self.0.verified(), self.1.verified())
    }
}

impl TxExt for SignedTransaction {
    type Out = verifier::Transaction;
    type Verified = VerifiedTransaction;
    type Hash = H256;

    fn hash(&self) -> Self::Hash {
        UnverifiedTransaction::hash(self)
    }

    fn local(self) -> Self::Out {
        verifier::Transaction::Local(self.into())
    }

    fn retracted(self) -> Self::Out {
        verifier::Transaction::Retracted(self.into())
    }

    fn unverified(self) -> Self::Out {
        verifier::Transaction::Unverified(self.into())
    }

    fn verified(self) -> Self::Verified {
        VerifiedTransaction::from_pending_block_transaction(self)
    }
}

impl TxExt for Vec<SignedTransaction> {
    type Out = Vec<verifier::Transaction>;
    type Verified = Vec<VerifiedTransaction>;
    type Hash = Vec<H256>;

    fn hash(&self) -> Self::Hash {
        self.iter().map(|tx| tx.hash()).collect()
    }

    fn local(self) -> Self::Out {
        self.into_iter()
            .map(Into::into)
            .map(verifier::Transaction::Local)
            .collect()
    }

    fn retracted(self) -> Self::Out {
        self.into_iter()
            .map(Into::into)
            .map(verifier::Transaction::Retracted)
            .collect()
    }

    fn unverified(self) -> Self::Out {
        self.into_iter()
            .map(Into::into)
            .map(verifier::Transaction::Unverified)
            .collect()
    }

    fn verified(self) -> Self::Verified {
        self.into_iter()
            .map(VerifiedTransaction::from_pending_block_transaction)
            .collect()
    }
}

pub trait PairExt {
    type Type;

    fn into_vec(self) -> Vec<Self::Type>;
}

impl<A> PairExt for (A, A) {
    type Type = A;
    fn into_vec(self) -> Vec<A> {
        vec![self.0, self.1]
    }
}
