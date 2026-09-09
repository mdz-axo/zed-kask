//! Curator-directive application — the Curation→Cybernetics compliance subloop.
//!
//! Extracted from the cybernetics_loop god-module. `process_inbox` (in the
//! facade) drains the directive channel and calls `handle_curation_directive`,
//! which dampens repeated directives, applies each via `apply_directive`, and
//! persists an acknowledgment carrying the ACTUAL outcome (`DirectiveOutcome`):
//! "applied" only when a supported effect changed live state, "recorded" for
//! persisted requests, "log_only" when no effect handler exists, and the
//! escalation delivery states ("queued"/"attempted"/"missing_sink") for
//! `EscalateDomain`. Dampened directives produce no acknowledgment. The
//! `apply_*` methods are directive-internal.

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use hkask_types::curator::{EscalationSeverity, SchemaEvolutionType};
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};

/// What actually happened when a directive was processed — the truth value
/// the acknowledgment persists. "applied" is reserved for directives whose
/// supported effect changed live state.
#[derive(Clone, PartialEq, Eq)]
enum DirectiveOutcome {
    /// A supported effect was applied to live state.
    Applied,
    /// The request was persisted for later action (no live-state effect).
    Recorded,
    /// No effect handler exists — the directive was logged only.
    LogOnly,
    /// An explicit escalation was routed to the human-review queue.
    Escalated(EscalationDelivery),
}

/// The delivery state of an explicit `EscalateDomain` directive — the
/// acknowledgment's truth value for the human-review path. Queued,
/// attempted, and missing-sink are distinct so a confirmed durable
/// delivery is never conflated with a best-effort handoff or a missing
/// sink.
#[derive(Clone, PartialEq, Eq)]
enum EscalationDelivery {
    /// Confirmed in the reviewable queue (the queue-assigned id when the
    /// write inserted a new row; `None` when an existing pending row was
    /// superseded in place).
    Queued(Option<String>),
    /// Attempted but not confirmed — a best-effort sink that cannot report,
    /// or a failed write (surfaced via warn).
    Attempted,
    /// No escalation sink is wired.
    MissingSink,
}

impl DirectiveOutcome {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Recorded => "recorded",
            Self::LogOnly => "log_only",
            Self::Escalated(EscalationDelivery::Queued(_)) => "queued",
            Self::Escalated(EscalationDelivery::Attempted) => "attempted",
            Self::Escalated(EscalationDelivery::MissingSink) => "missing_sink",
        }
    }
}

impl super::CyberneticsLoop {
    pub(super) async fn handle_curation_directive(&self, directive: CuratorDirective) {
        // Dampen repeated directives to prevent feedback oscillation
        if self.dampener.should_dampen_directive(&directive) {
            tracing::debug!(
                target: "reg.cybernetics",
                directive = %directive.variant_name(),
                "Directive dampened (repeated within window)"
            );
        } else {
            let variant_name = directive.variant_name();
            let (outcome, payload) = self.apply_directive(directive).await;
            // The evolve request persists its own specific "recorded" record
            // carrying the full payload — a generic acknowledgment would
            // duplicate it (and previously lied "applied" on top of it).
            if outcome != DirectiveOutcome::Recorded {
                self.persist_directive_acknowledgment(variant_name, &outcome, payload);
            }
            tracing::info!(
                target: "reg.cybernetics",
                directive = %variant_name,
                outcome = outcome.as_str(),
                "Directive processed (Curation→Cybernetics compliance)"
            );
        }
    }

    async fn apply_directive(
        &self,
        directive: CuratorDirective,
    ) -> (DirectiveOutcome, Option<serde_json::Value>) {
        // Every variant is matched explicitly: adding a variant to
        // `CuratorDirective` without choosing an outcome here is a compile
        // error, not a silently-ignored directive acknowledged as applied.
        match directive {
            CuratorDirective::CalibrateThreshold {
                domain,
                new_threshold,
            } => {
                self.apply_calibrate_threshold(&domain, new_threshold).await;
                (DirectiveOutcome::Applied, None)
            }
            CuratorDirective::OverrideEnergyBudget { agent, new_budget } => {
                self.apply_override_cap(agent, new_budget).await;
                (DirectiveOutcome::Applied, None)
            }
            CuratorDirective::ClearOverride { agent } => {
                self.apply_clear_override(agent).await;
                (DirectiveOutcome::Applied, None)
            }
            CuratorDirective::ReplenishBudget {
                agent,
                amount,
                priority: _,
            } => {
                self.apply_credit_calls(agent, amount).await;
                (DirectiveOutcome::Applied, None)
            }
            CuratorDirective::UpdateCapabilities {
                agent,
                additions,
                removals,
            } => {
                // No capability-mutation handler exists (and none may be
                // invented just to make an acknowledgment true — capability
                // authority is consent-gated elsewhere). Logged only.
                tracing::info!(
                    target: "reg.cybernetics",
                    agent = %agent,
                    additions = ?additions,
                    removals = ?removals,
                    "UpdateCapabilities directive received — no capability-mutation handler exists; logged only"
                );
                (DirectiveOutcome::LogOnly, None)
            }
            CuratorDirective::SeekMoreEvidence {
                context,
                channel,
                confidence,
            } => {
                // No evidence-seeking handler is wired from this loop.
                // Logged only — the acknowledgment must not claim a
                // metacognition trigger that did not happen.
                tracing::info!(
                    target: "reg.cybernetics",
                    context = %context,
                    channel = %channel,
                    confidence = %confidence,
                    "SeekMoreEvidence directive received — no evidence-seeking handler exists; logged only"
                );
                (DirectiveOutcome::LogOnly, None)
            }
            CuratorDirective::EscalateDomain {
                domain,
                severity,
                evidence,
            } => self.apply_escalate_domain(domain, severity, evidence).await,
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
                (DirectiveOutcome::Recorded, None)
            }
        }
    }

    /// Deliver an explicit domain escalation to the reviewable escalation
    /// queue (the `curator_escalations` human-review surface), retaining
    /// domain, severity, and evidence. The queue entry is marked `explicit`
    /// and carries NO deficit/threshold fields — an explicit concern is a
    /// request for review, not a fabricated measured threshold breach.
    ///
    /// The message's condition key ("Explicit escalation ({domain},
    /// {severity})") is stable per concern: a re-raised concern updates the
    /// pending row (supersede) instead of duplicating it; different concerns
    /// get their own rows. Rapid repeats are handled by the directive
    /// dampener.
    ///
    /// NOT routed to the live `CurationInput` channel: that channel only
    /// carries `RuntimeAlert` (measured deficit/threshold); dressing an
    /// explicit concern as one would fabricate a sensor reading. The queue
    /// is the human-review path of record; the acknowledgment record (with
    /// the full payload) is the archive copy.
    async fn apply_escalate_domain(
        &self,
        domain: String,
        severity: EscalationSeverity,
        evidence: String,
    ) -> (DirectiveOutcome, Option<serde_json::Value>) {
        let (severity_str, confidence) = match severity {
            EscalationSeverity::Info => ("info", 0.25),
            EscalationSeverity::Warning => ("warning", 0.5),
            EscalationSeverity::Critical => ("critical", 1.0),
        };
        let output = format!("Explicit escalation ({domain}, {severity_str}) — {evidence}");
        let error_context = serde_json::json!({
            "explicit": true,
            "domain": domain,
            "severity": severity_str,
            "evidence": evidence,
        })
        .to_string();

        let delivery = match &self.alert_escalation_sink {
            None => {
                tracing::warn!(
                    target: "reg.cybernetics",
                    domain = %domain,
                    "EscalateDomain directive received — no escalation sink wired; not delivered"
                );
                EscalationDelivery::MissingSink
            }
            Some(sink) => match sink.try_persist_alert(&output, confidence, &error_context) {
                Ok(crate::AlertQueueOutcome::Confirmed(id)) => EscalationDelivery::Queued(id),
                Ok(crate::AlertQueueOutcome::Attempted) => EscalationDelivery::Attempted,
                Err(error) => {
                    tracing::warn!(
                        target: "reg.cybernetics",
                        domain = %domain,
                        error = %error,
                        "EscalateDomain queue write failed — surfaced as attempted, not queued"
                    );
                    EscalationDelivery::Attempted
                }
            },
        };

        let mut payload = serde_json::json!({
            "domain": domain,
            "severity": severity_str,
            "evidence": evidence,
        });
        if let EscalationDelivery::Queued(Some(ref id)) = delivery {
            payload["escalation_id"] = serde_json::Value::String(id.clone());
        }
        (DirectiveOutcome::Escalated(delivery), Some(payload))
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

    fn persist_directive_acknowledgment(
        &self,
        directive_type: &str,
        outcome: &DirectiveOutcome,
        payload: Option<serde_json::Value>,
    ) {
        if let Some(ref sink) = self.event_sink {
            let mut observation = serde_json::json!({
                "directive_type": directive_type,
                "outcome": outcome.as_str(),
            });
            // Merge the directive-specific payload (an explicit escalation's
            // domain/severity/evidence and queue id) into the acknowledgment
            // — the archive copy of the delivered concern.
            if let (Some(fields), Some(ack)) = (payload, observation.as_object_mut()) {
                for (key, value) in fields.as_object().into_iter().flatten() {
                    ack.insert(key.clone(), value.clone());
                }
            }
            let ack = RegulationRecord::new(
                WebID::from_persona(b"regulation"),
                Span::from_kind(SpanKind::CurationDirectiveAcknowledged),
                CyclePhase::Act,
                observation,
                0,
            );
            if let Err(e) = sink.persist(&ack) {
                tracing::warn!(
                    target: "reg.cybernetics",
                    error = %e,
                    "Failed to persist directive acknowledgment"
                );
            }
        } else {
            tracing::warn!(
                target: "reg.cybernetics",
                directive_type,
                "Directive acknowledgment dropped — no event_sink configured"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CyberneticsLoop;
    use crate::runtime::RegulationLedger;
    use hkask_types::curator::EscalationSeverity;
    use std::sync::{Arc, Mutex};
    use tokio::sync::{RwLock, mpsc};

    /// Capturing RegulationSink — records every persisted span's path and
    /// observation (the same pattern as the cycle tests) so acknowledgment
    /// outcomes can be asserted without a durable archive.
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

    /// Sink whose persist always fails — the acknowledgment-failure control.
    struct FailingSink;

    impl hkask_types::RegulationSink for FailingSink {
        fn persist(
            &self,
            _event: &hkask_types::RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            Err(hkask_types::InfrastructureError::Serialization(
                "sink unavailable".to_string(),
            ))
        }
    }

    fn directive_acks(sink: &CapturingSink) -> Vec<serde_json::Value> {
        sink.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(path, _)| path.contains("directive_acknowledged"))
            .map(|(_, observation)| observation.clone())
            .collect()
    }

    fn outcome_of(observation: &serde_json::Value) -> &str {
        observation
            .get("outcome")
            .and_then(|outcome| outcome.as_str())
            .unwrap_or("<missing>")
    }

    fn ack_for<'a>(acks: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
        acks.iter()
            .find(|observation| {
                observation.get("directive_type").and_then(|t| t.as_str()) == Some(name)
            })
            .unwrap_or_else(|| panic!("no acknowledgment for {name}: {acks:#?}"))
    }

    async fn loop_with_sink(
        sink: Arc<dyn hkask_types::RegulationSink>,
    ) -> (CyberneticsLoop, mpsc::UnboundedSender<CuratorDirective>) {
        let ledger = Arc::new(RwLock::new(RegulationLedger::default()));
        let (tx, rx) = mpsc::unbounded_channel();
        let regulation_loop = CyberneticsLoop::new(ledger)
            .with_event_sink(sink)
            .with_curator_directive_channel(rx);
        (regulation_loop, tx)
    }

    async fn cap_status(loop_: &CyberneticsLoop, agent: WebID) -> (u32, u32) {
        loop_
            .call_cap_manager
            .read()
            .await
            .all_agent_statuses()
            .await
            .into_iter()
            .find(|(id, _)| *id == agent)
            .map(|(_, status)| (status.ceiling, status.remaining))
            .expect("agent cap registered")
    }

    /// T07: every directive variant's acknowledgment reports what actually
    /// happened. Real effects are "applied" (with the state change asserted),
    /// persisted requests are "recorded" exactly once, log-only variants are
    /// "log_only", and variants with no handler are "unsupported". Pre-fix,
    /// every non-dampened directive persisted "applied" regardless of effect,
    /// and the evolve request produced a second, false "applied" record on
    /// top of its honest "recorded" one.
    ///
    /// Two loops: the metacognitive override cooldown (120s, all overrides
    /// suppressed after any metacognitive directive passes dedup) would
    /// dampen the cap batches after batch 1's SeekMoreEvidence — existing
    /// dampener behavior, not the behavior under test.
    #[tokio::test]
    async fn directive_acknowledgments_report_actual_outcomes() {
        // Loop A — the variants with no call-cap effect.
        let sink_a = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (loop_a, tx_a) =
            loop_with_sink(Arc::clone(&sink_a) as Arc<dyn hkask_types::RegulationSink>).await;

        let agent = WebID::from_persona(b"agent-a");
        for directive in [
            CuratorDirective::CalibrateThreshold {
                domain: "inference".to_string(),
                new_threshold: 42,
            },
            CuratorDirective::UpdateCapabilities {
                agent,
                additions: vec!["tool_x".to_string()],
                removals: vec![],
            },
            CuratorDirective::SeekMoreEvidence {
                context: "decision".to_string(),
                channel: "llm_confidence".to_string(),
                confidence: "0.5".to_string(),
            },
            CuratorDirective::EscalateDomain {
                domain: "storage".to_string(),
                severity: EscalationSeverity::Warning,
                evidence: "variety deficit".to_string(),
            },
            CuratorDirective::EvolveMcpToolSchema {
                server_name: "hkask-mcp-companies".to_string(),
                tool_name: "dcf_valuation".to_string(),
                evolution_type: SchemaEvolutionType::AddField,
                field_name: "discount_rate".to_string(),
                new_type: Some("f64".to_string()),
                rationale: "skill-use reports omit the rate".to_string(),
                evidence: "skill dcf step 3".to_string(),
            },
        ] {
            tx_a.send(directive).expect("send directive");
        }
        loop_a.process_inbox().await;

        let acks = directive_acks(&sink_a);
        assert_eq!(
            acks.len(),
            5,
            "exactly one acknowledgment per directive — the evolve request must \
             not also produce a generic applied record: {acks:#?}"
        );

        // Real effect → applied.
        assert_eq!(outcome_of(ack_for(&acks, "calibrate_threshold")), "applied");
        // Log-only variants → log_only (no capability-mutation or
        // evidence-seeking handler exists — inventing one is forbidden).
        assert_eq!(
            outcome_of(ack_for(&acks, "update_capabilities")),
            "log_only",
            "update_capabilities has no effect handler — must not be acknowledged as applied"
        );
        assert_eq!(
            outcome_of(ack_for(&acks, "seek_more_evidence")),
            "log_only",
            "seek_more_evidence has no effect handler — must not be acknowledged as applied"
        );
        // No escalation sink wired in this loop → missing_sink (T08 wired
        // the delivery path; the no-sink state is surfaced, not claimed
        // as delivered).
        assert_eq!(
            outcome_of(ack_for(&acks, "escalate_domain")),
            "missing_sink",
            "escalate_domain with no sink wired must surface missing_sink, not claim delivery"
        );
        // Persisted request → recorded, exactly once, with the payload.
        let evolve = ack_for(&acks, "evolve_mcp_tool_schema");
        assert_eq!(outcome_of(evolve), "recorded");
        assert_eq!(
            evolve.get("field_name").and_then(|f| f.as_str()),
            Some("discount_rate"),
            "the recorded request must carry the evolution payload"
        );

        // Loop B — the call-cap variants, each batch with a real state
        // assertion. Credit saturates at the ceiling, so calls are consumed
        // first to make the replenish observable.
        let sink_b = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (loop_b, tx_b) =
            loop_with_sink(Arc::clone(&sink_b) as Arc<dyn hkask_types::RegulationSink>).await;
        let cap_manager = loop_b.call_cap_manager.read().await;
        cap_manager.register_call_cap(agent, 10).await;
        for _ in 0..8 {
            cap_manager.charge_metered(&agent).await;
        }
        drop(cap_manager);
        assert_eq!(cap_status(&loop_b, agent).await, (10, 2));

        // Replenish: remaining calls must increase.
        tx_b.send(CuratorDirective::ReplenishBudget {
            agent,
            amount: 3,
            priority: None,
        })
        .expect("send replenish");
        loop_b.process_inbox().await;
        let acks_b = directive_acks(&sink_b);
        assert_eq!(outcome_of(ack_for(&acks_b, "replenish_budget")), "applied");
        assert_eq!(
            cap_status(&loop_b, agent).await,
            (10, 5),
            "the replenish must actually credit 3 calls (2 + 3, under the ceiling)"
        );

        // Override: real state must change.
        tx_b.send(CuratorDirective::OverrideEnergyBudget {
            agent,
            new_budget: 5,
        })
        .expect("send override");
        loop_b.process_inbox().await;
        let acks_b = directive_acks(&sink_b);
        assert_eq!(
            outcome_of(ack_for(&acks_b, "override_energy_budget")),
            "applied"
        );
        assert_eq!(
            cap_status(&loop_b, agent).await,
            (5, 5),
            "the override must actually install ceiling 5"
        );

        // Clear override: original ceiling restored.
        tx_b.send(CuratorDirective::ClearOverride { agent })
            .expect("send clear");
        loop_b.process_inbox().await;
        let acks_b = directive_acks(&sink_b);
        assert_eq!(outcome_of(ack_for(&acks_b, "clear_override")), "applied");
        assert_eq!(
            cap_status(&loop_b, agent).await,
            (10, 10),
            "clearing the override must restore the original ceiling 10"
        );

        // Exactly one acknowledgment per directive on each loop.
        assert_eq!(directive_acks(&sink_a).len(), 5);
        assert_eq!(directive_acks(&sink_b).len(), 3);
    }

    /// T07 control: a dampened (repeated) directive produces no
    /// acknowledgment — dampening is not application.
    #[tokio::test]
    async fn dampened_directive_is_not_acknowledged() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (regulation_loop, tx) =
            loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;

        for _ in 0..2 {
            tx.send(CuratorDirective::CalibrateThreshold {
                domain: "inference".to_string(),
                new_threshold: 42,
            })
            .expect("send directive");
        }
        regulation_loop.process_inbox().await;

        let acks = directive_acks(&sink);
        assert_eq!(
            acks.len(),
            1,
            "the repeated directive is dampened and must not be acknowledged: {acks:#?}"
        );
    }

    /// T07 control: an acknowledgment-persist failure is surfaced (warn) and
    /// does not panic or wedge the inbox — but it never upgrades the
    /// directive's outcome.
    #[tokio::test]
    async fn persist_failure_does_not_wedge_the_inbox() {
        let (regulation_loop, tx) =
            loop_with_sink(Arc::new(FailingSink) as Arc<dyn hkask_types::RegulationSink>).await;

        tx.send(CuratorDirective::CalibrateThreshold {
            domain: "inference".to_string(),
            new_threshold: 42,
        })
        .expect("send directive");
        regulation_loop.process_inbox().await;

        // The inbox drained (the send below would block otherwise is not
        // assertable on an unbounded channel, but processing completing
        // without panic is the contract).
        tx.send(CuratorDirective::CalibrateThreshold {
            domain: "storage".to_string(),
            new_threshold: 7,
        })
        .expect("send after failure");
        regulation_loop.process_inbox().await;
    }

    /// Escalation sink that records what it received and reports a scripted
    /// `try_persist_alert` outcome — the T08 delivery seam under test.
    struct ScriptedEscalationSink {
        received: Mutex<Vec<(String, f64, String)>>,
        result: Result<crate::AlertQueueOutcome, String>,
    }

    impl ScriptedEscalationSink {
        fn confirmed(id: &str) -> Self {
            Self {
                received: Mutex::new(Vec::new()),
                result: Ok(crate::AlertQueueOutcome::Confirmed(Some(id.to_string()))),
            }
        }
    }

    impl crate::AlertEscalationSink for ScriptedEscalationSink {
        fn persist_alert(&self, output: &str, confidence: f64, error_context: &str) {
            self.received
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((output.to_string(), confidence, error_context.to_string()));
        }

        fn try_persist_alert(
            &self,
            output: &str,
            confidence: f64,
            error_context: &str,
        ) -> Result<crate::AlertQueueOutcome, String> {
            self.persist_alert(output, confidence, error_context);
            self.result.clone()
        }
    }

    /// Escalation sink that only implements the best-effort `persist_alert`
    /// (the trait default `try_persist_alert`) — attempted, unconfirmed.
    struct BestEffortEscalationSink(Mutex<Vec<(String, f64, String)>>);

    impl crate::AlertEscalationSink for BestEffortEscalationSink {
        fn persist_alert(&self, output: &str, confidence: f64, error_context: &str) {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push((
                output.to_string(),
                confidence,
                error_context.to_string(),
            ));
        }
    }

    fn escalate_directive() -> CuratorDirective {
        CuratorDirective::EscalateDomain {
            domain: "storage".to_string(),
            severity: EscalationSeverity::Warning,
            evidence: "variety deficit".to_string(),
        }
    }

    /// T08: an undampened explicit escalation is delivered to the reviewable
    /// queue retaining domain, severity, and evidence — and the
    /// acknowledgment reports the confirmed queue outcome with the
    /// escalation id. The queue payload must NOT fabricate measured
    /// deficit/threshold fields: an explicit concern is not a sensor
    /// reading.
    #[tokio::test]
    async fn explicit_escalation_confirmed_in_queue() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (mut regulation_loop, tx) =
            loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;
        let escalation_sink = Arc::new(ScriptedEscalationSink::confirmed("esc-42"));
        regulation_loop.set_alert_escalation_sink(Some(
            Arc::clone(&escalation_sink) as Arc<dyn crate::AlertEscalationSink>
        ));

        tx.send(escalate_directive()).expect("send escalation");
        regulation_loop.process_inbox().await;

        // The sink received the escalation with the concern's identity.
        let received = escalation_sink
            .received
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(received.len(), 1, "exactly one queue delivery");
        let (output, confidence, error_context) = &received[0];
        assert!(
            output.contains("storage") && output.contains("warning"),
            "the queue output must name the domain and severity: {output}"
        );
        assert_eq!(*confidence, 0.5, "Warning maps to confidence 0.5");
        let context: serde_json::Value = serde_json::from_str(error_context).expect("context");
        assert_eq!(
            context.get("domain").and_then(|d| d.as_str()),
            Some("storage")
        );
        assert_eq!(
            context.get("severity").and_then(|s| s.as_str()),
            Some("warning")
        );
        assert_eq!(
            context.get("evidence").and_then(|e| e.as_str()),
            Some("variety deficit")
        );
        assert_eq!(
            context.get("explicit").and_then(|e| e.as_bool()),
            Some(true),
            "the queue entry must be marked as an explicit escalation"
        );
        assert!(
            context.get("deficit").is_none() && context.get("threshold").is_none(),
            "an explicit escalation must not fabricate measured deficit/threshold fields"
        );

        // The acknowledgment reports the confirmed outcome with the id and
        // retains the concern's identity.
        let acks = directive_acks(&sink);
        assert_eq!(acks.len(), 1);
        let ack = &acks[0];
        assert_eq!(outcome_of(ack), "queued");
        assert_eq!(
            ack.get("escalation_id").and_then(|i| i.as_str()),
            Some("esc-42")
        );
        assert_eq!(ack.get("domain").and_then(|d| d.as_str()), Some("storage"));
        assert_eq!(
            ack.get("severity").and_then(|s| s.as_str()),
            Some("warning")
        );
        assert_eq!(
            ack.get("evidence").and_then(|e| e.as_str()),
            Some("variety deficit")
        );
    }

    /// T08: a confirmed in-place supersede (an existing pending row updated,
    /// no new id) still reports "queued" — the concern IS in the queue.
    #[tokio::test]
    async fn explicit_escalation_supersede_reports_queued_without_id() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (mut regulation_loop, tx) =
            loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;
        regulation_loop.set_alert_escalation_sink(Some(Arc::new(ScriptedEscalationSink {
            received: Mutex::new(Vec::new()),
            result: Ok(crate::AlertQueueOutcome::Confirmed(None)),
        })
            as Arc<dyn crate::AlertEscalationSink>));

        tx.send(escalate_directive()).expect("send escalation");
        regulation_loop.process_inbox().await;

        let acks = directive_acks(&sink);
        assert_eq!(acks.len(), 1);
        assert_eq!(outcome_of(&acks[0]), "queued");
        assert!(
            acks[0].get("escalation_id").is_none(),
            "a superseded row has no new id — the ack must not invent one"
        );
    }

    /// T08: a missing escalation sink is surfaced as "missing_sink" — never
    /// silently dropped and never claimed as delivered.
    #[tokio::test]
    async fn explicit_escalation_without_sink_reports_missing_sink() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (regulation_loop, tx) =
            loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;

        tx.send(escalate_directive()).expect("send escalation");
        regulation_loop.process_inbox().await;

        let acks = directive_acks(&sink);
        assert_eq!(acks.len(), 1);
        assert_eq!(
            outcome_of(&acks[0]),
            "missing_sink",
            "no escalation sink wired — the ack must surface that, not claim delivery"
        );
    }

    /// T08: a failed queue write is surfaced as "attempted" — tried, not
    /// confirmed; never conflated with a confirmed queue write.
    #[tokio::test]
    async fn explicit_escalation_failed_write_reports_attempted() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (mut regulation_loop, tx) =
            loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;
        regulation_loop.set_alert_escalation_sink(Some(Arc::new(ScriptedEscalationSink {
            received: Mutex::new(Vec::new()),
            result: Err("queue unavailable".to_string()),
        })
            as Arc<dyn crate::AlertEscalationSink>));

        tx.send(escalate_directive()).expect("send escalation");
        regulation_loop.process_inbox().await;

        let acks = directive_acks(&sink);
        assert_eq!(acks.len(), 1);
        assert_eq!(
            outcome_of(&acks[0]),
            "attempted",
            "a failed write was tried — the ack must not claim it queued"
        );
    }

    /// T08: a best-effort sink (the trait default — cannot report the
    /// durable outcome) is "attempted", distinct from confirmed.
    #[tokio::test]
    async fn explicit_escalation_best_effort_sink_reports_attempted() {
        let sink = Arc::new(CapturingSink(Mutex::new(Vec::new())));
        let (mut regulation_loop, tx) =
            loop_with_sink(Arc::clone(&sink) as Arc<dyn hkask_types::RegulationSink>).await;
        regulation_loop.set_alert_escalation_sink(Some(Arc::new(BestEffortEscalationSink(
            Mutex::new(Vec::new()),
        ))
            as Arc<dyn crate::AlertEscalationSink>));

        tx.send(escalate_directive()).expect("send escalation");
        regulation_loop.process_inbox().await;

        let acks = directive_acks(&sink);
        assert_eq!(acks.len(), 1);
        assert_eq!(outcome_of(&acks[0]), "attempted");
    }
}
