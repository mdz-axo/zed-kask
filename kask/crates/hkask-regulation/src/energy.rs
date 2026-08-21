//! Per-agent tool-call cap.
//!
//! Model: 1 unit = 1 governed tool invocation. Each agent has a hard ceiling on
//! calls per regulation cycle; the cap resets to the ceiling each tick. A call
//! either fits (`remaining > 0`) or it does not, and the regulation loop's
//! `EnergyBudgetSensor` reads the usage ratio for its throttle set-point.
//!
//! Curation can override an agent's ceiling (`OverrideEnergyBudget`), clear the
//! override (`ClearOverride`), or credit calls (`ReplenishBudget`); an override
//! survives per-tick resets until cleared.

use hkask_types::WebID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Alert threshold ratio (used/ceiling) above which the regulation loop throttles.
pub(crate) const DEFAULT_CALL_CAP_ALERT_THRESHOLD: f64 = 0.8;

/// Per-tick call ceiling applied to an agent the composition root never
/// registered.
///
/// This is a runaway-loop breaker, not a budget: it is set high enough that no
/// terminating task reaches it, and low enough that a non-terminating tool loop
/// cannot run unbounded between regulation ticks. An unregistered agent is
/// auto-registered at this ceiling rather than denied, because a missing
/// registration is a wiring omission, not an authorization decision.
pub const DEFAULT_RUNAWAY_CALL_CEILING: u32 = 10_000;

/// Result of metering one call against an agent's runaway ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallMeterOutcome {
    /// The call was charged against an already-registered ceiling.
    Charged,
    /// The agent had no ceiling; one was created at
    /// [`DEFAULT_RUNAWAY_CALL_CEILING`] and the call charged against it. The
    /// caller should log this once — it means the composition root did not seed
    /// this persona.
    AutoRegistered,
    /// The agent has burned its whole per-tick ceiling. The call must not
    /// proceed; the cap resets on the next regulation tick.
    CeilingReached { ceiling: u32 },
}

/// A hard per-agent call ceiling with a mutable remaining counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallCap {
    ceiling: u32,
    remaining: u32,
}

impl CallCap {
    #[must_use]
    pub fn new(ceiling: u32) -> Self {
        Self {
            ceiling,
            remaining: ceiling,
        }
    }

    #[must_use]
    pub fn ceiling(&self) -> u32 {
        self.ceiling
    }

    #[must_use]
    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    /// True if at least one call is still available.
    #[must_use]
    pub fn can_proceed(&self) -> bool {
        self.remaining > 0
    }

    /// Consume one call. Returns `false` (and leaves the counter at 0) if empty.
    pub fn charge(&mut self) -> bool {
        if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            false
        }
    }

    /// Credit `amount` calls, saturating at the ceiling.
    pub fn credit(&mut self, amount: u32) {
        self.remaining = self.remaining.saturating_add(amount).min(self.ceiling);
    }

    /// Reset remaining to the ceiling (called once per regulation tick).
    pub fn reset(&mut self) {
        self.remaining = self.ceiling;
    }

    /// Replace the ceiling (used by curation overrides). Clamps `remaining` down
    /// if it now exceeds the new ceiling.
    pub fn set_ceiling(&mut self, ceiling: u32) {
        self.ceiling = ceiling;
        if self.remaining > ceiling {
            self.remaining = ceiling;
        }
    }

    /// Ratio of the ceiling that has been consumed (0.0 = full, 1.0 = empty).
    #[must_use]
    pub fn usage_ratio(&self) -> f64 {
        1.0 - self.remaining as f64 / self.ceiling.max(1) as f64
    }

    /// Ratio of the ceiling still remaining (1.0 = full, 0.0 = empty).
    #[must_use]
    pub fn remaining_ratio(&self) -> f64 {
        self.remaining as f64 / self.ceiling.max(1) as f64
    }
}

/// Read-only status snapshot for sensors and status queries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentCallCapStatus {
    pub ceiling: u32,
    pub remaining: u32,
    pub usage_ratio: f64,
}

impl From<&CallCap> for AgentCallCapStatus {
    fn from(cap: &CallCap) -> Self {
        Self {
            ceiling: cap.ceiling,
            remaining: cap.remaining,
            usage_ratio: cap.usage_ratio(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CallCapError {
    #[error("call cap exceeded: remaining {remaining}, ceiling {ceiling}")]
    Exceeded { remaining: u32, ceiling: u32 },
    #[error("persistence: {0}")]
    Persistence(String),
}

/// A curation override: remembers the agent's original ceiling so `clear_override`
/// can restore it.
#[derive(Debug, Clone, Copy)]
struct OverrideRecord {
    original_ceiling: u32,
    override_ceiling: u32,
}

/// Per-agent call-cap registry with curation overrides.
///
/// The cap/override maps are interior-mutable (`Arc<RwLock<..>>`) so the manager
/// can be shared behind an `Arc<RwLock<CallCapManager>>` while still exposing
/// `&self` async methods — matching the prior budget manager shape callers
/// depend on. Agents without a registered cap are denied (fail-closed); the
/// composition root must seed a cap for every agent making governed tool calls.
pub(crate) struct CallCapManager {
    caps: Arc<RwLock<HashMap<WebID, CallCap>>>,
    overrides: Arc<RwLock<HashMap<WebID, OverrideRecord>>>,
}

impl Default for CallCapManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CallCapManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            caps: Arc::new(RwLock::new(HashMap::new())),
            overrides: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register (or replace) an agent's call cap.
    pub async fn register_call_cap(&self, agent: WebID, ceiling: u32) {
        self.caps.write().await.insert(agent, CallCap::new(ceiling));
    }

    /// Fail-closed: an agent with no registered cap cannot proceed.
    pub async fn can_proceed(&self, agent: &WebID) -> bool {
        self.caps
            .read()
            .await
            .get(agent)
            .is_some_and(CallCap::can_proceed)
    }

    /// Consume one call. Returns `Err` if the agent has no cap or it is empty.
    pub async fn charge(&self, agent: &WebID) -> Result<(), CallCapError> {
        let mut caps = self.caps.write().await;
        let cap = caps.get_mut(agent).ok_or_else(|| {
            CallCapError::Persistence(format!("no call cap registered for agent {agent}"))
        })?;
        if cap.charge() {
            Ok(())
        } else {
            Err(CallCapError::Exceeded {
                remaining: cap.remaining(),
                ceiling: cap.ceiling(),
            })
        }
    }

    /// Meter one call, auto-registering an unknown agent at
    /// [`DEFAULT_RUNAWAY_CALL_CEILING`].
    ///
    /// This is the tool-dispatch path's entry point. Unlike [`Self::charge`], an
    /// unregistered agent is not an error: the ceiling exists to break runaway
    /// loops and to meter usage, so a missing registration auto-creates one and
    /// lets the call through (reporting [`CallMeterOutcome::AutoRegistered`] so
    /// the caller can log the wiring gap). The single refusal is
    /// [`CallMeterOutcome::CeilingReached`].
    ///
    /// Charging is a single write-locked read-modify-write, so concurrent callers
    /// cannot both observe remaining capacity and both charge it.
    pub async fn charge_metered(&self, agent: &WebID) -> CallMeterOutcome {
        let mut caps = self.caps.write().await;
        match caps.get_mut(agent) {
            Some(cap) => {
                if cap.charge() {
                    CallMeterOutcome::Charged
                } else {
                    CallMeterOutcome::CeilingReached {
                        ceiling: cap.ceiling(),
                    }
                }
            }
            None => {
                let mut cap = CallCap::new(DEFAULT_RUNAWAY_CALL_CEILING);
                cap.charge();
                caps.insert(*agent, cap);
                CallMeterOutcome::AutoRegistered
            }
        }
    }

    /// Credit `amount` calls to an agent (saturating at the ceiling).
    pub async fn credit(&self, agent: &WebID, amount: u32) {
        if let Some(cap) = self.caps.write().await.get_mut(agent) {
            cap.credit(amount);
        }
    }

    /// Snapshot every agent's status.
    pub async fn all_agent_statuses(&self) -> Vec<(WebID, AgentCallCapStatus)> {
        self.caps
            .read()
            .await
            .iter()
            .map(|(id, cap)| (*id, AgentCallCapStatus::from(cap)))
            .collect()
    }

    /// Reset every registered cap to its ceiling (one regulation tick). Agents
    /// with an active curation override reset to the override ceiling.
    pub async fn reset_all(&self) {
        let overrides = self.overrides.read().await;
        let mut caps = self.caps.write().await;
        for (agent, cap) in caps.iter_mut() {
            if let Some(rec) = overrides.get(agent) {
                cap.set_ceiling(rec.override_ceiling);
            }
            cap.reset();
        }
    }

    /// Curation override: install a new ceiling for an agent. The override
    /// survives per-tick resets until `clear_override` is called, which restores
    /// the agent's original ceiling. A second override replaces the first and
    /// still remembers the *original* (pre-override) ceiling for restore.
    pub async fn apply_override(&self, agent: WebID, ceiling: u32) {
        let mut overrides = self.overrides.write().await;
        let original = match overrides.get(&agent) {
            Some(rec) => rec.original_ceiling, // already overridden — keep the true original
            None => {
                let caps = self.caps.read().await;
                caps.get(&agent).map_or(ceiling, CallCap::ceiling)
            }
        };
        overrides.insert(
            agent,
            OverrideRecord {
                original_ceiling: original,
                override_ceiling: ceiling,
            },
        );
        drop(overrides);
        let mut caps = self.caps.write().await;
        if let Some(cap) = caps.get_mut(&agent) {
            cap.set_ceiling(ceiling);
            cap.reset();
        }
    }

    /// Remove a curation override, restoring the agent's original ceiling and
    /// resetting its remaining calls to that ceiling.
    pub async fn clear_override(&self, agent: WebID) {
        let removed = self.overrides.write().await.remove(&agent);
        if let Some(rec) = removed {
            let mut caps = self.caps.write().await;
            if let Some(cap) = caps.get_mut(&agent) {
                cap.set_ceiling(rec.original_ceiling);
                cap.reset();
            }
        }
    }

    /// Read access to the cap map (for persistence snapshots).
    pub async fn caps(&self) -> tokio::sync::RwLockReadGuard<'_, HashMap<WebID, CallCap>> {
        self.caps.read().await
    }

    /// Write access to the cap map (for restoring persisted state).
    pub async fn caps_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, HashMap<WebID, CallCap>> {
        self.caps.write().await
    }
}
