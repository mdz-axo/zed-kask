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
    #[error("Database maintenance lease unavailable for {path}: {reason}")]
    MaintenanceLease { path: String, reason: String },
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
    maintenance_lease: Option<std::sync::Arc<std::fs::File>>,
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
            maintenance_lease: None,
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
            maintenance_lease: None,
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
        Self::migrate_hmems_forgetting_spec(conn)?;
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

    /// Migrate existing `hmems` tables to the forgetting spec (operator
    /// ruling 2026-09-04): there is no "expired" state — memories age
    /// and are forgotten or deleted, and forgotten means deleted from the
    /// database. Rows carrying a `valid_to` timestamp (the former
    /// soft-delete marker) are forgotten rows under the old mechanism:
    /// delete them, then drop the column. `CREATE TABLE IF NOT EXISTS`
    /// won't remove the column from an already-existing table, so
    /// `ALTER TABLE ... DROP COLUMN` is needed for DBs created before the
    /// ruling. Deletion and schema change commit together; a failed column
    /// drop must not leave a partially migrated database.
    fn migrate_hmems_forgetting_spec(conn: &rusqlite::Connection) -> Result<(), DatabaseError> {
        // Serialize inspection with mutation so concurrent openers cannot
        // both decide to drop the column from the same schema version.
        let transaction =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
        let columns = transaction
            .prepare("PRAGMA table_info(hmems)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if columns.iter().any(|column| column == "valid_to") {
            let forgotten =
                transaction.execute("DELETE FROM hmems WHERE valid_to IS NOT NULL", [])?;
            transaction.execute_batch("ALTER TABLE hmems DROP COLUMN valid_to;")?;
            transaction.commit()?;
            tracing::info!(
                target: "reg.storage",
                forgotten,
                "Migration: forgetting spec applied — soft-deleted rows deleted, valid_to column dropped"
            );
        } else {
            transaction.commit()?;
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
        let lease = match &self.maintenance_lease {
            Some(lease) => lease.clone(),
            None => database_lease(&self.path, false)?,
        };
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
            // The manager lives as long as any pool clone or checked-out
            // connection, unlike the Database facade that handed it out.
            let _lease = &lease;
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

/// The exclusive lease is acquired before any rotation connection is opened
/// and remains held through replacement. Its private path prevents opening an
/// unrelated database under this lease.
pub(crate) struct QuiescedDatabase {
    path: String,
    lease: std::sync::Arc<std::fs::File>,
}

impl QuiescedDatabase {
    pub(crate) fn acquire(path: &str) -> Result<Self, DatabaseError> {
        Ok(Self {
            path: path.into(),
            lease: database_lease(path, true)?,
        })
    }

    pub(crate) fn open(&self, passphrase: &str) -> Result<Database, DatabaseError> {
        let mut database = Database::open(&self.path, passphrase)?;
        database.maintenance_lease = Some(self.lease.clone());
        Ok(database)
    }
}

fn database_lease(
    path: &str,
    exclusive: bool,
) -> Result<std::sync::Arc<std::fs::File>, DatabaseError> {
    // Keep the lease inode stable across database renames. Never unlink this
    // file: unlinking would let two owners lock different inodes for one DB.
    let path = std::path::Path::new(path);
    let canonical = if path.exists() {
        std::fs::canonicalize(path)
    } else {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        std::fs::canonicalize(parent)
            .map(|parent| parent.join(path.file_name().unwrap_or_default()))
    }
    .map_err(|error| DatabaseError::MaintenanceLease {
        path: path.display().to_string(),
        reason: error.to_string(),
    })?;
    let mut lock_path = canonical.as_os_str().to_os_string();
    lock_path.push(".maintenance-lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| DatabaseError::MaintenanceLease {
            path: canonical.display().to_string(),
            reason: error.to_string(),
        })?;
    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    result.map_err(|error| DatabaseError::MaintenanceLease {
        path: canonical.display().to_string(),
        reason: format!(
            "{error}; close existing database consumers or finish maintenance before retrying"
        ),
    })?;
    if !exclusive {
        let canonical_path = canonical
            .to_str()
            .ok_or_else(|| DatabaseError::MaintenanceLease {
                path: canonical.display().to_string(),
                reason: "Canonical database path is not UTF-8".into(),
            })?;
        crate::rotation::ensure_no_recovery_artifacts(canonical_path).map_err(|error| {
            DatabaseError::MaintenanceLease {
                path: canonical_path.into(),
                reason: error.to_string(),
            }
        })?;
    }
    Ok(std::sync::Arc::new(file))
}

impl Drop for Database {
    fn drop(&mut self) {
        // Only emit close for real databases (not :memory:).
        // Dropping this facade releases only its cached pool reference. Other
        // pool clones and checked-out connections retain the shared lease.
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
/// inv: a failed passphrase check never deletes or modifies the database;
///      a successful open applies schema migrations.
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

    fn create_hmem_migration_fixture(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
        connection.execute_batch(
            "CREATE TABLE hmems (
                id TEXT PRIMARY KEY, entity TEXT NOT NULL, attribute TEXT NOT NULL,
                value TEXT NOT NULL, valid_from TEXT NOT NULL, valid_to TEXT,
                recalled_at TEXT NOT NULL, confidence REAL NOT NULL, perspective TEXT,
                visibility TEXT NOT NULL, owner_webid TEXT NOT NULL, ontology TEXT
            );
            INSERT INTO hmems VALUES
                ('retained', 'entity', 'fact', '\"retained\"', '2026-09-01', NULL,
                 '2026-09-03', 0.7, 'author', 'shared', 'owner', '{\"dc_type\":\"bibo:Note\"}'),
                ('forgotten', 'entity', 'fact', '\"forgotten\"', '2026-09-01', '2026-09-04',
                 '2026-09-03', 0.5, NULL, 'private', 'owner', NULL);",
        )
    }

    #[test]
    fn forgetting_migration_purges_rows_on_encrypted_open_and_is_idempotent() -> anyhow::Result<()>
    {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("memory.db");
        let path = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 test path"))?;
        {
            let connection = rusqlite::Connection::open(path)?;
            connection.execute_batch("PRAGMA key = 'test_passphrase';")?;
            create_hmem_migration_fixture(&connection)?;
        }

        for _ in 0..2 {
            let database = open_or_repair(path, "test_passphrase")?;
            let pool = database.sqlite_pool()?;
            let connection = pool.get()?;
            let columns = connection
                .prepare("PRAGMA table_info(hmems)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            assert_eq!(columns.len(), 11);
            assert!(!columns.iter().any(|column| column == "valid_to"));
            let count: i64 =
                connection.query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))?;
            assert_eq!(count, 1, "forgotten rows must be absent, not filtered");
            let retained: bool = connection.query_row(
                "SELECT id = 'retained' AND value = '\"retained\"' AND valid_from = '2026-09-01'
                 AND recalled_at = '2026-09-03' AND confidence = 0.7 AND perspective = 'author'
                 AND visibility = 'shared' AND owner_webid = 'owner'
                 AND ontology = '{\"dc_type\":\"bibo:Note\"}' FROM hmems",
                [],
                |row| row.get(0),
            )?;
            assert!(retained, "migration must preserve every retained field");
            let integrity: String =
                connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
            assert_eq!(integrity, "ok");
        }
        Ok(())
    }

    #[test]
    fn forgetting_migration_drops_column_when_no_rows_are_forgotten() -> anyhow::Result<()> {
        let connection = rusqlite::Connection::open_in_memory()?;
        create_hmem_migration_fixture(&connection)?;
        connection.execute("DELETE FROM hmems WHERE id = 'forgotten'", [])?;
        Database::migrate_hmems_forgetting_spec(&connection)?;
        let column_count: i64 = connection.query_row(
            "SELECT count(*) FROM pragma_table_info('hmems') WHERE name = 'valid_to'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(column_count, 0);
        let count: i64 =
            connection.query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn forgetting_migration_rolls_back_deletion_if_column_drop_fails() -> anyhow::Result<()> {
        let connection = rusqlite::Connection::open_in_memory()?;
        create_hmem_migration_fixture(&connection)?;
        // A dependent index forces DROP COLUMN to fail after the DELETE.
        connection.execute_batch("CREATE INDEX prevent_drop ON hmems(valid_to);")?;
        assert!(Database::migrate_hmems_forgetting_spec(&connection).is_err());
        let count: i64 =
            connection.query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))?;
        assert_eq!(
            count, 2,
            "failed migration must leave the original rows intact"
        );
        connection.execute_batch("DROP INDEX prevent_drop;")?;
        Database::migrate_hmems_forgetting_spec(&connection)?;
        let count: i64 =
            connection.query_row("SELECT count(*) FROM hmems", [], |row| row.get(0))?;
        assert_eq!(count, 1, "migration must be retryable after failure");
        Ok(())
    }

    #[test]
    fn open_database_memory_path_bypasses_file_pool() {
        let db = open_database(":memory:", "test_passphrase").unwrap();
        drop(db);
    }
}
