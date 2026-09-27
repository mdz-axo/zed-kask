//! Legacy escalation-queue opener. Alerts now land on the Algedonic review
//! board (`crate::algedonic_board`); this queue only backs the status tool's
//! awaiting-review count until Phase D step 3 moves that reader to the board.

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
