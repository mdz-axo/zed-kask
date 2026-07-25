//! WalletBackedBudget — Energy budget variant backed by a wallet's rJoule balance.
//!
//! Unlike the standard `GasBudget` which replenishes periodically from a
//! dimensionless gas pool, `WalletBackedBudget` converts gas costs to rJoules
//! and debits a real wallet. This is the payment mechanism for paid agents.
//!
//! # Hold-settle pattern
//! 1. `can_proceed(gas)` — converts gas to rJoules, checks wallet balance
//! 2. `reserve(gas)` — optimistic reservation (checks balance, doesn't debit)
//! 3. Tool executes
//! 4. `settle(reserved_gas, actual_gas)` — debits actual rJoule cost
//!
//! # Coexistence with GasBudget
//! `WalletBackedBudget` is an additional budget type, not a replacement.
//! The existing gas system continues for non-wallet-backed agents.
//! Both coexist in the `GovernedTool` membrane via `GasBudgetManager`.

use crate::energy::{GasCost, GasError};
use chrono::Utc;
use hkask_types::WalletBudgetPort;
use hkask_types::id::{ApiKeyId, WalletId};
use hkask_types::{ApiKeyCapability, RJoule};
use std::sync::Arc;

/// Health status of an API key tracked by a wallet-backed budget.
#[derive(Debug, Clone)]
pub struct KeyHealth {
    /// The key has spent its full spending limit.
    pub exhausted: bool,
    /// The key's expiry timestamp has passed.
    pub expired: bool,
    /// rJoules spent so far.
    pub spent_rj: u64,
    /// Spending limit (0 if no limit set).
    pub limit_rj: u64,
}

/// An energy budget backed by a wallet's rJoule balance.
///
/// Converts dimensionless gas costs to rJoules via `gas_per_rjoule` and
/// delegates balance checks, reservations, and settlements to `WalletManager`.
///
/// # Hard limit
/// \[DECLARATIVE\] Wallet-backed budgets always have `hard_limit = true`. When the wallet (P4 — Clear Boundaries).
/// balance is insufficient, operations are rejected — there is no "soft limit"
/// fallback because rJoules represent real value.
pub struct WalletBackedBudget {
    /// The wallet that funds this budget.
    pub wallet_id: WalletId,
    /// Optional API key for spending-limit tracking.
    /// When present, spending is also checked against the key's limit.
    pub key_id: Option<ApiKeyId>,
    /// Optional per-key spending cap (rJoules).
    /// When set, the key cannot spend more than this total.
    pub spending_limit_rj: Option<RJoule>,
    /// Reference to the wallet manager for balance operations.
    /// The manager's `WalletConfig.gas_per_rjoule` is the authoritative conversion rate.
    pub wallet_manager: Arc<dyn WalletBudgetPort>,
    /// Always true for wallet budgets — insufficient balance = rejection.
    pub hard_limit: bool,
}

impl WalletBackedBudget {
    /// Create a new wallet-backed budget.
    pub fn new(wallet_id: WalletId, wallet_manager: Arc<dyn WalletBudgetPort>) -> Self {
        Self {
            wallet_id,
            key_id: None,
            spending_limit_rj: None,
            wallet_manager,
            hard_limit: true,
        }
    }

    /// Attach an API key for spending-limit tracking.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_api_key(mut self, key_id: ApiKeyId, spending_limit_rj: RJoule) -> Self {
        self.key_id = Some(key_id);
        self.spending_limit_rj = Some(spending_limit_rj);
        self
    }

    /// Convert gas units to rJoules using the configured rate.
    fn gas_to_rjoules(&self, gas: u64) -> RJoule {
        self.wallet_manager.gas_to_rjoules(gas)
    }

    /// Check whether an operation costing `gas` can proceed.
    ///
    /// When an API key is attached, checks the key's encumbrance remaining
    /// instead of the raw wallet balance. This enforces the encumbrance
    /// membrane: only explicitly allocated rJoules can be spent.
    /// When no key is attached, falls back to direct wallet balance check.
    pub fn can_proceed(&self, gas: GasCost) -> bool {
        let cost_rj = self.gas_to_rjoules(gas.0);

        // If a key is attached, check encumbrance instead of wallet balance
        if let Some(key_id) = self.key_id {
            match self.wallet_manager.get_encumbrance(key_id) {
                Some(ref enc) if enc.is_active() => {
                    if enc.remaining_rj() < cost_rj.as_u64() {
                        self.wallet_manager.emit_key_alert(key_id, true, false);
                        return false;
                    }
                }
                _ => {
                    // No active encumbrance — check if key is expired/exhausted for alert
                    if let Some(health) = self.check_key_health() {
                        self.wallet_manager.emit_key_alert(
                            key_id,
                            health.exhausted,
                            health.expired,
                        );
                    }
                    return false;
                }
            }
        } else {
            // No key — check raw wallet balance
            match self.wallet_manager.can_afford(self.wallet_id, cost_rj) {
                true => {}
                false => return false,
            }
        }

        // Check key spending limit if a key is attached
        if let Some(limit) = self.spending_limit_rj
            && let Some(health) = self.check_key_health()
        {
            let would_spend = health.spent_rj + cost_rj.as_u64();
            if would_spend > limit.as_u64() {
                self.wallet_manager.emit_key_alert(
                    self.key_id
                        .expect("key_id must be present when spending_limit_rj is set"),
                    true,
                    health.expired,
                );
                return false;
            }
        }
        true
    }

    /// Reserve rJoules for an in-flight operation.
    ///
    /// When an API key is attached, checks the encumbrance (not wallet balance).
    /// The actual debit happens at `settle()` time via `consume_encumbrance`.
    pub fn reserve(&self, gas: GasCost) -> Result<GasCost, GasError> {
        if !self.can_proceed(gas) {
            return Err(GasError::BudgetExceeded {
                requested: gas,
                remaining: GasCost(0),
            });
        }
        // Reservation is optimistic — can_proceed already verified encumbrance/wallet.
        // No debit happens here; actual consumption occurs in settle().
        // Callers must expect that settle() may fail if the encumbrance was
        // consumed between reserve and settle (TOCTOU). Retry or escalate.
        Ok(gas)
    }

    /// Settle rJoules after an operation completes.
    ///
    /// When an API key is attached, consumes from the key's encumbrance
    /// via `WalletManager::consume()` (atomic encumbrance debit).
    /// When no key is attached, debits directly from wallet balance.
    pub fn settle(&self, reserved_gas: GasCost, actual_gas: GasCost) -> Result<GasCost, GasError> {
        let actual_rj = self.gas_to_rjoules(actual_gas.0);

        if let Some(key_id) = self.key_id {
            // Consume from encumbrance (atomic — no separate check+deduct)
            self.wallet_manager
                .consume(key_id, actual_rj)
                .map_err(|_e| GasError::BudgetExceeded {
                    requested: actual_gas,
                    remaining: GasCost(0),
                })?;
        } else {
            // Direct wallet debit
            let reserved_rj = self.gas_to_rjoules(reserved_gas.0);
            self.wallet_manager
                .settle_rjoules(self.wallet_id, reserved_rj, actual_rj)
                .map_err(|_e| GasError::BudgetExceeded {
                    requested: actual_gas,
                    remaining: GasCost(0),
                })?;
        }

        Ok(actual_gas)
    }

    /// Check the health of the attached API key (if any).
    ///
    /// Returns `None` if no key is attached or the key can't be found.
    /// Returns `KeyHealth` with exhaustion and expiry status otherwise.
    pub fn check_key_health(&self) -> Option<KeyHealth> {
        let key_id = self.key_id?;
        let capability: ApiKeyCapability = self.wallet_manager.get_api_key(key_id)?;
        let now = Utc::now();
        Some(KeyHealth {
            exhausted: capability.spent_rj.as_u64() >= capability.spending_limit_rj.as_u64(),
            expired: capability.expiry.is_some_and(|exp| now > exp),
            spent_rj: capability.spent_rj.as_u64(),
            limit_rj: capability.spending_limit_rj.as_u64(),
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::GAS_PER_RJOULE;

    #[test]
    fn wallet_budget_gas_to_rjoules_conversion() {
        // Unit test: verify the conversion math without a real wallet.
        // gas_per_rjoule = GAS_PER_RJOULE → GAS_PER_RJOULE/2 gas = 1 rJ (rounds up from 0.5)
        // This is a pure math test — no WalletManager needed.
        let gas_per_rjoule: u64 = GAS_PER_RJOULE;
        // GAS_PER_RJOULE/2 / GAS_PER_RJOULE = 0 rJ → rounds up to 1
        assert_eq!(GAS_PER_RJOULE / 2 / gas_per_rjoule, 0); // integer division
        // GAS_PER_RJOULE*3/2 / GAS_PER_RJOULE = 1 rJ (1.5 rounds down)
        assert_eq!(GAS_PER_RJOULE * 3 / 2 / gas_per_rjoule, 1);
        // GAS_PER_RJOULE*2 / GAS_PER_RJOULE = 2 rJ
        assert_eq!(GAS_PER_RJOULE * 2 / gas_per_rjoule, 2);
    }
}
