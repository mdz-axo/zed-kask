//! The kata-kanban's trust-ledger store (Binding 5's substrate).
//!
//! Bridges the per-goal trust ledger (the Goal's `trust_events` Vec,
//! recorded by `kanban_goal_trust_record`) to the global trust account
//! (the `hkask-trust` crate's `TrustLedger`). Every trust event recorded
//! on a goal is ALSO recorded in the global trust ledger — the same
//! entity (`"trust:ledger"`), the same wire format (the `LedgerEvent`
//! JSON schema), the same storage model (h_mems in the kata-kanban's
//! own `HMemStore`). The substrate-independent wire format and query
//! logic live in `hkask_mcp_server::trust_ledger_hmem`, shared with the
//! curator's store so the two cannot drift.
//!
//! The regulation loop reads trust events by entity (`"trust:ledger"`);
//! this store makes the kata-kanban's trust events queryable by that
//! entity in the kata-kanban's own store. If the regulation loop gains
//! access to this store (via `goal_scan_for_host` or a shared database),
//! it reads the trust signal from here.

use chrono::{DateTime, Utc};
use hkask_mcp_server::trust_ledger_hmem;
use hkask_storage::HMemStore;
use hkask_trust::{LedgerEvent, LedgerEventId, Outcome, TrustError, TrustLedgerStore};
use hkask_types::WebID;

/// Stores each `LedgerEvent` as an h_mem in the kata-kanban's `HMemStore`:
/// entity `"trust:ledger"`, attribute = the event id (a UUID), value =
/// JSON carrying the full event. Binding 5's substrate for the kata-kanban.
pub struct KanbanTrustLedgerStore {
    store: HMemStore,
    webid: WebID,
}

impl KanbanTrustLedgerStore {
    pub fn new(store: HMemStore, webid: WebID) -> Self {
        Self { store, webid }
    }
}

impl TrustLedgerStore for KanbanTrustLedgerStore {
    fn append(&self, event: LedgerEvent) -> Result<(), TrustError> {
        let h_mem = trust_ledger_hmem::event_to_h_mem(&event, &self.webid);
        self.store
            .insert(&h_mem)
            .map_err(|e| trust_ledger_hmem::store_unavailable("kata-kanban", "insert", e))?;
        Ok(())
    }

    fn resolve_outcome(
        &self,
        id: &LedgerEventId,
        outcome: Outcome,
        oracle_note: Option<String>,
    ) -> Result<LedgerEvent, TrustError> {
        let h_mems = self
            .store
            .query_by_entity(trust_ledger_hmem::TRUST_LEDGER_ENTITY)
            .map_err(|e| {
                trust_ledger_hmem::store_unavailable("kata-kanban", "query_by_entity", e)
            })?;

        let (event, old_h_mem_id) =
            trust_ledger_hmem::resolve_event(&h_mems, id, outcome, oracle_note)?;

        // Delete the old h_mem, store the updated one (no update-in-place
        // on HMemStore; delete + insert is the replacement path).
        self.store
            .delete_by_id(&old_h_mem_id)
            .map_err(|e| trust_ledger_hmem::store_unavailable("kata-kanban", "delete_by_id", e))?;
        let updated = trust_ledger_hmem::event_to_h_mem(&event, &self.webid);
        self.store
            .insert(&updated)
            .map_err(|e| trust_ledger_hmem::store_unavailable("kata-kanban", "insert", e))?;

        Ok(event)
    }

    fn window(&self, since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
        let h_mems = self
            .store
            .query_by_entity(trust_ledger_hmem::TRUST_LEDGER_ENTITY)
            .map_err(|e| {
                trust_ledger_hmem::store_unavailable("kata-kanban", "query_by_entity", e)
            })?;

        Ok(trust_ledger_hmem::window_events(&h_mems, since))
    }
}
