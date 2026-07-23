//! MetacognitionLoop struct definition and core methods.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ports::EscalationEntry;
use hkask_regulation::meta_span::{
    CalibrationSpan, emit_meta_circuit_breaker, emit_meta_escalation, emit_meta_self_calibration,
};
use hkask_regulation::types::loops::{
    ActionType, Deviation, DeviationDirection, Loop, LoopId, RegulationData, RegulatoryAction,
    RegulatoryActionParams, SignalMetric,
};
use hkask_types::WebID;
use hkask_types::curator::CuratorDirective;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::a2a::A2AMessage;
use crate::curation::context::CuratorContext;
use crate::curator_agent::cat;
use crate::pod::CommunicationPosture;

use super::config::{HealthSnapshot, MC_TARGET, MetacognitionConfig};
use super::escalation::{DEFAULT_ESCALATION_VARIETY_DEFICIT, EscalationPolicy};

/// A calibration that has been applied but whose effectiveness-after
/// measurement is still pending. Stored in `pending_calibration` and
/// closed out on the next `self_calibrate` call with the current
/// effectiveness as `eff_after`.
#[derive(Debug, Clone, Copy)]
pub(super) struct PendingCalibration {
    /// Threshold value before the calibration was applied.
    pub(super) threshold_before: u64,
    /// Threshold value after the calibration was applied.
    pub(super) threshold_after: u64,
    /// Whether this calibration raised the threshold (vs. lowered).
    pub(super) raised: bool,
    /// Escalation drop count at the time the calibration was applied — the
    /// PRIMARY causal signal (lever-controlled). The close-out measures the
    /// delta: did the threshold change reduce drops?
    pub(super) dropped_before: u64,
    /// Regulation effectiveness at apply time — SECONDARY signal from a
    /// different loop. Retained for offline GEPA, not used for runtime judgment.
    pub(super) eff_before: f64,
    /// Decision source: "generative" (LLM template) or "fallback" (Rust rail).
    pub(super) source: &'static str,
}

/// Metacognition loop — Curator Agent's system governance mechanism.
pub struct MetacognitionLoop {
    pub(super) context: Arc<CuratorContext>,
    pub(super) config: MetacognitionConfig,
    pub(super) escalation_policy: EscalationPolicy,
    pub(super) last_snapshot_tx: tokio::sync::watch::Sender<Option<HealthSnapshot>>,
    /// Template output from the most recent template-driven compute cycle.
    /// Stored separately from HealthSnapshot to avoid race conditions —
    /// `sense()` wipes the snapshot each cycle but template output must
    /// survive across cycles for `generate_summary()` and `act()`.
    pub(super) last_template_output: RwLock<Option<serde_json::Value>>,
    /// Circuit breaker: consecutive template invocation failures.
    /// After 3 consecutive failures, skip template for 5 cycles.
    pub(super) consecutive_template_failures: std::sync::atomic::AtomicU64,
    pub(super) template_skip_remaining: std::sync::atomic::AtomicU64,
    pub(super) last_cal_dropped: std::sync::atomic::AtomicU64,
    pub(super) last_cal_directives: std::sync::atomic::AtomicU64,
    /// Cycles elapsed since the last threshold RAISE — gates lowering so a
    /// raise is not immediately reversed (anti-oscillation hysteresis).
    pub(super) calibrations_since_raise: std::sync::atomic::AtomicU64,
    /// The most recent applied calibration awaiting an effectiveness-after
    /// measurement. Closed out (emitted with eff_after) on the next
    /// self_calibrate call - the causal record for future learned adjustment.
    pub(super) pending_calibration: std::sync::Mutex<Option<PendingCalibration>>,
    /// Communication posture — loaded from the agent's persona.
    /// Governs speak/silent decisions and accommodation level.
    pub(super) communication_posture: CommunicationPosture,
    /// Agent name used in template compute and communication dispatch.
    /// Defaults to "curator" for backward compatibility; derived from
    /// `CuratorContext::handle().curator_id()` when constructed via
    /// `CuratorAgent`.
    agent_name: String,
}

impl MetacognitionLoop {
    /// Create a new metacognition loop without a BotHealthEvaluator.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — MetacognitionLoop monitors agent health
    /// pre:  `context` is a valid `Arc<CuratorContext>`; `config` is a
    ///       valid `MetacognitionConfig`.
    /// post: Returns a `MetacognitionLoop` with an `EscalationPolicy`
    ///       derived from `config.thresholds`, empty bot reports, and a
    ///       fresh watch channel for health snapshots.
    pub fn new(context: Arc<CuratorContext>, config: MetacognitionConfig) -> Self {
        Self::with_posture(
            context,
            config,
            CommunicationPosture::default(),
            "curator".to_string(),
        )
    }

    /// Create a new metacognition loop with a specific communication posture.
    ///
    /// This is the canonical constructor — `new()` delegates to it with defaults.
    pub fn with_posture(
        context: Arc<CuratorContext>,
        config: MetacognitionConfig,
        posture: CommunicationPosture,
        agent_name: String,
    ) -> Self {
        let escalation_policy = EscalationPolicy::new(config.thresholds.clone());
        let (last_snapshot_tx, _) = tokio::sync::watch::channel(None);
        Self {
            context,
            escalation_policy,
            config,
            last_snapshot_tx,
            last_template_output: RwLock::new(None),
            consecutive_template_failures: std::sync::atomic::AtomicU64::new(0),
            template_skip_remaining: std::sync::atomic::AtomicU64::new(0),
            last_cal_dropped: std::sync::atomic::AtomicU64::new(0),
            last_cal_directives: std::sync::atomic::AtomicU64::new(0),
            calibrations_since_raise: std::sync::atomic::AtomicU64::new(0),
            pending_calibration: std::sync::Mutex::new(None),
            communication_posture: posture,
            agent_name,
        }
    }

    /// Create a new metacognition loop with a BotHealthEvaluator.
    ///
    /// The evaluator reads gas data from the Regulation runtime and populates
    /// bot health reports at each cycle.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — classify bot energy health for Curator
    /// \[P4\] Constraining: Clear Boundaries — thresholds map consumption ratio to status
    /// pre:  `context` is a valid `Arc<CuratorContext>`; `config` is a
    ///       valid `MetacognitionConfig`; `evaluator` is a valid
    ///       `Arc<BotHealthEvaluator>`.
    /// post: Returns a `MetacognitionLoop` with the evaluator wired in.
    /// Access the metacognition configuration.
    pub fn config(&self) -> &MetacognitionConfig {
        &self.config
    }

    /// Return the agent name used in template compute and communication dispatch.
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Set the agent name for template compute and communication dispatch.
    ///
    /// Defaults to `"curator"` when constructed via [`new`](Self::new).
    /// Set explicitly when a persona-derived name is available.
    pub fn with_agent_name(mut self, name: String) -> Self {
        self.agent_name = name;
        self
    }

    /// Run a full cycle, returning the health snapshot.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — tick produces latest HealthSnapshot
    /// pre:  The loop has been registered and ticked at least once.
    /// post: On success, returns `Ok(HealthSnapshot)` — the latest
    ///       snapshot from the watch channel. If no snapshot has been
    ///       produced yet, returns `Err(CoreError::NoSnapshot)`.
    pub async fn run_cycle(&self) -> Result<HealthSnapshot, crate::error::CoreError> {
        info!(target: MC_TARGET, "Starting metacognition cycle");
        let signals = self.sense().await;
        let deviations = self.compare(&signals).await;
        let actions = self.compute(&deviations).await;
        self.act(&actions).await;
        self.last_snapshot_tx
            .borrow()
            .clone()
            .ok_or(crate::error::CoreError::NoSnapshot)
    }
    /// Generate a system state summary for posting to standing session.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — summary posts system state to standing session
    /// pre:  `snapshot` is a valid `&HealthSnapshot`.
    /// post: Returns a `String` containing a markdown-formatted summary
    ///       with timestamp, Regulation health, critical/total alerts, variety
    ///       counters, and bot status reports.
    pub fn generate_summary(&self, snapshot: &HealthSnapshot) -> String {
        use std::fmt::Write;
        if let Some(ref output) = *self.last_template_output.blocking_read() {
            let mut s = String::new();
            let _ = writeln!(s, "## Metacognition Update (LLM)");
            let _ = writeln!(
                s,
                "**Timestamp:** {}",
                snapshot.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
            );
            if let Some(diag) = output.get("diagnosis").and_then(|v| v.as_str()) {
                let _ = writeln!(s, "**Diagnosis:** {}", diag);
            }
            if let Some(plan) = output.get("remediation_plan").and_then(|v| v.as_array()) {
                for step in plan {
                    let action = step.get("action").and_then(|v| v.as_str()).unwrap_or("?");
                    let target = step
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("system");
                    let _ = writeln!(s, "- {} -> {}", action, target);
                }
            }
            return s;
        }

        let mut s = String::new();
        let _ = writeln!(s, "## Metacognition Update\n");
        let _ = writeln!(
            s,
            "**Timestamp:** {}",
            snapshot.timestamp.format("%Y-%m-%d %H:%M:%S UTC")
        );
        let _ = writeln!(s, "**Regulation Health:** {}", snapshot.reg_health);
        let _ = writeln!(s, "**Variety Deficit:** {}", snapshot.variety_deficit);
        let _ = writeln!(s, "**Critical Alerts:** {}", snapshot.critical_alerts);
        let _ = writeln!(s, "**Total Alerts:** {}\n", snapshot.total_alerts);
        if !snapshot.variety_counters.is_empty() {
            let _ = writeln!(s, "### Variety Counters");
            for (ns, variety) in &snapshot.variety_counters {
                let _ = writeln!(s, "- {}: {}", ns.as_str(), variety);
            }
            s.push('\n');
        }
        s
    }

    // Curator metacognition: evaluate, coach, direct

    /// Direct a bot to take action via A2A message.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — direct a bot to take corrective action
    /// pre:  `bot_name` is a non-empty string; `reason` is a non-empty
    ///       string; `self.context.a2a()` may be `Some` or `None`.
    /// post: If A2A is configured, sends a `TemplateDispatch` directive
    ///       to the bot and returns `Ok(())`. If A2A is not configured,
    ///       logs a warning and returns `Ok(())` (graceful degradation).
    ///       Returns `Err` on A2A send failure.
    pub async fn direct_bot(
        &self,
        bot_name: &str,
        reason: &str,
    ) -> Result<(), crate::error::CoreError> {
        let a2a = match self.context.a2a() {
            Some(a2a) => a2a,
            None => {
                warn!(
                    target: MC_TARGET,
                    bot = %bot_name,
                    "A2A port not configured — cannot direct bot"
                );
                return Ok(());
            }
        };

        let from = *self.context.handle().curator_id();
        let to = WebID::from_persona(bot_name.as_bytes());
        let correlation_id = format!("directive-{}-{}", bot_name, chrono::Utc::now().timestamp());

        let msg = A2AMessage::TemplateDispatch {
            from,
            to: Some(to),
            template_id: "directive".to_string(),
            input: serde_json::json!({ "reason": reason }),
            correlation_id,
        };

        a2a.send_message(msg).await?;

        info!(
            target: MC_TARGET,
            bot = %bot_name,
            reason = %reason,
            "Directive sent to bot via A2A"
        );

        Ok(())
    }

    /// Issue a CuratorDirective on the direct channel with DAMPEN filtering.
    /// Delegates to `CuratorContext::issue_directive()`.
    ///
    /// expect: "The system regulates agent behavior through cybernetic feedback"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — delegate directive to CuratorContext
    /// pre:  `directive` is a valid `CuratorDirective`.
    /// post: Delegates to `self.context.issue_directive(directive)`;
    ///       same post-conditions as `CuratorContext::issue_directive`.
    pub async fn issue_directive(&self, directive: CuratorDirective) {
        self.context.issue_directive(directive).await;
    }

    pub(super) async fn act_on_throttle(
        &self,
        action: &RegulatoryAction,
    ) -> Option<EscalationEntry> {
        // Source data from the HealthSnapshot built in sense(), not from the
        // action's typed RegulationData (which is NoData for metacognition actions).
        match action.parameters.reason.as_str() {
            "variety_deficit" | "calibrate" | "adjust_threshold" => {
                let deficit = self
                    .last_snapshot_tx
                    .borrow()
                    .as_ref()
                    .map(|s| s.variety_deficit)
                    .unwrap_or(0);
                let new_threshold = self.config.thresholds.variety_deficit;
                let directive = CuratorDirective::CalibrateThreshold {
                    domain: "variety".to_string(),
                    new_threshold,
                };
                self.issue_directive(directive).await;

                let error_context = format!(
                    "Total variety deficit ({}) exceeds threshold ({})",
                    deficit, new_threshold
                );
                Some(EscalationEntry::pending(
                    format!("Variety deficit: {}", deficit),
                    0.6,
                    error_context,
                ))
            }
            _ => None,
        }
    }

    /// Handle an Escalate action: route to the appropriate escalation
    /// handler based on the metric (critical_alerts, bot_failures, or unknown).
    ///
    /// Returns the escalation entry for the caller to write (either
    /// individually or as part of a batch).
    // NOTE: EscalationQueue is a Curation-owned durable queue. Direct writes
    // are intentional — it is an exception to the dispatch-only rule per the
    // authority DAG: Curation (L5) owns the escalation queue as its algedonic
    // regulation mechanism. This does NOT bypass the Communication Loop because
    // the queue is not a loop-to-loop message channel.
    pub(super) async fn act_on_escalate(
        &self,
        action: &RegulatoryAction,
    ) -> Option<EscalationEntry> {
        let reason = action.parameters.reason.as_str();
        match reason {
            "critical_alerts" => {
                let count = self
                    .last_snapshot_tx
                    .borrow()
                    .as_ref()
                    .map(|s| s.critical_alerts)
                    .unwrap_or(0);
                warn!(
                    target: MC_TARGET,
                    critical_alerts = count,
                    threshold = self.config.thresholds.critical_alerts,
                    "Critical alert count exceeds threshold"
                );
                Some(EscalationEntry::pending(
                    format!("System has {} critical alerts", count),
                    0.3,
                    format!(
                        "Critical alert count ({}) exceeds threshold ({})",
                        count, self.config.thresholds.critical_alerts
                    ),
                ))
            }
            // No bot-health subsystem exists (sense passes bot_failures: 0),
            // so no producer emits this reason and no data source is available.
            "bot_failures" => {
                warn!(
                    target: MC_TARGET,
                    "bot_failures escalation requested but no bot-health subsystem is wired"
                );
                None
            }
            "restart" | "rebalance" | "escalate" => {
                // Template-directed: diagnosis lives in last_template_output,
                // not in the action (which carries only the reason string).
                let diagnosis = {
                    let guard = self.last_template_output.read().await;
                    guard
                        .as_ref()
                        .and_then(|o| o.get("diagnosis").and_then(|v| v.as_str()))
                        .unwrap_or("template-diagnosed")
                        .to_string()
                };
                warn!(target: MC_TARGET, metric = %reason, diagnosis = %diagnosis, "Template-directed bot action");
                Some(EscalationEntry::pending(
                    format!("{} ({})", reason, diagnosis),
                    0.7,
                    format!("Template directed {}: {}", reason, diagnosis),
                ))
            }
            _ => {
                warn!(target: MC_TARGET, metric = %reason, "Unknown escalation metric");
                None
            }
        }
    }

    /// Log an unhandled action type (no-op).
    pub(super) fn act_on_no_action(&self, action: &RegulatoryAction) {
        info!(
            target: MC_TARGET,
            action_type = ?action.action_type,
            "Unhandled action type in MetacognitionLoop act()"
        );
    }

    /// Handle an `OverrideEnergyBudget` action: extract the typed
    /// `CuratorBudgetOverride` data and issue a `CuratorDirective`.
    ///
    /// This closes the gap where the LLM's `adjust_budget` remediation step
    /// was silently dropped (the action carried only a reason string).
    pub(super) async fn act_on_budget_override(&self, action: &RegulatoryAction) {
        if let RegulationData::CuratorBudgetOverride { agent, new_budget } = &action.parameters.data
        {
            let directive = CuratorDirective::OverrideEnergyBudget {
                agent: WebID::from_persona(agent.as_bytes()),
                new_budget: *new_budget,
            };
            self.issue_directive(directive).await;
            info!(
                target: MC_TARGET,
                agent = %agent,
                new_budget,
                "Issued OverrideEnergyBudget from template directive"
            );
        } else {
            warn!(
                target: MC_TARGET,
                "OverrideEnergyBudget action carried no CuratorBudgetOverride data — ignored"
            );
        }
    }

    // Explicit 4-stage cycle: sense → compare → compute → act
    // Delegation methods removed — RegulationLoop trait impl provides tick().

    /// Self-calibration: the Curator observes its OWN decision quality and
    /// adjusts its escalation sensitivity. This is the meta-cybernetic loop
    /// (Meta -> Curation -> Cybernetics), kept non-circular by reading in-process
    /// SelfQuality counters (not reg.* algedonic events, which CurationLoop
    /// reads for the system).
    ///
    /// Policy: generative-first. When a ManifestExecutor is wired, the Curator
    /// generates its own threshold adjustment via the
    /// curator/metacognition-self-calibrate template from self-quality evidence
    /// and the last calibration effectiveness delta - the Curator acting as its
    /// own generative entity. The Rust compute_threshold_adjustment is the
    /// safety-rail fallback (no executor, or template failure). The model answer
    /// is clamped to the bounded band; a raise resets the hysteresis cooldown.
    /// reg.meta.self_calibration records the decision source and before/after
    /// effectiveness so generative-vs-fallback quality can be compared.
    pub(super) async fn self_calibrate(&self) {
        let sq = self.context.self_quality().snapshot();
        let prev_dropped = self
            .last_cal_dropped
            .swap(sq.escalations_dropped, std::sync::atomic::Ordering::Relaxed);
        let prev_directives = self
            .last_cal_directives
            .swap(sq.directives_issued, std::sync::atomic::Ordering::Relaxed);
        let delta_dropped = sq.escalations_dropped.saturating_sub(prev_dropped);
        let delta_directives = sq.directives_issued.saturating_sub(prev_directives);
        let since_raise = self
            .calibrations_since_raise
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let effectiveness = self
            .last_snapshot_tx
            .borrow()
            .as_ref()
            .map(|s| s.regulation_effectiveness)
            .unwrap_or(0.0);
        let old = self.escalation_policy.thresholds().variety_deficit;

        // Close out the previous calibration (its eff_after = now) and capture
        // it as the causal evidence for the generative decision.
        let prev_pending = self
            .pending_calibration
            .lock()
            .expect("pending_calibration lock poisoned")
            .take();
        let last_calibration = prev_pending.map(|p| {
            let drop_delta = sq.escalations_dropped.saturating_sub(p.dropped_before);
            info!(
                target: MC_TARGET,
                source = p.source,
                drop_delta,
                eff_delta = effectiveness - p.eff_before,
                threshold_before = p.threshold_before,
                threshold_after = p.threshold_after,
                "Closed out pending calibration"
            );
            serde_json::json!({
                "threshold_before": p.threshold_before,
                "threshold_after": p.threshold_after,
                "direction": if p.raised { "raise" } else { "lower" },
                "drop_before": p.dropped_before,
                "drop_after": sq.escalations_dropped,
                "drop_delta": drop_delta,
                "eff_before": p.eff_before,
                "eff_after": effectiveness,
                "eff_delta": effectiveness - p.eff_before,
            })
        });

        // Decide: generative-first, Rust safety-rail fallback.
        let Some((proposed, source)) = self
            .decide_self_calibration(
                delta_dropped,
                delta_directives,
                effectiveness,
                old,
                since_raise,
                last_calibration.as_ref(),
            )
            .await
        else {
            return;
        };
        // Hard safety rail: clamp to the bounded band regardless of source.
        let new = proposed.clamp(DEFAULT_ESCALATION_VARIETY_DEFICIT, VARIETY_DEFICIT_CEILING);
        if new == old {
            return; // hold / no-op
        }
        let raised = new > old;

        let mut next = self.escalation_policy.thresholds();
        next.variety_deficit = new;
        self.escalation_policy.set_thresholds(next);
        if raised {
            self.calibrations_since_raise
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
        *self
            .pending_calibration
            .lock()
            .expect("pending_calibration lock poisoned") = Some(PendingCalibration {
            threshold_before: old,
            threshold_after: new,
            raised,
            dropped_before: sq.escalations_dropped,
            eff_before: effectiveness,
            source,
        });
        if let Some(sink) = self.context.regulation_sink() {
            emit_meta_self_calibration(
                sink.as_ref(),
                self.context.handle().curator_id(),
                &CalibrationSpan {
                    metric: "variety_deficit",
                    old,
                    new,
                    signal_before: Some(sq.escalations_dropped as f64),
                    signal_after: None,
                    eff_before: Some(effectiveness),
                    eff_after: None,
                    source,
                },
            );
        }
        tracing::info!(
            target: MC_TARGET,
            old,
            new,
            raised,
            source,
            delta_dropped,
            effectiveness,
            "Self-calibration applied to variety_deficit threshold"
        );
    }

    /// Route low-confidence escalations to skill-router for epistemic guidance.
    ///
    /// Consumes the `epistemic_route` signals collected in `act()` and invokes
    /// the skill-router template to find certainty-finding skills. Emits a
    /// `reg.meta.escalation` span with outcome "epistemic_routed" per signal,
    /// closing the loop from detection to action. Gracefully degrades when the
    /// skill catalog or manifest executor is not available (standalone CLI).
    pub(super) async fn route_epistemic_escalations(&self, signals: &[(f64, String)]) {
        if signals.is_empty() {
            return;
        }
        let catalog = match self.context.skill_catalog().await {
            Some(c) => c,
            None => return,
        };
        let executor = match self.context.manifest_executor().await {
            Some(e) => e,
            None => return,
        };
        let catalog_json: Vec<serde_json::Value> = catalog
            .iter()
            .map(|e| {
                serde_json::json!({
                    "name": e.name,
                    "description": e.description,
                    "template_type": e.template_type.as_str(),
                    "lexicon_terms": e.lexicon_terms,
                    "when_to_use": e.description,
                })
            })
            .collect();
        for (confidence, output) in signals {
            let mut ctx = HashMap::new();
            ctx.insert("task_description".to_string(), serde_json::json!(output));
            ctx.insert(
                "task_context".to_string(),
                serde_json::json!("Low-confidence escalation from Curator metacognition loop"),
            );
            ctx.insert(
                "skill_catalog".to_string(),
                serde_json::Value::Array(catalog_json.clone()),
            );
            ctx.insert("max_recommendations".to_string(), serde_json::json!(3));
            ctx.insert(
                "epistemic_state".to_string(),
                serde_json::json!({
                    "confidence": confidence,
                    "uncertainty_type": "perspective_blind",
                }),
            );
            match executor
                .execute_knowact("skill-router/skill-router-match.j2", &ctx)
                .await
            {
                Ok(response) => {
                    let coverage = response
                        .get("coverage_assessment")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let rec_count = response
                        .get("recommendations")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    info!(
                        target: MC_TARGET,
                        confidence = *confidence,
                        coverage_assessment = %coverage,
                        recommendation_count = rec_count,
                        "Epistemic routing completed"
                    );
                    if let Some(sink) = self.context.regulation_sink() {
                        emit_meta_escalation(
                            sink.as_ref(),
                            self.context.handle().curator_id(),
                            "epistemic_routed",
                            *confidence,
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        target: MC_TARGET,
                        error = %e,
                        confidence = *confidence,
                        "skill-router invocation failed for epistemic routing"
                    );
                }
            }
        }
    }

    /// Decide a new variety-deficit threshold. Generative-first (LLM template);
    /// Rust compute_threshold_adjustment safety-rail fallback. The sparse-data
    /// gate applies to both paths (don't burn LLM gas on a single directive;
    /// loss prevention bypasses the gate when escalations are being dropped).
    async fn decide_self_calibration(
        &self,
        delta_dropped: u64,
        delta_directives: u64,
        effectiveness: f64,
        old: u64,
        since_raise: u64,
        last_calibration: Option<&serde_json::Value>,
    ) -> Option<(u64, &'static str)> {
        if delta_directives < MIN_OBSERVATIONS_TO_CALIBRATE && delta_dropped < RAISE_DROP_THRESHOLD
        {
            return None;
        }
        if let Some(executor) = self.context.manifest_executor().await
            && let Some(new) = self
                .try_generative_calibration(
                    &executor,
                    effectiveness,
                    old,
                    delta_directives,
                    delta_dropped,
                    last_calibration,
                )
                .await
        {
            return Some((new, "generative"));
        }
        compute_threshold_adjustment(delta_dropped, delta_directives, old, since_raise)
            .map(|adj| (adj.new_variety_deficit, "fallback"))
    }

    /// Invoke the curator/metacognition-self-calibrate template to generate a
    /// proposed new threshold. Returns None on template failure (caller falls
    /// back to the Rust safety-rail).
    async fn try_generative_calibration(
        &self,
        executor: &Arc<hkask_templates::ManifestExecutor>,
        effectiveness: f64,
        old: u64,
        delta_directives: u64,
        delta_dropped: u64,
        last_calibration: Option<&serde_json::Value>,
    ) -> Option<u64> {
        let sq = self.context.self_quality().snapshot();
        let mut ctx = HashMap::new();
        // Deltas since last calibration (not cumulative totals) — the model
        // needs recent activity, not ever-growing monotonic counters.
        ctx.insert(
            "self_quality".into(),
            serde_json::json!({
                "directives_since_last_cal": delta_directives,
                "escalations_dropped_since_last_cal": delta_dropped,
                "circuit_breaker_trips_total": sq.circuit_breaker_trips,
            }),
        );
        ctx.insert("effectiveness".into(), serde_json::json!(effectiveness));
        ctx.insert("current_threshold".into(), serde_json::json!(old));
        ctx.insert(
            "threshold_floor".into(),
            serde_json::json!(DEFAULT_ESCALATION_VARIETY_DEFICIT),
        );
        ctx.insert(
            "threshold_ceiling".into(),
            serde_json::json!(VARIETY_DEFICIT_CEILING),
        );
        ctx.insert(
            "last_calibration".into(),
            serde_json::json!(last_calibration),
        );
        match executor
            .execute_knowact("curator/metacognition-self-calibrate.j2", &ctx)
            .await
        {
            Ok(out) => out
                .get("new_threshold")
                .and_then(|v| v.as_u64())
                .or_else(|| {
                    tracing::warn!(
                        target: MC_TARGET,
                        "Self-calibration template returned no new_threshold — falling back"
                    );
                    None
                }),
            Err(e) => {
                tracing::warn!(
                    target: MC_TARGET,
                    error = %e,
                    "Self-calibration template failed — falling back to Rust safety-rail"
                );
                None
            }
        }
    }

    /// Template-driven compute: invoke KnowAct templates for calibrated decisions.
    pub(super) async fn compute_with_templates(
        &self,
        executor: &Arc<hkask_templates::ManifestExecutor>,
        deviations: &[Deviation],
    ) -> Vec<RegulatoryAction> {
        let snapshot = self.last_snapshot_tx.borrow().clone();
        let mut ctx = HashMap::new();

        if let Some(ref snap) = snapshot {
            ctx.insert("system_health".into(), serde_json::json!(snap.reg_health));
            ctx.insert(
                "critical_alerts".into(),
                serde_json::json!(snap.critical_alerts),
            );
            ctx.insert("total_alerts".into(), serde_json::json!(snap.total_alerts));
            ctx.insert(
                "variety_deficit".into(),
                serde_json::json!(snap.variety_deficit),
            );
        }

        let issues: Vec<serde_json::Value> = deviations
            .iter()
            .filter(|d| d.direction == DeviationDirection::AboveSetPoint)
            .map(|d| {
                serde_json::json!({
                    "id": d.signal.metric.as_str(),
                    "source_bot": "regulation",
                    "type": d.signal.metric.as_str(),
                    "severity": if d.magnitude > 2.0 { "critical" } else { "warning" },
                    "first_observed": d.signal.timestamp.to_rfc3339(),
                    "occurrence_count": 1,
                    "current_impact": format!("value {} > set-point {}", d.signal.value, d.signal.set_point),
                    "resolution_attempts": [],
                })
            })
            .collect();
        ctx.insert("issues".into(), serde_json::json!(issues));

        // ── Communication events: drain and process via respond template ──
        let mut actions = Vec::new();
        let comm_events = self.context.drain_communication_events().await;
        if !comm_events.is_empty() {
            let agent_name = &self.agent_name;
            for event in &comm_events {
                let bias = self.communication_posture.convergence_bias;

                // Score message saliency against persona via condenser MCP tool.
                // Saliency pulls effective bias upward — domain-relevant messages
                // trigger stronger convergence. Graceful degradation: default 0.5 on error.
                let body = event
                    .observation
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let persona_score = match executor
                    .call_tool(
                        "condenser/condenser_score_saliency",
                        serde_json::json!({"text": body, "against": "persona"}),
                    )
                    .await
                {
                    Ok(resp) => resp.get("score").and_then(|v| v.as_f64()).unwrap_or(0.5),
                    Err(e) => {
                        tracing::debug!(
                            target: MC_TARGET,
                            error = %e,
                            "Condenser saliency unavailable, using default 0.5"
                        );
                        0.5
                    }
                };
                let effective_bias = (bias + persona_score * (1.0 - bias)).min(1.0);

                let decision = cat::evaluate(effective_bias, agent_name, event);
                if let cat::Decision::Speak { convergence_level } = decision {
                    let sender = event
                        .observation
                        .get("sender")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let body = event
                        .observation
                        .get("body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let room_id = event
                        .observation
                        .get("room_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");

                    let mut resp_ctx = HashMap::new();
                    resp_ctx.insert("message_body".into(), serde_json::json!(body));
                    resp_ctx.insert("sender".into(), serde_json::json!(sender));
                    resp_ctx.insert("room_id".into(), serde_json::json!(room_id));
                    resp_ctx.insert(
                        "convergence_bias".into(),
                        serde_json::json!(convergence_level),
                    );
                    resp_ctx.insert(
                        "invariant_traits".into(),
                        serde_json::json!(self.communication_posture.invariant_traits),
                    );

                    match executor
                        .execute_knowact("curator/metacognition-respond.j2", &resp_ctx)
                        .await
                    {
                        Ok(output) => {
                            if output
                                .get("should_respond")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                let response_body = output
                                    .get("response_body")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if !response_body.is_empty() {
                                    let tool_input = serde_json::json!({
                                        "room_id": room_id,
                                        "body": response_body,
                                    });
                                    match executor
                                        .call_tool("communication/send_message", tool_input)
                                        .await
                                    {
                                        Ok(_) => {
                                            tracing::info!(
                                                target: MC_TARGET,
                                                sender = %sender,
                                                room_id = %room_id,
                                                "Communication response sent via MCP"
                                            );
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                target: MC_TARGET,
                                                error = %e,
                                                "Failed to send communication response via MCP"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(target: MC_TARGET, error = %e, "Communication respond template failed");
                        }
                    }
                }
            }
        }

        // Circuit breaker: skip template after 3 consecutive failures,
        // retry after 5 skip cycles.
        let skip = self
            .template_skip_remaining
            .load(std::sync::atomic::Ordering::Relaxed);
        if skip > 0 {
            self.template_skip_remaining
                .store(skip - 1, std::sync::atomic::Ordering::Relaxed);
            return self.compute_with_thresholds(deviations);
        }

        let result = match executor
            .execute_knowact("curator/metacognition-diagnose.j2", &ctx)
            .await
        {
            Ok(r) => {
                self.consecutive_template_failures
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                r
            }
            Err(e) => {
                let failures = self
                    .consecutive_template_failures
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1;
                tracing::warn!(target: MC_TARGET, error = %e, consecutive_failures = failures, "Template failed");
                if failures >= 3 {
                    self.template_skip_remaining
                        .store(5, std::sync::atomic::Ordering::Relaxed);
                    self.context.self_quality().record_circuit_breaker();
                    if let Some(sink) = self.context.regulation_sink() {
                        emit_meta_circuit_breaker(
                            sink.as_ref(),
                            self.context.handle().curator_id(),
                            5,
                        );
                    }
                    tracing::warn!(target: MC_TARGET, "Circuit breaker tripped — skipping template for 5 cycles");
                }
                return self.compute_with_thresholds(deviations);
            }
        };

        // actions declared above for communication events; continued here
        if let Some(plan) = result.get("remediation_plan").and_then(|v| v.as_array()) {
            if plan.is_empty() {
                tracing::info!(target: MC_TARGET, "LLM returned empty remediation_plan — no actions");
            }
            for step in plan {
                let action_type = step.get("action").and_then(|v| v.as_str()).unwrap_or("");
                let target = step.get("target").and_then(|v| v.as_str()).unwrap_or("");
                if action_type.is_empty() {
                    tracing::warn!(target: MC_TARGET, ?step, "LLM produced malformed remediation step — missing 'action' field");
                }
                match action_type {
                    "calibrate" | "adjust_threshold" => actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        ActionType::Calibrate,
                        RegulatoryActionParams::reason("calibrate"),
                    )),
                    "adjust_budget" => {
                        let new_budget =
                            step.get("new_budget").and_then(|v| v.as_u64()).unwrap_or(0);
                        if new_budget > 0 && !target.is_empty() {
                            actions.push(RegulatoryAction::new(
                                LoopId::Curation,
                                ActionType::OverrideEnergyBudget,
                                RegulatoryActionParams::with_data(
                                    "adjust_budget",
                                    RegulationData::CuratorBudgetOverride {
                                        agent: target.to_string(),
                                        new_budget,
                                    },
                                ),
                            ));
                        }
                    }
                    "escalate" | "restart" | "rebalance" => actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        ActionType::Escalate,
                        RegulatoryActionParams::reason(action_type),
                    )),
                    _ => actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        ActionType::Notify,
                        RegulatoryActionParams::reason("notify"),
                    )),
                }
            }
        }

        // Store template output for act phase and generate_summary.
        // Uses a dedicated RwLock to avoid the race condition where
        // sense() would wipe template_output from HealthSnapshot.
        *self.last_template_output.write().await = Some(result.clone());

        actions
    }

    /// Fallback: Rust threshold comparison (standalone CLI, no executor).
    pub(super) fn compute_with_thresholds(
        &self,
        deviations: &[Deviation],
    ) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();
        for dev in deviations {
            match dev.signal.metric {
                SignalMetric::MetacognitionVarietyDeficit
                    if dev.direction == DeviationDirection::AboveSetPoint =>
                {
                    let _deficit = dev.signal.value as u64;
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        ActionType::Calibrate,
                        RegulatoryActionParams::reason("variety_deficit"),
                    ));
                }
                SignalMetric::MetacognitionCriticalAlerts
                    if dev.direction == DeviationDirection::AboveSetPoint =>
                {
                    let _count = dev.signal.value as u64;
                    actions.push(RegulatoryAction::new(
                        LoopId::Curation,
                        ActionType::Escalate,
                        RegulatoryActionParams::reason("critical_alerts"),
                    ));
                }
                _ => {}
            }
        }
        actions
    }
}

// ── Self-calibration decision logic (pure, independently tested) ────────────

/// Escalations dropped (since last calibration) at or above this triggers a RAISE.
const RAISE_DROP_THRESHOLD: u64 = 3;
/// Minimum directives since last calibration before any adjustment. Mirrors
/// `SetPointCalibrator`'s `min_total_observations` gate — don't calibrate on
/// sparse data (avoids oscillating on a single noisy directive). RAISE on
/// dropped escalations bypasses this (loss prevention).
const MIN_OBSERVATIONS_TO_CALIBRATE: u64 = 5;
/// Cycles that must elapse after a RAISE before a LOWER is permitted
/// (hysteresis — prevents immediate raise/lower oscillation). At a 10s tick
/// this is ~60s.
const LOWER_COOLDOWN: u64 = 6;
/// Absolute ceiling for the variety-deficit threshold (4x the system default).
/// Bounds runaway desensitization from repeated drops; the threshold lives in
/// the band [DEFAULT_ESCALATION_VARIETY_DEFICIT, VARIETY_DEFICIT_CEILING].
const VARIETY_DEFICIT_CEILING: u64 = DEFAULT_ESCALATION_VARIETY_DEFICIT.saturating_mul(4);

/// A computed threshold adjustment (the decision half of self-calibration).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ThresholdAdjustment {
    pub new_variety_deficit: u64,
    /// true = threshold raised (desensitize); false = lowered (sensitize).
    pub raised: bool,
}

/// Pure decision: given self-quality deltas, the current threshold, and
/// cycles since the last raise, decide whether to adjust the variety-deficit
/// threshold and in which direction.
///
/// - RAISE by 10% (min +1, capped at VARIETY_DEFICIT_CEILING) when
///   `delta_dropped >= RAISE_DROP_THRESHOLD`.
/// - LOWER by 5% (min -1, floored at DEFAULT_ESCALATION_VARIETY_DEFICIT) when
///   no escalations were dropped (`delta_dropped == 0`) AND
///   `since_raise >= LOWER_COOLDOWN` AND above the floor.
/// - No activity (`delta_directives == 0 && delta_dropped == 0`) => None.
///
/// RAISE takes precedence over LOWER (loss prevention over responsiveness).
/// Note: this function judges purely on the DIRECT signal (escalation drops) —
/// the lever-controlled metric. regulation_effectiveness is NOT used here (it's
/// a different loop's metric; retained only in the span for offline GEPA).
#[must_use]
pub(super) fn compute_threshold_adjustment(
    delta_dropped: u64,
    delta_directives: u64,
    old_threshold: u64,
    since_raise: u64,
) -> Option<ThresholdAdjustment> {
    // Don't calibrate on a sparse sample unless escalations are actively being
    // lost (RAISE bypasses the gate — loss prevention overrides patience).
    if delta_directives < MIN_OBSERVATIONS_TO_CALIBRATE && delta_dropped < RAISE_DROP_THRESHOLD {
        return None;
    }

    // RAISE: escalations being lost => too sensitive.
    if delta_dropped >= RAISE_DROP_THRESHOLD {
        let new = ((old_threshold / 10).max(1) + old_threshold).min(VARIETY_DEFICIT_CEILING);
        if new != old_threshold {
            return Some(ThresholdAdjustment {
                new_variety_deficit: new,
                raised: true,
            });
        }
    }

    // LOWER: no drops + cooldown elapsed + above the floor.
    if delta_dropped == 0
        && since_raise >= LOWER_COOLDOWN
        && old_threshold > DEFAULT_ESCALATION_VARIETY_DEFICIT
    {
        let new =
            (old_threshold - (old_threshold / 20).max(1)).max(DEFAULT_ESCALATION_VARIETY_DEFICIT);
        if new != old_threshold {
            return Some(ThresholdAdjustment {
                new_variety_deficit: new,
                raised: false,
            });
        }
    }

    None
}

#[cfg(test)]
mod self_calibrate_tests {
    use super::*;
    // DEFAULT_ESCALATION_VARIETY_DEFICIT is a `use` import in the parent, not
    // re-exported by `super::*`, so import it explicitly.
    use super::super::escalation::DEFAULT_ESCALATION_VARIETY_DEFICIT;

    #[test]
    fn no_activity_yields_no_adjustment() {
        assert!(compute_threshold_adjustment(0, 0, 100, 10).is_none());
    }

    #[test]
    fn sparse_sample_does_not_calibrate() {
        // 3 directives, no drops, healthy, cooldown met — but below
        // MIN_OBSERVATIONS_TO_CALIBRATE (5) => no lowering (patient).
        assert!(compute_threshold_adjustment(0, 3, 150, 6).is_none());
        assert!(compute_threshold_adjustment(0, 4, 150, 6).is_none());
    }

    #[test]
    fn dropped_escalations_raise_the_threshold() {
        // 3 drops at threshold 100 => 100 + max(10,1) = 110.
        let adj = compute_threshold_adjustment(3, 5, 100, 0).unwrap();
        assert!(adj.raised);
        assert_eq!(adj.new_variety_deficit, 110);
    }

    #[test]
    fn raise_bypasses_min_observations_gate() {
        // Only 1 directive but 3 drops => RAISE still fires (loss prevention).
        let adj = compute_threshold_adjustment(3, 1, 100, 0).unwrap();
        assert!(adj.raised);
    }

    #[test]
    fn raise_is_bounded_by_ceiling() {
        // Repeated raises converge to VARIETY_DEFICIT_CEILING (4x default = 400).
        let mut t = 100u64;
        for _ in 0..40 {
            if let Some(a) = compute_threshold_adjustment(3, 5, t, 0) {
                t = a.new_variety_deficit;
            }
        }
        assert_eq!(t, VARIETY_DEFICIT_CEILING, "threshold hit the ceiling");
        // At the ceiling, no further raise.
        assert!(compute_threshold_adjustment(3, 5, VARIETY_DEFICIT_CEILING, 0).is_none());
    }

    #[test]
    fn healthy_loop_lowers_after_cooldown() {
        // threshold 150, no drops, high effectiveness, cooldown met, enough
        // directives => 150-7 = 143.
        let adj = compute_threshold_adjustment(0, 6, 150, 6).unwrap();
        assert!(!adj.raised);
        assert_eq!(adj.new_variety_deficit, 143);
    }

    #[test]
    fn lowering_respects_cooldown() {
        // since_raise < LOWER_COOLDOWN => no lowering even if healthy.
        assert!(compute_threshold_adjustment(0, 6, 150, 5).is_none());
    }

    #[test]
    fn lowering_floored_at_default() {
        // threshold 105 => 105-5 = 100 (the floor).
        let adj = compute_threshold_adjustment(0, 6, 105, 6).unwrap();
        assert_eq!(adj.new_variety_deficit, DEFAULT_ESCALATION_VARIETY_DEFICIT);
        // At the floor already => no further lowering.
        assert!(
            compute_threshold_adjustment(0, 6, DEFAULT_ESCALATION_VARIETY_DEFICIT, 6).is_none()
        );
    }

    #[test]
    fn raise_takes_precedence_over_lower() {
        // Drops present AND healthy AND cooldown met => RAISE wins.
        let adj = compute_threshold_adjustment(3, 6, 150, 10).unwrap();
        assert!(adj.raised);
    }
}

#[cfg(test)]
mod epistemic_routing_tests {
    #![allow(dead_code)]
    use super::*;
    use hkask_regulation::RegulationLedger;
    use hkask_types::curator::CuratorHandle;
    use hkask_types::escalation::{EscalationBatch, EscalationEntry};
    use hkask_types::{BotID, EscalationID, InfrastructureError, TemplateID};

    /// No-op EscalationPort for testing — all operations succeed with empty results.
    struct NoopEscalationPort;

    impl hkask_types::ports::escalation::EscalationPort for NoopEscalationPort {
        fn list_pending(&self) -> Result<Vec<EscalationEntry>, InfrastructureError> {
            Ok(Vec::new())
        }
        fn get(&self, _id: &str) -> Result<Option<EscalationEntry>, InfrastructureError> {
            Ok(None)
        }
        fn resolve(&self, _id: &str, _resolved_by: &str) -> Result<(), InfrastructureError> {
            Ok(())
        }
        fn dismiss(&self, _id: &str, _dismissed_by: &str) -> Result<(), InfrastructureError> {
            Ok(())
        }
        fn persist_batch(&self, _batch: &EscalationBatch) -> Result<(), InfrastructureError> {
            Ok(())
        }
        fn add(
            &self,
            _template_id: TemplateID,
            _bot_id: BotID,
            _output: String,
            _confidence: f64,
            _retry_count: u32,
            _error_context: String,
        ) -> Result<EscalationID, InfrastructureError> {
            Ok(EscalationID::new())
        }
    }

    fn make_loop() -> MetacognitionLoop {
        let context = Arc::new(CuratorContext::new(
            CuratorHandle::system(),
            Arc::new(RegulationLedger::default()),
            None,
            Arc::new(NoopEscalationPort) as Arc<dyn hkask_types::ports::escalation::EscalationPort>,
        ));
        MetacognitionLoop::new(context, MetacognitionConfig::default())
    }

    #[tokio::test]
    async fn empty_signals_returns_immediately() {
        let mc = make_loop();
        // Should complete without error — no signals to route.
        mc.route_epistemic_escalations(&[]).await;
    }

    #[tokio::test]
    async fn no_skill_catalog_degrades_gracefully() {
        let mc = make_loop();
        // No skill catalog set and no manifest executor — should return
        // without invoking any template or panicking.
        let signals = vec![(0.2, "test low-confidence output".to_string())];
        mc.route_epistemic_escalations(&signals).await;
    }

    #[tokio::test]
    async fn no_executor_degrades_gracefully() {
        // Set a skill catalog but no manifest executor — should still
        // degrade gracefully (catalog without executor is useless).
        let context = Arc::new(CuratorContext::new(
            CuratorHandle::system(),
            Arc::new(RegulationLedger::default()),
            None,
            Arc::new(NoopEscalationPort) as Arc<dyn hkask_types::ports::escalation::EscalationPort>,
        ));
        context.set_skill_catalog(vec![]).await;
        let mc = MetacognitionLoop::new(context, MetacognitionConfig::default());
        let signals = vec![(0.3, "another low-confidence output".to_string())];
        mc.route_epistemic_escalations(&signals).await;
    }
}
