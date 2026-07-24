//! Budget — gas↔rJoule conversion and affordability checks.

use super::*;
use crate::price_feed::estimate_withdrawal_fee;

impl WalletManager {
    pub fn gas_to_rjoules(&self, gas: u64) -> RJoule {
        if gas == 0 {
            RJoule::ZERO
        } else {
            let rate = self.gas_per_rjoule.load(Ordering::Relaxed);
            let rj = gas / rate;
            RJoule::new(if rj == 0 { 1 } else { rj })
        }
    }

    pub fn rjoules_to_gas(&self, rj: RJoule) -> u64 {
        rj.as_u64() * self.gas_per_rjoule.load(Ordering::Relaxed)
    }

    pub fn gas_per_rjoule(&self) -> u64 {
        self.gas_per_rjoule.load(Ordering::Relaxed)
    }

    pub fn set_gas_per_rjoule(&self, rate: u64) {
        let rate = rate.max(1);
        self.gas_per_rjoule.store(rate, Ordering::Relaxed);
    }

    pub async fn estimate_withdrawal_fee(
        &self,
        actor: &WebID,
        chain: ChainId,
    ) -> Result<WithdrawalFee, WalletError> {
        let rate = self.price_feed.get_rate(chain).await.inspect_err(|e| {
            self.emit_chain_error_for_actor(
                actor,
                chain,
                "estimate_withdrawal_fee",
                &e.to_string(),
            );
        })?;
        Ok(estimate_withdrawal_fee(
            chain,
            &rate,
            self.config.rj_per_usdc,
        ))
    }

    pub(super) fn usdc_to_rjoules(&self, usdc_micro: u64) -> RJoule {
        let rj = (usdc_micro as u128 * self.config.rj_per_usdc as u128 / 1_000_000) as u64;
        RJoule::new(rj)
    }

    pub(super) fn rjoules_to_usdc(&self, rj: RJoule) -> u64 {
        (rj.as_u64() as u128 * 1_000_000 / self.config.rj_per_usdc as u128) as u64
    }

    pub fn can_afford(&self, wallet_id: WalletId, cost_rj: RJoule) -> Result<bool, WalletError> {
        let balance = self.get_balance(wallet_id)?;
        Ok(balance.rjoules >= cost_rj.as_u64())
    }

    pub fn reserve_rjoules(&self, wallet_id: WalletId, amount: RJoule) -> Result<(), WalletError> {
        if !self.can_afford(wallet_id, amount)? {
            let balance = self.get_balance(wallet_id)?;
            return Err(WalletError::InsufficientBalance {
                have: RJoule::new(balance.rjoules),
                need: amount,
            });
        }
        Ok(())
    }

    pub fn settle_rjoules(
        &self,
        wallet_id: WalletId,
        reserved: RJoule,
        actual: RJoule,
    ) -> Result<(), WalletError> {
        if actual > reserved {
            return Err(WalletError::ReservationExceeded { reserved, actual });
        }
        // Record the settlement as a Spend transaction. The caller (e.g.
        // WalletBackedBudget::settle) has the encumbrance context with key_id
        // and tool info, but settle_rjoules is a lower-level balance operation.
        // The transaction is recorded atomically by debit_rjoules.
        self.store.debit_rjoules(
            wallet_id,
            actual,
            TransactionType::Spend {
                key_id: ApiKeyId::from_name("settlement"),
                tool: "budget-settlement".to_string(),
                gas: 0,
                rj: actual,
            },
        )?;
        Ok(())
    }
}
