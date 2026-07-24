//! SemanticIndex — Curator's merged view of all pods' shared semantic data.
//!
//! Backed by the CuratorPod's own SQLCipher file. Tracks per-source-pod
//! cursors for incremental sync on Regulation events.

use hkask_storage::{HMem, HMemStore};
use hkask_types::id::PodID;
use std::collections::HashMap;

/// Merged index of all pods' shared semantic h_mems.
/// Lives on the CuratorPod, backed by its own SQLCipher.
pub struct SemanticIndex {
    /// HMem store backed by CuratorPod's database
    pub store: HMemStore,
    /// Last-seen h_mem rowid per source pod, for incremental sync
    cursors: HashMap<PodID, u64>,
}

impl SemanticIndex {
    /// Create a new empty index backed by the given HMemStore.
    pub fn new(store: HMemStore) -> Self {
        Self {
            store,
            cursors: HashMap::new(),
        }
    }

    /// Insert a semantic h_mem from a source pod.
    /// The source_pod is stored in the h_mem's `access.perspective` field
    /// (reused as provenance metadata — SemantiIndex h_mems have no episodic perspective).
    /// Returns true on successful insert.
    pub fn insert(
        &mut self,
        h_mem: &HMem,
        source_pod: PodID,
    ) -> Result<bool, hkask_storage::HMemError> {
        // Attach source pod provenance to the h_mem before storing.
        // PodID → WebID via UUID extraction (both are Id<T> wrapping Uuid).
        let mut h_mem = h_mem.clone();
        let webid = hkask_types::WebID::from_uuid(source_pod.as_uuid());
        h_mem.access = h_mem.access.with_perspective(webid);
        self.store.insert(&h_mem)?;
        Ok(true)
    }

    /// Query all h_mems for an entity across all source pods.
    #[must_use = "result must be used"]
    pub fn query_by_entity(&self, entity: &str) -> Result<Vec<HMem>, hkask_storage::HMemError> {
        self.store.query_by_entity(entity)
    }

    /// Get the PodID from a h_mem's source provenance (stored in access.perspective).
    #[must_use]
    pub fn source_pod_of(h_mem: &HMem) -> Option<PodID> {
        h_mem
            .access
            .perspective
            .map(|webid| PodID::from_uuid(webid.as_uuid()))
    }

    /// Query h_mems by entity and attribute.
    #[must_use = "result must be used"]
    pub fn query_by_entity_attribute(
        &self,
        entity: &str,
        attribute: &str,
    ) -> Result<Vec<HMem>, hkask_storage::HMemError> {
        self.store.query_by_entity_attribute(entity, attribute)
    }

    /// Get the cursor (last-seen h_mem rowid) for a source pod.
    /// Returns 0 if this pod has never published.
    #[must_use]
    pub fn cursor_for(&self, pod_id: &PodID) -> u64 {
        self.cursors.get(pod_id).copied().unwrap_or(0)
    }

    /// Advance the cursor for a source pod.
    pub fn advance_cursor(&mut self, pod_id: PodID, rowid: u64) {
        self.cursors.insert(pod_id, rowid);
    }

    /// Number of source pods tracked.
    #[must_use]
    pub fn source_count(&self) -> usize {
        self.cursors.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_types::{Visibility, WebID};

    fn make_store() -> HMemStore {
        let pool = SqliteDriver::in_memory_pool().unwrap();
        let driver = std::sync::Arc::new(SqliteDriver::new(pool));
        HMemStore::from_driver(driver)
    }

    fn make_h_mem(entity: &str, attribute: &str, value: &str) -> HMem {
        let owner = WebID::from_persona(b"test");
        HMem::new(
            entity,
            attribute,
            serde_json::Value::String(value.into()),
            owner,
        )
        .with_visibility(Visibility::Shared)
    }

    #[test]
    fn insert_and_query() {
        let store = make_store();
        let mut index = SemanticIndex::new(store);
        let h_mem = make_h_mem("test", "name", "hello");
        let pod_id = PodID::new();

        assert!(index.insert(&h_mem, pod_id).unwrap());
        let results = index.query_by_entity("test").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].attribute, "name");
    }

    #[test]
    fn cursor_tracking() {
        let store = make_store();
        let mut index = SemanticIndex::new(store);
        let pod_id = PodID::new();

        assert_eq!(index.cursor_for(&pod_id), 0);
        index.advance_cursor(pod_id, 42);
        assert_eq!(index.cursor_for(&pod_id), 42);
        assert_eq!(index.source_count(), 1);
    }
}
