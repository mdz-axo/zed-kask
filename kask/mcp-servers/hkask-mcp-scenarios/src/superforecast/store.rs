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
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::types::StoredForecastRecord;

const JOURNAL_COMPACT_THRESHOLD: usize = 100;

/// File-backed persistence using append-only journal + periodic snapshot compaction.
/// Each mutation appends one JSON line to the journal (O(1) write). On load, the
/// snapshot is loaded first, then journal entries are replayed on top (last write wins).
/// After JOURNAL_COMPACT_THRESHOLD entries, the journal is compacted into a full snapshot.
#[derive(Debug, Default)]
pub struct ForecastStore {
    pub(crate) records: HashMap<String, StoredForecastRecord>,
    pub(crate) data_path: Option<PathBuf>,
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
    fn save_entry(&self, key: &str, record: &StoredForecastRecord) -> io::Result<()> {
        if let Some(journal_path) = &self.journal_path {
            if let Some(parent) = journal_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            let mut line = serde_json::to_vec(&serde_json::json!({
                "key": key,
                "record": record
            }))?;
            line.push(b'\n');
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(journal_path)?;
            file.write_all(&line)?;
            file.sync_all()?;
        }
        Ok(())
    }

    /// Insert a record and persist via single-entry journal append.
    pub(crate) fn insert(&mut self, key: String, record: StoredForecastRecord) -> io::Result<()> {
        self.save_entry(&key, &record)?;
        self.records.insert(key, record);
        self.journal_count += 1;
        if self.journal_count >= JOURNAL_COMPACT_THRESHOLD {
            self.compact()?;
        }
        Ok(())
    }

    pub(crate) fn get(&self, key: &str) -> Option<&StoredForecastRecord> {
        self.records.get(key)
    }

    /// expect: "A snapshot failure must not destroy my recovery journal." [P1]
    /// pre: this store exclusively owns writes to its snapshot and journal.
    /// post: the journal is cleared only after a complete snapshot is published;
    /// any persistence failure is returned to the caller.
    pub fn persist(&mut self) -> io::Result<()> {
        self.compact()
    }

    fn compact(&mut self) -> io::Result<()> {
        if let Some(snapshot_path) = &self.data_path {
            let parent = snapshot_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent)?;
            // Same-directory replacement keeps the previous snapshot readable
            // until the complete new snapshot is ready; failed writes clean up
            // their temporary file without touching the recovery journal.
            let data = serde_json::to_vec_pretty(&self.records)?;
            let mut snapshot = tempfile::NamedTempFile::new_in(parent)?;
            snapshot.write_all(&data)?;
            snapshot.as_file().sync_all()?;
            snapshot
                .persist(snapshot_path)
                .map_err(|error| error.error)?;
            // On Unix, persist the rename before discarding its recovery input.
            #[cfg(unix)]
            fs::File::open(parent)?.sync_all()?;
            if let Some(journal_path) = &self.journal_path {
                let journal = fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(journal_path)?;
                journal.sync_all()?;
            }
        }
        self.journal_count = 0;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &StoredForecastRecord> {
        self.records.values()
    }

    pub(crate) fn resolved(&self) -> Vec<&StoredForecastRecord> {
        self.records
            .values()
            .filter(|r| r.outcome.is_some())
            .collect()
    }

    /// Resolved forecasts matching a domain category (case-insensitive
    /// substring match, mirroring the old `domain_bias_delta` matcher).
    /// Used by per-domain calibration: the bias for a category is computed
    /// only from resolved forecasts in that category.
    pub(crate) fn resolved_by_category(&self, category: &str) -> Vec<&StoredForecastRecord> {
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
