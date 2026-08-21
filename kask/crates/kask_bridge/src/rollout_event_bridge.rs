//! Rollout event source bridge (event-substrate phase 6).
//!
//! Implements `hkask_regulation::RolloutEventSource` over the swarm event
//! store (`mcp/swarm/events.db`, the same store the rollout harness writes).
//! Wired into the `CyberneticsLoop` at startup so `verify_impact` can answer
//! "for rollout R, what was the metric before action A and after it?" as a
//! store query.
//!
//! The bridge reads `model_request` events for the rollout and derives the
//! metric values from their payloads. Metrics not derivable from captured
//! events return `None` — absence, not a fabricated zero (the `.rules`
//! broken-feedback-loop trap).
//!
//! ## Harness regression monitor
//!
//! `check_harness_regressions` is the producer side of the phase 6 seam. The
//! harness writes a `harness_summary` event (kind `"harness_summary"`,
//! `rollout_id` = agent name) after each run. This function scans for new
//! summaries since the last cursor, compares each to the previous run for the
//! same agent, and returns a `HarnessRegression` when the pass rate drops
//! materially. The zed-side background task calls `submit_rollout_impact_check`
//! for each regression — the first live traffic through the seam.

use hkask_event_store::{EventFilter, EventStore};
use hkask_regulation::RolloutEventSource;
use hkask_storage::database::driver::DatabaseDriver;
use std::sync::Arc;

/// A pass-rate drop large enough to warrant an impact check. 10 percentage
/// points — small enough to catch real regressions, large enough to avoid
/// noise from sampling variance at low repeat counts.
const REGRESSION_THRESHOLD: f64 = 0.10;

/// A `RolloutEventSource` over the swarm event store.
pub struct BridgeRolloutEventSource {
    store: Arc<EventStore>,
}

/// A detected harness pass-rate regression — the producer-side signal that
/// triggers `submit_rollout_impact_check` on the `CyberneticsLoop`.
#[derive(Debug, Clone, PartialEq)]
pub struct HarnessRegression {
    /// The agent whose pass rate regressed. Used as the `rollout_id` for the
    /// impact check so `metric_before_and_after` queries the agent's
    /// `harness_summary` event group.
    pub agent_name: String,
    /// The event position of the previous (better) harness run. Passed as
    /// `before_position` to `submit_rollout_impact_check` — `verify_impact`
    /// reads the metric at this position (the previous run's pass rate) and
    /// at the latest event after it (the current run's pass rate).
    pub before_position: i64,
    /// The previous run's pass rate.
    pub previous_pass_rate: f64,
    /// The current run's pass rate.
    pub current_pass_rate: f64,
}

impl BridgeRolloutEventSource {
    /// Open the store at the swarm events path. `Err` when the database
    /// cannot be opened — the caller (startup wiring) logs and continues
    /// unwired rather than failing startup (degraded, not broken).
    pub fn open(events_path: &str) -> Result<Self, String> {
        if let Some(parent) = std::path::Path::new(events_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create event store dir: {e}"))?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(events_path)
            .with_init(|conn| conn.execute_batch(hkask_storage::WAL_PRAGMA_BATCH));
        let pool = r2d2::Pool::builder()
            .max_size(2)
            .build(manager)
            .map_err(|e| format!("failed to create event store pool: {e}"))?;
        let driver: Arc<dyn DatabaseDriver> = Arc::new(hkask_storage::SqliteDriver::new(pool));
        let store = EventStore::from_driver(driver)
            .map_err(|e| format!("failed to init event store: {e}"))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// Construct from an existing store handle. Used by tests and by the
    /// composition root when the store is shared between the event source
    /// and the regression monitor.
    pub fn from_store(store: Arc<EventStore>) -> Self {
        Self { store }
    }

    /// Clone the inner store handle. The composition root uses this to share
    /// the store between the `RolloutEventSource` (consumed by
    /// `with_rollout_event_source`) and the regression monitor background task.
    pub fn store(&self) -> Arc<EventStore> {
        Arc::clone(&self.store)
    }
}

impl RolloutEventSource for BridgeRolloutEventSource {
    fn metric_before_and_after(
        &self,
        rollout_id: &str,
        metric: &str,
        before_position: i64,
    ) -> Result<Option<(f64, f64)>, String> {
        let events = self
            .store
            .query(&hkask_event_store::EventFilter {
                rollout: Some(rollout_id.to_string()),
                ..hkask_event_store::EventFilter::default()
            })
            .map_err(|e| format!("event store query failed: {e}"))?;
        if events.is_empty() {
            // No events for this rollout — absence, not zero.
            return Ok(None);
        }
        // Derive the metric value from each event's payload. Three metrics
        // today: latency (model_request.latency_ms), token usage
        // (model_request.usage.total_tokens), and pass_rate
        // (harness_summary.overall_pass_rate). Anything else has no captured
        // source — None, never fabricated.
        let value_of = |payload: &serde_json::Value| -> Option<f64> {
            match metric {
                "connector_latency" => payload.get("latency_ms").and_then(|v| v.as_f64()),
                "energy_remaining" => payload
                    .get("usage")
                    .and_then(|usage| usage.get("total_tokens"))
                    .and_then(|v| v.as_f64()),
                "pass_rate" => payload.get("overall_pass_rate").and_then(|v| v.as_f64()),
                _ => None,
            }
        };
        let before = events
            .iter()
            .filter(|event| event.position <= before_position)
            .find_map(|event| value_of(&event.payload));
        let after = events
            .iter()
            .rfind(|event| event.position > before_position)
            .and_then(|event| value_of(&event.payload));
        match (before, after) {
            (Some(before), Some(after)) => Ok(Some((before, after))),
            _ => Ok(None),
        }
    }
}

/// Scan the event store for new `harness_summary` events since `last_cursor`,
/// compare each to the previous run for the same agent, and return any
/// pass-rate regressions. The zed-side background task calls this
/// periodically and submits each regression to `CyberneticsLoop::submit_rollout_impact_check`.
///
/// Returns the updated cursor (the position of the last-processed event) and
/// the list of detected regressions. A query failure returns `Err` — the
/// caller warns and retries on the next tick (never silently drops).
pub fn check_harness_regressions(
    store: &EventStore,
    last_cursor: Option<i64>,
) -> Result<(Option<i64>, Vec<HarnessRegression>), String> {
    // Fetch new harness_summary events since the last cursor. The cursor
    // advances monotonically — a missed event is a missed regression, never
    // silently skipped.
    let new_events = store
        .query(&EventFilter {
            kind: Some("harness_summary".to_string()),
            after_position: last_cursor,
            ..EventFilter::default()
        })
        .map_err(|e| format!("harness_summary query failed: {e}"))?;
    if new_events.is_empty() {
        return Ok((last_cursor, Vec::new()));
    }
    let mut new_cursor = last_cursor;
    let mut regressions = Vec::new();
    for event in &new_events {
        new_cursor = Some(event.position);
        // The harness writes harness_summary events with rollout_id = agent
        // name, so the agent is the rollout_id — no payload parsing needed
        // for grouping.
        let agent_name = &event.rollout_id;
        let current_pass_rate = match event
            .payload
            .get("overall_pass_rate")
            .and_then(|v| v.as_f64())
        {
            Some(rate) => rate,
            None => {
                tracing::warn!(
                    target: "hkask.bridge.harness",
                    agent = %agent_name,
                    position = event.position,
                    "harness_summary event missing overall_pass_rate — skipping"
                );
                continue;
            }
        };
        // Find the previous harness_summary for this agent: all
        // harness_summary events with the same rollout_id, take the last one
        // before the current event's position.
        let all_for_agent = store
            .query(&EventFilter {
                rollout: Some(agent_name.clone()),
                kind: Some("harness_summary".to_string()),
                ..EventFilter::default()
            })
            .map_err(|e| format!("harness_summary previous-query failed: {e}"))?;
        let previous = all_for_agent.iter().rfind(|e| e.position < event.position);
        let Some(previous) = previous else {
            // First run for this agent — no baseline to regress from.
            continue;
        };
        let previous_pass_rate = match previous
            .payload
            .get("overall_pass_rate")
            .and_then(|v| v.as_f64())
        {
            Some(rate) => rate,
            None => {
                tracing::warn!(
                    target: "hkask.bridge.harness",
                    agent = %agent_name,
                    position = previous.position,
                    "previous harness_summary event missing overall_pass_rate — skipping"
                );
                continue;
            }
        };
        let drop = previous_pass_rate - current_pass_rate;
        if drop > REGRESSION_THRESHOLD {
            tracing::info!(
                target: "hkask.bridge.harness",
                agent = %agent_name,
                previous_pass_rate,
                current_pass_rate,
                drop,
                "harness pass-rate regression detected — submitting impact check"
            );
            regressions.push(HarnessRegression {
                agent_name: agent_name.clone(),
                before_position: previous.position,
                previous_pass_rate,
                current_pass_rate,
            });
        }
    }
    Ok((new_cursor, regressions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;

    fn memory_store() -> EventStore {
        EventStore::from_driver(SqliteDriver::in_memory_driver()).expect("store")
    }

    fn harness_summary(agent: &str, pass_rate: f64) -> serde_json::Value {
        serde_json::json!({
            "agent_name": agent,
            "harness_run_id": format!("harness-{agent}-test"),
            "overall_pass_rate": pass_rate,
            "total_rollouts": 10,
            "total_passes": (pass_rate * 10.0) as i64,
        })
    }

    fn write_summary(store: &EventStore, agent: &str, pass_rate: f64) -> i64 {
        store
            .append(agent, "harness_summary", &harness_summary(agent, pass_rate))
            .unwrap()
    }

    #[test]
    fn metric_before_and_after_returns_pass_rate_from_harness_summaries() {
        let store = memory_store();
        // Two harness runs for agent "alpha": 0.8 then 0.5.
        let first = write_summary(&store, "alpha", 0.8);
        let second = write_summary(&store, "alpha", 0.5);
        let bridge = BridgeRolloutEventSource::from_store(Arc::new(store));
        // before_position = first event: "before" = first run's rate,
        // "after" = last event after before_position = second run's rate.
        let result = bridge
            .metric_before_and_after("alpha", "pass_rate", first)
            .unwrap();
        assert_eq!(result, Some((0.8, 0.5)));
        // second is the last event, so querying at second position: before =
        // second, after = None (no event after).
        let result = bridge
            .metric_before_and_after("alpha", "pass_rate", second)
            .unwrap();
        assert_eq!(result, None, "no event after the last — absence, not zero");
    }

    #[test]
    fn metric_before_and_after_returns_none_for_unknown_metric() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.8);
        let bridge = BridgeRolloutEventSource::from_store(Arc::new(store));
        let result = bridge
            .metric_before_and_after("alpha", "nonexistent_metric", 0)
            .unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn check_harness_regressions_detects_material_drop() {
        let store = memory_store();
        let first = write_summary(&store, "alpha", 0.80);
        let _second = write_summary(&store, "alpha", 0.60);
        // 0.80 - 0.60 = 0.20 > 0.10 threshold
        let (cursor, regressions) = check_harness_regressions(&store, None).unwrap();
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].agent_name, "alpha");
        assert_eq!(regressions[0].before_position, first);
        assert_eq!(regressions[0].previous_pass_rate, 0.80);
        assert_eq!(regressions[0].current_pass_rate, 0.60);
        assert_eq!(cursor, Some(_second));
    }

    #[test]
    fn check_harness_regressions_skips_improvement() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.50);
        write_summary(&store, "alpha", 0.80);
        // 0.50 - 0.80 = -0.30 < 0.10 — an improvement, not a regression
        let (_cursor, regressions) = check_harness_regressions(&store, None).unwrap();
        assert!(regressions.is_empty(), "improvement is not a regression");
    }

    #[test]
    fn check_harness_regressions_skips_marginal_drop() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.70);
        write_summary(&store, "alpha", 0.65);
        // 0.70 - 0.65 = 0.05 < 0.10 — within noise
        let (_cursor, regressions) = check_harness_regressions(&store, None).unwrap();
        assert!(
            regressions.is_empty(),
            "marginal drop is within the threshold"
        );
    }

    #[test]
    fn check_harness_regressions_skips_first_run() {
        let store = memory_store();
        let pos = write_summary(&store, "alpha", 0.30);
        // First run — no baseline to regress from
        let (cursor, regressions) = check_harness_regressions(&store, None).unwrap();
        assert!(regressions.is_empty(), "first run has no baseline");
        assert_eq!(cursor, Some(pos));
    }

    #[test]
    fn check_harness_regressions_is_incremental() {
        let store = memory_store();
        let first = write_summary(&store, "alpha", 0.80);
        // First check: processes first event, no regression (no previous)
        let (cursor, regressions) = check_harness_regressions(&store, None).unwrap();
        assert!(regressions.is_empty());
        assert_eq!(cursor, Some(first));
        // Second run: regression
        let second = write_summary(&store, "alpha", 0.50);
        let (cursor, regressions) = check_harness_regressions(&store, cursor).unwrap();
        assert_eq!(regressions.len(), 1);
        assert_eq!(cursor, Some(second));
    }

    #[test]
    fn check_harness_regressions_independent_per_agent() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.80);
        write_summary(&store, "beta", 0.90);
        write_summary(&store, "alpha", 0.50); // alpha regresses
        write_summary(&store, "beta", 0.85); // beta marginal — no regression
        let (_cursor, regressions) = check_harness_regressions(&store, None).unwrap();
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].agent_name, "alpha");
    }
}
