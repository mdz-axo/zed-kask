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

pub use types::{EventFilter, EventRecord, EventStoreError, RolloutKind, VerdictSource};

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::value::DbValue;
use hkask_storage::define_driver_store;
use std::sync::Arc;

define_driver_store!(EventStore, EventStoreError);

impl EventStore {
    /// Initialize the event schema. Idempotent — safe on an existing
    /// database. Called by the macro-generated `from_driver`.
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
        let now = chrono::Utc::now().to_rfc3339();
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
        row.map(|r| r.get_int(0).unwrap_or(0))
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
                    payload: serde_json::from_str(row.get_str(3)?)
                        .unwrap_or(serde_json::Value::Null),
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
        Ok(row.and_then(|r| r.get_int(0).ok()))
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
}
