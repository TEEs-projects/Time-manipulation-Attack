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

//! Transaction Scoring and Ordering
//!
//! Ethereum transactions from the same sender are ordered by `nonce`.
//! Low nonces need to be included first. If there are two transactions from the same sender
//! and with the same `nonce` only one of them can be included.
//! We choose the one with higher gas price, but also require that gas price increment
//! is high enough to prevent attacking miners by requiring them to reshuffle/reexecute
//! the queue too often.
//!
//! Transactions between senders are prioritized using `gas price`. Higher `gas price`
//! yields more profits for miners. Additionally we prioritize transactions that originate
//! from our local node (own transactions).

use std::cmp;

use super::{verifier, PrioritizationStrategy, ScoredTransaction, VerifiedTransaction};
use ethereum_types::U256;
use txpool::{self, scoring};

/// Transaction with the same (sender, nonce) can be replaced only if
/// `new_gas_price > old_gas_price + old_gas_price >> SHIFT`
const GAS_PRICE_BUMP_SHIFT: usize = 3; // 2 = 25%, 3 = 12.5%, 4 = 6.25%

/// Calculate minimal gas price requirement.
#[inline]
fn bump_gas_price(old_gp: U256) -> U256 {
    old_gp.saturating_add(old_gp >> GAS_PRICE_BUMP_SHIFT)
}
/// List of events that trigger updating of scores
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum ScoringEvent {
    /// Penalize transactions
    Penalize,
    /// Every time new block is added to blockchain, block base fee is changed and triggers score change.
    BlockBaseFeeChanged,
}
/// Simple, gas-price based scoring for transactions.
///
/// NOTE: Currently penalization does not apply to new transactions that enter the pool.
/// We might want to store penalization status in some persistent state.
#[derive(Debug, Clone)]
pub struct NonceAndGasPrice {
    /// Strategy for prioritization
    pub strategy: PrioritizationStrategy,
    /// Block base fee. Exists if the EIP 1559 is activated.
    pub block_base_fee: Option<U256>,
}

impl NonceAndGasPrice {
    /// Decide if the transaction should even be considered into the pool (if the pool is full).
    ///
    /// Used by Verifier to quickly reject transactions that don't have any chance to get into the pool later on,
    /// and save time on more expensive checks like sender recovery, etc.
    ///
    /// NOTE The method is never called for zero-gas-price transactions or local transactions
    /// (such transactions are always considered to the pool and potentially rejected later on)
    pub fn should_reject_early(
        &self,
        old: &VerifiedTransaction,
        new: &verifier::Transaction,
    ) -> bool {
        if old.priority().is_local() {
            return true;
        }

        old.effective_gas_price(self.block_base_fee) > new.effective_gas_price(self.block_base_fee)
    }
}

impl<P> txpool::Scoring<P> for NonceAndGasPrice
where
    P: ScoredTransaction + txpool::VerifiedTransaction,
{
    type Score = U256;
    type Event = ScoringEvent;

    fn compare(&self, old: &P, other: &P) -> cmp::Ordering {
        old.nonce().cmp(&other.nonce())
    }

    fn choose(&self, old: &P, new: &P) -> scoring::Choice {
        if old.nonce() != new.nonce() {
            return scoring::Choice::InsertNew;
        }

        let old_gp = old.effective_gas_price(self.block_base_fee);
        let new_gp = new.effective_gas_price(self.block_base_fee);

        let min_required_gp = bump_gas_price(old_gp);

        match min_required_gp.cmp(&new_gp) {
            cmp::Ordering::Greater => scoring::Choice::RejectNew,
            _ => scoring::Choice::ReplaceOld,
        }
    }

    fn update_scores(
        &self,
        txs: &[txpool::Transaction<P>],
        scores: &mut [U256],
        change: scoring::Change<ScoringEvent>,
    ) {
        use self::scoring::Change;

        match change {
            Change::Culled(_) => {}
            Change::RemovedAt(_) => {}
            Change::InsertedAt(i) | Change::ReplacedAt(i) => {
                assert!(i < txs.len());
                assert!(i < scores.len());

                scores[i] = txs[i].effective_gas_price(self.block_base_fee);
                let boost = match txs[i].priority() {
                    super::Priority::Local => 15,
                    super::Priority::Retracted => 10,
                    super::Priority::Regular => 0,
                };

                //boost local and retracted only if they are currently includable (base fee criteria)
                if self.block_base_fee.is_none() || scores[i] >= self.block_base_fee.unwrap() {
                    scores[i] = scores[i] << boost;
                }
            }
            // We are only sending an event in case of penalization.
            // So just lower the priority of all non-local transactions.
            Change::Event(event) => {
                match event {
                    ScoringEvent::Penalize => {
                        for (score, tx) in scores.iter_mut().zip(txs) {
                            // Never penalize local transactions.
                            if !tx.priority().is_local() {
                                *score = *score >> 3;
                            }
                        }
                    }
                    ScoringEvent::BlockBaseFeeChanged => {
                        for i in 0..txs.len() {
                            scores[i] = txs[i].transaction.effective_gas_price(self.block_base_fee);
                            let boost = match txs[i].priority() {
                                super::Priority::Local => 15,
                                super::Priority::Retracted => 10,
                                super::Priority::Regular => 0,
                            };

                            //boost local and retracted only if they are currently includable (base fee criteria)
                            if self.block_base_fee.is_none()
                                || scores[i] >= self.block_base_fee.unwrap()
                            {
                                scores[i] = scores[i] << boost;
                            }
                        }
                    }
                }
            }
        }
    }

    fn should_ignore_sender_limit(&self, new: &P) -> bool {
        new.priority().is_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pool::tests::tx::{Tx, TxExt};
    use std::sync::Arc;
    use txpool::Scoring;

    #[test]
    fn should_calculate_score_correctly() {
        // given
        let scoring = NonceAndGasPrice {
            strategy: PrioritizationStrategy::GasPriceOnly,
            block_base_fee: None,
        };
        let (tx1, tx2, tx3) = Tx::default().signed_triple();
        let transactions = vec![tx1, tx2, tx3]
            .into_iter()
            .enumerate()
            .map(|(i, tx)| {
                let mut verified = tx.verified();
                verified.priority = match i {
                    0 => ::pool::Priority::Local,
                    1 => ::pool::Priority::Retracted,
                    _ => ::pool::Priority::Regular,
                };
                txpool::Transaction {
                    insertion_id: 0,
                    transaction: Arc::new(verified),
                }
            })
            .collect::<Vec<_>>();
        let initial_scores = vec![U256::from(0), 0.into(), 0.into()];

        // No update required
        let mut scores = initial_scores.clone();
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::Culled(0));
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::Culled(1));
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::Culled(2));
        assert_eq!(scores, initial_scores);
        let mut scores = initial_scores.clone();
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::RemovedAt(0));
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::RemovedAt(1));
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::RemovedAt(2));
        assert_eq!(scores, initial_scores);

        // Compute score at given index
        let mut scores = initial_scores.clone();
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::InsertedAt(0));
        assert_eq!(scores, vec![32768.into(), 0.into(), 0.into()]);
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::InsertedAt(1));
        assert_eq!(scores, vec![32768.into(), 1024.into(), 0.into()]);
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::InsertedAt(2));
        assert_eq!(scores, vec![32768.into(), 1024.into(), 1.into()]);

        let mut scores = initial_scores.clone();
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::ReplacedAt(0));
        assert_eq!(scores, vec![32768.into(), 0.into(), 0.into()]);
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::ReplacedAt(1));
        assert_eq!(scores, vec![32768.into(), 1024.into(), 0.into()]);
        scoring.update_scores(&transactions, &mut *scores, scoring::Change::ReplacedAt(2));
        assert_eq!(scores, vec![32768.into(), 1024.into(), 1.into()]);

        // Check penalization
        scoring.update_scores(
            &transactions,
            &mut *scores,
            scoring::Change::Event(ScoringEvent::Penalize),
        );
        assert_eq!(scores, vec![32768.into(), 128.into(), 0.into()]);
    }
}
