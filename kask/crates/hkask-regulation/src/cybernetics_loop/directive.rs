//! Curator-directive application — the Curation→Cybernetics compliance subloop.
//!
//! Extracted from the cybernetics_loop god-module. `process_inbox` (in the
//! facade) drains the directive channel and calls `handle_curation_directive`,
//! which dampens repeated directives, applies each via `apply_directive`, and
//! persists an acknowledgment. The `apply_*` methods are directive-internal.

use hkask_types::CuratorDirective;
use hkask_types::WebID;
use hkask_types::curator::SchemaEvolutionType;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};

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
        } else {
            tracing::warn!(
                target: "reg.cybernetics",
                directive_type,
                "Directive acknowledgment dropped — no event_sink configured"
            );
        }
    }
}
