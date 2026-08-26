//! Cybernetics Loop — Homeostatic self-regulation (Loop 6)
//!
//! The Cybernetics Loop is a closed-loop controller, not a passive observer.
//! Its functional contract:
//!
//! 1. **Sense** — receive `reg.*` spans from all loops (tool invocations,
//!    prompt outcomes, agent pod lifecycle, connector I/O).
//! 2. **Compare** — evaluate each signal against homeostatic set-points:
//!    call-cap remaining, variety counter balance, error rate threshold,
//!    connector latency envelope.
//! 3. **Compute** — when a signal deviates beyond its set-point, produce an
//!    efferent signal: throttle, escalate, calibrate, or circuit-break.
//! 4. **Act** — dispatch the efferent signal to the target loop's `regulate`
//!    entry point.
//!
//! The loop is self-stabilizing: if the Cybernetics Loop itself becomes unstable
//! (e.g., alert cascade), the Curation Loop detects it via metacognitive monitoring
//! and intervenes. This is the two-level meta-loop stability guarantee.
//!
//! # Essential Subloops
//!
//! - 6.1 Access Guard (GUARD) — OCAP verification + sovereignty enforcement
//! - 6.3 Variety Sensing (SENSE) — measure variety across domains
//! - 6.4 Algedonic Regulation (ADAPT) — deficit → threshold → escalate
//! - 6.6 Revocation (WITHDRAW) — persistent deny-future
//!
//! Energy homeostasis is NOT a subloop — it is expressed as set-points
//! in `SetPoints` + regulation actions via `InferenceRegulation`.

mod cycle;
mod directive;

use crate::dampener::{Dampener, StagnationDetector};

/// Why a [`RolloutEventSource`] call could not be served.
///
/// The regulation crate owns the port but not the storage engine, so the
/// adapter's concrete error type is not nameable here. Each variant carries
/// the adapter's rendered detail; the variant itself is the part the loop
/// reasons about (a failed read blinds `verify_impact`; a failed write-back
/// only loses the persisted verdict).
#[derive(Debug, thiserror::Error)]
pub enum RolloutEventError {
    /// The backing store could not be queried for the rollout's events. The
    /// loop cannot tell "no baseline" from "store down", so it must warn
    /// rather than treat the absence as a measurement.
    #[error("event store query failed: {detail}")]
    Query { detail: String },
    /// The impact verdict could not be appended. The loop's in-memory
    /// `ImpactReport` is unaffected; only the durable record is lost.
    #[error("impact verdict write-back failed: {detail}")]
    WriteBack { detail: String },
}

/// A read-and-write view of rollout events for impact verification and
/// impact- verdict write-back (event-substrate phase 6). The regulation
/// crate defines the port; the swarm side implements it over
/// `hkask-event-store`. This keeps the regulation crate dependency-light
/// (no storage dep) while letting `verify_impact` answer "for rollout R,
/// what was the metric before action A and after it?" as a query instead
/// of a special-case struct walk, and write its impact verdict back so
/// downstream consumers (training bridge, regression monitor, ORIENT)
/// can see "the regulation system verified this action's impact."
///
/// Canonical model: Agent Lightning's `RewardData.source` — the
/// regulation loop's impact verdict is a `regulation_impact`-sourced
/// verdict event, distinct from `deterministic_evaluator` (the harness's
/// check) and `operator` (a human stamp).
pub trait RolloutEventSource: Send + Sync {
    /// The value of `metric` for `rollout_id` at the event position
    /// `before_position` (the last event before the action) and at the
    /// rollout's end. `None` when the rollout has no event for that metric
    /// — absence, not zero (a fabricated 0 would read as a real measurement).
    fn metric_before_and_after(
        &self,
        rollout_id: &str,
        metric: &str,
        before_position: i64,
    ) -> Result<Option<(f64, f64)>, RolloutEventError>;

    /// Write the regulation loop's impact verdict back to the event store
    /// as a `verdict` event with `source: regulation_impact`. Closes the
    /// feedback loop: the loop measured the action's impact and persists
    /// its judgment so downstream consumers can see it alongside the
    /// deterministic-evaluator verdicts the harness wrote.
    ///
    /// Takes primitives (not `VerdictSource`) so the regulation crate stays
    /// dependency-light — the adapter maps to the typed wire string.
    ///
    /// `before`/`after` are the metric values the loop measured.
    /// `improved` is the loop's directional judgment. `decision` is the
    /// `ActionDecision` string ("Accept"/"Worsen"/"Block").
    ///
    /// A write failure returns `Err` — the caller warns and continues
    /// (the loop's internal `ImpactReport` is unaffected; only the
    /// store write-back is lost). Never silently drops.
    fn append_impact_verdict(
        &self,
        rollout_id: &str,
        metric: &str,
        before: f64,
        after: f64,
        improved: bool,
        decision: &str,
    ) -> Result<(), RolloutEventError>;
}
use crate::energy::{CallCapManager, CallMeterOutcome};
use crate::sensor_provider::{EnergyBudgetSensor, SensorBus, VarietySensor};

use crate::runtime::{RegulationCycleEntry, RegulationLedger};
use crate::set_points::SetPoints;
use crate::strategy_evaluator::StrategyEvaluator;
use crate::system_simulator::MovingAverageExtrapolator;

use crate::loops::RegulationData;
use crate::loops::{
    ActionDecision, ActionType, CurationInput, LoopId, LoopMetrics, RegulatoryAction,
    RegulatoryActionParams, TriggerOrigin,
};

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use hkask_types::event::{RegulationSink, SpanKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::{RwLock, mpsc};

/// Runtime-calibratable regulation thresholds — mutable layer over `SetPoints` defaults.
struct CalibratedThresholds {
    stagnation_thresholds: HashMap<String, u32>,
    block_worsening_ratio: f64,
    substitution_after: u32,
}

/// The Cybernetics Loop — homeostatic self-regulation.
///
/// Implements the sense→compare→compute→act regulation cycle.
/// The Cybernetic Loop regulates all domain loops (Inference, Memory)
/// and may signal the Curation Loop via algedonic
/// alerts. It may NOT regulate the Curation Loop.
pub struct CyberneticsLoop {
    ledger: Arc<RwLock<RegulationLedger>>,
    call_cap_manager: Arc<RwLock<CallCapManager>>,
    set_points: SetPoints,
    /// Cascade detection — prevents unbounded sense→act cycles
    max_iterations: u32,
    dampener: Arc<Dampener>,
    /// When present, algedonic alerts are persisted to RegulationArchive for restart durability.
    event_sink: Option<Arc<dyn RegulationSink>>,
    /// When present, algedonic alerts are persisted to the reviewable escalation
    /// queue (the `EscalationQueue` on the curator's curator.db). This is the
    /// primary durable path for alert review — every escalated alert is written
    /// here unconditionally, so the Curator/user can review pending alerts via
    /// the `curator_escalations` MCP tool and resolve/dismiss them. The
    /// `event_sink` (`RegulationArchive`) remains as a secondary fallback for
    /// restart durability when this queue is unavailable.
    alert_escalation_sink: Option<Arc<dyn crate::algedonic::AlertEscalationSink>>,
    /// Direct alerts channel: Cybernetics → Curation (CurationInput).
    alerts_tx: Option<mpsc::UnboundedSender<CurationInput>>,
    alert_email_sink: Option<Arc<dyn crate::algedonic::AlertEmailSink>>,
    /// Direct tool consumption channel: McpRuntime::invoke → Cybernetics.
    /// Direct curator directive channel: Curation → Cybernetics.
    curator_directive_rx: Option<Arc<RwLock<mpsc::UnboundedReceiver<CuratorDirective>>>>,
    /// Externally-submitted rollout impact checks, drained by the next
    /// `tick`'s `verify_impact`. Producers (the rollout harness, the
    /// Curator) submit a `RolloutImpactCheck` when they want the loop to
    /// verify a rollout's metric movement across an action — this is the
    /// producer side of the event-substrate phase 6 seam.
    submitted_rollout_checks: tokio::sync::Mutex<Vec<RegulatoryAction>>,
    /// Loop-quality telemetry from the most recent tick cycle.
    loop_quality: RwLock<LoopMetrics>,
    /// Path for persisting call caps across restarts.
    budget_persistence_path: Option<std::path::PathBuf>,
    /// Detects regulatory plateaus — repeated ineffective (metric, action) pairs.
    /// Fermi-inspired early-stopping pattern for cybernetic regulation.
    stagnation_detector: Arc<StagnationDetector>,
    /// Pluggable metric sensors (Fermi Extractor pattern).
    sensor_registry: Arc<SensorBus>,
    /// Statistical learner for per-tool cost distributions and reliability.
    /// Multi-model strategy evaluator (Fermi improvement-loop pattern).
    strategy_evaluator: Mutex<StrategyEvaluator>,
    /// Predictive simulator for anticipatory regulation (Fermi dynamics pattern).
    simulator: MovingAverageExtrapolator,
    /// Runtime-calibratable thresholds — updated by `SetPointCalibrator` background task.
    calibrated_thresholds: Arc<RwLock<CalibratedThresholds>>,
    /// Optional rollout event source (event-substrate phase 6). When wired,
    /// `verify_impact` queries it for before/after metric values on rollouts
    /// the action targeted — the store becomes the impact data plane and the
    /// struct-walk below becomes the fallback instead of the only path.
    rollout_events: Option<Arc<dyn RolloutEventSource>>,
}

impl CyberneticsLoop {
    /// Create a new CyberneticsLoop with default set-points.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    pub fn new(ledger: Arc<RwLock<RegulationLedger>>) -> Self {
        Self::build(ledger, SetPoints::default())
    }

    /// Create a new CyberneticsLoop with custom set-points.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self with custom SetPoints applied at construction
    pub fn with_set_points(ledger: Arc<RwLock<RegulationLedger>>, set_points: SetPoints) -> Self {
        Self::build(ledger, set_points)
    }

    fn build(ledger: Arc<RwLock<RegulationLedger>>, set_points: SetPoints) -> Self {
        let dampener = Arc::new(Dampener::with_windows(
            std::time::Duration::from_secs(set_points.dampen_window_secs),
            std::time::Duration::from_secs(set_points.metacognitive_window_secs),
            std::time::Duration::from_secs(set_points.override_cooldown_secs),
        ));
        let max_iterations = set_points.max_iterations;
        let stagnation_detector = Arc::new(
            StagnationDetector::new(crate::set_points::DEFAULT_STAGNATION_THRESHOLD)
                .with_per_metric_thresholds(set_points.stagnation_thresholds.clone()),
        );
        let call_cap_manager = Arc::new(RwLock::new(CallCapManager::new()));
        let calibrated_thresholds = Arc::new(RwLock::new(CalibratedThresholds {
            stagnation_thresholds: set_points.stagnation_thresholds.clone(),
            block_worsening_ratio: set_points.block_worsening_ratio,
            substitution_after: set_points.substitution_after,
        }));
        let sensor_registry = {
            let registry = SensorBus::new();
            registry.register(Arc::new(EnergyBudgetSensor::new(
                Arc::clone(&call_cap_manager),
                set_points.energy_min_remaining,
            )));
            registry.register(Arc::new(VarietySensor::new(
                Arc::clone(&ledger),
                set_points.variety_max_deficit,
            )));
            let trace_dir = match std::env::var("HKASK_TRACE_DIR") {
                Ok(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
                _ => {
                    hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new("traces"))
                }
            };
            registry.register(Arc::new(crate::sensor_provider::TestCoverageSensor::new(
                trace_dir.clone(),
                set_points.coverage_floor,
            )));
            registry.register(Arc::new(crate::sensor_provider::MutationScoreSensor::new(
                trace_dir,
                set_points.mutation_score_floor,
            )));
            registry.register(Arc::new(
                crate::sensor_provider::ToolReliabilitySensor::new(
                    Arc::clone(&ledger),
                    set_points.tool_reliability_threshold,
                ),
            ));
            Arc::new(registry)
        };

        // F5: Warn about metrics that have policy rules but no sensor.
        // The policy has 30 rules covering 29 SignalMetric variants, but only
        // 5 sensors are registered (EnergyRemaining, VarietyDeficit,
        // TestCoverage, MutationScore, ToolReliability) plus one inline check
        // (AlgedonicLogApproachingCap). The remaining 23 metrics are blind —
        // their policy rules can never fire because no signal is ever
        // produced for them. This is a variety deficit on the sensing side
        // (Ashby's Law: the regulator's sensing variety must match the
        // system's disturbance variety). Adding sensors for these metrics
        // is a follow-up; the warn makes the gap visible at startup.
        {
            use crate::loops::SignalMetric;
            const SENSED: &[SignalMetric] = &[
                SignalMetric::EnergyRemaining,
                SignalMetric::VarietyDeficit,
                SignalMetric::TestCoverage,
                SignalMetric::MutationScore,
                SignalMetric::ToolReliability,
                SignalMetric::AlgedonicLogApproachingCap,
                SignalMetric::InferenceAvailable,
                SignalMetric::TripleCount,
                SignalMetric::LowConfidenceCount,
                SignalMetric::ConsolidationCandidates,
                SignalMetric::StorageUsage,
                SignalMetric::MemoryLife,
            ];
            const ALL_METRICS: &[SignalMetric] = &[
                SignalMetric::EnergyRemaining,
                SignalMetric::VarietyDeficit,
                SignalMetric::ErrorRate,
                SignalMetric::ConnectorLatency,
                SignalMetric::CommunicationQueueDepth,
                SignalMetric::StorageUsage,
                SignalMetric::MemoryLife,
                SignalMetric::TripleCount,
                SignalMetric::LowConfidenceCount,
                SignalMetric::CircuitBreakerState,
                SignalMetric::InferenceAvailable,
                SignalMetric::InferenceModelAvailable,
                SignalMetric::AlgedonicEvents,
                SignalMetric::AlgedonicLogApproachingCap,
                SignalMetric::PendingEscalations,
                SignalMetric::ConsolidationCandidates,
                SignalMetric::GoalStaleCount,
                SignalMetric::GoalExpiredCount,
                SignalMetric::MetacognitionVarietyDeficit,
                SignalMetric::MetacognitionCriticalAlerts,
                SignalMetric::WalletBalanceRatio,
                SignalMetric::WalletKeyHealth,
                SignalMetric::SeamCoverage,
                SignalMetric::ActionIneffective,
                SignalMetric::RegulatoryPlateau,
                SignalMetric::ActionDecisionBlocked,
                SignalMetric::ToolReliability,
                SignalMetric::TestCoverage,
                SignalMetric::MutationScore,
            ];
            let unsensed: Vec<&str> = ALL_METRICS
                .iter()
                .filter(|m| !SENSED.contains(m))
                .map(|m| m.as_str())
                .collect();
            if !unsensed.is_empty() {
                tracing::warn!(
                    target: "reg.cybernetics",
                    unsensed_count = unsensed.len(),
                    unsensed = ?unsensed,
                    "Metrics with policy rules but no sensor — these rules can never fire in production (Ashby's Law variety deficit on the sensing side)"
                );
            }
        }

        Self {
            ledger,
            call_cap_manager,
            set_points,
            max_iterations,
            dampener,
            event_sink: None,
            alert_escalation_sink: None,
            alerts_tx: None,
            alert_email_sink: None,
            curator_directive_rx: None,
            submitted_rollout_checks: tokio::sync::Mutex::new(Vec::new()),
            loop_quality: RwLock::new(LoopMetrics::default()),
            budget_persistence_path: None,
            stagnation_detector,
            sensor_registry,

            strategy_evaluator: Mutex::new(StrategyEvaluator::new()),
            simulator: MovingAverageExtrapolator::new(10),
            calibrated_thresholds,
            rollout_events: None,
        }
    }

    /// Algedonic alerts and directive acknowledgments persisted to RegulationArchive.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Wire the rollout event source (event-substrate phase 6). When wired,
    /// `verify_impact` queries it for before/after metric values before
    /// falling back to the in-memory re-sense path.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_rollout_event_source(mut self, source: Arc<dyn RolloutEventSource>) -> Self {
        self.rollout_events = Some(source);
        self
    }

    /// Set or clear the alert escalation sink after construction.
    ///
    /// Used by the composition root to lazily wire the escalation queue after
    /// the curator DB passphrase resolves (post-login deferred task), mirroring
    /// `set_event_sink`. Pass `None` to disable escalation-queue persistence.
    pub fn set_alert_escalation_sink(
        &mut self,
        sink: Option<Arc<dyn crate::algedonic::AlertEscalationSink>>,
    ) {
        self.alert_escalation_sink = sink;
    }

    /// Wire the direct alerts channel for Cybernetics → Curation CurationInput delivery.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_alerts_channel(mut self, tx: mpsc::UnboundedSender<CurationInput>) -> Self {
        self.alerts_tx = Some(tx);
        self
    }

    /// Wire the last-resort alert email sink — sends algedonic alerts via email
    /// when the live channel and persistence are both unavailable.
    ///
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_alert_email_sink(
        mut self,
        sink: Arc<dyn crate::algedonic::AlertEmailSink>,
    ) -> Self {
        self.alert_email_sink = Some(sink);
        self
    }

    /// Set or clear the alert email sink after construction.
    ///
    /// Used by the composition root to lazily wire the email sink after
    /// settings load (the env vars `HKASK_SMTP_USERNAME` etc. are populated
    /// from `KaskSettings::mcp_env()` in the deferred task, not at startup).
    /// Pass `None` to disable email alerts (the zero-config default).
    pub fn set_alert_email_sink(
        &mut self,
        sink: Option<Arc<dyn crate::algedonic::AlertEmailSink>>,
    ) {
        self.alert_email_sink = sink;
    }

    /// Replace the regulation event sink after construction.
    ///
    /// Used by the composition root to upgrade from `NoopEventSink` to a
    /// durable `RegulationArchive` once the curator DB passphrase resolves
    /// (post-login deferred task).
    pub fn set_event_sink(&mut self, sink: Arc<dyn RegulationSink>) {
        self.event_sink = Some(sink);
    }

    /// Wire the direct curator directive channel: Curation → Cybernetics.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_curator_directive_channel(
        mut self,
        rx: mpsc::UnboundedReceiver<CuratorDirective>,
    ) -> Self {
        self.curator_directive_rx = Some(Arc::new(RwLock::new(rx)));
        self
    }

    /// Wire an inference health source so the cybernetics loop can sense
    /// inference saturation and timeout storms.
    ///
    /// Without this, the loop reports `signal_count=0` during an inference
    /// timeout storm because its existing sensors read ledger/DB state, not
    /// inference dispatch state. The `InferenceHealthSensor` emits
    /// `SignalMetric::InferenceAvailable` when the inference layer is
    /// saturated or storming, closing the blind-feedback-loop gap.
    ///
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_inference_health_source(
        self,
        source: Arc<dyn crate::sensor_provider::InferenceHealthSource>,
    ) -> Self {
        self.sensor_registry.register(Arc::new(
            crate::sensor_provider::InferenceHealthSensor::new(source, 3),
        ));
        self
    }

    /// Wire an inference health source after construction.
    ///
    /// Used by the composition root to lazily wire the sensor after the
    /// `LanguageModelInferencePort` is created (in the deferred post-login
    /// task). The `with_inference_health_source` builder method can't be used
    /// there because the loop is already wrapped in `Arc<RwLock<...>>` by the
    /// time the port exists.
    pub fn set_inference_health_source(
        &mut self,
        source: Arc<dyn crate::sensor_provider::InferenceHealthSource>,
    ) {
        self.sensor_registry.register(Arc::new(
            crate::sensor_provider::InferenceHealthSensor::new(source, 3),
        ));
    }

    /// Wire a memory health source at construction time.
    ///
    /// The bridge implements `MemoryHealthSource` and passes an
    /// `Arc<dyn MemoryHealthSource>` here. The `MemoryHealthSensor` emits
    /// signals for `TripleCount`, `LowConfidenceCount`,
    /// `ConsolidationCandidates`, `StorageUsage`, and `MemoryLife` —
    /// closing 5 regulation loops that previously had policy rules but no
    /// sensor (dead policy).
    ///
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_memory_health_source(
        self,
        source: Arc<dyn crate::sensor_provider::MemoryHealthSource>,
        set_points: &crate::set_points::SetPoints,
    ) -> Self {
        self.sensor_registry
            .register(Arc::new(crate::sensor_provider::MemoryHealthSensor::new(
                source,
                set_points.triple_count_max,
                set_points.low_confidence_max,
                set_points.low_confidence_threshold,
                set_points.consolidation_floor,
                set_points.consolidation_candidates_max,
                set_points.storage_usage_max_ratio,
                set_points.memory_life_min_days,
            )));
        self
    }

    /// Wire a memory health source after construction.
    ///
    /// Used by the composition root to lazily wire the sensor after the
    /// memory store is opened (in the deferred post-login task).
    pub fn set_memory_health_source(
        &mut self,
        source: Arc<dyn crate::sensor_provider::MemoryHealthSource>,
    ) {
        self.sensor_registry
            .register(Arc::new(crate::sensor_provider::MemoryHealthSensor::new(
                source,
                self.set_points.triple_count_max,
                self.set_points.low_confidence_max,
                self.set_points.low_confidence_threshold,
                self.set_points.consolidation_floor,
                self.set_points.consolidation_candidates_max,
                self.set_points.storage_usage_max_ratio,
                self.set_points.memory_life_min_days,
            )));
    }

    /// Submit a rollout impact check for the next `verify_impact` pass.
    ///
    /// This is the producer side of the event-substrate phase 6 seam: a
    /// caller that observed a metric-relevant event on a rollout (e.g. the
    /// harness observing a pass-rate regression after a card change) asks
    /// the loop to verify the before/after movement from the rollout event
    /// store. The check is queued and answered on the next tick — the
    /// submitter never blocks on the answer.
    ///
    /// expect: "The system closes the cybernetic feedback loop by measuring action impact"
    /// post: the check is queued for the next tick's verify_impact
    pub async fn submit_rollout_impact_check(
        &self,
        rollout_id: String,
        before_position: i64,
        metric: String,
    ) {
        let action = RegulatoryAction::new(
            LoopId::Curation,
            ActionType::Notify,
            RegulatoryActionParams::with_data(
                "rollout_impact_check",
                RegulationData::RolloutImpactCheck {
                    rollout_id,
                    before_position,
                    metric,
                },
            ),
        );
        let mut queue = self.submitted_rollout_checks.lock().await;
        // Bound the queue: a producer runaway must not grow it unboundedly.
        // Dropping the OLDEST check is correct — the newest observations are
        // the most relevant, and the drop is visible (the queue is drained
        // and reported per tick).
        const MAX_SUBMITTED_CHECKS: usize = 64;
        if queue.len() >= MAX_SUBMITTED_CHECKS {
            queue.remove(0);
        }
        queue.push(action);
    }

    /// Record a tool outcome in the Regulation runtime for outcome quality tracking.
    ///
    /// Delegates to `RegulationLedger::record_outcome`. Called by `McpRuntime`
    /// after every governed tool invocation completes.
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    pub async fn record_outcome(&self, domain: &str, success: bool, error_kind: Option<&str>) {
        self.ledger
            .read()
            .await
            .record_outcome(domain, success, error_kind)
            .await;
    }

    /// Register a per-agent call cap (the hard ceiling on governed tool calls per
    /// regulation tick). The composition root must seed a cap for every agent
    /// that makes governed tool calls — agents without one are denied (fail-closed).
    ///
    /// expect: "The system enforces energy homeostasis through energy budget membrane regulation"
    pub async fn register_call_cap(&self, agent: WebID, ceiling: u32) {
        self.call_cap_manager
            .read()
            .await
            .register_call_cap(agent, ceiling)
            .await;
    }

    /// Check whether an agent still has calls available this tick.
    ///
    /// expect: "The system enforces energy homeostasis through energy budget membrane regulation"
    pub async fn can_proceed(&self, agent: &WebID) -> bool {
        self.call_cap_manager.read().await.can_proceed(agent).await
    }

    /// Meter one governed tool call, auto-registering an unknown agent at the
    /// default runaway ceiling. The tool-dispatch path uses this rather than
    /// [`Self::charge_call`] — see [`CallCapManager::charge_metered`].
    ///
    /// expect: "The system enforces energy homeostasis through energy budget membrane regulation"
    pub async fn charge_call_metered(&self, agent: &WebID) -> CallMeterOutcome {
        self.call_cap_manager
            .read()
            .await
            .charge_metered(agent)
            .await
    }

    /// Reset every registered cap to its ceiling (one regulation tick).
    ///
    /// expect: "The system enforces energy homeostasis through energy budget membrane regulation"
    pub async fn reset_all_caps(&self) {
        self.call_cap_manager.read().await.reset_all().await;
    }

    /// Called during sense() so directives are applied before computing actions.
    ///
    /// expect: "The system enforces homeostatic self-regulation through the five-phase cybernetic cycle"
    /// pre: called before each regulation tick to drain pending directives
    pub async fn process_inbox(&self) {
        // Drain direct curator directive channel.
        if let Some(ref rx) = self.curator_directive_rx {
            let mut cd_rx = rx.write().await;
            let mut cd_processed = 0;
            while let Ok(directive) = cd_rx.try_recv() {
                cd_processed += 1;
                self.handle_curation_directive(directive).await;
            }
            if cd_processed > 0 {
                tracing::info!(target: "reg.cybernetics", processed = cd_processed, "Processed direct curator directives");
            }
        }
        // Curation overrides persist until explicitly cleared via
        // `CuratorDirective::ClearOverride` — there is no TTL auto-expiry.
    }
}

impl CyberneticsLoop {
    /// Full regulation cycle with loop-quality telemetry.
    ///
    /// Measures elapsed time and computes `LoopMetrics` metrics (delay_ms,
    /// gain, fidelity_score, effectiveness_score) after each cycle. Calls
    /// `verify_impact` to close the feedback loop.
    pub async fn tick(&self) {
        let start = std::time::Instant::now();

        let signals = self.sense().await;
        // Emit a runtime-posture signal span so the runtime-posture-monitor
        // skill (and any downstream observer) has a production telemetry
        // substrate even when the skill cascade is not explicitly invoked.
        // The namespace `reg.runtime.select` is registered in
        // CANONICAL_NAMESPACES; without this emitter it would be skill-only.
        tracing::info!(
            target: "reg.runtime.select",
            signal_count = signals.len(),
            "REG"
        );
        let deviations = self.compare(&signals).await;
        let mut actions = self.compute(&deviations).await;
        // Drain externally-submitted rollout impact checks into this tick's
        // verification pass — the producer side of the phase 6 seam.
        let submitted: Vec<RegulatoryAction> =
            std::mem::take(&mut *self.submitted_rollout_checks.lock().await);
        if !submitted.is_empty() {
            tracing::debug!(
                target: "reg.cybernetics",
                count = submitted.len(),
                "drained submitted rollout impact checks into verify_impact"
            );
            actions.extend(submitted);
        }
        self.act(&actions).await;

        // Fermi impact-gate: verify whether actions improved their targets.
        let impact_reports = self.verify_impact(&actions).await;

        // Check regulation coherence.
        self.check_coherence(&actions);

        // Feed per-metric outcomes into strategy evaluator.
        // Collect promoted metrics in a locked scope; emit spans outside
        // to avoid holding MutexGuard across .await (not Send).
        let promoted_metrics = {
            let mut evaluator = self
                .strategy_evaluator
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let mut seen = std::collections::HashSet::new();
            let mut promoted = Vec::new();
            for report in &impact_reports {
                if seen.insert(report.metric) {
                    let metric_reports: Vec<_> = impact_reports
                        .iter()
                        .filter(|r| r.metric == report.metric)
                        .collect();
                    let accepted = metric_reports
                        .iter()
                        .filter(|r| r.decision == ActionDecision::Accept)
                        .count() as u64;
                    let staged = metric_reports
                        .iter()
                        .filter(|r| r.decision == ActionDecision::Stage)
                        .count() as u64;
                    let blocked = metric_reports
                        .iter()
                        .filter(|r| r.decision == ActionDecision::Block)
                        .count() as u64;
                    evaluator.record_cycle(report.metric, accepted, staged, blocked);
                    // Check for strategy promotion; emit Regulation span if promoted.
                    if evaluator.active_policy(report.metric) {
                        promoted.push(report.metric);
                    }
                }
            }
            promoted
        };
        for metric in promoted_metrics {
            self.emit_regulation_span(
                SpanKind::ActionSubstituted,
                serde_json::json!({
                    "event": "strategy_promoted",
                    "metric": metric.as_str(),
                }),
            )
            .await;
        }

        // Feed regulation health into Regulation for metacognition observability.
        {
            let accepted = impact_reports
                .iter()
                .filter(|r| r.decision == ActionDecision::Accept)
                .count() as u64;
            let staged = impact_reports
                .iter()
                .filter(|r| r.decision == ActionDecision::Stage)
                .count() as u64;
            let blocked = impact_reports
                .iter()
                .filter(|r| r.decision == ActionDecision::Block)
                .count() as u64;
            let ledger = self.ledger.read().await;
            let cumulative = ledger.regulation_health().await.effectiveness();
            ledger
                .record_regulation_cycle(RegulationCycleEntry {
                    timestamp: chrono::Utc::now(),
                    signals: signals.len() as u64,
                    deviations: deviations.len() as u64,
                    actions: actions.len() as u64,
                    verified: impact_reports.len() as u64,
                    accepted,
                    staged,
                    blocked,
                    cumulative_effectiveness: cumulative,
                })
                .await;
        }

        let elapsed_ms = start.elapsed().as_millis() as u64;

        let quality = LoopMetrics::from_cycle(
            elapsed_ms,
            &deviations,
            &actions,
            &impact_reports,
            TriggerOrigin::Scheduled,
        );
        *self.loop_quality.write().await = quality;

        tracing::debug!(
            target: "reg.cybernetics",
            delay_ms = quality.delay_ms,
            gain = quality.gain,
            fidelity = quality.fidelity_score,
            effectiveness = quality.effectiveness_score,
            deviations = deviations.len(),
            actions = actions.len(),
            impact_reports = impact_reports.len(),
            "Loop-quality telemetry recorded"
        );

        self.emit_regulation_span(
            SpanKind::LoopMetricsTelemetry,
            serde_json::json!({
                "delay_ms": quality.delay_ms,
                "gain": quality.gain,
                "fidelity_score": quality.fidelity_score,
                "effectiveness_score": quality.effectiveness_score,
                "trigger": format!("{:?}", quality.trigger),
                "deviations": deviations.len(),
                "actions": actions.len(),
                "impact_reports": impact_reports.len(),
            }),
        )
        .await;
    }
}

impl CyberneticsLoop {
    /// Return a snapshot of the most recent loop-quality telemetry.
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    pub async fn loop_quality(&self) -> LoopMetrics {
        *self.loop_quality.read().await
    }

    /// Return a reference to the current set-points (read-only).
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    pub fn set_points(&self) -> &SetPoints {
        &self.set_points
    }
}
