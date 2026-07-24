//! Ledger schema DDL — table creation and metadata initialization.

use hkask_storage::database::driver::DatabaseDriver;
use hkask_storage::database::value::DbValue;

/// The SQL DDL for ledger schema initialization.
///
/// Creates four tables:
/// - `_ledger_meta` — key/value metadata (created_at, ledger_id)
/// - `accounts` — named accounts grouped by namespace
/// - `transactions` — immutable transaction headers with unique reference
/// - `postings` — individual entries (source → destination, asset, amount)
///
/// Three indexes optimize the most common query patterns:
/// - `idx_postings_destination_asset` — balance queries by destination
/// - `idx_postings_source_asset` — balance queries by source
/// - `idx_transactions_reference` — idempotency check by reference
pub const SCHEMA_DDL: &str = "CREATE TABLE IF NOT EXISTS _ledger_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    namespace TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS transactions (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    reference TEXT UNIQUE NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS postings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id TEXT NOT NULL REFERENCES transactions(id),
    source TEXT NOT NULL,
    destination TEXT NOT NULL,
    asset TEXT NOT NULL,
    amount INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_postings_destination_asset
    ON postings(destination, asset);
CREATE INDEX IF NOT EXISTS idx_postings_source_asset
    ON postings(source, asset);
CREATE INDEX IF NOT EXISTS idx_transactions_reference
    ON transactions(reference);";

/// Initialize the schema and metadata on a fresh database.
///
/// Idempotent — safe to call on an existing database. Detects whether
/// the database is newly created (no `created_at` in `_ledger_meta`)
/// and inserts metadata if so.
pub fn init_schema(driver: &Arc<dyn DatabaseDriver>) -> Result<(), super::LedgerError> {
    driver.execute_batch(SCHEMA_DDL)?;

    let is_new = driver
        .query_optional(
            "SELECT COUNT(*) FROM _ledger_meta WHERE key = 'created_at'",
            &[],
        )?
        .map(|row| row.get_int(0).unwrap_or(0) == 0)
        .unwrap_or(true);

    if is_new {
        let now = chrono::Utc::now().to_rfc3339();
        let ledger_id = uuid::Uuid::new_v4().to_string();
        driver.execute(
            "INSERT INTO _ledger_meta (key, value) VALUES ('created_at', ?1), ('ledger_id', ?2)",
            &[DbValue::Text(now), DbValue::Text(ledger_id)],
        )?;
    }

    Ok(())
}

use std::sync::Arc;
