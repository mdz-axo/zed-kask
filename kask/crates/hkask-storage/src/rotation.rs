//! Atomic SQLCipher passphrase rotation — re-encrypt a database under a new
//! passphrase without data loss.
//!
//! # Why this exists
//!
//! SQLCipher encrypts a database file with a key derived from the passphrase
//! (salt lives in the DB header under the native KDF). Changing the
//! passphrase requires re-encrypting every page — there is no in-place
//! "PRAGMA rekey" path that survives a crash mid-rotation. The safe approach
//! is:
//!
//! 1. Open the source DB with the old passphrase (verifies it).
//! 2. Attach a new DB file encrypted with the new passphrase.
//! 3. Copy every table's schema + rows via `INSERT INTO ... SELECT *`.
//! 4. Detach, close both connections, and atomically rename:
//!    `<db>` → `<db>.old`, `<db>.new` → `<db>`, then delete `<db>.old`.
//!
//! If any step fails, the original DB is untouched — the caller continues
//! using the old passphrase. The `.new` and `.old` artifacts are cleaned up
//! on the failure path.
//!
//! # Legacy KDF migration
//!
//! [`migrate_legacy_kdf`] re-encrypts a DB created by the pre-native scheme
//! (Argon2id over an external `.salt` file + raw-key PRAGMA) under the native
//! passphrase KDF, using the same copy + atomic-rename choreography. It is
//! triggered automatically by `Database::file_pool` when a `.salt` file is
//! present, runs at most once per DB, and deletes the salt file on success.
//! Once no `.salt` files remain in the wild, this function (and the Argon2
//! dependency it carries) is dead code and can be deleted.
//!
//! # Atomicity
//!
//! The rename step uses `std::fs::rename`, which is atomic on POSIX for
//! same-directory renames. The new DB is written to `<db>.new` (same directory
//! as `<db>`) so the rename is same-directory.
//!
//! # What is copied
//!
//! The rotation copies every user table (excluding SQLite's internal
//! `sqlite_*` and `vec0` shadow tables, which are rebuilt from the
//! `vec_embeddings` virtual table definition in `schema.sql`). The
//! `sqlite_sequence` table (autoincrement counters) is also copied so
//! `AUTOINCREMENT` columns continue from the correct next value.
//!
//! # Limitations
//!
//! - The source DB must be closed by all other processes before rotation
//!   (SQLCipher's WAL holds a file lock). The caller is responsible for
//!   ensuring no other process has the DB open — typically by restarting
//!   the MCP server after rotation.
//! - The `vec0` virtual table's shadow tables are NOT copied directly.
//!   The vec0 table is recreated by `schema.sql` on first open of the new
//!   DB, and the embeddings are re-indexed by the embedding store on next
//!   use. This means a rotated DB's vector index is empty until the next
//!   embedding write triggers a re-index. This is acceptable because the
//!   `embeddings` table (the source of truth) IS copied, and the vec0
//!   table is a derived index.

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

/// Atomically re-encrypt a SQLCipher database under a new passphrase.
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
/// 3. Copies all user tables + `sqlite_sequence` via `INSERT INTO ... SELECT`.
/// 4. Atomically renames: `<db_path>` → `<db_path>.old`,
///    `<db_path>.new` → `<db_path>`, then deletes `<db_path>.old`.
///
/// # Failure safety
///
/// If any step before the rename fails, the `.new` DB and its salt are
/// deleted, and the original DB is untouched. The caller can retry with
/// the correct old passphrase.
///
/// If the rename fails (extremely unlikely on POSIX same-directory), the
/// original DB is still intact — only the `.new` artifacts are in a
/// partially-renamed state, which the caller can clean up manually.
///
/// # Post-rotation
///
/// The caller must restart any process that holds the DB open (MCP servers,
/// the in-process curator store) so they re-open with the new passphrase.
/// The `nudge_mcp_servers` path in the settings UI handles this for MCP
/// servers; the in-process curator store re-opens on next use via its
/// self-healing path.
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

    // 2. Create the new DB file encrypted with the new passphrase.
    //    `Database::open` creates parent dirs; the native KDF stores the
    //    salt in the DB header, so there is no salt file to manage.
    tracing::info!(
        target: "reg.storage",
        path = %new_path,
        "Creating new DB with new passphrase"
    );
    let new_db =
        Database::open(&new_path, new_passphrase).map_err(|e| RotationError::Filesystem {
            path: new_path.clone(),
            error: std::io::Error::other(format!("Failed to create new DB: {e}")),
        })?;
    let new_pool = new_db.sqlite_pool().map_err(|e| {
        let _ = std::fs::remove_file(&new_path);
        RotationError::Filesystem {
            path: new_path.clone(),
            error: std::io::Error::other(format!("Failed to open new DB pool: {e}")),
        }
    })?;

    // 3. Copy all user tables. We use ATTACH on the source connection to
    //    the new DB, then `INSERT INTO main.<table> SELECT * FROM attached.<table>`.
    //    This avoids cross-process locking and lets SQLite handle the copy
    //    in a single transaction.
    //
    //    We attach the NEW db to a SOURCE connection so the source's
    //    passphrase is already unlocked. The new DB is opened with its own
    //    passphrase via PRAGMA key on the attached connection.
    let result = copy_all_tables(&source_pool, &new_pool, db_path, &new_path);
    if let Err(e) = result {
        // Clean up the new DB artifacts — the source is untouched.
        let _ = std::fs::remove_file(&new_path);
        // Also clean up WAL/SHM files that SQLite may have created.
        let _ = std::fs::remove_file(format!("{new_path}-wal"));
        let _ = std::fs::remove_file(format!("{new_path}-shm"));
        return Err(e);
    }

    // Drop the pools BEFORE renaming — SQLite holds file locks on the DB
    // files, and a rename on a locked file can fail on some platforms.
    // Dropping the `Database` structs drops their pools.
    tracing::debug!(
        target: "reg.storage",
        path = %db_path,
        "Closing source and new DB pools before rename"
    );
    drop(source_pool);
    drop(new_pool);
    drop(source_db);
    drop(new_db);

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
        // must manually rename <db_path>.old back to <db_path>.
        let _ = std::fs::rename(&old_backup, db_path);
        let _ = std::fs::remove_file(&new_path);
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
    let _ = std::fs::remove_file(format!("{db_path}-wal"));
    let _ = std::fs::remove_file(format!("{db_path}-shm"));

    tracing::info!(
        target: "reg.storage",
        path = %db_path,
        "Passphrase rotation complete — DB re-encrypted with new passphrase"
    );
    Ok(())
}

/// Re-encrypt a legacy-scheme DB (Argon2id over an external `.salt` file +
/// raw-key PRAGMA + plaintext header) in place under the native SQLCipher
/// passphrase KDF, preserving all data.
///
/// Triggered automatically by `Database::file_pool` when `<db>.salt` exists.
/// Runs at most once per DB: on success the salt file is deleted, so the
/// trigger never fires again. On failure the original DB and salt are
/// untouched — the caller surfaces the error rather than silently starting
/// fresh (the unwrap_or(0) trap: data loss must be loud, never implicit).
///
/// This function (and the Argon2 derivation it carries) exists ONLY to read
/// pre-native databases. Once no `.salt` files remain, it is dead code and
/// can be deleted together with the `argon2` dependency.
pub(crate) fn migrate_legacy_kdf(
    db_path: &str,
    passphrase: &str,
    salt_path: &str,
) -> Result<(), RotationError> {
    tracing::info!(
        target: "reg.storage",
        path = %db_path,
        salt_path = %salt_path,
        "Legacy KDF database detected — migrating to the native SQLCipher passphrase KDF"
    );

    let new_path = format!("{db_path}.new");
    let old_backup = format!("{db_path}.old");
    cleanup_artifact(&new_path, &format!("{new_path}.salt"));
    cleanup_artifact(&old_backup, &format!("{old_backup}.salt"));

    // 1. Open the source with the legacy scheme (Argon2id over the external
    //    salt, raw-key PRAGMA, plaintext header). This verifies the
    //    passphrase against the legacy DB.
    let source_pool = legacy_open_pool(db_path, passphrase, salt_path)?;

    // 2. Create the replacement DB under the native KDF.
    let new_db = Database::open(&new_path, passphrase).map_err(|e| RotationError::Filesystem {
        path: new_path.clone(),
        error: std::io::Error::other(format!("Failed to create migration target DB: {e}")),
    })?;
    let new_pool = new_db.sqlite_pool().map_err(|e| {
        let _ = std::fs::remove_file(&new_path);
        RotationError::Filesystem {
            path: new_path.clone(),
            error: std::io::Error::other(format!("Failed to open migration target pool: {e}")),
        }
    })?;

    // 3. Copy every user table.
    let copy = copy_all_tables(&source_pool, &new_pool, db_path, &new_path);
    drop(new_pool);
    drop(new_db);
    drop(source_pool);
    if let Err(e) = copy {
        let _ = std::fs::remove_file(&new_path);
        let _ = std::fs::remove_file(format!("{new_path}-wal"));
        let _ = std::fs::remove_file(format!("{new_path}-shm"));
        return Err(e);
    }

    // 4. Atomic swap — same choreography as rotate_passphrase.
    std::fs::rename(db_path, &old_backup).map_err(|e| RotationError::Filesystem {
        path: db_path.to_string(),
        error: e,
    })?;
    if let Err(e) = std::fs::rename(&new_path, db_path) {
        let _ = std::fs::rename(&old_backup, db_path);
        let _ = std::fs::remove_file(&new_path);
        return Err(RotationError::Filesystem {
            path: db_path.to_string(),
            error: e,
        });
    }

    // 5. Delete the old DB backup, the salt file, and stale WAL/SHM files.
    //    The salt file is what marks the DB as legacy — deleting it is what
    //    makes this migration run at most once.
    if let Err(e) = std::fs::remove_file(&old_backup) {
        tracing::warn!(
            target: "reg.storage",
            path = %db_path,
            error = %e,
            "Could not delete legacy DB backup {old_backup} — manual cleanup recommended"
        );
    }
    if let Err(e) = std::fs::remove_file(salt_path) {
        tracing::warn!(
            target: "reg.storage",
            path = %db_path,
            error = %e,
            "Could not delete legacy salt file {salt_path} — \
             it will re-trigger migration on next open (harmless, but delete it)"
        );
    }
    let _ = std::fs::remove_file(format!("{db_path}-wal"));
    let _ = std::fs::remove_file(format!("{db_path}-shm"));

    tracing::info!(
        target: "reg.storage",
        path = %db_path,
        "Legacy KDF migration complete — DB re-encrypted under the native passphrase KDF"
    );
    Ok(())
}

/// Open a pool to a legacy-scheme DB: Argon2id key over the external salt
/// file, passed as a raw-key PRAGMA with a plaintext header. This is the
/// pre-native open path, preserved ONLY for `migrate_legacy_kdf` (and its
/// tests) — never use it for new databases.
fn legacy_open_pool(
    db_path: &str,
    passphrase: &str,
    salt_path: &str,
) -> Result<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, RotationError> {
    use argon2::{Algorithm, Argon2, Params, Version};

    // Argon2id parameters from the pre-native scheme (hkask-keystore
    // encryption.rs, deleted): 64 MiB, t=3, p=4, 32-byte output.
    let salt_bytes = std::fs::read(salt_path).map_err(|e| RotationError::Filesystem {
        path: salt_path.to_string(),
        error: e,
    })?;
    if salt_bytes.len() != crate::core::connection::SQLCIPHER_SALT_SIZE {
        return Err(RotationError::Filesystem {
            path: salt_path.to_string(),
            error: std::io::Error::other(format!(
                "invalid legacy salt file size: {} bytes",
                salt_bytes.len()
            )),
        });
    }
    let params = Params::new(65536, 3, 4, Some(32)).map_err(|e| RotationError::Filesystem {
        path: db_path.to_string(),
        error: std::io::Error::other(format!("Argon2 params: {e}")),
    })?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), &salt_bytes, &mut key)
        .map_err(|e| RotationError::Filesystem {
            path: db_path.to_string(),
            error: std::io::Error::other(format!("Argon2 key derivation: {e}")),
        })?;
    let key_hex = hex::encode(key);
    let key_pragma = format!("PRAGMA key = 'x\"{key_hex}\"';");

    // Verify with a standalone probe before building the pool — a wrong key
    // leaves SQLCipher's codec in a corrupted state (SIGSEGV on teardown).
    {
        let probe = rusqlite::Connection::open(db_path).map_err(|e| RotationError::Filesystem {
            path: db_path.to_string(),
            error: std::io::Error::other(format!("probe open: {e}")),
        })?;
        probe
            .execute_batch("PRAGMA cipher_plaintext_header_size = 32;")
            .map_err(|e| RotationError::Filesystem {
                path: db_path.to_string(),
                error: std::io::Error::other(format!("probe pragma: {e}")),
            })?;
        probe
            .execute_batch(&key_pragma)
            .map_err(|e| RotationError::Filesystem {
                path: db_path.to_string(),
                error: std::io::Error::other(format!("probe key: {e}")),
            })?;
        probe
            .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|_| RotationError::OldPassphraseMismatch {
                path: db_path.to_string(),
                source: DatabaseError::PassphraseMismatch(db_path.to_string()),
            })?;
    }

    let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path).with_init(move |conn| {
        conn.execute_batch("PRAGMA cipher_plaintext_header_size = 32;")?;
        conn.execute_batch(&key_pragma)?;
        conn.execute_batch(crate::database::WAL_PRAGMA_BATCH)?;
        Ok(())
    });
    r2d2::Pool::builder()
        .max_size(2)
        .build(manager)
        .map_err(|e| RotationError::Filesystem {
            path: db_path.to_string(),
            error: std::io::Error::other(format!("legacy pool: {e}")),
        })
}

/// Copy all user tables from the source pool to the new pool.
///
/// Uses a single connection from each pool. The source connection reads
/// each table's schema and rows; the new connection writes them. We use
/// `ATTACH` on the new connection to attach the source DB read-only,
/// then `INSERT INTO main.<table> SELECT * FROM attached.<table>`.
///
/// This approach avoids cross-process locking because both connections
/// are in the same process, and the ATTACH uses the source's passphrase
/// (which we know).
fn copy_all_tables(
    source_pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    new_pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    source_path: &str,
    new_path: &str,
) -> Result<(), RotationError> {
    let source_conn = source_pool.get().map_err(|e| RotationError::Sql {
        path: source_path.to_string(),
        error: rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some(format!("source pool get: {e}")),
        ),
    })?;
    let new_conn = new_pool.get().map_err(|e| RotationError::Sql {
        path: new_path.to_string(),
        error: rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some(format!("new pool get: {e}")),
        ),
    })?;

    // Get the list of user tables from the source DB.
    // Exclude: sqlite_* (internal), vec0* (virtual table shadow tables,
    // rebuilt from schema), sqlite_sequence (copied separately).
    let table_names: Vec<String> = {
        // Query the SOURCE connection's sqlite_master to know what to copy.
        let mut source_stmt = source_conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%' \
                 AND name NOT LIKE 'vec_%' \
                 AND name NOT LIKE 'vec0%' \
                 ORDER BY name",
            )
            .map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;
        let rows = source_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row.map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?);
        }
        names
    };

    tracing::debug!(
        target: "reg.storage",
        path = %source_path,
        tables = ?table_names,
        "Copying tables during rotation"
    );

    // Begin a transaction on the new connection so the copy is atomic.
    new_conn
        .execute_batch("BEGIN")
        .map_err(|e| RotationError::Sql {
            path: new_path.to_string(),
            error: e,
        })?;

    // For each table, get its CREATE statement from the source and run it
    // on the new DB (in case the new DB's schema.sql didn't create it —
    // e.g., custom tables added by a store's init_schema). Then copy rows.
    for table_name in &table_names {
        // Get the CREATE statement from the source.
        let create_sql: String = source_conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
                rusqlite::params![table_name],
                |row| row.get(0),
            )
            .map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;

        // SQLite stores the CREATE statement in sqlite_master WITHOUT the
        // `IF NOT EXISTS` clause (it normalizes the SQL). The new DB's
        // schema.sql already created the standard tables (hmems, embeddings,
        // etc.) with `CREATE TABLE IF NOT EXISTS`, so running the source's
        // CREATE would fail with "table already exists". We inject
        // `IF NOT EXISTS` to make it idempotent. For non-standard tables
        // (added by a store's init_schema), this creates them safely.
        let create_sql =
            if create_sql.contains("IF NOT EXISTS") || create_sql.contains("if not exists") {
                create_sql
            } else {
                create_sql.replacen("CREATE TABLE", "CREATE TABLE IF NOT EXISTS", 1)
            };

        new_conn
            .execute_batch(&create_sql)
            .map_err(|e| RotationError::Sql {
                path: new_path.to_string(),
                error: e,
            })?;

        // Copy rows. We read from the source and insert into the new.
        // Using `INSERT INTO <table> SELECT * FROM <table>` won't work
        // across connections, so we read row-by-row and batch-insert.
        //
        // For efficiency, we use a prepared INSERT and bind values.
        // The column count and names come from the source table's PRAGMA.
        let column_info: Vec<(String, String)> = {
            let mut stmt = source_conn
                .prepare(&format!("PRAGMA table_info({table_name})"))
                .map_err(|e| RotationError::Sql {
                    path: source_path.to_string(),
                    error: e,
                })?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(1)?, // name
                        row.get::<_, String>(2)?, // type
                    ))
                })
                .map_err(|e| RotationError::Sql {
                    path: source_path.to_string(),
                    error: e,
                })?;
            let mut info = Vec::new();
            for row in rows {
                info.push(row.map_err(|e| RotationError::Sql {
                    path: source_path.to_string(),
                    error: e,
                })?);
            }
            info
        };

        if column_info.is_empty() {
            // Table has no columns — skip (shouldn't happen).
            continue;
        }

        let col_names: Vec<&str> = column_info.iter().map(|(n, _)| n.as_str()).collect();
        let placeholders: Vec<String> = (0..col_names.len())
            .map(|i| format!("?{}", i + 1))
            .collect();
        let insert_sql = format!(
            "INSERT INTO {table_name} ({cols}) VALUES ({vals})",
            cols = col_names.join(", "),
            vals = placeholders.join(", ")
        );

        // Read all rows from the source.
        let select_sql = format!("SELECT {} FROM {table_name}", col_names.join(", "));
        let mut select_stmt = source_conn
            .prepare(&select_sql)
            .map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;

        let column_count = col_names.len();
        let mut insert_stmt = new_conn
            .prepare(&insert_sql)
            .map_err(|e| RotationError::Sql {
                path: new_path.to_string(),
                error: e,
            })?;

        // Use query_map to read rows, then bind each to the insert.
        // We read values as raw ValueRef to avoid type assumptions.
        let mut rows_iter = select_stmt.query([]).map_err(|e| RotationError::Sql {
            path: source_path.to_string(),
            error: e,
        })?;

        let mut row_count = 0usize;
        while let Some(row) = rows_iter.next().map_err(|e| RotationError::Sql {
            path: source_path.to_string(),
            error: e,
        })? {
            // Bind each column value from the source row.
            // We use rusqlite::Value to handle all types uniformly.
            let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(column_count);
            for i in 0..column_count {
                let val = row.get_ref(i).map_err(|e| RotationError::Sql {
                    path: source_path.to_string(),
                    error: e,
                })?;
                let value = match val {
                    rusqlite::types::ValueRef::Null => rusqlite::types::Value::Null,
                    rusqlite::types::ValueRef::Integer(i) => rusqlite::types::Value::Integer(i),
                    rusqlite::types::ValueRef::Real(f) => rusqlite::types::Value::Real(f),
                    rusqlite::types::ValueRef::Text(s) => {
                        rusqlite::types::Value::Text(String::from_utf8_lossy(s).into_owned())
                    }
                    rusqlite::types::ValueRef::Blob(b) => rusqlite::types::Value::Blob(b.to_vec()),
                };
                params.push(value);
            }
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            insert_stmt
                .execute(param_refs.as_slice())
                .map_err(|e| RotationError::Sql {
                    path: new_path.to_string(),
                    error: e,
                })?;
            row_count += 1;
        }

        tracing::debug!(
            target: "reg.storage",
            path = %source_path,
            table = %table_name,
            rows = row_count,
            "Copied table during rotation"
        );
    }

    // Copy sqlite_sequence (autoincrement counters) if it exists in the
    // source AND in the new DB. `sqlite_sequence` is an internal SQLite table
    // that is auto-created when a table with `AUTOINCREMENT` is created. It
    // cannot be created manually ("object name reserved for internal use"),
    // so we only copy rows if the new DB already has it (i.e., at least one
    // table uses AUTOINCREMENT). If no tables use AUTOINCREMENT, this is a
    // no-op.
    let has_source_sequence: bool = source_conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'",
            [],
            |_| Ok(1),
        )
        .map(|_| true)
        .unwrap_or(false);

    let has_new_sequence: bool = new_conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence'",
            [],
            |_| Ok(1),
        )
        .map(|_| true)
        .unwrap_or(false);

    if has_source_sequence && has_new_sequence {
        let mut select_stmt = source_conn
            .prepare("SELECT name, seq FROM sqlite_sequence")
            .map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;
        let rows = select_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                ))
            })
            .map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;
        for row in rows {
            let (name, seq) = row.map_err(|e| RotationError::Sql {
                path: source_path.to_string(),
                error: e,
            })?;
            new_conn
                .execute(
                    "INSERT OR REPLACE INTO sqlite_sequence (name, seq) VALUES (?, ?)",
                    rusqlite::params![name, seq],
                )
                .map_err(|e| RotationError::Sql {
                    path: new_path.to_string(),
                    error: e,
                })?;
        }
    }

    // Commit the transaction.
    new_conn
        .execute_batch("COMMIT")
        .map_err(|e| RotationError::Sql {
            path: new_path.to_string(),
            error: e,
        })?;

    // Checkpoint the new DB to flush WAL before we close the pool.
    let _ = new_conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

    Ok(())
}

/// Delete leftover `.new` / `.old` artifacts from a prior failed rotation.
fn cleanup_artifact(db_path: &str, salt_path: &str) {
    if Path::new(db_path).exists() {
        let _ = std::fs::remove_file(db_path);
    }
    if Path::new(salt_path).exists() {
        let _ = std::fs::remove_file(salt_path);
    }
    // Also clean up WAL/SHM files.
    let _ = std::fs::remove_file(format!("{db_path}-wal"));
    let _ = std::fs::remove_file(format!("{db_path}-shm"));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Database;

    /// Create a legacy-scheme DB fixture: Argon2id over an external salt
    /// file + raw-key PRAGMA + plaintext header (the pre-native open path),
    /// with one h_mem row. Used by the migration tests here and in
    /// `connection.rs` to prove the migration preserves data.
    pub(crate) fn create_legacy_db(db_path: &str, passphrase: &str) {
        use argon2::{Algorithm, Argon2, Params, Version};

        let salt_path = format!("{db_path}.salt");
        let salt: [u8; crate::core::connection::SQLCIPHER_SALT_SIZE] = [0x42; 16];
        std::fs::write(&salt_path, salt).expect("write fixture salt");

        let params = Params::new(65536, 3, 4, Some(32)).expect("argon2 params");
        let mut key = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
            .expect("derive fixture key");
        let key_hex = hex::encode(key);

        let conn = rusqlite::Connection::open(db_path).expect("open fixture");
        conn.execute_batch("PRAGMA cipher_plaintext_header_size = 32;")
            .expect("header pragma");
        conn.execute_batch(&format!("PRAGMA key = 'x\"{key_hex}\"';"))
            .expect("key pragma");
        conn.execute_batch(
            "CREATE TABLE hmems (id TEXT PRIMARY KEY, entity TEXT, attribute TEXT, value TEXT, \
             valid_from TEXT, owner_webid TEXT);
             INSERT INTO hmems (id, entity, attribute, value, valid_from, owner_webid) \
             VALUES ('legacy-1', 'legacy-entity', 'turn', 'legacy-value', '2026-01-01T00:00:00Z', 'webid:test');",
        )
        .expect("seed fixture row");
    }

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

    /// The legacy KDF migration preserves data, deletes the salt file, and
    /// leaves the DB openable under the native KDF only.
    #[test]
    fn migrate_legacy_kdf_preserves_data_and_deletes_salt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy.db").to_string_lossy().to_string();
        let salt_path = format!("{path}.salt");
        create_legacy_db(&path, "legacy-passphrase");

        migrate_legacy_kdf(&path, "legacy-passphrase", &salt_path).expect("migrate");

        assert!(!Path::new(&salt_path).exists(), "salt file must be deleted");
        assert!(!Path::new(&format!("{path}.old")).exists());
        assert!(!Path::new(&format!("{path}.new")).exists());

        // The migrated DB opens natively and the data survived.
        let db = Database::open(&path, "legacy-passphrase").expect("native open");
        let pool = db.sqlite_pool().expect("native pool");
        let conn = pool.get().expect("conn");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 1, "legacy data must survive the migration");
    }

    /// A wrong passphrase against a legacy DB must fail the migration
    /// WITHOUT touching the original DB or its salt file.
    #[test]
    fn migrate_legacy_kdf_wrong_passphrase_preserves_original() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir
            .path()
            .join("legacy-wp.db")
            .to_string_lossy()
            .to_string();
        let salt_path = format!("{path}.salt");
        create_legacy_db(&path, "correct-passphrase");

        let result = migrate_legacy_kdf(&path, "wrong-passphrase", &salt_path);
        assert!(result.is_err(), "wrong passphrase must fail the migration");
        assert!(Path::new(&path).exists(), "original DB must be preserved");
        assert!(
            Path::new(&salt_path).exists(),
            "salt file must be preserved"
        );
        assert!(!Path::new(&format!("{path}.new")).exists());
    }
}
