//! Shared SQLite/DatabaseDriver row and schema helpers.

use super::driver::DatabaseDriver;
use super::types::DbError;
use super::value::{DbRow, DbValue};

/// Whether `table` exists (the `sqlite_master` check).
pub fn table_exists(driver: &dyn DatabaseDriver, table: &str) -> Result<bool, DbError> {
    Ok(driver
        .query_optional(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            &[DbValue::Text(table.to_string())],
        )?
        .is_some())
}

/// Count rows in `table` where `column` equals `value`.
pub fn count_rows(
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

/// Optional text value → `DbValue` (`Null` when absent).
pub fn opt_param(value: Option<String>) -> DbValue {
    value.map(DbValue::Text).unwrap_or(DbValue::Null)
}

/// Read an optional `TEXT` column.
pub fn opt_text(row: &DbRow, name: &str) -> Result<Option<String>, DbError> {
    match row.get_named(name)? {
        DbValue::Null => Ok(None),
        DbValue::Text(text) => Ok(Some(text.clone())),
        other => Err(DbError::Database(format!(
            "column {name}: expected text, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;

    #[test]
    fn table_exists_and_count_rows_and_opt_helpers() {
        let driver = SqliteDriver::in_memory_driver();
        assert!(!table_exists(driver.as_ref(), "items").unwrap());

        driver
            .execute_batch("CREATE TABLE items (id TEXT PRIMARY KEY, note TEXT);")
            .unwrap();
        assert!(table_exists(driver.as_ref(), "items").unwrap());

        driver
            .execute(
                "INSERT INTO items (id, note) VALUES (?1, ?2)",
                &[DbValue::Text("a".into()), opt_param(None)],
            )
            .unwrap();
        driver
            .execute(
                "INSERT INTO items (id, note) VALUES (?1, ?2)",
                &[DbValue::Text("b".into()), opt_param(Some("hi".into()))],
            )
            .unwrap();

        assert_eq!(count_rows(driver.as_ref(), "items", "id", "a").unwrap(), 1);
        assert_eq!(count_rows(driver.as_ref(), "items", "id", "z").unwrap(), 0);

        let row_a = driver
            .query_optional(
                "SELECT note FROM items WHERE id = ?1",
                &[DbValue::Text("a".into())],
            )
            .unwrap()
            .unwrap();
        assert_eq!(opt_text(&row_a, "note").unwrap(), None);

        let row_b = driver
            .query_optional(
                "SELECT note FROM items WHERE id = ?1",
                &[DbValue::Text("b".into())],
            )
            .unwrap()
            .unwrap();
        assert_eq!(opt_text(&row_b, "note").unwrap(), Some("hi".to_string()));
    }
}
