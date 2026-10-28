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
}

impl MemoryConsolidator {
    pub fn new(store: Arc<MemoryStore>) -> Self {
        Self { store }
    }

    /// Delete h_mems at or below the confidence floor, counting failed deletions.
    pub fn consolidate(
        &self,
        perspective: &WebID,
        confidence_floor: f64,
    ) -> anyhow::Result<ConsolidationOutcome> {
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::{EmbeddingStore, HMem, HMemStore, SqliteDriver};
    use hkask_types::Confidence;

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
