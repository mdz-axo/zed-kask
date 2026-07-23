//! The Loop trait — sense → compare → compute → act → verify.
//!
//! This trait remains in hkask-regulation (not moved to hkask-types) because
//! external crates implement it for foreign types (e.g.,
//! `impl Loop for RwLock<CyberneticsLoop>`), which would violate the
//! orphan rule if the trait lived in hkask-types.
//!
//! The data types (Signal, Deviation, RegulatoryAction, etc.) are in hkask-types.

use hkask_types::loops::{Deviation, LoopId, RegulatoryAction, Signal};

/// A self-regulating loop — sense → compare → compute → act → verify.
///
/// Every loop implements this cycle. Authority flows downward
/// through the DAG: Curation → Cybernetics → domain loops.
///
/// Loop categories (Fermi-inspired distinction):
/// - **Model-fitting loops** adjust their own parameters (set-points, budgets,
///   thresholds) and can receive `Calibrate` directives. Implemented by
///   `CyberneticsLoop`.
/// - **Execution loops** do not self-calibrate — they execute within fixed
///   parameters (e.g., `SnapshotLoop`).
///
/// All async methods return `Send` futures so loops can run in
/// async tasks without `static` bounds issues.
#[async_trait::async_trait]
pub trait Loop: Send + Sync {
    fn id(&self) -> LoopId;

    /// Sense: observe current state and produce afferent signals.
    async fn sense(&self) -> Vec<Signal>;

    /// Compare: detect deviations from set-points.
    async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        signals.iter().filter_map(Deviation::from_signal).collect()
    }

    /// Compute: produce regulatory actions for detected deviations.
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction>;

    /// Act: execute regulatory actions (route through Communication Loop).
    async fn act(&self, actions: &[RegulatoryAction]);

    /// Verify: measure whether the previous cycle's actions improved their
    /// targeted metrics. Default no-op; model-fitting loops override this.
    ///
    /// Fermi pattern: the "impact gate" — after acting, re-read the targeted
    /// metric and compare against the pre-action value. Actions that repeatedly
    /// fail to improve should escalate rather than cycling in place.
    async fn verify_impact(
        &self,
        _previous_actions: &[RegulatoryAction],
    ) -> Vec<hkask_types::loops::ImpactReport> {
        Vec::new()
    }

    /// Full regulation cycle: sense → compare → compute → act → verify.
    ///
    /// Domain loops that override `tick()` must call `verify_impact` and
    /// propagate results (e.g., via Regulation spans or LoopMetrics computation)
    /// to close the cybernetic feedback loop.
    async fn tick(&self) {
        let signals = self.sense().await;
        let deviations = self.compare(&signals).await;
        let actions = self.compute(&deviations).await;
        self.act(&actions).await;
        let _impact = self.verify_impact(&actions).await;
        // Default impl logs but does not propagate — domain loops MUST override
        // tick() to wire impact reports into their LoopMetrics and Regulation spans.
        if !_impact.is_empty() {
            tracing::debug!(
                target: "hkask.loop",
                impact_count = _impact.len(),
                "Default tick(): verify_impact produced {} reports — override tick() to consume them",
                _impact.len()
            );
        }
    }
}
