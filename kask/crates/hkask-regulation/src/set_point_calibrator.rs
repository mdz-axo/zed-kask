//! SetPointCalibrator — Self-tuning regulation thresholds via RegulationArchive replay.
//!
//! The Conant-Ashby closure for set-points: queries persisted regulation outcome
//! events from the RegulationArchive, counts patterns per metric, and adjusts thresholds
//! within bounded ranges through a caller-provided callback.
//!
//! # Adjustment rules
//!
//! - Plateau detected ≥ threshold → widen stagnation threshold (more patient)
//! - Action blocked ≥ threshold → tighten block worsening ratio (more conservative)
//! - Substitutions ≥ threshold × 2 → decrease substitution_after (try alternatives sooner)
//!
//! # Minimum data requirement
//!
//! Skips adjustment until `min_total_observations` regulation events have accumulated.
//! Default: 30 events (configurable via `HKASK_SET_POINT_MIN_OBSERVATIONS`).

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hkask_types::InfrastructureError;
use hkask_types::LedgerStoragePort;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{info, warn};

/// Default calibration interval (1 hour — regulation events are sparse).
pub const DEFAULT_SET_POINT_CALIBRATION_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Minimum total regulation events before any adjustment.
/// Higher than the energy calibrator because regulation events are sparse
/// and set-point changes are policy decisions that shouldn't oscillate.
const DEFAULT_MIN_TOTAL_OBSERVATIONS: u64 = 50;

/// Minimum observations per metric to consider adjustment.
const MIN_PER_METRIC_OBSERVATIONS: usize = 3;

/// Adjustment step ratio (10% change per adjustment).
const ADJUSTMENT_STEP: f64 = 0.10;

/// Bounds for stagnation threshold.
const MAX_STAGNATION_THRESHOLD: u32 = 20;
const MIN_STAGNATION_THRESHOLD: u32 = 3;

/// Bounds for block worsening ratio.
const MAX_BLOCK_WORSENING_RATIO: f64 = 0.50;
const MIN_BLOCK_WORSENING_RATIO: f64 = 0.10;

/// Bounds for substitution delay.
const MIN_SUBSTITUTION_AFTER: u32 = 1;

/// Per-metric regulation outcome counts.
#[derive(Debug, Default)]
struct MetricOutcomes {
    substitutions: usize,
    blocks: usize,
    plateaus: usize,
    total: usize,
}

/// Calibration result: (metric_name, adjustment_description).
pub struct SetPointAdjustment {
    pub metric: String,
    pub field: String,
    pub old_value: String,
    pub new_value: String,
}

/// Auto-tuning calibrator for Regulation regulation set-points.
///
/// Queries the RegulationArchive for regulation events, counts outcomes per metric,
/// and applies adjustments through the provided callback. The callback receives
/// a list of adjustments and should apply them to the active SetPoints.
pub struct SetPointCalibrator {
    store: Arc<dyn LedgerStoragePort>,
    last_calibrated_at: tokio::sync::Mutex<DateTime<Utc>>,
    calibration_alive: AtomicBool,
    min_total_observations: u64,
}

impl SetPointCalibrator {
    /// Create a calibrator backed by the given event store.
    ///
    /// `initial_lookback` controls how far back the first calibration searches
    /// for events (e.g., 1 hour for fresh start, 24 hours for restart with history).
    pub fn new(store: Arc<dyn LedgerStoragePort>, initial_lookback: ChronoDuration) -> Self {
        Self {
            store,
            last_calibrated_at: tokio::sync::Mutex::new(Utc::now() - initial_lookback),
            calibration_alive: AtomicBool::new(false),
            min_total_observations: std::env::var("HKASK_SET_POINT_MIN_OBSERVATIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_MIN_TOTAL_OBSERVATIONS),
        }
    }

    /// Run one calibration pass. Returns adjustments that should be applied.
    ///
    /// Returns an empty vec if insufficient data, no patterns detected, or no
    /// adjustments needed. The caller should apply the returned adjustments
    /// to the active SetPoints.
    pub async fn evaluate(&self) -> Result<Vec<SetPointAdjustment>, InfrastructureError> {
        let until = Utc::now();
        let since = {
            let mut last = self.last_calibrated_at.lock().await;
            let s = *last;
            *last = until;
            s
        };

        let events = self.store.query_algedonic(since, 2000)?;

        let regulation_events: Vec<_> = events
            .iter()
            .filter(|e| e.span.namespace.short_name() == "outcome")
            .collect();

        if regulation_events.is_empty() {
            return Ok(vec![]);
        }

        let mut outcomes: HashMap<String, MetricOutcomes> = HashMap::new();
        let mut total_observations: u64 = 0;

        for event in &regulation_events {
            let metric = event
                .observation
                .get("metric")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let entry = outcomes.entry(metric.clone()).or_default();
            entry.total += 1;
            total_observations += 1;

            let local_path = event
                .span
                .path
                .strip_prefix(&format!("{}.", event.span.namespace.as_str()))
                .unwrap_or(event.span.path.as_str());
            match local_path {
                "action_substituted" => entry.substitutions += 1,
                "action_blocked" => entry.blocks += 1,
                "regulatory_plateau_detected" => entry.plateaus += 1,
                _ => {}
            }
        }

        if total_observations < self.min_total_observations {
            info!(
                target: "reg.outcome.calibration",
                total_observations,
                min_required = self.min_total_observations,
                "Set-point calibrator skipped — insufficient data",
            );
            return Ok(vec![]);
        }

        let mut adjustments = Vec::new();

        for (metric_name, counts) in &outcomes {
            if counts.total < MIN_PER_METRIC_OBSERVATIONS {
                continue;
            }

            // Plateau: widen stagnation threshold
            if counts.plateaus >= MIN_PER_METRIC_OBSERVATIONS {
                adjustments.push(SetPointAdjustment {
                    metric: metric_name.clone(),
                    field: "stagnation_threshold".into(),
                    old_value: "current".into(),
                    new_value: format!("widen_by_{:.0}pct", ADJUSTMENT_STEP * 100.0),
                });
            }

            // Block: tighten worsening ratio
            if counts.blocks >= MIN_PER_METRIC_OBSERVATIONS {
                adjustments.push(SetPointAdjustment {
                    metric: metric_name.clone(),
                    field: "block_worsening_ratio".into(),
                    old_value: "current".into(),
                    new_value: format!("tighten_by_{:.0}pct", ADJUSTMENT_STEP * 100.0),
                });
            }

            // Substitution: decrease substitution_after
            if counts.substitutions >= MIN_PER_METRIC_OBSERVATIONS * 2 {
                adjustments.push(SetPointAdjustment {
                    metric: metric_name.clone(),
                    field: "substitution_after".into(),
                    old_value: "current".into(),
                    new_value: "decrease_by_1".into(),
                });
            }
        }

        if !adjustments.is_empty() {
            info!(
                target: "reg.outcome.calibration",
                since = %since,
                until = %until,
                adjustment_count = adjustments.len(),
                total_events = total_observations,
                metrics_analyzed = outcomes.len(),
                "Set-point calibration complete — adjustments recommended",
            );
        }

        Ok(adjustments)
    }

    /// Apply the given adjustments to the provided `SetPoints` via a callback.
    ///
    /// The callback receives an `&mut SetPoints` and is expected to apply the
    /// adjustments. This is a synchronous operation — callers should provide
    /// a closure that has mutable access to the active set-points.
    pub fn apply_adjustments(
        adjustments: &[SetPointAdjustment],
        stagnation_thresholds: &mut HashMap<String, u32>,
        block_worsening_ratio: &mut f64,
        substitution_after: &mut u32,
    ) {
        for adj in adjustments {
            match adj.field.as_str() {
                "stagnation_threshold" => {
                    let current = stagnation_thresholds
                        .get(&adj.metric)
                        .copied()
                        .unwrap_or(crate::set_points::DEFAULT_STAGNATION_THRESHOLD);
                    let new = (current as f64 * (1.0 + ADJUSTMENT_STEP)).ceil() as u32;
                    let new = new.clamp(MIN_STAGNATION_THRESHOLD, MAX_STAGNATION_THRESHOLD);
                    if new != current {
                        info!(
                            target: "reg.outcome.calibration",
                            metric = adj.metric,
                            old = current,
                            new = new,
                            "Widening stagnation threshold",
                        );
                        stagnation_thresholds.insert(adj.metric.clone(), new);
                    }
                }
                "block_worsening_ratio" => {
                    let new = (*block_worsening_ratio * (1.0 - ADJUSTMENT_STEP))
                        .clamp(MIN_BLOCK_WORSENING_RATIO, MAX_BLOCK_WORSENING_RATIO);
                    if (new - *block_worsening_ratio).abs() > f64::EPSILON {
                        info!(
                            target: "reg.outcome.calibration",
                            metric = adj.metric,
                            old = block_worsening_ratio,
                            new = new,
                            "Tightening block worsening ratio",
                        );
                        *block_worsening_ratio = new;
                    }
                }
                "substitution_after" => {
                    let new = substitution_after
                        .saturating_sub(1)
                        .max(MIN_SUBSTITUTION_AFTER);
                    if *substitution_after != new {
                        info!(
                            target: "reg.outcome.calibration",
                            metric = adj.metric,
                            old = substitution_after,
                            new = new,
                            "Decreasing substitution delay",
                        );
                        *substitution_after = new;
                    }
                }
                _ => {}
            }
        }
    }

    /// Spawn a background calibration task.
    ///
    /// The `apply_fn` closure is called with adjustments after each calibration
    /// pass. It should apply them to the active `SetPoints` instance.
    pub fn spawn_calibration<F>(self: Arc<Self>, interval: Duration, apply_fn: F)
    where
        F: Fn(Vec<SetPointAdjustment>) + Send + Sync + 'static,
    {
        self.calibration_alive.store(true, Ordering::Release);
        let apply = Arc::new(apply_fn);
        tokio::spawn(async move {
            info!(
                target: "reg.outcome.calibration",
                interval_secs = interval.as_secs(),
                "Set-point calibrator started",
            );

            loop {
                tokio::time::sleep(interval).await;
                match self.evaluate().await {
                    Ok(adjustments) => {
                        if !adjustments.is_empty() {
                            apply(adjustments);
                        }
                    }
                    Err(e) => {
                        warn!(
                            target: "reg.outcome.calibration",
                            error = %e,
                            "Calibration tick failed — will retry",
                        );
                    }
                }
            }
        });
    }

    /// Whether the background calibration loop is running.
    pub fn is_alive(&self) -> bool {
        self.calibration_alive.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::RegulationArchive;
    use hkask_storage::database::sqlite::SqliteDriver;
    use hkask_types::WebID;
    use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanNamespace};

    fn make_event_store() -> (Arc<dyn LedgerStoragePort>, Arc<RegulationArchive>) {
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(RegulationArchive::from_driver(driver));
        let port = Arc::clone(&store) as Arc<dyn LedgerStoragePort>;
        (port, store)
    }

    fn regulation_event(metric: &str, path: &str) -> RegulationRecord {
        let ns = SpanNamespace::new("reg.outcome").expect("reg.outcome must be canonical");
        let span = Span::new(ns, path);
        RegulationRecord::new(
            WebID::from_persona(b"regulation"),
            span,
            CyclePhase::Act,
            serde_json::json!({"metric": metric}),
            0,
        )
    }

    #[tokio::test]
    async fn evaluate_skips_when_insufficient_data() {
        let (store, event_store) = make_event_store();
        let calibrator = SetPointCalibrator::new(store, ChronoDuration::hours(1));

        let sink: &dyn hkask_types::event::RegulationSink = &*event_store;
        let _ = sink.persist(&regulation_event("variety_deficit", "action_substituted"));
        let _ = sink.persist(&regulation_event("error_rate", "action_blocked"));

        let result = calibrator.evaluate().await.unwrap();
        assert!(result.is_empty(), "Should skip with <50 observations");
    }

    #[tokio::test]
    async fn evaluate_detects_plateau_pattern() {
        let (store, event_store) = make_event_store();

        // Persist regulation events
        let sink: &dyn hkask_types::event::RegulationSink = &*event_store;
        for _ in 0..55 {
            let _ = sink.persist(&regulation_event(
                "variety_deficit",
                "regulatory_plateau_detected",
            ));
        }

        // Verify events are queryable
        let events = store
            .query_algedonic(chrono::Utc::now() - chrono::Duration::minutes(5), 100)
            .unwrap();
        let reg_events: Vec<_> = events
            .iter()
            .filter(|e| e.span.namespace.short_name() == "outcome")
            .collect();
        assert!(
            !reg_events.is_empty(),
            "Events should be persisted and queryable (found {})",
            reg_events.len()
        );

        // Now verify the calibrator detects patterns
        let calibrator = SetPointCalibrator::new(store, ChronoDuration::hours(1));
        let result = calibrator.evaluate().await.unwrap();
        assert!(
            !result.is_empty(),
            "Should detect plateau pattern (events in DB: {}, min required: {})",
            reg_events.len(),
            calibrator.min_total_observations
        );
        assert!(
            result.iter().any(|a| a.field == "stagnation_threshold"),
            "Should recommend widening stagnation threshold"
        );
    }

    #[test]
    fn apply_adjustments_widens_stagnation() {
        let adjustments = vec![SetPointAdjustment {
            metric: "variety_deficit".into(),
            field: "stagnation_threshold".into(),
            old_value: "5".into(),
            new_value: "6".into(),
        }];

        let mut thresholds = HashMap::new();
        thresholds.insert("variety_deficit".into(), 5u32);
        let mut ratio = 0.20;
        let mut sub_after = 2u32;

        SetPointCalibrator::apply_adjustments(
            &adjustments,
            &mut thresholds,
            &mut ratio,
            &mut sub_after,
        );

        assert!(
            thresholds.get("variety_deficit").copied().unwrap() > 5,
            "Threshold should widen"
        );
    }
}
