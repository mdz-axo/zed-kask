//! Memory consolidator — confidence-based cleanup.
//!
//! Deletes h_mems at or below the supplied confidence floor.
//!
//! All h_mems are unified — the ontology blob carries dual-axis anchoring
//! (PKO process + DC state) but there is no type distinction.

use std::sync::Arc;

use crate::memory_store::MemoryStore;
use hkask_types::{ConsolidationOutcome, WebID};

/// Memory consolidator — confidence-based cleanup.
///
/// Deletes h_mems at or below the supplied confidence floor.
pub struct MemoryConsolidator {
    store: Arc<MemoryStore>,
    /// The hmems high-water rowid the last pass saw (hang-audit Pr4) — the
    /// skip signal. `None` until the first pass (cold start always sweeps).
    /// Cross-instance and cross-process sound by construction: the rowid
    /// lives in the DB file, so any writer's inserts move it — including
    /// the curator MCP child's, which an in-process counter would miss.
    last_rowid: std::sync::Mutex<Option<i64>>,
}

impl MemoryConsolidator {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self {
            store,
            last_rowid: std::sync::Mutex::new(None),
        }
    }

    /// Delete h_mems at or below the confidence floor, counting failed
    /// deletions — skipped when nothing can have changed since the last
    /// pass (hang-audit Pr4 over F5: the observed passes deleted 0 rows on
    /// every tick, ~2s a sweep).
    pub fn consolidate(
        &self,
        perspective: &WebID,
        confidence_floor: f64,
    ) -> anyhow::Result<ConsolidationOutcome> {
        // Gate on the hmems high-water rowid: every insert moves it and
        // every update is delete-plus-reinsert, so an unchanged value means
        // no new deletion candidates since the last pass — skip. A
        // gate-read failure runs the pass (fail-open); a LOWER rowid (the
        // last pass deleted the max row) also runs it — the gate can only
        // over-run, never under-run. One documented edge: a fresh insert
        // can reuse the rowid of a deleted max row; a fresh insert is not
        // a deletion candidate at the default floor and the next write
        // re-arms — cleanup semantics tolerate the delay.
        match self.store.max_h_mem_rowid() {
            Ok(current) => {
                let skip = {
                    let mut last = self
                        .last_rowid
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    match *last {
                        Some(last_rowid) if last_rowid == current => true,
                        _ => {
                            *last = Some(current);
                            false
                        }
                    }
                };
                if skip {
                    tracing::debug!(
                        target: "reg.consolidation",
                        confidence_floor,
                        "Consolidation skipped — no h_mem writes since the last pass"
                    );
                    return Ok(ConsolidationOutcome {
                        deleted_count: 0,
                        failed_count: 0,
                        skipped: true,
                    });
                }
            }
            Err(error) => {
                tracing::warn!(
                    target: "reg.consolidation",
                    %error,
                    "High-water rowid read failed — running the pass ungated"
                );
            }
        }
        tracing::info!(
            target: "reg.consolidation",
            perspective = %perspective,
            confidence_floor,
            "Consolidation starting"
        );

        let mut deleted_count = 0usize;
        let mut failed_count = 0usize;

        match self
            .store
            .low_confidence_h_mems(confidence_floor, usize::MAX)
        {
            Ok(candidates) if !candidates.is_empty() => {
                for h_mem in &candidates {
                    match self.store.delete_h_mem(&h_mem.id) {
                        Ok(()) => deleted_count += 1,
                        Err(e) => {
                            failed_count += 1;
                            tracing::warn!(
                                target: "reg.consolidation",
                                error = %e,
                                h_mem_id = ?h_mem.id,
                                "Failed to delete low-confidence h_mem during consolidation cleanup"
                            );
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    target: "reg.consolidation",
                    %error,
                    "low_confidence_h_mems: signal stale, skipping cleanup"
                );
            }
        }

        tracing::info!(
            target: "reg.consolidation",
            deleted = deleted_count,
            failed = failed_count,
            "Consolidation complete"
        );

        Ok(ConsolidationOutcome {
            deleted_count,
            failed_count,
            skipped: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::{EmbeddingStore, HMem, HMemStore, SqliteDriver};
    use hkask_types::Confidence;

    /// Hang-audit Pr4 (F5): the no-op pass is skipped. Nothing written
    /// between passes is skipped — after the documented fail-open re-run
    /// that follows a pass which deleted the max row (the high-water rowid
    /// drops, so the gate re-arms once) — and a fresh below-floor write
    /// re-arms the next pass.
    #[test]
    fn consolidate_skips_when_nothing_landed_since_the_last_pass() {
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver)).unwrap(),
            EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim()).unwrap(),
        ));
        let perspective = WebID::from_persona(b"curator");
        let below_floor = HMem::new(
            "gate:skip:1",
            "finding",
            serde_json::json!("v"),
            perspective,
        )
        .with_confidence(Confidence::new(0.2));
        store.store(below_floor).unwrap();

        let consolidator = MemoryConsolidator::new(Arc::clone(&store));
        let first = consolidator.consolidate(&perspective, 0.5).unwrap();
        assert_eq!(first.deleted_count, 1, "cold start always sweeps");
        assert!(!first.skipped);

        // The first pass deleted the max row, so the high-water rowid
        // DROPPED — the gate fails open and re-runs once (documented:
        // over-run, never under-run). It finds nothing and re-arms.
        let second = consolidator.consolidate(&perspective, 0.5).unwrap();
        assert!(
            !second.skipped,
            "a lowered rowid re-runs the pass (fail-open)"
        );
        assert_eq!(second.deleted_count, 0);

        // Nothing has landed since: now the pass is skipped.
        let third = consolidator.consolidate(&perspective, 0.5).unwrap();
        assert!(
            third.skipped,
            "no writes between passes — the pass is skipped"
        );
        assert_eq!(third.deleted_count, 0);

        let below_floor = HMem::new(
            "gate:skip:2",
            "finding",
            serde_json::json!("v"),
            perspective,
        )
        .with_confidence(Confidence::new(0.2));
        store.store(below_floor).unwrap();
        let fourth = consolidator.consolidate(&perspective, 0.5).unwrap();
        assert!(!fourth.skipped, "a fresh write re-arms the pass");
        assert_eq!(fourth.deleted_count, 1);
    }

    /// The gate is cross-instance (and so cross-process) sound: the signal
    /// lives in the DB file, so a write landing through ANOTHER store over
    /// the same driver — the curator MCP child's shape — re-arms this
    /// consolidator's pass. The in-process-counter alternative was blind
    /// to exactly this.
    #[test]
    fn the_gate_sees_writes_through_another_store_over_the_same_db() {
        let driver = SqliteDriver::in_memory_driver();
        let store_a = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver)).unwrap(),
            EmbeddingStore::from_driver(Arc::clone(&driver), hkask_storage::embedding_dim())
                .unwrap(),
        ));
        let store_b = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver)).unwrap(),
            EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim()).unwrap(),
        ));
        let perspective = WebID::from_persona(b"curator");
        let consolidator = MemoryConsolidator::new(store_b);

        // Cold sweep through B (empty store): runs, arms the gate.
        let first = consolidator.consolidate(&perspective, 0.5).unwrap();
        assert!(!first.skipped);

        // A below-floor write lands through STORE A — another instance over
        // the same DB.
        let row = HMem::new(
            "gate:cross:1",
            "finding",
            serde_json::json!("v"),
            perspective,
        )
        .with_confidence(Confidence::new(0.2));
        store_a.store(row).unwrap();

        // B's next pass must NOT skip — the rowid moved under it, and B's
        // sweep deletes A's row.
        let second = consolidator.consolidate(&perspective, 0.5).unwrap();
        assert!(!second.skipped, "another store's write re-arms the gate");
        assert_eq!(second.deleted_count, 1);
    }

    #[test]
    fn confidence_floor_is_inclusive_and_preserves_higher_confidence_rows() -> anyhow::Result<()> {
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver))?,
            EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim())?,
        ));
        let perspective = WebID::from_persona(b"curator");
        let mut deleted_ids = Vec::new();
        let mut survivor_ids = Vec::new();
        for (index, confidence) in [0.0, 0.25, 0.5, 0.500001, 0.9, 1.0].into_iter().enumerate() {
            let row = HMem::new(
                &format!("consolidation:boundary:{index}"),
                "finding",
                serde_json::json!("boundary finding"),
                perspective,
            )
            .with_confidence(Confidence::new(confidence));
            if confidence <= 0.5 {
                deleted_ids.push(row.id);
            } else {
                survivor_ids.push(row.id);
            }
            store.store(row)?;
        }
        let consolidator = MemoryConsolidator::new(Arc::clone(&store));
        let outcome = consolidator.consolidate(&perspective, 0.5)?;
        assert_eq!(outcome.deleted_count, 3);
        assert_eq!(outcome.failed_count, 0);
        for id in deleted_ids {
            assert!(store.get_by_id(&id)?.is_none());
        }
        for id in survivor_ids {
            assert!(store.get_by_id(&id)?.is_some());
        }
        assert_eq!(store.h_mem_count()?, 3);

        let repeated = consolidator.consolidate(&perspective, 0.5)?;
        assert_eq!(repeated.deleted_count, 0);
        assert_eq!(repeated.failed_count, 0);
        assert_eq!(store.h_mem_count()?, 3);
        Ok(())
    }

    #[test]
    fn confidence_cleanup_removes_orphaned_references_and_preserves_live_ones() -> anyhow::Result<()>
    {
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver))?,
            EmbeddingStore::from_driver(driver, hkask_storage::embedding_dim())?,
        ));
        let perspective = WebID::from_persona(b"curator");
        let vector = vec![0.1; hkask_storage::embedding_dim()];
        let mut deleted_ids = Vec::new();
        let mut survivor_ids = Vec::new();
        for (entity, passage, confidence) in [
            ("consolidation:empty", "empty entity finding", 0.25),
            ("consolidation:live", "low-confidence sibling", 0.5),
            ("consolidation:live", "surviving sibling", 0.9),
            ("consolidation:peer", "peer finding", 1.0),
        ] {
            let row = HMem::new(entity, passage, serde_json::json!(passage), perspective)
                .with_confidence(Confidence::new(confidence));
            if confidence <= 0.5 {
                deleted_ids.push(row.id);
            } else {
                survivor_ids.push(row.id);
            }
            store
                .prepare_semantic_write(row)?
                .publish(&vector, "test-model")?;
            assert!(store.has_embedding_for_passage(entity, passage)?);
        }
        store.record_co_occurrence(&[
            "consolidation:empty".to_string(),
            "consolidation:live".to_string(),
            "consolidation:peer".to_string(),
        ])?;
        assert_eq!(store.embedding_count()?, 4);
        assert_eq!(store.connectedness("consolidation:empty")?, 2);
        assert_eq!(store.connectedness("consolidation:live")?, 2);
        assert_eq!(store.connectedness("consolidation:peer")?, 2);

        let outcome = MemoryConsolidator::new(Arc::clone(&store)).consolidate(&perspective, 0.5)?;
        assert_eq!(outcome.deleted_count, 2);
        assert_eq!(outcome.failed_count, 0);
        for id in deleted_ids {
            assert!(store.get_by_id(&id)?.is_none());
        }
        for id in survivor_ids {
            assert!(store.get_by_id(&id)?.is_some());
        }
        assert_eq!(store.h_mem_count()?, 2);
        assert_eq!(store.embedding_count()?, 2);
        assert!(!store.has_embedding_for_passage("consolidation:empty", "empty entity finding")?);
        assert!(!store.has_embedding_for_passage("consolidation:live", "low-confidence sibling")?);
        assert!(store.has_embedding_for_passage("consolidation:live", "surviving sibling")?);
        assert!(store.has_embedding_for_passage("consolidation:peer", "peer finding")?);
        assert_eq!(store.connectedness("consolidation:empty")?, 0);
        assert_eq!(store.connectedness("consolidation:live")?, 1);
        assert_eq!(store.connectedness("consolidation:peer")?, 1);
        Ok(())
    }

    #[test]
    fn deletion_failure_is_counted_and_does_not_stop_confidence_cleanup() -> anyhow::Result<()> {
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(MemoryStore::new(
            HMemStore::from_driver(Arc::clone(&driver))?,
            EmbeddingStore::from_driver(Arc::clone(&driver), hkask_storage::embedding_dim())?,
        ));
        let perspective = WebID::from_persona(b"curator");
        let mut deleted_ids = Vec::new();
        let mut retained_ids = Vec::new();
        for (entity, confidence) in [
            ("consolidation:before", 0.1),
            ("consolidation:blocked", 0.2),
            ("consolidation:after", 0.3),
            ("consolidation:survivor", 0.9),
        ] {
            let row = HMem::new(entity, "finding", serde_json::json!(entity), perspective)
                .with_confidence(Confidence::new(confidence));
            if entity == "consolidation:blocked" || confidence > 0.5 {
                retained_ids.push(row.id);
            } else {
                deleted_ids.push(row.id);
            }
            store.store(row)?;
        }
        driver
            .sqlite_pool()
            .expect("SQLite fixture pool")
            .get()?
            .execute_batch(
                "CREATE TRIGGER fail_consolidation_delete BEFORE DELETE ON hmems
             WHEN OLD.entity = 'consolidation:blocked'
             BEGIN SELECT RAISE(FAIL, 'forced consolidation deletion failure'); END;",
            )?;

        let outcome = MemoryConsolidator::new(Arc::clone(&store)).consolidate(&perspective, 0.5)?;
        assert_eq!(outcome.deleted_count, 2);
        assert_eq!(outcome.failed_count, 1);
        for id in deleted_ids {
            assert!(store.get_by_id(&id)?.is_none());
        }
        for id in retained_ids {
            assert!(store.get_by_id(&id)?.is_some());
        }
        assert_eq!(store.h_mem_count()?, 2);
        assert_eq!(store.low_confidence_count(0.5)?, 1);
        Ok(())
    }
}
