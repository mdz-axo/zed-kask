#![forbid(unsafe_code)]
//! # hKask Event Store — append-only rollout event log
//!
//! The data plane of the event-substrate proposal (Agent Lightning's store,
//! ported). One table, two well-known event kinds, opaque pass-through for
//! everything else. Position in the log is identity — there is no separate
//! event ID.
//!
//! ## Well-known kinds
//!
//! - `model_request` — one inference call inside a rollout. Payload carries
//!   model, response status, latency, usage, finish reason. Captured
//!   automatically at the inference boundary; the store does not parse it.
//! - `verdict` — a judge's report on a rollout. Payload carries value,
//!   source (`deterministic_evaluator` | `operator` | `regulation_impact`),
//!   and reason.
//!
//! Everything else (skill spans, tool stats) is stored as an opaque kind +
//! JSON payload. The store does not parse what it doesn't need to.
//!
//! ## Interface budget
//!
//! Four functions: `append`, `query`, `compact`, `cursor`. If the interface
//! grows past this, the design is wrong (the deletion test applied up front).
//!
//! ## Retention
//!
//! Terminal rollouts compact to summaries; `compact` drops events for
//! rollouts that ended before a cutoff and records the count it dropped —
//! a drop is never silent (absence must be distinguishable from zero).

mod types;

pub use types::{EventFilter, RolloutKind, VerdictSource};
pub use types::{EventRecord, EventStoreError};

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::value::DbValue;
use hkask_types::time::now_rfc3339_z;
use std::sync::Arc;

/// Store backed by a provider-agnostic `DatabaseDriver`, with an injectable
/// clock for testable retention boundaries.
///
/// The `clock` field is the seam: production construction wires the real
/// `hkask_types::time::now_rfc3339_z` clock; tests inject a controllable clock
/// so retention cutoffs (`compact`, `strip_bodies`) can be exercised at exact
/// boundary instants without sleeping. The clock returns a `Z`-suffixed
/// RFC3339 string so stored `created_at` values sort lexically against
/// `Z`-suffixed cutoffs (see `now_rfc3339_z`).
#[derive(Clone)]
pub struct EventStore {
    driver: Arc<dyn DatabaseDriver>,
    clock: fn() -> String,
}

impl EventStore {
    /// Create a store backed by the given driver, using the real clock.
    ///
    /// Calls `Self::init_schema` for idempotent schema setup and propagates
    /// any schema-init failure rather than proceeding with a missing table.
    pub fn from_driver(driver: Arc<dyn DatabaseDriver>) -> Result<Self, EventStoreError> {
        Self::from_driver_with_clock(driver, now_rfc3339_z)
    }

    /// Create a store backed by the given driver with an injected clock.
    ///
    /// The clock fn returns the `created_at` timestamp assigned to each
    /// appended event. Tests inject a controllable clock to place events at
    /// exact instants relative to a retention cutoff.
    pub fn from_driver_with_clock(
        driver: Arc<dyn DatabaseDriver>,
        clock: fn() -> String,
    ) -> Result<Self, EventStoreError> {
        Self::init_schema(&driver)?;
        Ok(Self { driver, clock })
    }

    /// Access the underlying driver for direct queries.
    pub fn driver(&self) -> &Arc<dyn DatabaseDriver> {
        &self.driver
    }

    /// Initialize the event schema. Idempotent — safe on an existing
    /// database. Called by both constructors.
    fn init_schema(driver: &Arc<dyn DatabaseDriver>) -> Result<(), EventStoreError> {
        driver.execute_batch(SCHEMA_DDL)?;
        Ok(())
    }

    /// Append one event. The position (rowid) is assigned by the log and
    /// returned — it is the event's only identity.
    pub fn append(
        &self,
        rollout: &str,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<i64, EventStoreError> {
        if rollout.trim().is_empty() {
            return Err(EventStoreError::EmptyRolloutId);
        }
        if kind.trim().is_empty() {
            return Err(EventStoreError::EmptyKind);
        }
        let now = (self.clock)();
        // INSERT ... RETURNING executes insert + position read as ONE
        // statement on ONE connection. The prior INSERT-then-SELECT-MAX pair
        // raced under concurrent writers (the capture drainer and the
        // harness loop append concurrently by design, and each driver call
        // may take a different pooled connection): writer A could receive
        // writer B's position, violating position-is-identity.
        let row = self.driver.query_optional(
            "INSERT INTO events (rollout_id, kind, payload, created_at) \
             VALUES (?1, ?2, ?3, ?4) RETURNING position",
            &[
                DbValue::Text(rollout.to_string()),
                DbValue::Text(kind.to_string()),
                DbValue::Text(payload.to_string()),
                DbValue::Text(now),
            ],
        )?;
        // Propagate the get_int failure instead of silently coercing it to
        // 0 (`unwrap_or(0)`): a column-read error must surface, not be read
        // as position 0. NoPosition covers the case where INSERT succeeded
        // but RETURNING yielded no row.
        row.map(|r| r.get_int(0))
            .transpose()
            .map_err(EventStoreError::from)?
            .ok_or(EventStoreError::NoPosition)
    }

    /// Query events, newest-position last. Filters compose: a `None` filter
    /// field matches everything.
    pub fn query(&self, filter: &EventFilter) -> Result<Vec<EventRecord>, EventStoreError> {
        let mut sql = String::from(
            "SELECT position, rollout_id, kind, payload, created_at FROM events WHERE 1=1",
        );
        let mut params: Vec<DbValue> = Vec::new();
        if let Some(rollout) = &filter.rollout {
            params.push(DbValue::Text(rollout.clone()));
            sql.push_str(&format!(" AND rollout_id = ?{}", params.len()));
        }
        if let Some(kind) = &filter.kind {
            params.push(DbValue::Text(kind.clone()));
            sql.push_str(&format!(" AND kind = ?{}", params.len()));
        }
        if let Some(after_position) = filter.after_position {
            params.push(DbValue::Integer(after_position));
            sql.push_str(&format!(" AND position > ?{}", params.len()));
        }
        let mut limit_clause = String::new();
        if let Some(limit) = filter.limit {
            params.push(DbValue::Integer(limit as i64));
            limit_clause = format!(" LIMIT ?{}", params.len());
        }
        sql.push_str(" ORDER BY position ASC");
        sql.push_str(&limit_clause);
        let rows = self.driver.query(&sql, &params)?;
        rows.iter()
            .map(|row| {
                Ok(EventRecord {
                    position: row.get_int(0)?,
                    rollout_id: row.get_str(1)?.to_string(),
                    kind: row.get_str(2)?.to_string(),
                    // A stored payload that fails to parse is corruption, not
                    // a silently-nullable field — surface it rather than
                    // coercing to Null (which would hide the bad row).
                    payload: serde_json::from_str(row.get_str(3)?)?,
                    created_at: row.get_str(4)?.to_string(),
                })
            })
            .collect()
    }

    /// Compact: drop events for rollouts whose last event predates the
    /// cutoff, recording how many events were dropped. Returns the dropped
    /// count — callers must surface it, never swallow it (a silent drop is
    /// indistinguishable from "nothing happened").
    pub fn compact(&self, cutoff_rfc3339: &str) -> Result<usize, EventStoreError> {
        let dropped = self.driver.execute(
            "DELETE FROM events WHERE rollout_id IN (
                SELECT rollout_id FROM events
                GROUP BY rollout_id
                HAVING MAX(created_at) < ?1
            )",
            &[DbValue::Text(cutoff_rfc3339.to_string())],
        )?;
        Ok(dropped)
    }

    /// Strip the request/response bodies from `model_request` events older
    /// than the cutoff — the summary-compaction half of retention. Terminal
    /// rollouts keep their shape (model, latency, usage, verdict) but lose
    /// their bulk; the training bridge has already consumed the bodies it
    /// will consume by then. Returns the number of events stripped —
    /// surfaced, never silent.
    ///
    /// SQLite's `json_remove` updates the payload in place; events whose
    /// payload lacks the keys are unaffected (0 rows changed for them).
    pub fn strip_bodies(&self, cutoff_rfc3339: &str) -> Result<usize, EventStoreError> {
        let stripped = self.driver.execute(
            "UPDATE events SET payload = json_remove(payload, '$.request_body', '$.response_body') \
             WHERE kind = 'model_request' AND created_at < ?1 \
             AND json_type(payload, '$.request_body') IS NOT NULL",
            &[DbValue::Text(cutoff_rfc3339.to_string())],
        )?;
        Ok(stripped)
    }

    /// The current log position — the resume point for incremental readers.
    /// `None` on an empty log (absence, not zero: nothing has happened yet).
    pub fn cursor(&self) -> Result<Option<i64>, EventStoreError> {
        let row = self
            .driver
            .query_optional("SELECT MAX(position) FROM events", &[])?;
        // `SELECT MAX(position)` is an aggregate: it always returns exactly one
        // row, with a NULL column when the log is empty. So "empty log" surfaces
        // as `Some(row)` with a `Null` column, not as `None` from
        // `query_optional`. Treat `Null` as `Ok(None)` (the empty-log signal)
        // but propagate any other type mismatch as `Err(Database(...))`. The
        // prior `Ok(row.and_then(|r| r.get_int(0).ok()))` silently coerced a
        // real column-read error to "empty log" (`None`) — a broken feedback
        // loop — so this distinguishes the three cases explicitly.
        match row {
            None => Ok(None),
            Some(r) => match r.get(0)? {
                DbValue::Null => Ok(None),
                value => Ok(Some(value.as_int()?)),
            },
        }
    }
}

/// The event log schema. One table; `position` (rowid) is the identity.
const SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS events (
    position INTEGER PRIMARY KEY AUTOINCREMENT,
    rollout_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_rollout ON events(rollout_id, position);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind, position);";

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;

    fn memory_store() -> EventStore {
        EventStore::from_driver(SqliteDriver::in_memory_driver()).expect("store")
    }

    fn model_request_event(model: &str) -> serde_json::Value {
        serde_json::json!({
            "model": model,
            "status": "ok",
            "latency_ms": 120,
            "usage": { "total_tokens": 87 },
            "finish_reason": "stop",
        })
    }

    #[test]
    fn append_assigns_monotonic_positions() {
        let store = memory_store();
        let first = store
            .append("rollout-1", "model_request", &model_request_event("qwen"))
            .unwrap();
        let second = store
            .append("rollout-1", "verdict", &serde_json::json!({"pass": true}))
            .unwrap();
        assert!(second > first, "positions must be monotonic");
    }

    #[test]
    fn append_rejects_empty_rollout_and_kind() {
        let store = memory_store();
        assert!(matches!(
            store.append("", "model_request", &serde_json::json!({})),
            Err(EventStoreError::EmptyRolloutId)
        ));
        assert!(matches!(
            store.append("r", "", &serde_json::json!({})),
            Err(EventStoreError::EmptyKind)
        ));
    }

    #[test]
    fn query_filters_by_rollout_kind_and_position() {
        let store = memory_store();
        store
            .append("r1", "model_request", &model_request_event("a"))
            .unwrap();
        let mid = store
            .append("r1", "verdict", &serde_json::json!({"pass": true}))
            .unwrap();
        store
            .append("r2", "model_request", &model_request_event("b"))
            .unwrap();

        let r1 = store
            .query(&EventFilter {
                rollout: Some("r1".into()),
                ..EventFilter::default()
            })
            .unwrap();
        assert_eq!(r1.len(), 2);
        assert_eq!(r1[0].kind, "model_request");
        assert_eq!(r1[0].payload["model"], "a");

        let verdicts = store
            .query(&EventFilter {
                kind: Some("verdict".into()),
                ..EventFilter::default()
            })
            .unwrap();
        assert_eq!(verdicts.len(), 1);

        let after = store
            .query(&EventFilter {
                after_position: Some(mid),
                ..EventFilter::default()
            })
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].rollout_id, "r2");

        let limited = store
            .query(&EventFilter {
                limit: Some(2),
                ..EventFilter::default()
            })
            .unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn cursor_is_none_on_empty_and_some_after_append() {
        let store = memory_store();
        assert_eq!(store.cursor().unwrap(), None);
        store
            .append("r", "model_request", &model_request_event("a"))
            .unwrap();
        assert!(store.cursor().unwrap().is_some());
    }

    #[test]
    fn compact_drops_old_rollouts_and_counts() {
        let store = memory_store();
        store
            .append("old", "model_request", &model_request_event("a"))
            .unwrap();
        store
            .append("new", "model_request", &model_request_event("b"))
            .unwrap();
        // Cutoff between the two: RFC3339 timestamps sort lexically.
        // "old" events were written before "now - 0s"; use a far-future
        // cutoff for "new" by compacting with a cutoff after everything,
        // then verify the count semantics on a fresh store instead.
        let dropped = store.compact("9999-01-01T00:00:00Z").unwrap();
        assert_eq!(dropped, 2, "far-future cutoff drops everything");
        assert_eq!(store.cursor().unwrap(), None);
    }

    #[test]
    fn compact_keeps_recent_rollouts() {
        let store = memory_store();
        store
            .append("r", "model_request", &model_request_event("a"))
            .unwrap();
        // A cutoff in the past keeps everything.
        let dropped = store.compact("2000-01-01T00:00:00Z").unwrap();
        assert_eq!(dropped, 0);
        assert!(store.cursor().unwrap().is_some());
    }

    #[test]
    fn opaque_kinds_pass_through_unparsed() {
        let store = memory_store();
        let payload = serde_json::json!({"span": "reg.bughunt.probe", "arbitrary": [1, 2, 3]});
        store.append("r", "skill_span", &payload).unwrap();
        let events = store
            .query(&EventFilter {
                kind: Some("skill_span".into()),
                ..EventFilter::default()
            })
            .unwrap();
        assert_eq!(events[0].payload, payload, "payload round-trips verbatim");
    }

    #[test]
    fn concurrent_appends_receive_distinct_positions() {
        // The founding contract: position is identity. Two concurrent
        // appenders (the capture drainer and the harness verdict loop run
        // concurrently in production) must never receive the same position.
        // The store is Send + Sync via the driver, so spawn real threads.
        use std::sync::Arc;
        let store = Arc::new(memory_store());
        let mut handles = Vec::new();
        for thread_index in 0..4 {
            let store = Arc::clone(&store);
            handles.push(std::thread::spawn(move || {
                let mut positions = Vec::new();
                for i in 0..25 {
                    positions.push(
                        store
                            .append(
                                &format!("rollout-{thread_index}"),
                                "model_request",
                                &serde_json::json!({"i": i}),
                            )
                            .unwrap(),
                    );
                }
                positions
            }));
        }
        let mut all = Vec::new();
        for handle in handles {
            all.extend(handle.join().expect("thread must not panic"));
        }
        let total = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(
            all.len(),
            total,
            "every append must return a DISTINCT position — position is identity"
        );
        // And they must be contiguous 1..=total (AUTOINCREMENT from empty).
        let expected: Vec<i64> = (1..=total as i64).collect();
        assert_eq!(all, expected, "positions must be contiguous");
    }

    #[test]
    fn strip_bodies_removes_only_old_model_requests() {
        let store = memory_store();
        let old = serde_json::json!({
            "model": "m", "request_body": "the task", "response_body": "the answer"
        });
        let recent = serde_json::json!({
            "model": "m", "request_body": "keep me", "response_body": "keep me too"
        });
        let verdict = serde_json::json!({"pass": true});
        store.append("r-old", "model_request", &old).unwrap();
        store.append("r-old", "verdict", &verdict).unwrap();
        store.append("r-recent", "model_request", &recent).unwrap();

        // "now" is after everything written — all model_requests are old.
        let stripped = store.strip_bodies("9999-01-01T00:00:00Z").unwrap();
        assert_eq!(stripped, 2, "both model_request events are stripped");

        let events = store.query(&EventFilter::default()).unwrap();
        for event in &events {
            if event.kind == "model_request" {
                assert!(
                    event.payload.get("request_body").is_none(),
                    "bodies must be gone after strip"
                );
                assert!(event.payload.get("response_body").is_none());
                // Shape survives.
                assert_eq!(event.payload.get("model").unwrap(), "m");
            }
            if event.kind == "verdict" {
                // Non-model_request kinds are untouched.
                assert_eq!(event.payload.get("pass").unwrap(), &serde_json::json!(true));
            }
        }
    }

    #[test]
    fn strip_bodies_is_idempotent() {
        let store = memory_store();
        let body = serde_json::json!({"request_body": "x", "response_body": "y"});
        store.append("r", "model_request", &body).unwrap();
        assert_eq!(store.strip_bodies("9999-01-01T00:00:00Z").unwrap(), 1);
        // Second pass finds nothing to strip — the guard on json_type
        // prevents a no-op rewrite from counting.
        assert_eq!(store.strip_bodies("9999-01-01T00:00:00Z").unwrap(), 0);
    }

    // ── VerdictSource / RolloutKind wire round-trips ──────────────────

    #[test]
    fn verdict_source_as_str_round_trips() {
        for variant in [
            VerdictSource::DeterministicEvaluator,
            VerdictSource::Operator,
            VerdictSource::LlmJudged,
            VerdictSource::RegulationImpact,
        ] {
            let s = variant.as_str();
            assert_eq!(VerdictSource::from_str(s), Some(variant));
        }
        // Unknown string returns None, not a fabricated default.
        assert_eq!(VerdictSource::from_str("bogus"), None);
    }

    #[test]
    fn verdict_source_trust_classification() {
        assert!(VerdictSource::DeterministicEvaluator.is_trusted_for_task_success());
        assert!(VerdictSource::Operator.is_trusted_for_task_success());
        assert!(!VerdictSource::LlmJudged.is_trusted_for_task_success());
        assert!(!VerdictSource::RegulationImpact.is_trusted_for_task_success());
    }

    #[test]
    fn rollout_kind_as_str_round_trips() {
        for variant in [
            RolloutKind::Delegation,
            RolloutKind::Turn,
            RolloutKind::HarnessRun,
        ] {
            let s = variant.as_str();
            assert_eq!(RolloutKind::from_str(s), Some(variant));
        }
        assert_eq!(RolloutKind::from_str("bogus"), None);
    }

    #[test]
    fn verdict_event_carries_typed_source_in_payload() {
        // The store does not parse payloads, but the verdict event's
        // `source` field must be a `VerdictSource` wire string so consumers
        // can parse it back — not a hardcoded string that could drift.
        let store = memory_store();
        let payload = serde_json::json!({
            "pass": true,
            "source": VerdictSource::DeterministicEvaluator.as_str(),
            "rollout_kind": RolloutKind::Delegation.as_str(),
        });
        store.append("r1", "verdict", &payload).unwrap();
        let events = store
            .query(&EventFilter {
                kind: Some("verdict".into()),
                ..EventFilter::default()
            })
            .unwrap();
        let source_str = events[0].payload["source"].as_str().unwrap();
        assert_eq!(
            VerdictSource::from_str(source_str),
            Some(VerdictSource::DeterministicEvaluator)
        );
        let kind_str = events[0].payload["rollout_kind"].as_str().unwrap();
        assert_eq!(
            RolloutKind::from_str(kind_str),
            Some(RolloutKind::Delegation)
        );
    }

    // ── Clock seam + retention-boundary tests ────────────────────────────
    //
    // `fn() -> String` cannot capture mutable state, so the controllable
    // test clock reads a process-global `Mutex<String>`. Tests that need to
    // advance time between appends set the clock before each `append`. The
    // static is process-global, so these tests must not run concurrently with
    // each other — `--test-threads=1` is not required because each test resets
    // the clock to a fixed value before its first append and never reads the
    // clock outside its own appends, but the tests below are written to be
    // independent of one another's timing.

    static TEST_CLOCK: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    // Clock-based tests read/write the shared `TEST_CLOCK`; without serialization
    // one test's `set_test_clock` could land between another test's appends and
    // corrupt its boundary. This guard serializes only the clock-based tests —
    // the non-clock tests still run in parallel.
    static CLOCK_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn test_driver() -> Arc<dyn DatabaseDriver> {
        SqliteDriver::in_memory_driver()
    }

    fn test_clock() -> String {
        TEST_CLOCK
            .lock()
            .expect("test clock mutex poisoned")
            .clone()
    }

    fn set_test_clock(timestamp: &str) {
        *TEST_CLOCK.lock().expect("test clock mutex poisoned") = timestamp.to_string();
    }

    #[test]
    fn compact_drops_old_rollouts_keeps_recent_at_boundary() {
        let _guard = CLOCK_TEST_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Two rollouts written at distinct instants; the cutoff falls exactly
        // between them. The old rollout (last event at t1) must be dropped; the
        // new one (last event at t2) must survive. This pins the boundary:
        // `MAX(created_at) < cutoff` is strictly less-than, so an event written
        // exactly at the cutoff would survive (not tested here, but the
        // boundary is at the midpoint, not at an event instant).
        let store = EventStore::from_driver_with_clock(test_driver(), test_clock).expect("store");
        set_test_clock("2026-08-20T10:00:00.000Z");
        store
            .append("old", "model_request", &model_request_event("a"))
            .unwrap();
        set_test_clock("2026-08-20T12:00:00.000Z");
        store
            .append("new", "model_request", &model_request_event("b"))
            .unwrap();
        // Cutoff between t1 (10:00) and t2 (12:00).
        let dropped = store.compact("2026-08-20T11:00:00.000Z").unwrap();
        assert_eq!(
            dropped, 1,
            "old rollout (last event at 10:00) should be dropped"
        );
        let events = store.query(&EventFilter::default()).unwrap();
        assert!(
            events.iter().any(|e| e.rollout_id == "new"),
            "new rollout survives"
        );
        assert!(
            !events.iter().any(|e| e.rollout_id == "old"),
            "old rollout is gone"
        );
    }

    #[test]
    fn strip_bodies_strips_old_keeps_recent_at_boundary() {
        let _guard = CLOCK_TEST_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // One model_request before the cutoff (stripped), one after (kept).
        // The boundary is the cutoff itself; `created_at < cutoff` is strict.
        let store = EventStore::from_driver_with_clock(test_driver(), test_clock).expect("store");
        set_test_clock("2026-08-20T09:00:00.000Z");
        store
            .append(
                "r-old",
                "model_request",
                &serde_json::json!({
                    "model": "m", "request_body": "old task", "response_body": "old answer"
                }),
            )
            .unwrap();
        set_test_clock("2026-08-20T13:00:00.000Z");
        store
            .append(
                "r-recent",
                "model_request",
                &serde_json::json!({
                    "model": "m", "request_body": "keep me", "response_body": "keep me too"
                }),
            )
            .unwrap();
        let stripped = store.strip_bodies("2026-08-20T11:00:00.000Z").unwrap();
        assert_eq!(stripped, 1, "only the pre-cutoff model_request is stripped");

        let events = store.query(&EventFilter::default()).unwrap();
        let old_event = events
            .iter()
            .find(|e| e.rollout_id == "r-old")
            .expect("old event present");
        assert!(
            old_event.payload.get("request_body").is_none(),
            "old bodies stripped"
        );
        assert!(old_event.payload.get("response_body").is_none());
        let recent_event = events
            .iter()
            .find(|e| e.rollout_id == "r-recent")
            .expect("recent event present");
        assert_eq!(
            recent_event.payload.get("request_body").unwrap(),
            "keep me",
            "recent bodies survive"
        );
        assert_eq!(
            recent_event.payload.get("response_body").unwrap(),
            "keep me too"
        );
    }

    #[test]
    fn query_surfaces_corrupt_payload_as_error_not_null() {
        let _guard = CLOCK_TEST_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A row whose payload column is not valid JSON is corruption, not a
        // nullable field. `query` must return Err(PayloadParse(...)) rather
        // than silently coercing the bad payload to `Value::Null` (which would
        // hide the corrupted row behind a plausible-looking event).
        let store = EventStore::from_driver_with_clock(test_driver(), test_clock).expect("store");
        set_test_clock("2026-08-20T10:00:00.000Z");
        store
            .append("r", "model_request", &model_request_event("a"))
            .unwrap();
        // Inject a corrupt row directly, bypassing `append`'s serialization.
        store
            .driver()
            .execute(
                "INSERT INTO events (rollout_id, kind, payload, created_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                &[
                    DbValue::Text("r-bad".to_string()),
                    DbValue::Text("model_request".to_string()),
                    DbValue::Text("this is not json".to_string()),
                    DbValue::Text("2026-08-20T10:00:00.000Z".to_string()),
                ],
            )
            .unwrap();
        let result = store.query(&EventFilter::default());
        assert!(
            matches!(result, Err(EventStoreError::PayloadParse(_))),
            "corrupt payload must surface as PayloadParse, not silently coerce to Null; got {result:?}"
        );
    }

    #[test]
    fn cursor_is_none_on_empty_then_some_with_exact_max_after_appends() {
        let _guard = CLOCK_TEST_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // `cursor` returns Ok(None) on an empty log and Ok(Some(max_position))
        // after appends — with the exact max, not just "some value". The
        // column-read path must not silently coerce a read failure to None.
        let store = EventStore::from_driver_with_clock(test_driver(), test_clock).expect("store");
        set_test_clock("2026-08-20T10:00:00.000Z");
        assert_eq!(store.cursor().unwrap(), None, "empty log cursor is None");
        let first = store
            .append("r1", "model_request", &model_request_event("a"))
            .unwrap();
        let second = store
            .append("r2", "verdict", &serde_json::json!({"pass": true}))
            .unwrap();
        assert_eq!(store.cursor().unwrap(), Some(second));
        assert!(second > first, "sanity: positions are monotonic");
    }
}
