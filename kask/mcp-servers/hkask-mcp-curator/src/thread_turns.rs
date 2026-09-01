//! Thread-turn discovery — the one place that knows which entity prefixes
//! hold a thread's turns.
//!
//! Both ALWAYS-mode consumers must see the same turns: the on-demand
//! `curator_memory_extract` tool and the background distillation pass. The
//! two prefixes come from the bridge's ingest path
//! (`kask_bridge/src/memory/ingest.rs`):
//!
//! - `chat:thread:{id}` — the curator-perspective original (Private),
//!   written for **curator turns only**.
//! - `curator:thread:{id}` — the shared copy (Shared), written for
//!   **every turn**, curator and non-curator alike.
//!
//! The shared-copy prefix is therefore the complete set: a scan over it
//! alone sees every turn of every thread. The perspective prefix adds the
//! curator originals to extraction candidates. Querying only
//! `chat:thread:` is the one wrong shape — it hides every non-curator
//! turn.

use std::collections::HashMap;

use hkask_memory::{MemoryStore, MemoryStoreError};
use hkask_storage::HMem;

/// The curator-perspective turn prefix (Private originals, curator turns
/// only).
pub(crate) const PERSPECTIVE_TURN_PREFIX: &str = "chat:thread:";

/// The shared-copy turn prefix — every turn, the complete set.
pub(crate) const SHARED_TURN_PREFIX: &str = "curator:thread:";

/// One thread's turns as extraction presents them: the curator-perspective
/// originals plus the shared copies. A curator turn appears twice (its
/// Private original and its Shared copy) — both are valid evidence
/// citations, and the candidate list preserves that shape.
pub(crate) fn thread_turns(
    memory: &MemoryStore,
    thread_id: &str,
) -> Result<Vec<HMem>, MemoryStoreError> {
    let mut turns =
        memory.h_mems_by_entity_prefix(&format!("{PERSPECTIVE_TURN_PREFIX}{thread_id}"))?;
    turns.extend(memory.h_mems_by_entity_prefix(&format!("{SHARED_TURN_PREFIX}{thread_id}"))?);
    Ok(turns)
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

    /// `thread_turns` must surface BOTH storage prefixes: the
    /// curator-perspective originals and the shared copies. Dropping
    /// either prefix silently shrinks extraction's candidate set — the
    /// perspective prefix hides non-curator turns, the shared prefix
    /// hides the curator originals.
    #[test]
    fn thread_turns_reads_both_prefixes() {
        let store = store();
        let webid = hkask_types::WebID::new();
        store
            .store(HMem::new(
                "chat:thread:t1",
                "chatted",
                serde_json::json!("curator original"),
                webid,
            ))
            .expect("seed perspective turn");
        store
            .store(HMem::new(
                "curator:thread:t1",
                "turn",
                serde_json::json!("shared copy"),
                webid,
            ))
            .expect("seed shared turn");

        let turns = thread_turns(&store, "t1").expect("query turns");
        assert_eq!(
            turns.len(),
            2,
            "both prefixes' turns must be returned — got: {turns:?}"
        );
        assert!(turns.iter().any(|h| h.entity == "chat:thread:t1"));
        assert!(turns.iter().any(|h| h.entity == "curator:thread:t1"));
    }

    /// The distillation scan reads the shared-copy prefix — the complete
    /// set, because ingest writes a shared copy for every turn — and
    /// groups by thread id. A turn stored only under the perspective
    /// prefix (which ingest never does for a whole thread, since every
    /// turn also gets a shared copy) is deliberately NOT part of the
    /// scan: the pass distills shared copies, extraction additionally
    /// surfaces originals.
    #[test]
    fn shared_scan_groups_shared_copies_by_thread() {
        let store = store();
        let webid = hkask_types::WebID::new();
        store
            .store(HMem::new(
                "curator:thread:t1",
                "turn",
                serde_json::json!("t1 shared"),
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
        store
            .store(HMem::new(
                "chat:thread:t3",
                "chatted",
                serde_json::json!("perspective only"),
                webid,
            ))
            .expect("seed perspective-only");

        let since = chrono::Utc::now() - chrono::Duration::days(365);
        let by_thread = shared_turns_by_thread_since(&store, since).expect("scan");
        assert_eq!(
            by_thread.len(),
            2,
            "only threads with shared copies are scanned — got: {by_thread:?}"
        );
        assert_eq!(by_thread["t1"].len(), 1);
        assert_eq!(by_thread["t2"].len(), 1);
        assert!(
            !by_thread.contains_key("t3"),
            "perspective-only originals are extraction's, not the scan's"
        );
    }
}
