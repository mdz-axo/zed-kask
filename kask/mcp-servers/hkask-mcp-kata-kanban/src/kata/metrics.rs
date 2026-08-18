//! Metrics capture and Regulation variety — before/after measurement and improvement signal computation.

use super::*;

impl KataEngine {
    pub(super) fn capture_before_metrics(
        &self,
        manifest: &KataManifest,
        agent: &str,
        state: &mut KataState,
    ) {
        if manifest.metrics.is_empty() {
            return;
        }
        let Some(collector) = self.metric_collector.as_ref() else {
            return;
        };
        let mut metrics = serde_json::Map::new();
        for m in &manifest.metrics {
            if let Some(ref span) = m.span {
                match collector(agent, span) {
                    Ok(value) => {
                        metrics.insert(m.name.clone(), value);
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.kata",
                            metric = %m.name,
                            error = %e,
                            "Failed to capture before metric"
                        );
                    }
                }
            }
        }
        if !metrics.is_empty() {
            state.metric_before = Some(serde_json::Value::Object(metrics));
        }
    }

    pub(super) fn capture_after_metrics(
        &self,
        manifest: &KataManifest,
        agent: &str,
        state: &mut KataState,
    ) {
        if manifest.metrics.is_empty() {
            return;
        }
        let Some(collector) = self.metric_collector.as_ref() else {
            return;
        };
        let mut metrics = serde_json::Map::new();
        for m in &manifest.metrics {
            if let Some(ref span) = m.span {
                match collector(agent, span) {
                    Ok(value) => {
                        metrics.insert(m.name.clone(), value);
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "reg.kata",
                            metric = %m.name,
                            error = %e,
                            "Failed to capture after metric"
                        );
                    }
                }
            }
        }
        if !metrics.is_empty() {
            state.metric_after = Some(serde_json::Value::Object(metrics));
        }
    }

    pub(super) fn compute_improvement_signal(
        &self,
        state: &KataState,
    ) -> Option<ImprovementSignal> {
        let before = state.metric_before.as_ref()?;
        let after = state.metric_after.as_ref()?;

        let delta = match (before, after) {
            (serde_json::Value::Number(b), serde_json::Value::Number(a)) => {
                let bf = b.as_f64()?;
                let af = a.as_f64()?;
                Some(af - bf)
            }
            _ => None,
        };

        let direction = match delta {
            Some(d) if d > 0.0 => ImprovementDirection::Positive,
            Some(d) if d < 0.0 => ImprovementDirection::Negative,
            Some(_) => ImprovementDirection::Stalled,
            None => ImprovementDirection::NotMeasured,
        };

        Some(ImprovementSignal {
            metric_before: Some(before.clone()),
            metric_after: Some(after.clone()),
            delta,
            direction,
        })
    }

    pub(super) async fn increment_ledger_variety(&self, domain: &str, state_name: &str) {
        if let Some(ref ledger) = self.ledger_runtime {
            ledger
                .read()
                .await
                .increment_variety(domain, state_name)
                .await;
        }
    }

    pub(super) async fn check_reg_alerts(&self, manifest: &KataManifest, kata_type: &str) {
        let Some(ref ledger) = self.ledger_runtime else {
            return;
        };
        let alert = ledger
            .read()
            .await
            .check_variety(&manifest.ledger.span_namespace)
            .await;
        if let Some(a) = alert {
            tracing::warn!(
                target: "reg.kata",
                namespace = %manifest.ledger.span_namespace,
                kata_type = %kata_type,
                severity = ?a.severity,
                deficit = a.deficit,
                threshold = a.threshold,
                "REG"
            );
        }
    }

    /// Deduct inference cost from the bound kanban task's rJoule budget.
    ///
    /// Called after each inference call returns. Uses the observed USD cost
    /// from the `InferenceResult` (`cost_usd`), not token counts. rJoule is
    /// the inference energy budget (1 rJoule = $1 USD). When no task rJoule
    /// accountant is configured, this is a no-op (the kata engine runs
    /// standalone).
    ///
    /// `reason` describes the call: "inference: {model} (${cost}) [{step}]".
    ///
    /// `[P9]` Motivating: Homeostatic Self-Regulation — closes the per-task rJoule loop.
    /// pre:  result is a valid InferenceResult with cost_usd data
    /// post: task.rjoule_remaining is decremented by cost_usd (as micro-rJoules); GasEntry appended to audit trail
    pub(super) fn deduct_task_rjoules(&self, result: &hkask_types::InferenceResult, step_label: &str) {
        let Some(ref accountant) = self.task_gas_accountant else {
            return;
        };
        let cost_usd = result.cost_usd.unwrap_or(0.0);
        if cost_usd <= 0.0 {
            return; // No cost reported — nothing to deduct (local Ollama, zed IPC bridge)
        }
        // Convert USD to micro-rJoules (1 rJoule = $1, 1 micro-rJoule = $0.000001)
        // to preserve precision in the u64 budget.
        let cost_micro_rjoules = (cost_usd * 1_000_000.0) as u64;
        let reason = format!(
            "inference: {} (${:.4}) [{}]",
            result.model, cost_usd, step_label
        );
        match accountant(cost_micro_rjoules, &reason) {
            Ok(remaining) => {
                tracing::debug!(
                    target: "reg.kata",
                    step = %step_label,
                    cost_usd = cost_usd,
                    cost_micro_rjoules = cost_micro_rjoules,
                    remaining = remaining,
                    "Task rJoules deducted"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.kata",
                    step = %step_label,
                    cost_usd = cost_usd,
                    error = %e,
                    "Failed to deduct task rJoules"
                );
            }
        }
    }
}
