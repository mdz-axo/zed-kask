//! Thread-turn discovery — the one place that knows which entity prefix
//! holds a thread's turns.
//!
//! Both ALWAYS-mode consumers must see the same turns: the on-demand
//! `curator_memory_extract` tool and the background distillation pass.
//! The prefix comes from the bridge's ingest path
//! (`kask_bridge/src/memory/ingest.rs`):
//!
//! - `curator:thread:{id}` — the one copy of every turn, stored as
//!   cleaned, tagged chunk h_mems. Discovery is attribute-agnostic.
//!
//! A scan over this prefix alone sees every turn of every thread.

use std::collections::HashMap;

use hkask_memory::{MemoryStore, MemoryStoreError};
use hkask_storage::HMem;

/// The shared-copy turn prefix — every turn, the complete set.
pub(crate) const SHARED_TURN_PREFIX: &str = "curator:thread:";

/// One thread's turns as extraction presents them: the rows stored under
/// exactly `curator:thread:{thread_id}`. An exact read, so a thread id that
/// begins another thread's id (or an empty one) never returns the other
/// thread's turns, and the lookup uses the entity index.
pub(crate) fn thread_turns(
    memory: &MemoryStore,
    thread_id: &str,
) -> Result<Vec<HMem>, MemoryStoreError> {
    memory.h_mems_by_entity(&format!("{SHARED_TURN_PREFIX}{thread_id}"))
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

    /// `thread_turns` returns only the requested thread's turns.
    #[test]
    fn thread_turns_reads_one_thread() {
        let store = store();
        let webid = hkask_types::WebID::new();
        for (entity, text) in [("curator:thread:t1", "one"), ("curator:thread:t2", "two")] {
            store
                .store(HMem::new(
                    entity,
                    "chunk:1:0",
                    serde_json::json!(text),
                    webid,
                ))
                .expect("seed chunk");
        }

        let turns = thread_turns(&store, "t1").expect("query turns");
        assert_eq!(turns.len(), 1, "got: {turns:?}");
    }

    /// mcp-tool-review CU-03 pin: the read is exact. A thread id that is a
    /// prefix of another's, and the empty id, return none of the other
    /// thread's turns (the prefix read returned them all).
    #[test]
    fn thread_turns_never_returns_a_sibling_threads_turns() {
        let store = store();
        let webid = hkask_types::WebID::new();
        for entity in ["curator:thread:t1", "curator:thread:t10"] {
            store
                .store(HMem::new(
                    entity,
                    "chunk:1:0",
                    serde_json::json!("x"),
                    webid,
                ))
                .expect("seed chunk");
        }

        let turns = thread_turns(&store, "t1").expect("query turns");
        assert_eq!(turns.len(), 1, "got: {turns:?}");
        assert_eq!(turns[0].entity, "curator:thread:t1");
        assert!(thread_turns(&store, "t").expect("partial id").is_empty());
        assert!(thread_turns(&store, "").expect("empty id").is_empty());
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
