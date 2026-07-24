//! PostgresDriver — sqlx-backed DatabaseDriver for PostgreSQL.
//!
//! Bridges async sqlx to the sync DatabaseDriver trait via
//! tokio::runtime::Handle::block_on.
//!
//! # ⚠️ Deadlock Risk
//!
//! All methods call `block_on` internally. This will panic with
//! "Cannot start a runtime from within a runtime" if called from
//! within an async tokio context. Callers must ensure PostgresDriver
//! operations happen from a non-async context or a dedicated thread.
//!
//! # SQL Translation
//!
//! SQLite-style `?N` placeholders are translated to PostgreSQL `$N`.
//! Parameters are serialized to text; PostgreSQL coerces to the
//! target column type.

use super::driver::DatabaseDriver;
use super::types::{DbError, DbProvider};
use super::value::{DbRow, DbValue};
use base64::Engine;
use sqlx::Column;
use sqlx::Row;

pub struct PostgresDriver {
    pool: sqlx::PgPool,
    handle: tokio::runtime::Handle,
}

impl PostgresDriver {
    pub fn new(pool: sqlx::PgPool, handle: tokio::runtime::Handle) -> Self {
        Self { pool, handle }
    }

    /// Raw query execution without Regulation span emission.
    /// Called by `query` and `query_optional` which each emit their own spans.
    fn query_raw(&self, sql: &str, params: &[DbValue]) -> Result<Vec<DbRow>, DbError> {
        let sql = translate_placeholders(sql);
        let pool = self.pool.clone();
        let args: Vec<Option<String>> = params.iter().map(to_param).collect();
        let rows = self.handle.block_on(async move {
            let mut q = sqlx::query(&sql);
            for a in &args {
                q = q.bind(a);
            }
            q.fetch_all(&pool).await.map_err(pg_err)
        })?;
        Ok(rows.iter().map(row_to_dbrow).collect())
    }
}

// ── SQL translation: ?N → $N ─────────────────────────────────────────

fn translate_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let b = sql.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'?' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            out.push('$');
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                out.push(b[i] as char);
                i += 1;
            }
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

fn pg_err(e: sqlx::Error) -> DbError {
    DbError::Database(e.to_string())
}

// ── Param conversion: DbValue → Option<String> ───────────────────────
//
// PostgreSQL coerces text to the target column type (int, float, bool,
// bytea, etc.). NULL is represented as None.

fn to_param(v: &DbValue) -> Option<String> {
    match v {
        DbValue::Null => None,
        DbValue::Integer(i) => Some(i.to_string()),
        DbValue::Real(f) => Some(f.to_string()),
        DbValue::Text(s) => Some(s.clone()),
        DbValue::Blob(b) => Some(base64::engine::general_purpose::STANDARD.encode(b)),
        DbValue::Bool(b) => Some(if *b { "t".into() } else { "f".into() }),
    }
}

// ── Row decoding: PgRow → DbRow ──────────────────────────────────────

fn row_to_dbrow(row: &sqlx::postgres::PgRow) -> DbRow {
    let columns: Vec<String> = row.columns().iter().map(|c| c.name().to_string()).collect();
    let mut values = Vec::with_capacity(columns.len());
    for i in 0..columns.len() {
        // Try integer first (most common), then real, bool, text, blob.
        // PostgreSQL text coercion keeps integers as integers internally.
        let val = row
            .try_get::<Option<i64>, _>(i)
            .ok()
            .flatten()
            .map(DbValue::Integer)
            .or_else(|| {
                row.try_get::<Option<f64>, _>(i)
                    .ok()
                    .flatten()
                    .map(DbValue::Real)
            })
            .or_else(|| {
                row.try_get::<Option<bool>, _>(i)
                    .ok()
                    .flatten()
                    .map(DbValue::Bool)
            })
            .or_else(|| {
                row.try_get::<Option<Vec<u8>>, _>(i)
                    .ok()
                    .flatten()
                    .map(DbValue::Blob)
            })
            .or_else(|| {
                row.try_get::<Option<String>, _>(i)
                    .ok()
                    .flatten()
                    .map(DbValue::Text)
            })
            .unwrap_or(DbValue::Null);
        values.push(val);
    }
    DbRow::new(columns, values)
}

// ── DatabaseDriver impl ──────────────────────────────────────────────

impl DatabaseDriver for PostgresDriver {
    fn execute(&self, sql: &str, params: &[DbValue]) -> Result<usize, DbError> {
        let start = std::time::Instant::now();
        let table = super::regulation::extract_table(sql);
        let sql = translate_placeholders(sql);
        let pool = self.pool.clone();
        let args: Vec<Option<String>> = params.iter().map(to_param).collect();
        let result = self.handle.block_on(async move {
            let mut q = sqlx::query(&sql);
            for a in &args {
                q = q.bind(a);
            }
            let r = q.execute(&pool).await.map_err(pg_err)?;
            Ok(r.rows_affected() as usize)
        });
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
        let sql = translate_placeholders(sql);
        let pool = self.pool.clone();
        self.handle.block_on(async move {
            sqlx::query(&sql).execute(&pool).await.map_err(pg_err)?;
            Ok(())
        })
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
            Ok(rows) => {
                let row_count = rows.len();
                super::regulation::emit_storage_span(
                    "query_optional",
                    table,
                    duration_us,
                    row_count,
                    false,
                );
                match row_count {
                    0 => Ok(None),
                    1 => Ok(Some(rows.into_iter().next().unwrap())),
                    n => Err(DbError::Database(format!(
                        "query_optional: expected 0-1 rows, got {n}"
                    ))),
                }
            }
            Err(e) => {
                super::regulation::emit_storage_span("query_optional", table, duration_us, 0, true);
                Err(e)
            }
        }
    }

    fn provider(&self) -> DbProvider {
        DbProvider::Postgres
    }

    fn commit_tx(&self) -> Result<(), DbError> {
        self.execute_batch("COMMIT")
    }

    fn rollback_tx(&self) -> Result<(), DbError> {
        self.execute_batch("ROLLBACK")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        &self.pool
    }

    fn postgres_pool(&self) -> Option<&sqlx::PgPool> {
        Some(&self.pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_to_dollar() {
        assert_eq!(translate_placeholders("?1"), "$1");
        assert_eq!(
            translate_placeholders("a = ?1 AND b = ?2"),
            "a = $1 AND b = $2"
        );
        assert_eq!(translate_placeholders("?12"), "$12");
    }
}
