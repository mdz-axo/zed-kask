//! Alert escalation sink + queue opener — extracted from `memory.rs`
//! (deep-module split: the algedonic alert path implements a *different*
//! port — `hkask_regulation::AlertEscalationSink` — with zero coupling to the
//! memory port). `open_curator_escalation_queue` borrows `curator_db_path`
//! from the parent's re-export of `curator_stores`.

use hkask_storage::Database;
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
    let db = match Database::open(&db_path, passphrase) {
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
    fn persist_alert(&self, output: &str, confidence: f64, error_context: &str) {
        // Dedup at the source: if there is already a pending escalation with
        // the same output string, skip the insert. The regulation loop senses
        // the same deficit every cycle (e.g. an unwired efferent action), and
        // without this check it floods the queue with identical alerts every
        // tick. The operator reviews the first one; duplicates add no signal.
        match self.queue.has_pending_with_output(output) {
            Ok(true) => {
                tracing::debug!(
                    target: "reg.alert",
                    "Skipping duplicate escalation — pending alert with same output already in queue"
                );
                return;
            }
            Ok(false) => {} // no duplicate — proceed to insert
            Err(e) => {
                // Dedup check failed — don't block the insert. Best-effort:
                // a failing dedup query is preferable to losing the alert.
                tracing::warn!(
                    target: "reg.alert",
                    error = %e,
                    "Dedup check failed — proceeding to insert without dedup"
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
        match self.queue.has_pending_with_output(output) {
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
        match self.queue.resolve_pending_by_output(output, "cybernetics_loop:auto_resolve") {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(
                        target: "reg.alert",
                        count = count,
                        note = %resolution_note,
                        "Auto-resolved pending escalation — triggering condition cleared"
                    );
                } else {
                    // No pending escalation with this output — either it was
                    // already resolved/dismissed by the operator, or the output
                    // string doesn't match. Not an error; the condition is clear.
                    tracing::debug!(
                        target: "reg.alert",
                        "Auto-resolve found no pending escalation with this output — already cleared or not found"
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
