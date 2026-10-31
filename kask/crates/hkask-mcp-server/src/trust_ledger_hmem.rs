//! The HMem substrate for trust-surface persistence (Binding 5 — trust
//! events ARE h_mems; trust internalization v2 adds the proposal and
//! anchor entities).
//!
//! One copy of the substrate-independent core: the event↔h_mem wire format
//! (entity `trust:ledger`, attribute = the event id, value = JSON carrying
//! the full event), the proposal↔h_mem wire format (entity
//! `trust:ledger:proposal`, attribute = the proposal id), the anchor↔h_mem
//! wire format (entity `trust:anchor`, attribute = the anchor id), the
//! find-and-resolve reconstruction, and the window filter. The server stores
//! (the curator's `CuratorTrustLedgerStore` over `MemoryStore`, the
//! kata-kanban's `KanbanTrustLedgerStore` over `HMemStore`) supply only
//! their substrate calls — insert / delete / query — so the wire formats
//! cannot drift between them. The regulation loop's own constant
//! (`hkask-regulation`) reads events by the same entity from its store:
//! this module owns the servers' copy of that cross-crate contract.
//!
//! The proposal and anchor entities are exact-match distinct from
//! `trust:ledger` (the stores query `WHERE entity = ?`), so the event
//! window never sees them.

use chrono::{DateTime, Utc};
use hkask_storage::{HMem, HMemId};
use hkask_trust::{LedgerEvent, LedgerEventId, Outcome, RenegotiationProposal, TrustError, Weight};
use hkask_types::WebID;

/// The entity for trust-ledger event h_mems.
pub use hkask_trust::TRUST_LEDGER_ENTITY;

/// The entity for filed renegotiation proposals (trust internalization
/// v2): the agent's required move before deviating, durably awaiting the
/// user's decision. Exact-match distinct from [`TRUST_LEDGER_ENTITY`] so
/// the event window never reads proposal rows.
pub const TRUST_PROPOSAL_ENTITY: &str = "trust:ledger:proposal";

/// The entity for work anchors (trust internalization v2, step 1): the
/// goal understanding, the terrain, and the map edges, recorded before
/// work proceeds.
pub const TRUST_ANCHOR_ENTITY: &str = "trust:anchor";

/// A proposal awaiting the user's decision.
pub const PROPOSAL_STATUS_PENDING: &str = "pending";
/// The user accepted the replacement expectation.
pub const PROPOSAL_STATUS_ACCEPTED: &str = "accepted";
/// The user rejected; the original expectation stands.
pub const PROPOSAL_STATUS_REJECTED: &str = "rejected";

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
            Outcome::Renegotiated => "renegotiated",
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
            Some("renegotiated") => Outcome::Renegotiated,
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

/// Find one registered expectation by id among the store's event rows —
/// the proposal surface's event-existence and live-state checks read the
/// same rows the fold reads.
pub fn find_event(h_mems: &[HMem], id: &LedgerEventId) -> Result<LedgerEvent, TrustError> {
    h_mems
        .iter()
        .find(|h| h.attribute == id.as_str())
        .map(h_mem_to_event)
        .ok_or_else(|| TrustError::EventNotFound(id.to_string()))
}

// ── Renegotiation proposals (trust internalization v2) ───────────────────

/// One priced alternative inside a filed proposal (the wire form of
/// `hkask_trust::ProposalOption`).
#[derive(Clone, Debug, PartialEq)]
pub struct StoredProposalOption {
    /// What the agent would do instead.
    pub description: String,
    /// The option's priced trust delta in points (signed).
    pub trust_delta_points: i32,
    /// The option's cost to the user, stated.
    pub user_cost: String,
}

/// The user's recorded decision on a filed proposal — the decision, the
/// oracle note claiming the human veto (the gate is procedural: the
/// algedonic review verifies the note), and, on acceptance, the
/// replacement expectation and its registered event id.
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalResolutionRecord {
    /// "accepted" | "rejected".
    pub decision: String,
    /// When the decision was recorded.
    pub resolved_at: DateTime<Utc>,
    /// What claims the human decision (a conversation reference, a review
    /// note). The tool cannot authenticate its caller; this note is the
    /// audit surface the algedonic review verifies.
    pub oracle_note: String,
    /// The replacement expectation, when accepted.
    pub new_expectation: Option<String>,
    /// The replacement's registered event id, when accepted.
    pub new_event_id: Option<String>,
}

/// A filed renegotiation proposal as a durable h_mem: the pure payload
/// plus its lifecycle state — `pending` until the user's human veto lands,
/// then `accepted` or `rejected` with the resolution record.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredProposal {
    /// The proposal's own id (the h_mem attribute).
    pub proposal_id: String,
    /// The registered expectation in conflict.
    pub event_id: String,
    /// What the agent found, with measurements.
    pub conflict: String,
    /// The priced alternatives.
    pub options: Vec<StoredProposalOption>,
    /// The agent's recommendation, and why.
    pub recommendation: String,
    /// What the user will know after reading it that they didn't before.
    /// Non-empty or the proposal is invalid (the generative constraint).
    pub teaching_payload: String,
    /// `pending` | `accepted` | `rejected`.
    pub status: String,
    /// When the proposal was filed — BEFORE any deviation. A violation
    /// outcome with no preceding proposal is the silent-deviation class,
    /// provable from the rows.
    pub filed_at: DateTime<Utc>,
    /// The recorded decision, once the user has decided.
    pub resolution: Option<ProposalResolutionRecord>,
}

impl StoredProposal {
    /// The pure payload — the model's [`RenegotiationProposal`], which
    /// `resolve_proposal` consumes.
    pub fn payload(&self) -> RenegotiationProposal {
        RenegotiationProposal {
            event_id: LedgerEventId::from_string(&self.event_id),
            conflict: self.conflict.clone(),
            options: self
                .options
                .iter()
                .map(|o| hkask_trust::ProposalOption {
                    description: o.description.clone(),
                    trust_delta_points: o.trust_delta_points,
                    user_cost: o.user_cost.clone(),
                })
                .collect(),
            recommendation: self.recommendation.clone(),
            teaching_payload: self.teaching_payload.clone(),
        }
    }
}

/// Serialize a filed proposal into its h_mem: entity
/// [`TRUST_PROPOSAL_ENTITY`], attribute = the proposal id, value = JSON
/// carrying the proposal plus its lifecycle state.
pub fn proposal_to_h_mem(proposal: &StoredProposal, webid: &WebID) -> HMem {
    let resolution = match &proposal.resolution {
        None => serde_json::json!(null),
        Some(r) => serde_json::json!({
            "decision": r.decision,
            "resolved_at": r.resolved_at.to_rfc3339(),
            "oracle_note": r.oracle_note,
            "new_expectation": r.new_expectation,
            "new_event_id": r.new_event_id,
        }),
    };
    let value = serde_json::json!({
        "event_id": proposal.event_id,
        "conflict": proposal.conflict,
        "options": proposal
            .options
            .iter()
            .map(|o| serde_json::json!({
                "description": o.description,
                "trust_delta_points": o.trust_delta_points,
                "user_cost": o.user_cost,
            }))
            .collect::<Vec<_>>(),
        "recommendation": proposal.recommendation,
        "teaching_payload": proposal.teaching_payload,
        "status": proposal.status,
        "filed_at": proposal.filed_at.to_rfc3339(),
        "resolution": resolution,
    });
    HMem::new(TRUST_PROPOSAL_ENTITY, &proposal.proposal_id, value, *webid)
}

/// Rebuild a filed proposal from its h_mem (the inverse of
/// [`proposal_to_h_mem`]); absent fields fall back to the filed-state
/// defaults (status `pending`, no resolution, no options). Internal — the
/// stores consume the find/list helpers, not the raw converter.
fn h_mem_to_proposal(h: &HMem) -> StoredProposal {
    let v = &h.value;
    StoredProposal {
        proposal_id: h.attribute.clone(),
        event_id: v
            .get("event_id")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        conflict: v
            .get("conflict")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        options: v
            .get("options")
            .and_then(|o| o.as_array())
            .map(|options| {
                options
                    .iter()
                    .map(|o| StoredProposalOption {
                        description: o
                            .get("description")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                        trust_delta_points: o
                            .get("trust_delta_points")
                            .and_then(|p| p.as_i64())
                            .map(|p| p as i32)
                            .unwrap_or(0),
                        user_cost: o
                            .get("user_cost")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        recommendation: v
            .get("recommendation")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        teaching_payload: v
            .get("teaching_payload")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        status: v
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or(PROPOSAL_STATUS_PENDING)
            .to_string(),
        filed_at: v
            .get("filed_at")
            .and_then(|t| t.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now),
        resolution: v.get("resolution").and_then(|r| {
            let decision = r.get("decision")?.as_str()?.to_string();
            Some(ProposalResolutionRecord {
                decision,
                resolved_at: r
                    .get("resolved_at")
                    .and_then(|t| t.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now),
                oracle_note: r
                    .get("oracle_note")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                new_expectation: r
                    .get("new_expectation")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
                new_event_id: r
                    .get("new_event_id")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
            })
        }),
    }
}

/// Find one filed proposal by id among the store's rows. Returns the
/// rebuilt proposal plus the OLD row's id — the caller performs the
/// substrate delete + re-insert when recording the user's decision.
pub fn find_proposal(
    h_mems: &[HMem],
    proposal_id: &str,
) -> Result<(StoredProposal, HMemId), TrustError> {
    let h_mem = h_mems
        .iter()
        .find(|h| h.attribute == proposal_id)
        .ok_or_else(|| TrustError::ProposalNotFound(proposal_id.to_string()))?;
    Ok((h_mem_to_proposal(h_mem), h_mem.id))
}

/// All proposals among the rows, newest-filed first (the operator's
/// decision surface reads the freshest proposal first; ties break by
/// proposal id so the order is deterministic).
pub fn proposals(h_mems: &[HMem]) -> Vec<StoredProposal> {
    let mut all: Vec<StoredProposal> = h_mems.iter().map(h_mem_to_proposal).collect();
    all.sort_by(|a, b| {
        b.filed_at
            .cmp(&a.filed_at)
            .then_with(|| a.proposal_id.cmp(&b.proposal_id))
    });
    all
}

// ── Work anchors (trust internalization v2, step 1) ──────────────────────

/// A recorded work anchor as a durable h_mem: the pure anchor payload plus
/// its task key and the supersede ordering. The highest revision for a
/// task key is the current anchor; lower revisions stay as the audit trail.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredAnchor {
    /// The anchor's own id (the h_mem attribute).
    pub anchor_id: String,
    /// The task the anchor belongs to (the operator's read key).
    pub task_key: String,
    /// What the agent understands the goal to be.
    pub goal_understanding: String,
    /// The map as the agent sees it.
    pub terrain: String,
    /// Where the model's map ends — what it does not know yet.
    pub map_edges: Vec<String>,
    /// The supersede ordering: a later record for the same task key
    /// carries a higher revision and becomes current.
    pub revision: u64,
    /// When this anchor was recorded.
    pub recorded_at: DateTime<Utc>,
}

/// Serialize a recorded anchor into its h_mem: entity
/// [`TRUST_ANCHOR_ENTITY`], attribute = the anchor id.
pub fn anchor_to_h_mem(anchor: &StoredAnchor, webid: &WebID) -> HMem {
    let value = serde_json::json!({
        "task_key": anchor.task_key,
        "goal_understanding": anchor.goal_understanding,
        "terrain": anchor.terrain,
        "map_edges": anchor.map_edges,
        "revision": anchor.revision,
        "recorded_at": anchor.recorded_at.to_rfc3339(),
    });
    HMem::new(TRUST_ANCHOR_ENTITY, &anchor.anchor_id, value, *webid)
}

/// Rebuild a recorded anchor from its h_mem (the inverse of
/// [`anchor_to_h_mem`]). Internal — the stores consume
/// [`anchors_for_task`], not the raw converter.
fn h_mem_to_anchor(h: &HMem) -> StoredAnchor {
    let v = &h.value;
    StoredAnchor {
        anchor_id: h.attribute.clone(),
        task_key: v
            .get("task_key")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        goal_understanding: v
            .get("goal_understanding")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        terrain: v
            .get("terrain")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        map_edges: v
            .get("map_edges")
            .and_then(|m| m.as_array())
            .map(|edges| {
                edges
                    .iter()
                    .filter_map(|e| e.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        revision: v.get("revision").and_then(|r| r.as_u64()).unwrap_or(0),
        recorded_at: v
            .get("recorded_at")
            .and_then(|t| t.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now),
    }
}

/// The anchors recorded for one task key, highest revision first (the
/// current anchor reads first; the superseded trail follows).
pub fn anchors_for_task(h_mems: &[HMem], task_key: &str) -> Vec<StoredAnchor> {
    let mut for_task: Vec<StoredAnchor> = h_mems
        .iter()
        .map(h_mem_to_anchor)
        .filter(|a| a.task_key == task_key)
        .collect();
    for_task.sort_by(|a, b| {
        b.revision
            .cmp(&a.revision)
            .then_with(|| a.anchor_id.cmp(&b.anchor_id))
    });
    for_task
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_trust::{Outcome, Weight};

    /// The renegotiated outcome round-trips through the shared wire
    /// format (trust internalization v2): a renegotiated event stores
    /// as "renegotiated" and reads back as Renegotiated — never
    /// silently degraded to Unverified.
    #[test]
    fn renegotiated_round_trips_through_the_wire_format() {
        let event = LedgerEvent {
            id: LedgerEventId::new(),
            text: "fix the audio offset".to_string(),
            source: "turn-1".to_string(),
            weight: Weight::Explicit,
            outcome: Outcome::Renegotiated,
            ts: Utc::now(),
            oracle_note: Some("user accepted the replacement".to_string()),
        };
        let h_mem = event_to_h_mem(&event, &WebID::new());
        assert_eq!(
            h_mem.value.get("outcome").and_then(|o| o.as_str()),
            Some("renegotiated")
        );
        let round = h_mem_to_event(&h_mem);
        assert_eq!(round.outcome, Outcome::Renegotiated);
        assert_eq!(round.text, event.text);
        assert_eq!(round.weight, event.weight);
    }

    /// A filed proposal round-trips through the shared wire format: the
    /// priced options, the teaching payload, and the pending status all
    /// survive the h_mem row — the operator's decision surface reads what
    /// the agent filed.
    #[test]
    fn proposal_round_trips_through_the_wire_format() {
        let pending = StoredProposal {
            proposal_id: "11111111-1111-1111-1111-111111111111".to_string(),
            event_id: "22222222-2222-2222-2222-222222222222".to_string(),
            conflict: "the fix costs 4x the shortcut".to_string(),
            options: vec![StoredProposalOption {
                description: "fix the offset".to_string(),
                trust_delta_points: 12,
                user_cost: "4x the render time".to_string(),
            }],
            recommendation: "fix the offset: frame-exactness is the goal".to_string(),
            teaching_payload: "lossless and frame-exact are different properties".to_string(),
            status: PROPOSAL_STATUS_PENDING.to_string(),
            filed_at: Utc::now(),
            resolution: None,
        };
        let h_mem = proposal_to_h_mem(&pending, &WebID::new());
        assert_eq!(h_mem.entity, TRUST_PROPOSAL_ENTITY);
        assert_eq!(h_mem.attribute, pending.proposal_id);
        let round = h_mem_to_proposal(&h_mem);
        assert_eq!(round, pending, "the pending proposal round-trips intact");

        // The resolved proposal round-trips with its resolution record: the
        // decision, the oracle note claiming the human veto, and the
        // replacement's event id all survive.
        let mut accepted = pending;
        accepted.status = PROPOSAL_STATUS_ACCEPTED.to_string();
        accepted.resolution = Some(ProposalResolutionRecord {
            decision: "accepted".to_string(),
            resolved_at: Utc::now(),
            oracle_note: "user accepted in conversation 2026-10-31".to_string(),
            new_expectation: Some("ship the reel with the defect documented".to_string()),
            new_event_id: Some("33333333-3333-3333-3333-333333333333".to_string()),
        });
        let round = h_mem_to_proposal(&proposal_to_h_mem(&accepted, &WebID::new()));
        assert_eq!(round, accepted, "the resolution record round-trips intact");
    }

    /// The proposal list helpers order newest-filed first and find by id —
    /// the decision surface reads the freshest proposal first, and a
    /// missing id is a typed refusal naming it.
    #[test]
    fn proposal_helpers_order_and_find() {
        let older = StoredProposal {
            proposal_id: "aaaa1111-1111-1111-1111-111111111111".to_string(),
            event_id: "22222222-2222-2222-2222-222222222222".to_string(),
            conflict: "first".to_string(),
            options: vec![],
            recommendation: "r".to_string(),
            teaching_payload: "t".to_string(),
            status: PROPOSAL_STATUS_PENDING.to_string(),
            filed_at: Utc::now() - chrono::Duration::hours(2),
            resolution: None,
        };
        let newer = StoredProposal {
            proposal_id: "bbbb2222-2222-2222-2222-222222222222".to_string(),
            filed_at: Utc::now() - chrono::Duration::hours(1),
            conflict: "second".to_string(),
            ..older.clone()
        };
        let h_mems = vec![
            proposal_to_h_mem(&older, &WebID::new()),
            proposal_to_h_mem(&newer, &WebID::new()),
        ];

        let listed = proposals(&h_mems);
        assert_eq!(listed[0].proposal_id, newer.proposal_id, "newest first");
        assert_eq!(listed[1].proposal_id, older.proposal_id);

        let (found, row) =
            find_proposal(&h_mems, &older.proposal_id).expect("the older proposal is findable");
        assert_eq!(found.conflict, "first");
        assert!(
            !row.to_string().is_empty(),
            "the old row id returns for the update path"
        );

        let missing = find_proposal(&h_mems, "missing-id");
        assert!(matches!(
            missing,
            Err(hkask_trust::TrustError::ProposalNotFound(ref id)) if id == "missing-id"
        ));
    }

    /// An anchor round-trips through the shared wire format: the task key,
    /// the goal understanding, the terrain, and the map edges all survive
    /// the h_mem row — the operator's one read sees what the agent anchored.
    #[test]
    fn anchor_round_trips_through_the_wire_format() {
        let anchor = StoredAnchor {
            anchor_id: "44444444-4444-4444-4444-444444444444".to_string(),
            task_key: "card-fb845bd1".to_string(),
            goal_understanding: "build the anchor surface".to_string(),
            terrain: "the curator hosts the tool surface".to_string(),
            map_edges: vec![
                "the revision ordering".to_string(),
                "who owns anchor retention".to_string(),
            ],
            revision: 2,
            recorded_at: Utc::now(),
        };
        let h_mem = anchor_to_h_mem(&anchor, &WebID::new());
        assert_eq!(h_mem.entity, TRUST_ANCHOR_ENTITY);
        assert_eq!(h_mem.attribute, anchor.anchor_id);
        let round = h_mem_to_anchor(&h_mem);
        assert_eq!(round, anchor, "the anchor round-trips intact");

        // The task filter returns the highest revision first (the current
        // anchor), then the trail.
        let first = StoredAnchor {
            anchor_id: "55555555-5555-5555-5555-555555555555".to_string(),
            revision: 1,
            recorded_at: Utc::now() - chrono::Duration::hours(1),
            ..anchor.clone()
        };
        let h_mems = vec![
            anchor_to_h_mem(&first, &WebID::new()),
            anchor_to_h_mem(&anchor, &WebID::new()),
        ];
        let for_task = anchors_for_task(&h_mems, "card-fb845bd1");
        assert_eq!(for_task.len(), 2);
        assert_eq!(for_task[0].revision, 2, "the current anchor reads first");
        assert_eq!(for_task[1].revision, 1, "the trail follows");
        assert!(anchors_for_task(&h_mems, "other-task").is_empty());
    }
}
