//! Shared SQLite plumbing for the media server's two stores
//! (`transcript_store.rs` and `layer_store.rs` — one gallery database,
//! the D35 review's F6: this helper family was duplicated verbatim in
//! both; the 2026-11-07 collapse keeps one definition).
//!
//! The helpers return `DbError`; both stores' error enums carry
//! `#[from] DbError`, so `?` converts at every call site.

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::{DbRow, DbValue};

/// Whether `table` exists (the sqlite_master check).
pub(crate) fn table_exists(driver: &dyn DatabaseDriver, table: &str) -> Result<bool, DbError> {
    Ok(driver
        .query_optional(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            &[DbValue::Text(table.to_string())],
        )?
        .is_some())
}

/// Count rows in `table` where `column` equals `value`.
pub(crate) fn count_rows(
    driver: &dyn DatabaseDriver,
    table: &str,
    column: &str,
    value: &str,
) -> Result<usize, DbError> {
    let row = driver.query_optional(
        &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
        &[DbValue::Text(value.to_string())],
    )?;
    match row {
        Some(row) => Ok(row.get_int(0)? as usize),
        None => Ok(0),
    }
}

/// Optional text value → `DbValue` (Null when absent).
pub(crate) fn opt_param(value: Option<String>) -> DbValue {
    value.map(DbValue::Text).unwrap_or(DbValue::Null)
}

/// Read an optional TEXT column.
pub(crate) fn opt_text(row: &DbRow, name: &str) -> Result<Option<String>, DbError> {
    match row.get_named(name)? {
        DbValue::Null => Ok(None),
        DbValue::Text(text) => Ok(Some(text.clone())),
        other => Err(DbError::Database(format!(
            "column {name}: expected text, got {other:?}"
        ))),
    }
}
