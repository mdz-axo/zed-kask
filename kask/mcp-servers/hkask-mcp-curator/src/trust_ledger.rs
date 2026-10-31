//! Trust-ledger storage (Binding 5's substrate — trust events ARE h_mems).
//!
//! The four MCP tools (`trust_ledger_record_instruction`,
//! `trust_ledger_record_outcome`, `trust_ledger_state`,
//! `trust_ledger_steer`) live on `CuratorServer` alongside the other
//! curator tools (see the main file). This module provides the
//! `CuratorTrustLedgerStore` — the `TrustLedgerStore` implementation that
//! persists trust events as h_mems in the curator's `MemoryStore`
//! (entity `trust:ledger`, attribute = the event id, value = JSON
//! carrying the full event). The substrate-independent wire format and
//! query logic live in `hkask_mcp_server::trust_ledger_hmem`, shared
//! with the kata-kanban's store so the two cannot drift.
//!
//! Binding 5 closes mechanically: trust events ARE h_mems, so
//! `memory_insert`'s evidence gate (`fetch_existing_h_mem`) can cite a
//! trust event's h_mem as evidence for a trust lesson.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hkask_mcp_server::trust_ledger_hmem;
use hkask_memory::MemoryStore;
use hkask_trust::{LedgerEvent, LedgerEventId, Outcome, TrustError, TrustLedgerStore};
use hkask_types::WebID;

/// Stores each `LedgerEvent` as an h_mem in the curator's `MemoryStore`:
/// entity `trust:ledger`, attribute = the event id (a UUID), value =
/// JSON carrying the full event. Binding 5's substrate.
pub struct CuratorTrustLedgerStore {
    memory: Arc<MemoryStore>,
    webid: WebID,
}

impl CuratorTrustLedgerStore {
    pub fn new(memory: Arc<MemoryStore>, webid: WebID) -> Self {
        Self { memory, webid }
    }
}

impl TrustLedgerStore for CuratorTrustLedgerStore {
    fn append(&self, event: LedgerEvent) -> Result<(), TrustError> {
        let h_mem = trust_ledger_hmem::event_to_h_mem(&event, &self.webid);
        self.memory
            .store(h_mem)
            .map_err(|_| TrustError::StoreUnavailable)?;
        Ok(())
    }

    fn resolve_outcome(
        &self,
        id: &LedgerEventId,
        outcome: Outcome,
        oracle_note: Option<String>,
    ) -> Result<LedgerEvent, TrustError> {
        let h_mems = self
            .memory
            .query_deduped_untouched(trust_ledger_hmem::TRUST_LEDGER_ENTITY)
            .map_err(|_| TrustError::StoreUnavailable)?;

        let (event, old_h_mem_id) =
            trust_ledger_hmem::resolve_event(&h_mems, id, outcome, oracle_note)?;

        // Delete the old row, store the updated one (no update-in-place on
        // HMemStore; delete + insert is the replacement path).
        self.memory
            .delete_h_mem(&old_h_mem_id)
            .map_err(|_| TrustError::StoreUnavailable)?;
        let updated = trust_ledger_hmem::event_to_h_mem(&event, &self.webid);
        self.memory
            .store(updated)
            .map_err(|_| TrustError::StoreUnavailable)?;

        Ok(event)
    }

    fn window(&self, since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
        let h_mems = self
            .memory
            .query_deduped_untouched(trust_ledger_hmem::TRUST_LEDGER_ENTITY)
            .map_err(|_| TrustError::StoreUnavailable)?;

        Ok(trust_ledger_hmem::window_events(&h_mems, since))
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_trust::{InMemoryTrustLedger, Outcome, TrustLedger, Weight};

    /// Binding 5's round-trip: record → resolve → the trust account
    /// reflects the outcome (kept = +12 from prior 300 = 312).
    #[test]
    fn trust_event_round_trips() {
        let ledger = TrustLedger::new(std::sync::Arc::new(InMemoryTrustLedger::new()));
        let since = Utc::now() - chrono::Duration::hours(24);

        let id = ledger
            .record_instruction("verify every quote", "i", Weight::Explicit)
            .unwrap();
        ledger.record_outcome(&id, Outcome::Kept, None).unwrap();

        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.kept, 1);
        assert_eq!(stats.trust_points, 312);
    }

    /// StoreUnavailable surfaces, never zero.
    #[test]
    fn store_unavailable_surfaces() {
        struct Dead;
        impl TrustLedgerStore for Dead {
            fn append(&self, _e: LedgerEvent) -> Result<(), TrustError> {
                Err(TrustError::StoreUnavailable)
            }
            fn resolve_outcome(
                &self,
                _id: &LedgerEventId,
                _o: Outcome,
                _n: Option<String>,
            ) -> Result<LedgerEvent, TrustError> {
                Err(TrustError::StoreUnavailable)
            }
            fn window(&self, _s: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
                Err(TrustError::StoreUnavailable)
            }
        }
        let ledger = TrustLedger::new(std::sync::Arc::new(Dead));
        assert!(matches!(
            ledger.trust_state(Utc::now()),
            Err(TrustError::StoreUnavailable)
        ));
    }
}
