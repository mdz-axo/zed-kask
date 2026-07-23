//! Gas Budget Management — Registration, reservation, settlement, and replenishment
//!
//! Extracted from `CyberneticsLoop` per the Fowler audit (H8). Two consumers
//! justify the extraction: `CyberneticsLoop` (production regulation) and
//! `GovernedTool` (tool invocation membrane). The hold-settle pattern
//! (reserve → call → settle) is the core contract.
//!
//! # Gas Budget Lifecycle
//!
//! 1. **Register** — `register_gas_budget()` creates a budget for an agent
//! 2. **Reserve** — `reserve_gas()` holds budget for estimated cost
//! 3. **Settle** — `settle_gas()` adjusts to actual cost, refunds difference
//! 4. **Replenish** — `replenish_all_budgets()` / `replenish_agent_budget()` restore capacity
//!
//! # Metacognitive Override
//!
//! Curation can override an agent's budget via `apply_override_gas_budget()`.
//! Overridden agents are skipped during `replenish_all_budgets()` to preserve
//! the Curation directive. `apply_clear_override()` resumes normal replenishment.

use crate::energy::{AgentGasStatus, GasBudget, GasCost, GasError};
use crate::wallet_budget::WalletBackedBudget;
use crate::wallet_manager::WalletManager;
use hkask_types::WebID;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Record of an active Curation override on an agent's gas budget.
///
/// When Curation issues an `OverrideEnergyBudget` directive, the override is
/// recorded here so that `replenish_all_budgets()` does not overwrite it
/// on the next regulation cycle. This preserves the metacognitive override
/// mechanism — the core safety feature that lets Curation exceed
/// Cybernetics' set-point range.
struct OverrideRecord {
    /// When this override was issued (for TTL expiry)
    issued_at: chrono::DateTime<chrono::Utc>,
    /// \[NORMATIVE\] TTL in seconds (0 = no expiry, must be explicitly cleared) (P9 — Homeostatic Self-Regulation).
    ttl_secs: u64,
}

/// Gas Budget Manager — registration, reservation, settlement, and replenishment.
///
/// Owns the gas budget map and active override tracking. Extracted from
/// `CyberneticsLoop` to concentrate gas budget logic and allow direct access
/// from `GovernedTool` without going through the full loop.
pub struct GasBudgetManager {
    gas_budgets: Arc<RwLock<HashMap<WebID, GasBudget>>>,
    /// Wallet-backed budgets — checked before gas budgets.
    /// When an agent has a wallet budget, gas operations debit rJoules
    /// instead of consuming from the dimensionless gas pool.
    wallet_budgets: Arc<RwLock<HashMap<WebID, WalletBackedBudget>>>,
    active_overrides: Arc<RwLock<HashMap<WebID, OverrideRecord>>>,
    /// Previous remaining values for consumption velocity computation.
    previous_remaining: RwLock<HashMap<WebID, u64>>,
    /// SQLite-backed wallet manager for gas wallets (optional).
    wallet_manager: Option<Arc<WalletManager>>,
}

impl Default for GasBudgetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GasBudgetManager {
    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Create a new `GasBudgetManager` with empty budget and override maps.
    pub fn new() -> Self {
        Self {
            gas_budgets: Arc::new(RwLock::new(HashMap::new())),
            wallet_budgets: Arc::new(RwLock::new(HashMap::new())),
            active_overrides: Arc::new(RwLock::new(HashMap::new())),
            previous_remaining: RwLock::new(HashMap::new()),
            wallet_manager: None,
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Register a gas budget for an agent.
    pub async fn register_gas_budget(&self, agent: WebID, budget: GasBudget) {
        let mut budgets = self.gas_budgets.write().await;
        budgets.insert(agent, budget);
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Attach a WalletManager for gas wallet enforcement.
    pub fn set_wallet_manager(&mut self, mgr: Arc<WalletManager>) {
        self.wallet_manager = Some(mgr);
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Register a wallet-backed budget for an agent.
    /// Wallet budgets are checked before gas budgets.
    pub async fn register_wallet_budget(&self, agent: WebID, budget: WalletBackedBudget) {
        let mut budgets = self.wallet_budgets.write().await;
        budgets.insert(agent, budget);
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Check whether an agent can proceed with the given energy cost estimate.
    ///
    /// Checks wallet budgets first, then gas budgets.
    /// Returns `true` if the agent has no registered budget (soft limit)
    /// or if the budget has sufficient remaining capacity.
    pub async fn can_proceed(&self, agent: &WebID, gas: GasCost) -> bool {
        // 1. Check SQLite-backed WalletManager
        if let Some(ref wm) = self.wallet_manager
            && wm.has_wallet(agent).await
        {
            return wm.can_proceed(agent, gas).await;
        }
        // 2. Check WalletBackedBudget (rJoule-backed, Hedera)
        let wallet_budgets = self.wallet_budgets.read().await;
        if let Some(budget) = wallet_budgets.get(agent) {
            return budget.can_proceed(gas);
        }
        drop(wallet_budgets);
        // 3. Fall back to gas budget
        let budgets = self.gas_budgets.read().await;
        if let Some(budget) = budgets.get(agent) {
            budget.can_proceed(gas)
        } else {
            true
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Returns `None` if agent has no registered budget.
    pub async fn agent_gas_status(&self, agent: &WebID) -> Option<AgentGasStatus> {
        let budgets = self.gas_budgets.read().await;
        budgets.get(agent).map(AgentGasStatus::from)
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Hold-settle pattern: gas reserved but not consumed. Call `settle_gas()` after.
    /// Checks wallet budgets first, then gas budgets.
    pub async fn reserve_gas(&self, agent: &WebID, gas: GasCost) -> Result<GasCost, GasError> {
        // 1. Check SQLite-backed WalletManager
        if let Some(ref wm) = self.wallet_manager
            && wm.has_wallet(agent).await
        {
            return wm.spend(agent, gas).await;
        }
        // 2. Check WalletBackedBudget (rJoule)
        let wallet_budgets = self.wallet_budgets.read().await;
        if let Some(budget) = wallet_budgets.get(agent) {
            return budget.reserve(gas);
        }
        drop(wallet_budgets);
        // 3. Fall back to gas budget
        let mut budgets = self.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(agent) {
            budget.reserve(gas)
        } else {
            Ok(GasCost::ZERO)
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Settle gas: if actual < reserved, the difference is refunded.
    /// Checks wallet budgets first, then gas budgets.
    pub async fn settle_gas(
        &self,
        agent: &WebID,
        reserved_gas: GasCost,
        actual_gas: GasCost,
    ) -> Result<GasCost, GasError> {
        // 1. Wallet manager: spend already deducted, no hold-settle needed
        if let Some(ref wm) = self.wallet_manager
            && wm.has_wallet(agent).await
        {
            return Ok(actual_gas); // already spent during reserve
        }
        // 2. Wallet-backed budget (rJoule)
        let wallet_budgets = self.wallet_budgets.read().await;
        if let Some(budget) = wallet_budgets.get(agent) {
            return budget.settle(reserved_gas, actual_gas);
        }
        drop(wallet_budgets);
        // 3. Fall back to gas budget
        let mut budgets = self.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(agent) {
            budget.settle(reserved_gas, actual_gas)
        } else {
            Ok(GasCost::ZERO)
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// For estimated cost, prefer `reserve_gas` + `settle_gas`.
    pub async fn acquire_budget(&self, agent: &WebID, gas: GasCost) -> Result<GasCost, GasError> {
        let mut budgets = self.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(agent) {
            budget.consume(gas)
        } else {
            // No budget registered — cost is 0 (soft limit)
            Ok(GasCost::ZERO)
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Replenish all registered budgets, skipping agents with active Curation overrides.
    pub async fn replenish_all_budgets(&self) {
        // G9: Auto-release stale reservations before replenishing
        {
            let mut budgets = self.gas_budgets.write().await;
            for (agent, budget) in budgets.iter_mut() {
                if let Some(stale) = budget.stale_reservation() {
                    budget.release_stale_reservation();
                    tracing::warn!(
                        target: "reg.cybernetics",
                        agent = %agent,
                        released = stale.0,
                        "Auto-released stale gas reservation — caller never settled"
                    );
                }
            }
        }

        let budget_ids: Vec<WebID> = {
            let budgets = self.gas_budgets.read().await;
            budgets.keys().cloned().collect()
        };
        let overrides = self.active_overrides.read().await;
        for agent in budget_ids {
            if overrides.contains_key(&agent) {
                // Skip replenishment for agents with active Curation overrides
                continue;
            }
            let replenished = {
                let mut budgets = self.gas_budgets.write().await;
                if let Some(budget) = budgets.get_mut(&agent) {
                    let rate = budget.replenish_rate();
                    budget.replenish();
                    rate
                } else {
                    GasCost::ZERO
                }
            };
            if replenished.0 > 0 {
                tracing::debug!(
                    target: "reg.cybernetics",
                    agent = %agent,
                    replenish_rate = replenished.0,
                    "Replenished gas budget"
                );
            }
        }

        // G8: Consumption velocity — compute gas burned per agent since last cycle
        {
            let budgets = self.gas_budgets.read().await;
            let mut prev = self.previous_remaining.write().await;
            for (agent, budget) in budgets.iter() {
                let current = budget.remaining().0;
                if let Some(&previous) = prev.get(agent)
                    && current < previous
                {
                    let burned = previous - current;
                    tracing::debug!(
                        target: "reg.cybernetics",
                        agent = %agent,
                        gas_burned = burned,
                        remaining = current,
                        cap = budget.cap().0,
                        "Gas consumption velocity"
                    );
                }
                prev.insert(*agent, current);
            }
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Used by `CuratorDirective::ReplenishBudget`.
    pub async fn replenish_agent_budget(&self, agent: &WebID, amount: GasCost) {
        let mut budgets = self.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(agent) {
            budget.replenish_by(amount);
            tracing::info!(
                target: "reg.cybernetics",
                agent = %agent,
                amount = %amount,
                remaining = %budget.remaining(),
                "Replenished agent gas budget by directive"
            );
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Metacognitive override — recorded in active_overrides so replenish skips this agent.
    pub async fn apply_override_gas_budget(&self, agent: WebID, new_budget: GasCost) {
        // Default TTL of 0 means override persists until explicitly cleared
        let ttl_secs: u64 = 0;
        let mut budgets = self.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(&agent) {
            budget.reset_to(new_budget);
            tracing::warn!(
                target: "reg.cybernetics",
                agent = %agent,
                new_budget = %new_budget,
                "Applied OverrideEnergyBudget directive from Curation (set-point override)"
            );
        } else {
            budgets.insert(agent, GasBudget::new(new_budget));
            tracing::warn!(
                target: "reg.cybernetics",
                agent = %agent,
                new_budget = %new_budget,
                "Registered new gas budget from OverrideEnergyBudget directive"
            );
        }
        drop(budgets);
        // Record the override so replenish_all_budgets() skips this agent
        let mut overrides = self.active_overrides.write().await;
        overrides.insert(
            agent,
            OverrideRecord {
                issued_at: chrono::Utc::now(),
                ttl_secs,
            },
        );
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Removes agent from active_overrides, resuming normal replenishment.
    pub async fn apply_clear_override(&self, agent: WebID) {
        let mut overrides = self.active_overrides.write().await;
        if overrides.remove(&agent).is_some() {
            tracing::info!(
                target: "reg.cybernetics",
                agent = %agent,
                "Cleared Curation override — normal replenishment resumes"
            );
        } else {
            tracing::debug!(
                target: "reg.cybernetics",
                agent = %agent,
                "ClearOverride directive received but no active override found"
            );
        }
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Priority-scaled: when priority is provided, replenishment is weighted.
    pub async fn apply_replenish_budget(
        &self,
        agent: WebID,
        amount: GasCost,
        priority: Option<f64>,
    ) {
        let mut budgets = self.gas_budgets.write().await;
        if let Some(budget) = budgets.get_mut(&agent) {
            let replenished = if let Some(p) = priority {
                budget.replenish_by_weighted(amount, p)
            } else {
                budget.replenish_by(amount);
                GasCost(amount.0.min(budget.cap().0 - budget.remaining().0))
            };
            drop(budgets);
            tracing::info!(
                target: "reg.cybernetics",
                agent = %agent,
                amount = %amount,
                priority = priority,
                replenished = %replenished,
                "Replenished agent gas budget by directive"
            );
        }
    }
    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Expire overrides with non-zero TTL that have passed their expiry time.
    /// Called during the Cybernetics Loop's sense→compare→compute→act cycle.
    pub async fn expire_overrides(&self) {
        let mut overrides = self.active_overrides.write().await;
        let now = chrono::Utc::now();
        overrides.retain(|agent, record| {
            if record.ttl_secs == 0 {
                return true; // No TTL — persists until explicitly cleared
            }
            let expires_at = record.issued_at + chrono::Duration::seconds(record.ttl_secs as i64);
            if now > expires_at {
                tracing::info!(
                    target: "reg.cybernetics",
                    agent = %agent,
                    "Curation override expired — resuming normal replenishment"
                );
                false
            } else {
                true
            }
        });
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Return all registered agent budgets and their current status.
    pub async fn all_agent_statuses(&self) -> Vec<(WebID, AgentGasStatus)> {
        let budgets = self.gas_budgets.read().await;
        budgets
            .iter()
            .map(|(id, budget)| (*id, AgentGasStatus::from(budget)))
            .collect()
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Access the raw budget map (for serialization).
    pub async fn gas_budgets(&self) -> tokio::sync::RwLockReadGuard<'_, HashMap<WebID, GasBudget>> {
        self.gas_budgets.read().await
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Mutable access for restoring from persistence.
    pub async fn gas_budgets_mut(
        &self,
    ) -> tokio::sync::RwLockWriteGuard<'_, HashMap<WebID, GasBudget>> {
        self.gas_budgets.write().await
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Return wallet-backed agents whose balance is zero.
    pub async fn wallet_exhausted_agents(&self) -> Vec<WebID> {
        let wallets = self.wallet_budgets.read().await;
        wallets
            .iter()
            .filter(|(_, wb)| !wb.can_proceed(GasCost(1)))
            .map(|(id, _)| *id)
            .collect()
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Serialize all budgets to a JSON file for persistence across restarts.
    pub async fn save_all(&self, path: &std::path::Path) -> Result<(), GasError> {
        let budgets = self.gas_budgets.read().await;
        let wrapper = serde_json::json!({
            "version": 1,
            "budgets": &*budgets,
        });
        let json = serde_json::to_string_pretty(&wrapper)
            .map_err(|e| GasError::Persistence(format!("serialize: {e}")))?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                GasError::Persistence(format!("create_dir {}: {e}", parent.display()))
            })?;
        }
        tokio::fs::write(path, &json)
            .await
            .map_err(|e| GasError::Persistence(format!("write {}: {e}", path.display())))?;
        Ok(())
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    pub async fn load_all(&self, path: &std::path::Path) -> Result<usize, GasError> {
        let contents = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => {
                return Err(GasError::Persistence(format!(
                    "read {}: {e}",
                    path.display()
                )));
            }
        };
        let wrapper: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|e| GasError::Persistence(format!("parse {}: {e}", path.display())))?;
        let version = wrapper.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
        if version != 1 {
            return Err(GasError::Persistence(format!(
                "Unknown persistence version {} in {}",
                version,
                path.display()
            )));
        }
        let loaded: std::collections::HashMap<WebID, GasBudget> =
            serde_json::from_value(wrapper["budgets"].clone()).map_err(|e| {
                GasError::Persistence(format!("parse budgets {}: {e}", path.display()))
            })?;
        let count = loaded.len();
        let mut budgets = self.gas_budgets.write().await;
        for (id, budget) in loaded {
            budgets.insert(id, budget);
        }
        Ok(count)
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Iterate over wallet-backed budgets to produce wallet health signals.
    /// Returns `(balance_ratio, cap_ratio)` for each wallet-backed agent.
    /// balance_ratio: 0.0 = empty, 1.0 = full (relative to a nominal capacity).
    pub async fn wallet_balance_ratios(&self) -> Vec<(f64, f64)> {
        let budgets = self.wallet_budgets.read().await;
        let mut ratios = Vec::new();
        for budget in budgets.values() {
            // Get the wallet balance and compute a ratio.
            // We use a nominal capacity of 1_000_000 rJ as the denominator
            // (this is a simplified model — production would use 30-day moving avg).
            match budget.wallet_manager.get_balance(budget.wallet_id) {
                Ok(balance) => {
                    let nominal_cap: f64 = 1_000_000.0; // 1M rJ nominal capacity
                    let ratio = (balance.rjoules as f64 / nominal_cap).clamp(0.0, 1.0);
                    ratios.push((ratio, 1.0));
                }
                Err(_) => {
                    // Wallet error → treat as empty
                    ratios.push((0.0, 1.0));
                }
            }
        }
        ratios
    }

    /// expect: "The system manages per-agent energy budgets with depletion detection, replenishment, and budget-aware querying"
    /// Check API key health for all wallet-backed budgets.
    /// Returns `(agent, reason)` for each key that is exhausted or expired.
    pub async fn wallet_key_alerts(&self) -> Vec<(WebID, String)> {
        let budgets = self.wallet_budgets.read().await;
        let mut alerts = Vec::new();
        for (agent, budget) in budgets.iter() {
            if let Some(health) = budget.check_key_health() {
                if health.exhausted {
                    alerts.push((*agent, "key_exhausted".into()));
                }
                if health.expired {
                    alerts.push((*agent, "key_expired".into()));
                }
            }
        }
        alerts
    }
}
