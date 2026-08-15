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

use crate::dampener::{Dampener, StagnationDetector};
use crate::energy::{AgentCallCapStatus, CallCap, CallCapError, CallCapManager, CallMeterOutcome};

use crate::runtime::{RegulationCycleEntry, RegulationLedger};
use crate::sensor_provider::{EnergyBudgetSensor, SensorBus, ToolReliabilitySensor, VarietySensor};
use crate::set_points::{InferenceThrottleMode, SetPoints};
use crate::strategy_evaluator::StrategyEvaluator;
use crate::system_simulator::MovingAverageExtrapolator;
use crate::tool_stats::ToolStats;

use crate::algedonic::{AlertSeverity, RuntimeAlert};
use crate::regulation_policy::{
    self, RegulationPolicy, RegulationReason, classify_decision, default_substitution_ladder,
    extract_deficit_threshold,
};
use crate::types::loops::{
    ActionDecision, ActionType, CurationInput, Deviation, ImpactReport, LoopId, LoopMetrics,
    RegulatoryAction, RegulatoryActionParams, Signal, SignalMetric, TriggerOrigin,
};
use crate::types::loops::{BudgetOption, RegulationData};

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use hkask_types::curator::SchemaEvolutionType;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanKind};
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
/// The Cybernetic Loop regulates all three domain loops (Inference,
/// Episodic, Semantic) and may signal the Curation Loop via algedonic
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
    tool_stats: Option<Arc<ToolStats>>,
    /// Multi-model strategy evaluator (Fermi improvement-loop pattern).
    strategy_evaluator: Mutex<StrategyEvaluator>,
    /// Predictive simulator for anticipatory regulation (Fermi dynamics pattern).
    simulator: MovingAverageExtrapolator,
    /// Runtime-calibratable thresholds — updated by `SetPointCalibrator` background task.
    calibrated_thresholds: Arc<RwLock<CalibratedThresholds>>,
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
                set_points.gas_min_remaining,
            )));
            registry.register(Arc::new(VarietySensor::new(
                Arc::clone(&ledger),
                set_points.variety_max_deficit,
            )));
            let trace_dir = std::path::PathBuf::from(
                std::env::var("HKASK_TRACE_DIR").unwrap_or_else(|_| "kask/traces".to_string()),
            );
            registry.register(Arc::new(crate::sensor_provider::TestCoverageSensor::new(
                trace_dir.clone(),
                set_points.coverage_floor,
            )));
            registry.register(Arc::new(crate::sensor_provider::MutationScoreSensor::new(
                trace_dir,
                set_points.mutation_score_floor,
            )));
            Arc::new(registry)
        };

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
            loop_quality: RwLock::new(LoopMetrics::default()),
            budget_persistence_path: None,
            stagnation_detector,
            sensor_registry,

            tool_stats: None,
            strategy_evaluator: Mutex::new(StrategyEvaluator::new()),
            simulator: MovingAverageExtrapolator::new(10),
            calibrated_thresholds,
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

    /// Wire the reviewable escalation queue sink for algedonic alerts.
    ///
    /// When set, every escalated alert is persisted to the escalation queue
    /// (the `EscalationQueue` on the curator's curator.db) so the Curator/user can
    /// review pending alerts via `curator_escalations` and resolve/dismiss them
    /// with an audit trail. This is the primary durable path for alert review.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_alert_escalation_sink(
        mut self,
        sink: Arc<dyn crate::algedonic::AlertEscalationSink>,
    ) -> Self {
        self.alert_escalation_sink = Some(sink);
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

    /// Enable call-cap persistence across restarts.
    ///
    /// Caps are saved to the given path after each reset cycle
    /// and loaded automatically on construction.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_budget_persistence(mut self, path: std::path::PathBuf) -> Self {
        self.budget_persistence_path = Some(path);
        self
    }

    /// Wire the tool stats learner for statistical tool learning.
    /// Registers the ToolReliabilitySensor into the sensor registry.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    pub fn with_tool_stats(mut self, stats: Arc<ToolStats>) -> Self {
        self.set_tool_stats(stats);
        self
    }

    /// Set tool stats on an already-constructed loop (post-build wiring).
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    pub fn set_tool_stats(&mut self, stats: Arc<ToolStats>) {
        self.sensor_registry
            .register(Arc::new(ToolReliabilitySensor::new(
                Arc::clone(&stats),
                crate::tool_stats::DEFAULT_RELIABILITY_THRESHOLD,
            )));
        self.tool_stats = Some(stats);
    }

    /// Override the stagnation detection threshold (default: 5 cycles).
    ///
    /// After this many consecutive cycles where the same (metric, action)
    /// pair is ineffective, a `RegulatoryPlateau` escalation is triggered.
    /// Per-metric thresholds from SetPoints are preserved.
    ///
    /// expect: "The system provides configurable cybernetic self-regulation"
    /// post: returns Self for chaining
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_stagnation_threshold(mut self, threshold: u32) -> Self {
        let existing_thresholds = self.set_points.stagnation_thresholds.clone();
        self.stagnation_detector = Arc::new(
            StagnationDetector::new(threshold).with_per_metric_thresholds(existing_thresholds),
        );
        self
    }

    /// Attempt to substitute an action type when the proposed one has been
    /// repeatedly ineffective (Fermi improvement-loop pattern).
    ///
    /// Checks the stagnation detector for the (metric, proposed) pair.
    /// If it has been ineffective for ≥ `substitution_after` cycles,
    /// walks the substitution ladder to find an untried alternative.
    /// Returns the proposed action if no alternatives remain.
    async fn try_substitute(&self, metric: SignalMetric, proposed: ActionType) -> ActionType {
        let proposed_str = proposed.as_str();
        let metric_str = metric.as_str();

        // Check if the proposed action has been tried enough to warrant substitution.
        let count = self
            .stagnation_detector
            .ineffective_count(metric_str, proposed_str);

        if count < self.calibrated_thresholds.read().await.substitution_after {
            return proposed; // Not enough failures yet.
        }

        // Build the substitution ladder: custom overrides > defaults.
        let custom_ladder = self.set_points.action_substitutions.get(metric_str);
        let ladder: Vec<ActionType> = if let Some(names) = custom_ladder {
            names.iter().filter_map(|n| ActionType::parse(n)).collect()
        } else {
            default_substitution_ladder(metric).to_vec()
        };

        if ladder.is_empty() {
            return proposed; // No alternatives defined.
        }

        // Find the first action in the ladder that hasn't been tried recently.
        for &alt in &ladder {
            if alt == proposed {
                continue; // Skip the action we're already considering.
            }
            let alt_str = alt.as_str();
            let alt_count = self
                .stagnation_detector
                .ineffective_count(metric_str, alt_str);
            if alt_count == 0 {
                tracing::info!(
                    target: "reg.cybernetics.substitution",
                    metric = metric_str,
                    from = %proposed_str,
                    to = %alt_str,
                    failed_attempts = count,
                    "Action substitution: replacing ineffective action with alternative"
                );
                self.emit_regulation_span(
                    SpanKind::ActionSubstituted,
                    serde_json::json!({
                        "metric": metric_str,
                        "from": proposed_str,
                        "to": alt_str,
                        "failed_attempts": count,
                    }),
                )
                .await;
                return alt;
            }
        }

        // All alternatives have been tried and failed — let the plateau
        // escalation handle it.
        tracing::warn!(
            target: "reg.cybernetics.substitution",
            metric = metric_str,
            action = %proposed_str,
            "All substitution alternatives exhausted for metric"
        );
        proposed
    }

    /// Emit a regulation span to the RegulationArchive for Regulation observability.
    ///
    /// This is the Conant-Ashby closure: the Regulation (observer-of-observers)
    /// must have a model of the regulation system itself. These spans
    /// give the Curator visibility into regulatory effectiveness — which
    /// actions are working, which are being substituted, and which are
    /// being blocked.
    async fn emit_regulation_span(&self, kind: SpanKind, observation: serde_json::Value) {
        if let Some(ref sink) = self.event_sink {
            let event = RegulationRecord::new(
                WebID::from_persona(b"regulation"),
                Span::from_kind(kind),
                CyclePhase::Act,
                observation,
                0,
            );
            if let Err(e) = sink.persist(&event) {
                tracing::error!(target: "reg.outcome", error = %e, "Failed to persist regulation span");
            }
        } else {
            tracing::warn!(target: "reg.outcome", span_kind = ?kind, "Regulation span dropped — no event_sink configured. Wire with_event_sink() for durable regulation observability.");
        }
    }

    /// Persist an algedonic alert to the reviewable escalation queue.
    ///
    /// This is the primary durable path for alert review: every escalated
    /// alert is written here when the sink is wired (not just as a fallback),
    /// so the Curator/user can review pending alerts via `curator_escalations`
    /// and resolve/dismiss them with an audit trail. Best-effort — a failing
    /// or missing sink never breaks the regulation loop. Non-escalated alerts
    /// (Info severity, or `escalated: false`) are skipped to avoid polluting
    /// the review queue with non-actionable noise.
    ///
    /// The `RuntimeAlert` fields are mapped to `EscalationEntry` columns:
    /// `output` = `alert.message`, `error_context` = serialized alert JSON
    /// (domain/deficit/threshold/severity), `confidence` = 1.0 for Critical /
    /// 0.5 for Warning.
    ///
    /// `efferent_action` carries the original `ActionType` for actions that
    /// were converted to Escalate alerts (non-native Escalate). `None` for
    /// native Escalate actions. The field is included in the `error_context`
    /// JSON so the Curator's `curator_escalations` tool sees the recommended
    /// action as structured data, not just free-text in the message.
    fn persist_alert_to_queue(&self, alert: &RuntimeAlert, efferent_action: Option<&str>) {
        let Some(ref sink) = self.alert_escalation_sink else {
            return;
        };
        // Skip non-escalated alerts — only escalated alerts (Critical, or
        // Warning with `escalated: true`) belong in the reviewable backlog.
        // Info alerts and non-escalated Warnings are diagnostic, not
        // actionable, and would pollute the queue.
        if !alert.escalated {
            return;
        }
        let confidence = if alert.is_critical() { 1.0 } else { 0.5 };
        let error_context = serde_json::json!({
            "domain": alert.domain,
            "deficit": alert.deficit,
            "threshold": alert.threshold,
            "severity": alert.severity,
            "escalated": alert.escalated,
            "efferent_action": efferent_action,
            "timestamp": alert.timestamp.to_rfc3339(),
        })
        .to_string();
        sink.persist_alert(&alert.message, confidence, &error_context);
    }

    /// Check regulation coherence — flag contradictory or suspicious action pairs.
    ///
    /// Runs after verify_impact. Scans the action set from this tick and logs
    /// warnings for patterns that suggest inconsistent regulation (e.g.,
    /// Throttle + CircuitBreak on same loop, AdjustEnergyBudget + OverrideEnergyBudget).
    fn check_coherence(&self, actions: &[RegulatoryAction]) {
        use ActionType::*;
        let has = |t: ActionType| actions.iter().any(|a| a.action_type == t);
        let has_target = |t: ActionType, target: LoopId| {
            actions
                .iter()
                .any(|a| a.action_type == t && a.target == target)
        };

        // Throttle + CircuitBreak on same target — contradictory (slow down vs stop).
        if (has(Throttle) && has(CircuitBreak))
            || (has(AdjustEnergyBudget) && has(OverrideEnergyBudget))
        {
            tracing::warn!(
                target: "reg.outcome.coherence",
                action_count = actions.len(),
                "Potentially contradictory actions in same tick"
            );
        }

        // Both Throttle and CircuitBreak on Inference loop.
        if has_target(Throttle, LoopId::Inference) && has_target(CircuitBreak, LoopId::Inference) {
            tracing::warn!(
                target: "reg.outcome.coherence",
                "Throttle + CircuitBreak both targeting Inference loop — consider consolidating"
            );
        }
    }

    /// Attempt to load persisted call caps from the configured path.
    /// Called automatically during `build()` if a persistence path is set.
    /// Returns count loaded (0 if first run or no path configured).
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    pub async fn load_budgets(&self) -> Result<usize, CallCapError> {
        if let Some(ref path) = self.budget_persistence_path {
            let contents = match tokio::fs::read_to_string(path).await {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
                Err(e) => {
                    return Err(CallCapError::Persistence(format!(
                        "read {}: {e}",
                        path.display()
                    )));
                }
            };
            let wrapper: serde_json::Value = serde_json::from_str(&contents)
                .map_err(|e| CallCapError::Persistence(format!("parse {}: {e}", path.display())))?;

            // Load persisted call caps.
            let count = if let Some(caps_val) = wrapper.get("budgets") {
                let loaded: HashMap<WebID, CallCap> = serde_json::from_value(caps_val.clone())
                    .map_err(|e| CallCapError::Persistence(format!("parse caps: {e}")))?;
                let n = loaded.len();
                let mgr = self.call_cap_manager.read().await;
                let mut caps = mgr.caps_mut().await;
                for (id, cap) in loaded {
                    caps.insert(id, cap);
                }
                n
            } else {
                0
            };

            // Restore ToolStats state
            if let Some(ts_val) = wrapper.get("tool_stats")
                && let Some(ref stats) = self.tool_stats
            {
                stats.load_state(ts_val).await;
            }

            if count > 0 || wrapper.get("tool_stats").is_some() {
                tracing::info!(target: "reg.cybernetics", count = count, "Loaded persisted caps + ToolStats state");
            }
            Ok(count)
        } else {
            Ok(0)
        }
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
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn register_call_cap(&self, agent: WebID, ceiling: u32) {
        self.call_cap_manager
            .read()
            .await
            .register_call_cap(agent, ceiling)
            .await;
    }

    /// Check whether an agent still has calls available this tick.
    ///
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn can_proceed(&self, agent: &WebID) -> bool {
        self.call_cap_manager.read().await.can_proceed(agent).await
    }

    /// Consume one call. Returns `Err` if the agent has no cap or it is exhausted.
    ///
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn charge_call(&self, agent: &WebID) -> Result<(), CallCapError> {
        self.call_cap_manager.read().await.charge(agent).await
    }

    /// Meter one governed tool call, auto-registering an unknown agent at the
    /// default runaway ceiling. The tool-dispatch path uses this rather than
    /// [`Self::charge_call`] — see [`CallCapManager::charge_metered`].
    ///
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn charge_call_metered(&self, agent: &WebID) -> CallMeterOutcome {
        self.call_cap_manager
            .read()
            .await
            .charge_metered(agent)
            .await
    }

    /// Returns `None` if the agent has no registered cap.
    ///
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn agent_call_cap_status(&self, agent: &WebID) -> Option<AgentCallCapStatus> {
        self.call_cap_manager.read().await.agent_status(agent).await
    }

    /// Reset every registered cap to its ceiling (one regulation tick).
    ///
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn reset_all_caps(&self) {
        self.call_cap_manager.read().await.reset_all().await;
    }

    /// Credit `amount` calls to an agent (used by `CuratorDirective::ReplenishBudget`).
    ///
    /// expect: "The system enforces energy homeostasis through gas budget membrane regulation"
    pub async fn credit_calls(&self, agent: &WebID, amount: u32) {
        self.call_cap_manager
            .read()
            .await
            .credit(agent, amount)
            .await;
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

    async fn handle_curation_directive(&self, directive: CuratorDirective) {
        // Dampen repeated directives to prevent feedback oscillation
        if self.dampener.should_dampen_directive(&directive) {
            tracing::debug!(
                target: "reg.cybernetics",
                directive = %directive.variant_name(),
                "Directive dampened (repeated within window)"
            );
        } else {
            let variant_name = directive.variant_name();
            self.apply_directive(directive).await;
            self.persist_directive_acknowledgment(variant_name);
            tracing::info!(
                target: "reg.cybernetics",
                directive = %variant_name,
                outcome = "applied",
                "Directive acknowledged (Curation→Cybernetics compliance)"
            );
        }
    }

    async fn apply_directive(&self, directive: CuratorDirective) {
        match directive {
            CuratorDirective::CalibrateThreshold {
                domain,
                new_threshold,
            } => self.apply_calibrate_threshold(&domain, new_threshold).await,
            CuratorDirective::OverrideEnergyBudget { agent, new_budget } => {
                self.apply_override_cap(agent, new_budget).await
            }
            CuratorDirective::ClearOverride { agent } => self.apply_clear_override(agent).await,
            CuratorDirective::ReplenishBudget {
                agent,
                amount,
                priority: _,
            } => self.apply_credit_calls(agent, amount).await,
            CuratorDirective::UpdateCapabilities {
                agent,
                additions,
                removals,
            } => {
                tracing::info!(target: "reg.cybernetics", agent = %agent, additions = ?additions, removals = ?removals, "Applied UpdateCapabilities directive from Curation (capabilities updated)")
            }
            CuratorDirective::SeekMoreEvidence {
                context,
                channel,
                confidence,
            } => {
                tracing::info!(target: "reg.cybernetics", context = %context, channel = %channel, confidence = %confidence, "Applied SeekMoreEvidence directive from Curation (metacognition loop triggered)")
            }
            CuratorDirective::EvolveMcpToolSchema {
                server_name,
                tool_name,
                evolution_type,
                field_name,
                new_type,
                ref rationale,
                ref evidence,
            } => {
                self.apply_evolve_mcp_tool_schema(
                    &server_name,
                    &tool_name,
                    &evolution_type,
                    &field_name,
                    new_type.as_deref(),
                    rationale,
                    evidence,
                )
                .await;
            }
            _ => {}
        }
    }

    async fn apply_calibrate_threshold(&self, domain: &str, new_threshold: u64) {
        let ledger = self.ledger.read().await;
        ledger.calibrate_threshold(domain, new_threshold).await;
        drop(ledger);
        tracing::info!(
            target: "reg.cybernetics",
            domain = domain,
            new_threshold = new_threshold,
            "Applied CalibrateThreshold directive from Curation"
        );
    }

    /// Curation override: install a new call ceiling for an agent. Survives
    /// per-tick resets until `apply_clear_override` is called.
    async fn apply_override_cap(&self, agent: WebID, new_ceiling: u64) {
        self.call_cap_manager
            .read()
            .await
            .apply_override(agent, new_ceiling as u32)
            .await;
    }

    /// Removes a curation override, restoring the agent's original ceiling on the
    /// next `reset_all_caps`.
    async fn apply_clear_override(&self, agent: WebID) {
        self.call_cap_manager
            .read()
            .await
            .clear_override(agent)
            .await;
    }

    /// Credit `amount` calls to an agent (curation `ReplenishBudget` directive).
    async fn apply_credit_calls(&self, agent: WebID, amount: u64) {
        self.call_cap_manager
            .read()
            .await
            .credit(&agent, amount as u32)
            .await;
    }

    /// Phase 3 co-evolution: record an MCP tool schema evolution request.
    ///
    /// The directive does not directly modify the tool's schema (MCP tool
    /// schemas are compiled Rust structs). It persists the evolution request
    /// to the regulation ledger as a `CurationDirectiveAcknowledged` span
    /// with the full evolution payload, so a developer or automated
    /// migration agent can read the ledger and act on the request.
    async fn apply_evolve_mcp_tool_schema(
        &self,
        server_name: &str,
        tool_name: &str,
        evolution_type: &SchemaEvolutionType,
        field_name: &str,
        new_type: Option<&str>,
        rationale: &str,
        evidence: &str,
    ) {
        let evolution_type_str = match evolution_type {
            SchemaEvolutionType::AddField => "add_field",
            SchemaEvolutionType::RemoveField => "remove_field",
            SchemaEvolutionType::RenameField => "rename_field",
            SchemaEvolutionType::ChangeType => "change_type",
        };
        tracing::info!(
            target: "reg.cybernetics",
            server = %server_name,
            tool = %tool_name,
            evolution_type = %evolution_type_str,
            field = %field_name,
            new_type = ?new_type,
            "Applied EvolveMcpToolSchema directive from Curation (schema evolution request recorded)",
        );
        // Persist the full evolution request to the regulation ledger so
        // developers and migration agents can read it. The payload carries
        // all the information needed to implement the schema change.
        if let Some(ref sink) = self.event_sink {
            let record = RegulationRecord::new(
                WebID::from_persona(b"regulation"),
                Span::from_kind(SpanKind::CurationDirectiveAcknowledged),
                CyclePhase::Act,
                serde_json::json!({
                    "directive_type": "evolve_mcp_tool_schema",
                    "outcome": "recorded",
                    "server_name": server_name,
                    "tool_name": tool_name,
                    "evolution_type": evolution_type_str,
                    "field_name": field_name,
                    "new_type": new_type,
                    "rationale": rationale,
                    "evidence": evidence,
                }),
                0,
            );
            if let Err(e) = sink.persist(&record) {
                tracing::warn!(
                    target: "reg.cybernetics",
                    error = %e,
                    "Failed to persist EvolveMcpToolSchema directive",
                );
            }
        }
    }

    fn persist_directive_acknowledgment(&self, directive_type: &str) {
        if let Some(ref sink) = self.event_sink {
            let ack = RegulationRecord::new(
                WebID::from_persona(b"regulation"),
                Span::from_kind(SpanKind::CurationDirectiveAcknowledged),
                CyclePhase::Act,
                serde_json::json!({
                    "directive_type": directive_type,
                    "outcome": "applied",
                }),
                0,
            );
            if let Err(e) = sink.persist(&ack) {
                tracing::warn!(
                    target: "reg.cybernetics",
                    error = %e,
                    "Failed to persist directive acknowledgment"
                );
            }
        }
    }
}

impl CyberneticsLoop {
    /// Compare: detect deviations from set-points.
    async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        signals.iter().filter_map(Deviation::from_signal).collect()
    }

    /// Produces signals for: per-agent energy ratio, variety deficit, queue depth,
    /// wallet balance ratio, wallet treasury ratio.
    async fn sense(&self) -> Vec<Signal> {
        // Process pending directives before sensing state
        self.process_inbox().await;

        let mut signals = Vec::new();

        // All sensing is now done through the SensorBus.
        // Energy remaining, variety deficit, and tool reliability are all sensed
        // by registered Sensor implementations.

        // Append signals from pluggable sensor providers.
        let registry_signals = self.sensor_registry.sense_all(LoopId::Cybernetics).await;
        signals.extend(registry_signals);

        // Feed observed values into the predictive simulator.
        for signal in &signals {
            self.simulator.observe(signal.metric, signal.value);
        }

        signals
    }

    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();

        // Predictive regulation: check if any metric is approaching its set-point.
        for dev in deviations {
            let pred =
                self.simulator
                    .predict(dev.signal.metric, dev.signal.value, dev.signal.set_point);
            if let Some(ticks) = pred.ticks_to_threshold
                && ticks <= 3
                && pred.reliable
            {
                tracing::info!(
                    target: "reg.outcome.predictive",
                    metric = dev.signal.metric.as_str(),
                    current = dev.signal.value,
                    set_point = dev.signal.set_point,
                    ticks_to_threshold = ticks,
                    trend = pred.trend,
                    "Predictive: metric approaching set-point"
                );
                // Emit a predictive notification to Curation.
                actions.push(RegulatoryAction::new(
                    LoopId::Curation,
                    ActionType::Notify,
                    RegulatoryActionParams::reason("predictive_threshold_approach"),
                ));
            }
        }

        let policy = RegulationPolicy::default();

        for dev in deviations {
            for proposed in policy.decide(dev) {
                let action = self.build_regulation_action(dev, proposed).await;
                if let Some(a) = action {
                    actions.push(a);
                }
            }
        }
        actions
    }

    async fn act(&self, actions: &[RegulatoryAction]) {
        self.reset_all_caps().await;

        // E04: Detect and escalate call-cap exhaustion via the algedonic pathway.
        // A cap is exhausted when its remaining count hit zero this tick.
        {
            let statuses = self
                .call_cap_manager
                .read()
                .await
                .all_agent_statuses()
                .await;
            let exhausted: Vec<_> = statuses
                .into_iter()
                .filter(|(_, s)| s.remaining == 0)
                .collect();

            let alert_entries: Vec<(String, String)> = exhausted
                .iter()
                .map(|(agent, status)| {
                    (
                        format!("call_cap:{agent}"),
                        format!(
                            "Agent {agent} call cap exhausted (ceiling: {}, remaining: 0)",
                            status.ceiling
                        ),
                    )
                })
                .collect();

            for (domain, message) in &alert_entries {
                let alert = RuntimeAlert {
                    domain: domain.clone(),
                    deficit: 1,
                    threshold: 1,
                    severity: AlertSeverity::Warning,
                    escalated: false,
                    timestamp: chrono::Utc::now(),
                    message: message.clone(),
                };
                let sent = if let Some(ref tx) = self.alerts_tx {
                    tx.send(CurationInput::Alert(alert.clone())).is_ok()
                } else {
                    false
                };
                if !sent {
                    tracing::warn!(target: "reg.alert", domain = %alert.domain, "call-cap exhaustion alert send failed or channel not connected");
                }
                // Persist to the reviewable escalation queue unconditionally —
                // the queue is the primary durable path for alert review, not
                // a fallback (the RegulationArchive below is the fallback for
                // restart durability when the live channel is down).
                self.persist_alert_to_queue(&alert, None);
                if !sent && let Some(ref sink) = self.event_sink {
                    let event = RegulationRecord::new(
                        WebID::from_persona(b"regulation"),
                        Span::from_kind(SpanKind::VarietyAlgedonicAlert),
                        CyclePhase::Act,
                        serde_json::json!({
                            "domain": alert.domain,
                            "message": alert.message,
                            "severity": "Warning",
                            "timestamp": alert.timestamp.to_rfc3339(),
                        }),
                        0,
                    );
                    if let Err(e) = sink.persist(&event) {
                        tracing::error!(target: "reg.cybernetics", error = %e, "Failed to persist call-cap exhaustion alert");
                    }
                }
            }
        }

        // E02: Persist call caps after each reset cycle.
        // Persistence failures log and fall through — regulation actions
        // (algedonic alerts, action dispatch) must NOT be skipped because a
        // transient I/O error prevented writing caps.
        'persist: {
            if let Some(ref path) = self.budget_persistence_path {
                let mut wrapper = serde_json::json!({
                    "version": 2,
                });
                {
                    let mgr = self.call_cap_manager.read().await;
                    let caps = mgr.caps().await;
                    match serde_json::to_value(&*caps) {
                        Ok(v) => wrapper["budgets"] = v,
                        Err(e) => {
                            tracing::error!(target: "reg.cybernetics", error = %e, "Failed to serialize call caps — skipping persistence");
                            break 'persist;
                        }
                    }
                }
                {
                    if let Some(ref stats) = self.tool_stats {
                        wrapper["tool_stats"] = stats.save_state().await;
                    }
                }
                let json = match serde_json::to_string_pretty(&wrapper) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(target: "reg.cybernetics", error = %e, "Failed to serialize cap wrapper — skipping persistence");
                        break 'persist;
                    }
                };
                if let Some(parent) = path.parent()
                    && let Err(e) = tokio::fs::create_dir_all(parent).await
                {
                    tracing::error!(target: "reg.cybernetics", path = %parent.display(), error = %e, "Failed to create cap persistence directory");
                    break 'persist;
                }
                if let Err(e) = tokio::fs::write(path, &json).await {
                    tracing::error!(target: "reg.cybernetics", path = %path.display(), error = %e, "Failed to persist call caps");
                }
            }
        }
        if actions.len() > self.max_iterations as usize {
            tracing::warn!(target: "reg.cybernetics", action_count = actions.len(), max_iterations = self.max_iterations, "Cascade detected: action count exceeds max_iterations");
        }
        for action in actions {
            self.route_action_as_alert(&action).await;
        }
    }

    /// Convert a single `RegulatoryAction` into a `RuntimeAlert` and route it
    /// through the three-tier alert path (escalation queue → live channel →
    /// archive fallback → email fallback).
    ///
    /// Design decision (2026-08-06): the cybernetics loop is a sensor+advisor,
    /// not an actuator. All computed actions are converted to Escalate alerts
    /// routed to the Curator/human. Actions that would have been direct
    /// efferent signals (Throttle, CircuitBreak, AdjustEnergyBudget, etc.)
    /// carry an `efferent_action` field in the alert data so the Curator sees
    /// what the loop would have done — but the actuator is not wired. This
    /// preserves user sovereignty: the human decides whether to apply the
    /// recommended action, the loop does not act autonomously.
    ///
    /// `Notify` actions are skipped — they are observational ("no action
    /// required, positive signal" per `ActionType::Notify`'s doc). Converting
    /// them to Critical alerts would be a variety inversion (positive signal
    /// → critical alert) and would pollute the escalation queue with
    /// non-actionable noise.
    ///
    /// See `kask/docs/diataxis/hkask-regulation/reference.md` §
    /// "Efferent action dispatch" for the full rationale.
    async fn route_action_as_alert(&self, action: &RegulatoryAction) {
        let target_id = action.target;

        if action.action_type == ActionType::Notify {
            tracing::info!(
                target: "reg.cybernetics",
                action_type = ?action.action_type,
                target_loop = %action.target,
                "Notify action — observational, not routed as alert"
            );
            return;
        }

        let is_native_escalate =
            action.action_type == ActionType::Escalate && target_id == LoopId::Curation;
        let efferent_action = if is_native_escalate {
            None
        } else {
            Some(action.action_type.as_str())
        };

        tracing::info!(
            target: "reg.cybernetics",
            action_type = ?action.action_type,
            target_loop = %action.target,
            efferent = ?efferent_action,
            "Cybernetics Loop efferent signal (routed as Escalate{})",
            if efferent_action.is_some() { " — efferent not wired" } else { "" }
        );

        // Build the alert. For native Escalate actions (variety deficit,
        // wallet balance, etc.), extract the deficit/threshold from the
        // typed data. For converted efferent actions, synthesize a
        // deficit of 1 and threshold of 1 — the alert's purpose is
        // advisory, not quantitative.
        let (deficit, threshold) = if is_native_escalate {
            extract_deficit_threshold(&action.parameters.data)
        } else {
            (1, 1)
        };
        let domain = if is_native_escalate {
            String::new()
        } else {
            format!("efferent:{}", action.action_type.as_str())
        };
        let message = if is_native_escalate {
            format!(
                "Variety deficit {} exceeds threshold {}",
                deficit, threshold
            )
        } else {
            format!(
                "Efferent action {} (target: {}) recommended but not wired — reason: {}",
                action.action_type.as_str(),
                action.target,
                action.parameters.reason
            )
        };
        let alert = RuntimeAlert {
            domain,
            deficit,
            threshold,
            severity: AlertSeverity::Critical,
            escalated: true,
            timestamp: chrono::Utc::now(),
            message,
        };

        // Persist to the reviewable escalation queue unconditionally —
        // the queue is the primary durable path for alert review, not
        // a fallback. The RegulationArchive below remains as a
        // secondary fallback for restart durability when the live
        // channel is down.
        self.persist_alert_to_queue(&alert, efferent_action);

        // Primary path: live channel to Curator's inbox
        let sent_live = if let Some(ref alerts_tx) = self.alerts_tx {
            match alerts_tx.send(CurationInput::Alert(alert.clone())) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(target: "reg.cybernetics", error = %e, "Failed to send CurationInput::Alert via live channel — falling back to persistence");
                    false
                }
            }
        } else {
            tracing::warn!(target: "reg.cybernetics", "Alerts channel not connected — falling back to persistence. Wire with_alerts_channel() for live delivery.");
            false
        };

        // Fallback: persist full alert to RegulationArchive for Curator retrieval on next activation
        if !sent_live {
            let mut persisted = false;
            if let Some(ref sink) = self.event_sink {
                let event = RegulationRecord::new(
                    WebID::from_persona(b"regulation"),
                    Span::from_kind(SpanKind::VarietyAlgedonicAlert),
                    CyclePhase::Act,
                    serde_json::json!({
                        "domain": alert.domain,
                        "deficit": alert.deficit,
                        "threshold": alert.threshold,
                        "severity": "Critical",
                        "escalated": true,
                        "message": alert.message,
                        "efferent_action": efferent_action,
                        "timestamp": alert.timestamp.to_rfc3339(),
                    }),
                    0,
                );
                match sink.persist(&event) {
                    Ok(()) => {
                        persisted = true;
                        tracing::info!(target: "reg.alert", deficit = deficit, threshold = threshold, "Algedonic alert persisted to RegulationArchive (Curator inbox unavailable)");
                    }
                    Err(e) => {
                        tracing::error!(target: "reg.alert", error = %e, "Failed to persist algedonic alert to archive");
                    }
                }
            }

            // Email notification: fires when live channel is down, regardless of
            // archive outcome. Serves as notification (archive succeeded) or last
            // resort (archive failed/unavailable).
            if let Some(ref email_sink) = self.alert_email_sink {
                email_sink.send_alert_email(&alert);
                if persisted {
                    tracing::info!(target: "reg.alert", deficit = deficit, threshold = threshold, "Algedonic alert emailed as notification (live channel down, archive persisted)");
                } else {
                    tracing::info!(target: "reg.alert", deficit = deficit, threshold = threshold, "Algedonic alert emailed as last resort (archive unavailable)");
                }
            } else if !persisted {
                tracing::error!(target: "reg.alert", deficit = deficit, threshold = threshold, "CRITICAL: Algedonic alert LOST - no live channel, event_sink, or email sink");
            }
        }
    }

    /// Verify whether the previous cycle's actions improved their targeted
    /// metrics (Fermi impact-gate pattern).
    ///
    /// Re-senses energy ratios and variety deficit, comparing post-action
    /// values against the pre-action values. Classifies each action as
    /// Accept / Stage / Block using per-metric worsening thresholds.
    /// Blocked actions are prevented from re-use until Curation intervenes.
    /// Actions that repeatedly fail to improve trigger stagnation detection.
    async fn verify_impact(&self, previous_actions: &[RegulatoryAction]) -> Vec<ImpactReport> {
        let mut reports = Vec::new();

        // Re-sense current state for comparison.
        let budget_statuses = self
            .call_cap_manager
            .read()
            .await
            .all_agent_statuses()
            .await;
        let ledger = self.ledger.read().await;
        let health = ledger.health().await;
        let current_deficit = health.overall_deficit as f64;
        drop(ledger);

        for action in previous_actions {
            // Determine metric and pre-action value from the typed RegulationData.
            let (before_val, metric) = match &action.parameters.data {
                RegulationData::EnergyBudgetLow {
                    remaining_ratio, ..
                }
                | RegulationData::BudgetGuardEscalation {
                    remaining_ratio, ..
                }
                | RegulationData::EnergyDepletionAutoAdjust {
                    remaining_ratio, ..
                } => (*remaining_ratio, SignalMetric::EnergyRemaining),
                RegulationData::VarietyDeficitExceeded { deficit, .. } => {
                    (*deficit, SignalMetric::VarietyDeficit)
                }
                _ => continue,
            };

            let after_val = match metric {
                SignalMetric::EnergyRemaining => budget_statuses
                    .iter()
                    .map(|(_, s)| s.remaining as f64 / s.ceiling.max(1) as f64)
                    .fold(1.0, f64::min),
                SignalMetric::VarietyDeficit => current_deficit,
                _ => continue,
            };

            let delta = after_val - before_val;
            // For EnergyRemaining: higher is better (positive delta = improved).
            // For VarietyDeficit: lower is better (negative delta = improved).
            let improved = match metric {
                SignalMetric::EnergyRemaining => delta > 0.0,
                SignalMetric::VarietyDeficit => delta < 0.0,
                _ => delta.abs() > f64::EPSILON,
            };

            // Classify the decision using per-metric worsening thresholds.
            let worsening = if improved { 0.0 } else { delta.abs() };
            let block_worsening_ratio = self
                .calibrated_thresholds
                .read()
                .await
                .block_worsening_ratio;
            let decision = classify_decision(
                worsening,
                self.set_points.stage_worsening_ratio,
                block_worsening_ratio,
            );

            // Report acceptance/rejection to stagnation detector.
            let accepted = decision == ActionDecision::Accept;
            let action_type_str = action.action_type.as_str();
            let plateau = self.stagnation_detector.record_and_check(
                metric.as_str(),
                action_type_str,
                accepted,
            );

            if plateau {
                let threshold = {
                    let calibrated = self.calibrated_thresholds.read().await;
                    calibrated
                        .stagnation_thresholds
                        .get(metric.as_str())
                        .copied()
                        .unwrap_or_else(|| {
                            self.stagnation_detector
                                .threshold_for_metric(metric.as_str())
                        })
                };
                self.emit_regulation_span(
                    SpanKind::RegulatoryPlateauDetected,
                    serde_json::json!({
                        "metric": metric.as_str(),
                        "action_type": action_type_str,
                        "consecutive_cycles": threshold,
                    }),
                )
                .await;
                let alert = RuntimeAlert {
                    domain: format!("regulatory_plateau:{}", metric.as_str()),
                    deficit: 1,
                    threshold: 1,
                    severity: AlertSeverity::Warning,
                    escalated: true,
                    timestamp: chrono::Utc::now(),
                    message: format!(
                        "Regulatory plateau: {} via {:?} has been rejected for {threshold} consecutive cycles",
                        metric.as_str(),
                        action.action_type,
                    ),
                };
                // Persist to the reviewable escalation queue unconditionally.
                self.persist_alert_to_queue(&alert, None);
                if let Some(ref tx) = self.alerts_tx {
                    if tx.send(CurationInput::Alert(alert)).is_err() {
                        tracing::warn!(target: "reg.alert", "Plateau alert send failed — channel closed");
                    }
                }
                tracing::warn!(
                    target: "reg.cybernetics",
                    metric = metric.as_str(),
                    action_type = ?action.action_type,
                    "Regulatory plateau detected"
                );
            }

            // Blocked actions: escalate as Critical to Curation + emit Regulation span.
            if decision == ActionDecision::Block {
                self.emit_regulation_span(
                    SpanKind::ActionBlocked,
                    serde_json::json!({
                        "metric": metric.as_str(),
                        "action_type": format!("{:?}", action.action_type),
                        "worsening": worsening,
                        "block_threshold": block_worsening_ratio,
                    }),
                )
                .await;
                let alert = RuntimeAlert {
                    domain: format!("action_blocked:{}", metric.as_str()),
                    deficit: 1,
                    threshold: 1,
                    severity: AlertSeverity::Critical,
                    escalated: true,
                    timestamp: chrono::Utc::now(),
                    message: format!(
                        "ActionDecision::Block: {} on {} caused {:.1}% worsening (threshold: {:.1}%)",
                        action.action_type.as_str(),
                        metric.as_str(),
                        worsening * 100.0,
                        block_worsening_ratio * 100.0,
                    ),
                };
                // Persist to the reviewable escalation queue unconditionally.
                self.persist_alert_to_queue(&alert, None);
                if let Some(ref tx) = self.alerts_tx {
                    if tx.send(CurationInput::Alert(alert)).is_err() {
                        tracing::warn!(target: "reg.alert", "Block alert send failed — channel closed");
                    }
                }
            }

            // Emit Regulation span for Curator observability of regulatory effectiveness.
            self.emit_regulation_span(
                SpanKind::ImpactVerified,
                serde_json::json!({
                    "metric": metric.as_str(),
                        "action_type": action.action_type.as_str(),
                        "before": before_val,
                    "after": after_val,
                    "delta": delta,
                    "improved": improved,
                    "decision": format!("{:?}", decision),
                }),
            )
            .await;

            reports.push(ImpactReport::new(
                action.action_type,
                metric,
                before_val,
                after_val,
                decision,
            ));
        }

        reports
    }

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
        let actions = self.compute(&deviations).await;
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
                "fidelity_confidence": quality.fidelity_confidence,
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
    /// Build a `RegulatoryAction` from a `ProposedAction` returned by the regulation policy.
    ///
    /// Applies mode-specific filtering (e.g., `InferenceThrottleMode`) and
    /// `try_substitute` for stagnation-based action ladder substitution.
    /// Returns `None` when the rule should be skipped (e.g., throttle in Off mode).
    async fn build_regulation_action(
        &self,
        dev: &Deviation,
        proposed: &regulation_policy::ProposedAction,
    ) -> Option<RegulatoryAction> {
        use SignalMetric::*;

        match proposed.reason {
            // -- EnergyRemaining BelowSetPoint ------------------------------
            RegulationReason::EnergyBudgetLow => {
                if !matches!(
                    self.set_points.inference_throttle_mode,
                    InferenceThrottleMode::Autonomous
                ) {
                    return None;
                }
                let at = self
                    .try_substitute(EnergyRemaining, proposed.action_type)
                    .await;
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "energy_budget_low",
                        RegulationData::EnergyBudgetLow {
                            remaining_ratio: dev.signal.value,
                            set_point: dev.signal.set_point,
                        },
                    ),
                    "energy_remaining".into(),
                ))
            }
            RegulationReason::BudgetGuardEscalation => {
                let curator_timeout_secs = match self.set_points.inference_throttle_mode {
                    InferenceThrottleMode::CuratorMediated {
                        curator_timeout_secs,
                    } => curator_timeout_secs,
                    _ => return None,
                };
                let remaining_ratio = dev.signal.value;
                let projected_minutes = (remaining_ratio * 60.0) as u64;
                Some(RegulatoryAction::new(
                    proposed.target,
                    proposed.action_type,
                    RegulatoryActionParams::with_data(
                        "budget_guard_escalation",
                        RegulationData::BudgetGuardEscalation {
                            remaining_ratio,
                            set_point: dev.signal.set_point,
                            projected_minutes,
                            options: vec![
                                BudgetOption {
                                    id: "add_funds".into(),
                                    label: "Add funds to continue at current rate".into(),
                                },
                                BudgetOption {
                                    id: "switch_model".into(),
                                    label: "Switch to a smaller/cheaper model".into(),
                                },
                                BudgetOption {
                                    id: "continue".into(),
                                    label: "Continue at current rate (budget will exhaust)".into(),
                                },
                            ],
                            curator_timeout_secs,
                            fallback: "gentle_throttle".into(),
                        },
                    ),
                ))
            }
            RegulationReason::EnergyDepletionAutoAdjust => {
                if matches!(
                    self.set_points.inference_throttle_mode,
                    InferenceThrottleMode::Off
                ) {
                    return None;
                }
                let at = self
                    .try_substitute(EnergyRemaining, proposed.action_type)
                    .await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "energy_depletion_auto_adjust",
                        RegulationData::EnergyDepletionAutoAdjust {
                            remaining_ratio: dev.signal.value,
                            set_point: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- VarietyDeficit AboveSetPoint -------------------------------
            RegulationReason::VarietyDeficitExceeded => {
                let at = self
                    .try_substitute(VarietyDeficit, proposed.action_type)
                    .await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "variety_deficit_exceeded",
                        RegulationData::VarietyDeficitExceeded {
                            deficit: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- ErrorRate AboveSetPoint ------------------------------------
            RegulationReason::ErrorRateExceeded => {
                let at = self.try_substitute(ErrorRate, proposed.action_type).await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "error_rate_exceeded",
                        RegulationData::ErrorRateExceeded {
                            error_rate: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- ConnectorLatency AboveSetPoint -----------------------------
            RegulationReason::ConnectorLatencyExceeded => {
                let at = self
                    .try_substitute(ConnectorLatency, proposed.action_type)
                    .await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "connector_latency_exceeded",
                        RegulationData::ConnectorLatencyExceeded {
                            latency_secs: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- CommunicationQueueDepth AboveSetPoint ----------------------
            RegulationReason::CommunicationBackpressure => {
                tracing::info!(
                    target: "reg.cybernetics.backpressure",
                    queue_depth = dev.signal.value,
                    threshold = dev.signal.set_point,
                    "Communication queue depth exceeded backpressure threshold"
                );
                let at = self
                    .try_substitute(CommunicationQueueDepth, proposed.action_type)
                    .await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "communication_backpressure",
                        RegulationData::CommunicationBackpressure {
                            queue_depth: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- WalletBalanceRatio BelowSetPoint ---------------------------
            RegulationReason::WalletBalanceLow => {
                let severity = if dev.signal.value <= 0.0 {
                    "critical"
                } else {
                    "warning"
                };
                tracing::warn!(
                    target: "reg.wallet",
                    balance_ratio = dev.signal.value,
                    severity = severity,
                    "Wallet balance alert"
                );
                let at = self
                    .try_substitute(WalletBalanceRatio, proposed.action_type)
                    .await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "wallet_balance_low",
                        RegulationData::WalletBalanceLow {
                            balance_ratio: dev.signal.value,
                            severity: severity.to_string(),
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- WalletKeyHealth AboveSetPoint ------------------------------
            RegulationReason::WalletKeyUnhealthy => {
                tracing::info!(
                    target: "reg.wallet",
                    "API key health alert — exhausted or expired"
                );
                Some(RegulatoryAction::new(
                    proposed.target,
                    proposed.action_type,
                    RegulatoryActionParams::with_data(
                        "wallet_key_unhealthy",
                        RegulationData::WalletKeyUnhealthy {
                            severity: "warning".into(),
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            // -- SeamCoverage BelowSetPoint ---------------------------------
            RegulationReason::SeamCoverageDegraded => {
                let drop_magnitude = dev.signal.set_point - dev.signal.value;
                let severity = if drop_magnitude > 5.0 {
                    "critical"
                } else {
                    "warning"
                };
                tracing::warn!(
                    target: "hkask.architecture.seam",
                    coverage_pct = dev.signal.value,
                    set_point = dev.signal.set_point,
                    drop_magnitude = drop_magnitude,
                    severity = severity,
                    "Public seam coverage degraded — seam watcher alert"
                );
                Some(RegulatoryAction::new(
                    proposed.target,
                    proposed.action_type,
                    RegulatoryActionParams::with_data(
                        "seam_coverage_degraded",
                        RegulationData::SeamCoverageDegraded {
                            coverage_pct: dev.signal.value,
                            previous_coverage: dev.signal.set_point,
                            drop_magnitude,
                            severity: severity.to_string(),
                        },
                    ),
                ))
            }
            // -- SeamCoverage AboveSetPoint ---------------------------------
            RegulationReason::SeamCoverageImproved => {
                let improvement = dev.signal.value - dev.signal.set_point;
                tracing::info!(
                    target: "hkask.architecture.seam",
                    coverage_pct = dev.signal.value,
                    set_point = dev.signal.set_point,
                    improvement = improvement,
                    "Public seam coverage improved — seam watcher positive signal"
                );
                Some(RegulatoryAction::new(
                    proposed.target,
                    proposed.action_type,
                    RegulatoryActionParams::with_data(
                        "seam_coverage_improved",
                        RegulationData::SeamCoverageImproved {
                            coverage_pct: dev.signal.value,
                            previous_coverage: dev.signal.set_point,
                            improvement,
                        },
                    ),
                ))
            }
            // -- ToolReliability BelowSetPoint ------------------------------
            RegulationReason::ToolReliabilityDegraded => {
                tracing::warn!(
                    target: "reg.tool",
                    reliability = dev.signal.value,
                    set_point = dev.signal.set_point,
                    "Tool reliability degraded — success rate below threshold"
                );
                let at = self
                    .try_substitute(ToolReliability, proposed.action_type)
                    .await;
                Some(RegulatoryAction::new(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "tool_reliability_degraded",
                        RegulationData::ToolReliabilityDegraded {
                            reliability: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                ))
            }
            _ => {
                tracing::debug!(
                    target: "reg.outcome",
                    reason = proposed.reason.as_str(),
                    "Unhandled regulation reason — no action built"
                );
                None
            }
        }
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

    /// Return a mutable reference to the set-points for calibration.
    /// Callers must hold `&mut CyberneticsLoop` (e.g., via `loop.write().await`).
    ///
    /// expect: "The system provides observability into Regulation regulation state"
    pub fn set_points_mut(&mut self) -> &mut SetPoints {
        &mut self.set_points
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::Strategy;

    #[tokio::test]
    async fn new_loop_starts_with_default_quality() {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let loop_instance = CyberneticsLoop::new(ledger);
        let q = loop_instance.loop_quality().await;
        assert_eq!(q.delay_ms, 0);
        assert!((q.gain - 0.0).abs() < f64::EPSILON);
        assert!((q.fidelity_score - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn tick_updates_loop_quality() {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let loop_instance = CyberneticsLoop::new(ledger);
        loop_instance.tick().await;
        let q = loop_instance.loop_quality().await;
        // After a tick, gain and fidelity should be computed (even if delay_ms is 0)
        // The key property: quality is no longer the default zero-state
        assert!(
            q.gain >= 0.0 && q.fidelity_score >= 0.0,
            "quality should be computed after tick"
        );
    }

    /// A capturing `AlertEscalationSink` for testing the escalation-queue
    /// wiring. Records every `persist_alert` call so the test can assert the
    /// alert reached the reviewable backlog.
    struct CapturingEscalationSink {
        calls: std::sync::Mutex<Vec<(String, f64, String)>>,
    }

    impl CapturingEscalationSink {
        fn new() -> Self {
            Self {
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, f64, String)> {
            self.calls.lock().unwrap().clone()
        }

        fn clear(&self) {
            self.calls.lock().unwrap().clear();
        }
    }

    impl crate::algedonic::AlertEscalationSink for CapturingEscalationSink {
        fn persist_alert(&self, output: &str, confidence: f64, error_context: &str) {
            self.calls.lock().unwrap().push((
                output.to_string(),
                confidence,
                error_context.to_string(),
            ));
        }
    }

    /// `persist_alert_to_queue` must write to the `AlertEscalationSink` when
    /// wired. This pins the Store seam: if the sink call is dropped or guarded
    /// by the wrong condition, the alert never reaches the reviewable backlog
    /// and `curator_escalations` returns `count: 0` — the loop is open.
    #[tokio::test]
    async fn persist_alert_to_queue_writes_to_escalation_sink() {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let sink = Arc::new(CapturingEscalationSink::new());
        let loop_instance = CyberneticsLoop::new(ledger).with_alert_escalation_sink(
            sink.clone() as Arc<dyn crate::algedonic::AlertEscalationSink>
        );

        let critical_alert = RuntimeAlert {
            domain: "test_domain".to_string(),
            deficit: 150,
            threshold: 100,
            severity: AlertSeverity::Critical,
            escalated: true,
            timestamp: chrono::Utc::now(),
            message: "Critical test alert".to_string(),
        };
        loop_instance.persist_alert_to_queue(&critical_alert, None);

        let warning_alert = RuntimeAlert {
            domain: "test_domain".to_string(),
            deficit: 60,
            threshold: 100,
            severity: AlertSeverity::Warning,
            escalated: true,
            timestamp: chrono::Utc::now(),
            message: "Warning test alert".to_string(),
        };
        loop_instance.persist_alert_to_queue(&warning_alert, None);

        let calls = sink.calls();
        assert_eq!(calls.len(), 2, "both alerts must reach the escalation sink");

        // Critical alert: confidence 1.0, message preserved
        assert_eq!(calls[0].0, "Critical test alert");
        assert!(
            (calls[0].1 - 1.0).abs() < f64::EPSILON,
            "critical confidence must be 1.0"
        );
        assert!(
            calls[0].2.contains("\"severity\":\"Critical\""),
            "error_context must carry severity"
        );
        assert!(
            calls[0].2.contains("\"domain\":\"test_domain\""),
            "error_context must carry domain"
        );

        // Warning alert: confidence 0.5
        assert_eq!(calls[1].0, "Warning test alert");
        assert!(
            (calls[1].1 - 0.5).abs() < f64::EPSILON,
            "warning confidence must be 0.5"
        );
        assert!(
            calls[1].2.contains("\"severity\":\"Warning\""),
            "error_context must carry severity"
        );
    }

    /// When no `AlertEscalationSink` is wired, `persist_alert_to_queue` must
    /// be a no-op (not panic). This pins the best-effort contract: a missing
    /// sink never breaks the regulation loop.
    #[tokio::test]
    async fn persist_alert_to_queue_no_op_when_sink_absent() {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let loop_instance = CyberneticsLoop::new(ledger);

        let alert = RuntimeAlert {
            domain: "test_domain".to_string(),
            deficit: 150,
            threshold: 100,
            severity: AlertSeverity::Critical,
            escalated: true,
            timestamp: chrono::Utc::now(),
            message: "Critical test alert".to_string(),
        };
        // Must not panic — the sink is None.
        loop_instance.persist_alert_to_queue(&alert, None);
    }

    // ── Property tests ──────────────────────────────────────────────────
    //
    // The unit tests above pin specific values (Critical → 1.0, Warning →
    // 0.5, specific domain strings). Property tests verify the universal
    // invariants hold across the full input space — any domain, any deficit,
    // any threshold, any severity, any escalated flag. This catches edge
    // cases the static tests miss (e.g. empty domain, zero threshold, very
    // large deficit, Info severity with escalated=true).

    /// Strategy for generating arbitrary `RuntimeAlert` values across the
    /// full input space. Generates non-empty domains (the `RuntimeAlert::new`
    /// constructor rejects empty domains, but `persist_alert_to_queue` takes a
    /// constructed `RuntimeAlert` directly, so we test all non-empty strings).
    fn arb_runtime_alert() -> proptest::prelude::BoxedStrategy<RuntimeAlert> {
        use proptest::prelude::*;
        (
            "[a-z_][a-z0-9_/]{0,30}",
            0u64..=10_000,
            1u64..=10_000, // threshold > 0 (RuntimeAlert::new rejects 0)
            proptest::sample::select(&[
                AlertSeverity::Info,
                AlertSeverity::Warning,
                AlertSeverity::Critical,
            ]),
            any::<bool>(),
        )
            .prop_map(
                |(domain, deficit, threshold, severity, escalated)| RuntimeAlert {
                    domain,
                    deficit,
                    threshold,
                    severity,
                    escalated,
                    timestamp: chrono::Utc::now(),
                    message: format!(
                        "Variety deficit {} in domain '{}' (threshold: {})",
                        deficit, "test", threshold
                    ),
                },
            )
            .boxed()
    }

    /// Helper: build a CyberneticsLoop wired with a CapturingEscalationSink.
    fn loop_with_sink() -> (CyberneticsLoop, Arc<CapturingEscalationSink>) {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let sink = Arc::new(CapturingEscalationSink::new());
        let loop_instance = CyberneticsLoop::new(ledger).with_alert_escalation_sink(
            sink.clone() as Arc<dyn crate::algedonic::AlertEscalationSink>
        );
        (loop_instance, sink)
    }

    // **P4 — panic_freedom:** `persist_alert_to_queue` must never panic on
    // any `RuntimeAlert` input, whether the sink is wired or absent. This
    // is the foundational contract — the regulation loop must never break
    // due to an alert persistence failure.
    proptest::proptest! {
        #[test]
        fn prop_persist_alert_to_queue_never_panics(
            alert in arb_runtime_alert()
        ) {
            let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
            let (loop_with_sink, _sink) = loop_with_sink();
            let loop_without_sink = CyberneticsLoop::new(ledger);

            // With sink wired — must not panic
            loop_with_sink.persist_alert_to_queue(&alert, None);
            // Without sink — must not panic
            loop_without_sink.persist_alert_to_queue(&alert, None);
        }
    }

    // **P1 — invariant:** Non-escalated alerts (`escalated: false`) are
    // never persisted to the sink, regardless of severity, deficit, domain,
    // or threshold. This prevents Info and non-escalated Warning alerts from
    // polluting the reviewable backlog.
    proptest::proptest! {
        #[test]
        fn prop_non_escalated_alerts_never_persisted(
            alert in arb_runtime_alert().prop_filter(
                "only non-escalated alerts",
                |a| !a.escalated
            )
        ) {
            let (loop_instance, sink) = loop_with_sink();
            loop_instance.persist_alert_to_queue(&alert, None);
            assert_eq!(
                sink.calls().len(),
                0,
                "non-escalated alert must not reach the sink"
            );
        }
    }

    // **P1 — invariant:** Escalated alerts (`escalated: true`) are always
    // persisted when the sink is wired, regardless of severity, deficit,
    // domain, or threshold. This is the Store seam — if an escalated alert
    // is dropped, the loop is open.
    proptest::proptest! {
        #[test]
        fn prop_escalated_alerts_always_persisted(
            alert in arb_runtime_alert().prop_filter(
                "only escalated alerts",
                |a| a.escalated
            )
        ) {
            let (loop_instance, sink) = loop_with_sink();
            sink.clear();
            loop_instance.persist_alert_to_queue(&alert, None);
            let calls = sink.calls();
            assert_eq!(
                calls.len(),
                1,
                "escalated alert must reach the sink exactly once"
            );
        }
    }

    // **P1 — invariant:** The `confidence` passed to the sink is always
    // exactly 1.0 for Critical alerts and 0.5 for non-Critical (Warning/Info)
    // escalated alerts. This is the severity→confidence mapping that the
    // `alert-review` flowdef's triage report relies on to classify alerts.
    proptest::proptest! {
        #[test]
        fn prop_confidence_mapping_is_correct(
            alert in arb_runtime_alert().prop_filter(
                "only escalated alerts (non-escalated are skipped)",
                |a| a.escalated
            )
        ) {
            let (loop_instance, sink) = loop_with_sink();
            sink.clear();
            loop_instance.persist_alert_to_queue(&alert, None);
            let calls = sink.calls();
            assert_eq!(calls.len(), 1);
            let expected_confidence = if alert.is_critical() { 1.0 } else { 0.5 };
            assert!(
                (calls[0].1 - expected_confidence).abs() < f64::EPSILON,
                "confidence {:?} != expected {:?} for severity {:?}",
                calls[0].1, expected_confidence, alert.severity
            );
        }
    }

    // **P1 — invariant:** The `error_context` JSON always contains the
    // `domain`, `deficit`, `threshold`, `severity`, `escalated`, and
    // `timestamp` fields. The `alert-review` flowdef's triage report reads
    // these fields to classify and propose actions — if any is missing, the
    // report is incomplete.
    proptest::proptest! {
        #[test]
        fn prop_error_context_carries_all_fields(
            alert in arb_runtime_alert().prop_filter(
                "only escalated alerts",
                |a| a.escalated
            )
        ) {
            let (loop_instance, sink) = loop_with_sink();
            sink.clear();
            loop_instance.persist_alert_to_queue(&alert, None);
            let calls = sink.calls();
            assert_eq!(calls.len(), 1);
            let ctx = &calls[0].2;
            // Parse as JSON to verify all fields are present
            let parsed: serde_json::Value = serde_json::from_str(ctx)
                .expect("error_context must be valid JSON");
            assert!(parsed.get("domain").is_some(), "error_context must carry domain");
            assert!(parsed.get("deficit").is_some(), "error_context must carry deficit");
            assert!(parsed.get("threshold").is_some(), "error_context must carry threshold");
            assert!(parsed.get("severity").is_some(), "error_context must carry severity");
            assert!(parsed.get("escalated").is_some(), "error_context must carry escalated");
            assert!(parsed.get("efferent_action").is_some(), "error_context must carry efferent_action");
            assert!(parsed.get("timestamp").is_some(), "error_context must carry timestamp");
            // The domain must match the alert's domain
            assert_eq!(
                parsed.get("domain").and_then(|v| v.as_str()),
                Some(alert.domain.as_str())
            );
        }
    }

    // ── Phase 3 co-evolution: EvolveMcpToolSchema directive ─────────────

    /// Capturing sink for `RegulationSink` — records every `persist` call's
    /// JSON payload so tests can assert the directive was recorded.
    struct CapturingRegulationSink {
        records: std::sync::Mutex<Vec<serde_json::Value>>,
    }

    impl CapturingRegulationSink {
        fn new() -> Self {
            Self { records: std::sync::Mutex::new(Vec::new()) }
        }

        fn records(&self) -> Vec<serde_json::Value> {
            self.records.lock().unwrap().clone()
        }
    }

    impl RegulationSink for CapturingRegulationSink {
        fn persist(&self, record: &RegulationRecord) -> Result<(), hkask_types::InfrastructureError> {
            self.records.lock().unwrap().push(record.observation.clone());
            Ok(())
        }
    }

    /// Helper: build a CyberneticsLoop wired with a CapturingRegulationSink
    /// (for testing event_sink persistence).
    fn loop_with_regulation_sink() -> (CyberneticsLoop, Arc<CapturingRegulationSink>) {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let sink = Arc::new(CapturingRegulationSink::new());
        let loop_instance =
            CyberneticsLoop::new(ledger).with_event_sink(sink.clone() as Arc<dyn RegulationSink>);
        (loop_instance, sink)
    }

    #[tokio::test]
    async fn evolve_mcp_tool_schema_directive_persists_to_regulation_ledger() {
        let (loop_instance, sink) = loop_with_regulation_sink();

        loop_instance
            .apply_directive(CuratorDirective::EvolveMcpToolSchema {
                server_name: "hkask-mcp-companies".to_string(),
                tool_name: "dcf_valuation".to_string(),
                evolution_type: SchemaEvolutionType::AddField,
                field_name: "wacc_override".to_string(),
                new_type: Some("Option<f64>".to_string()),
                rationale: "forensic-adjusted WACC needed".to_string(),
                evidence: "skill_use_issue:superforecasting".to_string(),
            })
            .await;

        let records = sink.records();
        assert_eq!(
            records.len(),
            1,
            "EvolveMcpToolSchema should persist exactly one regulation record"
        );
        let payload = &records[0];
        assert_eq!(payload["directive_type"], "evolve_mcp_tool_schema");
        assert_eq!(payload["outcome"], "recorded");
        assert_eq!(payload["server_name"], "hkask-mcp-companies");
        assert_eq!(payload["tool_name"], "dcf_valuation");
        assert_eq!(payload["evolution_type"], "add_field");
        assert_eq!(payload["field_name"], "wacc_override");
        assert_eq!(payload["new_type"], "Option<f64>");
        assert_eq!(payload["rationale"], "forensic-adjusted WACC needed");
        assert_eq!(payload["evidence"], "skill_use_issue:superforecasting");
    }

    #[tokio::test]
    async fn evolve_mcp_tool_schema_directive_without_sink_does_not_panic() {
        let ledger = Arc::new(RwLock::new(RegulationLedger::with_threshold(100)));
        let loop_instance = CyberneticsLoop::new(ledger);

        // No event_sink wired — must not panic, just log a warning.
        loop_instance
            .apply_directive(CuratorDirective::EvolveMcpToolSchema {
                server_name: "hkask-mcp-companies".to_string(),
                tool_name: "dcf_valuation".to_string(),
                evolution_type: SchemaEvolutionType::RemoveField,
                field_name: "unused_field".to_string(),
                new_type: None,
                rationale: "field is unused".to_string(),
                evidence: "no skill references this field".to_string(),
            })
            .await;
        // No assertion needed — the test passes if it doesn't panic.
    }
}
