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
//! `HarnessRegressionMonitor` is the producer side of the phase 6 seam. The
//! harness writes a `harness_summary` event (kind `"harness_summary"`,
//! `rollout_id` = agent name) after each run. The monitor scans new summaries,
//! compares each to the previous run for the same agent, and offers a rollout
//! impact check when the pass rate drops materially. It acknowledges summaries
//! only through the contiguous prefix whose required checks were accepted.

use hkask_event_store::{EventFilter, EventStore};
use hkask_regulation::{
    CyberneticsLoop, RolloutEventError, RolloutEventSource, RolloutImpactSubmission,
};
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
struct HarnessRegression {
    /// The agent whose pass rate regressed. Used as the `rollout_id` for the
    /// impact check so `metric_before_and_after` queries the agent's
    /// `harness_summary` event group.
    agent_name: String,
    /// The event position of the previous (better) harness run. Passed as
    /// `before_position` to `submit_rollout_impact_check` — `verify_impact`
    /// reads the metric at this position (the previous run's pass rate) and
    /// at the latest event after it (the current run's pass rate).
    before_position: i64,
}

#[derive(Debug, Clone, PartialEq)]
struct ScannedHarnessSummary {
    event_position: i64,
    regression: Option<HarnessRegression>,
}

/// Result of one scheduler-driven harness-monitor poll.
///
/// `Complete` consumed every queried summary. `Backpressured` consumed only
/// the contiguous prefix ending before `blocked_event_position`; the blocked
/// event remains queryable from the monitor's retained cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessMonitorPoll {
    Complete {
        accepted_checks: usize,
        cursor: Option<i64>,
    },
    Backpressured {
        accepted_checks: usize,
        blocked_event_position: i64,
        capacity: usize,
    },
}

/// Stateful harness-summary monitor used by the production scheduler.
///
/// The cursor is owned beside the handoff operation so callers cannot advance
/// it independently of queue admission. Cancellation after acceptance but
/// before cursor mutation may retry a check; it cannot skip one.
#[derive(Debug, Default)]
pub struct HarnessRegressionMonitor {
    cursor: Option<i64>,
}

impl HarnessRegressionMonitor {
    #[cfg(test)]
    fn new(cursor: Option<i64>) -> Self {
        Self { cursor }
    }

    #[cfg(test)]
    fn cursor(&self) -> Option<i64> {
        self.cursor
    }

    /// Query new summaries, derive all regressions, then offer checks in event
    /// order. Query failure leaves the cursor untouched. Queue refusal leaves
    /// it at the accepted contiguous prefix so the blocked event is retried.
    pub async fn poll_once(
        &mut self,
        store: &EventStore,
        regulation: &CyberneticsLoop,
    ) -> Result<HarnessMonitorPoll, String> {
        let scanned = scan_harness_summaries(store, self.cursor)?;
        let mut accepted_checks = 0;
        for summary in scanned {
            if let Some(regression) = summary.regression {
                match regulation
                    .submit_rollout_impact_check(
                        regression.agent_name,
                        regression.before_position,
                        "pass_rate".to_string(),
                    )
                    .await
                {
                    RolloutImpactSubmission::Accepted => accepted_checks += 1,
                    RolloutImpactSubmission::QueueFull { capacity } => {
                        return Ok(HarnessMonitorPoll::Backpressured {
                            accepted_checks,
                            blocked_event_position: summary.event_position,
                            capacity,
                        });
                    }
                }
            }
            self.cursor = Some(summary.event_position);
        }
        Ok(HarnessMonitorPoll::Complete {
            accepted_checks,
            cursor: self.cursor,
        })
    }
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
        let manager = hkask_storage::SqliteConnectionManager::file(events_path)
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

    /// Construct from an existing store handle (test seam; production opens
    /// the store with `open`).
    #[doc(hidden)]
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
    ) -> Result<Option<(f64, f64)>, RolloutEventError> {
        let events = self
            .store
            .query(&hkask_event_store::EventFilter {
                rollout: Some(rollout_id.to_string()),
                ..hkask_event_store::EventFilter::default()
            })
            .map_err(|e| RolloutEventError::Query {
                detail: e.to_string(),
            })?;
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
        // Both sides extract from METRIC-VALUED events only, taking the
        // LATEST on each side of the detection point: "before" is the value
        // the detector saw (the latest at-or-before — taking the FIRST
        // would read an earlier baseline and could invert the verdict:
        // 0.4→0.6 "improved" where the real detection pair 0.9→0.6
        // degraded), and "after" is the latest measurement since (a
        // trailing non-metric event — e.g. a verdict — must not suppress
        // the comparison to None).
        let before = events
            .iter()
            .filter(|event| event.position <= before_position)
            .filter_map(|event| value_of(&event.payload))
            .next_back();
        let after = events
            .iter()
            .filter(|event| event.position > before_position)
            .filter_map(|event| value_of(&event.payload))
            .next_back();
        match (before, after) {
            (Some(before), Some(after)) => Ok(Some((before, after))),
            _ => Ok(None),
        }
    }

    fn append_impact_verdict(
        &self,
        rollout_id: &str,
        metric: &str,
        before: f64,
        after: f64,
        improved: bool,
        decision: &str,
    ) -> Result<(), RolloutEventError> {
        // Write the regulation loop's impact verdict back to the store as
        // a verdict event with source = regulation_impact. This closes the
        // feedback loop: downstream consumers (training bridge, regression
        // monitor, ORIENT) can see the regulation system's judgment
        // alongside the harness's deterministic-evaluator verdicts.
        //
        // The payload carries typed wire strings (VerdictSource::RegulationImpact,
        // RolloutKind::Delegation) so consumers can parse them back without
        // the store parsing payloads. The metric name identifies what was
        // measured; before/after/improved/decision carry the judgment.
        let payload = serde_json::json!({
            "pass": improved,
            "source": hkask_event_store::VerdictSource::RegulationImpact.as_str(),
            "rollout_kind": hkask_event_store::RolloutKind::Delegation.as_str(),
            "metric": metric,
            "before": before,
            "after": after,
            "improved": improved,
            "decision": decision,
        });
        self.store
            .append(rollout_id, "verdict", &payload)
            .map_err(|e| RolloutEventError::WriteBack {
                detail: e.to_string(),
            })?;
        Ok(())
    }
}

/// Scan without acknowledging. The monitor applies cursor changes only after
/// this complete query/derivation phase succeeds and each required handoff is
/// accepted.
fn scan_harness_summaries(
    store: &EventStore,
    last_cursor: Option<i64>,
) -> Result<Vec<ScannedHarnessSummary>, String> {
    let new_events = store
        .query(&EventFilter {
            kind: Some("harness_summary".to_string()),
            after_position: last_cursor,
            ..EventFilter::default()
        })
        .map_err(|e| format!("harness_summary query failed: {e}"))?;
    let mut scanned = Vec::with_capacity(new_events.len());
    for event in &new_events {
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
                    "harness_summary event missing overall_pass_rate — acknowledging without impact check"
                );
                scanned.push(ScannedHarnessSummary {
                    event_position: event.position,
                    regression: None,
                });
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
            scanned.push(ScannedHarnessSummary {
                event_position: event.position,
                regression: None,
            });
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
                    "previous harness_summary event missing overall_pass_rate — acknowledging current summary without impact check"
                );
                scanned.push(ScannedHarnessSummary {
                    event_position: event.position,
                    regression: None,
                });
                continue;
            }
        };
        let drop = previous_pass_rate - current_pass_rate;
        let regression = if drop > REGRESSION_THRESHOLD {
            tracing::info!(
                target: "hkask.bridge.harness",
                agent = %agent_name,
                previous_pass_rate,
                current_pass_rate,
                drop,
                "harness pass-rate regression detected — offering impact check"
            );
            Some(HarnessRegression {
                agent_name: agent_name.clone(),
                before_position: previous.position,
            })
        } else {
            None
        };
        scanned.push(ScannedHarnessSummary {
            event_position: event.position,
            regression,
        });
    }
    Ok(scanned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_regulation::{CyberneticsLoop, RegulationLedger, RolloutImpactSubmission};
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_storage::database::value::DbValue;
    use tokio::sync::RwLock;

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

    fn scan_results(
        store: &EventStore,
        cursor: Option<i64>,
    ) -> (Option<i64>, Vec<HarnessRegression>) {
        let scanned = scan_harness_summaries(store, cursor).unwrap();
        let new_cursor = scanned
            .last()
            .map(|summary| summary.event_position)
            .or(cursor);
        let regressions = scanned
            .into_iter()
            .filter_map(|summary| summary.regression)
            .collect();
        (new_cursor, regressions)
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

    /// The before/after extraction preserves the EXACT detector→
    /// verification pair. In a 0.4→0.9→0.6 history with the detector firing
    /// at the 0.9 event, the pair is (0.9, 0.6) — "before" is the value at
    /// the detection point (the LATEST metric-valued event at-or-before),
    /// not the earliest. Interleaved and trailing verdict events (no metric
    /// value) must not suppress the comparison — "after" is the latest
    /// METRIC-VALUED event after the detection point. Pre-fix, "before"
    /// took the first value (0.4 — the verdict could INVERT: 0.4→0.6 reads
    /// improved where the real 0.9→0.6 pair degraded) and a trailing
    /// verdict made "after" None, suppressing the evidence entirely.
    #[test]
    fn metric_before_and_after_preserves_the_detection_pair() {
        let store = memory_store();
        // History: 0.4 → 0.9 (detector fires here) → verdict → 0.6 → verdict.
        write_summary(&store, "alpha", 0.4);
        let detection = write_summary(&store, "alpha", 0.9);
        store
            .append(
                "alpha",
                "verdict",
                &serde_json::json!({"pass": true, "source": "deterministic"}),
            )
            .unwrap();
        write_summary(&store, "alpha", 0.6);
        store
            .append(
                "alpha",
                "verdict",
                &serde_json::json!({"pass": false, "source": "regulation_impact"}),
            )
            .unwrap();
        let bridge = BridgeRolloutEventSource::from_store(Arc::new(store));

        let result = bridge
            .metric_before_and_after("alpha", "pass_rate", detection)
            .unwrap();
        assert_eq!(
            result,
            Some((0.9, 0.6)),
            "the exact detection→verification pair, unsuppressed by verdict events"
        );
    }

    /// Control: metric identity is retained — an event carrying a
    /// DIFFERENT metric's value is never picked up as this metric's
    /// before/after.
    #[test]
    fn metric_before_and_after_skips_other_metric_events() {
        let store = memory_store();
        let detection = write_summary(&store, "alpha", 0.9);
        // A latency event after the detection — no pass_rate value.
        store
            .append(
                "alpha",
                "model_request",
                &serde_json::json!({"latency_ms": 1200}),
            )
            .unwrap();
        write_summary(&store, "alpha", 0.6);
        let bridge = BridgeRolloutEventSource::from_store(Arc::new(store));

        let result = bridge
            .metric_before_and_after("alpha", "pass_rate", detection)
            .unwrap();
        assert_eq!(
            result,
            Some((0.9, 0.6)),
            "the latency event must not be picked up as (or suppress) the pass_rate pair"
        );
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
    fn scan_harness_summaries_detects_material_drop() {
        let store = memory_store();
        let first = write_summary(&store, "alpha", 0.80);
        let _second = write_summary(&store, "alpha", 0.60);
        // 0.80 - 0.60 = 0.20 > 0.10 threshold
        let (cursor, regressions) = scan_results(&store, None);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].agent_name, "alpha");
        assert_eq!(regressions[0].before_position, first);
        assert_eq!(cursor, Some(_second));
    }

    #[test]
    fn scan_harness_summaries_skips_improvement() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.50);
        write_summary(&store, "alpha", 0.80);
        // 0.50 - 0.80 = -0.30 < 0.10 — an improvement, not a regression
        let (_cursor, regressions) = scan_results(&store, None);
        assert!(regressions.is_empty(), "improvement is not a regression");
    }

    #[test]
    fn scan_harness_summaries_skips_marginal_drop() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.70);
        write_summary(&store, "alpha", 0.65);
        // 0.70 - 0.65 = 0.05 < 0.10 — within noise
        let (_cursor, regressions) = scan_results(&store, None);
        assert!(
            regressions.is_empty(),
            "marginal drop is within the threshold"
        );
    }

    #[test]
    fn scan_harness_summaries_skips_first_run() {
        let store = memory_store();
        let pos = write_summary(&store, "alpha", 0.30);
        // First run — no baseline to regress from
        let (cursor, regressions) = scan_results(&store, None);
        assert!(regressions.is_empty(), "first run has no baseline");
        assert_eq!(cursor, Some(pos));
    }

    #[test]
    fn scan_harness_summaries_is_incremental() {
        let store = memory_store();
        let first = write_summary(&store, "alpha", 0.80);
        // First check: processes first event, no regression (no previous)
        let (cursor, regressions) = scan_results(&store, None);
        assert!(regressions.is_empty());
        assert_eq!(cursor, Some(first));
        // Second run: regression
        let second = write_summary(&store, "alpha", 0.50);
        let (cursor, regressions) = scan_results(&store, cursor);
        assert_eq!(regressions.len(), 1);
        assert_eq!(cursor, Some(second));
    }

    #[test]
    fn scan_harness_summaries_is_independent_per_agent() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.80);
        write_summary(&store, "beta", 0.90);
        write_summary(&store, "alpha", 0.50); // alpha regresses
        write_summary(&store, "beta", 0.85); // beta marginal — no regression
        let (_cursor, regressions) = scan_results(&store, None);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].agent_name, "alpha");
    }

    #[tokio::test]
    async fn blocked_submission_retains_cursor_at_contiguous_accepted_prefix() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.80);
        let baseline_cursor = write_summary(&store, "beta", 0.90);
        let alpha_regression = write_summary(&store, "alpha", 0.50);
        let beta_regression = write_summary(&store, "beta", 0.60);
        let regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        for position in 0..63 {
            assert_eq!(
                regulation
                    .submit_rollout_impact_check(
                        format!("preloaded-{position}"),
                        position,
                        "pass_rate".to_string(),
                    )
                    .await,
                RolloutImpactSubmission::Accepted
            );
        }
        let mut monitor = HarnessRegressionMonitor::new(Some(baseline_cursor));

        let result = monitor.poll_once(&store, &regulation).await.unwrap();

        assert_eq!(
            result,
            HarnessMonitorPoll::Backpressured {
                accepted_checks: 1,
                blocked_event_position: beta_regression,
                capacity: 64,
            }
        );
        assert_eq!(monitor.cursor(), Some(alpha_regression));

        regulation.tick().await;
        let retry = monitor.poll_once(&store, &regulation).await.unwrap();
        assert_eq!(
            retry,
            HarnessMonitorPoll::Complete {
                accepted_checks: 1,
                cursor: Some(beta_regression),
            }
        );
        assert_eq!(monitor.cursor(), Some(beta_regression));
    }

    #[tokio::test]
    async fn query_failure_leaves_monitor_cursor_unchanged() {
        let store = memory_store();
        let baseline_cursor = write_summary(&store, "alpha", 0.80);
        store
            .driver()
            .execute(
                "INSERT INTO events (rollout_id, kind, payload, created_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                &[
                    DbValue::Text("alpha".to_string()),
                    DbValue::Text("harness_summary".to_string()),
                    DbValue::Text("not-json".to_string()),
                    DbValue::Text("2026-09-16T00:00:00Z".to_string()),
                ],
            )
            .unwrap();
        let regulation = CyberneticsLoop::new(Arc::new(RwLock::new(RegulationLedger::default())));
        let mut monitor = HarnessRegressionMonitor::new(Some(baseline_cursor));

        assert!(monitor.poll_once(&store, &regulation).await.is_err());
        assert_eq!(monitor.cursor(), Some(baseline_cursor));
    }

    #[test]
    fn production_scheduler_uses_behavior_tested_monitor_handoff() {
        let main = include_str!("../../../../crates/zed/src/main.rs");
        assert!(main.contains("HarnessRegressionMonitor::default()"));
        assert!(main.contains(".poll_once(&store, &loop_guard)"));
        assert!(!main.contains("last_cursor = new_cursor"));
        assert!(!main.contains("check_harness_regressions("));
    }

    #[test]
    fn append_impact_verdict_writes_regulation_impact_event() {
        let store = memory_store();
        write_summary(&store, "alpha", 0.80);
        let bridge = BridgeRolloutEventSource::from_store(Arc::new(store.clone()));
        bridge
            .append_impact_verdict("alpha", "pass_rate", 0.80, 0.50, false, "Worsen")
            .unwrap();
        let events = store
            .query(&EventFilter {
                rollout: Some("alpha".into()),
                kind: Some("verdict".into()),
                ..EventFilter::default()
            })
            .unwrap();
        assert_eq!(events.len(), 1);
        let payload = &events[0].payload;
        assert_eq!(payload["source"], "regulation_impact");
        assert_eq!(payload["rollout_kind"], "delegation");
        assert_eq!(payload["metric"], "pass_rate");
        assert_eq!(payload["before"], 0.80);
        assert_eq!(payload["after"], 0.50);
        assert_eq!(payload["improved"], false);
        assert_eq!(payload["decision"], "Worsen");
        assert_eq!(
            hkask_event_store::VerdictSource::from_str(payload["source"].as_str().unwrap()),
            Some(hkask_event_store::VerdictSource::RegulationImpact)
        );
    }

    #[test]
    fn append_impact_verdict_errors_on_empty_rollout_id() {
        let store = memory_store();
        let bridge = BridgeRolloutEventSource::from_store(Arc::new(store));
        assert!(
            bridge
                .append_impact_verdict("", "pass_rate", 0.8, 0.5, false, "Worsen")
                .is_err()
        );
    }
}
