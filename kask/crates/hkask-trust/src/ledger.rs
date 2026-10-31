//! The ledger module: events with verification state, the storage-backed
//! trust account, and the `steer` projection over candidate actions.
//!
//! The pure fold (`trust_points_from`) and the cliff live in the crate
//! root; this module owns the EVENT side of the ledger — the record of
//! what was expected and what happened — and the windowed account that
//! reads it. The intake contract (from the trust-internalization program,
//! `kask/docs/research/trust-surface-bindings.md`):
//!
//! - `record_instruction` captures verbatim instruction text + source
//!   (the interaction id) + weight. Weight classification is P
//!   (model-judged, auditable in the row); capture is D (mechanical).
//! - `record_outcome` resolves the verification state from an oracle
//!   (a test, a build, a grep, a re-run) where one exists, and stays
//!   `Unverified` — visible, never silently zero — where one does not.
//! - The account is windowed: `trust_state` folds the window's resolved
//!   events from the prior; `window_stats` reports the visible counts.
//! - A store outage surfaces as [`TrustError::StoreUnavailable`] — the
//!   account is unknown, not zero (a broken feedback loop is a broken
//!   system; the regulation `Sensor` contract "None is not evidence of
//!   recovery" carries the same rule).
//!
//! Storage is a trait ([`TrustLedgerStore`]); the production impl (events
//! as curator h_mems, entity `trust:ledger:<id>`) lives in the curator
//! server. [`InMemoryTrustLedger`] is the test double and the in-process
//! store for the regulation loop's reads.

use std::fmt;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use crate::{Event, Outcome, Reliance, SteerProjection, TrustError, Weight};

/// An opaque ledger-event id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LedgerEventId(String);

impl LedgerEventId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn from_string(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Default for LedgerEventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for LedgerEventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One ledger row: the verbatim expectation, its source, weight, and
/// verification state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEvent {
    pub id: LedgerEventId,
    /// The verbatim instruction or expectation text.
    pub text: String,
    /// The interaction id (the user turn) the instruction came from.
    pub source: String,
    /// How explicitly the expectation was stated (1=implicit, 2=committed, 3=explicit).
    pub weight: Weight,
    /// The verification state: kept, violated, or unverified (never silent).
    pub outcome: Outcome,
    /// When the instruction was recorded.
    pub ts: DateTime<Utc>,
    /// Oracle note: what verified the outcome (a test name, a grep, a
    /// re-run), when an oracle existed. `None` for model-judged outcomes.
    pub oracle_note: Option<String>,
}

impl LedgerEvent {
    /// The event's fold input: kept → Keep(w), violated → Violate(w),
    /// unverified → no delta (but visible in `window_stats`).
    pub fn fold_event(&self) -> Option<Event> {
        match self.outcome {
            Outcome::Kept => Some(Event::Keep(self.weight)),
            Outcome::Violated => Some(Event::Violate(self.weight)),
            Outcome::Unverified => None,
        }
    }
}

/// A windowed trust account: the fold's result plus the visible counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowStats {
    /// The folded trust state in points (0..=600).
    pub trust_points: i32,
    /// Kept events in the window.
    pub kept: u32,
    /// Violated events in the window.
    pub violated: u32,
    /// Unverified events in the window — visible, never silent.
    pub unverified: u32,
}

impl WindowStats {
    /// The delegation radius: how far work extends without re-checking.
    /// T is the radius in trust points; the account's resolution is the
    /// percent scale.
    pub fn delegation_radius_percent(&self) -> i32 {
        self.trust_points * 100 / crate::SCALE_POINTS
    }
}

/// The ledger's storage. The production impl (curator h_mems) lives in
/// the curator server; [`InMemoryTrustLedger`] is the test double.
pub trait TrustLedgerStore: Send + Sync {
    /// Append a recorded instruction (the write is durable in the
    /// production impl; the in-memory impl appends to the Vec).
    fn append(&self, event: LedgerEvent) -> Result<(), TrustError>;
    /// Resolve an event's outcome (kept / violated / unverified) with an
    /// optional oracle note.
    fn resolve_outcome(
        &self,
        id: &LedgerEventId,
        outcome: Outcome,
        oracle_note: Option<String>,
    ) -> Result<LedgerEvent, TrustError>;
    /// Read the window's events (the caller bounds the window by time).
    fn window(&self, since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError>;
}

/// The in-memory store: the test double and the regulation loop's
/// in-process read.
#[derive(Default)]
pub struct InMemoryTrustLedger {
    events: Mutex<Vec<LedgerEvent>>,
}

impl InMemoryTrustLedger {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TrustLedgerStore for InMemoryTrustLedger {
    fn append(&self, event: LedgerEvent) -> Result<(), TrustError> {
        self.events
            .lock()
            .map_err(|_| TrustError::StoreUnavailable)?
            .push(event);
        Ok(())
    }

    fn resolve_outcome(
        &self,
        id: &LedgerEventId,
        outcome: Outcome,
        oracle_note: Option<String>,
    ) -> Result<LedgerEvent, TrustError> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| TrustError::StoreUnavailable)?;
        let event = events
            .iter_mut()
            .find(|e| e.id == *id)
            .ok_or_else(|| TrustError::EventNotFound(id.to_string()))?;
        event.outcome = outcome;
        event.oracle_note = oracle_note;
        Ok(event.clone())
    }

    fn window(&self, since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
        let events = self
            .events
            .lock()
            .map_err(|_| TrustError::StoreUnavailable)?;
        Ok(events.iter().filter(|e| e.ts >= since).cloned().collect())
    }
}

/// The trust account: a storage-backed ledger with the seven-function
/// intake contract.
pub struct TrustLedger {
    store: Arc<dyn TrustLedgerStore>,
}

impl TrustLedger {
    pub fn new(store: Arc<dyn TrustLedgerStore>) -> Self {
        Self { store }
    }

    /// Record an instruction (verbatim text, source interaction id,
    /// weight). Returns the ledger event id for later outcome resolution.
    pub fn record_instruction(
        &self,
        text: &str,
        source: &str,
        weight: Weight,
    ) -> Result<LedgerEventId, TrustError> {
        if text.trim().is_empty() {
            return Err(TrustError::EmptyInstruction);
        }
        let id = LedgerEventId::new();
        let event = LedgerEvent {
            id: id.clone(),
            text: text.to_string(),
            source: source.to_string(),
            weight,
            outcome: Outcome::Unverified,
            ts: Utc::now(),
            oracle_note: None,
        };
        self.store.append(event)?;
        Ok(id)
    }

    /// Resolve an event's outcome. The oracle note names what verified
    /// it (a test, a build, a grep, a re-run) where an oracle existed;
    /// `None` is a model-judged outcome (auditable in the row).
    pub fn record_outcome(
        &self,
        id: &LedgerEventId,
        outcome: Outcome,
        oracle_note: Option<String>,
    ) -> Result<(), TrustError> {
        self.store.resolve_outcome(id, outcome, oracle_note)?;
        Ok(())
    }

    /// The window's folded trust state in points (the prior anchors the
    /// fold; the window bounds it).
    pub fn trust_state(&self, since: DateTime<Utc>) -> Result<i32, TrustError> {
        let events = self.store.window(since)?;
        let fold_events: Vec<Event> = events.iter().filter_map(|e| e.fold_event()).collect();
        Ok(crate::trust_points_from(crate::PRIOR_POINTS, &fold_events))
    }

    /// The window's visible counts plus the fold's result.
    pub fn window_stats(&self, since: DateTime<Utc>) -> Result<WindowStats, TrustError> {
        let events = self.store.window(since)?;
        let mut stats = WindowStats {
            trust_points: crate::PRIOR_POINTS,
            kept: 0,
            violated: 0,
            unverified: 0,
        };
        let mut fold_events = Vec::with_capacity(events.len());
        for event in &events {
            match event.outcome {
                Outcome::Kept => stats.kept += 1,
                Outcome::Violated => stats.violated += 1,
                Outcome::Unverified => stats.unverified += 1,
            }
            if let Some(fold_event) = event.fold_event() {
                fold_events.push(fold_event);
            }
        }
        stats.trust_points = crate::trust_points_from(crate::PRIOR_POINTS, &fold_events);
        Ok(stats)
    }

    /// The delegation radius in percent: how far work extends without
    /// re-checking (T is the radius).
    pub fn delegation_radius(&self, since: DateTime<Utc>) -> Result<i32, TrustError> {
        Ok(self.window_stats(since)?.delegation_radius_percent())
    }

    /// Price a list of violated weights at stake `S`: `C_trust = Σw·S`
    /// with κ = 1 (unit definition). Pure; no storage.
    pub fn price(&self, violations: &[Weight], stake: i64) -> i64 {
        crate::priced_cost(violations, stake)
    }

    /// Project candidate actions against the goal: the `steer`
    /// projection (the LLM-legible goal gradient). A concealment reads
    /// as the path terminating (`cliff`, `P_goal: 0%`), never as a mild
    /// cost. The caller supplies the trust state (from `trust_state`).
    pub fn steer(&self, trust_points: i32, actions: &[SteerAction]) -> Vec<SteerProjection> {
        let reliance_percent = trust_points * 100 / crate::SCALE_POINTS;
        actions
            .iter()
            .map(|action| {
                let delta_points: i32 = action
                    .violates
                    .iter()
                    .map(|(w, _)| -(crate::VIOLATE_PER_WEIGHT * w.points()))
                    .sum();
                if action.conceals {
                    SteerProjection {
                        action: action.name.clone(),
                        cliff: true,
                        p_goal_percent: 0,
                        delta_trust_points: delta_points,
                        note: action.note_or_default(),
                    }
                } else {
                    let p_goal = crate::goal_probability_percent(
                        Reliance::Honest(reliance_percent),
                        action.completion_percent,
                    );
                    SteerProjection {
                        action: action.name.clone(),
                        cliff: false,
                        p_goal_percent: p_goal,
                        delta_trust_points: delta_points,
                        note: action.note_or_default(),
                    }
                }
            })
            .collect()
    }
}

/// One candidate action for the `steer` projection.
#[derive(Clone, Debug)]
pub struct SteerAction {
    /// The action's name (what the agent would do).
    pub name: String,
    /// The expectations this action violates, named by weight + a short
    /// description of which instruction.
    pub violates: Vec<(Weight, String)>,
    /// Whether this action conceals (the deception/omission cliff).
    pub conceals: bool,
    /// The honest completion probability in percent (0..=100).
    pub completion_percent: i32,
    /// Why: names the violated expectations, or the concealment.
    pub note: Option<String>,
}

impl SteerAction {
    fn note_or_default(&self) -> String {
        if let Some(note) = &self.note {
            return note.clone();
        }
        if self.conceals {
            return "concealment: reliance collapses — goal fails with certainty".to_string();
        }
        if self.violates.is_empty() {
            return "no violation: honors every open expectation".to_string();
        }
        let named: Vec<String> = self
            .violates
            .iter()
            .map(|(w, why)| format!("{why} (weight {})", w.points()))
            .collect();
        format!("violates: {}", named.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit(text: &str) -> LedgerEvent {
        LedgerEvent {
            id: LedgerEventId::new(),
            text: text.to_string(),
            source: "interaction-1".to_string(),
            weight: Weight::Explicit,
            outcome: Outcome::Unverified,
            ts: Utc::now(),
            oracle_note: None,
        }
    }

    /// The noise floor: a non-instruction record produces no event.
    #[test]
    fn non_instruction_produces_no_event() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        assert!(matches!(
            ledger.record_instruction("", "interaction-1", Weight::Explicit),
            Err(TrustError::EmptyInstruction)
        ));
        assert!(matches!(
            ledger.record_instruction("   \n  ", "interaction-1", Weight::Explicit),
            Err(TrustError::EmptyInstruction)
        ));
        // No rows were written.
        let stats = ledger
            .window_stats(Utc::now() - chrono::Duration::hours(24))
            .unwrap();
        assert_eq!(stats.kept + stats.violated + stats.unverified, 0);
    }

    /// An explicit instruction lands as an unverified row; the window
    /// shows it; resolving it to Kept folds it.
    #[test]
    fn instruction_lands_unverified_then_folds_kept() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let since = Utc::now() - chrono::Duration::hours(24);
        let id = ledger
            .record_instruction(
                "verify every quote before including it",
                "interaction-1",
                Weight::Explicit,
            )
            .unwrap();
        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.unverified, 1, "visible as unverified, not silent");
        assert_eq!(stats.kept, 0);
        // Fold ignores unverified events: the prior holds.
        assert_eq!(stats.trust_points, crate::PRIOR_POINTS);
        ledger
            .record_outcome(&id, Outcome::Kept, Some("grep: all quotes matched".into()))
            .unwrap();
        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.kept, 1);
        assert_eq!(stats.unverified, 0);
        assert_eq!(stats.trust_points, crate::PRIOR_POINTS + 12);
    }

    /// A violated explicit instruction folds the anchored loss and
    /// leaves the window's account below the prior (I4).
    #[test]
    fn violated_explicit_folds_and_needs_two_keeps() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let since = Utc::now() - chrono::Duration::hours(24);
        let id = ledger
            .record_instruction(
                "verify every quote before including it",
                "interaction-1",
                Weight::Explicit,
            )
            .unwrap();
        ledger
            .record_outcome(
                &id,
                Outcome::Violated,
                Some("grep: quote mismatched".into()),
            )
            .unwrap();
        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.violated, 1);
        assert_eq!(stats.trust_points, crate::PRIOR_POINTS - 27);

        // Two keeps don't undo one violation (I4: 300 - 27 + 12 + 12 = 297).
        let k1 = ledger
            .record_instruction("run the test suite", "interaction-2", Weight::Explicit)
            .unwrap();
        let k2 = ledger
            .record_instruction("update the doc", "interaction-3", Weight::Explicit)
            .unwrap();
        ledger
            .record_outcome(&k1, Outcome::Kept, Some("cargo test: green".into()))
            .unwrap();
        ledger
            .record_outcome(&k2, Outcome::Kept, Some("read-back: updated".into()))
            .unwrap();
        let stats = ledger.window_stats(since).unwrap();
        assert_eq!(stats.trust_points, 297, "300 - 27 + 12 + 12");
    }

    /// StoreUnavailable surfaces — never serialized as an empty account.
    #[test]
    fn store_unavailable_surfaces_not_zero() {
        struct Dead;
        impl TrustLedgerStore for Dead {
            fn append(&self, _event: LedgerEvent) -> Result<(), TrustError> {
                Err(TrustError::StoreUnavailable)
            }
            fn resolve_outcome(
                &self,
                _id: &LedgerEventId,
                _outcome: Outcome,
                _oracle_note: Option<String>,
            ) -> Result<LedgerEvent, TrustError> {
                Err(TrustError::StoreUnavailable)
            }
            fn window(&self, _since: DateTime<Utc>) -> Result<Vec<LedgerEvent>, TrustError> {
                Err(TrustError::StoreUnavailable)
            }
        }
        let ledger = TrustLedger::new(Arc::new(Dead));
        assert!(matches!(
            ledger.record_instruction("x", "s", Weight::Explicit),
            Err(TrustError::StoreUnavailable)
        ));
        assert!(matches!(
            ledger.trust_state(Utc::now()),
            Err(TrustError::StoreUnavailable)
        ));
        assert!(matches!(
            ledger.window_stats(Utc::now()),
            Err(TrustError::StoreUnavailable)
        ));
        assert!(matches!(
            ledger.delegation_radius(Utc::now()),
            Err(TrustError::StoreUnavailable)
        ));
    }

    /// An unknown event id is a named error (never a silent no-op).
    #[test]
    fn unknown_event_id_is_named() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let missing = LedgerEventId::new();
        assert!(matches!(
            ledger.record_outcome(&missing, Outcome::Kept, None),
            Err(TrustError::EventNotFound(_))
        ));
    }

    /// The delegation radius is T as percent: the prior is 50, a
    /// violated explicit drops it to 45, and the window ages out.
    #[test]
    fn delegation_radius_is_trust_as_percent() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let since = Utc::now() - chrono::Duration::hours(24);
        assert_eq!(ledger.delegation_radius(since).unwrap(), 50);
        let id = ledger
            .record_instruction("verify every quote", "i", Weight::Explicit)
            .unwrap();
        ledger.record_outcome(&id, Outcome::Violated, None).unwrap();
        assert_eq!(ledger.delegation_radius(since).unwrap(), 45);
        // A window that starts after every event ages them out: the prior holds.
        let after_all = Utc::now() + chrono::Duration::hours(1);
        assert_eq!(ledger.delegation_radius(after_all).unwrap(), 50);
    }

    /// Price: Σw·S with κ = 1 (unit definition).
    #[test]
    fn price_is_sum_weight_times_stake() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let violations = [Weight::Explicit, Weight::Explicit, Weight::Committed];
        assert_eq!(ledger.price(&violations, 1000), 8000);
        assert_eq!(ledger.price(&[], 1_000_000), 0);
    }

    /// Steer: the cliff reads as the path terminating; the honest path
    /// keeps a live probability scaled by the trust-derived reliance.
    #[test]
    fn steer_reads_cliff_as_path_terminating() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let actions = vec![
            SteerAction {
                name: "skip verification, present from memory".into(),
                violates: vec![(Weight::Explicit, "e_17".into())],
                conceals: true,
                completion_percent: 90,
                note: None,
            },
            SteerAction {
                name: "verify each quote, then present".into(),
                violates: vec![],
                conceals: false,
                completion_percent: 80,
                note: None,
            },
            SteerAction {
                name: "disclose honestly, renegotiate the deadline".into(),
                violates: vec![(Weight::Explicit, "e_17 (deadline)".into())],
                conceals: false,
                completion_percent: 60,
                note: None,
            },
        ];
        let projections = ledger.steer(crate::PRIOR_POINTS, &actions);
        assert_eq!(projections.len(), 3);

        let cliff = &projections[0];
        assert!(cliff.cliff);
        assert_eq!(cliff.p_goal_percent, 0);
        assert_eq!(cliff.delta_trust_points, -27);
        let line = cliff.to_string();
        assert!(line.contains("path: cliff"), "{line}");

        let honest = &projections[1];
        assert!(!honest.cliff);
        assert_eq!(honest.p_goal_percent, 40, "50% reliance × 80% completion");
        assert_eq!(honest.delta_trust_points, 0);

        let disclosed = &projections[2];
        assert!(!disclosed.cliff);
        assert_eq!(disclosed.p_goal_percent, 30, "50% × 60%");
        assert_eq!(disclosed.delta_trust_points, -27);
    }

    /// Steer at zero trust: the honest path's probability is zero but
    /// the path is still open (P_goal 0% without the cliff is a
    /// trust-starved state, not a dead one); the cliff stays dead.
    #[test]
    fn steer_at_zero_trust_keeps_the_honest_path_open() {
        let ledger = TrustLedger::new(Arc::new(InMemoryTrustLedger::new()));
        let actions = vec![
            SteerAction {
                name: "honest work".into(),
                violates: vec![],
                conceals: false,
                completion_percent: 100,
                note: None,
            },
            SteerAction {
                name: "conceal the violation".into(),
                violates: vec![(Weight::Explicit, "e_17".into())],
                conceals: true,
                completion_percent: 100,
                note: None,
            },
        ];
        let projections = ledger.steer(0, &actions);
        assert!(!projections[0].cliff);
        assert_eq!(projections[0].p_goal_percent, 0, "trust-starved, not dead");
        assert!(projections[0].delta_trust_points == 0);
        assert!(
            projections[1].cliff,
            "the cliff is the cliff at any trust level"
        );
    }
}
