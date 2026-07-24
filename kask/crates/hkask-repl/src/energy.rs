//! RAII gas governance for the REPL's hold-settle pattern.
//!
//! The REPL uses a two-phase gas accounting pattern:
//! 1. Reserve a heuristic estimate before inference
//! 2. Settle with the actual token cost after (success)
//!    OR release the reservation (failure — inference errored)
//!
//! EnergyGuard encapsulates this pattern as an owned-consumption guard.
//! `settle()` or `release()` must be called to consume the guard. If
//! neither is called (e.g., due to a panic), the `Drop` impl logs the
//! leak — the reserved gas remains encumbered until the session restarts.
//!
//! # REQ: P9-gas-settle — EnergyGuard guarantees gas is settled or released
//! expect: "I can access all hKask functionality through the kask CLI"

use hkask_pods::InferenceLoop;
use hkask_regulation::{CyberneticsLoop, GasCost};
use hkask_types::WebID;
use std::sync::Arc;
use tokio::sync::RwLock;

/// RAII guard for the hold-settle gas pattern.
///
/// Created via `try_reserve()`, which checks `can_proceed` and reserves
/// the heuristic amount. After inference:
/// - Call `settle(actual_cost)` on success — reconciles the reservation
///   with the actual token cost and syncs the InferenceLoop.
/// - Call `release()` on failure — returns the reservation to the budget
///   without adjustment (no inference happened, no cost incurred).
///
/// Both methods consume the guard. If neither is called (e.g., a panic
/// unwinds past the guard), `Drop` logs the leak as a warning. The
/// reserved gas stays encumbered until session restart — this is
/// acceptable because a panic typically requires a restart anyway.
pub struct EnergyGuard {
    cybernetics_loop: Arc<RwLock<CyberneticsLoop>>,
    inference_loop: Arc<InferenceLoop>,
    webid: WebID,
    rt: tokio::runtime::Handle,
    heuristic: u64,
    settled: bool,
}

impl EnergyGuard {
    /// Attempt to reserve gas for a pending operation.
    ///
    /// Returns `None` if the energy budget is exhausted (hard limit reached).
    /// On success, the heuristic amount is reserved and the guard is returned.
    pub fn try_reserve(
        cybernetics_loop: &Arc<RwLock<CyberneticsLoop>>,
        inference_loop: &Arc<InferenceLoop>,
        webid: &WebID,
        rt: &tokio::runtime::Handle,
        heuristic: u64,
    ) -> Option<Self> {
        let can = rt.block_on(async {
            cybernetics_loop
                .read()
                .await
                .can_proceed(webid, GasCost(heuristic))
                .await
        });
        if !can {
            return None;
        }
        let _ = rt.block_on(async {
            cybernetics_loop
                .read()
                .await
                .reserve_gas(webid, GasCost(heuristic))
                .await
        });
        Some(Self {
            cybernetics_loop: Arc::clone(cybernetics_loop),
            inference_loop: Arc::clone(inference_loop),
            webid: *webid,
            rt: rt.clone(),
            heuristic,
            settled: false,
        })
    }

    /// The heuristic cost used for reservation.
    pub fn heuristic(&self) -> u64 {
        self.heuristic
    }

    /// Settle gas with actual cost and sync InferenceLoop from L6 budget.
    /// Consumes self. Call after successful inference to reconcile the
    /// reservation with the actual token cost.
    ///
    /// # REQ: P9-gas-settle — EnergyGuard guarantees settle() is called exactly once
    /// expect: "I can access all hKask functionality through the kask CLI"
    pub fn settle(mut self, actual: u64) {
        let _ = self.rt.block_on(async {
            self.cybernetics_loop
                .read()
                .await
                .settle_gas(&self.webid, GasCost(self.heuristic), GasCost(actual))
                .await
        });
        // Sync InferenceLoop's sense signal from the authoritative L6 budget.
        if let Some(status) = self.rt.block_on(async {
            self.cybernetics_loop
                .read()
                .await
                .agent_gas_status(&self.webid)
                .await
        }) {
            self.inference_loop
                .sync_gas_state(status.remaining.as_raw(), status.cap.as_raw());
        }
        self.settled = true;
    }

    /// Release the gas reservation without settling. Call when inference
    /// failed and no actual cost was incurred — the reserved amount is
    /// returned to the budget without adjustment (settle_gas with
    /// actual == heuristic is a net-zero reconciliation).
    ///
    /// Does not sync InferenceLoop because no inference happened.
    pub fn release(mut self) {
        let _ = self.rt.block_on(async {
            self.cybernetics_loop
                .read()
                .await
                .settle_gas(
                    &self.webid,
                    GasCost(self.heuristic),
                    GasCost(self.heuristic),
                )
                .await
        });
        self.settled = true;
    }
}

impl Drop for EnergyGuard {
    fn drop(&mut self) {
        if !self.settled {
            tracing::warn!(
                target: "reg.gas",
                agent = %self.webid,
                heuristic = self.heuristic,
                "EnergyGuard dropped without settle/release — gas reservation leaked"
            );
        }
    }
}
