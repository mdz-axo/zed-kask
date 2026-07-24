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
pub(crate) fn embedding_dim() -> usize {
    std::env::var("HKASK_EMBEDDING_DIM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_EMBEDDING_DIM)
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
fn init_sqlite_vec_on(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
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

        let pool_size = std::env::var("HKASK_DB_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8);
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

pub fn check_passphrase(path: &str, passphrase: &str) -> Result<(), DatabaseError> {
    let db = Database::open(path, passphrase)?;
    // Verification happens during connect() — if the passphrase is wrong,
    // pool.get() returns an error that maps to PassphraseMismatch.
    let _ = db.sqlite_pool()?;
    Ok(())
}

/// expect: "A passphrase mistake never destroys my encrypted database."
/// \[P1\] Motivating: User Sovereignty — user data remains under the user's control.
/// pre: `path` identifies a SQLCipher database and `passphrase` is non-empty.
/// post: returns an opened database only when the passphrase verifies.
/// inv: never deletes or modifies the database or its salt file.
/// \[P4\] Constraining: Clear Boundaries — recovery is an explicit operation, not an implicit side effect.
pub fn open_or_repair(path: &str, passphrase: &str) -> Result<Database, DatabaseError> {
    let db = Database::open(path, passphrase)?;
    db.sqlite_pool()?;
    Ok(db)
}

pub fn open_database(path: &str, passphrase: &str) -> Result<Database, DatabaseError> {
    if path == ":memory:" {
        Database::in_memory()
    } else {
        Database::open(path, passphrase)
    }
}

fn generate_salt() -> [u8; SQLCIPHER_SALT_SIZE] {
    use rand::Rng;
    rand::rng().random()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_or_repair_preserves_files_on_passphrase_mismatch() {
        let tmp = std::env::temp_dir().join(format!("hkask-db-test-{}", rand::random::<u32>()));
        let db_path = tmp.join("database.db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let salt_path = format!("{}.salt", db_path_str);

        let db =
            Database::open(&db_path_str, "correct-passphrase").expect("create encrypted database");
        db.sqlite_pool().expect("initialize encrypted database");
        drop(db);

        let database_before = std::fs::read(&db_path).expect("database exists");
        let salt_before = std::fs::read(&salt_path).expect("salt exists");

        assert!(
            open_or_repair(&db_path_str, "incorrect-passphrase").is_err(),
            "wrong passphrase must fail without recovery"
        );
        assert_eq!(
            std::fs::read(&db_path).expect("database remains"),
            database_before
        );
        assert_eq!(
            std::fs::read(&salt_path).expect("salt remains"),
            salt_before
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn open_creates_parent_directories() {
        let tmp = std::env::temp_dir().join(format!("hkask-db-test-{}", rand::random::<u32>()));
        let db_path = tmp.join("a").join("b").join("c").join("test.db");
        let db_path_str = db_path.to_string_lossy().to_string();

        if db_path.exists() {
            std::fs::remove_file(&db_path).ok();
        }
        let result = Database::open(&db_path_str, "test-passphrase-123");
        assert!(result.is_ok(), "Database::open failed: {:?}", result.err());

        // open() only creates the salt file and parent dirs.
        // The .db file is created by connect() / sqlite_pool().
        let salt_path = format!("{}.salt", db_path_str);
        assert!(
            std::path::Path::new(&salt_path).exists(),
            "Salt file should exist at {}",
            salt_path
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
