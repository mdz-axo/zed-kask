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

/// A pre-resolution price snapshot: the market-implied probability at the
/// time the scanner first observed the market. The EARLIEST snapshot per
/// market is the honest probability-at-observation for the Brier loop —
/// scoring a market's post-resolution price instead is self-fulfilling
/// (the outcome is derived from that same terminal price, so Brier ≈ 0 by
/// construction and the reliability-tier demotion gate can never fire).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingSnapshot {
    pub bucket: String,
    pub probability: f64,
}

/// Journal row for persistence (bucket + observation per line).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JournalRow {
    bucket: String,
    probability: f64,
    outcome: bool,
}

/// Pending-journal row (market key + pre-resolution snapshot per line).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PendingJournalRow {
    market_key: String,
    bucket: String,
    probability: f64,
}

/// Derive the pending-snapshot journal path from the observation journal
/// path (`calibration.jsonl` → `calibration.pending.jsonl`).
fn pending_journal_path(path: &std::path::Path) -> std::path::PathBuf {
    let name = path.file_name().map_or_else(
        || path.to_string_lossy().to_string(),
        |n| n.to_string_lossy().to_string(),
    );
    let new_name = if let Some(stem) = name.strip_suffix(".jsonl") {
        format!("{stem}.pending.jsonl")
    } else {
        format!("{name}.pending")
    };
    path.with_file_name(new_name)
}

/// In-memory calibration store keyed by domain/series bucket.
#[derive(Debug, Default)]
pub struct CalibrationStore {
    buckets: HashMap<String, Vec<ResolvedObservation>>,
    /// Pre-resolution price snapshots, keyed by provider-stable market key
    /// (Kalshi `ticker`, Polymarket Gamma `id`).
    pending: HashMap<String, PendingSnapshot>,
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

    /// Record a pre-resolution snapshot for a market, keeping the EARLIEST
    /// observation per market key (the first price the scanner saw — a later
    /// price is resolution-informed and would bias the Brier loop). Returns
    /// whether this was the market's first snapshot.
    pub fn record_pending(&mut self, market_key: &str, snapshot: PendingSnapshot) -> bool {
        let len_before = self.pending.len();
        self.pending
            .entry(market_key.to_string())
            .or_insert(snapshot);
        self.pending.len() > len_before
    }

    /// Consume the pre-resolution snapshot for a market (removed once the
    /// resolution is recorded — the observation is durable in the journal).
    pub fn take_pending(&mut self, market_key: &str) -> Option<PendingSnapshot> {
        self.pending.remove(market_key)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Load a journal (JSONL, one {bucket, probability, outcome} per line)
    /// plus its pending-snapshot journal. A missing file is a fresh store
    /// (not an error); a malformed line is skipped with a warning — the
    /// calibration signal degrades to `stale` for that bucket rather than
    /// fabricating data.
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
        let pending_path = pending_journal_path(path);
        let pending_contents = match std::fs::read_to_string(&pending_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        for (line_no, line) in pending_contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<PendingJournalRow>(line) {
                Ok(row) => {
                    store.pending.insert(
                        row.market_key,
                        PendingSnapshot {
                            bucket: row.bucket,
                            probability: row.probability,
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "pending-snapshot journal {} line {} malformed ({e}); skipping",
                        pending_path.display(),
                        line_no + 1
                    );
                }
            }
        }
        Ok(store)
    }

    /// Persist the journal (JSONL) and the pending-snapshot journal.
    /// Atomic-ish: write to a temp file then rename, so a crash mid-write
    /// cannot truncate the existing journal.
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

        let pending_path = pending_journal_path(path);
        let mut pending_out = String::new();
        for (market_key, snapshot) in &self.pending {
            let row = PendingJournalRow {
                market_key: market_key.clone(),
                bucket: snapshot.bucket.clone(),
                probability: snapshot.probability,
            };
            pending_out.push_str(
                &serde_json::to_string(&row)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
            );
            pending_out.push('\n');
        }
        let pending_tmp = pending_path.with_extension("tmp");
        std::fs::write(&pending_tmp, pending_out)?;
        std::fs::rename(&pending_tmp, pending_path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_bucket_reads_stale_never_zero() {
        // The module's cybernetic invariant: no data ⇒ stale, not brier: 0 —
        // a synthetic 0 reads as "perfectly calibrated" and creates a
        // reinforcing loop.
        let store = CalibrationStore::new();
        let reading = read_calibration(&store, "politics");
        assert!(reading.stale);
        assert!(reading.brier.is_none());
        assert_eq!(reading.sample_size, 0);
    }

    #[test]
    fn perfect_predictions_score_zero_brier() {
        let mut store = CalibrationStore::new();
        for _ in 0..4 {
            store.record(
                "test",
                ResolvedObservation {
                    probability: 1.0,
                    outcome: true,
                },
            );
        }
        let brier = store.brier("test").expect("measured");
        assert!(brier.abs() < 1e-9);
    }

    #[test]
    fn coin_flip_predictions_score_quarter_brier() {
        let mut store = CalibrationStore::new();
        for outcome in [true, false] {
            store.record(
                "test",
                ResolvedObservation {
                    probability: 0.5,
                    outcome,
                },
            );
        }
        let brier = store.brier("test").expect("measured");
        assert!((brier - 0.25).abs() < 1e-9);
    }

    #[test]
    fn contains_guards_idempotent_ingest() {
        let mut store = CalibrationStore::new();
        let observation = ResolvedObservation {
            probability: 0.7,
            outcome: true,
        };
        assert!(!store.contains("test", &observation));
        store.record("test", observation);
        assert!(store.contains("test", &observation));
        assert!(!store.contains(
            "test",
            &ResolvedObservation {
                probability: 0.7,
                outcome: false
            }
        ));
        assert_eq!(store.sample_size("test"), 1);
    }

    #[test]
    fn save_load_round_trip_preserves_observations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("calibration.jsonl");
        let mut store = CalibrationStore::new();
        store.record(
            "politics",
            ResolvedObservation {
                probability: 0.8,
                outcome: true,
            },
        );
        store.record(
            "economics",
            ResolvedObservation {
                probability: 0.4,
                outcome: false,
            },
        );
        store.save(&path).expect("save");
        let loaded = CalibrationStore::load(&path).expect("load");
        assert_eq!(loaded.sample_size("politics"), 1);
        assert_eq!(loaded.sample_size("economics"), 1);
        assert!((loaded.brier("politics").expect("brier") - 0.04).abs() < 1e-9);
    }

    #[test]
    fn load_missing_file_is_fresh_store() {
        let store = CalibrationStore::load(std::path::Path::new(
            "/nonexistent/dir/calibration-journal.jsonl",
        ))
        .expect("a missing journal is a fresh store, not an error");
        assert_eq!(store.sample_size("anything"), 0);
        assert_eq!(store.pending_count(), 0);
    }

    #[test]
    fn record_pending_keeps_earliest_snapshot_per_market() {
        let mut store = CalibrationStore::new();
        assert!(store.record_pending(
            "KX-a",
            PendingSnapshot {
                bucket: "economics".into(),
                probability: 0.30
            },
        ));
        // A later (resolution-informed) price must not replace the first
        // observation.
        assert!(!store.record_pending(
            "KX-a",
            PendingSnapshot {
                bucket: "economics".into(),
                probability: 0.95
            },
        ));
        let snapshot = store.take_pending("KX-a").expect("snapshotted");
        assert!((snapshot.probability - 0.30).abs() < 1e-9);
        // Consumed.
        assert!(store.take_pending("KX-a").is_none());
    }

    #[test]
    fn pending_snapshots_persist_alongside_observations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("calibration.jsonl");
        let mut store = CalibrationStore::new();
        store.record(
            "politics",
            ResolvedObservation {
                probability: 0.8,
                outcome: true,
            },
        );
        store.record_pending(
            "KX-a",
            PendingSnapshot {
                bucket: "economics".into(),
                probability: 0.42,
            },
        );
        store.save(&path).expect("save");
        assert!(path.with_file_name("calibration.pending.jsonl").exists());
        let mut loaded = CalibrationStore::load(&path).expect("load");
        assert_eq!(loaded.sample_size("politics"), 1);
        let snapshot = loaded
            .take_pending("KX-a")
            .expect("pending survived restart");
        assert!((snapshot.probability - 0.42).abs() < 1e-9);
        assert_eq!(snapshot.bucket, "economics");
    }
}
