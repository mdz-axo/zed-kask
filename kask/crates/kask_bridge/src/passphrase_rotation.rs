//! Startup-coordinated database passphrase rotation (D9).
//!
//! A passphrase change is scheduled from the Security page and applied at
//! the next editor startup, before any consumer opens a database:
//!
//! 1. `schedule_db_passphrase_rotation` stores the new passphrase in the
//!    keychain pending slot and writes the confirmed inventory's rotate
//!    paths to the pending record (the passphrase never touches disk).
//! 2. `run_pending_db_passphrase_rotation` — called once at startup, before
//!    any database is opened — classifies every confirmed database (old key
//!    → rotate, new key → already done, neither → manual recovery), rotates
//!    with rollback, and only after every database is consistent writes the
//!    new passphrase to the main keychain slot and deletes the pending
//!    intent. The keychain write is always last: a partial rotation plus an
//!    updated keychain is the one state that leaves databases unopenable.
//!
//! The classification step is what makes a crash between the rotation and
//! the keychain write recoverable: on the next startup the databases that
//! already carry the new key are skipped, the rest are rotated, and the
//! keychain catches up. A failed attempt records its error in the pending
//! record (the Security page surfaces it) and keeps the intent.

use hkask_keystore::keychain::{Keychain, resolve_db_passphrase_string};
use hkask_keystore::keychain_keys::{KEY_DB_PASSPHRASE, KEY_DB_PASSPHRASE_PENDING};
use hkask_storage::ConfirmedInventory;
use std::path::{Path, PathBuf};

/// Location of the pending-rotation record, under the hKask data dir and
/// beside the maintenance catalogue. Never holds the passphrase itself —
/// only the confirmed rotate paths and the last application error.
const PENDING_ROTATION_RELATIVE_PATH: &str = "maintenance/pending-db-rotation.json";

/// Error type for scheduling and applying a coordinated passphrase rotation.
#[derive(Debug, thiserror::Error)]
pub enum PassphraseRotationError {
    /// The new passphrase is empty or shorter than the storage layer accepts.
    #[error("Invalid new passphrase: {0}")]
    InvalidNewPassphrase(String),
    /// A keychain access failed (other than the documented absence cases).
    #[error("Keychain access failed: {0}")]
    Keychain(#[from] hkask_keystore::keychain::KeychainError),
    /// The pending slot and the pending record disagree — recover via the
    /// Security page's cancel action or by scheduling again.
    #[error(
        "The pending passphrase change is inconsistent ({detail}) — cancel the \
         scheduled change on the Security page, or reconfirm the inventory and \
         schedule it again"
    )]
    Inconsistent { detail: String },
    /// The pending-rotation record could not be read or written.
    #[error("Pending-rotation record error: {0}")]
    PendingRecord(String),
    /// A database exists but opens with neither the current nor the
    /// scheduled passphrase — rotation refuses to guess.
    #[error(
        "Database {path} opens with neither the current nor the scheduled \
         passphrase — manual recovery is required before the change can apply"
    )]
    NeitherKeyOpens { path: PathBuf },
    /// A rotation failed; databases rotated in this attempt were rolled
    /// back to the old passphrase (a failed rollback is named in `message`).
    #[error("{message}")]
    RotationFailed { message: String },
}

/// The durable state of a scheduled, not-yet-applied rotation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PendingRotationState {
    pub rotate_paths: Vec<PathBuf>,
    /// The error from the last application attempt, if any — surfaced by
    /// the Security page so a repeatedly-failing change cannot sit silent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// What `apply_db_rotation` did, for honest startup logging.
#[derive(Debug)]
pub struct RotationSummary {
    pub rotated: Vec<PathBuf>,
    pub already_new: Vec<PathBuf>,
    pub missing: Vec<PathBuf>,
    pub same_passphrase: bool,
}

/// Absolute path of the pending-rotation record.
pub fn pending_rotation_path() -> PathBuf {
    hkask_types::agent_paths::resolve_under_data_dir(Path::new(PENDING_ROTATION_RELATIVE_PATH))
}

/// Schedule a passphrase change: persist the pending intent (keychain slot
/// plus record file). Nothing is rotated and the main keychain slot is not
/// touched here — that happens at the next startup via
/// `run_pending_db_passphrase_rotation`.
pub fn schedule_db_passphrase_rotation(
    new_passphrase: &str,
    receipt: &ConfirmedInventory,
) -> Result<(), PassphraseRotationError> {
    let trimmed = new_passphrase.trim();
    if trimmed.is_empty() {
        return Err(PassphraseRotationError::InvalidNewPassphrase(
            "the new passphrase cannot be empty".to_string(),
        ));
    }
    if new_passphrase.len() < 8 {
        return Err(PassphraseRotationError::InvalidNewPassphrase(format!(
            "the new passphrase must be at least 8 characters (got {})",
            new_passphrase.len()
        )));
    }
    // The record is written first: a schedule that fails partway leaves a
    // record the strict reader surfaces for cancellation, never a phantom
    // pending slot.
    write_pending_record(
        &pending_rotation_path(),
        &PendingRotationState {
            rotate_paths: receipt.rotate_paths().to_vec(),
            last_error: None,
        },
    )?;
    Keychain.store_by_key(KEY_DB_PASSPHRASE_PENDING, new_passphrase)?;
    Ok(())
}

/// Read the scheduled-rotation state, if one exists. Both halves of the
/// intent (keychain slot and record file) must agree; a half-written
/// intent is surfaced as `Inconsistent` rather than guessed about.
pub fn read_pending_rotation_state() -> Result<Option<PendingRotationState>, PassphraseRotationError>
{
    let record = read_pending_record(&pending_rotation_path())?;
    let slot = pending_slot_present()?;
    match (slot, record) {
        (false, None) => Ok(None),
        (true, Some(record)) => Ok(Some(record)),
        (true, None) => Err(PassphraseRotationError::Inconsistent {
            detail: "the pending keychain entry has no pending-rotation record".to_string(),
        }),
        (false, Some(_)) => Err(PassphraseRotationError::Inconsistent {
            detail: "the pending-rotation record has no pending keychain entry".to_string(),
        }),
    }
}

/// Cancel a scheduled change: delete both halves of the intent. Absent
/// halves are not an error — cancelling twice must stay idempotent.
pub fn cancel_pending_db_rotation() -> Result<(), PassphraseRotationError> {
    match Keychain.delete_by_key(KEY_DB_PASSPHRASE_PENDING) {
        Ok(()) | Err(hkask_keystore::keychain::KeychainError::NotFound(_)) => {}
        Err(error) => return Err(error.into()),
    }
    if let Err(error) = std::fs::remove_file(pending_rotation_path()) {
        if error.kind() != std::io::ErrorKind::NotFound {
            return Err(PassphraseRotationError::PendingRecord(format!(
                "could not remove the pending-rotation record: {error}"
            )));
        }
    }
    Ok(())
}

/// Apply a scheduled rotation at editor startup, before any database is
/// opened. Returns `Ok(false)` when nothing is scheduled.
///
/// Ordering invariant (module docs): every database finishes rotating
/// before the keychain carries the new passphrase. On failure the
/// databases and keychain keep the old passphrase, the pending intent
/// survives with its recorded error, and the next startup retries.
pub fn run_pending_db_passphrase_rotation() -> Result<bool, PassphraseRotationError> {
    let Some(state) = read_pending_rotation_state()? else {
        return Ok(false);
    };
    let new_passphrase = Keychain.retrieve_by_key(KEY_DB_PASSPHRASE_PENDING)?;
    let old_passphrase = resolve_db_passphrase_string()?;
    let summary = match apply_db_rotation(&old_passphrase, &new_passphrase, &state.rotate_paths) {
        Ok(summary) => summary,
        Err(error) => {
            let detail = error.to_string();
            if let Err(record_error) = write_pending_record(
                &pending_rotation_path(),
                &PendingRotationState {
                    rotate_paths: state.rotate_paths.clone(),
                    last_error: Some(detail),
                },
            ) {
                tracing::warn!(
                    target: "hkask.identity",
                    error = %record_error,
                    "could not record the pending-rotation failure"
                );
            }
            return Err(error);
        }
    };
    // Rotation complete — NOW the keychain may carry the new passphrase.
    // Writing it before every database is consistent is the one state that
    // leaves databases unopenable; this ordering is the whole invariant.
    Keychain.store_by_key(KEY_DB_PASSPHRASE, &new_passphrase)?;
    cancel_pending_db_rotation()?;
    tracing::info!(
        target: "hkask.identity",
        rotated = summary.rotated.len(),
        already_new = summary.already_new.len(),
        missing = summary.missing.len(),
        same_passphrase = summary.same_passphrase,
        "Applied the scheduled database passphrase rotation"
    );
    Ok(true)
}

/// Classify and rotate the confirmed databases from `old_passphrase` to
/// `new_passphrase`. Pure core of the startup runner — explicit keys and
/// paths keep it testable against disposable databases.
pub fn apply_db_rotation(
    old_passphrase: &str,
    new_passphrase: &str,
    paths: &[PathBuf],
) -> Result<RotationSummary, PassphraseRotationError> {
    if old_passphrase == new_passphrase {
        // Nothing to rotate — the scheduled value matches the current one.
        return Ok(RotationSummary {
            rotated: Vec::new(),
            already_new: paths.to_vec(),
            missing: Vec::new(),
            same_passphrase: true,
        });
    }
    let mut to_rotate = Vec::new();
    let mut already_new = Vec::new();
    let mut missing = Vec::new();
    for path in paths {
        if !path.is_file() {
            missing.push(path.clone());
            continue;
        }
        let path_str = path.to_string_lossy().into_owned();
        if hkask_storage::verify_database_key(&path_str, old_passphrase).is_ok() {
            to_rotate.push(path.clone());
        } else if hkask_storage::verify_database_key(&path_str, new_passphrase).is_ok() {
            // A previous attempt rotated this database before failing (or
            // before the keychain write) — skip it and let the keychain
            // catch up once the rest are done.
            already_new.push(path.clone());
        } else {
            return Err(PassphraseRotationError::NeitherKeyOpens { path: path.clone() });
        }
    }
    let mut rotated: Vec<PathBuf> = Vec::new();
    for path in &to_rotate {
        let path_str = path.to_string_lossy().into_owned();
        if let Err(error) =
            hkask_storage::rotate_passphrase(&path_str, old_passphrase, new_passphrase)
        {
            // Roll back the databases this attempt already moved so the
            // system is consistent on the old passphrase again. A rollback
            // failure is named loudly — those databases stay on the NEW
            // passphrase and the keychain must not follow them.
            let mut rollback_failures = Vec::new();
            for done in &rotated {
                let done_str = done.to_string_lossy().into_owned();
                if let Err(rollback_error) =
                    hkask_storage::rotate_passphrase(&done_str, new_passphrase, old_passphrase)
                {
                    rollback_failures.push(format!("{}: {rollback_error}", done.display()));
                }
            }
            let mut message = format!("rotation of {} failed: {error}", path.display());
            if !rollback_failures.is_empty() {
                message.push_str(&format!(
                    " — ROLLBACK ALSO FAILED for {} (these databases remain on the \
                     NEW passphrase; do not save it until they are manually \
                     re-encrypted): {}",
                    rotated
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    rollback_failures.join("; ")
                ));
            }
            return Err(PassphraseRotationError::RotationFailed { message });
        }
        rotated.push(path.clone());
    }
    Ok(RotationSummary {
        rotated,
        already_new,
        missing,
        same_passphrase: false,
    })
}

fn write_pending_record(
    record_path: &Path,
    state: &PendingRotationState,
) -> Result<(), PassphraseRotationError> {
    if let Some(parent) = record_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            PassphraseRotationError::PendingRecord(format!(
                "could not create {}: {error}",
                parent.display()
            ))
        })?;
    }
    let body = serde_json::to_string_pretty(state).map_err(|error| {
        PassphraseRotationError::PendingRecord(format!("could not encode: {error}"))
    })?;
    std::fs::write(record_path, body).map_err(|error| {
        PassphraseRotationError::PendingRecord(format!(
            "could not write {}: {error}",
            record_path.display()
        ))
    })
}

fn read_pending_record(
    record_path: &Path,
) -> Result<Option<PendingRotationState>, PassphraseRotationError> {
    match std::fs::read_to_string(record_path) {
        Ok(body) => serde_json::from_str(&body).map(Some).map_err(|error| {
            PassphraseRotationError::PendingRecord(format!(
                "could not parse {}: {error}",
                record_path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PassphraseRotationError::PendingRecord(format!(
            "could not read {}: {error}",
            record_path.display()
        ))),
    }
}

fn pending_slot_present() -> Result<bool, PassphraseRotationError> {
    match Keychain.retrieve_by_key(KEY_DB_PASSPHRASE_PENDING) {
        Ok(_) => Ok(true),
        Err(hkask_keystore::keychain::KeychainError::NotFound(_)) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::Database;

    fn make_test_db(dir: &Path, name: &str, passphrase: &str) -> PathBuf {
        let path = dir.join(name);
        let db = Database::open(path.to_string_lossy().as_ref(), passphrase).expect("open");
        let pool = db.sqlite_pool().expect("pool");
        let conn = pool.get().expect("conn");
        conn.execute(
            "INSERT INTO hmems (id, entity, attribute, value, valid_from, owner_webid) \
             VALUES ('probe-1', 'e', 'a', 'v', '2026-01-01T00:00:00Z', 'webid:test')",
            [],
        )
        .expect("seed row");
        path
    }

    fn opens_with(path: &Path, passphrase: &str) -> bool {
        hkask_storage::verify_database_key(&path.to_string_lossy(), passphrase).is_ok()
    }

    fn probe(path: &Path, passphrase: &str) -> String {
        hkask_storage::verify_database_key(&path.to_string_lossy(), passphrase)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "opens".to_string())
    }

    /// expect: "Changing my passphrase keeps every database's data" [P1]
    #[test]
    fn apply_rotates_every_confirmed_database_and_preserves_rows() {
        let directory = tempfile::tempdir().expect("temporary databases");
        let first = make_test_db(directory.path(), "first.db", "old-passphrase");
        let second = make_test_db(directory.path(), "second.db", "old-passphrase");
        let summary = apply_db_rotation(
            "old-passphrase",
            "new-passphrase",
            &[first.clone(), second.clone()],
        )
        .expect("rotate both");
        assert_eq!(summary.rotated.len(), 2);
        assert!(summary.already_new.is_empty());
        for path in [&first, &second] {
            assert!(opens_with(path, "new-passphrase"));
            assert!(!opens_with(path, "old-passphrase"));
            let db = Database::open(&path.to_string_lossy(), "new-passphrase").expect("reopen");
            let pool = db.sqlite_pool().expect("pool");
            let conn = pool.get().expect("conn");
            let count: i64 = conn
                .query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))
                .expect("rows survived");
            assert_eq!(count, 1);
        }
    }

    /// A crash between rotating and the keychain write must be completable,
    /// not a wedge: the next run skips what already carries the new key.
    #[test]
    fn apply_completes_a_crashed_rotation_and_skips_already_rotated_databases() {
        let directory = tempfile::tempdir().expect("temporary databases");
        let first = make_test_db(directory.path(), "first.db", "old-passphrase");
        let second = make_test_db(directory.path(), "second.db", "old-passphrase");
        hkask_storage::rotate_passphrase(
            &first.to_string_lossy(),
            "old-passphrase",
            "new-passphrase",
        )
        .expect("pre-rotate first");
        let summary = apply_db_rotation(
            "old-passphrase",
            "new-passphrase",
            &[first.clone(), second.clone()],
        )
        .expect("complete the interrupted rotation");
        assert_eq!(summary.already_new, vec![first.clone()]);
        assert_eq!(summary.rotated, vec![second.clone()]);
        assert!(opens_with(&first, "new-passphrase"));
        assert!(opens_with(&second, "new-passphrase"));
    }

    /// expect: "A failed passphrase change leaves my data exactly as it was" [P1]
    #[test]
    fn apply_rolls_back_to_the_old_passphrase_when_one_rotation_fails() {
        let directory = tempfile::tempdir().expect("temporary databases");
        let first = make_test_db(directory.path(), "first.db", "old-passphrase");
        let blocked = make_test_db(directory.path(), "blocked.db", "old-passphrase");
        // Seed an invalid embedding vector (blob shorter than `dimensions`
        // implies): the rotation copy rebuilds the vector index from
        // canonical rows and invalid vectors fail rather than vanish, so
        // `blocked` opens normally with the old key but fails mid-rotation
        // — the rollback-triggering case.
        {
            let db = Database::open(&blocked.to_string_lossy(), "old-passphrase").expect("open");
            let pool = db.sqlite_pool().expect("pool");
            let conn = pool.get().expect("conn");
            conn.execute(
                "INSERT INTO embeddings (id, entity_ref, vector, dimensions, model) \
                 VALUES ('bad-1', 'e', x'00112233', 1024, 'm')",
                [],
            )
            .expect("seed invalid vector");
        }
        let error = apply_db_rotation(
            "old-passphrase",
            "new-passphrase",
            &[first.clone(), blocked.clone()],
        )
        .expect_err("rotation refuses on invalid vectors");
        assert!(
            error.to_string().contains("blocked.db"),
            "the failure names the database: {error}"
        );
        assert!(
            opens_with(&first, "old-passphrase"),
            "rolled back: {}",
            probe(&first, "old-passphrase")
        );
        assert!(
            opens_with(&blocked, "old-passphrase"),
            "untouched: {}",
            probe(&blocked, "old-passphrase")
        );
    }

    #[test]
    fn apply_rejects_a_database_that_neither_key_opens() {
        let directory = tempfile::tempdir().expect("temporary databases");
        let normal = make_test_db(directory.path(), "normal.db", "old-passphrase");
        let foreign = make_test_db(directory.path(), "foreign.db", "unrelated-key");
        let error = apply_db_rotation(
            "old-passphrase",
            "new-passphrase",
            &[normal.clone(), foreign.clone()],
        )
        .expect_err("classification refuses to guess");
        assert!(matches!(
            error,
            PassphraseRotationError::NeitherKeyOpens { .. }
        ));
        assert!(opens_with(&normal, "old-passphrase"), "nothing rotated");
        assert!(opens_with(&foreign, "unrelated-key"), "untouched");
    }

    #[test]
    fn apply_reports_nothing_to_do_for_the_same_passphrase() {
        let directory = tempfile::tempdir().expect("temporary databases");
        let db = make_test_db(directory.path(), "same.db", "old-passphrase");
        let summary =
            apply_db_rotation("old-passphrase", "old-passphrase", &[db.clone()]).expect("no-op");
        assert!(summary.same_passphrase);
        assert!(summary.rotated.is_empty());
        assert!(opens_with(&db, "old-passphrase"));
    }

    #[test]
    fn pending_record_round_trips_through_disk() {
        let directory = tempfile::tempdir().expect("pending record");
        let record = directory.path().join("pending-db-rotation.json");
        assert!(
            read_pending_record(&record)
                .expect("absent record")
                .is_none()
        );
        let state = PendingRotationState {
            rotate_paths: vec![PathBuf::from("/tmp/example.db")],
            last_error: None,
        };
        write_pending_record(&record, &state).expect("write record");
        let read = read_pending_record(&record)
            .expect("read record")
            .expect("present");
        assert_eq!(read.rotate_paths, state.rotate_paths);
        assert!(read.last_error.is_none());
        std::fs::remove_file(&record).expect("remove record");
        assert!(
            read_pending_record(&record)
                .expect("absent record")
                .is_none()
        );
    }
}
