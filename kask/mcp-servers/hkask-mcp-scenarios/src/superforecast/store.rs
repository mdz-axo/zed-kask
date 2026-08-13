//! File-backed persistence for forecasts — append-only journal + periodic
//! snapshot compaction.
//!
//! Extracted from `superforecast.rs` (deep-module split). Each mutation appends
//! one JSON line to the journal (O(1) write). On load, the snapshot is loaded
//! first, then journal entries are replayed on top (last write wins). After
//! `JOURNAL_COMPACT_THRESHOLD` entries, the journal is compacted into a full
//! snapshot.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::types::StoredForecastRecord;

const JOURNAL_COMPACT_THRESHOLD: usize = 100;

/// File-backed persistence using append-only journal + periodic snapshot compaction.
/// Each mutation appends one JSON line to the journal (O(1) write). On load, the
/// snapshot is loaded first, then journal entries are replayed on top (last write wins).
/// After JOURNAL_COMPACT_THRESHOLD entries, the journal is compacted into a full snapshot.
#[derive(Debug, Default)]
pub struct ForecastStore {
    pub records: HashMap<String, StoredForecastRecord>,
    pub data_path: Option<PathBuf>,
    journal_path: Option<PathBuf>,
    journal_count: usize,
}

impl ForecastStore {
    /// Create a new store, loading snapshot + journal replay from disk.
    pub fn new(data_path: Option<PathBuf>) -> Self {
        let journal_path = data_path.as_ref().map(|p| {
            let mut jp = p.clone();
            jp.set_extension("json.journal");
            jp
        });
        let mut store = Self {
            records: HashMap::new(),
            data_path,
            journal_path,
            journal_count: 0,
        };
        store.load();
        store
    }

    /// Load: snapshot first, then replay journal on top (last write wins).
    fn load(&mut self) {
        if let Some(ref path) = self.data_path
            && path.exists()
            && let Ok(data) = fs::read_to_string(path)
            && let Ok(records) =
                serde_json::from_str::<HashMap<String, StoredForecastRecord>>(&data)
        {
            self.records = records;
        }
        if let Some(ref jp) = self.journal_path
            && jp.exists()
            && let Ok(data) = fs::read_to_string(jp)
        {
            for line in data.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<serde_json::Value>(trimmed)
                    && let (Some(key), Some(record)) = (
                        entry.get("key").and_then(|v| v.as_str()),
                        entry.get("record"),
                    )
                    && let Ok(rec) = serde_json::from_value::<StoredForecastRecord>(record.clone())
                {
                    self.records.insert(key.to_string(), rec);
                    self.journal_count += 1;
                }
            }
        }
    }

    /// Append a single record entry to the journal (O(1) write per mutation).
    /// Only writes the changed record, not the full dataset.
    fn save_entry(&self, key: &str, record: &StoredForecastRecord) {
        if let (Some(jp), Some(dp)) = (&self.journal_path, &self.data_path) {
            if let Some(parent) = dp.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                tracing::warn!(target: "hkask.mcp.scenarios.forecast", error = %e, "Failed to create parent dir for forecast journal");
            }
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(jp)
                && let Ok(line) = serde_json::to_string(&serde_json::json!({
                    "key": key,
                    "record": record
                }))
                && let Err(e) = writeln!(file, "{}", line)
            {
                tracing::warn!(target: "hkask.mcp.scenarios.forecast", error = %e, "Failed to append to forecast journal — in-memory state is ahead of disk");
            }
        }
    }

    /// Insert a record and persist via single-entry journal append.
    pub fn insert(&mut self, key: String, record: StoredForecastRecord) {
        self.save_entry(&key, &record);
        self.records.insert(key, record);
        self.journal_count += 1;
        if self.journal_count >= JOURNAL_COMPACT_THRESHOLD {
            self.compact();
        }
    }

    pub fn get(&self, key: &str) -> Option<&StoredForecastRecord> {
        self.records.get(key)
    }

    /// Get mutable reference. Caller must call `persist()` after modification
    /// to durably persist changes, or use `insert` to persist via journal append.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut StoredForecastRecord> {
        self.records.get_mut(key)
    }

    /// Persist all changes (writes full snapshot, truncates journal).
    pub fn persist(&self) {
        self.compact();
    }

    /// Compact: write full snapshot, truncate journal.
    fn compact(&self) {
        if let Some(ref dp) = self.data_path {
            if let Some(parent) = dp.parent()
                && let Err(e) = fs::create_dir_all(parent)
            {
                tracing::warn!(target: "hkask.mcp.scenarios.forecast", error = %e, "Failed to create parent dir for forecast snapshot");
            }
            if let Ok(data) = serde_json::to_string_pretty(&self.records) {
                if let Err(e) = fs::write(dp, data) {
                    tracing::warn!(target: "hkask.mcp.scenarios.forecast", error = %e, "Failed to write forecast snapshot — in-memory state is ahead of disk");
                }
                if let Some(ref jp) = self.journal_path
                    && let Err(e) = fs::write(jp, "")
                {
                    tracing::warn!(target: "hkask.mcp.scenarios.forecast", error = %e, "Failed to truncate forecast journal after compaction");
                }
            }
        }
    }

    /// Force compaction regardless of threshold.
    pub fn force_compact(&self) {
        self.compact();
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if the forecast store contains no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &StoredForecastRecord> {
        self.records.values()
    }

    pub fn resolved(&self) -> Vec<&StoredForecastRecord> {
        self.records
            .values()
            .filter(|r| r.outcome.is_some())
            .collect()
    }

    /// Resolved forecasts matching a domain category (case-insensitive
    /// substring match, mirroring the old `domain_bias_delta` matcher).
    /// Used by per-domain calibration: the bias for a category is computed
    /// only from resolved forecasts in that category.
    pub fn resolved_by_category(&self, category: &str) -> Vec<&StoredForecastRecord> {
        let normalized = category.to_ascii_lowercase();
        self.records
            .values()
            .filter(|r| r.outcome.is_some())
            .filter(|r| {
                r.category
                    .as_deref()
                    .is_some_and(|c| c.to_ascii_lowercase().contains(&normalized))
            })
            .collect()
    }

    pub(crate) fn filtered_by_subject(&self, subject: &str) -> Self {
        Self {
            records: self
                .records
                .iter()
                .filter(|(_, record)| record.subject == subject)
                .map(|(key, record)| (key.clone(), record.clone()))
                .collect(),
            data_path: None,
            journal_path: None,
            journal_count: 0,
        }
    }
}
