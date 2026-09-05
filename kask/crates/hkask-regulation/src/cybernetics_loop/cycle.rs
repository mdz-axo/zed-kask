//! The regulation cycle — sense → compare → compute → act → verify.
//!
//! Extracted from the cybernetics_loop god-module. The facade's `tick`
//! orchestrates these phases; each phase is `pub(super)` so the facade can
//! call it. Action construction (`build_regulation_action`), alert routing
//! (`route_action_as_alert`), and the cycle-internal helpers
//! (`try_substitute`, `persist_alert_to_queue`) are private to this module.

use crate::algedonic::{AlertSeverity, RuntimeAlert};
use crate::loops::{
    ActionDecision, ActionType, CurationInput, Deviation, ImpactReport, LoopId, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric,
};
use crate::loops::{BudgetOption, RegulationData};
use crate::regulation_policy::{
    self, RegulationPolicy, RegulationReason, classify_decision, default_substitution_ladder,
    extract_deficit_threshold,
};
use crate::set_points::InferenceThrottleMode;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};

impl super::CyberneticsLoop {
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
    pub(super) async fn emit_regulation_span(
        &self,
        kind: SpanKind,
        observation: serde_json::Value,
    ) {
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
    fn persist_alert_to_queue(
        &self,
        alert: &RuntimeAlert,
        efferent_action: Option<&str>,
        recovery_signal: Option<&Signal>,
    ) {
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
            "recovery_signal": recovery_signal,
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
    pub(super) fn check_coherence(&self, actions: &[RegulatoryAction]) {
        use ActionType::*;
        let has = |t: ActionType| actions.iter().any(|a| a.action_type == t);
        let has_target = |t: ActionType, target: LoopId| {
            actions
                .iter()
                .any(|a| a.action_type == t && a.target == target)
        };

        let mut conflicts: Vec<String> = Vec::new();

        // Throttle + CircuitBreak — contradictory (slow down vs stop).
        // When both target Inference, use the more specific message instead
        // of the generic one to avoid double-alerting for the same conflict.
        if has(Throttle) && has(CircuitBreak) {
            if has_target(Throttle, LoopId::Inference)
                && has_target(CircuitBreak, LoopId::Inference)
            {
                tracing::warn!(
                    target: "reg.outcome.coherence",
                    "Throttle + CircuitBreak both targeting Inference loop — consider consolidating"
                );
                conflicts.push("contradictory_actions: Throttle+CircuitBreak on Inference".into());
            } else {
                tracing::warn!(
                    target: "reg.outcome.coherence",
                    action_count = actions.len(),
                    "Potentially contradictory Throttle + CircuitBreak in same tick"
                );
                conflicts.push("contradictory_actions: Throttle+CircuitBreak".into());
            }
        }

        // AdjustEnergyBudget + OverrideEnergyBudget — contradictory (manual vs forced).
        if has(AdjustEnergyBudget) && has(OverrideEnergyBudget) {
            tracing::warn!(
                target: "reg.outcome.coherence",
                action_count = actions.len(),
                "Potentially contradictory AdjustEnergyBudget + OverrideEnergyBudget in same tick"
            );
            conflicts.push("contradictory_actions: AdjustEnergyBudget+OverrideEnergyBudget".into());
        }

        // Persist coherence conflicts to the escalation queue so the Curator
        // can see them — not just a log warning that may be missed. Before
        // this fix, check_coherence was advisory-only: it detected conflicts
        // but neither suppressed the conflicting actions nor alerted the
        // Curator. The coherence check was a sensor with no actuator (B4).
        for conflict in &conflicts {
            let alert = RuntimeAlert {
                domain: format!("reg.coherence:{conflict}"),
                deficit: 1,
                threshold: 1,
                severity: AlertSeverity::Warning,
                escalated: true,
                timestamp: chrono::Utc::now(),
                message: format!(
                    "Regulation coherence conflict detected: {conflict} ({} actions this tick)",
                    actions.len()
                ),
            };
            self.persist_alert_to_queue(&alert, None, None);
            if let Some(ref tx) = self.alerts_tx {
                if tx.send(CurationInput::Alert(alert)).is_err() {
                    tracing::warn!(target: "reg.alert", "Coherence alert send failed — channel closed");
                }
            }
        }
    }

    /// Compare: detect deviations from set-points.
    pub(super) async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        signals.iter().filter_map(Deviation::from_signal).collect()
    }

    /// Produces signals for: per-agent energy ratio, variety deficit, queue depth.
    pub(super) async fn sense(&self) -> Vec<Signal> {
        // Process pending directives before sensing state
        self.process_inbox().await;

        let mut signals = Vec::new();

        // All sensing is now done through the SensorBus.
        // Energy remaining, variety deficit, and tool reliability are all sensed
        // by registered Sensor implementations.

        // Append signals from pluggable sensor providers.
        let registry_signals = self.sensor_registry.sense_all(LoopId::Cybernetics).await;
        signals.extend(registry_signals);

        // Sense the in-memory algedonic log cap. When the log approaches its
        // cap, emit a signal so the operator (or the `algedonic-review` skill)
        // can review and clear reviewed entries before they are evicted unread.
        // The set-point is 0.0 — any positive value (1.0 = approaching cap) is
        // a deviation.
        signals.push(Signal::new(LoopId::Cybernetics, SignalMetric::AlgedonicLogApproachingCap,
            if self.ledger.read().await.alert_log_approaching_cap().await { 1.0 } else { 0.0 }, 0.0));

        // Sense the algedonic log's population state: actionable events
        // (Warning or Critical — Info entries are healthy-range diagnostics
        // that don't demand review), escalated-but-unresolved events, and
        // critical alerts. Set-points are 0.0 — any positive count is a
        // deviation. The dampener prevents repeat-escalation spam while an
        // alert awaits review; `clear_reviewed_alerts` closes the loop.
        let (actionable_count, escalated_count, critical_count) = {
            let ledger = self.ledger.read().await;
            (
                ledger.actionable_alert_count().await,
                ledger.escalated_alert_count().await,
                ledger.critical_alerts().await.len(),
            )
        };
        {
            signals.push(Signal::new(
                LoopId::Cybernetics,
                SignalMetric::AlgedonicEvents,
                actionable_count as f64,
                0.0,
            ));
        }
        {
            signals.push(Signal::new(
                LoopId::Cybernetics,
                SignalMetric::PendingEscalations,
                escalated_count as f64,
                0.0,
            ));
        }
        {
            signals.push(Signal::new(
                LoopId::Cybernetics,
                SignalMetric::MetacognitionCriticalAlerts,
                critical_count as f64,
                0.0,
            ));
        }

        // Sense whether a model-bearing inference health source has been
        // wired (see the `inference_health_wired` field). The composition
        // root wires the source only after the default LanguageModel
        // resolves; an unwired loop means inference is unusable — the state
        // `NoModelInferencePort` exists for. Grace: the first ticks after
        // boot often precede the deferred task's wiring; firing on those
        // ticks would report a false model outage on every slow boot
        // (observed live: a boot wired the no-op port at +4s while the model
        // resolved later). 3 ticks (~30s) covers the common transient; a
        // genuinely unconfigured system fires after that and self-clears
        // the moment the source is wired.
        const MODEL_WIRING_GRACE_TICKS: usize = 3;
        let ticks_elapsed = self.tick_count.load(std::sync::atomic::Ordering::Relaxed);
        if self.inference_health_wired || ticks_elapsed >= MODEL_WIRING_GRACE_TICKS {
            signals.push(Signal::new(
                LoopId::Cybernetics,
                SignalMetric::InferenceModelAvailable,
                if self.inference_health_wired { 1.0 } else { 0.0 },
                1.0,
            ));
        }

        // Feed observed values into the predictive simulator.
        for signal in &signals {
            self.simulator.observe(signal.metric, signal.value);
        }

        signals
    }

    pub(super) async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
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
                // Notify actions are observational — they signal "approaching
                // threshold" but carry no efferent action. Logging them here
                // makes the observation visible without inflating the action
                // count (which would make gain > 1.0, breaking the documented
                // 0.0–1.0 contract). route_action_as_alert skips Notify
                // actions, so adding them to `actions` would also be a silent
                // drop (F8).
            }
        }

        let policy = RegulationPolicy::default();

        for dev in deviations {
            for proposed in policy.decide(dev) {
                let action = self.build_regulation_action(dev, proposed).await;
                if let Some(a) = action {
                    if a.action_type == ActionType::Notify {
                        // Observational actions (Notify) are logged here, not
                        // added to `actions`. They signal "metric observed"
                        // but carry no efferent action — route_action_as_alert
                        // would skip them (F8), and counting them would inflate
                        // gain beyond 1.0 (B1). Logging preserves observability.
                        tracing::info!(
                            target: "reg.cybernetics",
                            metric = a.metric_name.as_deref().unwrap_or("unknown"),
                            reason = %a.parameters.reason,
                            "Notify action — observational, not routed as alert"
                        );
                    } else {
                        actions.push(a);
                    }
                }
            }
        }
        actions
    }

    pub(super) async fn act(&self, actions: &[RegulatoryAction]) {
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
                self.persist_alert_to_queue(&alert, None, None);
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

        // Build the alert. For native Escalate actions, extract the
        // deficit/threshold from the typed data when available. For converted
        // efferent actions, synthesize a deficit of 1 and threshold of 1 — the
        // alert's purpose is advisory, not quantitative.
        let (deficit, threshold, message) = if is_native_escalate {
            // Message composition lives in regulation_policy::alert_message —
            // the single source of truth for this format. verify_impact's
            // auto-resolve reconstruction calls the same helper; a local
            // format! here would let the two sites drift and silently break
            // the dedup-match.
            let message = regulation_policy::alert_message(
                &action.parameters.data,
                &action.parameters.reason,
            );
            match extract_deficit_threshold(&action.parameters.data) {
                Some((d, t)) => (d, t, message),
                None => {
                    // No quantitative data (NoData or non-threshold variant) —
                    // the (1, 1) sentinel matches the advisory pattern used by
                    // efferent and plateau alerts: "one issue, threshold one
                    // issue." The previous (0, 0) fallback produced misleading
                    // error_context JSON that triage read as "no deficit, no
                    // threshold" — indistinguishable from a broken sense input
                    // returning zero.
                    (1, 1, message)
                }
            }
        } else {
            let msg = format!(
                "Efferent action {} (target: {}) recommended but not wired — reason: {}",
                action.action_type.as_str(),
                action.target,
                action.parameters.reason
            );
            (1, 1, msg)
        };
        let domain = if is_native_escalate {
            String::new()
        } else {
            format!("efferent:{}", action.action_type.as_str())
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

        // Source-level dedup: if there is already a pending escalation with
        // this condition (the reason prefix — see `alert_condition`), skip
        // the entire routing (persist, live channel, archive). The regulation
        // loop senses the same deficit every cycle; without this check it
        // re-escalates every tick, flooding the queue, the live channel, and
        // the archive with alerts for one condition. Matching is on the
        // condition, not the full message — the embedded value changes every
        // cycle, so exact-match dedup never hits. The operator reviews the
        // first one; when they resolve/dismiss it, the next cycle escalates
        // again.
        if let Some(ref sink) = self.alert_escalation_sink {
            if sink.has_pending_alert(&alert.message) {
                tracing::debug!(
                    target: "reg.cybernetics",
                    action_type = ?action.action_type,
                    target_loop = %action.target,
                    "Suppressing duplicate efferent alert — pending escalation already in queue"
                );
                return;
            }
        }

        // Persist to the reviewable escalation queue unconditionally —
        // the queue is the primary durable path for alert review, not
        // a fallback. The RegulationArchive below remains as a
        // secondary fallback for restart durability when the live
        // channel is down.
        let observation = action.metric_name.as_deref().and_then(SignalMetric::from_str_name)
            .and_then(|metric| self.observations.lock().get(&metric).cloned());
        self.persist_alert_to_queue(&alert, efferent_action, observation.as_ref());

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
    pub(super) async fn verify_impact(
        &self,
        previous_actions: &[RegulatoryAction],
    ) -> Vec<ImpactReport> {
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
            // Event-substrate path (phase 6): when the action's parameters
            // name a rollout and a rollout event source is wired, query the
            // store for the before/after values. The struct walk below is the
            // fallback for actions that don't target a rollout. A query
            // failure (Err) or a no-data result (Ok(None)) is warned and skips
            // the action — it does NOT fall through to the struct walk: a
            // RolloutImpactCheck carries no before value to re-sense, so
            // falling through would silently drop the submitted check with no
            // signal (the .rules broken-feedback-loop trap).
            //
            // The two paths that proceed (store-answered Ok(Some) and the
            // struct-walk fallback) converge on the SAME classify/stagnation/
            // block tail below — the store path sets (metric, before, after)
            // and jumps past the struct walk; it must not bypass the stagnation
            // detector or the block escalation, or a store-answered failure
            // would never trigger plateau detection while a re-sensed one
            // would.
            let mut store_answered = false;
            let (mut before_val, mut metric) = (0.0, SignalMetric::EnergyRemaining);
            let mut after_val = 0.0;
            // Capture the rollout_id when the store answers so the impact
            // verdict write-back below can target the same rollout.
            let mut store_rollout_id: Option<String> = None;
            if let Some(source) = &self.rollout_events
                && let Some((rollout_id, before_position)) = action.parameters.data.rollout_target()
            {
                // Only `RolloutImpactCheck` reaches here (rollout_target is
                // Some only for that variant). Handle every store outcome
                // explicitly — a silent fall-through to the struct-walk below
                // would drop the submitted check with no signal, which is
                // indistinguishable from "the check never ran" (the .rules
                // broken-feedback-loop trap: never silently discard errors).
                match source.metric_before_and_after(
                    &rollout_id,
                    action.parameters.data.metric_name(),
                    before_position,
                ) {
                    Ok(Some((queried_before, queried_after))) => {
                        metric = SignalMetric::from_str_name(action.parameters.data.metric_name())
                            .unwrap_or(SignalMetric::EnergyRemaining);
                        before_val = queried_before;
                        after_val = queried_after;
                        store_answered = true;
                        store_rollout_id = Some(rollout_id.clone());
                        tracing::debug!(
                            target: "reg.cybernetics",
                            rollout = %rollout_id,
                            before = queried_before,
                            after = queried_after,
                            "verify_impact answered from the rollout event store"
                        );
                    }
                    Ok(None) => {
                        // The store has no before/after for this rollout/metric.
                        // A submitted check with no store answer must be visible
                        // — warn so "no baseline" is distinguishable from "no
                        // check ran." There is nothing to fall back to (a
                        // RolloutImpactCheck carries no before value), so skip.
                        tracing::warn!(
                            target: "reg.cybernetics",
                            rollout = %rollout_id,
                            metric = action.parameters.data.metric_name(),
                            "rollout impact check found no events for this metric — no baseline to verify against"
                        );
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "reg.cybernetics",
                            rollout = %rollout_id,
                            metric = action.parameters.data.metric_name(),
                            error = %error,
                            "rollout impact check store query failed — verdict not computed"
                        );
                        continue;
                    }
                }
            }
            if !store_answered {
                // Fallback: determine metric and pre-action value from the
                // typed RegulationData, then re-sense the after value.
                // The per-variant (metric, before-value) table lives on
                // `RegulationData::impact_before_value`, colocated with the
                // variants it describes.
                let (fallback_before, fallback_metric) = match action
                    .parameters
                    .data
                    .impact_before_value()
                {
                    Some((data_metric, data_before)) => (data_before, data_metric),
                    None => {
                        // Actions whose RegulationData variant carries no
                        // before-value (NoData and the meta-regulatory /
                        // observational arms) can't be verified via the
                        // struct-walk. Warn so the skip is visible — a
                        // silent continue would make "no verification ran"
                        // indistinguishable from "verification ran and
                        // passed" (the .rules broken-feedback-loop trap).
                        // Full impact verification for these actions
                        // requires carrying the before-value in a typed
                        // RegulationData variant (follow-up).
                        tracing::warn!(
                            target: "reg.cybernetics",
                            metric = action.metric_name.as_deref().unwrap_or("unknown"),
                            reason = %action.parameters.reason,
                            "verify_impact: unhandled RegulationData variant — no before-value to verify against, skipping"
                        );
                        continue;
                    }
                };
                before_val = fallback_before;
                metric = fallback_metric;
                after_val = match metric {
                    SignalMetric::EnergyRemaining => budget_statuses
                        .iter()
                        .map(|(_, s)| s.remaining as f64 / s.ceiling.max(1) as f64)
                        .fold(1.0, f64::min),
                    SignalMetric::VarietyDeficit => current_deficit,
                    SignalMetric::ToolReliability => {
                        // Re-sense the aggregate success rate from the ledger
                        // using the same equal-weighted domain aggregation as
                        // `ToolReliabilitySensor::sense` so the before/after
                        // values are comparable. Zero tracked domains is no
                        // data — warn and skip rather than reporting 0.0,
                        // which would read as "fully degraded" (the .rules
                        // unwrap_or(0) trap).
                        let ledger = self.ledger.read().await;
                        let domains = ledger.tracked_outcome_domains().await;
                        let mut sum = 0.0;
                        let mut tracked = 0;
                        for domain in &domains {
                            if let Some(rate) = ledger.outcome_success_rate(domain).await {
                                sum += rate;
                                tracked += 1;
                            }
                        }
                        if tracked == 0 {
                            tracing::warn!(
                                target: "reg.cybernetics",
                                "verify_impact: ledger has no tracked tool-outcome domains — cannot re-sense reliability, skipping"
                            );
                            continue;
                        }
                        sum / tracked as f64
                    }
                    SignalMetric::ContextServerHealth => {
                        // Re-sense fleet health from the source stored on the loop.
                        // If the source is not wired, warn and skip — a silent
                        // 0.0 would read as "fleet fully degraded" (the .rules
                        // unwrap_or(0) trap).
                        if let Some(ref source) = self.context_server_health_source {
                            let healthy = source.healthy_count().await;
                            let total = source.total_count().await;
                            if total == 0 {
                                tracing::warn!(
                                    target: "reg.cybernetics",
                                    "verify_impact: context-server health source reports 0 total servers — cannot re-sense, skipping"
                                );
                                continue;
                            }
                            healthy as f64 / total as f64
                        } else {
                            tracing::warn!(
                                target: "reg.cybernetics",
                                "verify_impact: context-server health source not wired — cannot re-sense fleet health, skipping"
                            );
                            continue;
                        }
                    }
                    SignalMetric::OcrSilentFailures => {
                        // Re-sense the recent-window count from the source stored
                        // on the loop. A broken or unwired source must warn and
                        // skip — a silent 0.0 would read as "storm over" and
                        // falsely auto-resolve the escalation (the .rules
                        // unwrap_or(0) trap).
                        if let Some(ref source) = self.ocr_health_source {
                            match source.recent_silent_failures().await {
                                Ok(count) => count as f64,
                                Err(error) => {
                                    tracing::warn!(
                                        target: "reg.cybernetics",
                                        error = %error,
                                        "verify_impact: OCR health source unreadable — cannot re-sense, skipping"
                                    );
                                    continue;
                                }
                            }
                        } else {
                            tracing::warn!(
                                target: "reg.cybernetics",
                                "verify_impact: OCR health source not wired — cannot re-sense silent failures, skipping"
                            );
                            continue;
                        }
                    }
                    _ => continue,
                };
            }

            let delta = after_val - before_val;
            // The per-metric direction table lives on
            // `SignalMetric::impact_direction`, colocated with the metric
            // it describes. No direction = no verified impact path: any
            // nonzero delta counts as a change.
            let improved = match metric.impact_direction() {
                Some(true) => delta > 0.0,
                Some(false) => delta < 0.0,
                None => false,
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

            // Tolerated noise is acceptable, but only observed improvement
            // resets stagnation. Recommendations are not proof of intervention.
            let action_type_str = action.action_type.as_str();
            let plateau = self.stagnation_detector.record_and_check(
                metric.as_str(),
                action_type_str,
                improved,
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
                        "Regulatory plateau: {} via {:?} has shown no observed improvement for {threshold} consecutive cycles",
                        metric.as_str(),
                        action.action_type,
                    ),
                };
                // Persist to the reviewable escalation queue unconditionally.
                self.persist_alert_to_queue(&alert, None, None);
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
                self.persist_alert_to_queue(&alert, None, None);
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

            // Event-substrate write-back: when the store answered the
            // before/after query, persist the regulation loop's impact
            // verdict back to the store as a `regulation_impact`-sourced
            // verdict event. This closes the feedback loop — downstream
            // consumers (training bridge, regression monitor, ORIENT) can
            // see the regulation system's judgment alongside the harness's
            // deterministic-evaluator verdicts. A write failure is warned
            // and never silently dropped (the .rules failure-signal rule:
            // a missing write-back means the loop's judgment is invisible to
            // store consumers, which must be distinguishable from "no impact
            // check ran").
            if store_answered
                && let Some(source) = &self.rollout_events
                && let Some(rollout_id) = &store_rollout_id
            {
                if let Err(error) = source.append_impact_verdict(
                    rollout_id,
                    action.parameters.data.metric_name(),
                    before_val,
                    after_val,
                    improved,
                    &format!("{:?}", decision),
                ) {
                    tracing::warn!(
                        target: "reg.cybernetics",
                        rollout = %rollout_id,
                        error = %error,
                        "impact verdict write-back failed — the loop's judgment is not persisted to the store"
                    );
                }
            }

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
                Some(RegulatoryAction::with_metric(
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
                    dev.signal.metric.as_str().into(),
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
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "energy_depletion_auto_adjust",
                        RegulationData::EnergyDepletionAutoAdjust {
                            remaining_ratio: dev.signal.value,
                            set_point: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // -- VarietyDeficit AboveSetPoint -------------------------------
            RegulationReason::VarietyDeficitExceeded => {
                let at = self
                    .try_substitute(VarietyDeficit, proposed.action_type)
                    .await;
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "variety_deficit_exceeded",
                        RegulationData::VarietyDeficitExceeded {
                            deficit: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // -- ErrorRate AboveSetPoint ------------------------------------
            RegulationReason::ErrorRateExceeded => {
                let at = self.try_substitute(ErrorRate, proposed.action_type).await;
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "error_rate_exceeded",
                        RegulationData::ErrorRateExceeded {
                            error_rate: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // -- ConnectorLatency AboveSetPoint -----------------------------
            RegulationReason::ConnectorLatencyExceeded => {
                let at = self
                    .try_substitute(ConnectorLatency, proposed.action_type)
                    .await;
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "connector_latency_exceeded",
                        RegulationData::ConnectorLatencyExceeded {
                            latency_secs: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
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
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "communication_backpressure",
                        RegulationData::CommunicationBackpressure {
                            queue_depth: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // -- Wallet and SeamCoverage handlers removed 2026-08-30 —
            // residuals of the deleted wallet module (219c74b180) and a
            // never-built seam watcher; no sensor ever emitted these
            // metrics, so these arms were unreachable.
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
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        "tool_reliability_degraded",
                        RegulationData::ToolReliabilityDegraded {
                            reliability: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // -- Observational metrics → Notify (no substitution ladder) --
            RegulationReason::StorageUsageObserved
            | RegulationReason::TripleCountObserved
            | RegulationReason::LowConfidenceCountObserved
            | RegulationReason::ConsolidationCandidatesObserved
            | RegulationReason::PendingEscalationsObserved => Some(RegulatoryAction::with_metric(
                proposed.target,
                proposed.action_type,
                RegulatoryActionParams::reason(proposed.reason.as_str()),
                dev.signal.metric.as_str().into(),
            )),
            // -- Meta-regulatory Escalate and domain-specific regulation.
            //    All have substitution ladders — try_substitute walks the
            //    ladder when the proposed action is stagnating. Actions
            //    carry NoData (no typed RegulationData variant for these
            //    reasons yet) and the metric name for impact verification. --
            RegulationReason::AlgedonicEventsExceeded
            | RegulationReason::AlgedonicLogApproachingCap
            | RegulationReason::GoalsStale
            | RegulationReason::GoalsExpired
            | RegulationReason::MetacognitionCriticalAlerts
            | RegulationReason::MemoryLifeLow
            | RegulationReason::CircuitBreakerOpen
            | RegulationReason::InferenceUnavailable
            | RegulationReason::ModelUnavailable => {
                let at = self
                    .try_substitute(dev.signal.metric, proposed.action_type)
                    .await;
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::reason(proposed.reason.as_str()),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // OcrSilentFailuresExceeded carries the storm count so
            // verify_impact can re-sense and compare as entries age out of
            // the window, and auto_resolve_cleared can close the escalation
            // when the storm ends — the same typed-data pattern as
            // ContextServerFleetDegraded below.
            RegulationReason::OcrSilentFailuresExceeded => {
                let at = self
                    .try_substitute(dev.signal.metric, proposed.action_type)
                    .await;
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(
                        proposed.reason.as_str(),
                        RegulationData::OcrSilentFailuresExceeded {
                            count: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // ContextServerFleetDegraded carries typed fleet-health data so
            // verify_impact can re-sense and compare, and extract_deficit_threshold
            // can populate the error_context with real counts instead of (0, 0).
            RegulationReason::ContextServerFleetDegraded => {
                let at = self
                    .try_substitute(dev.signal.metric, proposed.action_type)
                    .await;
                let data = if let Some(ref source) = self.context_server_health_source {
                    let healthy = source.healthy_count().await as u64;
                    let total = source.total_count().await as u64;
                    RegulationData::ContextServerFleetHealth {
                        healthy_count: healthy,
                        total_count: total,
                    }
                } else {
                    // Source not wired — fall back to NoData. verify_impact
                    // will skip (warned), matching the pre-fix behavior.
                    RegulationData::NoData
                };
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    at,
                    RegulatoryActionParams::with_data(proposed.reason.as_str(), data),
                    dev.signal.metric.as_str().into(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::CyberneticsLoop;
    use crate::loops::{
        ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData,
        RegulatoryAction, RegulatoryActionParams, Signal, SignalMetric,
    };
    use crate::regulation_policy::RegulationPolicy;
    use crate::runtime::RegulationLedger;
    use crate::{RolloutEventError, RolloutEventSource};
    use std::sync::{Arc, Mutex};
    use tokio::sync::RwLock;

    /// A recorded `append_impact_verdict` call — exactly what the loop wrote back.
    #[derive(Debug, Clone, PartialEq)]
    struct RecordedVerdict {
        rollout_id: String,
        metric: String,
        before: f64,
        after: f64,
        improved: bool,
        decision: String,
    }

    /// A test double for `RolloutEventSource` that returns a configured
    /// `metric_before_and_after` result and records `append_impact_verdict`
    /// calls so the test can assert exactly what the loop persisted.
    ///
    /// `metric_before_and_after` is configurable (Ok(Some) / Ok(None) / Err)
    /// to exercise the three branches of the event-substrate path. The
    /// no-data and error branches must NOT record a verdict and must NOT fall
    /// through to the struct-walk fallback (B1).
    struct MockRolloutEventSource {
        /// `Err` holds only the failure detail; the typed
        /// [`RolloutEventError`] is built at the trait boundary so the mock
        /// does not force `Clone` onto the port's error type.
        before_after: Mutex<Result<Option<(f64, f64)>, String>>,
        verdicts: Mutex<Vec<RecordedVerdict>>,
    }

    impl MockRolloutEventSource {
        fn answering(before: f64, after: f64) -> Self {
            Self {
                before_after: Mutex::new(Ok(Some((before, after)))),
                verdicts: Mutex::new(Vec::new()),
            }
        }
        fn empty() -> Self {
            Self {
                before_after: Mutex::new(Ok(None)),
                verdicts: Mutex::new(Vec::new()),
            }
        }
        fn failing(error: &str) -> Self {
            Self {
                before_after: Mutex::new(Err(error.to_string())),
                verdicts: Mutex::new(Vec::new()),
            }
        }
        fn recorded(&self) -> Vec<RecordedVerdict> {
            self.verdicts.lock().expect("verdicts lock").clone()
        }
    }

    impl RolloutEventSource for MockRolloutEventSource {
        fn metric_before_and_after(
            &self,
            _rollout_id: &str,
            _metric: &str,
            _before_position: i64,
        ) -> Result<Option<(f64, f64)>, RolloutEventError> {
            self.before_after
                .lock()
                .expect("before_after lock")
                .clone()
                .map_err(|detail| RolloutEventError::Query { detail })
        }
        fn append_impact_verdict(
            &self,
            rollout_id: &str,
            metric: &str,
            before: f64,
            after: f64,
            improved: bool,
            decision: &str,
        ) -> Result<(), RolloutEventError> {
            self.verdicts
                .lock()
                .expect("verdicts lock")
                .push(RecordedVerdict {
                    rollout_id: rollout_id.to_string(),
                    metric: metric.to_string(),
                    before,
                    after,
                    improved,
                    decision: decision.to_string(),
                });
            Ok(())
        }
    }

    fn loop_with_source<S: RolloutEventSource + 'static>(source: Arc<S>) -> CyberneticsLoop {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        CyberneticsLoop::new(ledger).with_rollout_event_source(source)
    }

    /// A recording `AlertEscalationSink` — captures the exact strings the
    /// loop persisted and auto-resolved so the two message-format sites
    /// can be asserted byte-identical.
    struct RecordingEscalationSink {
        persisted: Mutex<Vec<String>>,
        auto_resolved: Mutex<Vec<String>>,
    }

    impl RecordingEscalationSink {
        fn new() -> Self {
            Self {
                persisted: Mutex::new(Vec::new()),
                auto_resolved: Mutex::new(Vec::new()),
            }
        }
    }

    impl crate::AlertEscalationSink for RecordingEscalationSink {
        fn persist_alert(&self, output: &str, _confidence: f64, _error_context: &str) {
            self.persisted
                .lock()
                .expect("persisted lock")
                .push(output.to_string());
        }
        fn auto_resolve_cleared(&self, output: &str, _resolution_note: &str) {
            self.auto_resolved
                .lock()
                .expect("auto_resolved lock")
                .push(output.to_string());
        }
    }

    /// expect: "Immediate acceptance of advice does not prove that an alert cleared" [P9]
    #[tokio::test]
    async fn accepted_observation_does_not_resolve_alert() {
        let sink = Arc::new(RecordingEscalationSink::new());
        let mut regulation_loop =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation_loop.set_alert_escalation_sink(Some(sink.clone()));
        let action = RegulatoryAction::new(
            LoopId::Curation,
            ActionType::Escalate,
            RegulatoryActionParams::with_data(
                "energy_budget_low",
                RegulationData::EnergyBudgetLow {
                    remaining_ratio: 0.15,
                    set_point: 0.20,
                },
            ),
        );
        regulation_loop.route_action_as_alert(&action).await;
        regulation_loop.verify_impact(&[action]).await;
        assert_eq!(sink.persisted.lock().expect("persisted").len(), 1);
        assert!(sink.auto_resolved.lock().expect("resolved").is_empty());
    }

    fn rollout_impact_check(rollout_id: &str, metric: &str) -> RegulatoryAction {
        RegulatoryAction::new(
            LoopId::Curation,
            ActionType::Notify,
            RegulatoryActionParams::with_data(
                "rollout_impact_check",
                RegulationData::RolloutImpactCheck {
                    rollout_id: rollout_id.to_string(),
                    before_position: 1,
                    metric: metric.to_string(),
                },
            ),
        )
    }

    /// S1 + happy path: a store-answered pass_rate regression writes a verdict
    /// event labeled with the REAL metric name ("pass_rate"), not the
    /// `SignalMetric` fallback ("energy_remaining"). Before the fix the
    /// write-back passed `metric.as_str()`, which fell through
    /// `from_str_name("pass_rate").unwrap_or(EnergyRemaining)` →
    /// `"energy_remaining"` — self-describing JSON that lied about its
    /// content.
    #[test]
    fn verify_impact_write_back_records_real_metric_name() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let source = Arc::new(MockRolloutEventSource::answering(0.8, 0.5));
            let regulation_loop = loop_with_source(Arc::clone(&source));
            let reports = regulation_loop
                .verify_impact(&[rollout_impact_check("alpha", "pass_rate")])
                .await;
            assert_eq!(
                reports.len(),
                1,
                "a store-answered check produces one report"
            );
            let verdicts = source.recorded();
            assert_eq!(verdicts.len(), 1, "the impact verdict is written back once");
            let verdict = &verdicts[0];
            assert_eq!(verdict.rollout_id, "alpha");
            // S1: the verdict records the real metric name, not the SignalMetric
            // fallback. Before the fix this asserted "energy_remaining".
            assert_eq!(verdict.metric, "pass_rate");
            assert_eq!(verdict.before, 0.8);
            assert_eq!(verdict.after, 0.5);
            assert!(!verdict.improved, "a pass-rate drop is not an improvement");
            assert!(
                matches!(verdict.decision.as_str(), "Accept" | "Stage" | "Block"),
                "decision is a known ActionDecision variant, got {}",
                verdict.decision
            );
        });
    }

    /// B1 (no-data): a submitted impact check the store can't answer must NOT
    /// silently fall through to the struct-walk (which `continue`s for
    /// `RolloutImpactCheck`) with no signal. It skips the action — no report,
    /// no verdict write-back. Before the fix the `Ok(None)` was swallowed by
    /// `if let Ok(Some(..))` and the check vanished.
    #[test]
    fn verify_impact_store_no_data_skips_without_verdict() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let source = Arc::new(MockRolloutEventSource::empty());
            let regulation_loop = loop_with_source(Arc::clone(&source));
            let reports = regulation_loop
                .verify_impact(&[rollout_impact_check("alpha", "pass_rate")])
                .await;
            assert!(reports.is_empty(), "no report when the store has no data");
            assert!(
                source.recorded().is_empty(),
                "no verdict written back when the store has no data"
            );
        });
    }

    /// B1 (error): a store error must be surfaced (warned) and skip the action,
    /// not be silently discarded by the `if let Ok(Some(..))` swallowing the
    /// `Err`. Before the fix the error was dropped with no warn and no report.
    #[test]
    fn verify_impact_store_error_skips_without_verdict() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let source = Arc::new(MockRolloutEventSource::failing("store down"));
            let regulation_loop = loop_with_source(Arc::clone(&source));
            let reports = regulation_loop
                .verify_impact(&[rollout_impact_check("alpha", "pass_rate")])
                .await;
            assert!(reports.is_empty(), "no report when the store errors");
            assert!(
                source.recorded().is_empty(),
                "no verdict written back on a store error"
            );
        });
    }

    /// Pins the ToolReliabilityDegraded arm of the verify_impact struct-walk.
    /// Before the fix the typed variant fell into the NoData catch-all and
    /// every tool_reliability_degraded escalation skipped verification — the
    /// live-observed "verify_impact: action carries NoData ... skipping" warn
    /// on every tick. The arm re-senses the after-value from the ledger's
    /// tracked outcomes using the same equal-weighted aggregation as
    /// ToolReliabilitySensor::sense.
    #[test]
    fn verify_impact_reliability_action_verifies_against_ledger() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));

            let reliability_action = || {
                RegulatoryAction::new(
                    LoopId::Curation,
                    ActionType::Escalate,
                    RegulatoryActionParams::with_data(
                        "tool_reliability_degraded",
                        RegulationData::ToolReliabilityDegraded {
                            reliability: 0.6667,
                            threshold: 0.8,
                        },
                    ),
                )
            };

            // Empty ledger: no tracked domains is no data — skip without a
            // report (not a 0.0 after-value, which would read as "fully
            // degraded", the .rules unwrap_or(0) trap).
            let reports = regulation_loop.verify_impact(&[reliability_action()]).await;
            assert!(reports.is_empty(), "no tracked domains → no report");

            // Seed one domain to 100% success (4/4). The re-sensed
            // after-value (1.0) improves over the before-value (0.6667).
            {
                let ledger_guard = ledger.read().await;
                for _ in 0..4 {
                    ledger_guard
                        .record_outcome("reliability_verify_test", true, None)
                        .await;
                }
            }
            let reports = regulation_loop.verify_impact(&[reliability_action()]).await;
            assert_eq!(reports.len(), 1, "seeded ledger → one report");
            let report = &reports[0];
            assert_eq!(report.metric, SignalMetric::ToolReliability);
            assert!(
                (report.before - 0.6667).abs() < 1e-9,
                "before is the escalation-time reliability"
            );
            assert!(
                (report.after - 1.0).abs() < 1e-9,
                "after is re-sensed from the ledger"
            );
            assert!(report.improved, "1.0 > 0.6667 is an improvement");
            assert_eq!(report.decision, ActionDecision::Accept);
        });
    }

    /// Pins Fix 2: every RegulationReason that has a policy rule must produce
    /// Some(action) from build_regulation_action — not None via a catch-all.
    /// Before the fix, 18 of 30 reasons fell through `_ => None`, silently
    /// dropping actions and leaving the loop open for those metrics. The
    /// match is now exhaustive (no `_ =>` arm), so the compiler enforces
    /// closure at compile time. This test verifies the behavioral side:
    /// each newly-handled arm actually returns Some(action).
    #[test]
    fn build_regulation_action_produces_action_for_all_new_reasons() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let regulation_loop = loop_with_source(Arc::new(MockRolloutEventSource::empty()));
            let policy = RegulationPolicy::default();

            // (metric, direction, value, set_point) for each newly-handled rule.
            // AboveSetPoint: value > set_point. BelowSetPoint: value < set_point.
            use DeviationDirection::*;
            use SignalMetric::*;
            let cases: &[(SignalMetric, DeviationDirection, f64, f64)] = &[
                // Category A: Observational (Notify, AboveSetPoint)
                (StorageUsage, AboveSetPoint, 1.0, 0.0),
                (TripleCount, AboveSetPoint, 1.0, 0.0),
                (LowConfidenceCount, AboveSetPoint, 1.0, 0.0),
                (ConsolidationCandidates, AboveSetPoint, 1.0, 0.0),
                (PendingEscalations, AboveSetPoint, 1.0, 0.0),
                // Category B: Meta-regulatory (Escalate, AboveSetPoint)
                (AlgedonicEvents, AboveSetPoint, 1.0, 0.0),
                (AlgedonicLogApproachingCap, AboveSetPoint, 1.0, 0.0),
                (GoalStaleCount, AboveSetPoint, 1.0, 0.0),
                (GoalExpiredCount, AboveSetPoint, 1.0, 0.0),
                (MetacognitionCriticalAlerts, AboveSetPoint, 1.0, 0.0),
                // Category C: Domain-specific
                (MemoryLife, BelowSetPoint, 0.0, 1.0),
                (CircuitBreakerState, AboveSetPoint, 1.0, 0.0),
                (InferenceAvailable, BelowSetPoint, 0.0, 1.0),
                (InferenceModelAvailable, BelowSetPoint, 0.0, 1.0),
                (ContextServerHealth, BelowSetPoint, 0.0, 1.0),
                (OcrSilentFailures, AboveSetPoint, 14.0, 0.0),
            ];

            for &(metric, direction, value, set_point) in cases {
                let signal = Signal::new(LoopId::Cybernetics, metric, value, set_point);
                let deviation = Deviation::from_signal(&signal)
                    .unwrap_or_else(|| panic!("{metric:?} should deviate from set_point"));
                assert_eq!(
                    deviation.direction, direction,
                    "{metric:?} deviation direction mismatch"
                );
                let proposed = policy.decide(&deviation);
                assert!(
                    !proposed.is_empty(),
                    "{metric:?} {:?} must have a policy rule",
                    direction
                );
                for p in proposed {
                    let action = regulation_loop.build_regulation_action(&deviation, p).await;
                    assert!(
                        action.is_some(),
                        "{metric:?} {:?} build_regulation_action returned None for reason {:?}",
                        direction,
                        p.reason
                    );
                }
            }
        });
    }

    /// Pins F8 + B1: compute() must NOT include Notify actions in the
    /// returned vector. Notify actions are observational — they signal
    /// "metric observed" but carry no efferent action. Including them
    /// would inflate gain beyond 1.0 (B1) and they'd be silently dropped
    /// by route_action_as_alert (F8). The fix logs them in compute() and
    /// excludes them from the actions vector.
    #[test]
    fn compute_excludes_notify_actions() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let regulation_loop = loop_with_source(Arc::new(MockRolloutEventSource::empty()));
            // StorageUsage AboveSetPoint triggers a Notify rule.
            let signal = Signal::new(LoopId::Cybernetics, SignalMetric::StorageUsage, 1.0, 0.0);
            let deviation = Deviation::from_signal(&signal)
                .expect("StorageUsage 1.0 vs set_point 0.0 should deviate");
            let actions = regulation_loop.compute(&[deviation]).await;
            assert!(
                actions.iter().all(|a| a.action_type != ActionType::Notify),
                "compute() must not return Notify actions — they are observational, not regulatory"
            );
        });
    }

    /// Pins B2: fidelity matching must use metric_name only, not string
    /// fallback on reason. An action without metric_name but with a reason
    /// that would have matched under the old fallback (e.g., "low" matching
    /// "energy_budget_low") must NOT count as a match. Before the fix, the
    /// string fallback produced false positives by conflating direction
    /// semantics ("low" matched both "energy_budget_low" and
    /// "low_confidence_count").
    #[test]
    fn from_cycle_fidelity_no_string_fallback() {
        use crate::loops::core::{LoopMetrics, TriggerOrigin};
        let signal = Signal::new(LoopId::Cybernetics, SignalMetric::EnergyRemaining, 0.1, 0.2);
        let deviation = Deviation::from_signal(&signal).unwrap();
        // An action with no metric_name but a reason that contains "low" —
        // under the old fallback this would have matched EnergyRemaining
        // BelowSetPoint via reason.contains("low").
        let action = RegulatoryAction::new(
            LoopId::Curation,
            ActionType::Escalate,
            RegulatoryActionParams::reason("some_unrelated_low_thing"),
        );
        let metrics =
            LoopMetrics::from_cycle(0, &[deviation], &[action], &[], TriggerOrigin::Scheduled);
        assert_eq!(
            metrics.fidelity_score, 0.0,
            "action without metric_name must not match via string fallback"
        );
    }

    /// D-sensing (2026-08-30): the algedonic log's population state must be
    /// sensed — `AlgedonicEvents`, `PendingEscalations`, and
    /// `MetacognitionCriticalAlerts` were policy-only (rules that could never
    /// fire) before this. A clean log emits none of the three; a critical
    /// outcome alert (0% success → Critical, escalated) emits all three with
    /// value 1.0 against set-point 0.0.
    #[test]
    fn sense_reports_algedonic_log_population() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));

            // Clean log: none of the three population signals.
            let signals = regulation_loop.sense().await;
            assert!(
                !signals
                    .iter()
                    .any(|s| s.metric == SignalMetric::AlgedonicEvents),
                "clean log must not emit AlgedonicEvents"
            );
            assert!(
                !signals
                    .iter()
                    .any(|s| s.metric == SignalMetric::PendingEscalations),
                "clean log must not emit PendingEscalations"
            );
            assert!(
                !signals
                    .iter()
                    .any(|s| s.metric == SignalMetric::MetacognitionCriticalAlerts),
                "clean log must not emit MetacognitionCriticalAlerts"
            );

            // An Info-only log must NOT emit AlgedonicEvents: Info entries
            // are healthy-range diagnostics (a variety check slightly below
            // expected), not review demands. Before the actionable-count
            // fix, any log population — including normal-use Info noise —
            // fired the sensor as a standing escalation.
            {
                let ledger_guard = ledger.read().await;
                ledger_guard
                    .increment_variety("variety_info_test", "only_tool")
                    .await;
                let log_count = ledger_guard.alert_log_count().await;
                drop(ledger_guard);
                assert!(
                    log_count > 0,
                    "sanity: the variety check pushed an Info alert"
                );
            }
            let signals = regulation_loop.sense().await;
            assert!(
                !signals
                    .iter()
                    .any(|s| s.metric == SignalMetric::AlgedonicEvents),
                "an Info-only log must not emit AlgedonicEvents"
            );

            // A critical outcome alert: 0% success over 5 operations
            // (the minimum sample `check_outcome` evaluates) → Critical,
            // escalated. `record_outcome` re-checks thresholds on every record,
            // so the alert fires on the fifth failure.
            let ledger_guard = ledger.read().await;
            for _ in 0..5 {
                ledger_guard.record_outcome("sense_test", false, None).await;
            }
            let alert_count = ledger_guard.alert_log_count().await;
            drop(ledger_guard);
            assert!(alert_count > 0, "0% success must produce an alert");

            let signals = regulation_loop.sense().await;
            for (metric, expected_value) in [
                (SignalMetric::AlgedonicEvents, 1.0),
                (SignalMetric::PendingEscalations, 1.0),
                (SignalMetric::MetacognitionCriticalAlerts, 1.0),
            ] {
                let signal = signals
                    .iter()
                    .find(|s| s.metric == metric)
                    .unwrap_or_else(|| panic!("{metric:?} must be sensed"));
                assert_eq!(signal.value, expected_value);
                assert_eq!(signal.set_point, 0.0);
            }
        });
    }

    /// `record_variety` is the dispatch twin of `record_outcome`: one call
    /// per governed tool invocation, tool name as the observed state. It
    /// feeds the ledger's variety trackers — the VarietySensor's data
    /// source. Repeats of the same tool count once (variety is distinct
    /// tools, not call volume).
    #[test]
    fn record_variety_feeds_ledger_trackers() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));
            regulation_loop
                .record_variety("hkask-mcp-media", "gallery_search")
                .await;
            regulation_loop
                .record_variety("hkask-mcp-media", "gallery_search")
                .await;
            regulation_loop
                .record_variety("hkask-mcp-media", "gallery_add_audio")
                .await;
            let ledger_guard = ledger.read().await;
            assert_eq!(
                ledger_guard.variety_for_domain("hkask-mcp-media").await,
                2,
                "distinct tool names are the variety — repeats don't count"
            );
        });
    }

    struct StubHealthSource;

    #[async_trait::async_trait]
    impl crate::sensor_provider::InferenceHealthSource for StubHealthSource {
        async fn in_flight(&self) -> usize {
            0
        }
        async fn max_concurrency(&self) -> usize {
            96
        }
        async fn recent_timeout_count(&self) -> u64 {
            0
        }
    }

    /// `InferenceModelAvailable` (2026-08-30): an unwired inference health
    /// source means the default model never resolved — unusable inference.
    /// The signal must stay silent during the boot grace window (slow
    /// registry population is not an outage), fire after it, and clear the
    /// moment the source is wired.
    #[test]
    fn sense_reports_unwired_inference_model_after_grace() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let mut regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));

            // Before the grace period: no signal — the deferred task often
            // wires the model a few seconds after the first ticks.
            let signals = regulation_loop.sense().await;
            assert!(
                !signals
                    .iter()
                    .any(|s| s.metric == SignalMetric::InferenceModelAvailable),
                "grace window must not report a model outage"
            );

            // After 3 ticks with no wired source: model-unavailable.
            regulation_loop
                .tick_count
                .store(3, std::sync::atomic::Ordering::Relaxed);
            let signals = regulation_loop.sense().await;
            let signal = signals
                .iter()
                .find(|s| s.metric == SignalMetric::InferenceModelAvailable)
                .expect("unwired inference must be sensed after grace");
            assert_eq!(signal.value, 0.0);
            assert_eq!(signal.set_point, 1.0);

            // Once the source is wired (model resolved): no signal.
            regulation_loop.set_inference_health_source(Arc::new(StubHealthSource));
            let signals = regulation_loop.sense().await;
            assert!(
                !signals.iter().any(|s| s.metric == SignalMetric::InferenceModelAvailable && Deviation::from_signal(s).is_some()),
                "wired source means the model resolved — no outage deviation"
            );
        });
    }

    /// Capturing RegulationSink — records every persisted span's path and
    /// observation so the tick-emission policy can be asserted without a
    /// durable archive.
    struct CapturingSink(Mutex<Vec<(String, serde_json::Value)>>);

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

    /// Idle cycles emit exactly one heartbeat span per hour (tick 1, then
    /// every 360 ticks) carrying the all-zero payload plus `heartbeat: true`
    /// and `tick_count`. Without it, a converged loop and a dead ticker are
    /// indistinguishable — both produce archive silence.
    #[test]
    fn idle_loop_emits_hourly_heartbeat_span() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>)
                .with_inference_health_source(Arc::new(StubHealthSource));

            // 361 ticks: heartbeat at tick 1, silence through tick 359,
            // heartbeat at tick 360, tick 361 silent again.
            for _ in 0..361 {
                regulation_loop.tick().await;
            }

            let spans = sink.0.lock().unwrap_or_else(|e| e.into_inner());
            assert_eq!(
                spans.len(),
                2,
                "361 idle ticks must emit exactly 2 spans (tick 1 + tick 360)"
            );
            for (index, (path, observation)) in spans.iter().enumerate() {
                assert_eq!(path, "reg.outcome.loop_quality");
                assert_eq!(
                    observation.get("heartbeat"),
                    Some(&serde_json::json!(true)),
                    "idle span {index} must be a heartbeat"
                );
                assert_eq!(
                    observation.get("deviations").and_then(|v| v.as_u64()),
                    Some(0),
                    "the zeros are the health reading"
                );
                assert_eq!(observation.get("actions").and_then(|v| v.as_u64()), Some(0));
                assert_eq!(
                    observation.get("impact_reports").and_then(|v| v.as_u64()),
                    Some(0)
                );
            }
            assert_eq!(
                spans[0].1.get("tick_count").and_then(|v| v.as_u64()),
                Some(1),
                "first heartbeat is the boot announcement"
            );
            assert_eq!(
                spans[1].1.get("tick_count").and_then(|v| v.as_u64()),
                Some(360),
                "second heartbeat lands on the hourly boundary"
            );
        });
    }

    /// Signal-bearing cycles emit the normal telemetry span WITHOUT the
    /// heartbeat discriminator — the flag must never leak into real signal
    /// events, or triage would read a live deviation as an idle heartbeat.
    /// Ticks 1-3 sit inside the model-wiring grace window (idle; tick 1 is
    /// the boot heartbeat); ticks 4-5 sense the unwired-model deviation.
    #[test]
    fn signal_ticks_emit_telemetry_without_heartbeat_flag() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
            // No inference health source: after the 3-tick grace the loop
            // senses the model-unavailable deviation on every tick.
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);

            for _ in 0..5 {
                regulation_loop.tick().await;
            }

            let spans = sink.0.lock().unwrap_or_else(|e| e.into_inner());
            let heartbeats: Vec<&serde_json::Value> = spans
                .iter()
                .filter(|(path, observation)| {
                    path == "reg.outcome.loop_quality" && observation.get("heartbeat").is_some()
                })
                .map(|(_, observation)| observation)
                .collect();
            let signal_telemetry: Vec<&serde_json::Value> = spans
                .iter()
                .filter(|(path, observation)| {
                    path == "reg.outcome.loop_quality" && observation.get("heartbeat").is_none()
                })
                .map(|(_, observation)| observation)
                .collect();
            assert_eq!(
                heartbeats.len(),
                1,
                "tick 1 is the boot heartbeat; ticks 2-3 are silent (grace, idle)"
            );
            assert_eq!(
                heartbeats[0].get("tick_count").and_then(|v| v.as_u64()),
                Some(1)
            );
            assert!(
                !signal_telemetry.is_empty(),
                "post-grace ticks must emit signal telemetry"
            );
            assert_eq!(
                signal_telemetry[0]
                    .get("deviations")
                    .and_then(|v| v.as_u64()),
                Some(1),
                "tick 4 (first post-grace tick) senses exactly the model deviation"
            );
            assert!(
                signal_telemetry.iter().all(|observation| {
                    observation
                        .get("deviations")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        >= 1
                }),
                "signal telemetry carries deviations, never the heartbeat flag"
            );
        });
    }
}
