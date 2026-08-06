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
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ResolvedObservation {
    pub probability: f64,
    pub outcome: bool,
}

/// Journal row for persistence (bucket + observation per line).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JournalRow {
    bucket: String,
    probability: f64,
    outcome: bool,
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
        self.buckets
            .entry(bucket.to_string())
            .or_default()
            .push(observation);
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

    /// Whether an identical observation already exists in the bucket —
    /// idempotent ingest guard for the resolution scanner.
    pub fn contains(&self, bucket: &str, observation: &ResolvedObservation) -> bool {
        self.buckets.get(bucket).is_some_and(|v| {
            v.iter().any(|o| {
                (o.probability - observation.probability).abs() < 1e-9
                    && o.outcome == observation.outcome
            })
        })
    }

    pub fn sample_size(&self, bucket: &str) -> u64 {
        self.buckets.get(bucket).map_or(0, |v| v.len() as u64)
    }

    /// Load a journal (JSONL, one {bucket, probability, outcome} per line).
    /// A missing file is a fresh store (not an error); a malformed line is
    /// skipped with a warning — the calibration signal degrades to `stale`
    /// for that bucket rather than fabricating data.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let mut store = Self::new();
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(store),
            Err(e) => return Err(e),
        };
        for (line_no, line) in contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<JournalRow>(line) {
                Ok(row) => store.record(
                    &row.bucket,
                    ResolvedObservation {
                        probability: row.probability,
                        outcome: row.outcome,
                    },
                ),
                Err(e) => {
                    tracing::warn!(
                        "calibration journal {} line {} malformed ({e}); skipping",
                        path.display(),
                        line_no + 1
                    );
                }
            }
        }
        Ok(store)
    }

    /// Persist the journal (JSONL). Atomic-ish: write to a temp file then
    /// rename, so a crash mid-write cannot truncate the existing journal.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = String::new();
        for (bucket, observations) in &self.buckets {
            for o in observations {
                let row = JournalRow {
                    bucket: bucket.clone(),
                    probability: o.probability,
                    outcome: o.outcome,
                };
                out.push_str(
                    &serde_json::to_string(&row)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
                );
                out.push('\n');
            }
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
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
