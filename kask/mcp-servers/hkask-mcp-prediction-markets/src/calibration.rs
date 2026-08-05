//! Market-calibration store and Brier computation (T5).
//!
//! Reuses `hkask-forecast` math — never reimplemented here. The store is an
//! in-memory journal of resolved (probability-at-observation, outcome) pairs
//! per domain/series bucket; persistence is the T12 event-base decision.
//!
//! Cybernetic invariant: a bucket with no data or a read failure is `stale`,
//! never `brier: 0` — a synthetic 0 reads as "perfectly calibrated" and
//! creates a reinforcing loop (the `.rules` unwrap_or(0) trap generalized).

use std::collections::HashMap;

use hkask_forecast::brier_score_multi;

/// One resolved observation: the market-implied probability at observation
/// time and the realized outcome.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedObservation {
    pub probability: f64,
    pub outcome: bool,
}

/// In-memory calibration store keyed by domain/series bucket.
#[derive(Debug, Default)]
pub struct CalibrationStore {
    buckets: HashMap<String, Vec<ResolvedObservation>>,
}

impl CalibrationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a resolved observation into a bucket (e.g. "politics",
    /// "economics", or a series ticker).
    pub fn record(&mut self, bucket: &str, observation: ResolvedObservation) {
        self.buckets.entry(bucket.to_string()).or_default().push(observation);
    }

    /// Brier score for a bucket. `Err(())` when the bucket is missing or
    /// empty — the caller maps this to `stale: true`, never `brier: 0`.
    pub fn brier(&self, bucket: &str) -> Result<f64, ()> {
        let observations = self.buckets.get(bucket).ok_or(())?;
        if observations.is_empty() {
            return Err(());
        }
        let probabilities: Vec<f64> = observations.iter().map(|o| o.probability).collect();
        let outcomes: Vec<bool> = observations.iter().map(|o| o.outcome).collect();
        brier_score_multi(&probabilities, &outcomes).map_err(|_| ())
    }

    pub fn sample_size(&self, bucket: &str) -> u64 {
        self.buckets.get(bucket).map_or(0, |v| v.len() as u64)
    }
}

/// The calibration block returned on market records / the calibration tool.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CalibrationReading {
    pub bucket: String,
    pub brier: Option<f64>,
    pub sample_size: u64,
    /// True whenever the Brier could not be measured (missing bucket, empty
    /// sample, or math failure). The only honest "no signal" representation.
    pub stale: bool,
}

/// Read the calibration signal for a bucket. Thin samples and missing
/// buckets yield `stale: true` with `brier: None` — never a synthetic 0.
pub fn read_calibration(store: &CalibrationStore, bucket: &str) -> CalibrationReading {
    let sample_size = store.sample_size(bucket);
    match store.brier(bucket) {
        Ok(brier) => CalibrationReading {
            bucket: bucket.to_string(),
            brier: Some(brier),
            sample_size,
            stale: false,
        },
        Err(()) => CalibrationReading {
            bucket: bucket.to_string(),
            brier: None,
            sample_size,
            stale: true,
        },
    }
}
