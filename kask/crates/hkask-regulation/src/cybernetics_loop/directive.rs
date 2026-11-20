//! Curator-directive application — the Curation→Cybernetics compliance subloop.
//!
//! Extracted from the cybernetics_loop god-module. `process_inbox` (in the
//! facade) drains the directive channel and calls `handle_curation_directive`,
//! which dampens repeated directives, applies each via `apply_directive`, and
//! persists an acknowledgment carrying the ACTUAL outcome (`DirectiveOutcome`):
//! "applied" when a supported effect changed live state, and the escalation
//! delivery states ("confirmed"/"attempted"/"missing_sink") for
//! `EscalateDomain`. Dampened directives produce no acknowledgment. The
//! `apply_*` methods are directive-internal.

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use hkask_types::curator::EscalationSeverity;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};

/// What actually happened when a directive was processed — the truth value
/// the acknowledgment persists. "applied" is reserved for directives whose
/// supported effect changed live state.
#[derive(Clone, PartialEq, Eq)]
enum DirectiveOutcome {
    /// A supported effect was applied to live state.
    Applied,
    /// An explicit escalation was routed to the review board.
    Escalated(EscalationDelivery),
}

/// The delivery state of an explicit `EscalateDomain` directive — the
/// acknowledgment's truth value for the board path. Confirmed,
/// attempted, and missing-sink are distinct so a confirmed
/// delivery is never conflated with a best-effort handoff or a missing
/// sink.
#[derive(Clone, PartialEq, Eq)]
enum EscalationDelivery {
    /// Confirmed on the board (new task id, or `None` when the existing
    /// open card received a repeat comment).
    Confirmed(Option<String>),
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
            Self::Escalated(EscalationDelivery::Confirmed(_)) => "confirmed",
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
            self.persist_directive_acknowledgment(variant_name, &outcome, payload);
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
            CuratorDirective::EscalateDomain {
                domain,
                severity,
                evidence,
            } => self.apply_escalate_domain(domain, severity, evidence).await,
        }
    }

    /// Deliver an explicit domain escalation to the Algedonic review board,
    /// retaining domain, severity and evidence. Its card carries NO measured
    /// deficit/threshold fields — an explicit concern is a
    /// request for review, not a fabricated measured threshold breach.
    ///
    /// The message's condition key ("Explicit escalation ({domain},
    /// {severity})") is stable per concern: a re-raised concern updates the
    /// existing open card via a comment; different concerns get their own cards. Rapid repeats are handled by the directive
    /// dampener.
    ///
    /// NOT routed to the live `CurationInput` channel: that channel only
    /// carries `RuntimeAlert` (measured deficit/threshold); dressing an
    /// explicit concern as one would fabricate a sensor reading. The board
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
            Some(sink) => match sink
                .try_persist_alert(&output, confidence, &error_context)
                .await
            {
                Ok(crate::AlertDeliveryOutcome::Confirmed(id)) => EscalationDelivery::Confirmed(id),
                Ok(crate::AlertDeliveryOutcome::Attempted) => EscalationDelivery::Attempted,
                Err(error) => {
                    tracing::warn!(
                        target: "reg.cybernetics",
                        domain = %domain,
                        error = %error,
                        "EscalateDomain board write failed — surfaced as attempted, not confirmed"
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
        if let EscalationDelivery::Confirmed(Some(ref id)) = delivery {
            payload["task_id"] = serde_json::Value::String(id.clone());
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
mod tests;
