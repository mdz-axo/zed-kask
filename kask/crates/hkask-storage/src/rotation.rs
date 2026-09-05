//! SQLCipher passphrase rotation for a quiesced database.
//!
//! # Preservation
//!
//! SQLCipher's `sqlcipher_export` copies the source schema and rows into a
//! new encrypted attachment, including FTS shadow data, triggers, indexes,
//! rowids, and AUTOINCREMENT counters. The attachment is reopened to load
//! exported virtual-table declarations. Before replacement, the vector index
//! is rebuilt from canonical `embeddings` rows and the destination must pass
//! foreign-key and integrity checks. Invalid vectors fail rather than vanish.
//!
//! Export/check failures leave the source file in place under its old key;
//! rotation-owned connections close before the incomplete copy is removed.
//!
//! # Lifecycle precondition and recovery limits
//!
//! All other consumers, including in-process pools, must be closed BEFORE
//! calling this function. They must reopen with the resulting authoritative
//! key afterwards. Restarting only after rotation does not meet this
//! precondition. Coordinated settings/curator quiescence is not implemented
//! here (core-review T11 remains open).
//!
//! Individual same-directory renames are atomic on POSIX, but the sequence
//! `<db>` → `<db>.old`, `<db>.new` → `<db>` is not a crash-atomic transaction.
//! Restoration is attempted if the second rename fails; a failed restoration
//! names the backup for operator recovery. This API does not provide
//! multi-database/keychain crash atomicity.

use std::path::Path;

use crate::core::connection::{Database, DatabaseError};

/// Error type for passphrase rotation.
#[derive(Debug, thiserror::Error)]
pub enum RotationError {
    /// The old passphrase does not match the existing DB.
    #[error("Old passphrase does not match the database at {path}: {source}")]
    OldPassphraseMismatch {
        path: String,
        #[source]
        source: DatabaseError,
    },
    /// The new passphrase is invalid (too short, empty, etc.).
    #[error("Invalid new passphrase: {0}")]
    InvalidNewPassphrase(String),
    /// A filesystem operation failed during rotation.
    #[error("Filesystem error during rotation of {path}: {error}")]
    Filesystem { path: String, error: std::io::Error },
    /// A SQL operation failed during the copy.
    #[error("SQL error during rotation of {path}: {error}")]
    Sql {
        path: String,
        #[source]
        error: rusqlite::Error,
    },
}

/// Re-encrypt a quiesced SQLCipher database under a new passphrase.
///
/// expect: "My stored data and search results survive a passphrase change" [P1]
/// pre: every other database consumer has closed its handles
/// post: a verified encrypted copy replaces the source; canonical embeddings are indexed
///
/// # Arguments
///
/// - `db_path`: Path to the existing `.db` file (e.g. `curator.db`).
/// - `old_passphrase`: The current passphrase. Must match the DB.
/// - `new_passphrase`: The desired new passphrase. Must be >=8 chars.
///
/// # What happens
///
/// 1. Opens `<db_path>` with `old_passphrase` (verifies it).
/// 2. Creates `<db_path>.new` encrypted with `new_passphrase`.
/// 3. Exports the schema/data, rebuilds KNN, and validates the destination.
/// 4. Renames: `<db_path>` → `<db_path>.old`,
///    `<db_path>.new` → `<db_path>`, then deletes `<db_path>.old`.
///
/// # Failure safety
///
/// If export or verification fails before rename, the incomplete `.new` DB
/// is removed and the original file remains usable under the old key.
///
/// If replacement fails, restoration of `.old` is attempted. A restoration
/// failure logs the backup path that the operator must recover manually.
///
/// # Post-rotation
///
/// The caller must reopen previously closed consumers with the new passphrase
/// after success, or the old passphrase after a pre-replacement failure.
/// This function neither coordinates those consumers nor updates the keychain.
pub fn rotate_passphrase(
    db_path: &str,
    old_passphrase: &str,
    new_passphrase: &str,
) -> Result<(), RotationError> {
    if new_passphrase.is_empty() {
        return Err(RotationError::InvalidNewPassphrase(
            "New passphrase cannot be empty".to_string(),
        ));
    }
    if new_passphrase.len() < 8 {
        return Err(RotationError::InvalidNewPassphrase(format!(
            "New passphrase must be at least 8 characters (got {})",
            new_passphrase.len()
        )));
    }
    if old_passphrase == new_passphrase {
        // No-op — nothing to rotate. Return early so we don't touch the DB.
        tracing::info!(
            target: "reg.storage",
            path = %db_path,
            "Passphrase rotation skipped — old and new passphrases are identical"
        );
        return Ok(());
    }

    let new_path = format!("{db_path}.new");
    let old_backup = format!("{db_path}.old");

    // Clean up any leftover .new/.old artifacts from a prior failed rotation.
    // These are safe to delete because a successful rotation deletes them.
    cleanup_artifact(&new_path, &format!("{new_path}.salt"));
    cleanup_artifact(&old_backup, &format!("{old_backup}.salt"));

    // 1. Open the source DB with the old passphrase. This verifies the
    //    passphrase and gives us a connection to read from.
    tracing::info!(
        target: "reg.storage",
        path = %db_path,
        "Starting passphrase rotation — opening source DB with old passphrase"
    );
    let source_db = Database::open(db_path, old_passphrase).map_err(|e| {
        RotationError::OldPassphraseMismatch {
            path: db_path.to_string(),
            source: e,
        }
    })?;
    // Force pool creation — this is where passphrase verification actually
    // happens (the probe connection in `file_pool` runs `SELECT count(*) FROM
    // sqlite_master`).
    let source_pool =
        source_db
            .sqlite_pool()
            .map_err(|e| RotationError::OldPassphraseMismatch {
                path: db_path.to_string(),
                source: e,
            })?;

    // Export into an empty encrypted destination. Pre-initializing its schema
    // would conflict with the source's virtual tables, triggers, and indexes.
    let result = copy_all_tables(&source_pool, db_path, &new_path, new_passphrase);
    // Close every rotation-owned connection before cleanup or replacement.
    // Other consumers must already have been closed by the caller.
    drop(source_pool);
    drop(source_db);
    if let Err(error) = result {
        remove_artifact(&new_path);
        remove_artifact(&format!("{new_path}-wal"));
        remove_artifact(&format!("{new_path}-shm"));
        return Err(error);
    }

    // 4. Atomically rename. On POSIX, same-directory renames are atomic.
    //    a. Rename old DB → .old
    //    b. Rename new DB → <db_path>
    //    c. Delete .old
    //
    //    If step (b) fails after (a) succeeds, the DB is in a state where
    //    <db_path> doesn't exist but <db_path>.old does. The caller can
    //    manually rename .old back. This is the least-bad failure mode.
    tracing::info!(
        target: "reg.storage",
        path = %db_path,
        "Atomically renaming DB files"
    );

    // a. Rename old DB → .old
    std::fs::rename(db_path, &old_backup).map_err(|e| RotationError::Filesystem {
        path: db_path.to_string(),
        error: e,
    })?;

    // b. Rename new DB → <db_path>
    if let Err(e) = std::fs::rename(&new_path, db_path) {
        // Attempt to restore the old DB. If this also fails, the operator
        // must manually rename <db_path>.old back to <db_path> — say so
        // loudly, or the returned error (which names only the original
        // rename failure) leaves them unaware the restore also failed.
        if let Err(restore_err) = std::fs::rename(&old_backup, db_path) {
            tracing::error!(
                target: "reg.storage",
                path = %db_path,
                backup = %old_backup,
                original_error = %e,
                restore_error = %restore_err,
                "DB restore after failed rotation rename ALSO failed — \
                 manually rename {old_backup} back to {db_path} before restarting"
            );
        }
        remove_artifact(&new_path);
        return Err(RotationError::Filesystem {
            path: db_path.to_string(),
            error: e,
        });
    }

    // c. Delete .old
    if Path::new(&old_backup).exists() {
        if let Err(e) = std::fs::remove_file(&old_backup) {
            tracing::warn!(
                target: "reg.storage",
                path = %db_path,
                error = %e,
                "Could not delete old DB backup {old_backup} — \
                 manual cleanup recommended"
            );
        }
    }

    // Clean up WAL/SHM files from the old DB (they're stale now).
    remove_artifact(&format!("{db_path}-wal"));
    remove_artifact(&format!("{db_path}-shm"));

    tracing::info!(
        target: "reg.storage",
        path = %db_path,
        "Passphrase rotation complete — DB re-encrypted with new passphrase"
    );
    Ok(())
}

/// SQLCipher owns schema export, including contentless FTS shadow tables,
/// indexes, triggers, rowids, and AUTOINCREMENT high-water marks. Rebuilding
/// only the vector index from canonical rows avoids preserving an incomplete
/// derived index. No destination is published until all checks succeed.
fn copy_all_tables(
    source_pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    source_path: &str,
    new_path: &str,
    new_passphrase: &str,
) -> Result<(), RotationError> {
    let connection = source_pool.get().map_err(|error| RotationError::Sql {
        path: source_path.to_string(),
        error: rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some(format!("source pool get: {error}")),
        ),
    })?;
    let export = || -> rusqlite::Result<()> {
        // Export must be free to create/load mutually dependent tables. Check
        // the complete destination afterwards rather than relying on row order.
        connection.pragma_update(None, "foreign_keys", false)?;
        connection.execute(
            "ATTACH DATABASE ?1 AS rotated KEY ?2",
            rusqlite::params![new_path, new_passphrase],
        )?;
        connection.query_row("SELECT sqlcipher_export('rotated')", [], |_| Ok(()))?;
        // Export writes virtual-table declarations through sqlite_schema;
        // reopen the attachment so SQLite loads those declarations before use.
        connection.execute_batch("DETACH DATABASE rotated")?;
        connection.execute(
            "ATTACH DATABASE ?1 AS rotated KEY ?2",
            rusqlite::params![new_path, new_passphrase],
        )?;
        rebuild_vector_index(&connection)?;
        let mut foreign_keys = connection.prepare("PRAGMA rotated.foreign_key_check")?;
        if foreign_keys.query([])?.next()?.is_some() {
            return Err(copy_validation_error(
                "destination foreign-key check failed",
            ));
        }
        drop(foreign_keys);
        let integrity: String =
            connection.query_row("PRAGMA rotated.integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(copy_validation_error(&format!(
                "destination integrity check failed: {integrity}"
            )));
        }
        connection.execute_batch("DETACH DATABASE rotated")?;
        Ok(())
    };
    let result = export();
    // This pool is private to rotation, but leave its connection policy intact
    // on both success and error. Closing the pool also detaches failed exports.
    if let Err(error) = connection.pragma_update(None, "foreign_keys", true) {
        tracing::warn!(target: "reg.storage", error = %error, "Could not restore rotation connection foreign-key policy");
        if result.is_ok() {
            return Err(RotationError::Sql {
                path: new_path.to_string(),
                error,
            });
        }
    }
    result.map_err(|error| RotationError::Sql {
        path: new_path.to_string(),
        error,
    })
}

fn copy_validation_error(message: &str) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
        Some(message.to_string()),
    )
}

fn rebuild_vector_index(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute("DELETE FROM rotated.vec_embeddings", [])?;
    {
        let mut source =
            transaction.prepare("SELECT rowid, vector, dimensions FROM rotated.embeddings")?;
        let mut rows = source.query([])?;
        let mut insert = transaction
            .prepare("INSERT INTO rotated.vec_embeddings(rowid, embedding) VALUES (?1, ?2)")?;
        while let Some(row) = rows.next()? {
            let rowid: i64 = row.get(0)?;
            let vector: Vec<u8> = row.get(1)?;
            let dimensions: i64 = row.get(2)?;
            if dimensions <= 0
                || dimensions.checked_mul(4) != i64::try_from(vector.len()).ok()
                || vector.chunks_exact(4).any(|bytes| {
                    !f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).is_finite()
                })
            {
                return Err(copy_validation_error(&format!(
                    "invalid canonical embedding at rowid {rowid}"
                )));
            }
            insert.execute(rusqlite::params![rowid, vector])?;
        }
    }
    transaction.commit()
}

/// Remove a rotation artifact, ignoring "not found" (the common case —
/// leftovers that were already cleaned) and warning on anything else. A
/// failed cleanup must not abort the rotation, but it must not be silent
/// either: a leftover `.new`/`.old` file or stale WAL is operator-visible
/// state.
fn remove_artifact(path: &str) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                target: "reg.storage",
                path = %path,
                error = %e,
                "failed to remove rotation artifact — manual cleanup may be needed"
            );
        }
    }
}

/// Delete leftover `.new` / `.old` artifacts from a prior failed rotation.
fn cleanup_artifact(db_path: &str, salt_path: &str) {
    if Path::new(db_path).exists() {
        remove_artifact(db_path);
    }
    if Path::new(salt_path).exists() {
        remove_artifact(salt_path);
    }
    // Also clean up WAL/SHM files.
    remove_artifact(&format!("{db_path}-wal"));
    remove_artifact(&format!("{db_path}-shm"));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Database;

    fn make_test_db(dir: &Path, name: &str, passphrase: &str) -> String {
        let path = dir.join(name).to_string_lossy().to_string();
        let db = Database::open(&path, passphrase).expect("open");
        let pool = db.sqlite_pool().expect("pool");
        let conn = pool.get().expect("conn");
        // Insert a test row into hmems.
        conn.execute(
            "INSERT INTO hmems (id, entity, attribute, value, valid_from, owner_webid) \
             VALUES ('test-1', 'test-entity', 'test-attr', 'test-value', '2026-01-01T00:00:00Z', 'webid:test')",
            [],
        )
        .expect("insert");
        // Insert a test row into embeddings.
        let vector = vec![0.1f32; 1024];
        let vector_bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
        conn.execute(
            "INSERT INTO embeddings (id, entity_ref, vector, dimensions, model) \
             VALUES ('emb-1', 'test-entity', ?, 1024, 'test-model')",
            rusqlite::params![vector_bytes],
        )
        .expect("insert embedding");
        path
    }

    /// expect: "Rotating my populated RSS database preserves feeds, search, and future updates" [P1]
    #[test]
    fn rotate_passphrase_preserves_rss_feed_entries_and_search() {
        // Read the actual server DDL without introducing a storage → server dependency.
        let source = include_str!("../../../mcp-servers/hkask-mcp-research/src/research/db.rs");
        let schema = source
            .split_once("pub const RSS_SCHEMA_DDL: &str = \"")
            .expect("RSS DDL declaration")
            .1
            .split_once("\";")
            .expect("RSS DDL end")
            .0;
        let directory = tempfile::tempdir().expect("temporary database");
        let path = directory
            .path()
            .join("rss.db")
            .to_string_lossy()
            .into_owned();
        {
            let database = Database::open_with_extensions(&path, "old-passphrase", schema)
                .expect("RSS database");
            let pool = database.sqlite_pool().expect("RSS pool");
            let connection = pool.get().expect("RSS connection");
            connection.execute_batch(
                "INSERT INTO feeds(id,url,title) VALUES (7,'https://example.test/rss','Example');
                 INSERT INTO subscriptions(feed_id,stream_id) VALUES (7,'feed/example');
                 INSERT INTO entries(id,feed_id,entry_id,title,content) VALUES (42,7,'first','Quasar','Preserved search text');
                 INSERT INTO entry_states(entry_id,is_read) VALUES (42,1);
                 INSERT INTO feeds(id,url) VALUES (100,'https://example.test/deleted');
                 DELETE FROM feeds WHERE id=100;",
            ).expect("populated RSS schema");
        }
        rotate_passphrase(&path, "old-passphrase", "new-passphrase").expect("rotate populated RSS");
        let database = Database::open(&path, "new-passphrase").expect("reopen");
        let pool = database.sqlite_pool().expect("pool");
        let connection = pool.get().expect("connection");
        let entry: i64 = connection
            .query_row(
                "SELECT e.id FROM entries e JOIN entries_fts f ON e.id=f.rowid
             JOIN feeds ON feeds.id=e.feed_id JOIN entry_states s ON s.entry_id=e.id
             WHERE entries_fts MATCH 'Quasar' AND s.is_read=1",
                [],
                |row| row.get(0),
            )
            .expect("preserved search and foreign keys");
        assert_eq!(entry, 42);
        assert!(
            connection
                .prepare("PRAGMA foreign_key_check")
                .expect("foreign key check")
                .query([])
                .expect("check rows")
                .next()
                .expect("check result")
                .is_none()
        );
        connection
            .execute_batch(
                "INSERT INTO feeds(url) VALUES ('https://example.test/next');
             INSERT INTO entries(feed_id,entry_id,title) VALUES (7,'second','Nebula');",
            )
            .expect("post-rotation insert");
        let feed: i64 = connection
            .query_row(
                "SELECT id FROM feeds WHERE url='https://example.test/next'",
                [],
                |row| row.get(0),
            )
            .expect("new feed");
        assert_eq!(feed, 101, "AUTOINCREMENT retains deleted high-water mark");
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM entries_fts WHERE entries_fts MATCH 'Nebula'",
                [],
                |row| row.get(0),
            )
            .expect("insert trigger");
        assert_eq!(count, 1);
        connection
            .execute("DELETE FROM entries WHERE id=42", [])
            .expect("delete trigger");
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM entries_fts WHERE entries_fts MATCH 'Quasar'",
                [],
                |row| row.get(0),
            )
            .expect("deleted search entry");
        assert_eq!(count, 0);
    }

    /// expect: "Old and new memories remain searchable after passphrase rotation" [P1]
    #[test]
    fn rotate_passphrase_preserves_knn_recall_after_reopen_and_new_write() {
        let directory = tempfile::tempdir().expect("temporary database");
        let path = make_test_db(directory.path(), "recall.db", "old-passphrase");
        let query = vec![0.1f32; 1024];
        {
            let database = Database::open(&path, "old-passphrase").expect("open");
            let pool = database.sqlite_pool().expect("pool");
            let connection = pool.get().expect("connection");
            // Non-contiguous rowids detect accidental renumbering during copy.
            connection
                .execute("UPDATE embeddings SET rowid=73 WHERE id='emb-1'", [])
                .expect("row identity");
            connection.execute_batch("INSERT INTO vec_embeddings(rowid,embedding) SELECT rowid,vector FROM embeddings;").expect("initial index");
        }
        rotate_passphrase(&path, "old-passphrase", "new-passphrase").expect("rotate");
        let database = Database::open(&path, "new-passphrase").expect("reopen");
        let driver: std::sync::Arc<dyn crate::database::driver::DatabaseDriver> =
            std::sync::Arc::new(crate::database::sqlite::SqliteDriver::new(
                database.sqlite_pool().expect("pool"),
            ));
        let embeddings =
            crate::EmbeddingStore::from_driver(driver.clone(), 1024).expect("embedding store");
        let before = embeddings.search(&query, 1).expect("old nearest neighbor");
        assert_eq!(
            before
                .first()
                .expect("old memory recalled")
                .embedding
                .entity_ref,
            "test-entity"
        );
        let connection = driver
            .sqlite_pool()
            .expect("SQLite pool")
            .get()
            .expect("connection");
        let rowid: i64 = connection
            .query_row("SELECT rowid FROM embeddings WHERE id='emb-1'", [], |row| {
                row.get(0)
            })
            .expect("rowid");
        assert_eq!(rowid, 73);
        connection.execute_batch(
            "INSERT INTO hmems(id,entity,attribute,value,valid_from,owner_webid) VALUES ('new-memory','new-entity','test','new','2026-09-04T00:00:00Z','webid:test');",
        ).expect("new h_mem");
        drop(connection);
        let mut other = vec![0.0f32; 1024];
        other[0] = 1.0;
        embeddings
            .store("new-entity", &other, "test-model", None)
            .expect("new embedding");
        for (vector, entity) in [(&query, "test-entity"), (&other, "new-entity")] {
            let result = embeddings.search(vector, 1).expect("nearest neighbor");
            assert_eq!(
                result
                    .first()
                    .expect("memory recalled")
                    .embedding
                    .entity_ref,
                entity
            );
            let blob: Vec<u8> = vector
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect();
            let connection = driver
                .sqlite_pool()
                .expect("pool")
                .get()
                .expect("connection");
            let recalled: String = connection
                .query_row(
                    "SELECT h.entity FROM vec_embeddings v JOIN embeddings e ON e.rowid=v.rowid
                 JOIN hmems h ON h.entity=e.entity_ref WHERE v.embedding MATCH ?1 AND v.k=1",
                    [blob],
                    |row| row.get(0),
                )
                .expect("h_mem recall JOIN");
            assert_eq!(recalled, entity);
        }
    }

    /// expect: "Invalid embedding data aborts rotation without changing my database key" [P1]
    #[test]
    fn malformed_embeddings_abort_rotation_before_replacement() {
        for (dimensions, vector) in [
            (1024, vec![0u8; 3]),
            (1, 0.5f32.to_le_bytes().to_vec()),
            (
                1024,
                vec![f32::NAN; 1024]
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect(),
            ),
        ] {
            let directory = tempfile::tempdir().expect("temporary database");
            let path = make_test_db(directory.path(), "invalid.db", "old-passphrase");
            {
                let database = Database::open(&path, "old-passphrase").expect("open");
                let pool = database.sqlite_pool().expect("pool");
                pool.get()
                    .expect("connection")
                    .execute(
                        "UPDATE embeddings SET dimensions=?1, vector=?2",
                        rusqlite::params![dimensions, vector],
                    )
                    .expect("malformed canonical vector fixture");
            }
            let error = rotate_passphrase(&path, "old-passphrase", "new-passphrase")
                .expect_err("invalid vector must fail");
            assert!(matches!(error, RotationError::Sql { .. }));
            assert_eq!(count_hmems(&path, "old-passphrase"), 1);
            assert_eq!(count_embeddings(&path, "old-passphrase"), 1);
            assert!(!Path::new(&format!("{path}.new")).exists());
            assert!(!Path::new(&format!("{path}.old")).exists());
        }
    }

    /// expect: "A foreign-key violation fails preservation checks rather than publishing a broken copy" [P1]
    #[test]
    fn foreign_key_validation_failure_preserves_source() {
        let directory = tempfile::tempdir().expect("temporary database");
        let path = make_test_db(directory.path(), "invalid-fk.db", "old-passphrase");
        {
            let database = Database::open(&path, "old-passphrase").expect("open");
            let pool = database.sqlite_pool().expect("pool");
            pool.get()
                .expect("connection")
                .execute_batch(
                    "PRAGMA foreign_keys=OFF;
                 CREATE TABLE parents(id INTEGER PRIMARY KEY);
                 CREATE TABLE children(parent_id INTEGER REFERENCES parents(id));
                 INSERT INTO children VALUES (1);",
                )
                .expect("invalid foreign-key fixture");
        }
        let error = rotate_passphrase(&path, "old-passphrase", "new-passphrase")
            .expect_err("foreign-key violation");
        assert!(error.to_string().contains("foreign-key check failed"));
        assert_eq!(count_hmems(&path, "old-passphrase"), 1);
        assert!(!Path::new(&format!("{path}.new")).exists());
        assert!(!Path::new(&format!("{path}.old")).exists());
    }

    fn count_hmems(path: &str, passphrase: &str) -> i64 {
        let db = Database::open(path, passphrase).expect("open");
        let pool = db.sqlite_pool().expect("pool");
        let conn = pool.get().expect("conn");
        conn.query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))
            .expect("count")
    }

    fn count_embeddings(path: &str, passphrase: &str) -> i64 {
        let db = Database::open(path, passphrase).expect("open");
        let pool = db.sqlite_pool().expect("pool");
        let conn = pool.get().expect("conn");
        conn.query_row("SELECT count(*) FROM embeddings", [], |row| row.get(0))
            .expect("count")
    }

    #[test]
    fn rotate_passphrase_copies_all_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = make_test_db(dir.path(), "test.db", "old-passphrase");

        // Verify the DB has data.
        assert_eq!(count_hmems(&path, "old-passphrase"), 1);
        assert_eq!(count_embeddings(&path, "old-passphrase"), 1);

        // Rotate.
        rotate_passphrase(&path, "old-passphrase", "new-passphrase").expect("rotate");

        // The old passphrase should no longer work — `sqlite_pool()` verifies
        // the passphrase via a probe connection.
        let old_db = Database::open(&path, "old-passphrase").expect("open struct");
        let old_pool_result = old_db.sqlite_pool();
        assert!(
            old_pool_result.is_err(),
            "old passphrase should fail at pool creation"
        );

        // The new passphrase should work and have all data.
        assert_eq!(count_hmems(&path, "new-passphrase"), 1);
        assert_eq!(count_embeddings(&path, "new-passphrase"), 1);

        // No leftover artifacts.
        assert!(!Path::new(&format!("{path}.old")).exists());
        assert!(!Path::new(&format!("{path}.new")).exists());
        assert!(!Path::new(&format!("{path}.salt.old")).exists());
    }

    #[test]
    fn rotate_passphrase_wrong_old_passphrase_fails_safely() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = make_test_db(dir.path(), "test.db", "correct-old");

        // Try to rotate with the wrong old passphrase.
        let result = rotate_passphrase(&path, "wrong-old", "new-passphrase");
        assert!(result.is_err(), "rotation should fail");

        // The original DB should be intact and usable with the correct passphrase.
        assert_eq!(count_hmems(&path, "correct-old"), 1);

        // No leftover artifacts.
        assert!(!Path::new(&format!("{path}.new")).exists());
        assert!(!Path::new(&format!("{path}.new.salt")).exists());
    }

    #[test]
    fn rotate_passphrase_short_new_passphrase_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = make_test_db(dir.path(), "test.db", "old-passphrase");

        let result = rotate_passphrase(&path, "old-passphrase", "short");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RotationError::InvalidNewPassphrase(_)
        ));

        // Original DB intact.
        assert_eq!(count_hmems(&path, "old-passphrase"), 1);
    }

    #[test]
    fn rotate_passphrase_same_passphrase_is_noop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = make_test_db(dir.path(), "test.db", "same-passphrase");

        let result = rotate_passphrase(&path, "same-passphrase", "same-passphrase");
        assert!(result.is_ok());

        // DB unchanged.
        assert_eq!(count_hmems(&path, "same-passphrase"), 1);
    }

    #[test]
    fn rotate_passphrase_cleans_up_prior_artifacts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = make_test_db(dir.path(), "test.db", "old-passphrase");

        // Simulate leftover artifacts from a prior failed rotation.
        std::fs::write(format!("{path}.new"), b"garbage").expect("write");
        std::fs::write(format!("{path}.old"), b"garbage-old").expect("write");

        // Rotate — should clean up artifacts first.
        rotate_passphrase(&path, "old-passphrase", "new-passphrase").expect("rotate");

        assert_eq!(count_hmems(&path, "new-passphrase"), 1);
        assert!(!Path::new(&format!("{path}.old")).exists());
        assert!(!Path::new(&format!("{path}.new")).exists());
    }
}
