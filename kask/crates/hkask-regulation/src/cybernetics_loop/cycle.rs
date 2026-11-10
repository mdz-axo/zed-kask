//! The regulation cycle — sense → compare → compute → act → verify.
//!
//! Extracted from the cybernetics_loop god-module. The facade's `tick`
//! orchestrates these phases; each phase is `pub(super)` so the facade can
//! call it. Action construction (`build_regulation_action`), alert routing
//! (`route_action_as_alert`), and the cycle-internal helpers
//! (`deliver_alert_to_board`) is private to this module.

use crate::algedonic::{AlertSeverity, RuntimeAlert};
use crate::loops::RegulationData;
use crate::loops::{
    ActionType, CurationInput, Deviation, LoopId, RegulatoryAction, RegulatoryActionParams, Signal,
    SignalMetric,
};
use crate::regulation_policy::{
    self, RegulationPolicy, RegulationReason, extract_deficit_threshold,
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

    /// Deliver an algedonic alert to the reviewable board.
    ///
    /// This is the primary board delivery path for escalated alerts; the
    /// sink creates a card or comments on the existing open condition card.
    /// Delivery is best-effort and does not break the regulation loop.
    /// Non-escalated alerts are skipped.
    ///
    /// The `RuntimeAlert` fields are mapped to the board alert payload:
    /// `output` = `alert.message`, `error_context` = serialized alert JSON
    /// (domain/deficit/threshold/severity), `confidence` = 1.0 for Critical /
    /// 0.5 for Warning.
    ///
    async fn deliver_alert_to_board(
        &self,
        alert: &RuntimeAlert,
        recovery_signal: Option<&Signal>,
    ) -> Option<crate::AlertDeliveryOutcome> {
        let sink = self.alert_escalation_sink.as_ref()?;
        // Skip non-escalated alerts — only escalated alerts (Critical, or
        // Warning with `escalated: true`) belong in the reviewable backlog.
        // Info alerts and non-escalated Warnings are diagnostic, not
        // actionable, and would pollute the board.
        if !alert.escalated {
            return None;
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
        // triage can name the failing domain from the card context itself —
        // the board is the one surface the Curator reviews, and the aggregate
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
        // Experimentation-stuck alerts carry the stuck experiment ids so the board
        // card names what to unstick — the count in the message cannot. The
        // ids are re-read at delivery time (the freshest identity: an
        // experiment resolved between sense and delivery drops out honestly).
        let experimentation_stuck_related = recovery_signal
            .is_some_and(|signal| signal.metric == SignalMetric::ExperimentationStuckExperiments)
            || alert.domain.contains("experimentation_stuck");
        if experimentation_stuck_related
            && let Some(source) = self.experimentation_health_source.as_ref()
        {
            match source
                .stuck_running_experiments(
                    crate::sensor_provider::DEFAULT_EXPERIMENTATION_STALE_DAYS,
                )
                .await
            {
                Ok(ids) if !ids.is_empty() => {
                    error_context["stuck_experiments"] = serde_json::json!(ids);
                }
                Ok(_) => {}
                Err(error) => {
                    // The alert still delivers with the count; the identity
                    // enrichment degrades visibly, never silently.
                    tracing::warn!(
                        target: "reg.alert",
                        error = %error,
                        "Experimentation-stuck alert delivered without experiment ids — the registry re-read failed"
                    );
                }
            }
        }
        match sink
            .try_persist_alert(&alert.message, confidence, &error_context.to_string())
            .await
        {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                tracing::warn!(
                    target: "reg.alert",
                    error = %error,
                    domain = %alert.domain,
                    "Failed to deliver alert to the Algedonic review board"
                );
                None
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
        // `AlgedonicEvents`'s tolerance is operator-configurable
        // (`algedonic_events_max`, default 0 — option (B) on algedonic card
        // f69162f4, operator ruling 2026-10-19: the knob exists, the policy
        // is unchanged until the operator sets it); the other two stay 0.0
        // — unresolved escalations and critical alerts are never normal.
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
                self.set_points.algedonic_events_max as f64,
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

        signals
    }

    pub(super) async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        let mut actions = Vec::new();

        let policy = RegulationPolicy::default();

        for dev in deviations {
            let proposed = policy.decide(dev);
            if proposed.is_empty() {
                // Fail-visible, never silently dropped: a deviation with no
                // matching (metric, direction) rule is the exact gap the
                // policy-closure test names ("sensed and then silently
                // dropped by decide"). The exhaustive healthy-direction
                // match in `Deviation::from_signal` and this warn are the
                // two halves of the same contract — the match prevents the
                // reachable gap, the warn surfaces any future one.
                tracing::warn!(
                    target: "reg.cybernetics",
                    metric = dev.signal.metric.as_str(),
                    direction = ?dev.direction,
                    value = dev.signal.value,
                    set_point = dev.signal.set_point,
                    "Deviation matched no policy rule — sensed but undispositionable"
                );
                continue;
            }
            for proposed in proposed {
                let action = self.build_regulation_action(dev, proposed).await;
                if let Some(disposition) = action {
                    actions.push(disposition);
                }
            }
        }
        actions
    }

    pub(super) async fn act(&self, actions: &[RegulatoryAction]) {
        // Without board delivery, successful archive fallback is the condition's
        // process-local retention authority. Drop latches as soon as their
        // escalation disposition disappears so a recurrence emits again.
        let active_fallback_conditions = actions
            .iter()
            .filter(|action| {
                action.action_type == ActionType::Escalate && action.target == LoopId::Curation
            })
            .map(|action| {
                let message = regulation_policy::alert_message(
                    &action.parameters.data,
                    &action.parameters.reason,
                );
                regulation_policy::alert_condition(&message).to_string()
            })
            .collect::<std::collections::HashSet<_>>();
        {
            let mut retained = self
                .fallback_alert_conditions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.alert_escalation_sink.is_some() {
                retained.clear();
            } else {
                retained.retain(|condition| active_fallback_conditions.contains(condition));
            }
        }

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
                // Board-reviewable (operator ruling 2026-11-07, dual-axis
                // review T3): fresh cap exhaustion is a discrete, actionable
                // observation — an agent stopped mid-work or a ceiling is
                // mis-set, both worth an operator look. An escalated Warning
                // reaches the board (§9.3, confidence 0.5); the board's
                // open-condition matching coalesces repeats as card comments
                // while D65 keeps each fresh exhaustion uncoalesced on the
                // loop side.
                let alert = RuntimeAlert {
                    domain: domain.clone(),
                    deficit: 1,
                    threshold: 1,
                    severity: AlertSeverity::Warning,
                    escalated: true,
                    timestamp: chrono::Utc::now(),
                    message: message.clone(),
                    occurrence_count: 1,
                };
                let sent = if let Some(ref tx) = self.alerts_tx {
                    tx.send(CurationInput::Alert(alert.clone())).is_ok()
                } else {
                    false
                };
                if !sent {
                    tracing::warn!(target: "reg.alert", domain = %alert.domain, "call-cap exhaustion alert send failed or channel not connected");
                }
                // Deliver to the board; archive only if the live channel
                // cannot carry the event.
                self.deliver_alert_to_board(&alert, None).await;
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

    /// Route an escalation through the review board, live Curator channel,
    /// and archive fallback. A repeat comment on an open card suppresses
    /// duplicate live/archive events. Informational dispositions are not incidents.
    /// Returns true only when the board or archive confirms retention.
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
        let condition = regulation_policy::alert_condition(&message).to_string();
        if self.alert_escalation_sink.is_none()
            && self
                .fallback_alert_conditions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains(&condition)
        {
            tracing::debug!(target: "reg.cybernetics", %condition, "Suppressing duplicate archive-fallback alert");
            return true;
        }
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
            occurrence_count: 1,
        };

        // Deliver the alert to the board. The RegulationArchive remains
        // a secondary fallback for restart durability.
        // A recovery signal must be a measurable deviation: self-recovery
        // uses `recovered_by` only on `is_recovery_trigger`. The
        // observations map holds the previous sense pass's snapshot, so an
        // event-driven escalation routed mid-sense (a reopened circuit)
        // would otherwise attach a stale, healthy reading — a trigger that
        // can never be recovered-by, stranding the card in manual review. Drop non-trigger
        // observations; such an escalation becomes manual-review-only
        // (null recovery signal), like permanent inference failures.
        let observation = action
            .metric_name
            .as_deref()
            .and_then(SignalMetric::from_str_name)
            .and_then(|metric| self.observations.lock().get(&metric).cloned())
            .filter(|signal| signal.is_recovery_trigger());
        let delivery = self
            .deliver_alert_to_board(&alert, observation.as_ref())
            .await;
        if matches!(delivery, Some(crate::AlertDeliveryOutcome::Confirmed(None))) {
            // The existing card received a repeat; no second live alert or
            // archive event should compete with its review record.
            return true;
        }
        // New tool-reliability conditions also emit a per-domain diagnosis
        // span; repeated conditions are recorded only on the existing card.
        if observation
            .as_ref()
            .is_some_and(|signal| signal.metric == SignalMetric::ToolReliability)
        {
            self.emit_tool_outcome_breakdown().await;
        }
        let board_confirmed = matches!(
            delivery,
            Some(crate::AlertDeliveryOutcome::Confirmed(Some(_)))
        );

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

            if !archive_persisted {
                tracing::error!(target: "reg.alert", deficit = deficit, threshold = threshold, "CRITICAL: Algedonic alert LOST - no live channel or event_sink");
            }
        }
        if archive_persisted && self.alert_escalation_sink.is_none() {
            self.fallback_alert_conditions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(condition);
        }
        board_confirmed || archive_persisted
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
                // §9.4 on the log surface: the warn is transition-oriented
                // (entry or changed reading), not per-tick.
                if self.condition_warn_transition(&dev.signal) {
                    tracing::warn!(
                        target: "reg.tool",
                        reliability = dev.signal.value,
                        set_point = dev.signal.set_point,
                        "Tool reliability degraded — success rate below threshold"
                    );
                }
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
            RegulationReason::LowConfidenceCountObserved
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
            | RegulationReason::MemoryStoreGrowth
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
            // Carry the stuck-experiment count into escalation evidence; the
            // delivery path enriches the board card's context with the ids.
            RegulationReason::ExperimentationStuckExperimentsExceeded => {
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    proposed.action_type,
                    RegulatoryActionParams::with_data(
                        proposed.reason.as_str(),
                        RegulationData::ExperimentationStuckExperimentsExceeded {
                            count: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
            // Carry the observed native-first rate into escalation evidence —
            // the drift signal the affordance-injection intervention targets.
            RegulationReason::NativeFirstRateExceeded => {
                if self.condition_warn_transition(&dev.signal) {
                    tracing::warn!(
                        target: "reg.agent",
                        native_first_rate = dev.signal.value,
                        set_point = dev.signal.set_point,
                        "Native-first rate exceeded — substantive turns completing without instruments"
                    );
                }
                Some(RegulatoryAction::with_metric(
                    proposed.target,
                    proposed.action_type,
                    RegulatoryActionParams::with_data(
                        proposed.reason.as_str(),
                        RegulationData::NativeFirstRateExceeded {
                            rate: dev.signal.value,
                            threshold: dev.signal.set_point,
                        },
                    ),
                    dev.signal.metric.as_str().into(),
                ))
            }
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
                    // Source not wired — fall back to NoData, matching the
                    // pre-fix behavior.
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
mod tests;
