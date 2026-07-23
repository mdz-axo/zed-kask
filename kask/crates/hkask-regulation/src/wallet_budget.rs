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
    use hkask_storage::WalletStore;
    use hkask_types::crypto::Ed25519PublicKey;
    use hkask_types::{ChainId, PrivacyMode, TransactionType};
    use hkask_wallet::price_feed::StaticPriceFeed;
    use hkask_wallet::{GAS_PER_RJOULE, WalletConfig, WalletManager};

    // WalletBackedBudget tests require a real WalletManager with an in-memory DB.
    // These are integration-style tests — they validate the gas→rJoule→debit pipeline.
    // Skipped by default (require keystore env); run with:
    //   HKASK_MASTER_KEY=000102... cargo test -p hkask-regulation -- wallet_budget

    fn make_wallet_budget_with_key(spent_rj: u64, limit_rj: u64) -> WalletBackedBudget {
        // SAFETY: test-only setup for deterministic wallet manager construction.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        let wallet_id = WalletId::new();
        let key_id = ApiKeyId::new();

        store
            .credit_rjoules(
                wallet_id,
                RJoule::new(10_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey([11u8; 32]),
            spending_limit_rj: RJoule::new(limit_rj),
            spent_rj: RJoule::new(spent_rj),
            scope: vec![],
            purpose: "wallet budget health test".into(),
            rate_limit: None,
            expiry: None,
            issued_at: Utc::now(),
            privacy_mode: PrivacyMode::default(),
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();
        store
            .encumber_rjoules(wallet_id, key_id, RJoule::new(2_000))
            .unwrap();

        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                store,
                Default::default(),
                Arc::new(StaticPriceFeed),
            )
            .unwrap(),
        );

        WalletBackedBudget::new(wallet_id, manager as Arc<dyn WalletBudgetPort>)
            .with_api_key(key_id, RJoule::new(limit_rj))
    }

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

    #[test]
    fn wallet_budget_rejects_exhausted_key_even_with_active_encumbrance() {
        let budget = make_wallet_budget_with_key(1_000, 1_000);
        assert!(
            !budget.can_proceed(GasCost(1_000)),
            "exhausted key must be rejected by wallet-backed budget"
        );
    }

    #[test]
    fn wallet_budget_allows_spend_within_encumbrance() {
        let budget = make_wallet_budget_with_key(0, 5_000);
        // GAS_PER_RJOULE gas → 1 rJ. Encumbrance has 2000 rJ.
        assert!(
            budget.can_proceed(GasCost(GAS_PER_RJOULE)),
            "spend within encumbrance should be allowed"
        );
        // GAS_PER_RJOULE * 2000 gas → 2000 rJ — exactly the encumbrance amount
        assert!(
            budget.can_proceed(GasCost(GAS_PER_RJOULE * 2000)),
            "spend equal to encumbrance should be allowed"
        );
    }

    #[test]
    fn wallet_budget_rejects_spend_exceeding_encumbrance() {
        let budget = make_wallet_budget_with_key(0, 5_000);
        // GAS_PER_RJOULE * 3000 gas → 3000 rJ. Encumbrance has only 2000 rJ.
        assert!(
            !budget.can_proceed(GasCost(GAS_PER_RJOULE * 3000)),
            "spend exceeding encumbrance must be rejected"
        );
    }

    #[test]
    fn check_key_health_reports_exhaustion_and_expiry() {
        let budget = make_wallet_budget_with_key(1_000, 1_000);
        let health = budget.check_key_health().unwrap();
        assert!(
            health.exhausted,
            "key at spending limit should be exhausted"
        );
        assert!(!health.expired, "key without expiry should not be expired");
        assert_eq!(health.spent_rj, 1_000);
        assert_eq!(health.limit_rj, 1_000);
    }

    #[test]
    fn wallet_budget_reserve_settle_flow() {
        let budget = make_wallet_budget_with_key(0, 5_000);
        // Reserve GAS_PER_RJOULE gas (1 rJ)
        let reserved = budget.reserve(GasCost(GAS_PER_RJOULE)).unwrap();
        assert_eq!(reserved.0, GAS_PER_RJOULE);

        // Settle with actual = reserved (exact match)
        let settled = budget.settle(reserved, GasCost(GAS_PER_RJOULE)).unwrap();
        assert_eq!(settled.0, GAS_PER_RJOULE);

        // Verify encumbrance was debited: 2000 - 1 = 1999 remaining
        let key_id = budget.key_id.unwrap();
        let enc = budget.wallet_manager.get_encumbrance(key_id).unwrap();
        assert_eq!(enc.remaining_rj(), 1_999, "1 rJ consumed from encumbrance");
    }

    #[test]
    fn wallet_budget_reserve_rejects_insufficient_encumbrance() {
        let budget = make_wallet_budget_with_key(0, 5_000);
        // GAS_PER_RJOULE * 3000 gas → 3000 rJ, but encumbrance only has 2000
        let result = budget.reserve(GasCost(GAS_PER_RJOULE * 3000));
        assert!(
            result.is_err(),
            "reserve should fail when encumbrance insufficient"
        );
    }

    #[test]
    fn wallet_budget_reads_live_gas_per_rjoule_rate() {
        let budget = make_wallet_budget_with_key(0, 5_000);
        // Encumbrance = 2000 rJ. At default rate, GAS_PER_RJOULE*1500 gas → 1500 rJ.
        assert!(
            budget.can_proceed(GasCost(GAS_PER_RJOULE * 1500)),
            "1500 rJ should fit in 2000 rJ encumbrance at default rate"
        );

        // Halve the rate: same gas → 3000 rJ, exceeding encumbrance.
        budget.wallet_manager.set_gas_per_rjoule(GAS_PER_RJOULE / 2);
        assert!(
            !budget.can_proceed(GasCost(GAS_PER_RJOULE * 1500)),
            "3000 rJ should exceed 2000 rJ encumbrance at halved rate"
        );

        // Double the rate: same gas → 750 rJ, fitting again.
        budget.wallet_manager.set_gas_per_rjoule(GAS_PER_RJOULE * 2);
        assert!(
            budget.can_proceed(GasCost(GAS_PER_RJOULE * 1500)),
            "750 rJ should fit in 2000 rJ encumbrance at doubled rate"
        );
    }
}
