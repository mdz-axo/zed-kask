//! `RegulationLoop` trait implementation for `MetacognitionLoop`.

use crate::ports::{EscalationBatch, EscalationEntry};
use hkask_regulation::meta_span::emit_meta_escalation;
use hkask_regulation::types::loops::{
    ActionType, Deviation, LoopId, RegulationLoop, RegulatoryAction, Signal, SignalMetric,
};
use hkask_types::BotID;
use tracing::{info, warn};

use super::config::{HealthSnapshot, MC_TARGET};
use super::format::format_health_status;
use super::loop_body::MetacognitionLoop;
use super::persistence::persist_escalation_with_retry;
use hkask_types::curator::EscalationSeverity;

// RegulationLoop — sense → compare → compute → act
#[async_trait::async_trait]
impl RegulationLoop for MetacognitionLoop {
    fn id(&self) -> LoopId {
        // Metacognition is a worker within Curation (Loop 5), not a governing loop.
        LoopId::Curation
    }

    /// Sense: read Regulation health, variety counters, alerts, and bot status.
    /// Builds and stores a HealthSnapshot.
    async fn sense(&self) -> Vec<Signal> {
        info!(target: MC_TARGET, "Starting metacognition sense phase");

        let reg_health = self.context.ledger().health().await;
        let reg_health_str = format_health_status(&reg_health);

        let variety_counters = self.context.ledger().variety().await;
        let all_alerts = self.context.ledger().alerts().await;
        let critical_alerts = self.context.ledger().critical_alerts().await;
        let reg_health = self.context.ledger().regulation_health().await;

        // Compute total variety deficit (same logic as evaluate_and_adapt)
        let mut total_variety_deficit = 0u64;
        for (ns, variety) in &variety_counters {
            let deficit = self
                .config
                .expected_variety_per_domain
                .saturating_sub(*variety);
            if deficit > 0 {
                total_variety_deficit += deficit;
                if deficit > self.config.thresholds.variety_deficit {
                    warn!(
                        target: MC_TARGET,
                        domain = %ns.as_str(),
                        variety = variety,
                        deficit = deficit,
                        "Variety deficit exceeds threshold"
                    );
                }
            }
        }

        // Delegate escalation condition checking to the policy.
        let alerts = self.escalation_policy.check_conditions(
            total_variety_deficit as f64,
            critical_alerts.len() as u64,
            0, // bot_failures: no bot health subsystem
        );
        for alert in &alerts {
            match alert.severity {
                EscalationSeverity::Info => {}
                EscalationSeverity::Warning => warn!(
                    target: MC_TARGET,
                    trigger = ?alert.trigger,
                    value = alert.value,
                    threshold = alert.threshold,
                    "Escalation policy: warning condition detected"
                ),
                EscalationSeverity::Critical => warn!(
                    target: MC_TARGET,
                    trigger = ?alert.trigger,
                    value = alert.value,
                    threshold = alert.threshold,
                    "Escalation policy: critical condition detected"
                ),
            }
        }

        // Build and store snapshot for compute/act phases
        let regulation_effectiveness = reg_health.effectiveness();
        let snapshot = HealthSnapshot {
            timestamp: chrono::Utc::now(),
            reg_health: reg_health_str,
            variety_counters: variety_counters.clone(),
            variety_deficit: total_variety_deficit,
            critical_alerts: critical_alerts.len(),
            total_alerts: all_alerts.len(),
            regulation_effectiveness,
        };
        // `send_replace` returns the previous value and Errs only if the
        // channel is closed — which can't happen here because we own the
        // `Sender`. Ignore the previous value (we just wrote).
        let _ = self.last_snapshot_tx.send_replace(Some(snapshot));

        // Produce afferent signals
        let lid = LoopId::Curation;
        let t = &self.config.thresholds;
        vec![
            Signal::new(
                lid,
                SignalMetric::MetacognitionVarietyDeficit,
                total_variety_deficit as f64,
                t.variety_deficit as f64,
            ),
            Signal::new(
                lid,
                SignalMetric::MetacognitionCriticalAlerts,
                critical_alerts.len() as f64,
                t.critical_alerts as f64 - 0.5,
            ),
        ]
    }

    /// Compute: map deviations to regulatory actions.
    ///
    /// Per P3 (Generative Space), when a ManifestExecutor is available,
    /// calibrated decisions are produced by KnowAct templates, not Rust
    /// threshold comparison. Falls back to Rust logic when no executor
    /// is configured (standalone CLI).
    async fn compute(&self, deviations: &[Deviation]) -> Vec<RegulatoryAction> {
        if let Some(executor) = self.context.manifest_executor().await {
            return self.compute_with_templates(&executor, deviations).await;
        }
        self.compute_with_thresholds(deviations)
    }

    /// Act: issue CuratorDirectives, direct bots, and post escalations.
    ///
    /// When a template output is available (from compute_with_templates),
    /// "restart" and "rebalance" actions trigger bot direction via A2A
    /// in addition to escalation queue audit entries.
    async fn act(&self, actions: &[RegulatoryAction]) {
        let mut escalation_entries: Vec<EscalationEntry> = Vec::new();

        for action in actions {
            // Template-driven bot direction: when the LLM says restart/rebalance,
            // send an A2A directive to the target bot before posting the escalation.
            // Data is now typed RegulationData; non-regulation actions carry NoData.
            // Template-driven directives are routed via CuratorDirective instead.
            match action.action_type {
                ActionType::Calibrate => {
                    if let Some(entry) = self.act_on_throttle(action).await {
                        escalation_entries.push(entry);
                    }
                }
                ActionType::Escalate => {
                    if let Some(entry) = self.act_on_escalate(action).await {
                        escalation_entries.push(entry);
                    }
                }
                ActionType::OverrideEnergyBudget => {
                    self.act_on_budget_override(action).await;
                }
                _ => self.act_on_no_action(action),
            }
        }

        // Collect low-confidence escalation signals for epistemic routing.
        // The Curator does not have direct access to the skill catalog, so it
        // emits a reg.meta.escalation span with outcome "epistemic_route" to
        // signal that a downstream consumer with catalog access should invoke
        // skill-router with an epistemic_state input to find certainty-finding skills.
        let low_confidence_signals: Vec<(f64, String)> = escalation_entries
            .iter()
            .filter(|e| e.confidence < 0.5 && e.retry_count < 3)
            .map(|e| (e.confidence, e.output.clone()))
            .collect();

        // Write escalations: batch if concurrent count meets/exceeds threshold, otherwise write individually.
        let threshold = self.config.max_concurrent_escalations;
        if escalation_entries.len() >= threshold {
            let batch = EscalationBatch::new(escalation_entries, "consolidated", threshold);
            let summary = batch.summary();
            // Invoke escalate template for LLM-formatted notification when executor available
            let summary = if let Some(executor) = self.context.manifest_executor().await {
                // Feed the actual HealthSnapshot (built in sense()) and the real
                // batch entries into the escalate template — not empty defaults.
                let snap = self.last_snapshot_tx.borrow().clone();
                let entry_outputs: Vec<String> =
                    batch.entries.iter().map(|e| e.output.clone()).collect();
                let mut ctx = std::collections::HashMap::new();
                ctx.insert(
                    "critical_issues".into(),
                    serde_json::json!(entry_outputs.clone()),
                );
                ctx.insert(
                    "system_health".into(),
                    serde_json::json!(
                        snap.as_ref()
                            .map(|s| s.reg_health.clone())
                            .unwrap_or_else(|| "unknown".to_string())
                    ),
                );
                ctx.insert(
                    "variety_deficit".into(),
                    serde_json::json!(snap.as_ref().map(|s| s.variety_deficit).unwrap_or(0)),
                );
                ctx.insert(
                    "active_alerts".into(),
                    serde_json::json!(snap.as_ref().map(|s| s.total_alerts).unwrap_or(0)),
                );
                ctx.insert("bot_failures".into(), serde_json::json!([]));
                ctx.insert("energy_budget_status".into(), serde_json::json!("unknown"));
                ctx.insert("required_actions".into(), serde_json::json!(entry_outputs));
                match executor
                    .execute_knowact("curator/metacognition-escalate.j2", &ctx)
                    .await
                {
                    Ok(output) => output
                        .get("notification")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&summary)
                        .to_string(),
                    Err(_) => summary,
                }
            } else {
                summary
            };
            info!(target: MC_TARGET, batch_id = %batch.id, entry_count = batch.entries.len(), threshold, "Consolidating escalations into batch");
            let batch_template_id = hkask_types::TemplateID::new();
            let batch_error_context = {
                let template_ids: Vec<_> = batch
                    .entries
                    .iter()
                    .map(|e| e.template_id.to_string())
                    .collect();
                format!(
                    "Consolidated batch: {} escalation(s) from templates: {}",
                    batch.entries.len(),
                    template_ids.join(", ")
                )
            };
            let batch_confidence = batch
                .entries
                .iter()
                .map(|e| e.confidence)
                .fold(f64::MAX, f64::min);
            if let Err(e) = persist_escalation_with_retry(
                self.context.escalation_port(),
                batch_template_id,
                BotID::new(),
                &summary,
                batch_confidence,
                0,
                &batch_error_context,
            )
            .await
            {
                self.context.self_quality().record_escalation_dropped();
                if let Some(sink) = self.context.regulation_sink() {
                    emit_meta_escalation(
                        sink.as_ref(),
                        self.context.handle().curator_id(),
                        "dropped",
                        batch_confidence,
                    );
                }
                tracing::error!(
                    target: "reg.curation.escalation",
                    batch_id = %batch.id,
                    error = %e,
                    batch_size = batch.entries.len(),
                    "Failed to persist consolidated escalation batch after retries — escalations LOST"
                );
            } else if let Some(sink) = self.context.regulation_sink() {
                emit_meta_escalation(
                    sink.as_ref(),
                    self.context.handle().curator_id(),
                    "persisted",
                    batch_confidence,
                );
            }
        } else {
            let mut lost_count = 0u32;
            for entry in escalation_entries {
                if let Err(e) = persist_escalation_with_retry(
                    self.context.escalation_port(),
                    entry.template_id,
                    entry.bot_id,
                    &entry.output,
                    entry.confidence,
                    entry.retry_count,
                    &entry.error_context,
                )
                .await
                {
                    lost_count += 1;
                    self.context.self_quality().record_escalation_dropped();
                    if let Some(sink) = self.context.regulation_sink() {
                        emit_meta_escalation(
                            sink.as_ref(),
                            self.context.handle().curator_id(),
                            "dropped",
                            entry.confidence,
                        );
                    }
                    tracing::error!(
                        target: "reg.curation.escalation",
                        template_id = %entry.template_id,
                        bot_id = %entry.bot_id,
                        error = %e,
                        "Failed to persist escalation after retries — escalation LOST"
                    );
                } else if let Some(sink) = self.context.regulation_sink() {
                    emit_meta_escalation(
                        sink.as_ref(),
                        self.context.handle().curator_id(),
                        "persisted",
                        entry.confidence,
                    );
                }
            }
            if lost_count > 0 {
                tracing::error!(
                    target: "reg.curation.escalation",
                    lost = lost_count,
                    "{} escalation(s) could not be persisted — check escalation queue health",
                    lost_count
                );
            }
        }

        // Emit epistemic routing signals for low-confidence escalations.
        // Downstream consumers (with skill catalog access) can invoke skill-router
        // with epistemic_state { confidence, uncertainty_type } to find certainty-
        // finding skills (metacognition, improv, zoom-out, falsifiability).
        if let Some(sink) = self.context.regulation_sink() {
            for (confidence, output) in &low_confidence_signals {
                emit_meta_escalation(
                    sink.as_ref(),
                    self.context.handle().curator_id(),
                    "epistemic_route",
                    *confidence,
                );
                info!(
                    target: "reg.curation.escalation",
                    confidence = *confidence,
                    output_snippet = %output.chars().take(100).collect::<String>(),
                    "Low-confidence escalation flagged for epistemic routing — downstream consumer should invoke skill-router with epistemic_state"
                );
            }
        }

        // Meta-cybernetic self-management: generative-first threshold
        // self-calibration (LLM template) with Rust safety-rail fallback.
        // Route low-confidence escalations through skill-router to find
        // certainty-finding skills. Closes the epistemic_route → epistemic_routed loop.
        self.route_epistemic_escalations(&low_confidence_signals)
            .await;

        // Meta-cybernetic self-management: generative-first threshold
        // self-calibration (LLM template) with Rust safety-rail fallback.
        self.self_calibrate().await;
    }
}
