//! DatabaseDriver trait — the port that stores code against.
//!
//! The trait is dyn-compatible so stores can hold `Arc<dyn DatabaseDriver>`.
//! Ergonomic query helpers (`query_map`, `query_row`) are provided as free
//! functions — they call the trait methods and add type mapping on top.

use super::transaction::TransactionHandle;
use super::types::{DbError, DbProvider};
use super::value::{DbRow, DbValue};

/// The database driver abstraction.
///
/// Stores use `&dyn DatabaseDriver` instead of raw `rusqlite::Connection`.
/// Each provider (SQLite, PostgreSQL) implements this trait. New providers
/// are added without changing store code.
///
/// This trait is dyn-compatible — all methods are object-safe.
/// Ergonomic helpers like `query_map()` are free functions in this module.
pub trait DatabaseDriver: Send + Sync {
    /// Execute a parameterized statement (INSERT, UPDATE, DELETE, DDL).
    fn execute(&self, sql: &str, params: &[DbValue]) -> Result<usize, DbError>;

    /// Execute a batch of SQL statements (schema creation).
    fn execute_batch(&self, sql: &str) -> Result<(), DbError>;

    /// Query rows, returning all results as `DbRow` values.
    fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, DbError>;

    /// Query a single optional row.
    fn query_optional(&self, sql: &str, params: &[DbValue]) -> Result<Option<DbRow>, DbError>;

    /// The provider backing this driver.
    fn provider(&self) -> DbProvider;

    /// Internal: commit current transaction (called by TransactionHandle).
    #[doc(hidden)]
    fn commit_tx(&self) -> Result<(), DbError>;

    /// Internal: rollback current transaction (called by TransactionHandle).
    #[doc(hidden)]
    fn rollback_tx(&self) -> Result<(), DbError>;

    /// Access the driver as `Any` for provider-specific downcasting.
    /// Used by stores that need provider-specific operations (e.g.,
    /// sqlite-vec vector search which requires raw rusqlite connection).
    fn as_any(&self) -> &dyn std::any::Any;

    /// Access the SQLite connection pool, if this is a SqliteDriver.
    fn sqlite_pool(&self) -> Option<&r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>> {
        None
    }

    /// Access the PostgreSQL connection pool, if this is a PostgresDriver.
    fn postgres_pool(&self) -> Option<&sqlx::PgPool> {
        None
    }

    /// Start a transaction, returning a RAII guard.
    /// Auto-rollbacks on drop if not committed.
    ///
    /// Only available on concrete types, not `dyn DatabaseDriver`.
    fn transaction(&self) -> Result<TransactionHandle<'_>, DbError>
    where
        Self: Sized,
    {
        self.execute_batch("BEGIN TRANSACTION")?;
        Ok(TransactionHandle::new(self))
    }
}

// ── Ergonomic query helpers (free functions, work with `&dyn DatabaseDriver`) ──

/// Query rows and map each to a domain type via a closure.
///
/// expect: "The system provides ergonomic query helpers over the DatabaseDriver trait"
/// pre:  driver is connected; sql is valid for the provider; params match placeholders
/// post: returns Ok(`Vec<T>`) with one element per query result row, Err(DbError) on failure
///
/// Replaces the `stmt.query_map(params, |row| { ... })` pattern from raw rusqlite.
///
/// # Example
/// ```ignore
/// let users: Vec<User> = query_map(&*self.driver,
///     "SELECT id, name FROM users WHERE active = ?1",
///     &[DbValue::Bool(true)],
///     |row| Ok(User { id: row.get_str(0)?, name: row.get_str(1)? })
/// )?;
/// ```
pub fn query_map<T, F>(
    driver: &dyn DatabaseDriver,
    sql: &str,
    params: &[DbValue],
    f: F,
) -> Result<Vec<T>, DbError>
where
    F: Fn(&DbRow) -> Result<T, DbError>,
{
    let rows = driver.query(sql, params)?;
    rows.iter().map(f).collect()
}

/// Query one row and map it, returning `None` if no rows found.
///
/// expect: "The system provides ergonomic query helpers over the DatabaseDriver trait"
/// pre:  driver is connected; sql is valid for the provider; params match placeholders
/// post: returns Ok(Some(T)) if a row was found, Ok(None) if empty, Err(DbError) on failure
pub fn query_row<T, F>(
    driver: &dyn DatabaseDriver,
    sql: &str,
    params: &[DbValue],
    f: F,
) -> Result<Option<T>, DbError>
where
    F: Fn(&DbRow) -> Result<T, DbError>,
{
    driver
        .query_optional(sql, params)?
        .map(|r| f(&r))
        .transpose()
}
