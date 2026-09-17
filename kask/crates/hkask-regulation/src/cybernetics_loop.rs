//! Cybernetics Loop — Homeostatic self-regulation (Loop 6)
//!
//! The Cybernetics Loop is a closed-loop controller, not a passive observer.
//! Its functional contract:
//!
//! 1. **Sense** — read registered sensors plus atomic inference-resilience
//!    snapshots and receipts.
//! 2. **Compare** — evaluate each signal against its homeostatic set-point.
//! 3. **Compute** — select a truthful `Notify` or `Escalate` disposition.
//! 4. **Act** — record observations or route evidence-bearing escalations.
//!    Fast inference circuit intervention is a nested loop at the dispatch
//!    boundary; its receipts return here for assessment and escalation.
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
use crate::sensor_provider::{SensorBus, VarietySensor};

use crate::runtime::RegulationLedger;
use crate::set_points::SetPoints;
use crate::strategy_evaluator::StrategyEvaluator;
use crate::system_simulator::MovingAverageExtrapolator;

use crate::loops::{ActionDecision, CurationInput, LoopMetrics, TriggerOrigin};

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
}

const MAX_SUBMITTED_ROLLOUT_CHECKS: usize = 64;

/// Admission result for a rollout impact check.
///
/// Acceptance means only that the check is retained for the next assessment
/// pass. It does not imply that assessment ran or that a verdict was written.
#[must_use = "the producer must retain its source event until submission is accepted"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolloutImpactSubmission {
    Accepted,
    QueueFull { capacity: usize },
}

#[derive(Clone)]
struct RolloutImpactCheck {
    rollout_id: String,
    before_position: i64,
    metric: String,
}

#[derive(Default)]
struct LoopTelemetryState {
    steady_fingerprint: Option<serde_json::Value>,
    intervention_observation: Option<Option<usize>>,
    suppressed_cycles: usize,
}

struct LoopTelemetryDecision {
    emit: bool,
    idle_heartbeat: bool,
    steady_state_heartbeat: bool,
    condition_cleared: bool,
    suppressed_cycles: usize,
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
    /// Whether a model-bearing inference health source has been wired. The
    /// composition root wires the source only after the default
    /// `LanguageModel` resolves; before that (not logged in, registry still
    /// loading) inference is unusable — the state `NoModelInferencePort`
    /// exists for. Sensed as `SignalMetric::InferenceModelAvailable`.
    inference_health_wired: bool,
    /// Atomic resilience observation source supplied by the inference dispatch boundary.
    inference_resilience_source: Option<Arc<dyn crate::InferenceResilienceSource>>,
    /// Last intervention receipt consumed from the resilience source.
    inference_intervention_cursor: std::sync::atomic::AtomicU64,
    /// Ticks since construction. The first ticks after boot often precede
    /// the deferred task's model wiring (observed live: a boot wired the
    /// no-op port at +4s while the model resolved later); sensing
    /// model-unavailability on those ticks would report a false outage on
    /// every slow boot. The grace constant lives at the sense site.
    tick_count: std::sync::atomic::AtomicUsize,
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
    submitted_rollout_checks: tokio::sync::Mutex<Vec<RolloutImpactCheck>>,
    /// Loop-quality telemetry from the most recent tick cycle.
    loop_quality: RwLock<LoopMetrics>,
    /// Coalesces only semantically identical persistent signal telemetry.
    loop_telemetry_state: Mutex<LoopTelemetryState>,
    /// Detects regulatory plateaus — repeated ineffective (metric, action) pairs.
    /// Fermi-inspired early-stopping pattern for cybernetic regulation.
    stagnation_detector: Arc<StagnationDetector>,
    /// Pluggable metric sensors (Fermi Extractor pattern).
    sensor_registry: Arc<SensorBus>,
    observations: parking_lot::Mutex<HashMap<crate::loops::SignalMetric, crate::loops::Signal>>,
    /// Statistical learner for per-tool cost distributions and reliability.
    /// Multi-model strategy evaluator (Fermi improvement-loop pattern).
    strategy_evaluator: Mutex<StrategyEvaluator>,
    /// Predictive simulator for anticipatory regulation (Fermi dynamics pattern).
    simulator: MovingAverageExtrapolator,
    /// Runtime-calibratable thresholds — updated by `SetPointCalibrator` background task.
    calibrated_thresholds: Arc<RwLock<CalibratedThresholds>>,
    /// Optional rollout event source (event-substrate phase 6). When wired,
    /// `verify_impact` queries it for before/after metric values named by a
    /// typed `RolloutImpactCheck`; there is no advisory re-sense fallback.
    rollout_events: Option<Arc<dyn RolloutEventSource>>,
    /// Optional context-server health source retained so advisory construction
    /// can carry the current fleet counts as quantitative trigger evidence.
    /// The sensor registry holds a `ContextServerHealthSensor` wrapping the
    /// same source for the sense phase.
    context_server_health_source:
        Option<Arc<dyn crate::sensor_provider::ContextServerHealthSource>>,
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
        }));
        let sensor_registry = {
            let registry = SensorBus::new();

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

        Self {
            ledger,
            call_cap_manager,
            set_points,
            max_iterations,
            inference_health_wired: false,
            inference_resilience_source: None,
            inference_intervention_cursor: std::sync::atomic::AtomicU64::new(0),
            tick_count: std::sync::atomic::AtomicUsize::new(0),
            dampener,
            event_sink: None,
            alert_escalation_sink: None,
            alerts_tx: None,
            alert_email_sink: None,
            curator_directive_rx: None,
            submitted_rollout_checks: tokio::sync::Mutex::new(Vec::new()),
            loop_quality: RwLock::new(LoopMetrics::default()),
            loop_telemetry_state: Mutex::new(LoopTelemetryState::default()),
            stagnation_detector,
            sensor_registry,
            observations: parking_lot::Mutex::new(HashMap::new()),

            strategy_evaluator: Mutex::new(StrategyEvaluator::new()),
            simulator: MovingAverageExtrapolator::new(10),
            calibrated_thresholds,
            rollout_events: None,
            context_server_health_source: None,
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
    /// `verify_impact` queries it for the before/after values named by typed
    /// rollout checks.
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
    /// the curator DB passphrase resolves (deferred task), mirroring
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
    /// (deferred task).
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

    /// Wire the atomic inference resilience source at construction.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_inference_resilience_source(
        mut self,
        source: Arc<dyn crate::InferenceResilienceSource>,
    ) -> Self {
        self.set_inference_resilience_source(source);
        self
    }

    /// Replace the inference resilience source after model wiring or rewiring.
    pub fn set_inference_resilience_source(
        &mut self,
        source: Arc<dyn crate::InferenceResilienceSource>,
    ) {
        self.inference_health_wired = true;
        self.inference_resilience_source = Some(source);
        self.inference_intervention_cursor
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Wire a context-server health source so the cybernetics loop can sense
    /// MCP servers stuck in `Starting` or `Error`.
    ///
    /// Without this, the loop reports `signal_count=0` while every context
    /// server is hung on `initialize` (the 600s timeout storm) because its
    /// existing sensors read ledger/DB state, not context-server process
    /// state. The `ContextServerHealthSensor` emits
    /// `SignalMetric::ContextServerHealth` when the fleet is degraded,
    /// closing the blind-feedback-loop gap.
    ///
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_context_server_health_source(
        mut self,
        source: Arc<dyn crate::sensor_provider::ContextServerHealthSource>,
    ) -> Self {
        self.sensor_registry.replace(
            crate::loops::SignalMetric::ContextServerHealth,
            Arc::new(crate::sensor_provider::ContextServerHealthSensor::new(
                Arc::clone(&source),
            )),
        );
        self.context_server_health_source = Some(source);
        self
    }

    /// Wire an OCR health source so the cybernetics loop can sense OCR
    /// silent-failure storms in the corpus MCP server.
    ///
    /// Without this, the loop reports `signal_count=0` during an OCR
    /// silent-failure storm (a dead-but-responsive OCR endpoint returning
    /// HTTP 200 with empty content on every Complex page) because the
    /// `reg.pipeline.ocr.silent_failure` warns live in the corpus
    /// subprocess's tracing — the loop's existing sensors read ledger/DB
    /// state in the zed main process. The `OcrHealthSensor` emits
    /// `SignalMetric::OcrSilentFailures` from the cross-process health
    /// file, closing the blind-feedback-loop gap.
    ///
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_ocr_health_source(
        self,
        source: Arc<dyn crate::sensor_provider::OcrHealthSource>,
    ) -> Self {
        self.sensor_registry
            .register(Arc::new(crate::sensor_provider::OcrHealthSensor::new(
                source,
            )));
        self
    }

    /// Wire a context-server health source after construction.
    ///
    /// Used by the composition root to lazily wire the sensor after the
    /// per-project `ContextServerStore` is available. The `with_*` builder
    /// method can't be used there because the loop is already wrapped in
    /// `Arc<RwLock<...>>` by the time the store exists.
    pub fn set_context_server_health_source(
        &mut self,
        source: Arc<dyn crate::sensor_provider::ContextServerHealthSource>,
    ) {
        self.sensor_registry.replace(
            crate::loops::SignalMetric::ContextServerHealth,
            Arc::new(crate::sensor_provider::ContextServerHealthSensor::new(
                Arc::clone(&source),
            )),
        );
        self.context_server_health_source = Some(source);
    }

    /// Wire a memory health source after construction.
    ///
    /// Used by the composition root to lazily wire the sensor after the
    /// memory store is opened (in the deferred task).
    pub fn set_memory_health_source(
        &mut self,
        source: Arc<dyn crate::sensor_provider::MemoryHealthSource>,
    ) {
        use crate::loops::SignalMetric;
        for metric in [
            SignalMetric::MemoryLife,
            SignalMetric::TripleCount,
            SignalMetric::LowConfidenceCount,
            SignalMetric::ConsolidationCandidates,
        ] {
            self.sensor_registry.replace(
                metric,
                Arc::new(crate::sensor_provider::MemoryHealthSensor::new(
                    source.clone(),
                    metric,
                    &self.set_points,
                )),
            );
        }
    }

    /// Submit a rollout impact check for the next `verify_impact` pass.
    ///
    /// This is the producer side of the event-substrate phase 6 seam: a
    /// caller that observed a metric-relevant event on a rollout (e.g. the
    /// harness observing a pass-rate regression after a card change) asks
    /// the loop to verify the before/after movement from the rollout event
    /// store. An accepted check is queued and answered on the next tick — the
    /// submitter never blocks on assessment. A full queue returns typed
    /// backpressure without replacing an earlier accepted check.
    ///
    /// expect: "The system closes the cybernetic feedback loop by measuring action impact"
    /// post: returns whether the check was retained for the next tick's verify_impact
    pub async fn submit_rollout_impact_check(
        &self,
        rollout_id: String,
        before_position: i64,
        metric: String,
    ) -> RolloutImpactSubmission {
        let check = RolloutImpactCheck {
            rollout_id,
            before_position,
            metric,
        };
        let mut queue = self.submitted_rollout_checks.lock().await;
        // Bound the queue without invalidating an earlier acceptance. When the
        // queue is full, the producer retains its event cursor and retries
        // after the next tick drains accepted checks.
        if queue.len() >= MAX_SUBMITTED_ROLLOUT_CHECKS {
            return RolloutImpactSubmission::QueueFull {
                capacity: MAX_SUBMITTED_ROLLOUT_CHECKS,
            };
        }
        queue.push(check);
        RolloutImpactSubmission::Accepted
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

    /// Record a behavioral variety observation — the dispatch twin of
    /// `record_outcome`. One call per governed tool invocation, with the tool
    /// name as the observed state: distinct tools exercised per domain per
    /// 60s window is the system's behavioral repertoire in use, and a
    /// persistent gap against the expected variety is rut behavior (the
    /// domain-level aggregate of the per-tool retry death spiral — Ashby's
    /// requisite-variety signal).
    ///
    /// Delegates to `RegulationLedger::increment_variety`, which also runs
    /// the algedonic check for the domain. Called by `McpRuntime::invoke`
    /// beside `record_outcome` so reliability and variety share one domain
    /// registry (the MCP server name).
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    pub async fn record_variety(&self, domain: &str, state_name: &str) {
        self.ledger
            .read()
            .await
            .increment_variety(domain, state_name)
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
    fn loop_telemetry_decision(
        &self,
        deviations: &[crate::loops::Deviation],
        actions: &[crate::loops::RegulatoryAction],
        interventions_confirmed: Option<usize>,
        force_transition: bool,
        tick_number: usize,
    ) -> LoopTelemetryDecision {
        const HEARTBEAT_INTERVAL_TICKS: usize = 360; // 10s scheduled cadence → hourly
        let fingerprint = (!deviations.is_empty() || !actions.is_empty()).then(|| {
            let deviations = deviations
                .iter()
                .map(|deviation| {
                    serde_json::json!({
                        "metric": deviation.signal.metric,
                        "value_bits": deviation.signal.value.to_bits(),
                        "set_point_bits": deviation.signal.set_point.to_bits(),
                        "magnitude_bits": deviation.magnitude.to_bits(),
                        "direction": deviation.direction,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({"deviations": deviations, "advisories": actions})
        });
        let mut state = self
            .loop_telemetry_state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let intervention_changed = state
            .intervention_observation
            .is_some_and(|previous| previous != interventions_confirmed);
        state.intervention_observation = Some(interventions_confirmed);
        let force_transition = force_transition || intervention_changed;

        match fingerprint {
            Some(fingerprint) if state.steady_fingerprint.as_ref() != Some(&fingerprint) => {
                let suppressed_cycles = std::mem::take(&mut state.suppressed_cycles);
                state.steady_fingerprint = Some(fingerprint);
                LoopTelemetryDecision {
                    emit: true,
                    idle_heartbeat: false,
                    steady_state_heartbeat: false,
                    condition_cleared: false,
                    suppressed_cycles,
                }
            }
            Some(_) if force_transition => LoopTelemetryDecision {
                emit: true,
                idle_heartbeat: false,
                steady_state_heartbeat: false,
                condition_cleared: false,
                suppressed_cycles: std::mem::take(&mut state.suppressed_cycles),
            },
            Some(_) => {
                state.suppressed_cycles = state.suppressed_cycles.saturating_add(1);
                if tick_number.is_multiple_of(HEARTBEAT_INTERVAL_TICKS) {
                    LoopTelemetryDecision {
                        emit: true,
                        idle_heartbeat: false,
                        steady_state_heartbeat: true,
                        condition_cleared: false,
                        suppressed_cycles: std::mem::take(&mut state.suppressed_cycles),
                    }
                } else {
                    LoopTelemetryDecision {
                        emit: false,
                        idle_heartbeat: false,
                        steady_state_heartbeat: false,
                        condition_cleared: false,
                        suppressed_cycles: 0,
                    }
                }
            }
            None => {
                let condition_cleared = state.steady_fingerprint.take().is_some();
                let suppressed_cycles = std::mem::take(&mut state.suppressed_cycles);
                let idle_heartbeat = !condition_cleared
                    && !force_transition
                    && (tick_number == 1 || tick_number.is_multiple_of(HEARTBEAT_INTERVAL_TICKS));
                LoopTelemetryDecision {
                    emit: condition_cleared || force_transition || idle_heartbeat,
                    idle_heartbeat,
                    steady_state_heartbeat: false,
                    condition_cleared,
                    suppressed_cycles,
                }
            }
        }
    }

    /// Full regulation cycle with loop-quality telemetry.
    ///
    /// Measures elapsed time and computes separate rollout-impact and
    /// observational advice-review progress after each cycle.
    /// Computed advisories are routed for operator action; only externally
    /// submitted checks with before/after evidence enter `verify_impact`.
    pub async fn tick(&self) {
        let start = std::time::Instant::now();
        self.tick_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let signals = self.sense().await;
        *self.observations.lock() = signals
            .iter()
            .map(|signal| (signal.metric, signal.clone()))
            .collect();
        let (advice_reconciliation, advice_observation_available) = if let Some(sink) =
            &self.alert_escalation_sink
        {
            match sink.reconcile_conditions(&signals) {
                Ok(reconciliation) => (reconciliation, true),
                Err(error) => {
                    tracing::warn!(target: "reg.alert", %error, "Advice-review reconciliation unavailable; receipts retained");
                    (crate::AdviceReviewReconciliation::default(), false)
                }
            }
        } else {
            (crate::AdviceReviewReconciliation::default(), false)
        };
        let deviations = self.compare(&signals).await;
        let actions = self.compute(&deviations).await;
        // Drain externally submitted checks separately from computed advice.
        // Computed actions are routed to the operator and have no causal
        // impact to verify until an intervention is confirmed. Rollout checks
        // already carry an evidence-bearing before/after query contract.
        let impact_checks = std::mem::take(&mut *self.submitted_rollout_checks.lock().await);
        if !impact_checks.is_empty() {
            tracing::debug!(
                target: "reg.cybernetics",
                count = impact_checks.len(),
                "drained submitted rollout impact checks into verify_impact"
            );
        }
        self.act(&actions).await;

        // Fermi impact-gate: verify only evidence-bearing submitted checks.
        let impact_reports = self.verify_impact(&impact_checks).await;

        // Publish finalized observational reviews separately from rollout
        // impact. The queue-assigned event id makes archive insertion
        // idempotent; acknowledgment follows durable insertion or confirmation
        // that the same event already exists.
        let mut published_advice_reviews = Vec::new();
        if let Some(sink) = &self.alert_escalation_sink {
            for receipt in &advice_reconciliation.pending_receipts {
                match self.persist_advice_review_receipt(receipt).await {
                    Ok(Some(inserted)) => {
                        match sink.acknowledge_advice_review(receipt) {
                            Ok(true) => {}
                            Ok(false) => tracing::debug!(
                                target: "reg.alert",
                                receipt_id = %receipt.event_id,
                                "Advice-review publication acknowledgment conflicted; retrying idempotently"
                            ),
                            Err(error) => tracing::warn!(
                                target: "reg.alert",
                                %error,
                                receipt_id = %receipt.event_id,
                                "Advice-review publication acknowledgment failed; retrying idempotently"
                            ),
                        }
                        if inserted {
                            published_advice_reviews.push(receipt.clone());
                        }
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!(
                        target: "reg.outcome",
                        %error,
                        receipt_id = %receipt.event_id,
                        "Advice-review receipt publication failed; durable receipt retained"
                    ),
                }
            }
        }

        // Feed per-metric rollout outcomes into strategy evaluator. Advice
        // reviews remain observational and never enter this causal-impact path.
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
            ledger.record_cycle_outcome(accepted, staged, blocked).await;
        }

        let elapsed_ms = start.elapsed().as_millis() as u64;

        let quality = LoopMetrics::from_cycle(
            elapsed_ms,
            &deviations,
            &actions,
            &impact_reports,
            &published_advice_reviews,
            TriggerOrigin::Scheduled,
        );
        *self.loop_quality.write().await = quality;

        tracing::debug!(
            target: "reg.cybernetics",
            delay_ms = quality.delay_ms,
            response_coverage = quality.response_coverage,
            fidelity = quality.fidelity_score,
            rollout_progress = ?quality.rollout_progress_score,
            advice_review_progress = ?quality.advice_review.progress_score,
            deviations = deviations.len(),
            advisories_computed = actions.len(),
            rollout_impact_reports = impact_reports.len(),
            advice_reviews_finalized = quality.advice_review.finalized,
            "Loop-quality telemetry recorded"
        );

        // Coalesce only semantically identical persistent deviation/advisory
        // cycles. Changed values, clearing, rollout measurements, and newly
        // published advice reviews always emit. Exact repeats accumulate until
        // the existing hourly tick boundary re-announces liveness and reports
        // how many records were suppressed.
        let tick_number = self.tick_count.load(std::sync::atomic::Ordering::Relaxed);
        let interventions_confirmed =
            advice_observation_available.then_some(advice_reconciliation.interventions_confirmed);
        let decision = self.loop_telemetry_decision(
            &deviations,
            &actions,
            interventions_confirmed,
            !impact_reports.is_empty() || !published_advice_reviews.is_empty(),
            tick_number,
        );
        if decision.emit {
            let mut observation = serde_json::json!({
                "delay_ms": quality.delay_ms,
                "response_coverage": quality.response_coverage,
                "fidelity_score": quality.fidelity_score,
                "rollout_progress_score": quality.rollout_progress_score,
                "advice_review_progress_score": quality.advice_review.progress_score,
                "trigger": format!("{:?}", quality.trigger),
                "deviations": deviations.len(),
                "advisories_computed": actions.len(),
                "interventions_confirmed": interventions_confirmed,
                "rollout_impact_reports": impact_reports.len(),
                "advice_reviews_finalized": quality.advice_review.finalized,
                "advice_reviews_recovered": quality.advice_review.recovered,
                "advice_reviews_improved": quality.advice_review.improved,
                "advice_reviews_no_improvement": quality.advice_review.no_improvement,
                "advice_reviews_insufficient_evidence": quality.advice_review.insufficient_evidence,
                "advice_review_observation_available": advice_observation_available,
                "advice_review_causal_attribution": (quality.advice_review.finalized > 0).then_some("unverified"),
                "suppressed_steady_state_cycles": decision.suppressed_cycles,
            });
            if decision.idle_heartbeat || decision.steady_state_heartbeat {
                observation["tick_count"] = serde_json::Value::from(tick_number);
                let health = self.ledger.read().await.health().await;
                observation["alert_log_count"] = serde_json::Value::from(health.alert_log_count);
                observation["alert_log_cap"] = serde_json::Value::from(health.alert_log_cap);
                observation["alert_log_approaching_cap"] =
                    serde_json::Value::Bool(health.alert_log_approaching_cap);
            }
            if decision.idle_heartbeat {
                observation["heartbeat"] = serde_json::Value::Bool(true);
            }
            if decision.steady_state_heartbeat {
                observation["steady_state_heartbeat"] = serde_json::Value::Bool(true);
            }
            if decision.condition_cleared {
                observation["condition_cleared"] = serde_json::Value::Bool(true);
            }
            self.emit_regulation_span(SpanKind::LoopMetricsTelemetry, observation)
                .await;
            // The idle heartbeat also carries the per-domain tool-outcome
            // breakdown when any domain has samples. Persistent-condition
            // summaries stay one record so the coalescer cannot amplify itself.
            if decision.idle_heartbeat {
                self.emit_tool_outcome_breakdown().await;
            }
        }
    }
}

#[cfg(test)]
mod submission_tests {
    use super::{CyberneticsLoop, MAX_SUBMITTED_ROLLOUT_CHECKS, RolloutImpactSubmission};
    use crate::runtime::RegulationLedger;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn saturated_rollout_queue_refuses_without_evicting_accepted_checks() {
        let regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));

        for position in 0..MAX_SUBMITTED_ROLLOUT_CHECKS {
            assert_eq!(
                regulation
                    .submit_rollout_impact_check(
                        format!("accepted-{position}"),
                        position as i64,
                        "pass_rate".to_string(),
                    )
                    .await,
                RolloutImpactSubmission::Accepted
            );
        }

        assert_eq!(
            regulation
                .submit_rollout_impact_check(
                    "refused".to_string(),
                    MAX_SUBMITTED_ROLLOUT_CHECKS as i64,
                    "pass_rate".to_string(),
                )
                .await,
            RolloutImpactSubmission::QueueFull {
                capacity: MAX_SUBMITTED_ROLLOUT_CHECKS,
            }
        );

        let queue = regulation.submitted_rollout_checks.lock().await;
        assert_eq!(queue.len(), MAX_SUBMITTED_ROLLOUT_CHECKS);
        assert_eq!(
            queue.first().map(|check| check.rollout_id.as_str()),
            Some("accepted-0")
        );
        assert!(queue.iter().all(|check| check.rollout_id != "refused"));
    }
}
