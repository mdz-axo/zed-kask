//! Curator sovereign-store adapter for `kask_bridge` — delegates SQLCipher
//! store opening, path resolution, and throttled self-healing to `hkask_memory`.

use std::sync::Arc;

use hkask_memory::{
    CuratorOpenConfig, CuratorOpenFailure, MemoryConsolidator, MemoryStore, SelfHealingStore,
    open_curator_store_set,
};
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::{InfrastructureError, WebID};

use super::open_regulation_archive;

pub(crate) use hkask_memory::{
    journal_storage_open, resolve_curator_db_path as curator_db_path,
    resolve_curator_private_db_path as curator_private_db_path,
};

/// Open a `RegulationArchive` (durable regulation-span store) on the curator's
/// sovereign `curator.db` — the same DB the curator MCP server's `reg_query` and
/// `curator_algedonic_log` tools read. Returns `None` on any failure; the
/// caller degrades to `NoopEventSink` with a warn.
pub fn open_curator_regulation_archive(passphrase: &str) -> Option<CuratorRegulationArchive> {
    open_regulation_archive(&curator_db_path(), passphrase, "curator")
}

/// The curator archive handle, shared with recorders wired before it opens.
pub type CuratorRegulationArchive = Arc<hkask_storage::RegulationArchive>;

/// Persist one operator-feedback observation before its recording surface
/// reports success.
pub fn persist_operator_feedback(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    persist_skill_span(archive, skill_id, "operator_feedback", payload)
}

/// Persist one skill activation outcome (`reg.skill.<id>.outcome`).
pub fn persist_skill_outcome(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    persist_skill_span(archive, skill_id, "outcome", payload)
}

/// Persist one tool failure that occurred while a skill was active
/// (`reg.skill.<id>.tool_failure`).
pub fn persist_skill_tool_failure(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    persist_skill_span(archive, skill_id, "tool_failure", payload)
}

fn persist_skill_span(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    phase: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    if skill_id.trim().is_empty() {
        return Err(InfrastructureError::Serialization(format!(
            "skill {phase} skill_id must not be empty"
        )));
    }
    let namespace = SpanNamespace::new("reg.skill").ok_or_else(|| {
        InfrastructureError::Serialization("reg.skill namespace is not registered".to_string())
    })?;
    let event = RegulationRecord::new(
        WebID::from_persona(b"curator"),
        Span::new(namespace, &format!("{skill_id}.{phase}")),
        CyclePhase::Sense,
        payload,
        0,
    );
    archive.persist(&event)
}

/// Load the durable operator-feedback history in chronological order.
pub(crate) fn load_operator_feedback(
    archive: &hkask_storage::RegulationArchive,
) -> Result<Vec<(String, serde_json::Value)>, InfrastructureError> {
    let mut feedback = Vec::new();
    for event in archive.query_operator_feedback()? {
        let Some(skill_id) = event
            .span
            .path
            .strip_prefix("reg.skill.")
            .and_then(|path| path.strip_suffix(".operator_feedback"))
            .filter(|skill_id| !skill_id.is_empty())
        else {
            tracing::warn!(
                target: "reg.storage",
                span_path = %event.span.path,
                "Ignoring malformed durable operator-feedback span path"
            );
            continue;
        };
        feedback.push((skill_id.to_string(), event.observation));
    }
    Ok(feedback)
}

/// Rebuild the bounded RegulationLedger working view from durable feedback.
pub async fn hydrate_operator_feedback(
    archive: &hkask_storage::RegulationArchive,
    ledger: &hkask_regulation::RegulationLedger,
) -> Result<usize, InfrastructureError> {
    let mut restored = 0usize;
    for (skill_id, payload) in load_operator_feedback(archive)? {
        if hkask_regulation::OperatorFeedbackObservation::from_payload(&payload).is_none() {
            tracing::warn!(
                target: "reg.storage",
                skill_id = %skill_id,
                "Ignoring malformed durable operator-feedback payload"
            );
            continue;
        }
        ledger
            .record_skill_span(&skill_id, "operator_feedback", payload)
            .await;
        restored += 1;
    }
    Ok(restored)
}

/// Bridge-side handle over a sovereign Curator store (`curator.db` or
/// `curator-private.db`), backed by `hkask_memory::SelfHealingStore`.
pub(crate) struct CuratorStore {
    healing: SelfHealingStore<Option<Arc<MemoryStore>>, String>,
    embedding_dim: usize,
}

impl CuratorStore {
    pub(crate) fn new(
        db_path: String,
        passphrase: &str,
        embedding_dim: usize,
        memory_life_days: f64,
    ) -> Self {
        let (initial, outage) =
            open_curator_store_with_reason(&db_path, passphrase, embedding_dim, memory_life_days);
        if initial.is_none() {
            tracing::error!(
                target: "reg.memory",
                db_path = %db_path,
                "Curator memory store unavailable — the curator runs WITHOUT \
                 memory. Re-open will be retried with backoff on access; check \
                 the DB path above, SQLCipher locks, and HKASK_DB_PASSPHRASE."
            );
        }
        let reopen_path = db_path.clone();
        let reopen_passphrase = passphrase.to_string();
        let healing =
            SelfHealingStore::new(db_path, initial, outage, true, Option::is_none, move || {
                open_curator_store_with_reason(
                    &reopen_path,
                    &reopen_passphrase,
                    embedding_dim,
                    memory_life_days,
                )
            });
        Self {
            healing,
            embedding_dim,
        }
    }

    #[cfg(test)]
    fn with_retry_interval(mut self, interval: std::time::Duration) -> Self {
        self.healing = self.healing.with_retry_interval(interval);
        self
    }

    pub(crate) fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }

    #[cfg(test)]
    pub(crate) fn for_tests(store: Option<Arc<MemoryStore>>) -> Self {
        Self {
            healing: SelfHealingStore::disabled(store, Option::is_none),
            embedding_dim: 1024,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_for_tests(&self, store: Option<Arc<MemoryStore>>) {
        self.healing.replace_state(store);
    }

    pub(crate) fn availability(&self) -> bool {
        self.healing.is_available()
    }

    pub(crate) fn get(&self) -> Option<Arc<MemoryStore>> {
        self.healing.get()
    }

    pub(crate) fn outage_summary(&self) -> Option<String> {
        self.healing.outage()
    }
}

pub(crate) fn build_curator_consolidation(
    consolidation_cadence_secs: u64,
    store: &Option<Arc<MemoryStore>>,
) -> Option<Arc<MemoryConsolidator>> {
    if consolidation_cadence_secs == 0 {
        return None;
    }
    let Some(store) = store else {
        return None;
    };
    Some(Arc::new(MemoryConsolidator::new(Arc::clone(store))))
}

fn format_curator_open_failure(failure: CuratorOpenFailure) -> String {
    match failure {
        CuratorOpenFailure::MissingDbPath => "database path not resolved".to_string(),
        CuratorOpenFailure::MissingPassphrase => "HKASK_DB_PASSPHRASE not resolved".to_string(),
        CuratorOpenFailure::DatabaseOpen(err) => err.to_string(),
        CuratorOpenFailure::PartialInit(reason) => reason,
    }
}

fn open_curator_store_with_reason(
    db_path: &str,
    passphrase: &str,
    embedding_dim: usize,
    memory_life_days: f64,
) -> (Option<Arc<MemoryStore>>, Option<String>) {
    let config = CuratorOpenConfig {
        embedding_dim,
        memory_life_days,
        allow_degraded_embeddings: false,
        journal_open_span: true,
    };
    let (stores, failure) = open_curator_store_set(Some(db_path), Some(passphrase), config);
    if stores.memory.is_some() {
        tracing::info!(
            target: "reg.memory",
            db_path = %db_path,
            "Curator memory store opened"
        );
    }
    (stores.memory, failure.map(format_curator_open_failure))
}

#[cfg(test)]
fn open_curator_store(
    db_path: &str,
    passphrase: &str,
    embedding_dim: usize,
    memory_life_days: f64,
) -> Option<Arc<MemoryStore>> {
    open_curator_store_with_reason(db_path, passphrase, embedding_dim, memory_life_days).0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "The `kask.memory.memory_life_days` setting governs the bridge
    /// store's actual decay — not just the regulation sensor that reports it." [P1]
    #[test]
    fn open_curator_store_applies_configured_memory_life_days() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db_path = directory.path().join("curator.db");
        let db_path = db_path.to_string_lossy().to_string();
        hkask_storage::open_or_repair(&db_path, "test-passphrase").expect("create db");

        let store = open_curator_store(&db_path, "test-passphrase", 1024, 30.0).expect("store");
        assert_eq!(
            store.memory_life_days(),
            30.0,
            "the configured decay constant must reach the store that actually decays"
        );
    }

    /// expect: "The store wired by `open_curator_store` journals
    /// `reg.memory.encode` spans through a ledger archive sharing the store's
    /// own driver — the open dedup must not silently drop the ledger." [P1, Pr1]
    #[test]
    fn store_ledger_shares_the_store_driver() -> anyhow::Result<()> {
        let directory = tempfile::tempdir().expect("tempdir");
        let db_path = directory.path().join("curator.db");
        let db_path = db_path.to_string_lossy().to_string();
        hkask_storage::open_or_repair(&db_path, "test-passphrase").expect("create db");

        let store = open_curator_store(&db_path, "test-passphrase", 1024, 30.0).expect("store");
        let row = hkask_storage::HMem::new(
            "hang-audit:open-dedup",
            "finding",
            serde_json::json!("ledger pin"),
            hkask_types::WebID::from_persona(b"curator"),
        );
        store.store(row)?;

        let reader = open_regulation_archive(&db_path, "test-passphrase", "pin")
            .expect("a separate archive over the same DB");
        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let records = reader.query_records(since, Some("reg.memory.encode"), 10)?;
        assert!(
            !records.is_empty(),
            "the store's shared-driver ledger must journal reg.memory.encode spans"
        );

        let opens = reader.query_records(since, Some("reg.storage.open"), 10)?;
        assert!(
            opens
                .iter()
                .any(|record| record.span.path == "reg.storage.open.curator"),
            "the curator store's open cost must be journaled under reg.storage.open"
        );
        Ok(())
    }

    /// expect: `get()` re-opens the store once an initial open failure clears,
    /// while throttling immediate back-to-back retries under the default 5s backoff.
    #[test]
    fn get_heals_the_store_once_the_open_failure_clears() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db_path = directory.path().join("curator.db");
        let db_path = db_path.to_string_lossy().to_string();
        std::fs::write(&db_path, b"not a database").expect("seed the corrupt db file");

        let store = CuratorStore::new(db_path.clone(), "test-passphrase", 1024, 30.0)
            .with_retry_interval(std::time::Duration::ZERO);
        assert!(
            !store.availability(),
            "the corrupt file must fail the initial open"
        );
        assert!(
            store.get().is_none(),
            "the heal re-open must fail while the corrupt file occupies the path"
        );

        std::fs::remove_file(&db_path).expect("clear the corruption");
        let healed = store
            .get()
            .expect("get() must heal once the re-open can succeed");
        assert_eq!(
            healed.memory_life_days(),
            30.0,
            "the healed store carries the constructor's decay constant"
        );
        assert!(
            store.availability(),
            "availability() reflects the healed store"
        );
    }

    /// The gemba walk reads skill activation outcomes from the archive, so an
    /// outcome recorded in one session is still there after a restart.
    #[test]
    fn skill_outcome_survives_archive_restart() {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let archive = hkask_storage::RegulationArchive::from_driver(driver.clone())
            .expect("regulation archive");
        persist_skill_outcome(
            &archive,
            "code-review",
            serde_json::json!({"success": true}),
        )
        .expect("outcome is durable");
        let restarted =
            hkask_storage::RegulationArchive::from_driver(driver).expect("reopened archive");
        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let events = restarted
            .query_records(since, Some("reg.skill.code-review.outcome"), 10)
            .expect("query skill outcome");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].observation["success"], true);
    }
}
