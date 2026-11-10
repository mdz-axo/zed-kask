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

use crate::dampener::Dampener;

use crate::energy::{CallCapManager, CallMeterOutcome};
use crate::sensor_provider::{SensorBus, VarietySensor};

use crate::runtime::RegulationLedger;
use crate::set_points::SetPoints;

use crate::loops::{CurationInput, LoopMetrics, TriggerOrigin};

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use hkask_types::event::{RegulationSink, SpanKind};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::{RwLock, mpsc};

#[derive(Default)]
struct LoopTelemetryState {
    steady_fingerprint: Option<serde_json::Value>,

    suppressed_cycles: usize,
}

struct LoopTelemetryDecision {
    emit: bool,
    idle_heartbeat: bool,
    steady_state_heartbeat: bool,
    condition_cleared: bool,
    suppressed_cycles: usize,
}

/// Log-surface condition coalescing — the degradation warn arms' counterpart
/// of [`LoopTelemetryState`] (§9.4): which deviated conditions have already
/// warned, keyed by metric with their `(value_bits, set_point_bits)`
/// fingerprint. Observed motive (2026-10-31): a frozen native-first reading
/// (7 of 11 substantive turns, byte-identical for six minutes while the
/// window sat unchanged between turns) re-warned on every 10-second tick —
/// the escalation path dedups onto the open board card and the telemetry
/// path coalesces by fingerprint, but the `tracing` warn surface had no
/// transition discipline.
#[derive(Default)]
struct ConditionLogState {
    warned: HashMap<crate::loops::SignalMetric, (u64, u64)>,
}

impl ConditionLogState {
    /// Record a condition-warn decision: `true` when this call is a
    /// transition (first occurrence or changed reading) and the warn
    /// should emit; `false` when the condition persists unchanged — its
    /// steady state is carried by the board's open condition card and the
    /// telemetry heartbeat, not by per-tick log lines.
    fn note_condition(&mut self, signal: &crate::loops::Signal) -> bool {
        let fingerprint = (signal.value.to_bits(), signal.set_point.to_bits());
        match self.warned.insert(signal.metric, fingerprint) {
            Some(previous) if previous == fingerprint => false,
            _ => true,
        }
    }

    /// Drop conditions that left deviation so a recurrence warns again,
    /// returning the cleared metrics for the clearing announcement.
    fn clear_absent(
        &mut self,
        deviations: &[crate::loops::Deviation],
    ) -> Vec<crate::loops::SignalMetric> {
        let deviated: HashSet<crate::loops::SignalMetric> = deviations
            .iter()
            .map(|deviation| deviation.signal.metric)
            .collect();
        let mut cleared = Vec::new();
        self.warned.retain(|metric, _| {
            if deviated.contains(metric) {
                true
            } else {
                cleared.push(*metric);
                false
            }
        });
        cleared
    }
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
    /// When present, delivers algedonic alerts to the reviewable kanban board
    /// via the governed tool channel. The `event_sink` retains regulation
    /// history independently if board delivery fails.
    alert_escalation_sink: Option<Arc<dyn crate::algedonic::AlertEscalationSink>>,
    /// Direct alerts channel: Cybernetics → Curation (CurationInput).
    alerts_tx: Option<mpsc::UnboundedSender<CurationInput>>,
    /// Direct tool consumption channel: McpRuntime::invoke → Cybernetics.
    /// Direct curator directive channel: Curation → Cybernetics.
    curator_directive_rx: Option<Arc<RwLock<mpsc::UnboundedReceiver<CuratorDirective>>>>,
    /// Loop-quality telemetry from the most recent tick cycle.
    loop_quality: RwLock<LoopMetrics>,
    /// Coalesces only semantically identical persistent signal telemetry.
    loop_telemetry_state: Mutex<LoopTelemetryState>,
    /// Log-surface condition coalescing for the degradation warn arms —
    /// see [`ConditionLogState`] (the §9.4 transition discipline applied to
    /// the warn surface: entry and changed readings emit, identical
    /// persistent cycles are coalesced, clearing re-arms).
    condition_log_state: Mutex<ConditionLogState>,
    /// Conditions already archived while board delivery was unavailable.
    fallback_alert_conditions: Mutex<HashSet<String>>,
    /// Pluggable metric sensors (Fermi Extractor pattern).
    sensor_registry: Arc<SensorBus>,
    observations: parking_lot::Mutex<HashMap<crate::loops::SignalMetric, crate::loops::Signal>>,
    /// Optional context-server health source retained so advisory construction
    /// can carry the current fleet counts as quantitative trigger evidence.
    /// The sensor registry holds a `ContextServerHealthSensor` wrapping the
    /// same source for the sense phase.
    context_server_health_source:
        Option<Arc<dyn crate::sensor_provider::ContextServerHealthSource>>,
    /// Optional experimentation health source retained so alert delivery can
    /// re-read the stuck-experiment ids for the board card's context (the
    /// count in the message cannot name WHICH experiments to unstick).
    /// The sensor registry holds an `ExperimentationHealthSensor` wrapping the
    /// same source for the sense phase.
    experimentation_health_source:
        Option<Arc<dyn crate::sensor_provider::ExperimentationHealthSource>>,
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
        let call_cap_manager = Arc::new(RwLock::new(CallCapManager::new()));
        let sensor_registry = {
            let registry = SensorBus::new();

            registry.register(Arc::new(VarietySensor::new(
                Arc::clone(&ledger),
                set_points.variety_max_deficit,
            )));
            registry.register(Arc::new(
                crate::sensor_provider::ToolReliabilitySensor::new(
                    Arc::clone(&ledger),
                    set_points.tool_reliability_threshold,
                ),
            ));
            registry.register(Arc::new(
                crate::sensor_provider::NativeFirstRateSensor::new(
                    Arc::clone(&ledger),
                    set_points.native_first_rate_threshold,
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
            experimentation_health_source: None,
            inference_intervention_cursor: std::sync::atomic::AtomicU64::new(0),
            tick_count: std::sync::atomic::AtomicUsize::new(0),
            dampener,
            event_sink: None,
            alert_escalation_sink: None,
            alerts_tx: None,
            curator_directive_rx: None,
            loop_quality: RwLock::new(LoopMetrics::default()),
            loop_telemetry_state: Mutex::new(LoopTelemetryState::default()),
            condition_log_state: Mutex::new(ConditionLogState::default()),
            fallback_alert_conditions: Mutex::new(HashSet::new()),
            sensor_registry,
            observations: parking_lot::Mutex::new(HashMap::new()),
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

    /// Set or clear the alert escalation sink after construction.
    ///
    /// The composition root installs the board connector after constructing
    /// its governed tool invoker. Pass `None` to disable board delivery.
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

    /// Wire an experimentation health source after construction (§P8.9 step 1 —
    /// the Layer-A afferent pathway for the experimentation program).
    ///
    /// Used by the composition root to wire the sensor over the evolution
    /// registry's health snapshot. Without this, the loop is blind to stuck
    /// experiments: a running experiment unresolved past the stale set point
    /// (D-3: 7 days) or with its whole declared budget recorded but no
    /// verdict produces no signal — the blind-feedback-loop trap.
    pub fn set_experimentation_health_source(
        &mut self,
        source: Arc<dyn crate::sensor_provider::ExperimentationHealthSource>,
    ) {
        // Retain the source for the alert-delivery path (the stuck ids in
        // the board card's context); the sensor takes its own Arc.
        self.experimentation_health_source = Some(Arc::clone(&source));
        self.sensor_registry.replace(
            crate::loops::SignalMetric::ExperimentationStuckExperiments,
            Arc::new(crate::sensor_provider::ExperimentationHealthSensor::new(
                source,
                crate::sensor_provider::DEFAULT_EXPERIMENTATION_STALE_DAYS,
            )),
        );
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
    /// [`Self::charge_call_metered`] — see [`CallCapManager::charge_metered`].
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
    /// §9.4's transition discipline on the log surface: the degradation warn
    /// arms in `build_regulation_action` (tool reliability, native-first rate)
    /// emit on entry and on a changed (value, set-point) reading; an unchanged
    /// persistent condition is coalesced — the
    /// board's open condition card and the telemetry heartbeat carry its
    /// steady state, not per-tick log lines.
    fn condition_warn_transition(&self, signal: &crate::loops::Signal) -> bool {
        self.condition_log_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .note_condition(signal)
    }

    /// The clearing leg: after the cycle's actions are built, a warned
    /// condition that left deviation announces once and re-arms its latch
    /// so a recurrence warns again.
    fn report_cleared_conditions(&self, deviations: &[crate::loops::Deviation]) {
        for metric in self
            .condition_log_state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear_absent(deviations)
        {
            tracing::info!(
                target: "reg.cybernetics",
                metric = metric.as_str(),
                "Regulation condition cleared — back within set point"
            );
        }
    }

    fn loop_telemetry_decision(
        &self,
        deviations: &[crate::loops::Deviation],
        actions: &[crate::loops::RegulatoryAction],
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
                    && (tick_number == 1 || tick_number.is_multiple_of(HEARTBEAT_INTERVAL_TICKS));
                LoopTelemetryDecision {
                    emit: condition_cleared || idle_heartbeat,
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
    /// Measures elapsed time and computes loop-quality metrics after each
    /// cycle. Computed advisories are routed for operator action.
    pub async fn tick(&self) {
        let start = std::time::Instant::now();
        self.tick_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let signals = self.sense().await;
        *self.observations.lock() = signals
            .iter()
            .map(|signal| (signal.metric, signal.clone()))
            .collect();
        if let Some(sink) = &self.alert_escalation_sink {
            if let Err(error) = sink.reconcile_conditions(&signals).await {
                tracing::warn!(target: "reg.alert", %error, "Alert condition reconciliation failed");
            }
        }
        let deviations = self.compare(&signals).await;
        let actions = self.compute(&deviations).await;
        self.act(&actions).await;
        // §9.4's clearing leg on the log surface: a warned condition that
        // left deviation announces once and re-arms its warn latch. Runs
        // after compute/act so this tick's own entries are never cleared.
        self.report_cleared_conditions(&deviations);

        // Feed regulation health into Regulation for metacognition
        // observability: the per-cycle counter behind the Loop Cycles
        // status, plus the cycle's deviation/disposition outcome — the
        // live demand signal the LoopView assembly reads to distinguish
        // healthy homeostasis (demand met) from a live defect (deviations
        // with no typed disposition).
        let undispositioned = deviations
            .iter()
            .filter(|dev| {
                !actions
                    .iter()
                    .any(|action| action.metric_name.as_deref() == Some(dev.signal.metric.as_str()))
            })
            .count() as u64;
        self.ledger
            .read()
            .await
            .record_cycle(deviations.len() as u64, undispositioned)
            .await;

        let elapsed_ms = start.elapsed().as_millis() as u64;

        let quality =
            LoopMetrics::from_cycle(elapsed_ms, &deviations, &actions, TriggerOrigin::Scheduled);
        *self.loop_quality.write().await = quality;

        // Coalesce only semantically identical persistent deviation/advisory
        // cycles. Changed values and clearing always emit. Exact repeats
        // accumulate until
        // the existing hourly tick boundary re-announces liveness and reports
        // how many records were suppressed.
        let tick_number = self.tick_count.load(std::sync::atomic::Ordering::Relaxed);

        let decision = self.loop_telemetry_decision(&deviations, &actions, tick_number);
        if decision.emit {
            let mut observation = serde_json::json!({
                "delay_ms": quality.delay_ms,
                "response_coverage": quality.response_coverage,
                "fidelity_score": quality.fidelity_score,

                "trigger": format!("{:?}", quality.trigger),
                "deviations": deviations.len(),
                "advisories_computed": actions.len(),
                // Name the deviating metric(s), not just count them — a
                // count-only row left the operator unable to see WHICH
                // signal deviated (observed live through 2026-11-02: the
                // weeks-long `deviations: 1` signature was identifiable
                // only by board-card archaeology). Always present; empty
                // is the explicit zero-deviation reading.
                "deviating_metrics": deviations
                    .iter()
                    .map(|dev| {
                        serde_json::json!({
                            "metric": dev.signal.metric.as_str(),
                            "value": dev.signal.value,
                            "set_point": dev.signal.set_point,
                            "direction": format!("{:?}", dev.direction),
                        })
                    })
                    .collect::<Vec<_>>(),

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

/// Test support shared by the cycle and directive test modules — the
/// CapturingSink both previously hand-copied (the duplication ratchet's
/// cross-file pair).
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::Mutex;

    /// Capturing RegulationSink — records every persisted span's path and
    /// observation so tick and acknowledgment policies can be asserted
    /// without a durable archive.
    pub(crate) struct CapturingSink(pub(crate) Mutex<Vec<(String, serde_json::Value)>>);

    impl hkask_types::RegulationSink for CapturingSink {
        fn persist(
            &self,
            event: &hkask_types::RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((event.span.path.clone(), event.observation.clone()));
            Ok(())
        }
    }
}
