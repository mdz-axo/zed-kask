//! Trust-ledger storage (Binding 5's substrate — trust events ARE h_mems).
//!
//! The four MCP tools (`trust_ledger_record_instruction`,
//! `trust_ledger_record_outcome`, `trust_ledger_state`,
//! `trust_ledger_steer`) live on `CuratorServer` alongside the other
//! curator tools (see the main file). This module provides the
//! `CuratorTrustLedgerStore` — the `TrustLedgerStore` implementation
//! that persists trust events as h_mems in the curator's `MemoryStore`
//! (entity `trust:ledger`, attribute = the event id, value = JSON
//! carrying the full event).
//!
//! Binding 5 closes mechanically: trust events ARE h_mems, so
//! `memory_insert`'s evidence gate (`fetch_existing_h_mem`) can cite a
//! trust event's h_mem as evidence for a trust lesson.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use hkask_memory::MemoryStore;
use hkask_storage::HMem;
use hkask_trust::{LedgerEvent, LedgerEventId, Outcome, TrustError, TrustLedgerStore, Weight};
use hkask_types::WebID;

/// The entity prefix for all trust-ledger h_mems.
pub const TRUST_LEDGER_ENTITY: &str = "trust:ledger";

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
        HMem::new(TRUST_LEDGER_ENTITY, event.id.as_str(), value, self.webid)
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

impl TrustLedgerStore for CuratorTrustLedgerStore {
    fn append(&self, event: LedgerEvent) -> Result<(), TrustError> {
        let h_mem = self.event_to_h_mem(&event);
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
            .query_deduped_untouched(TRUST_LEDGER_ENTITY)
            .map_err(|_| TrustError::StoreUnavailable)?;

        let h_mem = h_mems
            .iter()
            .find(|h| h.attribute == id.as_str())
            .ok_or_else(|| TrustError::EventNotFound(id.to_string()))?;

        // Reconstruct the event with the resolved outcome.
        let mut event = Self::h_mem_to_event(h_mem);
        event.outcome = outcome;
        event.oracle_note = oracle_note;

        // Delete the old row, store the updated one (no update-in-place on
        // HMemStore; delete + insert is the replacement path).
        let old_h_mem_id = h_mem.id;
        self.memory
            .delete_h_mem(&old_h_mem_id)
            .map_err(|_| TrustError::StoreUnavailable)?;
        let updated = self.event_to_h_mem(&event);
        self.memory
            .store(updated)
            .map_err(|_| TrustError::StoreUnavailable)?;

        Ok(event)
    }

    fn window(&self, since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
        let h_mems = self
            .memory
            .query_deduped_untouched(TRUST_LEDGER_ENTITY)
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
