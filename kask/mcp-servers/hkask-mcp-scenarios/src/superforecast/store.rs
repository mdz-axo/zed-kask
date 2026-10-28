//! File-backed persistence for records — append-only journal + periodic
//! snapshot compaction.
//!
//! Each mutation appends one JSON event to the journal. A complete declaration
//! occupies one event regardless of member count: replay decodes every member
//! before publishing any. Snapshot plus journal replay remains last-write-wins.
//! After `JOURNAL_COMPACT_THRESHOLD` record mutations, automatic compaction
//! publishes a full snapshot; failure leaves the complete journal recoverable.
//!
//! The generic `JournalStore<T>` core (Phase A P1 of the
//! companies/FMP/EODHD/FIBO removal plan) is the one implementation of the
//! journal+snapshot machinery — the equity forecast store (from the retired
//! companies server) is the second instantiation, not a copied module.
//! `ForecastStore` stays as the `StoredForecastRecord`-typed instantiation
//! with its record-specific accessors (`resolved`,
//! `filtered_by_subject`) so the calibration and composition callers keep
//! their exact shapes.
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Existing single-record history and whole-declaration journal events.
/// A batch is deserialized completely before any member becomes visible.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum JournalEntry<T> {
    Record { key: String, record: T },
    Batch { records: Vec<(String, T)> },
}

const JOURNAL_COMPACT_THRESHOLD: usize = 100;
/// File-backed persistence using append-only journal + periodic snapshot compaction.
/// Each mutation appends one complete event. A batch publishes all members
/// together after syncing the event; loading decodes the entire batch first.
/// Compaction counts record mutations, not independent forecast outcomes.
#[derive(Debug)]
pub struct JournalStore<T> {
    pub(crate) records: HashMap<String, T>,
    pub(crate) data_path: Option<PathBuf>,
    journal_path: Option<PathBuf>,
    journal_count: usize,
    clock: fn() -> chrono::DateTime<chrono::Utc>,
}

impl<T> Default for JournalStore<T> {
    fn default() -> Self {
        Self {
            records: HashMap::new(),
            data_path: None,
            journal_path: None,
            journal_count: 0,
            clock: chrono::Utc::now,
        }
    }
}

impl<T: for<'de> serde::Deserialize<'de> + serde::Serialize> JournalStore<T> {
    /// Inject the external clock boundary without changing tool request data.
    #[doc(hidden)]
    pub fn set_clock(&mut self, clock: fn() -> chrono::DateTime<chrono::Utc>) {
        self.clock = clock;
    }

    pub(crate) fn now(&self) -> chrono::DateTime<chrono::Utc> {
        (self.clock)()
    }
    /// Create a new store, loading snapshot + journal replay from disk.
    pub fn new(data_path: Option<PathBuf>) -> Self {
        let journal_path = data_path.as_ref().map(|p| {
            let mut jp = p.clone();
            jp.set_extension("json.journal");
            jp
        });
        let mut store = Self {
            data_path,
            journal_path,
            ..Self::default()
        };
        store.load();
        store
    }
    /// Load: snapshot first, then replay journal on top (last write wins).
    /// Every failure arm is surfaced: a corrupt snapshot otherwise presents as
    /// an empty store, and the calibration feedback loop reads "no history"
    /// as "no forecasts yet" — a data failure indistinguishable from no data.
    fn load(&mut self) {
        if let Some(ref path) = self.data_path
            && path.exists()
        {
            match fs::read_to_string(path) {
                Ok(data) => match serde_json::from_str::<HashMap<String, T>>(&data) {
                    Ok(records) => self.records = records,
                    Err(error) => tracing::warn!(
                        target: "hkask.mcp.scenarios",
                        %error,
                        "Forecast snapshot is corrupt — replaying the journal only; \
                         calibration history is degraded this session"
                    ),
                },
                Err(error) => tracing::warn!(
                    target: "hkask.mcp.scenarios",
                    %error,
                    "Forecast snapshot is unreadable — replaying the journal only; \
                     calibration history is degraded this session"
                ),
            }
        }
        if let Some(ref jp) = self.journal_path
            && jp.exists()
        {
            match fs::read_to_string(jp) {
                Ok(data) => {
                    let mut skipped_lines = 0usize;
                    for line in data.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<JournalEntry<T>>(trimmed) {
                            Ok(JournalEntry::Record { key, record }) => {
                                self.records.insert(key, record);
                                self.journal_count += 1;
                            }
                            Ok(JournalEntry::Batch { records }) => {
                                self.journal_count += records.len();
                                self.records.extend(records);
                            }
                            Err(_) => skipped_lines += 1,
                        }
                    }
                    if skipped_lines > 0 {
                        tracing::warn!(
                            target: "hkask.mcp.scenarios",
                            skipped_lines,
                            "Journal lines failed to replay and were skipped — those \
                             records are lost to recall this session"
                        );
                    }
                }
                Err(error) => tracing::warn!(
                    target: "hkask.mcp.scenarios",
                    %error,
                    "Forecast journal is unreadable — snapshot-only state; records \
                     journaled after the last compaction are lost this session"
                ),
            }
        }
    }
    /// Append a complete mutation; a declaration batch occupies one journal line.
    fn save_entries(&self, records: &[(String, T)]) -> io::Result<()> {
        if let Some(journal_path) = &self.journal_path {
            if let Some(parent) = journal_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            let event = match records {
                [(key, record)] => serde_json::json!({"key": key, "record": record}),
                _ => serde_json::json!({"records": records}),
            };
            let mut line = serde_json::to_vec(&event)?;
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
    /// Insert one record through the same durable publication boundary.
    pub fn insert(&mut self, key: String, record: T) -> io::Result<()> {
        self.insert_batch(vec![(key, record)])
    }
    /// Sync the complete set in one journal event before publishing membership.
    /// Compaction may fail after publication; all members remain recoverable,
    /// so the caller can finish snapshot publication with an identical retry.
    pub(crate) fn insert_batch(&mut self, records: Vec<(String, T)>) -> io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        self.save_entries(&records)?;
        self.journal_count += records.len();
        self.records.extend(records);
        if self.journal_count >= JOURNAL_COMPACT_THRESHOLD {
            self.compact()?;
        }
        Ok(())
    }
    pub fn get(&self, key: &str) -> Option<&T> {
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
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.records.values()
    }
}
/// The scenario `StoredForecastRecord` store — the journal machinery with the
/// record-specific accessors the calibration and composition callers use.
/// Type alias rather than a newtype so `Debug`/`Default` and the existing
/// `store.get(...)` call sites keep their shapes.
pub type ForecastStore = JournalStore<crate::types::StoredForecastRecord>;
/// Record-specific accessors (kept out of the generic core: they read
/// `StoredForecastRecord` fields).
mod forecast_accessors {
    use super::ForecastStore;
    use crate::types::StoredForecastRecord;
    impl ForecastStore {
        pub(crate) fn resolved(&self) -> Vec<&StoredForecastRecord> {
            self.records
                .values()
                .filter(|r| r.outcome.is_some())
                .collect()
        }
        pub(crate) fn eligible_resolved(&self) -> Vec<&StoredForecastRecord> {
            self.resolved()
                .into_iter()
                .filter(|r| r.declaration.as_ref().is_some_and(|d| d.is_prospective()))
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
                clock: self.clock,
            }
        }
    }
}
