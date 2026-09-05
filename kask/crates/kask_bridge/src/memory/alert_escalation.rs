//! Alert escalation sink + queue opener — extracted from `memory.rs`
//! (deep-module split: the algedonic alert path implements a *different*
//! port — `hkask_regulation::AlertEscalationSink` — with zero coupling to the
//! memory port). `open_curator_escalation_queue` borrows `curator_db_path`
//! from the parent's re-export of `curator_stores`.

use hkask_storage::open_or_repair;
use std::sync::Arc;

use super::curator_db_path;

/// Open an `EscalationQueue` (reviewable alert backlog) on the curator's
/// sovereign `curator.db` — the same DB the curator MCP server's
/// `curator_escalations` / `curator_escalation_resolve` /
/// `curator_escalation_dismiss` tools read. Returns `None` on any failure;
/// the caller degrades to no escalation-queue persistence with a warn.
///
/// Mirrors the regulation archive opener (`open_regulation_archive`) —
/// same DB, same passphrase, same resolution path. The queue is the
/// primary durable path for alert review: `CyberneticsLoop` writes
/// escalated alerts here unconditionally so the Curator/user can review and
/// resolve them.
pub fn open_curator_escalation_queue(
    passphrase: &str,
) -> Option<Arc<hkask_storage::EscalationQueue>> {
    let db_path = curator_db_path();
    let db = match open_or_repair(&db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                target: "reg.storage",
                error = %e,
                db_path = %db_path,
                "Failed to open curator DB for escalation queue"
            );
            return None;
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, "Failed to get SQLite pool for escalation queue");
            return None;
        }
    };
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path.as_str()),
    );
    match hkask_storage::EscalationQueue::from_driver(driver) {
        Ok(queue) => Some(Arc::new(queue)),
        Err(e) => {
            tracing::warn!(target: "reg.storage", error = %e, "Failed to init EscalationQueue schema");
            None
        }
    }
}
/// Adapter implementing `hkask_regulation::AlertEscalationSink` by forwarding
/// algedonic alerts to the `EscalationQueue` (the reviewable backlog on the
/// curator's `curator.db`).
///
/// This closes the Store seam: `CyberneticsLoop` calls
/// `persist_alert_to_queue` → this adapter → `EscalationQueue::add` → the
/// `escalations` table → `curator_escalations` MCP tool reads it. The queue
/// write is best-effort; a failing or missing queue never breaks the
/// regulation loop.
pub struct BridgeAlertEscalationSink {
    queue: Arc<hkask_storage::EscalationQueue>,
}

impl BridgeAlertEscalationSink {
    pub fn new(queue: Arc<hkask_storage::EscalationQueue>) -> Self {
        Self { queue }
    }
}

impl hkask_regulation::AlertEscalationSink for BridgeAlertEscalationSink {
    fn reconcile_conditions(&self, observations: &[hkask_regulation::Signal]) {
        let entries = match self.queue.list_advice_observations() {
            Ok(entries) => entries,
            Err(error) => { tracing::warn!(target: "reg.alert", %error, "Recovery reconciliation unavailable; alerts retained"); return; }
        };
        let now = chrono::Utc::now();
        for entry in entries {
            let mut context: serde_json::Value = match serde_json::from_str(&entry.error_context) {
                Ok(context) => context,
                Err(error) => { tracing::warn!(target: "reg.alert", %error, "Invalid escalation context"); continue; }
            };
            let Some(value) = context.get("recovery_signal").filter(|value| !value.is_null()) else { continue; };
            let trigger: hkask_regulation::Signal = match serde_json::from_value(value.clone()) {
                Ok(signal) => signal,
                Err(error) => { tracing::warn!(target: "reg.alert", %error, "Invalid recovery signal"); continue; }
            };
            let current = observations.iter().find(|current| current.metric == trigger.metric);
            if context.pointer("/advice_review/finalized").and_then(|value| value.as_bool()) != Some(true) {
                let applied_at = context.get("applied_at").filter(|value| !value.is_null()).map(|value| serde_json::from_value::<chrono::DateTime<chrono::Utc>>(value.clone())).transpose();
                let baseline = context.get("applied_baseline").filter(|value| !value.is_null()).map(|value| serde_json::from_value::<hkask_regulation::Signal>(value.clone())).transpose();
                let (Ok(applied_at), Ok(baseline)) = (applied_at, baseline) else {
                    tracing::warn!(target: "reg.alert", "Invalid advice application metadata; review not performed"); continue;
                };
                let status = trigger.advice_review(baseline.as_ref(), current, applied_at, now);
                context["latest_observation"] = serde_json::json!(current);
                context["advice_review"] = serde_json::json!({
                    "status": status, "observed_at": now, "causal_attribution": "unverified",
                    "finalized": !matches!(status, "awaiting_action" | "observation_window"),
                });
                match self.queue.update_advice_context(&entry.id.to_string(), &entry.error_context, &context.to_string()) {
                    Ok(true) => {},
                    Ok(false) => { tracing::debug!(target: "reg.alert", "Advice changed concurrently; retry on next tick"); continue; },
                    Err(error) => { tracing::warn!(target: "reg.alert", %error, "Advice observation could not be saved"); continue; }
                }
            }
            if current.is_some_and(|current| trigger.recovered_by(current)) {
                self.auto_resolve_cleared(&entry.output, "Fresh observation satisfies the original triggering threshold");
            }
        }
    }

    fn persist_alert(&self, output: &str, confidence: f64, error_context: &str) {
        // Supersede at the source: a pending escalation for the same
        // condition is updated in place (latest output/context,
        // retry_count+1) instead of appending a duplicate row per re-sensed
        // cycle. The condition key strips the per-cycle value
        // (`alert_condition`), so a persistent deficit — whose embedded
        // value changes every tick — updates ONE reviewable row rather than
        // flooding the queue. The operator reviews that row; when they
        // resolve or dismiss it, the next cycle inserts a fresh one.
        let condition = hkask_regulation::alert_condition(output);
        match self.queue.supersede_pending_by_condition(
            condition,
            output,
            confidence,
            error_context,
        ) {
            Ok(true) => {
                tracing::debug!(
                    target: "reg.alert",
                    "Superseded pending escalation — condition re-fired while pending"
                );
                return;
            }
            Ok(false) => {} // no existing pending — insert below
            Err(e) => {
                // Supersede failed — don't block the insert. Best-effort:
                // a failing dedup query is preferable to losing the alert.
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Supersede check failed — proceeding to insert without dedup"
                );
            }
        }

        // `EscalationQueue::add` requires `template_id` and `bot_id` args that
        // don't map from a `RuntimeAlert` — use auto-generated defaults (the
        // same defaults `EscalationEntry::pending` uses). The structured alert
        // fields are preserved in `error_context` (JSON).
        let template_id = hkask_types::TemplateID::new();
        let bot_id = hkask_types::BotID::new();
        match self.queue.add(
            template_id,
            bot_id,
            output.to_string(),
            confidence,
            0,
            error_context.to_string(),
        ) {
            Ok(id) => {
                tracing::debug!(
                    target: "reg.alert",
                    escalation_id = %id,
                    "Algedonic alert persisted to escalation queue"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Failed to persist algedonic alert to escalation queue"
                );
            }
        }
    }

    fn has_pending_alert(&self, output: &str) -> bool {
        // Condition match, not exact output: the pending escalation's
        // embedded value differs from this cycle's, so exact matching
        // never suppresses a re-sensed condition.
        let condition = hkask_regulation::alert_condition(output);
        match self.queue.has_pending_with_condition(condition) {
            Ok(true) => true,
            Ok(false) => false,
            Err(e) => {
                tracing::debug!(
                    target: "reg.alert",
                    error = %e,
                    "Dedup query failed — assuming no pending alert"
                );
                false
            }
        }
    }

    fn auto_resolve_cleared(&self, output: &str, resolution_note: &str) {
        // Condition match, not exact output: the persisted escalation and
        // the clearing cycle's reconstruction embed different values (they
        // were sensed in different cycles), so exact matching would leave
        // the stale escalation pending forever.
        let condition = hkask_regulation::alert_condition(output);
        match self
            .queue
            .resolve_pending_by_condition(condition, "cybernetics_loop:auto_resolve")
        {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(
                        target: "reg.alert",
                        count = count,
                        note = %resolution_note,
                        "Auto-resolved pending escalation — triggering condition cleared"
                    );
                } else {
                    // No pending escalation with this condition — either it
                    // was already resolved/dismissed by the operator, or the
                    // condition never escalated. Not an error; the condition
                    // is clear.
                    tracing::debug!(
                        target: "reg.alert",
                        "Auto-resolve found no pending escalation with this condition — already cleared or not found"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Auto-resolve query failed — escalation remains pending"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fleet {
        healthy: AtomicUsize,
        total: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl hkask_regulation::ContextServerHealthSource for Fleet {
        async fn healthy_count(&self) -> usize {
            self.healthy.load(Ordering::SeqCst)
        }
        async fn total_count(&self) -> usize {
            self.total.load(Ordering::SeqCst)
        }
    }

    /// expect: "Recovery on a later tick resolves exactly the original condition, not partial or absent data" [P9]
    #[tokio::test]
    async fn later_tick_reconciles_durable_conditions() {
        let queue = Arc::new(
            hkask_storage::EscalationQueue::from_driver(
                hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
            )
            .expect("queue"),
        );
        let sink = Arc::new(BridgeAlertEscalationSink::new(queue.clone()));
        let fleet = Arc::new(Fleet {
            healthy: AtomicUsize::new(2),
            total: AtomicUsize::new(10),
        });
        let build = || {
            let mut regulation = hkask_regulation::CyberneticsLoop::new(Arc::new(
                tokio::sync::RwLock::new(hkask_regulation::RegulationLedger::default()),
            ))
            .with_context_server_health_source(fleet.clone());
            regulation.set_alert_escalation_sink(Some(sink.clone()));
            regulation
        };
        let regulation = build();
        regulation.tick().await;
        let original = queue
            .list_pending()
            .expect("pending")
            .into_iter()
            .find(|entry| entry.error_context.contains("context_server_health"))
            .expect("fleet escalation");
        fleet.healthy.store(3, Ordering::SeqCst);
        regulation.tick().await;
        assert!(
            queue
                .list_pending()
                .expect("pending")
                .iter()
                .any(|entry| entry.id == original.id)
        );
        fleet.total.store(0, Ordering::SeqCst);
        regulation.tick().await;
        assert!(
            queue
                .list_pending()
                .expect("pending")
                .iter()
                .any(|entry| entry.id == original.id)
        );
        // A rebuilt loop must reconcile the persisted condition as well.
        drop(regulation);
        let regulation = build();
        fleet.total.store(10, Ordering::SeqCst);
        fleet.healthy.store(10, Ordering::SeqCst);
        regulation.tick().await;
        assert!(
            !queue
                .list_pending()
                .expect("pending")
                .iter()
                .any(|entry| entry.id == original.id)
        );
        let resolved_at = queue
            .get(&original.id.to_string())
            .expect("entry")
            .expect("retained")
            .resolved_at;
        regulation.tick().await;
        assert_eq!(
            queue
                .get(&original.id.to_string())
                .expect("entry")
                .expect("retained")
                .resolved_at,
            resolved_at
        );
    }
}
