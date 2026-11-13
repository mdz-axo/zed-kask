//! File-backed persistence for records — append-only journal + periodic
//! snapshot compaction.
//!
//! Each mutation appends one JSON event to the journal. A complete declaration
//! occupies one event regardless of member count: replay decodes every member
//! before publishing any. Snapshot plus journal replay remains last-write-wins.
//! After `JOURNAL_COMPACT_THRESHOLD` record mutations, automatic compaction
//! publishes a full snapshot; failure leaves the complete journal recoverable.
//!
//! A snapshot or journal that cannot be read back completely leaves the store
//! unloaded (`load_error`): every write is refused, because the next snapshot
//! would replace the file and drop the records that failed to load.
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

/// The refusal a write returns while the store's files have not loaded.
pub(crate) fn unloaded_error(reason: &str) -> io::Error {
    io::Error::other(reason.to_owned())
}

/// What to tell the caller when `path` did not load.
pub(crate) fn unloaded_reason(path: &Path, what: &str) -> String {
    format!(
        "{} {what}; a write now would replace it and lose its records. Repair the file or move it aside, then restart the server",
        path.display()
    )
}
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
    /// Set when the snapshot or journal did not load completely.
    load_error: Option<String>,
    /// The journal ends in a line with no newline (an append cut short).
    /// The next write compacts first, so nothing is appended after it.
    journal_has_tail: bool,
}

impl<T> Default for JournalStore<T> {
    fn default() -> Self {
        Self {
            records: HashMap::new(),
            data_path: None,
            journal_path: None,
            journal_count: 0,
            clock: chrono::Utc::now,
            load_error: None,
            journal_has_tail: false,
        }
    }
}

impl<T> JournalStore<T> {
    /// Why the store's files did not load, when they did not. While this is
    /// set the in-memory records are incomplete and every write is refused.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
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
    /// A file that does not read back completely sets `load_error`: the store
    /// then refuses writes, since a corrupt snapshot otherwise presents as an
    /// empty store and the next snapshot would replace the file for good.
    fn load(&mut self) {
        if let Some(path) = self.data_path.clone()
            && path.exists()
        {
            match fs::read_to_string(&path) {
                Ok(data) => match serde_json::from_str::<HashMap<String, T>>(&data) {
                    Ok(records) => self.records = records,
                    Err(error) => self.fail_load(&path, &format!("does not parse ({error})")),
                },
                Err(error) => self.fail_load(&path, &format!("cannot be read ({error})")),
            }
        }
        if let Some(journal_path) = self.journal_path.clone()
            && journal_path.exists()
        {
            match fs::read_to_string(&journal_path) {
                Ok(data) => {
                    // Every acknowledged append ends in a newline. Whatever
                    // follows the last newline is an append cut short.
                    let complete_len = data.rfind('\n').map_or(0, |index| index + 1);
                    let (complete, tail) = data.split_at(complete_len);
                    let lines = complete.lines().map(str::trim).filter(|l| !l.is_empty());
                    for (index, line) in lines.enumerate() {
                        match serde_json::from_str::<JournalEntry<T>>(line) {
                            Ok(entry) => self.replay(entry),
                            Err(error) => self.fail_load(
                                &journal_path,
                                &format!("line {} does not parse ({error})", index + 1),
                            ),
                        }
                    }
                    if !tail.trim().is_empty() {
                        self.journal_has_tail = true;
                        // A whole record that lost only its newline is kept;
                        // a fragment was never acknowledged and is dropped.
                        match serde_json::from_str::<JournalEntry<T>>(tail.trim()) {
                            Ok(entry) => self.replay(entry),
                            Err(_) => tracing::warn!(
                                target: "hkask.mcp.scenarios",
                                "Journal ends in an unfinished line; it was never acknowledged and is ignored"
                            ),
                        }
                    }
                }
                Err(error) => self.fail_load(&journal_path, &format!("cannot be read ({error})")),
            }
        }
    }

    fn replay(&mut self, entry: JournalEntry<T>) {
        match entry {
            JournalEntry::Record { key, record } => {
                self.records.insert(key, record);
                self.journal_count += 1;
            }
            JournalEntry::Batch { records } => {
                self.journal_count += records.len();
                self.records.extend(records);
            }
        }
    }

    fn fail_load(&mut self, path: &Path, what: &str) {
        let reason = unloaded_reason(path, what);
        tracing::warn!(target: "hkask.mcp.scenarios", %reason, "Store did not load");
        self.load_error.get_or_insert(reason);
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
        if let Some(reason) = &self.load_error {
            return Err(unloaded_error(reason));
        }
        if records.is_empty() {
            return Ok(());
        }
        if self.journal_has_tail {
            // Publish what loaded and empty the journal, so this append
            // does not land after the unfinished line.
            self.compact()?;
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
        if let Some(reason) = &self.load_error {
            return Err(unloaded_error(reason));
        }
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
        self.journal_has_tail = false;
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
                load_error: None,
                journal_has_tail: false,
            }
        }
    }
}
