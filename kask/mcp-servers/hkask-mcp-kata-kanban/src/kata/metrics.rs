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

    /// Deduct inference token cost from the bound kanban task's gas budget.
    ///
    /// Called after each inference call returns. Uses the actual token usage
    /// from the `InferenceResult` as the cost. When no task gas accountant
    /// is configured, this is a no-op (the kata engine runs standalone).
    ///
    /// `reason` describes the call: "inference: {model} ({tokens} tokens)".
    ///
    /// `[P9]` Motivating: Homeostatic Self-Regulation — closes the per-task gas loop.
    /// pre:  result is a valid InferenceResult with usage data
    /// post: task.gas_remaining is decremented by total_tokens; GasEntry appended to audit trail
    pub(super) fn deduct_task_gas(&self, result: &hkask_types::InferenceResult, step_label: &str) {
        let Some(ref accountant) = self.task_gas_accountant else {
            return;
        };
        let cost = u64::from(result.usage.total_tokens);
        if cost == 0 {
            return; // No tokens consumed — nothing to deduct
        }
        let reason = format!(
            "inference: {} ({} tokens) [{}]",
            result.model, cost, step_label
        );
        match accountant(cost, &reason) {
            Ok(remaining) => {
                tracing::debug!(
                    target: "reg.kata",
                    step = %step_label,
                    cost = cost,
                    remaining = remaining,
                    "Task gas deducted"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.kata",
                    step = %step_label,
                    cost = cost,
                    error = %e,
                    "Failed to deduct task gas"
                );
            }
        }
    }
}
