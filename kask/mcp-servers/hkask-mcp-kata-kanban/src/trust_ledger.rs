//! The kata-kanban's trust-ledger store (Binding 5's substrate).
//!
//! Bridges the per-goal trust ledger (the Goal's `trust_events` Vec,
//! recorded by `kanban_goal_trust_record`) to the global trust account
//! (the `hkask-trust` crate's `TrustLedger`). Every trust event recorded
//! on a goal is ALSO recorded in the global trust ledger — the same
//! entity (`"trust:ledger"`), the same wire format (the `LedgerEvent`
//! JSON schema), the same storage model (h_mems in the kata-kanban's
//! own `HMemStore`).
//!
//! The regulation loop reads trust events by entity (`"trust:ledger"`);
//! this store makes the kata-kanban's trust events queryable by that
//! entity in the kata-kanban's own store. If the regulation loop gains
//! access to this store (via `goal_scan_for_host` or a shared database),
//! it reads the trust signal from here.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hkask_storage::{HMem, HMemStore};
use hkask_trust::{LedgerEvent, LedgerEventId, Outcome, TrustError, TrustLedgerStore, Weight};
use hkask_types::WebID;

/// The entity prefix for all trust-ledger h_mems.
pub const TRUST_LEDGER_ENTITY: &str = "trust:ledger";

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

    fn event_to_h_mem(&self, event: &LedgerEvent) -> HMem {
        let value = serde_json::json!({
            "text": event.text,
            "source": event.source,
            "weight": event.weight.points(),
            "outcome": match event.outcome {
                Outcome::Kept => "kept",
                Outcome::Violated => "violated",
                Outcome::Unverified => "unverified",
            },
            "ts": event.ts.to_rfc3339(),
            "oracle_note": event.oracle_note,
        });
        HMem::new(
            TRUST_LEDGER_ENTITY,
            event.id.as_str(),
            value,
            self.webid.clone(),
        )
    }

    fn h_mem_to_event(h: &HMem) -> LedgerEvent {
        let v = &h.value;
        LedgerEvent {
            id: LedgerEventId::from_string(&h.attribute),
            text: v
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string(),
            source: v
                .get("source")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            weight: match v.get("weight").and_then(|w| w.as_i64()) {
                Some(1) => Weight::Implicit,
                Some(2) => Weight::Committed,
                _ => Weight::Explicit,
            },
            outcome: match v.get("outcome").and_then(|o| o.as_str()) {
                Some("kept") => Outcome::Kept,
                Some("violated") => Outcome::Violated,
                _ => Outcome::Unverified,
            },
            ts: v
                .get("ts")
                .and_then(|t| t.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
            oracle_note: v
                .get("oracle_note")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string()),
        }
    }
}

impl TrustLedgerStore for KanbanTrustLedgerStore {
    fn append(&self, event: LedgerEvent) -> Result<(), TrustError> {
        let h_mem = self.event_to_h_mem(&event);
        self.store
            .insert(&h_mem)
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
            .store
            .query_by_entity(TRUST_LEDGER_ENTITY)
            .map_err(|_| TrustError::StoreUnavailable)?;

        let h_mem = h_mems
            .iter()
            .find(|h| h.attribute == id.as_str())
            .ok_or_else(|| TrustError::EventNotFound(id.to_string()))?;

        // Reconstruct the event with the resolved outcome.
        let mut event = Self::h_mem_to_event(h_mem);
        event.outcome = outcome;
        event.oracle_note = oracle_note;

        // Delete the old h_mem, store the updated one (no update-in-place
        // on HMemStore; delete + insert is the replacement path).
        self.store
            .delete_by_id(&h_mem.id)
            .map_err(|_| TrustError::StoreUnavailable)?;
        let updated = self.event_to_h_mem(&event);
        self.store
            .insert(&updated)
            .map_err(|_| TrustError::StoreUnavailable)?;

        Ok(event)
    }

    fn window(&self, since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
        let h_mems = self
            .store
            .query_by_entity(TRUST_LEDGER_ENTITY)
            .map_err(|_| TrustError::StoreUnavailable)?;

        Ok(h_mems
            .iter()
            .filter(|h| {
                h.value
                    .get("ts")
                    .and_then(|t| t.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc) >= since)
                    .unwrap_or(false)
            })
            .map(Self::h_mem_to_event)
            .collect())
    }
}
