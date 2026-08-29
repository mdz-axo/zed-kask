//! Database connection with SQLCipher encryption.
//!
//! Uses SQLCipher's native passphrase KDF: `PRAGMA key = '<passphrase>'`
//! derives the page key via PBKDF2 inside SQLCipher, and the salt lives in
//! the DB file header — no external key material, no custom KDF.
//!
//! # Architecture
//!
//! ```text
//! Database::open(path, passphrase)  →  validates passphrase, no SQLite connection
//! Database::connect()               →  creates r2d2 pool with encryption + WAL + schema
//! ```rust,no_run
//!
//! `open()` handles file infrastructure. `connect()` handles everything
//! SQLite-related. One path for each. No dual-path bugs.

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
    /// Open a database at `path`, creating parent directories if needed.
    ///
    /// Validates the passphrase. Does NOT open a SQLite connection — call
    /// `sqlite_pool()` for that. Creates no external key material: with the
    /// native SQLCipher KDF the salt lives in the DB file header.
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

        tracing::info!(
            target: "reg.storage",
            operation = "open",
            path = %path,
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
        // SQLCipher native passphrase KDF. The passphrase is passed as a
        // SQL string literal (single quotes doubled) — SQLCipher derives
        // the page key via PBKDF2 internally and stores the salt in the DB
        // header. No external key material exists to lose.
        let escaped = self.passphrase.replace('\'', "''");
        let key_pragma = format!("PRAGMA key = '{escaped}';");

        // Verify the passphrase with a standalone connection BEFORE creating
        // the pool. A wrong key leaves SQLCipher's native codec in a corrupted
        // state; when the pool later drops that connection during teardown,
        // the codec cleanup can SIGSEGV. By verifying first, the pool only
        // ever holds connections with a validated key.
        {
            let probe = rusqlite::Connection::open(&self.path)
                .map_err(|e| DatabaseError::SqlCipher(format!("probe open: {e}")))?;
            probe.execute_batch(&key_pragma)?;
            probe
                .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
                .map_err(|_| DatabaseError::PassphraseMismatch(self.path.clone()))?;
        }

        let path = self.path.clone();

        let manager = r2d2_sqlite::SqliteConnectionManager::file(&path).with_init(move |conn| {
            // Load sqlite-vec per-connection (before schema init — vec0).
            init_sqlite_vec_on(conn)?;
            conn.execute_batch(&key_pragma)?;
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
        // verification — a wrong passphrase produces an error here. The
        // standalone probe above is the authoritative verifier; reaching this
        // branch with "not a database" maps to PassphraseMismatch (never
        // destructive — a corrupt file is preserved for manual recovery).
        let conn = pool.get().map_err(|e| {
            let msg = e.to_string().to_lowercase();
            if msg.contains("file is not a database") || msg.contains("not a database") {
                DatabaseError::PassphraseMismatch(self.path.clone())
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
/// inv: never deletes or modifies the database.
/// \[P4\] Constraining: Clear Boundaries — recovery is an explicit operation, not an implicit side effect.
///
/// With the native SQLCipher KDF there is no external key material to lose,
/// so there is nothing to "repair": a wrong passphrase returns
/// `PassphraseMismatch` (the DB is preserved for manual recovery) and a
/// corrupt file returns `Corrupted`.
pub fn open_or_repair(path: &str, passphrase: &str) -> Result<Database, DatabaseError> {
    let db = Database::open(path, passphrase)?;
    db.sqlite_pool()?;
    Ok(db)
}

pub fn open_database(path: &str, passphrase: &str) -> Result<Database, DatabaseError> {
    if path == ":memory:" {
        Database::in_memory()
    } else {
        // All production file paths go through `open_or_repair` — which, with
        // the native KDF, is open + pool creation.
        open_or_repair(path, passphrase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "hkask-storage-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir.join("test.db").to_string_lossy().to_string()
    }

    /// Native KDF round-trip: open, write, reopen with the same passphrase,
    /// read back. No salt file is ever created.
    #[test]
    fn native_kdf_round_trip_preserves_data_and_creates_no_salt() {
        let db_path = temp_db_path("native-roundtrip");
        let salt_path = format!("{db_path}.salt");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(&salt_path);

        {
            let db = open_or_repair(&db_path, "test_passphrase").unwrap();
            let pool = db.sqlite_pool().unwrap();
            let conn = pool.get().unwrap();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS roundtrip (id INTEGER PRIMARY KEY, v TEXT);
                 INSERT OR REPLACE INTO roundtrip (id, v) VALUES (1, 'hello');",
            )
            .unwrap();
        }
        assert!(
            !std::path::Path::new(&salt_path).exists(),
            "the native KDF must never create a salt file"
        );

        {
            let db = open_or_repair(&db_path, "test_passphrase").unwrap();
            let pool = db.sqlite_pool().unwrap();
            let conn = pool.get().unwrap();
            let v: String = conn
                .query_row("SELECT v FROM roundtrip WHERE id = 1", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                v, "hello",
                "data must survive close/reopen under the native KDF"
            );
        }

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{db_path}-wal"));
        let _ = std::fs::remove_file(format!("{db_path}-shm"));
    }

    /// A wrong passphrase must return PassphraseMismatch and PRESERVE the DB
    /// file — a passphrase mistake never destroys the database.
    #[test]
    fn wrong_passphrase_returns_mismatch_and_preserves_db() {
        let db_path = temp_db_path("wrong-pass");
        let _ = std::fs::remove_file(&db_path);

        {
            let db = open_or_repair(&db_path, "correct_passphrase").unwrap();
            drop(db);
        }
        assert!(std::path::Path::new(&db_path).exists());

        let err = match open_or_repair(&db_path, "wrong_passphrase!!") {
            Err(e) => e,
            Ok(db) => {
                drop(db);
                panic!("wrong passphrase must not open the DB");
            }
        };
        assert!(
            matches!(err, DatabaseError::PassphraseMismatch(_)),
            "wrong passphrase must be PassphraseMismatch, got: {err:?}"
        );
        assert!(
            std::path::Path::new(&db_path).exists(),
            "a wrong passphrase must never delete the DB"
        );

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{db_path}-wal"));
        let _ = std::fs::remove_file(format!("{db_path}-shm"));
    }

    #[test]
    fn open_database_memory_path_bypasses_file_pool() {
        let db = open_database(":memory:", "test_passphrase").unwrap();
        drop(db);
    }
}
