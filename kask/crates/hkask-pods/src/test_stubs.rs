//! Minimal stub implementations of `StorageDriver` and `EmbeddingPort`
//! for test harnesses and early development before `kask_bridge` provides
//! real implementations over sqlez.

use hkask_types::storage::{DbRow, DbValue, StorageDriver};
use hkask_types::{
    EmbeddingError, EmbeddingPort, NotFound, SimilarityResult, StoredEmbedding,
};
use std::sync::Arc;

/// A no-op `StorageDriver` that returns empty results for all queries.
/// Useful for test harnesses where storage isn't needed.
#[derive(Default, Clone)]
pub struct StubStorageDriver;

impl StorageDriver for StubStorageDriver {
    fn execute(&self, _sql: &str, _params: &[DbValue]) -> Result<usize, hkask_types::DbError> {
        Ok(0)
    }
    fn execute_batch(&self, _sql: &str) -> Result<(), hkask_types::DbError> {
        Ok(())
    }
    fn query(&self, _sql: &str, _params: &[DbValue]) -> Result<Vec<DbRow>, hkask_types::DbError> {
        Ok(Vec::new())
    }
    fn query_optional(
        &self,
        _sql: &str,
        _params: &[DbValue],
    ) -> Result<Option<DbRow>, hkask_types::DbError> {
        Ok(None)
    }
    fn commit_tx(&self) -> Result<(), hkask_types::DbError> {
        Ok(())
    }
    fn rollback_tx(&self) -> Result<(), hkask_types::DbError> {
        Ok(())
    }
}

/// A no-op `EmbeddingPort` that returns empty results for all searches.
#[derive(Default, Clone)]
pub struct StubEmbeddingPort;

impl EmbeddingPort for StubEmbeddingPort {
    fn store(
        &self,
        _entity_ref: &str,
        _vector: &[f32],
        _model: &str,
    ) -> Result<String, EmbeddingError> {
        Ok(hkask_types::EmbeddingID::new().to_string())
    }
    fn get(&self, entity_ref: &str) -> Result<StoredEmbedding, EmbeddingError> {
        Err(EmbeddingError::NotFound(NotFound {
            entity_type: "embedding".to_string(),
            id: entity_ref.to_string(),
        }))
    }
    fn search(
        &self,
        _query_embedding: &[f32],
        _limit: usize,
    ) -> Result<Vec<SimilarityResult>, EmbeddingError> {
        Ok(Vec::new())
    }
    fn delete(&self, _entity_ref: &str) -> Result<(), EmbeddingError> {
        Ok(())
    }
    fn count(&self) -> Result<usize, EmbeddingError> {
        Ok(0)
    }
    fn query_by_prefix(&self, _prefix: &str) -> Result<Vec<String>, EmbeddingError> {
        Ok(Vec::new())
    }
    fn get_all_by_prefix(
        &self,
        _prefix: &str,
    ) -> Result<Vec<(String, Vec<f32>)>, EmbeddingError> {
        Ok(Vec::new())
    }
}

/// Convenience: create a stub (StorageDriver, EmbeddingPort) pair for test harnesses.
pub fn stub_storage_pair() -> (
    Arc<dyn StorageDriver>,
    Arc<dyn EmbeddingPort>,
) {
    (
        Arc::new(StubStorageDriver) as Arc<dyn StorageDriver>,
        Arc::new(StubEmbeddingPort) as Arc<dyn EmbeddingPort>,
    )
}
