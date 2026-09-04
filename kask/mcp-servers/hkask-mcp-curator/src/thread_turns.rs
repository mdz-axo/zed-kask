//! Thread-turn discovery — the one place that knows which entity prefix
//! holds a thread's turns.
//!
//! Both ALWAYS-mode consumers must see the same turns: the on-demand
//! `curator_memory_extract` tool and the background distillation pass.
//! The prefix comes from the bridge's ingest path
//! (`kask_bridge/src/memory/ingest.rs`):
//!
//! - `curator:thread:{id}` — the shared copy (Shared), written for
//!   **every turn**, curator and non-curator alike. Since the
//!   2026-09-04 single-copy ruling, a turn's content is stored as
//!   cleaned, tagged chunk h_mems under this entity (attribute
//!   `chunk:{index}`); legacy rows under the same entity carry the old
//!   whole-turn `turn` attribute. Discovery is attribute-agnostic —
//!   both shapes are extraction candidates.
//!
//! The shared-copy prefix is therefore the complete set: a scan over it
//! alone sees every turn of every thread. The former `chat:thread:`
//! curator-perspective prefix was retired by the same ruling (its rows
//! were byte-identical duplicates; the legacy rows were expired by the
//! 2026-09-04 therapy hygiene pass).

use std::collections::HashMap;

use hkask_memory::{MemoryStore, MemoryStoreError};
use hkask_storage::HMem;

/// The shared-copy turn prefix — every turn, the complete set.
pub(crate) const SHARED_TURN_PREFIX: &str = "curator:thread:";

/// One thread's turns as extraction presents them: the shared copies
/// (legacy whole-turn rows and chunk rows alike).
pub(crate) fn thread_turns(
    memory: &MemoryStore,
    thread_id: &str,
) -> Result<Vec<HMem>, MemoryStoreError> {
    memory.h_mems_by_entity_prefix(&format!("{SHARED_TURN_PREFIX}{thread_id}"))
}

/// Every thread's shared-copy turns since `since`, grouped by thread id —
/// the distillation scan. Time-bounded so the pass never loads the whole
/// store; complete because ingest writes a shared copy for every turn.
pub(crate) fn shared_turns_by_thread_since(
    memory: &MemoryStore,
    since: chrono::DateTime<chrono::Utc>,
) -> Result<HashMap<String, Vec<HMem>>, MemoryStoreError> {
    let mut by_thread: HashMap<String, Vec<HMem>> = HashMap::new();
    for turn in memory.h_mems_by_prefix_since(SHARED_TURN_PREFIX, since)? {
        let Some(thread_id) = turn.entity.strip_prefix(SHARED_TURN_PREFIX) else {
            continue;
        };
        by_thread
            .entry(thread_id.to_string())
            .or_default()
            .push(turn);
    }
    Ok(by_thread)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    fn store() -> MemoryStore {
        let driver = SqliteDriver::in_memory_driver();
        let h_mem_store =
            hkask_storage::HMemStore::from_driver(Arc::clone(&driver)).expect("h_mem store");
        MemoryStore::try_new_without_embeddings(h_mem_store).expect("memory store")
    }

    /// `thread_turns` reads the shared-copy prefix — the complete set under
    /// the single-copy design. Both row shapes (legacy `turn` attribute and
    /// chunk `chunk:{n}` attribute) are candidates: discovery is
    /// attribute-agnostic.
    #[test]
    fn thread_turns_reads_shared_prefix_both_attribute_shapes() {
        let store = store();
        let webid = hkask_types::WebID::new();
        store
            .store(HMem::new(
                "curator:thread:t1",
                "turn",
                serde_json::json!("legacy whole-turn row"),
                webid,
            ))
            .expect("seed legacy turn");
        store
            .store(HMem::new(
                "curator:thread:t1",
                "chunk:0",
                serde_json::json!("chunk row"),
                webid,
            ))
            .expect("seed chunk");

        let turns = thread_turns(&store, "t1").expect("query turns");
        assert_eq!(
            turns.len(),
            2,
            "legacy and chunk rows are both extraction candidates — got: {turns:?}"
        );
    }

    /// The distillation scan reads the shared-copy prefix — the complete
    /// set, because ingest writes a shared copy for every turn — and
    /// groups by thread id.
    #[test]
    fn shared_scan_groups_shared_copies_by_thread() {
        let store = store();
        let webid = hkask_types::WebID::new();
        store
            .store(HMem::new(
                "curator:thread:t1",
                "chunk:0",
                serde_json::json!("t1 chunk"),
                webid,
            ))
            .expect("seed t1");
        store
            .store(HMem::new(
                "curator:thread:t2",
                "turn",
                serde_json::json!("t2 shared"),
                webid,
            ))
            .expect("seed t2");

        let since = chrono::Utc::now() - chrono::Duration::days(365);
        let by_thread = shared_turns_by_thread_since(&store, since).expect("scan");
        assert_eq!(
            by_thread.len(),
            2,
            "threads with shared copies are scanned — got: {by_thread:?}"
        );
        assert_eq!(by_thread["t1"].len(), 1);
        assert_eq!(by_thread["t2"].len(), 1);
    }
}
