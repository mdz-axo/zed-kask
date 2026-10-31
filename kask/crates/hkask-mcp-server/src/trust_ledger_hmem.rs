//! The HMem substrate for trust-ledger persistence (Binding 5 — trust
//! events ARE h_mems).
//!
//! One copy of the substrate-independent core: the event↔h_mem wire format
//! (entity `trust:ledger`, attribute = the event id, value = JSON carrying
//! the full event), the find-and-resolve reconstruction, and the window
//! filter. The server stores (the curator's `CuratorTrustLedgerStore` over
//! `MemoryStore`, the kata-kanban's `KanbanTrustLedgerStore` over
//! `HMemStore`) supply only their substrate calls — insert / delete /
//! query — so the wire format cannot drift between them. The regulation
//! loop's own constant (`hkask-regulation`) reads events by the same
//! entity from its store: this module owns the servers' copy of that
//! cross-crate contract.

use chrono::{DateTime, Utc};
use hkask_storage::{HMem, HMemId};
use hkask_trust::{LedgerEvent, LedgerEventId, Outcome, TrustError, Weight};
use hkask_types::WebID;

/// The entity prefix for all trust-ledger h_mems.
pub const TRUST_LEDGER_ENTITY: &str = "trust:ledger";

/// Serialize a trust event into its h_mem: entity `trust:ledger`,
/// attribute = the event id, value = JSON carrying the full event.
pub fn event_to_h_mem(event: &LedgerEvent, webid: &WebID) -> HMem {
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
    HMem::new(TRUST_LEDGER_ENTITY, event.id.as_str(), value, *webid)
}

/// Rebuild a trust event from its h_mem (the inverse of
/// [`event_to_h_mem`]); absent fields fall back to empty/unverified
/// defaults and the event id from the attribute.
pub fn h_mem_to_event(h: &HMem) -> LedgerEvent {
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

/// Find the event's row among the store's query results and rebuild it with
/// the resolved outcome. Returns the rebuilt event plus the OLD row's id —
/// the caller performs the substrate delete + re-insert (no update-in-place
/// on HMemStore; delete + insert is the replacement path).
pub fn resolve_event(
    h_mems: &[HMem],
    id: &LedgerEventId,
    outcome: Outcome,
    oracle_note: Option<String>,
) -> Result<(LedgerEvent, HMemId), TrustError> {
    let h_mem = h_mems
        .iter()
        .find(|h| h.attribute == id.as_str())
        .ok_or_else(|| TrustError::EventNotFound(id.to_string()))?;

    // Reconstruct the event with the resolved outcome.
    let mut event = h_mem_to_event(h_mem);
    event.outcome = outcome;
    event.oracle_note = oracle_note;
    Ok((event, h_mem.id))
}

/// The events in `h_mems` recorded at or after `since` — the ts parsed from
/// each row's JSON; a row without a parseable ts is in no window.
pub fn window_events(h_mems: &[HMem], since: DateTime<Utc>) -> Vec<LedgerEvent> {
    h_mems
        .iter()
        .filter(|h| {
            h.value
                .get("ts")
                .and_then(|t| t.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc) >= since)
                .unwrap_or(false)
        })
        .map(h_mem_to_event)
        .collect()
}
