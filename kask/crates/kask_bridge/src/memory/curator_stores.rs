//! Curator sovereign-store infrastructure — extracted from `memory.rs`
//! (deep-module split: the curator's `curator.db` open path, self-healing store
//! handle, and consolidation builder are a one-way dependency of the memory
//! port and independent of the user-store orchestration that remains in
//! `memory.rs`).
//!
//! The shared `open_regulation_archive` helper stays in `memory.rs` because
//! the user-store path (`RealMemoryPort::new`) also calls it; this submodule
//! reaches it via `use super::open_regulation_archive`.

use hkask_memory::{MemoryConsolidator, MemoryStore};
use hkask_storage::{EmbeddingStore, HMemStore, open_or_repair};
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::{InfrastructureError, WebID};
use std::sync::{Arc, RwLock};

use super::open_regulation_archive;

/// Resolve the curator's sovereign `curator.db` path (same resolution as
/// `open_curator_store`): `HKASK_CURATOR_DB` if set, else
/// `agents/curator/curator.db` under the hKask data dir.
pub(crate) fn curator_db_path() -> String {
    std::env::var("HKASK_CURATOR_DB").unwrap_or_else(|_| {
        let p = hkask_types::agent_paths::agent_db("curator");
        let resolved = hkask_types::agent_paths::resolve_under_data_dir(&p);
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        resolved.to_string_lossy().to_string()
    })
}

/// Resolve the curator's PRIVATE memory DB path (operator ruling
/// 2026-11-05 — the separate private curator memory database): sibling
/// file `curator-private.db` in the curator's agent dir, `HKASK_CURATOR_PRIVATE_DB`
/// to override. Same passphrase chain and data-dir resolution as the public
/// store; a separate SQLCipher file so private-turn content never lands in
/// the public DB and a public-store backup/export is clean by construction.
pub(crate) fn curator_private_db_path() -> String {
    std::env::var("HKASK_CURATOR_PRIVATE_DB").unwrap_or_else(|_| {
        let p = hkask_types::agent_paths::agent_dir("curator").join("curator-private.db");
        let resolved = hkask_types::agent_paths::resolve_under_data_dir(&p);
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        resolved.to_string_lossy().to_string()
    })
}
/// Open a `RegulationArchive` (durable regulation-span store) on the curator's
/// sovereign `curator.db` — the same DB the curator MCP server's `reg_query` and
/// `curator_algedonic_log` tools read. Returns `None` on any failure; the
/// caller degrades to `NoopEventSink` with a warn.
///
/// Used by `main.rs` to wire the `McpRuntime` and `CyberneticsLoop` event sinks
/// to durable storage on the curator's curator.db.
pub fn open_curator_regulation_archive(passphrase: &str) -> Option<CuratorRegulationArchive> {
    open_regulation_archive(&curator_db_path(), passphrase, "curator")
}

/// The curator archive handle, shared with recorders wired before it opens.
pub type CuratorRegulationArchive = Arc<hkask_storage::RegulationArchive>;

/// Persist one operator-feedback observation before its recording surface
/// reports success. The archive is authoritative; the RegulationLedger is a
/// bounded working view hydrated from these records after restart.
pub fn persist_operator_feedback(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    persist_skill_span(archive, skill_id, "operator_feedback", payload)
}

/// Persist one skill activation outcome (`reg.skill.<id>.outcome`, payload
/// carries `success`, `invoker` and optional `error`) so the algedonic
/// review's gemba walk can read it through `reg_query`. Skill evaluation
/// happens in that review, not in the session that ran the skill (operator
/// ruling 2026-09-24), so the outcome must outlive the session. Observe stage
/// of `DIAG-ARCH-LEARNING-LOOP-001` (`kask/docs/diagrams/architecture.md`).
pub fn persist_skill_outcome(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    persist_skill_span(archive, skill_id, "outcome", payload)
}

/// Persist one tool failure that occurred while a skill was active
/// (`reg.skill.<id>.tool_failure`): the always-on, mechanical skill-use
/// issue channel the gemba walk reads alongside outcomes.
pub fn persist_skill_tool_failure(
    archive: &hkask_storage::RegulationArchive,
    skill_id: &str,
    payload: serde_json::Value,
) -> Result<(), InfrastructureError> {
    persist_skill_span(archive, skill_id, "tool_failure", payload)
}

/// Journal one `reg.storage.open.<role>` record — the wall-clock cost of a
/// store or archive open (hang-audit Pr5: the per-open telemetry that gives
/// Pr2's launch verification its measured basis). Best-effort by design: a
/// failure warns and never fails the open it is describing.
pub(crate) fn journal_storage_open(
    archive: &hkask_storage::RegulationArchive,
    role: &str,
    duration_ms: u64,
) {
    let Some(namespace) = SpanNamespace::new("reg.storage") else {
        tracing::warn!(
            target: "reg.storage",
            "reg.storage namespace is not registered — open timing not journaled"
        );
        return;
    };
    let event = RegulationRecord::new(
        WebID::from_persona(b"curator"),
        Span::new(namespace, &format!("open.{role}")),
        CyclePhase::Sense,
        serde_json::json!({ "duration_ms": duration_ms }),
        0,
    );
    if let Err(e) = archive.persist(&event) {
        tracing::warn!(
            target: "reg.storage",
            error = %e,
            role,
            "Failed to journal the reg.storage.open record"
        );
    }
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
/// Malformed paths are surfaced and excluded rather than converted into a
/// fabricated skill identity.
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
/// Invalid payloads remain archived for diagnosis but never become fabricated
/// rejection observations in drift sensing.
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

/// The curator's sovereign store, with self-healing open.
///
/// One store holds all of the curator's h_mems — the `HMemOntology` blob
/// on each h_mem carries dual-axis anchoring, so no second store struct
/// is needed.
///
/// Wraps the `Option<Arc<MemoryStore>>` in an `RwLock` plus the parameters
/// needed to re-open it (db path, passphrase, embedding dim, decay constant).
/// When the initial open fails, the store is `None` and every access via
/// `get()` re-attempts the open. A successful re-open restores curator
/// memory mid-session.
pub(crate) struct CuratorStore {
    store: RwLock<Option<Arc<MemoryStore>>>,
    db_path: String,
    passphrase: String,
    embedding_dim: usize,
    memory_life_days: f64,
    heal_attempt_logged: std::sync::atomic::AtomicBool,
    heal_enabled: bool,
}

impl CuratorStore {
    pub(crate) fn new(
        db_path: String,
        passphrase: &str,
        embedding_dim: usize,
        memory_life_days: f64,
    ) -> Self {
        let store = open_curator_store(&db_path, passphrase, embedding_dim, memory_life_days);
        if store.is_none() {
            tracing::error!(
                target: "reg.memory",
                db_path = %db_path,
                "Curator memory store unavailable — the curator runs WITHOUT                  memory. Every curator-turn write will be                  attempted again on ingestion (self-healing); check the DB                  path above, that no other process holds the SQLCipher lock,                  and that the passphrase matches the user's hKask keychain entry."
            );
        }
        Self {
            store: RwLock::new(store),
            db_path,
            passphrase: passphrase.to_string(),
            embedding_dim,
            memory_life_days,
            heal_attempt_logged: std::sync::atomic::AtomicBool::new(false),
            heal_enabled: true,
        }
    }

    /// The store's vector width (the vec0 table's declared width —
    /// `float[N]` before the wave-6 int8 layer, `int8[N]` after). Embedding
    /// requests must request this width from MRL-capable models so
    /// returned vectors always fit the table.
    pub(crate) fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }

    #[cfg(test)]
    pub(crate) fn for_tests(store: Option<Arc<MemoryStore>>) -> Self {
        Self {
            store: RwLock::new(store),
            db_path: String::new(),
            passphrase: String::new(),
            embedding_dim: 1024,
            memory_life_days: hkask_memory::MemoryStore::default_memory_life_days(),
            heal_attempt_logged: std::sync::atomic::AtomicBool::new(false),
            heal_enabled: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_for_tests(&self, store: Option<Arc<MemoryStore>>) {
        if let Ok(mut guard) = self.store.write() {
            *guard = store;
        }
    }

    pub(crate) fn availability(&self) -> bool {
        match self.store.read() {
            Ok(guard) => guard.is_some(),
            Err(_) => false,
        }
    }

    pub(crate) fn get(&self) -> Option<Arc<MemoryStore>> {
        let needs_heal = match self.store.read() {
            Ok(guard) => guard.is_none(),
            Err(_) => true,
        };
        if needs_heal && self.heal_enabled {
            self.try_heal();
        }
        match self.store.read() {
            Ok(guard) => (*guard).clone(),
            Err(_) => None,
        }
    }

    pub(crate) fn try_heal(&self) {
        let fresh = open_curator_store(
            &self.db_path,
            &self.passphrase,
            self.embedding_dim,
            self.memory_life_days,
        );
        let fresh_ok = fresh.is_some();
        let replaced = match self.store.write() {
            Ok(mut guard) => {
                let was_down = guard.is_none();
                if fresh_ok && was_down {
                    *guard = fresh;
                    true
                } else {
                    false
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "reg.memory",
                    error = %e,
                    "Curator store lock poisoned — cannot attempt heal"
                );
                false
            }
        };
        if replaced {
            tracing::info!(
                target: "reg.memory",
                db_path = %curator_db_path(),
                "Curator memory store healed — curator memory restored"
            );
            self.heal_attempt_logged
                .store(false, std::sync::atomic::Ordering::Relaxed);
        } else if !fresh_ok {
            if !self
                .heal_attempt_logged
                .swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                tracing::warn!(
                    target: "reg.memory",
                    db_path = %curator_db_path(),
                    "Curator memory store still unavailable after re-open                      attempt — curator-turn writes are being dropped"
                );
            }
        }
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
fn open_curator_store(
    db_path: &str,
    passphrase: &str,
    embedding_dim: usize,
    memory_life_days: f64,
) -> Option<Arc<MemoryStore>> {
    let opened_at = std::time::Instant::now();
    let open_role = std::path::Path::new(db_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let curator_db_path = db_path.to_string();

    let db = match open_or_repair(&curator_db_path, passphrase) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                db_path = %curator_db_path,
                "Failed to open curator DB — curator copies will be skipped.                  Set HKASK_CURATOR_DB to override the path, or ensure the                  curator agent directory exists under the hKask data dir."
            );
            return None;
        }
    };
    let pool = match db.sqlite_pool() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to get SQLite pool for curator DB"
            );
            return None;
        }
    };
    let driver: Arc<dyn hkask_storage::DatabaseDriver> = Arc::new(
        hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, curator_db_path.as_str()),
    );
    let h_mem_store = match HMemStore::from_driver(Arc::clone(&driver)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to create HMemStore for curator DB"
            );
            return None;
        }
    };
    // zed-kask hang audit Pr1 (open dedup, 2026-11-11): the store's ledger
    // archive reuses the driver this function already opened on the DB
    // instead of paying a second `open_or_repair` — another SQLCipher key
    // derivation, pool build and schema pass on the same file — for every
    // `CuratorStore::new` (the curator and private twins). The clone must
    // happen before the embedding store moves the driver below. `from_driver`
    // runs the same idempotent `init_schema` the dedicated open ran, and
    // failure keeps the prior degraded semantics (no ledger, warn, store
    // continues).
    let ledger = match hkask_storage::RegulationArchive::from_driver(Arc::clone(&driver)) {
        Ok(archive) => Some(Arc::new(archive)),
        Err(e) => {
            tracing::warn!(
                target: "reg.storage",
                error = %e,
                "Failed to init the ledger RegulationArchive on the shared curator driver"
            );
            None
        }
    };
    let embedding_store = match EmbeddingStore::from_driver(driver, embedding_dim) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "reg.memory",
                error = %e,
                "Failed to create EmbeddingStore for curator DB"
            );
            return None;
        }
    };

    // Hang-audit Pr5: journal the open's wall-clock cost through the ledger
    // the store is about to own — measured to the store build, before this
    // record's own write. Best-effort (see journal_storage_open).
    let open_duration_ms = opened_at.elapsed().as_millis() as u64;
    if let Some(archive) = &ledger {
        journal_storage_open(archive, &open_role, open_duration_ms);
    }
    let store = Arc::new({
        // Budgets are gone entirely (operator ruling 2026-09-04, completed
        // 2026-09-08): forgetting is time-based
        // (`kask.memory.forgetting_days`) and distillation-gated, never
        // count-based — no storage budget, no count-based pruning, no
        // usage-ratio sensor. The h_mem count is still monitored against an
        // absolute set-point (`TripleCount`).
        //
        // The decay constant comes from the caller (the operator's
        // `kask.memory.memory_life_days` setting) — recall-time decay must
        // follow the configured S, not the store default.
        let base =
            MemoryStore::new(h_mem_store, embedding_store).with_memory_life_days(memory_life_days);
        // Wire the `reg.memory.encode` span sink on the curator's own DB —
        // mirrors the user-store wiring in `RealMemoryPort::new`. The
        // curator's regulation archive is the same DB the curator MCP
        // server's `reg_query` and `curator_algedonic_log` tools read.
        match ledger {
            Some(archive) => base.with_ledger(archive),
            None => base,
        }
    });
    tracing::info!(
        target: "reg.memory",
        db_path = %curator_db_path,
        duration_ms = open_duration_ms,
        "Curator memory store opened —          curator turns will be ingested into curator memory (perspective = curator)"
    );
    Some(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: "The `kask.memory.memory_life_days` setting governs the bridge
    /// store's actual decay — not just the regulation sensor that reports
    /// it." [P1]
    /// pre: an encrypted curator DB exists at the caller-supplied path; the
    /// decay constant arrives exactly as `RealMemoryPort::new` threads it
    /// from `KaskMemorySettings.memory_life_days`.
    /// post: the opened store carries the configured constant — the store
    /// the sensor reads (`RealMemoryPort::memory_life_days`) IS the store
    /// that decays.
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
    /// own driver — the open dedup must not silently drop the ledger."
    /// [P1, hang audit Pr1]
    /// pre: an encrypted curator DB at a fresh path; `open_curator_store`
    /// builds the h_mem, embedding and ledger stores over ONE driver (the
    /// `RegulationArchive::from_driver` dedup, 2026-11-11) instead of paying
    /// a second `open_or_repair` on the same file.
    /// post: one `store()` write emits a `reg.memory.encode` span that a
    /// SEPARATE archive opened over the same file reads back — the ledger is
    /// wired, and it writes to the shared DB, not a private in-memory one.
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

        // Hang-audit Pr5: the open itself must journal its wall-clock cost.
        let opens = reader.query_records(since, Some("reg.storage.open"), 10)?;
        assert!(
            opens
                .iter()
                .any(|record| record.span.path == "reg.storage.open.curator"),
            "the curator store's open cost must be journaled under reg.storage.open"
        );
        Ok(())
    }

    /// expect: the production self-heal branch — `get()` re-opens the store
    /// when the initial open failed — restores curator memory mid-session.
    /// `for_tests` constructs with `heal_enabled: false` (the outage-probe
    /// pins depend on heal being disabled), so without this test the live
    /// `needs_heal && heal_enabled → try_heal` branch in `get()` has zero
    /// test consumers — the capability-stripped-constructor shape (every
    /// existing test disables the very capability it should pin).
    /// pre: the db_path's initial open fails — a garbage file occupies the
    /// path, so `open_or_repair`'s passphrase probe rejects it and `new()`
    /// holds `None`.
    /// post: while the corruption persists, `get()` stays `None` (the heal
    /// re-open fails the same probe); once the failure clears (the garbage
    /// removed, so the re-open creates a fresh DB), `get()` returns the
    /// healed store carrying the ORIGINAL constructor parameters — the
    /// decay constant reaches the healed store exactly as the initial one.
    #[test]
    fn get_heals_the_store_once_the_open_failure_clears() {
        let directory = tempfile::tempdir().expect("tempdir");
        let db_path = directory.path().join("curator.db");
        let db_path = db_path.to_string_lossy().to_string();
        std::fs::write(&db_path, b"not a database").expect("seed the corrupt db file");

        let store = CuratorStore::new(db_path.clone(), "test-passphrase", 1024, 30.0);
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
    /// outcome recorded in one session is still there after a restart
    /// (operator ruling 2026-09-24).
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
        assert!(persist_skill_outcome(&archive, " ", serde_json::json!({})).is_err());
        drop(archive);

        let restarted =
            hkask_storage::RegulationArchive::from_driver(driver).expect("restarted archive");
        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let records = restarted
            .query_records(since, Some("reg.skill"), 10)
            .expect("query outcomes");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].span.path, "reg.skill.code-review.outcome");
        assert_eq!(records[0].observation, serde_json::json!({"success": true}));
    }

    /// expect: a tool failure captured under an active skill is durable and
    /// readable by the gemba walk's `reg_query` after restart.
    #[test]
    fn skill_tool_failure_survives_archive_restart() {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let archive = hkask_storage::RegulationArchive::from_driver(driver.clone())
            .expect("regulation archive");
        let payload = serde_json::json!({
            "invoker": "Zed Agent",
            "tool": "web_extract",
            "error": "[unavailable] provider down",
        });
        persist_skill_tool_failure(&archive, "code-review", payload.clone())
            .expect("tool failure is durable");
        drop(archive);

        let restarted =
            hkask_storage::RegulationArchive::from_driver(driver).expect("restarted archive");
        let since = chrono::Utc::now() - chrono::Duration::hours(1);
        let records = restarted
            .query_records(since, Some("reg.skill"), 10)
            .expect("query failures");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].span.path, "reg.skill.code-review.tool_failure");
        assert_eq!(records[0].observation, payload);
    }

    #[tokio::test]
    async fn operator_feedback_survives_archive_restart() {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let archive = hkask_storage::RegulationArchive::from_driver(driver.clone())
            .expect("regulation archive");
        let payload = serde_json::json!({"accepted": false, "note": "too broad"});

        persist_operator_feedback(&archive, "task-breakdown", payload.clone())
            .expect("feedback must be durable before acknowledgment");
        drop(archive);

        let restarted =
            hkask_storage::RegulationArchive::from_driver(driver).expect("restarted archive");
        assert_eq!(
            load_operator_feedback(&restarted).expect("load durable feedback"),
            vec![("task-breakdown".to_string(), payload)]
        );

        let ledger = hkask_regulation::RegulationLedger::with_threshold(1);
        assert_eq!(
            hydrate_operator_feedback(&restarted, &ledger)
                .await
                .expect("hydrate durable feedback"),
            1
        );
        assert_eq!(
            ledger.skill_ids_with_feedback("operator_feedback").await,
            vec!["task-breakdown".to_string()]
        );
    }
}
