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

//! Replacing Transactions
//!
//! When queue limits are reached, a new transaction may replace one already
//! in the pool. The decision whether to reject, replace or retain both is
//! delegated to an implementation of `ShouldReplace`.
//!
//! Here we decide based on the sender, the nonce and gas price, and finally
//! on the `Readiness` of the transactions when comparing them

use std::cmp;

use super::{client, ScoredTransaction};
use ethereum_types::{H160 as Address, U256};
use txpool::{
    self,
    scoring::{Choice, Scoring},
    ReplaceTransaction, VerifiedTransaction,
};

/// Choose whether to replace based on the sender, the score, the `Readiness`,
/// and finally the `Validity` of the transactions being compared.
#[derive(Debug)]
pub struct ReplaceByScoreReadinessAndValidity<S, C> {
    scoring: S,
    client: C,
    /// Block base fee of the latest block, exists if the EIP 1559 is activated
    block_base_fee: Option<U256>,
}

impl<S, C> ReplaceByScoreReadinessAndValidity<S, C> {
    /// Create a new `ReplaceByScoreReadinessAndValidity`
    pub fn new(scoring: S, client: C, block_base_fee: Option<U256>) -> Self {
        Self {
            scoring,
            client,
            block_base_fee,
        }
    }

    /// Check if any choice could be made based on transaction sender.
    ///
    /// If both _old_ and _new_ transactions have the same sender, sender ordering
    /// rules are applied. Local transactions are neither rejected nor evicted.
    fn should_replace_by_sender<T>(
        &self,
        old: &ReplaceTransaction<T>,
        new: &ReplaceTransaction<T>,
    ) -> Option<Choice>
    where
        T: VerifiedTransaction<Sender = Address> + ScoredTransaction,
        S: Scoring<T>,
    {
        let both_local = old.priority().is_local() && new.priority().is_local();

        if old.sender() == new.sender() {
            // prefer earliest transaction
            let choice = match new.nonce().cmp(&old.nonce()) {
                cmp::Ordering::Equal => self.scoring.choose(&old, &new),
                _ if both_local => Choice::InsertNew,
                cmp::Ordering::Less => Choice::ReplaceOld,
                cmp::Ordering::Greater => Choice::RejectNew,
            };
            return Some(choice);
        }

        if both_local {
            // We should neither reject nor evict local transactions
            return Some(Choice::InsertNew);
        }

        None
    }

    /// Check if any choice could be made based on transaction score.
    ///
    /// New transaction's score should be greater than old transaction's score,
    /// otherwise the new transaction will be rejected.
    fn should_replace_by_score<T>(
        &self,
        old: &ReplaceTransaction<T>,
        new: &ReplaceTransaction<T>,
    ) -> Option<Choice>
    where
        T: ScoredTransaction,
    {
        let old_score = (old.priority(), old.effective_gas_price(self.block_base_fee));
        let new_score = (new.priority(), new.effective_gas_price(self.block_base_fee));

        if new_score <= old_score {
            return Some(Choice::RejectNew);
        }

        None
    }

    /// Check if new transaction is a replacement transaction.
    ///
    /// With replacement transactions we can safely return `InsertNew`, because
    /// we don't need to remove `old` (worst transaction in the pool) since `new` will replace
    /// some other transaction in the pool so we will never go above limit anyway.
    fn should_replace_as_replacement<T>(
        &self,
        _old: &ReplaceTransaction<T>,
        new: &ReplaceTransaction<T>,
    ) -> Option<Choice>
    where
        S: Scoring<T>,
    {
        if let Some(txs) = new.pooled_by_sender {
            if let Ok(index) = txs.binary_search_by(|old| self.scoring.compare(old, new)) {
                return match self.scoring.choose(&txs[index], new) {
                    Choice::ReplaceOld => Some(Choice::InsertNew),
                    choice => Some(choice),
                };
            }
        }
        None
    }

    /// Check if any choice could be made based on transaction readiness.
    ///
    /// Future transaction could not replace ready transaction.
    fn should_replace_by_readiness<T>(
        &self,
        old: &ReplaceTransaction<T>,
        new: &ReplaceTransaction<T>,
    ) -> Option<Choice>
    where
        T: VerifiedTransaction<Sender = Address> + ScoredTransaction + PartialEq,
        C: client::NonceClient,
    {
        let state = &self.client;
        // calculate readiness based on state nonce + pooled txs from same sender
        let is_ready = |replace: &ReplaceTransaction<T>| {
            let mut nonce = state.account_nonce(replace.sender());
            if let Some(txs) = replace.pooled_by_sender {
                for tx in txs.iter() {
                    if nonce == tx.nonce() && *tx.transaction != ***replace.transaction {
                        nonce = nonce.saturating_add(U256::from(1))
                    } else {
                        break;
                    }
                }
            }
            nonce == replace.nonce()
        };

        if !is_ready(new) && is_ready(old) {
            // prevent a ready transaction being replace by a non-ready transaction
            return Some(Choice::RejectNew);
        }

        None
    }

    /// Check if any choice could be made based on transaction validity.
    ///
    /// Transaction is considered _invalid_ if sender has not enough
    /// balance to pay maximum price for given transaction and all
    /// previous transactions in the transaction pool.
    ///
    /// Invalid transaction could not replace valid transaction.
    fn should_replace_by_validity<T>(
        &self,
        old: &ReplaceTransaction<T>,
        new: &ReplaceTransaction<T>,
    ) -> Option<Choice>
    where
        T: VerifiedTransaction<Sender = Address> + ScoredTransaction + PartialEq,
        C: client::BalanceClient,
    {
        let state = &self.client;
        // calculate readiness based on state balance + pooled txs from same sender
        let is_valid = |replace: &ReplaceTransaction<T>| {
            let mut balance = state.account_balance(replace.sender());
            if let Some(txs) = replace.pooled_by_sender {
                for tx in txs.iter() {
                    if tx.nonce() < replace.nonce() {
                        balance = {
                            let (balance, overflow) = balance.overflowing_sub(tx.cost());
                            if overflow {
                                return false;
                            }
                            balance
                        }
                    } else {
                        break;
                    }
                }
            }
            balance >= replace.cost()
        };

        if !is_valid(new) && is_valid(old) {
            // prevent a valid transaction being replace by an invalid transaction
            return Some(Choice::RejectNew);
        }

        None
    }
}

impl<T, S, C> txpool::ShouldReplace<T> for ReplaceByScoreReadinessAndValidity<S, C>
where
    T: VerifiedTransaction<Sender = Address> + ScoredTransaction + PartialEq,
    S: Scoring<T>,
    C: client::NonceClient + client::BalanceClient,
{
    fn should_replace(&self, old: &ReplaceTransaction<T>, new: &ReplaceTransaction<T>) -> Choice {
        // TODO: For now we verify that transaction is replacement only in case if new transaction
        //       has better score, as it was done that way before refactoring. Is there any
        //       reason why we cannot move replacement check before checking the scores?
        self.should_replace_by_sender(old, new)
            .or_else(|| self.should_replace_by_score(old, new))
            .or_else(|| self.should_replace_as_replacement(old, new))
            .or_else(|| self.should_replace_by_readiness(old, new))
            .or_else(|| self.should_replace_by_validity(old, new))
            .unwrap_or(Choice::ReplaceOld) // if all checks have passed, new transaction can replace the old one.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crypto::publickey::{Generator, KeyPair, Random};
    use pool::{
        scoring::*,
        tests::{
            client::TestClient,
            tx::{Tx, TxExt},
        },
        PrioritizationStrategy, VerifiedTransaction,
    };
    use std::sync::Arc;
    use txpool::{scoring::Choice::*, ShouldReplace};

    fn local_tx_verified(tx: Tx, keypair: &KeyPair) -> VerifiedTransaction {
        let mut verified_tx = tx.unsigned().sign(keypair.secret(), None).verified();
        verified_tx.priority = ::pool::Priority::Local;
        verified_tx
    }

    fn should_replace(
        replace: &dyn ShouldReplace<VerifiedTransaction>,
        old: VerifiedTransaction,
        new: VerifiedTransaction,
    ) -> Choice {
        let old_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(old),
        };
        let new_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(new),
        };
        let old = ReplaceTransaction::new(&old_tx, Default::default());
        let new = ReplaceTransaction::new(&new_tx, Default::default());
        replace.should_replace(&old, &new)
    }

    fn from_verified(tx: VerifiedTransaction) -> txpool::Transaction<VerifiedTransaction> {
        txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx),
        }
    }

    #[test]
    fn should_always_accept_local_transactions_unless_same_sender_and_nonce() {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        // same sender txs
        let keypair = Random.generate();

        let same_sender_tx1 = local_tx_verified(
            Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            },
            &keypair,
        );

        let same_sender_tx2 = local_tx_verified(
            Tx {
                nonce: 2,
                gas_price: 100,
                ..Default::default()
            },
            &keypair,
        );

        let same_sender_tx3 = local_tx_verified(
            Tx {
                nonce: 2,
                gas_price: 200,
                ..Default::default()
            },
            &keypair,
        );

        // different sender txs
        let sender1 = Random.generate();
        let different_sender_tx1 = local_tx_verified(
            Tx {
                nonce: 2,
                gas_price: 1,
                ..Default::default()
            },
            &sender1,
        );

        let sender2 = Random.generate();
        let different_sender_tx2 = local_tx_verified(
            Tx {
                nonce: 1,
                gas_price: 10,
                ..Default::default()
            },
            &sender2,
        );

        assert_eq!(
            should_replace(&replace, same_sender_tx1.clone(), same_sender_tx2.clone()),
            InsertNew
        );
        assert_eq!(
            should_replace(&replace, same_sender_tx2.clone(), same_sender_tx1.clone()),
            InsertNew
        );

        assert_eq!(
            should_replace(
                &replace,
                different_sender_tx1.clone(),
                different_sender_tx2.clone()
            ),
            InsertNew
        );
        assert_eq!(
            should_replace(
                &replace,
                different_sender_tx2.clone(),
                different_sender_tx1.clone()
            ),
            InsertNew
        );

        // txs with same sender and nonce
        assert_eq!(
            should_replace(&replace, same_sender_tx2.clone(), same_sender_tx3.clone()),
            ReplaceOld
        );
        assert_eq!(
            should_replace(&replace, same_sender_tx3.clone(), same_sender_tx2.clone()),
            RejectNew
        );
    }

    #[test]
    fn should_replace_same_sender_by_nonce() {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        let tx1 = Tx {
            nonce: 1,
            gas_price: 1,
            ..Default::default()
        };
        let tx2 = Tx {
            nonce: 2,
            gas_price: 100,
            ..Default::default()
        };
        let tx3 = Tx {
            nonce: 2,
            gas_price: 110,
            ..Default::default()
        };
        let tx4 = Tx {
            nonce: 2,
            gas_price: 130,
            ..Default::default()
        };

        let keypair = Random.generate();
        let txs = vec![tx1, tx2, tx3, tx4]
            .into_iter()
            .map(|tx| tx.unsigned().sign(keypair.secret(), None).verified())
            .collect::<Vec<_>>();

        assert_eq!(
            should_replace(&replace, txs[0].clone(), txs[1].clone()),
            RejectNew
        );
        assert_eq!(
            should_replace(&replace, txs[1].clone(), txs[0].clone()),
            ReplaceOld
        );

        assert_eq!(
            should_replace(&replace, txs[1].clone(), txs[2].clone()),
            RejectNew
        );
        assert_eq!(
            should_replace(&replace, txs[2].clone(), txs[1].clone()),
            RejectNew
        );

        assert_eq!(
            should_replace(&replace, txs[1].clone(), txs[3].clone()),
            ReplaceOld
        );
        assert_eq!(
            should_replace(&replace, txs[3].clone(), txs[1].clone()),
            RejectNew
        );
    }

    #[test]
    fn should_replace_different_sender_by_priority_and_gas_price() {
        // given
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(0).with_balance(1_000_000);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        let tx_regular_low_gas = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.signed().verified()
        };
        let tx_regular_high_gas = {
            let tx = Tx {
                nonce: 2,
                gas_price: 10,
                ..Default::default()
            };
            tx.signed().verified()
        };
        let tx_local_low_gas = {
            let tx = Tx {
                nonce: 2,
                gas_price: 1,
                ..Default::default()
            };
            let mut verified_tx = tx.signed().verified();
            verified_tx.priority = ::pool::Priority::Local;
            verified_tx
        };
        let tx_local_high_gas = {
            let tx = Tx {
                nonce: 1,
                gas_price: 10,
                ..Default::default()
            };
            let mut verified_tx = tx.signed().verified();
            verified_tx.priority = ::pool::Priority::Local;
            verified_tx
        };

        assert_eq!(
            should_replace(
                &replace,
                tx_regular_low_gas.clone(),
                tx_regular_high_gas.clone()
            ),
            ReplaceOld
        );
        assert_eq!(
            should_replace(
                &replace,
                tx_regular_high_gas.clone(),
                tx_regular_low_gas.clone()
            ),
            RejectNew
        );

        assert_eq!(
            should_replace(
                &replace,
                tx_regular_high_gas.clone(),
                tx_local_low_gas.clone()
            ),
            ReplaceOld
        );
        assert_eq!(
            should_replace(
                &replace,
                tx_local_low_gas.clone(),
                tx_regular_high_gas.clone()
            ),
            RejectNew
        );

        assert_eq!(
            should_replace(
                &replace,
                tx_local_low_gas.clone(),
                tx_local_high_gas.clone()
            ),
            InsertNew
        );
        assert_eq!(
            should_replace(
                &replace,
                tx_local_high_gas.clone(),
                tx_regular_low_gas.clone()
            ),
            RejectNew
        );
    }

    #[test]
    fn should_not_replace_ready_transaction_with_future_transaction() {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        let tx_ready_low_score = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.signed().verified()
        };
        let tx_future_high_score = {
            let tx = Tx {
                nonce: 3, // future nonce
                gas_price: 10,
                ..Default::default()
            };
            tx.signed().verified()
        };

        assert_eq!(
            should_replace(&replace, tx_ready_low_score, tx_future_high_score),
            RejectNew
        );
    }

    #[test]
    fn should_not_replace_valid_transaction_with_invalid_transaction() {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_balance(64000);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        let tx_valid_low_score = {
            let tx = Tx::gas_price(1);
            tx.signed().verified()
        };
        let (tx_valid_high_score, tx_invalid_high_score) = {
            let tx = Tx::gas_price(3).with_value(1000);
            tx.signed_pair().verified()
        };

        let old_tx = from_verified(tx_valid_low_score);
        let new_tx = from_verified(tx_invalid_high_score);
        let new_tx_sender_pool = [from_verified(tx_valid_high_score)];
        let old = ReplaceTransaction::new(&old_tx, Default::default());
        let new = ReplaceTransaction::new(&new_tx, Some(&new_tx_sender_pool));

        assert_eq!(replace.should_replace(&old, &new), RejectNew);
    }

    #[test]
    fn should_compute_readiness_with_pooled_transactions_from_the_same_sender_as_the_existing_transaction(
    ) {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        let old_sender = Random.generate();
        let tx_old_ready_1 = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.unsigned().sign(&old_sender.secret(), None).verified()
        };
        let tx_old_ready_2 = {
            let tx = Tx {
                nonce: 2,
                gas_price: 1,
                ..Default::default()
            };
            tx.unsigned().sign(&old_sender.secret(), None).verified()
        };
        let tx_old_ready_3 = {
            let tx = Tx {
                nonce: 3,
                gas_price: 1,
                ..Default::default()
            };
            tx.unsigned().sign(&old_sender.secret(), None).verified()
        };

        let new_tx = {
            let tx = Tx {
                nonce: 3, // future nonce
                gas_price: 10,
                ..Default::default()
            };
            tx.signed().verified()
        };

        let old_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx_old_ready_3),
        };
        let pooled_txs = [
            txpool::Transaction {
                insertion_id: 0,
                transaction: Arc::new(tx_old_ready_1),
            },
            txpool::Transaction {
                insertion_id: 0,
                transaction: Arc::new(tx_old_ready_2),
            },
        ];

        let new_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(new_tx),
        };

        let old = ReplaceTransaction::new(&old_tx, Some(&pooled_txs));
        let new = ReplaceTransaction::new(&new_tx, Default::default());

        assert_eq!(replace.should_replace(&old, &new), RejectNew);
    }

    #[test]
    fn should_compute_readiness_with_pooled_transactions_from_the_same_sender_as_the_new_transaction(
    ) {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1).with_balance(1_000_000);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        // current transaction is ready but has a lower gas price than the new one
        let old_tx = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.signed().verified()
        };

        let new_sender = Random.generate();
        let tx_new_ready_1 = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.unsigned().sign(&new_sender.secret(), None).verified()
        };
        let tx_new_ready_2 = {
            let tx = Tx {
                nonce: 2,
                gas_price: 1,
                ..Default::default()
            };
            tx.unsigned().sign(&new_sender.secret(), None).verified()
        };
        let tx_new_ready_3 = {
            let tx = Tx {
                nonce: 3,
                gas_price: 10, // hi
                ..Default::default()
            };
            tx.unsigned().sign(&new_sender.secret(), None).verified()
        };

        let old_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(old_tx),
        };

        let new_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx_new_ready_3),
        };
        let pooled_txs = [
            txpool::Transaction {
                insertion_id: 0,
                transaction: Arc::new(tx_new_ready_1),
            },
            txpool::Transaction {
                insertion_id: 0,
                transaction: Arc::new(tx_new_ready_2),
            },
        ];

        let old = ReplaceTransaction::new(&old_tx, None);
        let new = ReplaceTransaction::new(&new_tx, Some(&pooled_txs));

        assert_eq!(replace.should_replace(&old, &new), ReplaceOld);
    }

    #[test]
    fn should_accept_local_tx_with_same_sender_and_nonce_with_better_gas_price() {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        // current transaction is ready
        let old_tx = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.signed().verified()
        };

        let new_sender = Random.generate();
        let tx_new_ready_1 = local_tx_verified(
            Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            },
            &new_sender,
        );

        let tx_new_ready_2 = local_tx_verified(
            Tx {
                nonce: 1,
                gas_price: 2, // same nonce, higher gas price
                ..Default::default()
            },
            &new_sender,
        );

        let old_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(old_tx),
        };

        let new_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx_new_ready_2),
        };
        let pooled_txs = [txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx_new_ready_1),
        }];

        let old = ReplaceTransaction::new(&old_tx, None);
        let new = ReplaceTransaction::new(&new_tx, Some(&pooled_txs));

        assert_eq!(replace.should_replace(&old, &new), InsertNew);
    }

    #[test]
    fn should_reject_local_tx_with_same_sender_and_nonce_with_worse_gas_price() {
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let client = TestClient::new().with_nonce(1);
        let replace = ReplaceByScoreReadinessAndValidity::new(scoring, client, None);

        // current transaction is ready
        let old_tx = {
            let tx = Tx {
                nonce: 1,
                gas_price: 1,
                ..Default::default()
            };
            tx.signed().verified()
        };

        let new_sender = Random.generate();
        let tx_new_ready_1 = local_tx_verified(
            Tx {
                nonce: 1,
                gas_price: 2,
                ..Default::default()
            },
            &new_sender,
        );

        let tx_new_ready_2 = local_tx_verified(
            Tx {
                nonce: 1,
                gas_price: 1, // same nonce, lower gas price
                ..Default::default()
            },
            &new_sender,
        );

        let old_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(old_tx),
        };

        let new_tx = txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx_new_ready_2),
        };
        let pooled_txs = [txpool::Transaction {
            insertion_id: 0,
            transaction: Arc::new(tx_new_ready_1),
        }];

        let old = ReplaceTransaction::new(&old_tx, None);
        let new = ReplaceTransaction::new(&new_tx, Some(&pooled_txs));

        assert_eq!(replace.should_replace(&old, &new), RejectNew);
    }
}
