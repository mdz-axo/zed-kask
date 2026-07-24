//! WalletStore — SQLite-backed persistence for rJoule balances, transactions, API keys.
//!
//! # Schema (5 tables)
//! - `wallet_balances` — one row per wallet, current rJoule balance
//! - `wallet_transactions` — append-only ledger of all balance changes
//! - `api_keys` — issued Ed25519 capability tokens with spending limits
//! - `deposit_addresses` — derived deposit addresses per wallet per chain
//! - `deposit_references` — one-time shielded deposit references (anti-replay)

pub mod api_keys;
pub mod balances;
pub mod deposits;
pub mod encumbrances;
#[cfg(test)]
mod tests;
pub mod transactions;

use crate::define_driver_store;
use hkask_types::WalletError;

define_driver_store!(WalletStore);

// ── WalletStore cross-cutting methods ──────────────────────────────────────────

impl WalletStore {
    /// Initialize the wallet schema (idempotent).
    ///
    /// Creates all wallet tables if they don't already exist:
    /// `wallet_balances`, `wallet_transactions`, `api_keys`,
    /// `encumbrances`, `deposit_addresses`, `deposit_references`.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — wallet schema
    /// post: all wallet tables exist
    fn init_schema(driver: &std::sync::Arc<dyn crate::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );
            CREATE TABLE IF NOT EXISTS deposit_addresses (
                wallet_id TEXT NOT NULL,
                chain TEXT NOT NULL,
                address TEXT NOT NULL,
                derivation_index INTEGER NOT NULL,
                privacy_mode TEXT NOT NULL,
                UNIQUE(wallet_id, chain, privacy_mode, derivation_index)
            );
            CREATE TABLE IF NOT EXISTS deposit_references (
                reference TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                chain TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                spent INTEGER NOT NULL DEFAULT 0
            );",
        );
        tracing::info!(target: "hkask.storage", "WalletStore schema initialized");
    }

    /// Enable SQLite WAL (Write-Ahead Logging) mode for better concurrency.
    ///
    /// WAL mode allows concurrent reads while a write is in progress,
    /// significantly improving throughput under multi-agent API key spend loads.
    /// Without WAL, all operations serialize on the connection mutex.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// post: journal_mode set to WAL
    /// post: synchronous set to NORMAL (balance durability vs performance)
    ///
    /// Call once after store creation, before any wallet operations.
    /// Enable WAL mode for better concurrency.
    ///
    /// expect: "The system provides durable storage for wallet data"
    /// \[P3\] Motivating: Generative Space — enable WAL for wallet concurrency
    /// \[P7\] Constraining: Evolutionary Architecture — WAL mode emerged from multi-agent load
    /// post: journal_mode set to WAL, synchronous set to NORMAL
    pub fn enable_wal_mode(&self) -> Result<(), WalletError> {
        // Standard WAL PRAGMAs — see crate::database::init_wal_pragmas for rationale.
        self.driver
            .execute_batch(crate::database::WAL_PRAGMA_BATCH)?;
        self.driver.execute_batch("PRAGMA synchronous=NORMAL;")?;
        tracing::info!(target: "hkask.storage", "WalletStore WAL mode enabled");
        Ok(())
    }
}
