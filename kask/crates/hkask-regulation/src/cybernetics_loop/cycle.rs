//! The regulation cycle — sense → compare → compute → act → verify.
//!
//! Extracted from the cybernetics_loop god-module. The facade's `tick`
//! orchestrates these phases; each phase is `pub(super)` so the facade can
//! call it. Action construction (`build_regulation_action`), alert routing
//! (`route_action_as_alert`), and the cycle-internal helpers
//! (`persist_alert_to_queue`) is private to this module.

use crate::algedonic::{AlertSeverity, RuntimeAlert};
use crate::loops::RegulationData;
use crate::loops::{
    ActionDecision, ActionType, CurationInput, Deviation, ImpactReport, LoopId, RegulatoryAction,
    RegulatoryActionParams, Signal, SignalMetric,
};
use crate::regulation_policy::{
    self, RegulationPolicy, RegulationReason, classify_decision, extract_deficit_threshold,
};
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};

impl super::CyberneticsLoop {
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
    async fn persist_alert_to_queue(
        &self,
        alert: &RuntimeAlert,
        recovery_signal: Option<&Signal>,
    ) -> bool {
        let Some(ref sink) = self.alert_escalation_sink else {
            return false;
        };
        // Skip non-escalated alerts — only escalated alerts (Critical, or
        // Warning with `escalated: true`) belong in the reviewable backlog.
        // Info alerts and non-escalated Warnings are diagnostic, not
        // actionable, and would pollute the queue.
        if !alert.escalated {
            return true;
        }
        let confidence = if alert.is_critical() { 1.0 } else { 0.5 };
        let mut error_context = serde_json::json!({
            "domain": alert.domain,
            "deficit": alert.deficit,
            "threshold": alert.threshold,
            "severity": alert.severity,
            "escalated": alert.escalated,
            "recovery_signal": recovery_signal,
            "timestamp": alert.timestamp.to_rfc3339(),
        });
        // Tool-reliability alerts carry the per-domain outcome breakdown so
        // triage can name the failing domain from the escalation row itself —
        // the queue is the one surface the Curator reviews, and the aggregate
        // success rate in the message cannot name a domain. Covers both
        // alert shapes: the degradation alert (identified by its recovery
        // signal's metric) and the plateau alert (domain
        // "regulatory_plateau:tool_reliability", which carries no recovery
        // signal).
        let tool_reliability_related = recovery_signal
            .is_some_and(|signal| signal.metric == SignalMetric::ToolReliability)
            || alert.domain.contains("tool_reliability");
        if tool_reliability_related {
            let breakdown = self.ledger.read().await.outcome_breakdown().await;
            if !breakdown.is_empty() {
                error_context["outcome_breakdown"] = serde_json::json!(breakdown);
            }
        }
        match sink.try_persist_alert(&alert.message, confidence, &error_context.to_string()) {
            Ok(crate::AlertQueueOutcome::Confirmed(_)) => true,
            Ok(crate::AlertQueueOutcome::Attempted) => false,
            Err(error) => {
                tracing::warn!(
                    target: "reg.alert",
                    error = %error,
                    domain = %alert.domain,
                    "Failed to persist alert to the reviewable escalation queue"
                );
                false
            }
        }
    }

    /// Emit the per-domain tool-outcome breakdown span
    /// (`reg.outcome.tool_domains`) — the diagnosis surface that names the
    /// failing domain(s) with per-kind error tallies, retrievable from the
    /// algedonic log via the curator MCP tools. Called when a
    /// tool-reliability alert fires (degradation or plateau) and on the
    /// hourly heartbeat, so the breakdown is available both at deviation
    /// time and as a periodic snapshot while healthy.
    pub(super) async fn emit_tool_outcome_breakdown(&self) {
        let breakdown = self.ledger.read().await.outcome_breakdown().await;
        if breakdown.is_empty() {
            return;
        }
        self.emit_regulation_span(
            SpanKind::ToolOutcomeBreakdown,
            serde_json::json!({ "domains": breakdown }),
        )
        .await;
    }

    /// Compare: detect deviations from set-points.
    pub(super) async fn compare(&self, signals: &[Signal]) -> Vec<Deviation> {
        signals.iter().filter_map(Deviation::from_signal).collect()
    }

    async fn sense_inference_resilience(&self) -> Vec<Signal> {
        enum ResilienceEvent {
            Intervention(crate::InferenceInterventionReceipt),
            PermanentFailure(crate::InferencePermanentFailureReceipt),
        }

        impl ResilienceEvent {
            fn id(&self) -> u64 {
                match self {
                    Self::Intervention(receipt) => receipt.id,
                    Self::PermanentFailure(receipt) => receipt.id,
                }
            }
        }

        let Some(source) = &self.inference_resilience_source else {
            return Vec::new();
        };
        let cursor = self
            .inference_intervention_cursor
            .load(std::sync::atomic::Ordering::Relaxed);
        let observation = match source.observe_since(cursor).await {
            Ok(observation) => observation,
            Err(error) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %error,
                    "inference resilience observation failed"
                );
                return Vec::new();
            }
        };
        let mut events: Vec<_> = observation
            .interventions
            .iter()
            .cloned()
            .map(ResilienceEvent::Intervention)
            .chain(
                observation
                    .permanent_failures
                    .iter()
                    .cloned()
                    .map(ResilienceEvent::PermanentFailure),
            )
            .collect();
        events.sort_unstable_by_key(ResilienceEvent::id);

        let mut acknowledged_cursor = cursor;
        let mut recovery_failed = false;
        for event in events {
            let event_id = event.id();
            let reopened = matches!(
                &event,
                ResilienceEvent::Intervention(receipt)
                    if receipt.kind == crate::InferenceInterventionKind::CircuitReopened
            );
            if event_id != acknowledged_cursor.saturating_add(1) {
                tracing::warn!(
                    target: "reg.inference",
                    expected_id = acknowledged_cursor.saturating_add(1),
                    observed_id = event_id,
                    source_next_cursor = observation.next_cursor,
                    "inference receipt sequence contains a gap — cursor not advanced"
                );
                break;
            }

            let handled = match event {
                ResilienceEvent::Intervention(receipt) => {
                    let escalation_handled =
                        if receipt.kind == crate::InferenceInterventionKind::CircuitReopened {
                            let action = RegulatoryAction::with_metric(
                                LoopId::Curation,
                                ActionType::Escalate,
                                RegulatoryActionParams::reason("circuit_breaker_open"),
                                SignalMetric::CircuitBreakerState.as_str().to_string(),
                            );
                            self.route_action_as_alert(&action).await
                        } else {
                            true
                        };
                    if !escalation_handled {
                        false
                    } else {
                        let kind =
                            if receipt.kind == crate::InferenceInterventionKind::CircuitClosed {
                                SpanKind::InferenceObservedRecovery
                            } else {
                                SpanKind::InferenceCircuitTransition
                            };
                        if let Some(sink) = &self.event_sink {
                            let regulation_event = RegulationRecord::new(
                                WebID::from_persona(b"regulation"),
                                Span::from_kind(kind),
                                CyclePhase::Sense,
                                serde_json::json!({
                                    "intervention_id": receipt.id,
                                    "transition": receipt.kind,
                                    "occurred_at": receipt.occurred_at,
                                    "observed_at": observation.snapshot.observed_at,
                                    "circuit_state": observation.snapshot.circuit_state,
                                    "observed_recovery": receipt.kind
                                        == crate::InferenceInterventionKind::CircuitClosed,
                                    "causal_attribution": "unverified",
                                }),
                                0,
                            );
                            match sink.persist(&regulation_event) {
                                Ok(()) => true,
                                Err(error) => {
                                    tracing::warn!(
                                        target: "reg.inference",
                                        error = %error,
                                        intervention_id = receipt.id,
                                        "inference intervention receipt persistence failed"
                                    );
                                    false
                                }
                            }
                        } else {
                            tracing::warn!(
                                target: "reg.inference",
                                intervention_id = receipt.id,
                                "inference intervention receipt not acknowledged — no event sink configured"
                            );
                            false
                        }
                    }
                }
                ResilienceEvent::PermanentFailure(failure) => {
                    let action = RegulatoryAction::with_metric(
                        LoopId::Curation,
                        ActionType::Escalate,
                        RegulatoryActionParams::reason(format!(
                            "inference_permanent_failure:{:?} — {}",
                            failure.kind, failure.detail
                        )),
                        "inference_permanent_failure".to_string(),
                    );
                    self.route_action_as_alert(&action).await
                }
            };

            if !handled {
                break;
            }
            recovery_failed |= reopened;
            acknowledged_cursor = event_id;
        }
        self.inference_intervention_cursor
            .store(acknowledged_cursor, std::sync::atomic::Ordering::Relaxed);

        vec![Signal::new(
            LoopId::Inference,
            SignalMetric::CircuitBreakerState,
            if recovery_failed { 1.0 } else { 0.0 },
            0.0,
        )]
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
        signals.extend(self.sense_inference_resilience().await);

        // Sense the in-memory algedonic log cap. When the log approaches its
        // cap, emit a signal so the operator (or the `algedonic-review` skill)
        // can review and clear reviewed entries before they are evicted unread.
        // The set-point is 0.0 — any positive value (1.0 = approaching cap) is
        // a deviation.
        signals.push(Signal::new(
            LoopId::Cybernetics,
            SignalMetric::AlgedonicLogApproachingCap,
            if self.ledger.read().await.alert_log_approaching_cap().await {
                1.0
            } else {
                0.0
            },
            0.0,
        ));

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
                if self.inference_health_wired {
                    1.0
                } else {
                    0.0
                },
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
                // Predictive threshold proximity is telemetry, not a policy
                // disposition; record it without synthesizing a deviation.
            }
        }

        let policy = RegulationPolicy::default();

        for dev in deviations {
            for proposed in policy.decide(dev) {
                let action = self.build_regulation_action(dev, proposed).await;
                if let Some(disposition) = action {
                    actions.push(disposition);
                }
            }
        }
        actions
    }

    pub(super) async fn act(&self, actions: &[RegulatoryAction]) {
        // E04: capture call-cap exhaustion BEFORE the per-tick reset — the
        // reset replenishes every cap (remaining = ceiling), so reading
        // after it would never observe remaining == 0 and the exhaustion
        // alert could never fire (the pre-fix defect: the alert was dead
        // code). Charges land between ticks (metered dispatches), so the
        // pre-reset read is the only moment the exhaustion is observable.
        let exhausted: Vec<_> = {
            let statuses = self
                .call_cap_manager
                .read()
                .await
                .all_agent_statuses()
                .await;
            statuses
                .into_iter()
                .filter(|(_, status)| status.remaining == 0)
                .collect()
        };
        self.reset_all_caps().await;

        // E04: Detect and escalate call-cap exhaustion via the algedonic pathway.
        // A cap is exhausted when its remaining count hit zero this tick.
        {
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
                self.persist_alert_to_queue(&alert, None).await;
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

    /// Route a real escalation disposition through the durable queue, live
    /// Curator channel, archive, and email fallback. Informational dispositions
    /// remain observations and are not promoted to incidents. Returns true only
    /// when a pending condition or durable sink confirms the evidence is retained.
    async fn route_action_as_alert(&self, action: &RegulatoryAction) -> bool {
        if action.action_type == ActionType::Notify {
            tracing::info!(
                target: "reg.cybernetics",
                metric = action.metric_name.as_deref().unwrap_or("unknown"),
                "Notify disposition observed"
            );
            return true;
        }
        if action.target != LoopId::Curation {
            tracing::warn!(
                target: "reg.cybernetics",
                target_loop = %action.target,
                "Escalate disposition rejected because its target is not Curation"
            );
            return false;
        }

        let message =
            regulation_policy::alert_message(&action.parameters.data, &action.parameters.reason);
        let (deficit, threshold) =
            extract_deficit_threshold(&action.parameters.data).unwrap_or((1, 1));
        let alert = RuntimeAlert {
            domain: action.parameters.reason.clone(),
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
                    "Suppressing duplicate escalation — pending condition already in queue"
                );
                return true;
            }
        }

        // Persist to the reviewable escalation queue unconditionally —
        // the queue is the primary durable path for alert review, not
        // a fallback. The RegulationArchive below remains as a
        // secondary fallback for restart durability when the live
        // channel is down.
        let observation = action
            .metric_name
            .as_deref()
            .and_then(SignalMetric::from_str_name)
            .and_then(|metric| self.observations.lock().get(&metric).cloned());
        // Tool-reliability alerts also emit the per-domain breakdown span so
        // the algedonic log names the failing domain at alert time — the
        // escalation row carries the same breakdown in its error_context.
        if observation
            .as_ref()
            .is_some_and(|signal| signal.metric == SignalMetric::ToolReliability)
        {
            self.emit_tool_outcome_breakdown().await;
        }
        let queue_confirmed = self
            .persist_alert_to_queue(&alert, observation.as_ref())
            .await;

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

        // Fallback: persist the full alert to RegulationArchive when the live
        // channel is unavailable. The returned status lets receipt consumers
        // distinguish durable handling from best-effort delivery.
        let mut archive_persisted = false;
        if !sent_live {
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
                        "timestamp": alert.timestamp.to_rfc3339(),
                    }),
                    0,
                );
                match sink.persist(&event) {
                    Ok(()) => {
                        archive_persisted = true;
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
                if archive_persisted {
                    tracing::info!(target: "reg.alert", deficit = deficit, threshold = threshold, "Algedonic alert emailed as notification (live channel down, archive persisted)");
                } else {
                    tracing::info!(target: "reg.alert", deficit = deficit, threshold = threshold, "Algedonic alert emailed as last resort (archive unavailable)");
                }
            } else if !archive_persisted {
                tracing::error!(target: "reg.alert", deficit = deficit, threshold = threshold, "CRITICAL: Algedonic alert LOST - no live channel, event_sink, or email sink");
            }
        }
        queue_confirmed || archive_persisted
    }

    /// Verify evidence-bearing rollout impact checks (Fermi impact-gate pattern).
    ///
    /// The typed input excludes central Notify/Escalate dispositions: neither
    /// is a measured rollout intervention. Each check is answered by the rollout event source with
    /// comparable before/after observations, then classified using relative
    /// worsening thresholds.
    pub(super) async fn verify_impact(
        &self,
        impact_checks: &[super::RolloutImpactCheck],
    ) -> Vec<ImpactReport> {
        let mut reports = Vec::new();

        for check in impact_checks {
            let Some(source) = &self.rollout_events else {
                tracing::warn!(
                    target: "reg.cybernetics",
                    rollout = %check.rollout_id,
                    metric = %check.metric,
                    "rollout impact check has no event source — verdict not computed"
                );
                continue;
            };
            let Some(metric) = SignalMetric::from_str_name(&check.metric) else {
                tracing::warn!(
                    target: "reg.cybernetics",
                    rollout = %check.rollout_id,
                    metric = %check.metric,
                    "rollout impact check named an unknown metric — verdict not computed"
                );
                continue;
            };
            let (before_val, after_val) = match source.metric_before_and_after(
                &check.rollout_id,
                &check.metric,
                check.before_position,
            ) {
                Ok(Some(values)) => values,
                Ok(None) => {
                    tracing::warn!(
                        target: "reg.cybernetics",
                        rollout = %check.rollout_id,
                        metric = %check.metric,
                        "rollout impact check found no events for this metric — no baseline to verify against"
                    );
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "reg.cybernetics",
                        rollout = %check.rollout_id,
                        metric = %check.metric,
                        error = %error,
                        "rollout impact check store query failed — verdict not computed"
                    );
                    continue;
                }
            };

            let delta = after_val - before_val;
            let Some(higher_is_better) = metric.impact_direction() else {
                tracing::warn!(
                    target: "reg.cybernetics",
                    rollout = %check.rollout_id,
                    metric = %check.metric,
                    "rollout impact check metric has no impact direction — verdict not computed"
                );
                continue;
            };
            let improved = if higher_is_better {
                delta > 0.0
            } else {
                delta < 0.0
            };
            let worsening = if improved || delta.abs() <= f64::EPSILON {
                0.0
            } else if before_val.abs() <= f64::EPSILON {
                tracing::warn!(
                    target: "reg.cybernetics",
                    rollout = %check.rollout_id,
                    metric = %check.metric,
                    after = after_val,
                    "rollout impact check has a zero baseline — relative verdict not computed"
                );
                continue;
            } else {
                delta.abs() / before_val.abs()
            };
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

            let action_type = ActionType::Notify;
            let action_type_str = action_type.as_str();
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
                        action_type,
                    ),
                };
                let latched = self
                    .alert_escalation_sink
                    .as_ref()
                    .is_some_and(|sink| sink.has_pending_alert(&alert.message));
                if !latched {
                    self.emit_regulation_span(
                        SpanKind::RegulatoryPlateauDetected,
                        serde_json::json!({
                            "metric": metric.as_str(),
                            "action_type": action_type_str,
                            "consecutive_cycles": threshold,
                        }),
                    )
                    .await;
                    if metric == SignalMetric::ToolReliability {
                        self.emit_tool_outcome_breakdown().await;
                    }
                    self.persist_alert_to_queue(&alert, None).await;
                    if let Some(ref tx) = self.alerts_tx
                        && tx.send(CurationInput::Alert(alert)).is_err()
                    {
                        tracing::warn!(target: "reg.alert", "Plateau alert send failed — channel closed");
                    }
                }
            }

            if decision == ActionDecision::Block {
                self.emit_regulation_span(
                    SpanKind::ActionBlocked,
                    serde_json::json!({
                        "metric": metric.as_str(),
                        "action_type": format!("{:?}", action_type),
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
                        "ActionDecision::Block: impact check for {} observed {:.1}% relative worsening (threshold: {:.1}%)",
                        metric.as_str(),
                        worsening * 100.0,
                        block_worsening_ratio * 100.0,
                    ),
                };
                self.persist_alert_to_queue(&alert, None).await;
                if let Some(ref tx) = self.alerts_tx
                    && tx.send(CurationInput::Alert(alert)).is_err()
                {
                    tracing::warn!(target: "reg.alert", "Block alert send failed — channel closed");
                }
            }

            self.emit_regulation_span(
                SpanKind::ImpactVerified,
                serde_json::json!({
                    "metric": metric.as_str(),
                    "action_type": action_type_str,
                    "before": before_val,
                    "after": after_val,
                    "delta": delta,
                    "improved": improved,
                    "decision": format!("{:?}", decision),
                }),
            )
            .await;

            if let Err(error) = source.append_impact_verdict(
                &check.rollout_id,
                &check.metric,
                before_val,
                after_val,
                improved,
                &format!("{:?}", decision),
            ) {
                tracing::warn!(
                    target: "reg.cybernetics",
                    rollout = %check.rollout_id,
                    error = %error,
                    "impact verdict write-back failed — the loop's judgment is not persisted to the store"
                );
            }

            reports.push(ImpactReport::new(
                action_type,
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
    /// Converts a typed policy disposition into its evidence-bearing payload.
    async fn build_regulation_action(
        &self,
        dev: &Deviation,
        proposed: &regulation_policy::ProposedAction,
    ) -> Option<RegulatoryAction> {
        match proposed.reason {
            // -- VarietyDeficit AboveSetPoint -------------------------------
            RegulationReason::VarietyDeficitExceeded => {
                let at = proposed.action_type;
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

            // -- ToolReliability BelowSetPoint ------------------------------
            RegulationReason::ToolReliabilityDegraded => {
                tracing::warn!(
                    target: "reg.tool",
                    reliability = dev.signal.value,
                    set_point = dev.signal.set_point,
                    "Tool reliability degraded — success rate below threshold"
                );
                let at = proposed.action_type;
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
            RegulationReason::TripleCountObserved
            | RegulationReason::LowConfidenceCountObserved
            | RegulationReason::ConsolidationCandidatesObserved
            | RegulationReason::PendingEscalationsObserved => Some(RegulatoryAction::with_metric(
                proposed.target,
                proposed.action_type,
                RegulatoryActionParams::reason(proposed.reason.as_str()),
                dev.signal.metric.as_str().into(),
            )),
            // -- Meta-regulatory and domain-specific Escalate dispositions.
            //    These carry NoData when no quantitative variant exists.
            RegulationReason::AlgedonicEventsExceeded
            | RegulationReason::AlgedonicLogApproachingCap
            | RegulationReason::GoalsStale
            | RegulationReason::GoalsExpired
            | RegulationReason::MetacognitionCriticalAlerts
            | RegulationReason::MemoryLifeLow
            | RegulationReason::CircuitBreakerOpen
            | RegulationReason::ModelUnavailable => Some(RegulatoryAction::with_metric(
                proposed.target,
                proposed.action_type,
                RegulatoryActionParams::reason(proposed.reason.as_str()),
                dev.signal.metric.as_str().into(),
            )),
            // Carry the observed storm count into escalation evidence.
            RegulationReason::OcrSilentFailuresExceeded => Some(RegulatoryAction::with_metric(
                proposed.target,
                proposed.action_type,
                RegulatoryActionParams::with_data(
                    proposed.reason.as_str(),
                    RegulationData::OcrSilentFailuresExceeded {
                        count: dev.signal.value,
                        threshold: dev.signal.set_point,
                    },
                ),
                dev.signal.metric.as_str().into(),
            )),
            // Carry typed fleet-health data so extract_deficit_threshold can
            // populate the advisory context with real counts instead of (0, 0).
            RegulationReason::ContextServerFleetDegraded => {
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
                    proposed.action_type,
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
    use crate::cybernetics_loop::RolloutImpactCheck;
    use crate::loops::{
        ActionDecision, ActionType, Deviation, DeviationDirection, LoopId, RegulationData,
        RegulatoryAction, RegulatoryActionParams, Signal, SignalMetric,
    };
    use crate::regulation_policy::RegulationPolicy;
    use crate::runtime::RegulationLedger;
    use crate::{RolloutEventError, RolloutEventSource};
    use hkask_types::WebID;
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
    /// to exercise the three event-substrate outcomes. The no-data and error
    /// branches must not record a verdict.
    struct MockRolloutEventSource {
        /// `Err` holds only the failure detail; the typed
        /// [`RolloutEventError`] is built at the trait boundary so the mock
        /// does not force `Clone` onto the port's error type.
        before_after: Mutex<Result<Option<(f64, f64)>, String>>, // string-error-ok
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

    struct ChangingOcrHealthSource {
        reads: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl crate::OcrHealthSource for ChangingOcrHealthSource {
        async fn recent_silent_failures(&self) -> Result<u64, crate::OcrHealthError> {
            let read = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(if read == 0 { 1 } else { 2 })
        }
    }

    /// A recording `AlertEscalationSink` — captures the exact strings the
    /// loop persisted and auto-resolved so the two message-format sites
    /// can be asserted byte-identical.
    struct RecordingEscalationSink {
        persisted: Mutex<Vec<String>>,
        /// The error_context JSON each persist carried — the breakdown test
        /// asserts on its contents.
        contexts: Mutex<Vec<String>>,
        auto_resolved: Mutex<Vec<String>>,
    }

    impl RecordingEscalationSink {
        fn new() -> Self {
            Self {
                persisted: Mutex::new(Vec::new()),
                contexts: Mutex::new(Vec::new()),
                auto_resolved: Mutex::new(Vec::new()),
            }
        }
    }

    impl crate::AlertEscalationSink for RecordingEscalationSink {
        fn persist_alert(&self, output: &str, _confidence: f64, error_context: &str) {
            self.persisted
                .lock()
                .expect("persisted lock")
                .push(output.to_string());
            self.contexts
                .lock()
                .expect("contexts lock")
                .push(error_context.to_string());
        }
        fn auto_resolve_cleared(&self, output: &str, _resolution_note: &str) {
            self.auto_resolved
                .lock()
                .expect("auto_resolved lock")
                .push(output.to_string());
        }
    }

    /// An `AlertEscalationSink` whose `has_pending_alert` flips on demand —
    /// pins the plateau latch: while the queue holds a pending escalation
    /// for the plateau condition, re-detections must suppress the span, the
    /// queue persist, and the live-channel send.
    struct ConfirmingEscalationSink {
        persisted: Mutex<Vec<String>>,
    }

    impl ConfirmingEscalationSink {
        fn new() -> Self {
            Self {
                persisted: Mutex::new(Vec::new()),
            }
        }
    }

    impl crate::AlertEscalationSink for ConfirmingEscalationSink {
        fn try_persist_alert(
            &self,
            output: &str,
            _confidence: f64,
            _error_context: &str,
        ) -> Result<crate::AlertQueueOutcome, crate::AlertPersistError> {
            self.persisted
                .lock()
                .expect("persisted lock")
                .push(output.to_string());
            Ok(crate::AlertQueueOutcome::Confirmed(Some(
                "test-escalation".to_string(),
            )))
        }

        fn persist_alert(&self, output: &str, _confidence: f64, _error_context: &str) {
            self.persisted
                .lock()
                .expect("persisted lock")
                .push(output.to_string());
        }

        fn has_pending_alert(&self, _output: &str) -> bool {
            !self.persisted.lock().expect("persisted lock").is_empty()
        }
    }

    struct LatchingEscalationSink {
        pending: Mutex<bool>,
        persisted: Mutex<Vec<String>>,
    }

    impl LatchingEscalationSink {
        fn new() -> Self {
            Self {
                pending: Mutex::new(false),
                persisted: Mutex::new(Vec::new()),
            }
        }

        fn set_pending(&self, pending: bool) {
            *self.pending.lock().expect("pending lock") = pending;
        }
    }

    impl crate::AlertEscalationSink for LatchingEscalationSink {
        fn persist_alert(&self, output: &str, _confidence: f64, _error_context: &str) {
            self.persisted
                .lock()
                .expect("persisted lock")
                .push(output.to_string());
        }
        fn has_pending_alert(&self, _output: &str) -> bool {
            *self.pending.lock().expect("pending lock")
        }
    }

    /// A computed action is advice until an operator applies it. Routing that
    /// advice must not immediately re-read the metric and attribute concurrent
    /// movement to an intervention that did not occur.
    #[tokio::test]
    async fn tick_does_not_verify_unapplied_ocr_advice() {
        let ocr = Arc::new(ChangingOcrHealthSource {
            reads: std::sync::atomic::AtomicUsize::new(0),
        });
        let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
                .with_ocr_health_source(ocr.clone())
                .with_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
        regulation.set_alert_escalation_sink(Some(escalation.clone()));

        regulation.tick().await;

        assert_eq!(
            ocr.reads.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the tick may sense the condition once but must not re-sense advice as intervention"
        );
        assert!(
            escalation
                .persisted
                .lock()
                .expect("persisted lock")
                .iter()
                .any(|message| message.starts_with("ocr_silent_failures_exceeded")),
            "the detected OCR condition must still produce an actionable advisory"
        );
        let spans = archive.0.lock().expect("archive lock");
        assert!(
            !spans.iter().any(|(path, _)| path == "impact_verified"),
            "unapplied advice has no impact verdict"
        );
        assert!(
            !spans.iter().any(|(path, _)| path == "action_blocked"),
            "unapplied advice cannot be classified as harmful"
        );
    }

    /// T12: call-cap exhaustion is detected BEFORE the per-tick reset —
    /// the reset replenishes every cap (remaining = ceiling), so reading
    /// after it would never observe remaining == 0 and the E04 exhaustion
    /// alert could never fire (the pre-fix defect: the alert was dead
    /// code). Controls: the reset still replenishes; a replenished agent
    /// does not re-alert without new exhaustion; and the reset alone earns
    /// no advice-progress credit — the exhaustion alert is transient
    /// (escalated: false, no recovery signal), so it never enters the
    /// reviewable queue and the auto-resolve machinery has nothing to
    /// credit the reset with.
    #[tokio::test]
    async fn cap_exhaustion_is_detected_before_the_reset_replenishes() {
        let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation_loop =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation_loop
            .set_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
        regulation_loop.set_alert_escalation_sink(Some(escalation.clone()));

        let agent = WebID::from_persona(b"agent-x");
        {
            let cap_manager = regulation_loop.call_cap_manager.read().await;
            cap_manager.register_call_cap(agent, 2).await;
            // Exhaust between ticks, as metered dispatches would.
            cap_manager.charge_metered(&agent).await;
            cap_manager.charge_metered(&agent).await;
        }

        // No live alerts channel wired — the alert falls to the archive.
        regulation_loop.act(&[]).await;

        {
            let records = archive.0.lock().unwrap_or_else(|e| e.into_inner());
            let exhaustion_records: Vec<_> = records
                .iter()
                .filter(|(path, observation)| {
                    path.contains("algedonic")
                        && observation
                            .get("message")
                            .and_then(|m| m.as_str())
                            .is_some_and(|m| m.contains("exhausted"))
                })
                .collect();
            assert_eq!(
                exhaustion_records.len(),
                1,
                "the exhausted agent must produce exactly one alert: {records:#?}"
            );
            assert!(
                exhaustion_records[0]
                    .1
                    .get("message")
                    .and_then(|m| m.as_str())
                    .is_some_and(|m| m.contains(&agent.to_string())),
                "the alert must name the exhausted agent"
            );
        }

        // The reset still replenished: the cap is back at its ceiling.
        let statuses = regulation_loop
            .call_cap_manager
            .read()
            .await
            .all_agent_statuses()
            .await;
        let status = statuses
            .iter()
            .find(|(id, _)| *id == agent)
            .expect("registered");
        assert_eq!((status.1.ceiling, status.1.remaining), (2, 2));

        // Control: a second act() with no new charges does not re-alert —
        // replenishment alone is not exhaustion.
        regulation_loop.act(&[]).await;
        let records = archive.0.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            records
                .iter()
                .filter(|(path, observation)| {
                    path.contains("algedonic")
                        && observation
                            .get("message")
                            .and_then(|m| m.as_str())
                            .is_some_and(|m| m.contains("exhausted"))
                })
                .count(),
            1,
            "a replenished agent must not re-alert"
        );

        // Control: the reset alone earns no advice-progress credit — the
        // transient exhaustion alert never enters the reviewable queue
        // (nothing persisted, nothing auto-resolved).
        assert!(
            escalation.persisted.lock().expect("persisted").is_empty(),
            "the transient exhaustion alert must not enter the reviewable queue"
        );
        assert!(
            escalation
                .auto_resolved
                .lock()
                .expect("resolved")
                .is_empty(),
            "the automatic reset must not earn advice-progress credit"
        );
    }

    /// expect: "Routing advice does not prove that an alert cleared" [P9]
    #[tokio::test]
    async fn routing_advice_does_not_resolve_alert() {
        let sink = Arc::new(RecordingEscalationSink::new());
        let mut regulation_loop =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation_loop.set_alert_escalation_sink(Some(sink.clone()));
        let action = RegulatoryAction::with_metric(
            LoopId::Curation,
            ActionType::Escalate,
            RegulatoryActionParams::reason("test_escalation"),
            "test_metric".to_string(),
        );
        regulation_loop.route_action_as_alert(&action).await;
        assert_eq!(sink.persisted.lock().expect("persisted").len(), 1);
        assert!(sink.auto_resolved.lock().expect("resolved").is_empty());
    }

    fn rollout_impact_check(rollout_id: &str, metric: &str) -> RolloutImpactCheck {
        RolloutImpactCheck {
            rollout_id: rollout_id.to_string(),
            before_position: 1,
            metric: metric.to_string(),
        }
    }

    /// The plateau latch: while a pending escalation for the plateau
    /// condition sits in the review queue, re-detections suppress the
    /// entire routing (span, queue persist, live channel). Before the
    /// latch, a persistent plateau re-fired every cycle — the
    /// live-observed retry_count 37 on the queue row, the
    /// `plateau_detected` span flood behind the algedonic log-cap breach,
    /// and the same alert in the Curator inbox every 10s. The stagnation
    /// detector keeps counting while latched, so resolving the escalation
    /// re-fires on the next detection.
    #[tokio::test]
    async fn plateau_alert_latches_while_pending() {
        let source = Arc::new(MockRolloutEventSource::answering(0.2, 0.2));
        let mut regulation = loop_with_source(source.clone());
        let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let escalation = Arc::new(LatchingEscalationSink::new());
        regulation.set_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
        regulation.set_alert_escalation_sink(Some(escalation.clone()));

        let action = rollout_impact_check("trace", "tool_reliability");
        // Constant before/after — every cycle is ineffective, so the
        // stagnation detector reaches its threshold and the plateau fires.
        *source.before_after.lock().expect("source") = Ok(Some((0.2, 0.2)));
        let stagnation_threshold = regulation
            .stagnation_detector
            .threshold_for_metric("tool_reliability");
        for _ in 0..stagnation_threshold {
            regulation
                .verify_impact(std::slice::from_ref(&action))
                .await;
        }
        let plateau_spans = || {
            archive
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|(path, _)| path == "reg.outcome.plateau_detected")
                .count()
        };
        assert_eq!(
            escalation.persisted.lock().expect("persisted").len(),
            1,
            "the first plateau detection persists one escalation"
        );
        assert_eq!(plateau_spans(), 1, "the first detection emits one span");

        // The operator now has a pending escalation — further detections
        // must be latched: no new persist, no new span.
        escalation.set_pending(true);
        for _ in 0..3 {
            regulation
                .verify_impact(std::slice::from_ref(&action))
                .await;
        }
        assert_eq!(
            escalation.persisted.lock().expect("persisted").len(),
            1,
            "latched re-detections must not re-persist"
        );
        assert_eq!(
            plateau_spans(),
            1,
            "latched re-detections must not re-emit the plateau span"
        );
        assert!(
            regulation
                .stagnation_detector
                .ineffective_count("tool_reliability", "Notify")
                >= stagnation_threshold,
            "the stagnation detector keeps counting while latched — resolving \
             the escalation re-fires the next detection"
        );
    }

    /// Tool-reliability alerts carry the per-domain outcome breakdown in
    /// both durable surfaces: the escalation row's error_context (what the
    /// Curator reviews via `curator_escalations`) and the
    /// `reg.outcome.tool_domains` span (what the algedonic log serves via
    /// `curator_algedonic_log`). Before this, the escalation carried only
    /// the aggregate success rate — triage could see THAT tools were
    /// failing but never WHICH domain or with what error kinds.
    #[tokio::test]
    async fn tool_reliability_alert_carries_outcome_breakdown() {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        {
            let ledger_guard = ledger.read().await;
            for _ in 0..crate::sensor_provider::TOOL_RELIABILITY_MIN_DOMAIN_SAMPLES {
                ledger_guard
                    .record_outcome("media", false, Some("internal"))
                    .await;
            }
        }
        let archive = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));
        regulation_loop
            .set_event_sink(Arc::clone(&archive) as Arc<dyn hkask_types::RegulationSink>);
        regulation_loop.set_alert_escalation_sink(Some(escalation.clone()));
        // The sense phase records the observation the alert routing looks
        // up; seed it directly so the test exercises the routing, not the bus.
        regulation_loop.observations.lock().insert(
            SignalMetric::ToolReliability,
            Signal::new(
                LoopId::Cybernetics,
                SignalMetric::ToolReliability,
                0.0,
                0.80,
            ),
        );

        let action = RegulatoryAction::with_metric(
            LoopId::Curation,
            ActionType::Escalate,
            RegulatoryActionParams::with_data(
                "tool_reliability_degraded",
                RegulationData::ToolReliabilityDegraded {
                    reliability: 0.0,
                    threshold: 0.8,
                },
            ),
            "tool_reliability".into(),
        );
        regulation_loop.route_action_as_alert(&action).await;

        let contexts = escalation.contexts.lock().expect("contexts");
        assert_eq!(contexts.len(), 1, "the degradation alert persists once");
        let context: serde_json::Value =
            serde_json::from_str(&contexts[0]).expect("error_context is JSON");
        let breakdown = context
            .get("outcome_breakdown")
            .expect("tool-reliability alerts carry the breakdown");
        assert!(
            breakdown.to_string().contains("media"),
            "the breakdown names the failing domain: {breakdown}"
        );
        assert!(
            breakdown.to_string().contains("internal"),
            "the breakdown carries the error kind: {breakdown}"
        );

        let spans = archive.0.lock().unwrap_or_else(|e| e.into_inner());
        let tool_domains: Vec<_> = spans
            .iter()
            .filter(|(path, _)| path == "reg.outcome.tool_domains")
            .collect();
        assert_eq!(tool_domains.len(), 1, "one breakdown span at alert time");
        assert!(
            tool_domains[0]
                .1
                .get("domains")
                .expect("domains field")
                .to_string()
                .contains("media"),
            "the span names the failing domain"
        );
    }

    /// expect: "Accepted constant degradation/noise accumulates stagnation; only observed progress resets it" [P9]
    #[tokio::test]
    async fn accepted_noise_does_not_erase_stagnation_or_imply_progress() {
        use crate::loops::core::{LoopMetrics, TriggerOrigin};
        let source = Arc::new(MockRolloutEventSource::answering(0.2, 0.2));
        let regulation = loop_with_source(source.clone());
        let action = rollout_impact_check("trace", "tool_reliability");
        for (index, after) in [0.2, 0.199, 0.2, 0.2].into_iter().enumerate() {
            *source.before_after.lock().expect("source") = Ok(Some((0.2, after)));
            let reports = regulation
                .verify_impact(std::slice::from_ref(&action))
                .await;
            let report = reports.first().expect("verified");
            assert_eq!(report.decision, ActionDecision::Accept);
            assert!(!report.improved);
            assert_eq!(
                regulation
                    .stagnation_detector
                    .ineffective_count("tool_reliability", ActionType::Notify.as_str()),
                (index + 1) as u32
            );
            assert_eq!(
                LoopMetrics::from_cycle(0, &[], &[], &reports, TriggerOrigin::Scheduled)
                    .observed_progress_score,
                0.0
            );
        }
        *source.before_after.lock().expect("source") = Ok(Some((0.2, 0.3)));
        let reports = regulation
            .verify_impact(std::slice::from_ref(&action))
            .await;
        assert!(reports.first().expect("report").improved);
        assert_eq!(
            regulation
                .stagnation_detector
                .ineffective_count("tool_reliability", ActionType::Notify.as_str()),
            0
        );
    }

    /// Worsening thresholds are ratios, not raw metric units. A pass-rate
    /// decline from 0.10 to 0.07 is 30% relative worsening and must cross the
    /// default 20% block threshold even though the absolute delta is only 0.03.
    #[tokio::test]
    async fn verify_impact_classifies_relative_worsening() {
        let source = Arc::new(MockRolloutEventSource::answering(0.10, 0.07));
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation = loop_with_source(source);
        regulation.set_alert_escalation_sink(Some(escalation.clone()));

        let reports = regulation
            .verify_impact(&[rollout_impact_check("relative", "pass_rate")])
            .await;

        assert_eq!(
            reports.first().expect("impact report").decision,
            ActionDecision::Block
        );
        let messages = escalation.persisted.lock().expect("persisted lock");
        assert_eq!(messages.len(), 1, "one block alert is persisted");
        assert_eq!(
            messages.first().expect("block alert"),
            "ActionDecision::Block: impact check for pass_rate observed 30.0% relative worsening (threshold: 20.0%)",
            "the alert reports an observation, not unsupported causation"
        );
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

    /// A submitted impact check the store cannot answer must surface the
    /// missing baseline and produce neither a report nor a verdict write-back.
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

    /// Notify is a truthful handled disposition and contributes to response coverage.
    #[test]
    fn compute_includes_notify_dispositions() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let regulation_loop = loop_with_source(Arc::new(MockRolloutEventSource::empty()));
            // TripleCount AboveSetPoint triggers a Notify rule.
            let signal = Signal::new(LoopId::Cybernetics, SignalMetric::TripleCount, 1.0, 0.0);
            let deviation = Deviation::from_signal(&signal)
                .expect("TripleCount 1.0 vs set_point 0.0 should deviate");
            let actions = regulation_loop.compute(&[deviation]).await;
            assert_eq!(actions.len(), 1);
            assert!(
                actions
                    .first()
                    .is_some_and(|action| action.action_type == ActionType::Notify),
                "the observation must be represented as a handled Notify disposition"
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
        let action = RegulatoryAction {
            target: LoopId::Curation,
            action_type: ActionType::Escalate,
            parameters: RegulatoryActionParams::reason("some_unrelated_low_thing"),
            metric_name: None,
        };
        let metrics =
            LoopMetrics::from_cycle(0, &[deviation], &[action], &[], TriggerOrigin::Scheduled);
        assert_eq!(
            metrics.fidelity_score, 0.0,
            "action without metric_name must not match via string fallback"
        );
    }

    struct MemoryObservations(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl crate::MemoryHealthSource for MemoryObservations {
        async fn h_mem_count(&self) -> Option<usize> {
            match self.0.load(std::sync::atomic::Ordering::SeqCst) {
                0 => Some(1_000_000),
                1 => Some(0),
                _ => None,
            }
        }
        async fn low_confidence_count(&self, _: f64) -> Option<usize> {
            self.h_mem_count().await
        }
        async fn memory_life_days(&self) -> f64 {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                0.0
            } else {
                36_500.0
            }
        }
    }

    /// expect: "Every memory metric can report recovery independently; unavailable stores do not report zero" [P9]
    #[tokio::test]
    async fn memory_observations_report_all_metrics_and_recovery() {
        let source = Arc::new(MemoryObservations(std::sync::atomic::AtomicUsize::new(0)));
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation.set_memory_health_source(source.clone());
        let metrics = [
            SignalMetric::MemoryLife,
            SignalMetric::TripleCount,
            SignalMetric::LowConfidenceCount,
            SignalMetric::ConsolidationCandidates,
        ];
        let degraded = regulation.sense().await;
        for metric in metrics {
            let signal = degraded
                .iter()
                .find(|signal| signal.metric == metric)
                .expect("every memory metric sensed");
            assert!(Deviation::from_signal(signal).is_some(), "{metric:?}");
        }
        source.0.store(1, std::sync::atomic::Ordering::SeqCst);
        let healthy = regulation.sense().await;
        for metric in metrics {
            let signal = healthy
                .iter()
                .find(|signal| signal.metric == metric)
                .expect("healthy metric observable");
            assert!(Deviation::from_signal(signal).is_none(), "{metric:?}");
            assert!(
                degraded
                    .iter()
                    .find(|signal| signal.metric == metric)
                    .expect("trigger")
                    .recovered_by(signal)
            );
        }
        source.0.store(2, std::sync::atomic::Ordering::SeqCst);
        let unavailable = regulation.sense().await;
        assert_eq!(
            unavailable
                .iter()
                .filter(|signal| metrics.contains(&signal.metric))
                .count(),
            1,
            "only configuration remains observable without a store"
        );
    }

    /// D-sensing (2026-08-30): the algedonic log's population state must be
    /// sensed — `AlgedonicEvents`, `PendingEscalations`, and
    /// `MetacognitionCriticalAlerts` were policy-only (rules that could never
    /// fire) before this. A clean log emits healthy zero observations; a critical
    /// outcome alert (0% success → Critical, escalated) emits all three with
    /// value 1.0 against set-point 0.0.
    #[test]
    fn sense_reports_algedonic_log_population() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger));

            // Healthy observations distinguish recovery from unavailable sensing.
            let signals = regulation_loop.sense().await;
            for metric in [
                SignalMetric::AlgedonicEvents,
                SignalMetric::PendingEscalations,
                SignalMetric::MetacognitionCriticalAlerts,
            ] {
                let signal = signals
                    .iter()
                    .find(|signal| signal.metric == metric)
                    .unwrap_or_else(|| panic!("{metric:?} must be observed"));
                assert_eq!(signal.value, 0.0);
                assert_eq!(signal.set_point, 0.0);
                assert!(Deviation::from_signal(signal).is_none());
            }

            // An Info-only log must not create a deviation: Info entries
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
            let signal = signals
                .iter()
                .find(|signal| signal.metric == SignalMetric::AlgedonicEvents)
                .expect("Info-only log must remain observable");
            assert_eq!(signal.value, 0.0);
            assert_eq!(signal.set_point, 0.0);
            assert!(Deviation::from_signal(signal).is_none());

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
                assert!(Deviation::from_signal(signal).is_some());
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
                .record_variety("hkask-mcp-media", "gallery_add_media")
                .await;
            let ledger_guard = ledger.read().await;
            assert_eq!(
                ledger_guard.variety_for_domain("hkask-mcp-media").await,
                2,
                "distinct tool names are the variety — repeats don't count"
            );
        });
    }

    struct HealthyResilienceSource;

    #[async_trait::async_trait]
    impl crate::InferenceResilienceSource for HealthyResilienceSource {
        async fn observe_since(
            &self,
            cursor: u64,
        ) -> Result<crate::InferenceObservation, crate::InferenceObservationError> {
            Ok(crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: chrono::Utc::now(),
                    in_flight: 0,
                    max_concurrency: 96,
                    recent_timeout_count: 0,
                    circuit_state: crate::InferenceCircuitState::Closed,
                },
                interventions: Vec::new(),
                permanent_failures: Vec::new(),
                next_cursor: cursor,
            })
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
            regulation_loop.set_inference_resilience_source(Arc::new(HealthyResilienceSource));
            let signals = regulation_loop.sense().await;
            assert!(
                !signals
                    .iter()
                    .any(|s| s.metric == SignalMetric::InferenceModelAvailable
                        && Deviation::from_signal(s).is_some()),
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

    struct FailOnceCapturingSink {
        fail_next: std::sync::atomic::AtomicBool,
        persisted: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl hkask_types::RegulationSink for FailOnceCapturingSink {
        fn persist(
            &self,
            event: &hkask_types::RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            if self
                .fail_next
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err(hkask_types::InfrastructureError::Serialization(
                    "sink unavailable".to_string(),
                ));
            }
            self.persisted
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((event.span.path.clone(), event.observation.clone()));
            Ok(())
        }
    }

    struct StubResilienceSource {
        observation: crate::InferenceObservation,
    }

    #[async_trait::async_trait]
    impl crate::InferenceResilienceSource for StubResilienceSource {
        async fn observe_since(
            &self,
            cursor: u64,
        ) -> Result<crate::InferenceObservation, crate::InferenceObservationError> {
            let mut observation = self.observation.clone();
            observation
                .interventions
                .retain(|receipt| receipt.id > cursor);
            observation
                .permanent_failures
                .retain(|receipt| receipt.id > cursor);
            observation.next_cursor = observation
                .interventions
                .iter()
                .map(|receipt| receipt.id)
                .chain(
                    observation
                        .permanent_failures
                        .iter()
                        .map(|receipt| receipt.id),
                )
                .max()
                .unwrap_or(cursor);
            Ok(observation)
        }
    }

    struct CursorRecordingResilienceSource {
        observation: crate::InferenceObservation,
        cursors: Mutex<Vec<u64>>,
    }

    #[async_trait::async_trait]
    impl crate::InferenceResilienceSource for CursorRecordingResilienceSource {
        async fn observe_since(
            &self,
            cursor: u64,
        ) -> Result<crate::InferenceObservation, crate::InferenceObservationError> {
            self.cursors
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(cursor);
            let mut observation = self.observation.clone();
            observation
                .interventions
                .retain(|receipt| receipt.id > cursor);
            observation
                .permanent_failures
                .retain(|receipt| receipt.id > cursor);
            observation.next_cursor = observation
                .interventions
                .iter()
                .map(|receipt| receipt.id)
                .chain(
                    observation
                        .permanent_failures
                        .iter()
                        .map(|receipt| receipt.id),
                )
                .max()
                .unwrap_or(cursor);
            Ok(observation)
        }
    }

    /// expect: "A resilience receipt remains pending until its observation is durably recorded"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: the first attempt to persist an observed recovery fails and the second succeeds
    /// post: the second observation retries the same cursor and only the third advances past the receipt
    #[tokio::test]
    async fn failed_inference_receipt_persistence_does_not_advance_cursor() {
        let now = chrono::Utc::now();
        let source = Arc::new(CursorRecordingResilienceSource {
            observation: crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: now,
                    in_flight: 0,
                    max_concurrency: 2,
                    recent_timeout_count: 0,
                    circuit_state: crate::InferenceCircuitState::Closed,
                },
                interventions: vec![crate::InferenceInterventionReceipt {
                    id: 1,
                    kind: crate::InferenceInterventionKind::CircuitClosed,
                    occurred_at: now,
                }],
                permanent_failures: Vec::new(),
                next_cursor: 1,
            },
            cursors: Mutex::new(Vec::new()),
        });
        let sink = Arc::new(FailOnceCapturingSink {
            fail_next: std::sync::atomic::AtomicBool::new(true),
            persisted: Mutex::new(Vec::new()),
        });
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
        regulation.set_inference_resilience_source(Arc::clone(&source) as Arc<_>);

        regulation.sense().await;
        regulation.sense().await;
        regulation.sense().await;

        assert_eq!(
            *source
                .cursors
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            vec![0, 0, 1]
        );
        assert_eq!(
            sink.persisted
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            1
        );
    }

    /// expect: "A failed half-open receipt stays pending until its escalation is durably handled"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: a circuit-reopened receipt is observed while no escalation or archive sink is available
    /// post: the next observation retries from the same cursor instead of acknowledging lost evidence
    #[tokio::test]
    async fn failed_inference_escalation_does_not_advance_cursor() {
        let now = chrono::Utc::now();
        let source = Arc::new(CursorRecordingResilienceSource {
            observation: crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: now,
                    in_flight: 0,
                    max_concurrency: 2,
                    recent_timeout_count: 1,
                    circuit_state: crate::InferenceCircuitState::Open,
                },
                interventions: vec![crate::InferenceInterventionReceipt {
                    id: 1,
                    kind: crate::InferenceInterventionKind::CircuitReopened,
                    occurred_at: now,
                }],
                permanent_failures: Vec::new(),
                next_cursor: 1,
            },
            cursors: Mutex::new(Vec::new()),
        });
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation.set_inference_resilience_source(Arc::clone(&source) as Arc<_>);

        regulation.sense().await;
        regulation.sense().await;

        assert_eq!(
            *source
                .cursors
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            vec![0, 0]
        );
    }

    /// expect: "A recovered inference circuit is recorded as observed recovery, not causal proof"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: the resilience source reports a later circuit-closed receipt
    /// post: regulation persists the receipt with causal attribution unverified
    #[tokio::test]
    async fn circuit_close_receipt_records_observed_recovery() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let now = chrono::Utc::now();
        let source = Arc::new(StubResilienceSource {
            observation: crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: now,
                    in_flight: 0,
                    max_concurrency: 2,
                    recent_timeout_count: 0,
                    circuit_state: crate::InferenceCircuitState::Closed,
                },
                interventions: vec![crate::InferenceInterventionReceipt {
                    id: 1,
                    kind: crate::InferenceInterventionKind::CircuitClosed,
                    occurred_at: now,
                }],
                permanent_failures: Vec::new(),
                next_cursor: 1,
            },
        });
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>);
        regulation.set_inference_resilience_source(source);

        regulation.sense().await;

        let spans = sink.0.lock().unwrap_or_else(|error| error.into_inner());
        let recovery = spans
            .iter()
            .find(|(path, _)| path == "reg.inference.observed_recovery")
            .map(|(_, observation)| observation);
        assert_eq!(
            recovery.and_then(|value| value["causal_attribution"].as_str()),
            Some("unverified"),
            "persisted spans: {spans:?}"
        );
    }

    /// expect: "Initial circuit opening corrects locally without creating an operator alert"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: the resilience boundary reports its first circuit-open transition
    /// post: regulation records the transition but does not escalate before recovery is attempted
    #[tokio::test]
    async fn initial_inference_circuit_open_does_not_escalate() {
        let now = chrono::Utc::now();
        let source = Arc::new(StubResilienceSource {
            observation: crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: now,
                    in_flight: 0,
                    max_concurrency: 2,
                    recent_timeout_count: 3,
                    circuit_state: crate::InferenceCircuitState::Open,
                },
                interventions: vec![crate::InferenceInterventionReceipt {
                    id: 1,
                    kind: crate::InferenceInterventionKind::CircuitOpened,
                    occurred_at: now,
                }],
                permanent_failures: Vec::new(),
                next_cursor: 1,
            },
        });
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation.set_inference_resilience_source(source);
        regulation.set_alert_escalation_sink(Some(
            Arc::clone(&escalation) as Arc<dyn crate::AlertEscalationSink>
        ));

        regulation.tick().await;

        assert!(
            escalation
                .persisted
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        );
    }

    /// expect: "A failed half-open recovery probe escalates once with its real condition"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: the local resilience boundary reports that the circuit reopened
    /// post: central regulation routes a native circuit-breaker escalation
    #[tokio::test]
    async fn reopened_inference_circuit_routes_native_escalation() {
        let now = chrono::Utc::now();
        let source = Arc::new(StubResilienceSource {
            observation: crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: now,
                    in_flight: 0,
                    max_concurrency: 2,
                    recent_timeout_count: 3,
                    circuit_state: crate::InferenceCircuitState::Open,
                },
                interventions: vec![crate::InferenceInterventionReceipt {
                    id: 1,
                    kind: crate::InferenceInterventionKind::CircuitReopened,
                    occurred_at: now,
                }],
                permanent_failures: Vec::new(),
                next_cursor: 1,
            },
        });
        let escalation = Arc::new(ConfirmingEscalationSink::new());
        let event_sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())))
                .with_event_sink(Arc::clone(&event_sink) as Arc<dyn hkask_types::RegulationSink>);
        regulation.set_inference_resilience_source(source);
        regulation.set_alert_escalation_sink(Some(
            Arc::clone(&escalation) as Arc<dyn crate::AlertEscalationSink>
        ));

        regulation.tick().await;

        let persisted = escalation
            .persisted
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(persisted.len(), 1);
        assert_eq!(
            persisted.first().map(String::as_str),
            Some("circuit_breaker_open — regulatory escalation")
        );
        drop(persisted);
        let spans = event_sink
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let loop_quality = spans
            .iter()
            .find(|(path, _)| path == "reg.outcome.loop_quality")
            .map(|(_, observation)| observation);
        assert_eq!(
            loop_quality.and_then(|observation| observation["actions"].as_u64()),
            Some(1),
            "the deduplicated policy disposition must remain visible in loop-quality telemetry"
        );
    }

    /// expect: "Permanent inference failures escalate for operator correction without opening a transient circuit"
    /// [P9] Motivating: Homeostatic Self-Regulation
    /// pre: the resilience source reports an authorization failure while the circuit is closed
    /// post: one native escalation carries the failure class and no unwired-action wording
    #[tokio::test]
    async fn permanent_inference_failure_routes_native_escalation() {
        let now = chrono::Utc::now();
        let source = Arc::new(StubResilienceSource {
            observation: crate::InferenceObservation {
                snapshot: crate::InferenceSnapshot {
                    observed_at: now,
                    in_flight: 0,
                    max_concurrency: 2,
                    recent_timeout_count: 0,
                    circuit_state: crate::InferenceCircuitState::Closed,
                },
                interventions: Vec::new(),
                permanent_failures: vec![crate::InferencePermanentFailureReceipt {
                    id: 1,
                    kind: crate::InferencePermanentFailureKind::Authorization,
                    detail: "provider rejected credential".to_string(),
                    occurred_at: now,
                }],
                next_cursor: 1,
            },
        });
        let escalation = Arc::new(RecordingEscalationSink::new());
        let mut regulation =
            CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        regulation.set_inference_resilience_source(source);
        regulation.set_alert_escalation_sink(Some(
            Arc::clone(&escalation) as Arc<dyn crate::AlertEscalationSink>
        ));

        regulation.sense().await;

        let persisted = escalation
            .persisted
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(persisted.len(), 1);
        assert_eq!(
            persisted.first().map(String::as_str),
            Some(
                "inference_permanent_failure:Authorization — provider rejected credential — regulatory escalation"
            )
        );
    }

    /// Idle cycles emit exactly one heartbeat span per hour (tick 1, then
    /// every 360 ticks) carrying the all-zero payload plus `heartbeat: true`,
    /// `tick_count`, and the alert log's fill state. Without it, a converged
    /// loop and a dead ticker are indistinguishable — both produce archive
    /// silence.
    #[test]
    fn idle_loop_emits_hourly_heartbeat_span() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
            let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
            let regulation_loop = CyberneticsLoop::new(Arc::clone(&ledger))
                .with_event_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>)
                .with_inference_resilience_source(Arc::new(HealthyResilienceSource));

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
                assert_eq!(observation.get("actions").and_then(|v| v.as_u64()), Some(0));
                assert_eq!(
                    observation.get("impact_reports").and_then(|v| v.as_u64()),
                    Some(0)
                );
                // The heartbeat carries the alert log's fill state so the
                // cap trend is visible from any session, not just Curator
                // sessions with the curator_status agent tool.
                assert_eq!(
                    observation.get("alert_log_count").and_then(|v| v.as_u64()),
                    Some(0),
                    "idle span {index} reports an empty alert log"
                );
                assert!(
                    observation
                        .get("alert_log_cap")
                        .and_then(|v| v.as_u64())
                        .is_some_and(|cap| cap >= 1),
                    "idle span {index} carries the configured cap"
                );
                assert_eq!(
                    observation.get("alert_log_approaching_cap"),
                    Some(&serde_json::json!(false)),
                    "an empty log is not approaching its cap"
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
