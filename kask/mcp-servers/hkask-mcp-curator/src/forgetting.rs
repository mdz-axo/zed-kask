//! Distillation-gated episodic forgetting — the automatic leg of the
//! goldfish principle (operator ruling 2026-09-04: therapy-driven
//! per-session passes plus automatic forgetting; budgets are deprecated,
//! so the policy is time-based and distillation-gated, never count-based).
//!
//! Named "forgetting" per the operator's 2026-09-04 naming ruling — one
//! forgets memories; "retirement" was a workplace metaphor that didn't
//! survive review. Distinct from "decay" (memory-system-specification.md
//! §7): decay is the confidence curve R(t) = exp(-t/S); forgetting is
//! deletion — the forgotten rows are removed from the database (operator
//! ruling 2026-09-04: there is no "expired" state; memories age and are
//! forgotten or deleted). Two mechanisms, two names.
//!
//! A thread's shared-copy turns are deleted — and their embeddings
//! deleted — once the thread's newest distillation watermark has aged
//! past the forgetting threshold, and only the turns that watermark
//! COVERS: a turn is covered exactly when its `observed_at` is at or
//! before the watermark's `through` position (the same boundary the
//! distillation pass uses to select pending turns). Turns added after
//! the last distillation pass are not covered — their lessons are not
//! proven extracted — so they survive with their embeddings until a
//! later pass distills them and the advanced watermark ages past the
//! threshold. The watermark proves the lessons were extracted; nothing
//! without that proof is deleted. Because deletion is scoped to rows the
//! pass READ as covered (never a blind prefix delete), a turn racing in
//! during the pass is uncovered by construction and survives.
//!
//! Since the 2026-09-04 single-copy ruling there is no separate
//! curator-perspective original to preserve: a turn's content lives only
//! in its shared-copy chunks, so forgetting the shared copies forgets
//! the covered turns (the lessons stay). The legacy `chat:thread:` rows
//! that predate the ruling were forgotten (deleted) by the 2026-09-04
//! therapy hygiene pass.
//!
//! A thread whose every turn is covered keeps the fast whole-entity path
//! (all its content is proven distilled, including legacy embeddings no
//! passage text can attribute). When a covered chunk's passage text also
//! appears in an uncovered chunk, the ambiguous embedding rows are KEPT —
//! a duplicate surviving recall is a bounded leak; deleting it would
//! erase an uncovered chunk's semantic recall, the exact loss this pass
//! exists to prevent.
//!
//! The pass also sweeps vector rows orphaned from their metadata (KNN's
//! inner join already ignores them; the sweep reclaims the space).
//!
//! Pinned by the tests below: only qualifying threads are forgotten,
//! shared copies only, coverage-scoped deletion, idempotence, and the
//! orphan sweep.

use crate::distillation::{WATERMARK_PREFIX, parse_watermark_through};
use crate::thread_turns::SHARED_TURN_PREFIX;
use hkask_memory::{MemoryStore, MemoryStoreError};
use hkask_storage::HMem;
use std::collections::{HashMap, HashSet};

/// Default forgetting age (days) for a distilled thread's shared-copy
/// turns (read from `HKASK_MEMORY_FORGETTING_DAYS`, injected from
/// `kask.memory.forgetting_days`). 0 disables the pass.
pub(crate) const DEFAULT_FORGETTING_DAYS: u64 = 7;

/// Forgetting age threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ForgettingConfig {
    pub days: u64,
}

impl Default for ForgettingConfig {
    fn default() -> Self {
        Self {
            days: DEFAULT_FORGETTING_DAYS,
        }
    }
}

impl ForgettingConfig {
    /// Read from env. Malformed values warn naming the value and fall
    /// back to the default — never a silent fallback.
    pub(crate) fn from_env() -> Self {
        Self {
            days: match std::env::var("HKASK_MEMORY_FORGETTING_DAYS") {
                Ok(raw) => parse_days_value(&raw),
                Err(_) => DEFAULT_FORGETTING_DAYS,
            },
        }
    }
}

/// Pure parse: malformed values warn naming the value and fall back to
/// the default — never a silent fallback.
fn parse_days_value(raw: &str) -> u64 {
    match raw.trim().parse::<u64>() {
        Ok(value) => value,
        Err(_) => {
            tracing::warn!(
                target: "hkask.mcp.curator.forgetting",
                env = "HKASK_MEMORY_FORGETTING_DAYS",
                value = %raw,
                "Malformed forgetting setting — using default"
            );
            DEFAULT_FORGETTING_DAYS
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ForgettingOutcome {
    pub threads_examined: usize,
    pub threads_forgotten: usize,
    pub turns_deleted: usize,
    pub embeddings_deleted: usize,
    pub orphans_swept: usize,
}

/// One forgetting pass: delete the shared-copy turns (and their embeddings)
/// that the newest distillation watermark of each thread COVERS, for every
/// thread whose watermark has aged past `min_age_days`, then sweep orphaned
/// vector rows.
pub(crate) fn forget_distilled_threads(
    memory: &MemoryStore,
    now: chrono::DateTime<chrono::Utc>,
    min_age_days: u64,
) -> Result<ForgettingOutcome, MemoryStoreError> {
    let cutoff = now - chrono::Duration::days(min_age_days as i64);
    let mut outcome = ForgettingOutcome::default();

    // Newest watermark per thread — a thread is distilled through its
    // newest watermark; older watermarks are prior passes. The h_mem is
    // kept so its `through` position (the coverage boundary) is available.
    let mut newest_by_thread: HashMap<String, HMem> = HashMap::new();
    for watermark in memory.h_mems_by_entity_prefix(WATERMARK_PREFIX)? {
        if watermark.attribute != "distilled_through" {
            continue;
        }
        let Some(thread_id) = watermark.entity.strip_prefix(WATERMARK_PREFIX) else {
            continue;
        };
        match newest_by_thread.get_mut(thread_id) {
            Some(newest) => {
                if watermark.observed_at > newest.observed_at {
                    *newest = watermark;
                }
            }
            None => {
                newest_by_thread.insert(thread_id.to_string(), watermark);
            }
        }
    }
    outcome.threads_examined = newest_by_thread.len();

    for (thread_id, newest) in newest_by_thread {
        if newest.observed_at >= cutoff {
            continue; // still inside the grace window
        }
        // The coverage boundary: turns at or before `through` are proven
        // distilled. Without a parseable `through` there is no proof — the
        // thread is skipped rather than guessed at.
        let Some(through) = parse_watermark_through(&newest) else {
            tracing::warn!(
                target: "hkask.mcp.curator.forgetting",
                thread_id = %thread_id,
                "Watermark carries no parseable `through` position — no coverage proof, thread skipped"
            );
            continue;
        };
        let shared_entity = format!("{SHARED_TURN_PREFIX}{thread_id}");
        let turns = memory.h_mems_by_entity_prefix(&shared_entity)?;
        let (covered, uncovered): (Vec<&HMem>, Vec<&HMem>) =
            turns.iter().partition(|turn| turn.observed_at <= through);

        let (deleted_rows, deleted_embeddings) = if uncovered.is_empty() {
            // Everything the thread holds is proven distilled (including
            // legacy embeddings no passage text can attribute) — the whole
            // entity can go. Embeddings are deleted first so the count is
            // captured here; the prefix delete is self-cleaning (it sweeps
            // emptied entities' references itself) and would find nothing
            // left to remove.
            let deleted_embeddings = memory.delete_embeddings_by_entity(&shared_entity)?;
            let deleted_rows = memory.delete_h_mems_by_entity_prefix(&shared_entity)?;
            (deleted_rows, deleted_embeddings)
        } else {
            // Delete only the rows READ as covered — per id, never a blind
            // prefix delete. A turn racing in (observed_at > through) is
            // uncovered by construction and untouched.
            let mut deleted_rows = 0;
            for turn in &covered {
                memory.delete_h_mem(&turn.id)?;
                deleted_rows += 1;
            }
            // Embeddings carry no timestamp; their link to a chunk is the
            // shared passage text (ingest writes both from one string).
            // A passage that also appears in an uncovered chunk cannot be
            // attributed — keep those rows (conservative: a bounded leak
            // beats erasing an uncovered chunk's semantic recall).
            let uncovered_passages: HashSet<&str> = uncovered
                .iter()
                .filter_map(|turn| turn.value.as_str())
                .collect();
            let covered_passages: Vec<String> = covered
                .iter()
                .filter_map(|turn| turn.value.as_str())
                .filter(|passage| !uncovered_passages.contains(*passage))
                .map(str::to_string)
                .collect();
            (
                deleted_rows,
                memory.delete_embeddings_by_entity_passages(&shared_entity, &covered_passages)?,
            )
        };
        // Count the thread only when work was done — a qualifying thread
        // whose covered turns are already deleted (a prior pass) is a no-op,
        // not a second forgetting. Pinned by forgetting_is_idempotent.
        if deleted_rows > 0 || deleted_embeddings > 0 {
            outcome.turns_deleted += deleted_rows;
            outcome.embeddings_deleted += deleted_embeddings;
            outcome.threads_forgotten += 1;
        }
    }

    outcome.orphans_swept = memory.delete_orphaned_embeddings()?;
    Ok(outcome)
}

/// One forgetting pass over the curator's own DB. Store failures warn
/// and skip — the timer must survive every outcome.
pub(crate) fn run_forgetting_pass(
    db: &crate::CuratorDb,
    now: chrono::DateTime<chrono::Utc>,
    forgetting_days: u64,
) -> ForgettingOutcome {
    let stores = db.get();
    let Some(memory) = stores.memory.as_ref() else {
        tracing::warn!(
            target: "hkask.mcp.curator.forgetting",
            "Curator memory store unavailable — forgetting pass skipped (store self-heals on next open)"
        );
        return ForgettingOutcome::default();
    };
    match forget_distilled_threads(memory, now, forgetting_days) {
        Ok(outcome) => {
            hkask_types::regulation::RegulationSpan::Curation.emit("memory_forgotten");
            outcome
        }
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.curator.forgetting",
                %error,
                "Memory forgetting pass failed — skipped this cycle"
            );
            ForgettingOutcome::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_storage::{EmbeddingStore, HMem, HMemStore};
    use hkask_types::WebID;
    use std::sync::Arc;

    fn store_with_driver() -> (
        MemoryStore,
        Arc<dyn hkask_storage::database::driver::DatabaseDriver>,
    ) {
        let driver = SqliteDriver::in_memory_driver();
        let h_mem_store = HMemStore::from_driver(driver.clone()).expect("hmem store");
        let embedding_store =
            EmbeddingStore::from_driver(driver.clone(), hkask_storage::embedding_dim())
                .expect("embedding store");
        (MemoryStore::new(h_mem_store, embedding_store), driver)
    }

    fn seed_turn(
        memory: &MemoryStore,
        entity: &str,
        value: &str,
        observed: chrono::DateTime<chrono::Utc>,
    ) {
        let h_mem = HMem::new(
            entity,
            "turn",
            serde_json::Value::String(value.to_string()),
            WebID::new(),
        );
        let mut h_mem = h_mem;
        h_mem.observed_at = observed;
        memory.store(h_mem).expect("seed turn");
    }

    fn seed_watermark(
        memory: &MemoryStore,
        thread_id: &str,
        observed: chrono::DateTime<chrono::Utc>,
    ) {
        let h_mem = HMem::new(
            &format!("{WATERMARK_PREFIX}{thread_id}"),
            "distilled_through",
            serde_json::json!({"through": observed.to_rfc3339(), "turns": 1}),
            WebID::new(),
        );
        let mut h_mem = h_mem;
        h_mem.observed_at = observed;
        memory.store(h_mem).expect("seed watermark");
    }

    fn seed_embedding(memory: &MemoryStore, entity: &str, passage: &str, vector_first: f32) {
        let mut vector = vec![0.0f32; hkask_storage::embedding_dim()];
        vector[0] = vector_first;
        vector[1] = 1.0 - vector_first;
        memory
            .store_embedding(entity, &vector, "test-model", Some(passage))
            .expect("seed embedding");
    }

    /// Only threads whose NEWEST watermark has aged past the threshold
    /// are forgotten — and only their shared-copy turns. Recent
    /// threads, never-distilled threads, curator-perspective originals,
    /// and the watermarks themselves are untouched.
    #[test]
    fn forgetting_deletes_only_aged_distilled_shared_turns() {
        let (memory, driver) = store_with_driver();
        let now = chrono::Utc::now();
        let old = now - chrono::Duration::days(10);

        // Old-watermark thread: two shared turns (covered by the watermark —
        // written before its `through` position) + one private original.
        let covered_turn = old - chrono::Duration::days(1);
        seed_turn(
            &memory,
            "curator:thread:old-thread",
            "old turn 1",
            covered_turn,
        );
        seed_turn(
            &memory,
            "curator:thread:old-thread",
            "old turn 2",
            covered_turn,
        );
        seed_turn(
            &memory,
            "chat:thread:old-thread",
            "private original",
            covered_turn,
        );
        seed_embedding(&memory, "curator:thread:old-thread", "old turn 1", 1.0);
        seed_watermark(&memory, "old-thread", old);

        // Recent-watermark thread: must survive.
        let recent = now - chrono::Duration::days(1);
        seed_turn(
            &memory,
            "curator:thread:recent-thread",
            "recent turn",
            recent,
        );
        seed_embedding(&memory, "curator:thread:recent-thread", "recent turn", 1.0);
        seed_watermark(&memory, "recent-thread", recent);

        // Never-distilled thread: must survive (no watermark).
        seed_turn(
            &memory,
            "curator:thread:undistilled",
            "undistilled turn",
            recent,
        );
        seed_embedding(
            &memory,
            "curator:thread:undistilled",
            "undistilled turn",
            1.0,
        );

        let outcome = forget_distilled_threads(&memory, now, 7).expect("forgetting pass");
        assert_eq!(
            outcome.threads_examined, 2,
            "two watermarked threads examined"
        );
        assert_eq!(
            outcome.threads_forgotten, 1,
            "only the aged thread is forgotten"
        );
        assert_eq!(
            outcome.turns_deleted, 2,
            "both shared turns of the aged thread are deleted"
        );
        assert_eq!(
            outcome.embeddings_deleted, 1,
            "the aged thread's embedding is deleted"
        );

        assert!(
            memory
                .h_mems_by_entity_prefix("curator:thread:old-thread")
                .expect("query")
                .is_empty(),
            "the aged thread's shared turns are deleted"
        );
        assert_eq!(
            memory
                .h_mems_by_entity_prefix("chat:thread:old-thread")
                .expect("query")
                .len(),
            1,
            "the pass never touches the retired perspective prefix — legacy rows there were forgotten (deleted) by the therapy hygiene pass, not here"
        );
        assert_eq!(
            memory
                .h_mems_by_entity_prefix("curator:thread:recent-thread")
                .expect("query")
                .len(),
            1,
            "the recent thread survives the grace window"
        );
        assert_eq!(
            memory
                .h_mems_by_entity_prefix("curator:thread:undistilled")
                .expect("query")
                .len(),
            1,
            "a never-distilled thread is never forgotten — no watermark, no proof of extraction"
        );
        assert_eq!(
            memory
                .h_mems_by_entity_prefix(WATERMARK_PREFIX)
                .expect("query")
                .len(),
            2,
            "watermarks are never deleted — they are the idempotence markers"
        );
        let stored_rows = driver
            .query_optional("SELECT count(*) FROM hmems", &[])
            .expect("raw row count")
            .expect("count row")
            .get_int(0)
            .expect("integer count");
        assert_eq!(
            stored_rows, 5,
            "the two forgotten turns must be physically absent"
        );
    }

    /// expect: "Forgetting deletes only what the watermark proves was
    /// distilled — turns added after the last pass stay recallable." [P1]
    /// pre: the thread's newest watermark (through = T) has aged past the
    /// threshold, and the thread holds covered turns (observed_at ≤ T) plus
    /// turns added after the last distillation pass (observed_at > T), each
    /// with its own embedding.
    /// post: covered turns and their embeddings are hard-deleted; the newer
    /// turns and their embeddings survive, and semantic recall returns the
    /// surviving chunk but never the forgotten one. The pass deletes only
    /// rows it read as covered — a turn racing in (observed_at > T) is
    /// uncovered by construction and can never be swept by a prefix delete.
    #[test]
    fn forgetting_preserves_turns_newer_than_the_watermark() {
        let (memory, driver) = store_with_driver();
        let now = chrono::Utc::now();
        let through = now - chrono::Duration::days(10);
        seed_turn(
            &memory,
            "curator:thread:mixed",
            "covered turn",
            through - chrono::Duration::days(1),
        );
        seed_turn(
            &memory,
            "curator:thread:mixed",
            "newer turn",
            now - chrono::Duration::days(1),
        );
        seed_embedding(&memory, "curator:thread:mixed", "covered turn", 1.0);
        seed_embedding(&memory, "curator:thread:mixed", "newer turn", 0.0);
        seed_watermark(&memory, "mixed", through);

        let outcome = forget_distilled_threads(&memory, now, 7).expect("forgetting pass");
        assert_eq!(
            outcome.threads_forgotten, 1,
            "the covered half is work done"
        );
        assert_eq!(outcome.turns_deleted, 1, "only the covered turn is deleted");
        assert_eq!(
            outcome.embeddings_deleted, 1,
            "only the covered embedding is deleted"
        );

        let surviving: Vec<String> = memory
            .h_mems_by_entity_prefix("curator:thread:mixed")
            .expect("query")
            .into_iter()
            .filter_map(|turn| turn.value.as_str().map(str::to_string))
            .collect();
        assert_eq!(
            surviving,
            vec!["newer turn".to_string()],
            "the turn added after the last distillation pass must survive"
        );

        // Semantic recall through the real KNN seam: the surviving chunk is
        // reachable, the forgotten chunk is not.
        let mut probe = vec![0.0f32; hkask_storage::embedding_dim()];
        probe[1] = 1.0; // the newer turn's direction
        let passages: Vec<Option<String>> = memory
            .search_similar(&probe, 10)
            .expect("knn")
            .into_iter()
            .map(|hit| hit.embedding.passage_text)
            .collect();
        assert!(
            passages.contains(&Some("newer turn".to_string())),
            "the newer turn's embedding must stay recallable: {passages:?}"
        );
        assert!(
            !passages.contains(&Some("covered turn".to_string())),
            "the forgotten chunk's embedding must leave semantic recall: {passages:?}"
        );

        let embedding_rows: i64 = driver
            .query_optional(
                "SELECT count(*) FROM embeddings WHERE entity_ref = ?",
                &[hkask_storage::database::value::DbValue::Text(
                    "curator:thread:mixed".to_string(),
                )],
            )
            .expect("embedding count")
            .expect("count row")
            .get_int(0)
            .expect("integer count");
        assert_eq!(
            embedding_rows, 1,
            "exactly the newer turn's embedding remains"
        );
    }

    /// expect: "An embedding whose passage cannot be attributed to one side of
    /// the coverage boundary is kept, never guessed away." [P1]
    /// pre: a covered chunk and an uncovered chunk share the same passage text
    /// (duplicate content straddling the watermark).
    /// post: both h_mems behave precisely (the covered one deleted by id, the
    /// uncovered one kept), but the ambiguous embedding rows are KEPT — the
    /// conservative direction: a duplicate embedding surviving recall is a
    /// bounded leak until the thread fully forgets; deleting it would erase
    /// an uncovered chunk's semantic recall, the exact loss this pass must
    /// never commit.
    #[test]
    fn forgetting_keeps_ambiguous_passage_embeddings() {
        let (memory, driver) = store_with_driver();
        let now = chrono::Utc::now();
        let through = now - chrono::Duration::days(10);
        seed_turn(
            &memory,
            "curator:thread:dup",
            "duplicate text",
            through - chrono::Duration::days(1),
        );
        seed_turn(
            &memory,
            "curator:thread:dup",
            "duplicate text",
            now - chrono::Duration::days(1),
        );
        seed_turn(
            &memory,
            "curator:thread:dup",
            "distinct covered",
            through - chrono::Duration::days(1),
        );
        seed_embedding(&memory, "curator:thread:dup", "duplicate text", 1.0);
        seed_embedding(&memory, "curator:thread:dup", "duplicate text", 0.0);
        seed_embedding(&memory, "curator:thread:dup", "distinct covered", 1.0);
        seed_watermark(&memory, "dup", through);

        let outcome = forget_distilled_threads(&memory, now, 7).expect("forgetting pass");
        assert_eq!(outcome.turns_deleted, 2, "the covered h_mems go by id");
        assert_eq!(
            outcome.embeddings_deleted, 1,
            "only the unambiguous covered embedding goes"
        );

        let surviving: Vec<String> = memory
            .h_mems_by_entity_prefix("curator:thread:dup")
            .expect("query")
            .into_iter()
            .filter_map(|turn| turn.value.as_str().map(str::to_string))
            .collect();
        assert_eq!(
            surviving,
            vec!["duplicate text".to_string()],
            "the uncovered duplicate h_mem survives"
        );
        let embedding_rows: i64 = driver
            .query_optional(
                "SELECT count(*) FROM embeddings WHERE entity_ref = ?",
                &[hkask_storage::database::value::DbValue::Text(
                    "curator:thread:dup".to_string(),
                )],
            )
            .expect("embedding count")
            .expect("count row")
            .get_int(0)
            .expect("integer count");
        assert_eq!(
            embedding_rows, 2,
            "both ambiguous duplicate embeddings are kept, plus nothing else"
        );
    }

    /// expect: "Without a parseable coverage boundary the pass deletes
    /// nothing." [P1]
    /// pre: the thread's newest watermark has aged past the threshold but
    /// carries no `through` position (malformed value).
    /// post: the thread is examined but skipped — no proof of coverage, no
    /// deletion. Failing closed beats deleting unproven content.
    #[test]
    fn forgetting_skips_threads_without_a_parseable_watermark() {
        let (memory, _driver) = store_with_driver();
        let now = chrono::Utc::now();
        let through = now - chrono::Duration::days(10);
        seed_turn(
            &memory,
            "curator:thread:malformed",
            "some turn",
            through - chrono::Duration::days(1),
        );
        let h_mem = HMem::new(
            &format!("{WATERMARK_PREFIX}malformed"),
            "distilled_through",
            serde_json::json!({"not_through": true}),
            WebID::new(),
        );
        let mut h_mem = h_mem;
        h_mem.observed_at = through;
        memory.store(h_mem).expect("seed malformed watermark");

        let outcome = forget_distilled_threads(&memory, now, 7).expect("forgetting pass");
        assert_eq!(outcome.threads_examined, 1);
        assert_eq!(
            outcome.threads_forgotten, 0,
            "no coverage proof — nothing may be deleted"
        );
        assert_eq!(
            memory
                .h_mems_by_entity_prefix("curator:thread:malformed")
                .expect("query")
                .len(),
            1,
            "the unproven thread is untouched"
        );
    }

    /// The pass is idempotent: deleted turns stay deleted, deleted
    /// embeddings stay deleted, a second pass changes nothing.
    #[test]
    fn forgetting_is_idempotent() {
        let (memory, _driver) = store_with_driver();
        let now = chrono::Utc::now();
        let old = now - chrono::Duration::days(10);
        seed_turn(
            &memory,
            "curator:thread:old-thread",
            "old turn",
            old - chrono::Duration::days(1),
        );
        seed_embedding(&memory, "curator:thread:old-thread", "old turn", 1.0);
        seed_watermark(&memory, "old-thread", old);

        let first = forget_distilled_threads(&memory, now, 7).expect("first pass");
        assert_eq!(first.turns_deleted, 1);
        let second = forget_distilled_threads(&memory, now, 7).expect("second pass");
        assert_eq!(
            second,
            ForgettingOutcome {
                threads_examined: 1,
                threads_forgotten: 0,
                turns_deleted: 0,
                embeddings_deleted: 0,
                orphans_swept: 0,
            },
            "a second pass must be a no-op — deleted turns stay deleted"
        );
    }

    /// The orphan sweep removes vector rows whose metadata row is gone.
    /// KNN's inner join already ignores them; the sweep reclaims space
    /// (and cleans up after therapy SQL passes that delete metadata
    /// without vec access).
    #[test]
    fn forgetting_sweeps_orphaned_vectors() {
        let (memory, driver) = store_with_driver();
        let now = chrono::Utc::now();
        let old = now - chrono::Duration::days(10);
        seed_turn(
            &memory,
            "curator:thread:old-thread",
            "old turn",
            old - chrono::Duration::days(1),
        );
        seed_embedding(&memory, "curator:thread:old-thread", "old turn", 1.0);
        seed_watermark(&memory, "old-thread", old);

        // Orphan the embedding: delete the metadata row directly, leaving
        // the vector row (the shape a metadata-only SQL deletion leaves).
        driver
            .execute(
                "DELETE FROM embeddings WHERE entity_ref = ?",
                &[hkask_storage::database::value::DbValue::Text(
                    "curator:thread:old-thread".to_string(),
                )],
            )
            .expect("orphan the metadata row");

        let outcome = forget_distilled_threads(&memory, now, 7).expect("forgetting pass");
        assert_eq!(outcome.orphans_swept, 1, "the orphaned vector row is swept");
    }

    /// The forgetting age parse: malformed values fall back to the
    /// default (the warn lives in the parse, mirroring the distillation
    /// config's pure-parse test — env mutation is unsafe under the
    /// crate's `forbid(unsafe_code)`).
    #[test]
    fn forgetting_config_parse_falls_back_on_malformed() {
        assert_eq!(parse_days_value("14"), 14);
        assert_eq!(parse_days_value(" 21 "), 21);
        assert_eq!(parse_days_value("soon"), DEFAULT_FORGETTING_DAYS);
        assert_eq!(parse_days_value(""), DEFAULT_FORGETTING_DAYS);
    }
}
