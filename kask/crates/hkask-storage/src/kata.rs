//! Kata practice history — SQLite-backed persistence for habit tracking,
//! automaticity scoring, and streak computation.
//!
//! Each practice session logs userpod name, date, kata type, practice name,
//! steps completed, and gas consumed.
use crate::database::driver::{query_map, query_row};
use crate::database::value::DbValue;
use crate::{define_driver_store, impl_from_db_error};
use hkask_types::InfrastructureError;
define_driver_store!(KataHistoryStore);

/// A single kata practice session entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KataHistoryEntry {
    pub id: i64,
    pub userpod_name: String,
    pub date: String,
    pub kata_type: String,
    pub practice_name: String,
    pub steps_completed: usize,
    pub gas_consumed: u64,
    pub created_at: String,
}

/// Error type for kata history operations.
#[derive(Debug, thiserror::Error)]
pub enum KataHistoryError {
    #[error("Infrastructure error: {0}")]
    Infra(#[from] InfrastructureError),
    #[error("Parse error: {0}")]
    Parse(String),
}
impl_from_db_error!(KataHistoryError, Infra);

impl KataHistoryStore {
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS kata_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                userpod_name TEXT NOT NULL,
                date TEXT NOT NULL,
                kata_type TEXT NOT NULL,
                practice_name TEXT NOT NULL,
                steps_completed INTEGER NOT NULL DEFAULT 0,
                gas_consumed INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        );
    }

    /// Record a kata practice session.
    pub fn record(
        &self,
        userpod_name: &str,
        date: &str,
        kata_type: &str,
        practice_name: &str,
        steps_completed: usize,
        gas_consumed: u64,
    ) -> Result<i64, KataHistoryError> {
        let driver = &*self.driver;
        driver.execute(
            "INSERT INTO kata_history (userpod_name, date, kata_type, practice_name, steps_completed, gas_consumed) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                DbValue::Text(userpod_name.to_string()),
                DbValue::Text(date.to_string()),
                DbValue::Text(kata_type.to_string()),
                DbValue::Text(practice_name.to_string()),
                DbValue::Integer(steps_completed as i64),
                DbValue::Integer(gas_consumed as i64),
            ],
        )?;
        Ok(
            query_row(driver, "SELECT MAX(id) FROM kata_history", &[], |row| {
                row.get_int(0)
            })?
            .unwrap_or(0),
        )
    }

    /// Retrieve all entries for a userpod, ordered by date descending.
    #[must_use = "result must be used"]
    pub fn entries_for_userpod(
        &self,
        userpod_name: &str,
    ) -> Result<Vec<KataHistoryEntry>, KataHistoryError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, userpod_name, date, kata_type, practice_name, steps_completed, gas_consumed, created_at FROM kata_history WHERE userpod_name = ?1 ORDER BY date DESC",
            &[DbValue::Text(userpod_name.to_string())],
            |row| {
                Ok(KataHistoryEntry {
                    id: row.get_int(0)?,
                    userpod_name: row.get_str(1)?.to_string(),
                    date: row.get_str(2)?.to_string(),
                    kata_type: row.get_str(3)?.to_string(),
                    practice_name: row.get_str(4)?.to_string(),
                    steps_completed: row.get_int(5)? as usize,
                    gas_consumed: row.get_int(6)? as u64,
                    created_at: row.get_str(7)?.to_string(),
                })
            },
        )?)
    }

    /// Count total entries for a userpod.
    pub fn count_entries_for_userpod(&self, userpod_name: &str) -> Result<usize, KataHistoryError> {
        let count = query_row(
            &*self.driver,
            "SELECT COUNT(*) FROM kata_history WHERE userpod_name = ?1",
            &[DbValue::Text(userpod_name.to_string())],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        Ok(count as usize)
    }

    /// Count entries for a userpod on a specific date.
    pub fn count_entries_on(
        &self,
        userpod_name: &str,
        date: &str,
    ) -> Result<usize, KataHistoryError> {
        let count = query_row(
            &*self.driver,
            "SELECT COUNT(*) FROM kata_history WHERE userpod_name = ?1 AND date = ?2",
            &[
                DbValue::Text(userpod_name.to_string()),
                DbValue::Text(date.to_string()),
            ],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        Ok(count as usize)
    }

    /// Get the most recent entry for a userpod.
    #[must_use = "result must be used"]
    pub fn last_entry_for_userpod(
        &self,
        userpod_name: &str,
    ) -> Result<Option<KataHistoryEntry>, KataHistoryError> {
        Ok(query_row(
            &*self.driver,
            "SELECT id, userpod_name, date, kata_type, practice_name, steps_completed, gas_consumed, created_at FROM kata_history WHERE userpod_name = ?1 ORDER BY date DESC, id DESC LIMIT 1",
            &[DbValue::Text(userpod_name.to_string())],
            |row| {
                Ok(KataHistoryEntry {
                    id: row.get_int(0)?,
                    userpod_name: row.get_str(1)?.to_string(),
                    date: row.get_str(2)?.to_string(),
                    kata_type: row.get_str(3)?.to_string(),
                    practice_name: row.get_str(4)?.to_string(),
                    steps_completed: row.get_int(5)? as usize,
                    gas_consumed: row.get_int(6)? as u64,
                    created_at: row.get_str(7)?.to_string(),
                })
            },
        )?)
    }

    /// Get all entries for a userpod within a date range (inclusive).
    pub fn entries_in_range(
        &self,
        userpod_name: &str,
        from_date: &str,
        to_date: &str,
    ) -> Result<Vec<KataHistoryEntry>, KataHistoryError> {
        Ok(query_map(
            &*self.driver,
            "SELECT id, userpod_name, date, kata_type, practice_name, steps_completed, gas_consumed, created_at FROM kata_history WHERE userpod_name = ?1 AND date >= ?2 AND date <= ?3 ORDER BY date DESC",
            &[
                DbValue::Text(userpod_name.to_string()),
                DbValue::Text(from_date.to_string()),
                DbValue::Text(to_date.to_string()),
            ],
            |row| {
                Ok(KataHistoryEntry {
                    id: row.get_int(0)?,
                    userpod_name: row.get_str(1)?.to_string(),
                    date: row.get_str(2)?.to_string(),
                    kata_type: row.get_str(3)?.to_string(),
                    practice_name: row.get_str(4)?.to_string(),
                    steps_completed: row.get_int(5)? as usize,
                    gas_consumed: row.get_int(6)? as u64,
                    created_at: row.get_str(7)?.to_string(),
                })
            },
        )?)
    }

    /// Delete entries older than a given date.
    pub fn delete_entries_before(&self, before_date: &str) -> Result<usize, KataHistoryError> {
        let count = self.driver.execute(
            "DELETE FROM kata_history WHERE date < ?1",
            &[DbValue::Text(before_date.to_string())],
        )?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    fn make_test_store() -> KataHistoryStore {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory SQLite pool");
        let driver = SqliteDriver::new(pool);
        KataHistoryStore::from_driver(Arc::new(driver))
    }

    #[test]
    fn record_and_retrieve_entry() {
        let store = make_test_store();
        store
            .record("Alice", "2026-06-15", "starter", "starter-kata", 5, 0)
            .unwrap();
        let entries = store.entries_for_userpod("Alice").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].userpod_name, "Alice");
        assert_eq!(entries[0].kata_type, "starter");
        assert_eq!(entries[0].steps_completed, 5);
    }

    #[test]
    fn count_entries_per_date() {
        let store = make_test_store();
        store
            .record("Alice", "2026-06-14", "coaching", "coaching-kata", 5, 200)
            .unwrap();
        store
            .record("Alice", "2026-06-15", "starter", "starter-kata", 3, 0)
            .unwrap();
        store
            .record(
                "Bob",
                "2026-06-15",
                "improvement",
                "improvement-kata",
                4,
                15000,
            )
            .unwrap();
        assert_eq!(store.count_entries_for_userpod("Alice").unwrap(), 2);
        assert_eq!(store.count_entries_on("Alice", "2026-06-15").unwrap(), 1);
        assert_eq!(store.count_entries_for_userpod("Bob").unwrap(), 1);
    }

    #[test]
    fn last_entry_for_userpod() {
        let store = make_test_store();
        store
            .record("Alice", "2026-06-14", "starter", "starter-kata", 5, 0)
            .unwrap();
        store
            .record(
                "Alice",
                "2026-06-15",
                "improvement",
                "improvement-kata",
                4,
                15000,
            )
            .unwrap();
        let last = store.last_entry_for_userpod("Alice").unwrap().unwrap();
        assert_eq!(last.date, "2026-06-15");
        assert_eq!(last.kata_type, "improvement");
        assert_eq!(last.gas_consumed, 15000);
    }

    #[test]
    fn no_entries_returns_none() {
        let store = make_test_store();
        let last = store.last_entry_for_userpod("Nobody").unwrap();
        assert!(last.is_none());
    }

    #[test]
    fn delete_entries_before() {
        let store = make_test_store();
        store
            .record("Alice", "2026-06-13", "starter", "starter-kata", 5, 0)
            .unwrap();
        store
            .record("Alice", "2026-06-14", "starter", "starter-kata", 5, 0)
            .unwrap();
        store
            .record(
                "Alice",
                "2026-06-15",
                "improvement",
                "improvement-kata",
                4,
                15000,
            )
            .unwrap();
        let deleted = store.delete_entries_before("2026-06-14").unwrap();
        assert_eq!(deleted, 1);
        let remaining = store.entries_for_userpod("Alice").unwrap();
        assert_eq!(remaining.len(), 2);
    }
}
