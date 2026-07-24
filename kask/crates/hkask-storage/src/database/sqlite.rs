//! SqliteDriver — rusqlite-backed DatabaseDriver implementation.
//!
//! Uses `r2d2` connection pooling with WAL mode for concurrent read access.
//! SQLCipher encryption is handled at the connection level (PRAGMA key)
//! via the pool's connection initializer.

use super::driver::DatabaseDriver;
use super::types::{DbError, DbProvider};
use super::value::{DbRow, DbValue};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::sync::Arc;

/// Standard SQLite PRAGMAs for WAL-mode connections.
///
/// **Ordering invariant**: `busy_timeout` MUST be set before
/// `journal_mode = WAL` because the WAL mode change acquires a brief
/// exclusive lock. With `busy_timeout = 0` (SQLite default), any lock
/// contention fails immediately with `SQLITE_BUSY` instead of retrying.
/// This caused `r2d2: database is locked` errors on per-agent databases.
///
/// Call this on every raw `rusqlite::Connection` or in every r2d2
/// `with_init` closure before schema operations.
pub const WAL_PRAGMA_BATCH: &str =
    "PRAGMA busy_timeout = 5000; PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;";

/// Apply the standard WAL PRAGMAs to a raw `rusqlite::Connection`.
///
/// This is the single source of truth for PRAGMA ordering. Crates that
/// depend on `hkask-database` should call this instead of inlining PRAGMA
/// strings. Crates that don't depend on `hkask-database` should replicate
/// the ordering: `busy_timeout` before `journal_mode = WAL`.
pub fn init_wal_pragmas(conn: &mut rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(WAL_PRAGMA_BATCH)
}

/// SQLite implementation of DatabaseDriver.
///
/// Wraps an `r2d2::Pool<SqliteConnectionManager>` with WAL mode enabled.
/// Each `execute`/`query` call acquires a connection from the pool and
/// returns it on completion, enabling concurrent read access.
pub struct SqliteDriver {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteDriver {
    /// Create a new SQLite driver from a connection pool.
    ///
    /// The pool should be configured with WAL mode and any encryption
    /// PRAGMAs before being passed here.
    pub fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    /// Create a pool for an in-memory database (testing only).
    ///
    /// Uses `max_size(1)` because `SqliteConnectionManager::memory()`
    /// create a separate in-memory database per connection. A pool size
    /// greater than 1 would scatter writes across independent databases,
    /// breaking read-your-writes semantics for tests.
    pub fn in_memory_pool() -> Result<Pool<SqliteConnectionManager>, r2d2::Error> {
        let manager = SqliteConnectionManager::memory()
            .with_init(|conn| conn.execute_batch("PRAGMA foreign_keys = ON;"));
        Pool::builder().max_size(1).build(manager)
    }

    /// Create an in-memory driver for testing (one-liner convenience).
    pub fn in_memory_driver() -> Arc<dyn super::driver::DatabaseDriver> {
        Arc::new(Self::new(Self::in_memory_pool().expect("in-memory pool")))
    }

    /// Acquire a raw `rusqlite::Connection` from the pool.
    ///
    /// Used by stores that need direct rusqlite access (e.g., sqlite-vec
    /// virtual tables that don't work through the DbValue abstraction).
    /// The connection is returned to the pool when the guard is dropped.
    pub fn acquire_raw(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, DbError> {
        self.pool
            .get()
            .map_err(|e| DbError::Connection(e.to_string()))
    }

    /// Get a reference to the pool (for stores that need it).
    pub fn pool(&self) -> &Pool<SqliteConnectionManager> {
        &self.pool
    }

    /// Raw query execution without Regulation span emission.
    /// Called by `query` and `query_optional` which each emit their own spans.
    fn query_raw(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, DbError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| DbError::Connection(e.to_string()))?;
        let rusqlite_params = Self::to_rusqlite_params(params);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            rusqlite_params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| DbError::Database(e.to_string()))?;
        let columns: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Self::row_to_dbrow(row, &columns)
            })
            .map_err(|e| DbError::Database(e.to_string()))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| DbError::Database(e.to_string()))?);
        }
        Ok(results)
    }

    fn to_rusqlite_params(params: &[DbValue]) -> Vec<Box<dyn rusqlite::types::ToSql>> {
        params
            .iter()
            .map(|v| -> Box<dyn rusqlite::types::ToSql> {
                match v {
                    DbValue::Null => Box::new(rusqlite::types::Null),
                    DbValue::Integer(i) => Box::new(*i),
                    DbValue::Real(f) => Box::new(*f),
                    DbValue::Text(s) => Box::new(s.clone()),
                    DbValue::Blob(b) => Box::new(b.clone()),
                    DbValue::Bool(b) => Box::new(*b as i64),
                }
            })
            .collect()
    }

    fn row_to_dbrow(row: &rusqlite::Row, columns: &[String]) -> rusqlite::Result<DbRow> {
        let mut values = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            let val = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => DbValue::Null,
                rusqlite::types::ValueRef::Integer(i) => DbValue::Integer(i),
                rusqlite::types::ValueRef::Real(f) => DbValue::Real(f),
                rusqlite::types::ValueRef::Text(s) => {
                    DbValue::Text(String::from_utf8_lossy(s).into_owned())
                }
                rusqlite::types::ValueRef::Blob(b) => DbValue::Blob(b.to_vec()),
            };
            values.push(val);
        }
        Ok(DbRow::new(columns.to_vec(), values))
    }
}

impl DatabaseDriver for SqliteDriver {
    fn as_any(&self) -> &dyn std::any::Any {
        &self.pool
    }

    fn sqlite_pool(&self) -> Option<&Pool<SqliteConnectionManager>> {
        Some(&self.pool)
    }

    fn execute(&self, sql: &str, params: &[DbValue]) -> Result<usize, DbError> {
        let start = std::time::Instant::now();
        let table = super::regulation::extract_table(sql);
        let conn = self
            .pool
            .get()
            .map_err(|e| DbError::Connection(e.to_string()))?;
        let rusqlite_params = Self::to_rusqlite_params(params);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            rusqlite_params.iter().map(|p| p.as_ref()).collect();
        let result = conn
            .execute(sql, param_refs.as_slice())
            .map_err(|e| DbError::Database(e.to_string()));
        let duration_us = start.elapsed().as_micros() as u64;
        match &result {
            Ok(rows) => {
                super::regulation::emit_storage_span("execute", table, duration_us, *rows, false)
            }
            Err(_) => super::regulation::emit_storage_span("execute", table, duration_us, 0, true),
        }
        result
    }

    fn execute_batch(&self, sql: &str) -> Result<(), DbError> {
        let conn = self
            .pool
            .get()
            .map_err(|e| DbError::Connection(e.to_string()))?;
        conn.execute_batch(sql)
            .map_err(|e| DbError::Database(e.to_string()))
    }

    fn query(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, DbError> {
        let start = std::time::Instant::now();
        let table = super::regulation::extract_table(sql);
        let result = self.query_raw(sql, params);
        let duration_us = start.elapsed().as_micros() as u64;
        match &result {
            Ok(rows) => {
                super::regulation::emit_storage_span("query", table, duration_us, rows.len(), false)
            }
            Err(_) => super::regulation::emit_storage_span("query", table, duration_us, 0, true),
        }
        result
    }

    fn query_optional(&self, sql: &str, params: &[DbValue]) -> Result<Option<DbRow>, DbError> {
        let start = std::time::Instant::now();
        let table = super::regulation::extract_table(sql);
        let result = self.query_raw(sql, params);
        let duration_us = start.elapsed().as_micros() as u64;
        match result {
            Ok(mut rows) => {
                let row_count = rows.len();
                super::regulation::emit_storage_span(
                    "query_optional",
                    table,
                    duration_us,
                    row_count,
                    false,
                );
                if rows.is_empty() {
                    Ok(None)
                } else if rows.len() == 1 {
                    Ok(Some(rows.remove(0)))
                } else {
                    Err(DbError::Database(format!(
                        "query_optional expected 0-1 rows, got {}",
                        rows.len()
                    )))
                }
            }
            Err(e) => {
                super::regulation::emit_storage_span("query_optional", table, duration_us, 0, true);
                Err(e)
            }
        }
    }

    fn provider(&self) -> DbProvider {
        DbProvider::Sqlite
    }

    fn commit_tx(&self) -> Result<(), DbError> {
        self.execute_batch("COMMIT")
    }

    fn rollback_tx(&self) -> Result<(), DbError> {
        self.execute_batch("ROLLBACK")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_driver() -> SqliteDriver {
        SqliteDriver::new(SqliteDriver::in_memory_pool().unwrap())
    }

    #[test]
    fn execute_and_query_roundtrip() {
        let driver = make_driver();
        driver
            .execute_batch("CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        driver
            .execute(
                "INSERT INTO test (name) VALUES (?)",
                &[DbValue::Text("hello".into())],
            )
            .unwrap();
        let rows = driver.query("SELECT * FROM test", &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].get_named("name").unwrap().as_text().unwrap(),
            "hello"
        );
    }

    #[test]
    fn query_optional_returns_none_for_empty() {
        let driver = make_driver();
        driver.execute_batch("CREATE TABLE t (x INTEGER)").unwrap();
        let result = driver
            .query_optional("SELECT * FROM t WHERE x = 1", &[])
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn transaction_commit_and_rollback() {
        let driver = make_driver();
        driver.execute_batch("CREATE TABLE t (x INTEGER)").unwrap();
        let tx = driver.transaction().unwrap();
        driver
            .execute("INSERT INTO t VALUES (?)", &[DbValue::Integer(42)])
            .unwrap();
        tx.commit().unwrap();
        let rows = driver.query("SELECT * FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 1);

        let tx = driver.transaction().unwrap();
        driver
            .execute("INSERT INTO t VALUES (?)", &[DbValue::Integer(99)])
            .unwrap();
        drop(tx); // rollback on drop
        let rows = driver.query("SELECT * FROM t", &[]).unwrap();
        assert_eq!(rows.len(), 1);
    }
}
