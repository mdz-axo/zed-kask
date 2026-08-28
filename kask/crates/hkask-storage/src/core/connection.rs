//! Database connection with SQLCipher encryption.
//!
//! Uses SQLCipher with AES-256-CBC encryption. Passphrases are derived
//! using Argon2id to produce 256-bit encryption keys.
//!
//! # Architecture
//!
//! ```text
//! Database::open(path, passphrase)  →  writes salt file, no SQLite connection
//! Database::connect()               →  creates r2d2 pool with encryption + WAL + schema
//! ```rust,no_run
//!
//! `open()` handles file infrastructure. `connect()` handles everything
//! SQLite-related. One path for each. No dual-path bugs.

use hkask_keystore::derive_key;
use thiserror::Error;

/// Default embedding dimension (configurable via HKASK_EMBEDDING_DIM)
pub(crate) const DEFAULT_EMBEDDING_DIM: usize = 1024;
pub fn embedding_dim() -> usize {
    match std::env::var("HKASK_EMBEDDING_DIM") {
        Ok(raw) => match raw.parse::<usize>() {
            Ok(dim) if dim > 0 => dim,
            _ => {
                tracing::warn!(
                    target: "reg.storage",
                    value = %raw,
                    fallback = DEFAULT_EMBEDDING_DIM,
                    "HKASK_EMBEDDING_DIM malformed or non-positive; using default",
                );
                DEFAULT_EMBEDDING_DIM
            }
        },
        Err(_) => DEFAULT_EMBEDDING_DIM,
    }
}

/// Load the sqlite-vec extension into a single connection.
///
/// Per-connection loading avoids `sqlite3_auto_extension`, whose
/// process-global registration is deprecated on Apple platforms and is a
/// known teardown-segfault source (the sqlite-vec author reports unreliable
/// segfaults from the auto-extension path). Scoping the extension's lifetime
/// to each connection means its state is torn down with the connection, not
/// orphaned at process exit. Must run BEFORE schema init, which creates
/// `vec0` virtual tables.
///
/// SAFETY: `sqlite3_vec_init` is the canonical C entry point
/// `int sqlite3_vec_init(sqlite3*, char**, const sqlite3_api_routines*)`.
/// The `sqlite_vec` crate declares it with no Rust args, so we transmute to
/// the real 3-arg signature and pass a live `sqlite3*` handle from the
/// connection. The two pointer args are NULL (no error message out-param,
/// no custom API routines) — the documented static-link invocation.
#[allow(unsafe_code)]
pub(crate) fn init_sqlite_vec_on(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    type Sqlite3ExtInitFn = unsafe extern "C" fn(
        *mut rusqlite::ffi::sqlite3,
        *mut *mut std::os::raw::c_char,
        *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::os::raw::c_int;
    // SAFETY: transmuting the zero-arg Rust import to the real 3-arg C entry
    // point is the documented sqlite-vec static-link pattern. The handle is
    // live for the duration of the call; the two pointer args are NULL.
    let init_fn: Sqlite3ExtInitFn = unsafe {
        std::mem::transmute::<_, Sqlite3ExtInitFn>(sqlite_vec::sqlite3_vec_init as *const ())
    };
    let rc = unsafe { init_fn(conn.handle(), std::ptr::null_mut(), std::ptr::null()) };
    if rc != rusqlite::ffi::SQLITE_OK {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rc),
            Some(format!("sqlite3_vec_init failed (rc={rc})")),
        ));
    }
    Ok(())
}

pub(crate) const SQLCIPHER_SALT_SIZE: usize = 16;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum DatabaseError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("SQLCipher error: {0}")]
    SqlCipher(String),
    #[error("Key derivation error: {0}")]
    KeyDerivation(String),
    #[error("Passphrase mismatch — database was encrypted with a different passphrase: {0}")]
    PassphraseMismatch(String),
    #[error("Corrupted database — file is not a valid SQLite database: {0}")]
    Corrupted(String),
    /// DB file exists but its salt file is missing. The DB is permanently
    /// unopenable without its original salt — no passphrase can recover it.
    /// Remediable: `open_or_repair` deletes the orphaned DB and recreates.
    #[error("DB file exists at {db_path} but salt file is missing at {salt_path}")]
    SaltMissing {
        db_path: String,
        salt_path: String,
    },
}

/// Database handle — path, passphrase, and whether it's a new file.
///
/// `open()` handles file infrastructure (directories, salt file).
/// `sqlite_pool()` creates an r2d2 pool with SQLCipher encryption, WAL mode,
/// and schema initialization. No dual-path — one method per responsibility.
///
/// The pool is cached after first creation — subsequent calls return the
/// same pool. This prevents the "separate in-memory database per call"
/// pitfall when `Database::in_memory()` is passed around.
pub struct Database {
    path: String,
    passphrase: String,
    extensions: Option<String>,
    /// Cached r2d2 pool — created on first `sqlite_pool()` call.
    pool_cache: std::sync::Mutex<Option<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>>>,
}

impl Database {
    /// Open a database at `path`, creating the salt file if new.
    ///
    /// Validates the passphrase. Creates parent directories. Does NOT
    /// open a SQLite connection — call `connect()` for that.
    fn open_impl(
        path: &str,
        passphrase: &str,
        extensions: Option<&str>,
    ) -> Result<Self, DatabaseError> {
        if passphrase.is_empty() {
            return Err(DatabaseError::KeyDerivation(
                "Passphrase cannot be empty".to_string(),
            ));
        }
        if passphrase.len() < 8 {
            return Err(DatabaseError::KeyDerivation(
                "Passphrase must be at least 8 characters".to_string(),
            ));
        }

        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::SqlCipher(format!(
                    "Failed to create database directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        let salt_path = format!("{}.salt", path);
        let salt_existed = if std::path::Path::new(&salt_path).exists() {
            let salt_bytes = std::fs::read(&salt_path).map_err(|e| {
                DatabaseError::SqlCipher(format!("Failed to read salt file: {}", e))
            })?;
            if salt_bytes.len() != SQLCIPHER_SALT_SIZE {
                return Err(DatabaseError::SqlCipher(
                    "Invalid salt file size".to_string(),
                ));
            }
            true
        } else if std::path::Path::new(path).exists() {
            // The DB file exists but its salt file is missing. The DB was
            // encrypted with the original salt — generating a new salt would
            // create a permanent key mismatch that makes the DB unopenable
            // and that self-healing cannot fix (each heal regenerates another
            // mismatched salt). Return a typed `SaltMissing` error so
            // `open_or_repair` can match on the variant (not a string) and
            // delete the orphaned DB to start fresh.
            return Err(DatabaseError::SaltMissing {
                db_path: path.to_string(),
                salt_path: salt_path.clone(),
            });
        } else {
            let salt = generate_salt();
            std::fs::write(&salt_path, salt)
                .map_err(|e| DatabaseError::SqlCipher(format!("Failed to write salt: {}", e)))?;
            false
        };

        tracing::info!(
            target: "reg.storage",
            operation = "open",
            path = %path,
            is_new = !salt_existed,
            "Database opened"
        );

        Ok(Self {
            path: path.to_string(),
            passphrase: passphrase.to_string(),
            extensions: extensions.map(|s| s.to_string()),
            pool_cache: std::sync::Mutex::new(None),
        })
    }

    pub fn open(path: &str, passphrase: &str) -> Result<Self, DatabaseError> {
        Self::open_impl(path, passphrase, None)
    }

    pub fn open_with_extensions(
        path: &str,
        passphrase: &str,
        extensions: &str,
    ) -> Result<Self, DatabaseError> {
        Self::open_impl(path, passphrase, Some(extensions))
    }

    fn in_memory_impl(extensions: Option<&str>) -> Result<Self, DatabaseError> {
        Ok(Self {
            path: String::from(":memory:"),
            passphrase: String::new(),
            extensions: extensions.map(|s| s.to_string()),
            pool_cache: std::sync::Mutex::new(None),
        })
    }

    pub fn in_memory() -> Result<Self, DatabaseError> {
        Self::in_memory_impl(None)
    }

    pub fn in_memory_with_extensions(extensions: &str) -> Result<Self, DatabaseError> {
        Self::in_memory_impl(Some(extensions))
    }

    fn initialize_schema(conn: &rusqlite::Connection) -> Result<(), DatabaseError> {
        let schema = include_str!("sql/schema.sql");
        let dim = embedding_dim();
        conn.execute_batch(&schema.replace("$DIM", &dim.to_string()))?;
        Self::migrate_embeddings_passage_text(conn)?;
        Ok(())
    }

    /// Migrate existing `embeddings` tables: add `passage_text TEXT` column
    /// if it doesn't exist. `CREATE TABLE IF NOT EXISTS` won't add the column
    /// to an already-existing table, so `ALTER TABLE` is needed for DBs
    /// created before this column was introduced. SQLite has no
    /// `ADD COLUMN IF NOT EXISTS`, so we check `PRAGMA table_info` first.
    fn migrate_embeddings_passage_text(conn: &rusqlite::Connection) -> Result<(), DatabaseError> {
        let mut stmt = conn.prepare("PRAGMA table_info(embeddings)")?;
        let has_column = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let has_column = has_column
            .filter_map(|r| r.ok())
            .any(|name| name == "passage_text");
        if !has_column {
            conn.execute_batch("ALTER TABLE embeddings ADD COLUMN passage_text TEXT;")?;
            tracing::info!(
                target: "reg.storage",
                "Migration: added passage_text column to embeddings table"
            );
        }
        Ok(())
    }

    /// Create an r2d2 connection pool for this database.
    ///
    /// The pool is cached — subsequent calls return the same pool.
    /// This handles:
    /// - SQLCipher encryption (PRAGMA key + header_size for new DBs)
    /// - WAL mode, busy timeout, synchronous=NORMAL, foreign keys, mmap, cache
    /// - Schema initialization on the first connection
    ///
    /// For in-memory databases, creates an unencrypted pool.
    pub fn sqlite_pool(
        &self,
    ) -> Result<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, DatabaseError> {
        {
            let cached = self
                .pool_cache
                .lock()
                .map_err(|e| DatabaseError::SqlCipher(format!("pool lock: {e}")))?;
            if let Some(ref pool) = *cached {
                return Ok(pool.clone());
            }
        }
        let pool = if self.path == ":memory:" {
            self.in_memory_pool()?
        } else {
            self.file_pool()?
        };
        self.pool_cache
            .lock()
            .map_err(|e| DatabaseError::SqlCipher(format!("pool lock: {e}")))?
            .replace(pool.clone());
        Ok(pool)
    }

    /// Run a passive WAL checkpoint, reclaim free pages, and analyze indices.
    ///
    /// Call periodically (e.g. on a maintenance tick) to prevent WAL
    /// checkpoint starvation under long-lived readers and vec0 shadow-table
    /// bloat from re-embedding churn. PASSIVE mode checkpoints as much as
    /// possible without blocking concurrent readers/writers.
    /// `incremental_vacuum` reclaims pages freed by vec0 DELETE operations
    /// (shadow tables are not reclaimed by ordinary VACUUM).
    /// `PRAGMA optimize` refreshes index statistics.
    pub fn checkpoint(&self) -> Result<(), DatabaseError> {
        if self.path == ":memory:" {
            return Ok(());
        }
        let pool = self.sqlite_pool()?;
        let conn = pool
            .get()
            .map_err(|e| DatabaseError::SqlCipher(e.to_string()))?;
        conn.execute_batch(
            "PRAGMA wal_checkpoint(PASSIVE);
             PRAGMA incremental_vacuum;
             PRAGMA optimize;",
        )
        .map_err(|e| DatabaseError::SqlCipher(format!("checkpoint: {e}")))?;
        Ok(())
    }

    fn in_memory_pool(
        &self,
    ) -> Result<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, DatabaseError> {
        // Use max_size(1) because SqliteConnectionManager::memory() creates
        // a separate in-memory database per connection. A pool size >1 would
        // scatter writes across independent databases.
        let manager = r2d2_sqlite::SqliteConnectionManager::memory().with_init(|conn| {
            // Load sqlite-vec per-connection before schema init (vec0 tables).
            init_sqlite_vec_on(conn)?;
            conn.execute_batch("PRAGMA foreign_keys = ON;")
        });
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .build(manager)
            .map_err(|e| DatabaseError::SqlCipher(e.to_string()))?;
        let conn = pool
            .get()
            .map_err(|e| DatabaseError::SqlCipher(e.to_string()))?;
        Self::initialize_schema(&conn)?;
        if let Some(ext) = &self.extensions {
            conn.execute_batch(ext)?;
        }
        Ok(pool)
    }

    fn file_pool(&self) -> Result<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, DatabaseError> {
        let salt_path = format!("{}.salt", self.path);
        let salt_bytes = std::fs::read(&salt_path)
            .map_err(|e| DatabaseError::SqlCipher(format!("Failed to read salt file: {}", e)))?;
        if salt_bytes.len() != SQLCIPHER_SALT_SIZE {
            return Err(DatabaseError::SqlCipher(
                "Invalid salt file size".to_string(),
            ));
        }
        let mut salt = [0u8; SQLCIPHER_SALT_SIZE];
        salt.copy_from_slice(&salt_bytes);

        let key = derive_key(&self.passphrase, &salt)
            .map_err(|e| DatabaseError::KeyDerivation(e.to_string()))?;
        let key_hex = hex::encode(*key);

        // Verify the passphrase with a standalone connection BEFORE creating
        // the pool. A wrong key leaves SQLCipher's native codec in a corrupted
        // state; when the pool later drops that connection during teardown,
        // the codec cleanup can SIGSEGV. By verifying first, the pool only
        // ever holds connections with a validated key.
        {
            let probe = rusqlite::Connection::open(&self.path)
                .map_err(|e| DatabaseError::SqlCipher(format!("probe open: {e}")))?;
            probe.execute_batch("PRAGMA cipher_plaintext_header_size = 32;")?;
            probe.execute_batch(&format!("PRAGMA key = 'x\"{}\"';", key_hex))?;
            probe
                .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
                .map_err(|_| DatabaseError::PassphraseMismatch(self.path.clone()))?;
        }

        let path = self.path.clone();

        let manager = r2d2_sqlite::SqliteConnectionManager::file(&path).with_init(move |conn| {
            // Load sqlite-vec per-connection (before PRAGMA key). The extension
            // only registers its virtual-table module here; it touches no DB
            // pages, so loading before decryption is safe and matches the
            // prior auto-extension timing. Must precede schema init (vec0).
            init_sqlite_vec_on(conn)?;
            // cipher_plaintext_header_size MUST be set on EVERY connection to a
            // database created with it, not only on first creation. SQLCipher
            // reads the salt location from this pragma; omitting it on reopen
            // makes the codec misparse page 1. This MUST run before PRAGMA key
            // because PRAGMA key triggers encryption of page 1.
            conn.execute_batch("PRAGMA cipher_plaintext_header_size = 32;")?;
            conn.execute_batch(&format!("PRAGMA key = 'x\"{}\"';", key_hex))?;
            // Standard WAL PRAGMAs — busy_timeout MUST precede journal_mode = WAL
            // (see super::database::init_wal_pragmas for rationale).
            conn.execute_batch(crate::database::WAL_PRAGMA_BATCH)?;
            // Additional performance tuning for the main registry DB pool.
            conn.execute_batch(
                "PRAGMA synchronous = NORMAL;
                     PRAGMA mmap_size = 268435456;
                     PRAGMA cache_size = -65536;
                     PRAGMA wal_autocheckpoint = 256;
                     PRAGMA auto_vacuum = INCREMENTAL;",
            )
        });

        let pool_size = match std::env::var("HKASK_DB_POOL_SIZE") {
            Ok(raw) => match raw.parse::<u32>() {
                Ok(size) if size > 0 => size,
                _ => {
                    tracing::warn!(
                        target: "reg.storage",
                        value = %raw,
                        fallback = 8,
                        "HKASK_DB_POOL_SIZE malformed or non-positive; using default",
                    );
                    8
                }
            },
            Err(_) => 8,
        };
        let pool = r2d2::Pool::builder()
            .max_size(pool_size)
            .build(manager)
            .map_err(|e| DatabaseError::SqlCipher(e.to_string()))?;

        // Initialize schema on first connection. Also serves as passphrase
        // verification — a wrong passphrase produces an error here.
        let conn = pool.get().map_err(|e| {
            let msg = e.to_string().to_lowercase();
            if msg.contains("file is not a database") || msg.contains("not a database") {
                if std::path::Path::new(&salt_path).exists() {
                    DatabaseError::PassphraseMismatch(self.path.clone())
                } else {
                    DatabaseError::Corrupted(format!("{}: {}", self.path, e))
                }
            } else {
                DatabaseError::SqlCipher(e.to_string())
            }
        })?;
        Self::initialize_schema(&conn)?;
        if let Some(ext) = &self.extensions {
            conn.execute_batch(ext)?;
        }
        Ok(pool)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        // Only emit close for real databases (not :memory:).
        // The pool is dropped here, closing all connections.
        if self.path != ":memory:" {
            tracing::info!(
                target: "reg.storage",
                operation = "close",
                path = %self.path,
                "Database closed"
            );
        }
    }
}

/// expect: "A passphrase mistake never destroys my encrypted database."
/// \[P1\] Motivating: User Sovereignty — user data remains under the user's control.
/// pre: `path` identifies a SQLCipher database and `passphrase` is non-empty.
/// post: returns an opened database only when the passphrase verifies.
/// inv: never deletes or modifies the database or its salt file.
/// \[P4\] Constraining: Clear Boundaries — recovery is an explicit operation, not an implicit side effect.
///
/// The one exception to the "never deletes" invariant: when the DB file
/// exists but its salt file is missing (`SaltMissing`), the DB is permanently
/// unopenable — no passphrase can decrypt it without its original salt. In
/// this case the function deletes the orphaned DB and its WAL/SHM files,
/// then creates a fresh database. This is the "repair" the function name
/// promises. A wrong passphrase does NOT trigger this path — it returns
/// `PassphraseMismatch`, preserving the DB for manual recovery.
pub fn open_or_repair(path: &str, passphrase: &str) -> Result<Database, DatabaseError> {
    match Database::open(path, passphrase) {
        Ok(db) => {
            db.sqlite_pool()?;
            Ok(db)
        }
        Err(DatabaseError::SaltMissing { db_path, salt_path }) => {
            tracing::warn!(
                target: "reg.storage",
                db_path = %db_path,
                salt_path = %salt_path,
                "DB file exists but salt is missing — deleting orphaned DB and creating fresh"
            );
            // Log cleanup errors — a failed delete shouldn't abort the repair
            // (the subsequent open will produce the real error), but the
            // operator must see it to distinguish "couldn't delete" from
            // "couldn't open." `let _ =` would silently swallow per .rules.
            if let Err(e) = std::fs::remove_file(&db_path) {
                tracing::warn!(
                    target: "reg.storage",
                    error = %e,
                    path = %db_path,
                    "Failed to delete orphaned DB file during repair"
                );
            }
            if let Err(e) = std::fs::remove_file(format!("{db_path}-wal")) {
                tracing::warn!(
                    target: "reg.storage",
                    error = %e,
                    path = format!("{db_path}-wal"),
                    "Failed to delete orphaned WAL file during repair"
                );
            }
            if let Err(e) = std::fs::remove_file(format!("{db_path}-shm")) {
                tracing::warn!(
                    target: "reg.storage",
                    error = %e,
                    path = format!("{db_path}-shm"),
                    "Failed to delete orphaned SHM file during repair"
                );
            }
            let db = Database::open(path, passphrase)?;
            db.sqlite_pool()?;
            Ok(db)
        }
        Err(e) => Err(e),
    }
}

pub fn open_database(path: &str, passphrase: &str) -> Result<Database, DatabaseError> {
    if path == ":memory:" {
        Database::in_memory()
    } else {
        // Route file paths through `open_or_repair` so all production callers
        // get the self-healing repair contract — a missing salt file deletes
        // the orphaned DB and recreates instead of permanently breaking.
        // `Database::open` remains the explicit no-repair path for callers
        // that want manual control (rotation, tests).
        open_or_repair(path, passphrase)
    }
}

fn generate_salt() -> [u8; SQLCIPHER_SALT_SIZE] {
    use rand::Rng;
    rand::rng().random()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the self-healing repair: if the DB file exists but its salt file
    /// is missing, `open_or_repair` must delete the orphaned DB and create a
    /// fresh one instead of generating a mismatched salt that makes the DB
    /// permanently unopenable.
    ///
    /// Before the fix, `open_impl` unconditionally generated a new salt when
    /// the salt file was missing — even if the DB file existed and was
    /// encrypted with the original salt. The new salt never matched, so
    /// `file_pool` failed with `PassphraseMismatch` on every heal attempt,
    /// and the self-healing loop could never recover.
    #[test]
    fn open_or_repair_self_heals_when_salt_missing_but_db_exists() {
        let dir = std::env::temp_dir().join(format!(
            "hkask-storage-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("heal_test.db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let salt_path = format!("{db_path_str}.salt");

        // Clean up any prior run.
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);
        let _ = std::fs::remove_file(format!("{db_path_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_path_str}-shm"));

        // 1. Create a valid DB.
        let db = open_or_repair(&db_path_str, "test_passphrase").unwrap();
        drop(db);
        assert!(db_path.exists(), "DB file should exist after initial open");
        assert!(
            std::path::Path::new(&salt_path).exists(),
            "Salt file should exist after initial open"
        );

        // 2. Simulate the failure: delete only the salt file.
        std::fs::remove_file(&salt_path).unwrap();
        assert!(db_path.exists(), "DB file should still exist");
        assert!(
            !std::path::Path::new(&salt_path).exists(),
            "Salt file should be deleted"
        );

        // 3. open_or_repair must self-heal: delete the orphaned DB and create fresh.
        let db = open_or_repair(&db_path_str, "test_passphrase").unwrap();
        drop(db);
        assert!(db_path.exists(), "DB file should exist after heal");
        assert!(
            std::path::Path::new(&salt_path).exists(),
            "Salt file should exist after heal"
        );

        // 4. The healed DB must be openable (not permanently broken).
        let db = open_or_repair(&db_path_str, "test_passphrase").unwrap();
        drop(db);

        // Clean up.
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);
        let _ = std::fs::remove_file(format!("{db_path_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_path_str}-shm"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pin that a wrong passphrase does NOT trigger the self-healing delete
    /// path — a `PassphraseMismatch` must preserve the DB for manual recovery.
    #[test]
    fn open_or_repair_wrong_passphrase_does_not_delete_db() {
        let dir = std::env::temp_dir().join(format!(
            "hkask-storage-test-wp-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("wrong_pass_test.db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let salt_path = format!("{db_path_str}.salt");

        // Clean up any prior run.
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);

        // 1. Create a valid DB with the correct passphrase.
        let db = open_or_repair(&db_path_str, "correct_passphrase").unwrap();
        drop(db);

        // 2. Try to open with a wrong passphrase — must fail, not delete.
        let result = open_or_repair(&db_path_str, "wrong_passphrase");
        assert!(result.is_err(), "Wrong passphrase must fail");
        assert!(
            db_path.exists(),
            "DB file must NOT be deleted on wrong passphrase"
        );
        assert!(
            std::path::Path::new(&salt_path).exists(),
            "Salt file must NOT be deleted on wrong passphrase"
        );

        // 3. The DB must still be openable with the correct passphrase.
        let db = open_or_repair(&db_path_str, "correct_passphrase").unwrap();
        drop(db);

        // Clean up.
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);
        let _ = std::fs::remove_file(format!("{db_path_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_path_str}-shm"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pin that `open_database` (the dispatcher used by `ServerContext`)
    /// routes file paths through `open_or_repair` — so all production callers
    /// get the self-healing repair, not just the 2 that call `open_or_repair`
    /// directly. Before the fix, `open_database` called `Database::open`
    /// directly, bypassing repair.
    #[test]
    fn open_database_self_heals_when_salt_missing() {
        let dir = std::env::temp_dir().join(format!(
            "hkask-storage-test-odb-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("dispatcher_test.db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let salt_path = format!("{db_path_str}.salt");

        // Clean up any prior run.
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);
        let _ = std::fs::remove_file(format!("{db_path_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_path_str}-shm"));

        // 1. Create a valid DB via the dispatcher.
        let db = open_database(&db_path_str, "test_passphrase").unwrap();
        drop(db);
        assert!(db_path.exists());
        assert!(std::path::Path::new(&salt_path).exists());

        // 2. Delete only the salt file.
        std::fs::remove_file(&salt_path).unwrap();

        // 3. open_database must self-heal (via open_or_repair routing).
        let db = open_database(&db_path_str, "test_passphrase").unwrap();
        drop(db);
        assert!(db_path.exists(), "DB file should exist after heal via dispatcher");
        assert!(
            std::path::Path::new(&salt_path).exists(),
            "Salt file should exist after heal via dispatcher"
        );

        // Clean up.
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);
        let _ = std::fs::remove_file(format!("{db_path_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_path_str}-shm"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pin that `:memory:` paths still bypass repair (in-memory DBs have no
    /// salt file and no file to repair).
    #[test]
    fn open_database_memory_path_bypasses_repair() {
        let db = open_database(":memory:", "test_passphrase").unwrap();
        drop(db);
    }
}
