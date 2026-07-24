//! Agent wallet persistence — per-agent gas/rJoule balance tracking.
//!
//! Maps agent identity (WebID) to wallet identity (WalletId) and caches
//! gas/rJoule balances for Regulation energy regulation. The canonical financial
//! data lives in `hkask-storage::WalletStore`; this store is a regulatory
//! cache layered on top — it holds references, not funds.

use hkask_storage::database::driver::{query_map, query_row};
use hkask_storage::database::types::DbError;
use hkask_storage::database::value::DbValue;
use hkask_storage::define_driver_store;
use hkask_types::{InfrastructureError, WebID};
use thiserror::Error;

define_driver_store!(WalletStore);

#[derive(Debug, Error)]
pub enum AgentWalletError {
    #[error("database: {0}")]
    Database(#[from] InfrastructureError),
}

impl From<DbError> for AgentWalletError {
    fn from(e: DbError) -> Self {
        AgentWalletError::Database(InfrastructureError::from(e))
    }
}

impl WalletStore {
    fn init_schema(driver: &std::sync::Arc<dyn hkask_storage::database::driver::DatabaseDriver>) {
        let _ = driver.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_wallets (
                agent_webid TEXT PRIMARY KEY NOT NULL,
                wallet_id INTEGER NOT NULL,
                gas_balance INTEGER NOT NULL DEFAULT 0,
                rjoule_balance INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );",
        );
    }

    pub fn insert_wallet(
        &self,
        agent: &WebID,
        wallet_id: i64,
        gas_balance: i64,
        rjoule_balance: i64,
    ) -> Result<(), AgentWalletError> {
        self.driver.execute(
            "INSERT INTO agent_wallets (agent_webid, wallet_id, gas_balance, rjoule_balance, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            &[
                DbValue::Text(agent.to_string()),
                DbValue::Integer(wallet_id),
                DbValue::Integer(gas_balance),
                DbValue::Integer(rjoule_balance),
            ],
        )?;
        Ok(())
    }

    pub fn update_gas_balance(
        &self,
        agent: &WebID,
        gas_balance: i64,
    ) -> Result<(), AgentWalletError> {
        self.driver.execute(
            "UPDATE agent_wallets SET gas_balance = ?1 WHERE agent_webid = ?2",
            &[
                DbValue::Integer(gas_balance),
                DbValue::Text(agent.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn get_wallet(&self, agent: &WebID) -> Result<Option<WalletRow>, AgentWalletError> {
        let rows: Vec<WalletRow> = query_map(
            &*self.driver,
            "SELECT agent_webid, wallet_id, gas_balance, rjoule_balance, created_at
             FROM agent_wallets WHERE agent_webid = ?1",
            &[DbValue::Text(agent.to_string())],
            |row| {
                Ok(WalletRow {
                    agent: WebID::from_persona(row.get_str(0)?.as_bytes()),
                    wallet_id: row.get_int(1)?,
                    gas_balance: row.get_int(2)?,
                    rjoule_balance: row.get_int(3)?,
                    created_at: row.get_str(4)?.to_string(),
                })
            },
        )?;
        Ok(rows.into_iter().next())
    }

    pub fn has_wallet(&self, agent: &WebID) -> Result<bool, AgentWalletError> {
        let count: i64 = query_row(
            &*self.driver,
            "SELECT COUNT(*) FROM agent_wallets WHERE agent_webid = ?1",
            &[DbValue::Text(agent.to_string())],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        Ok(count > 0)
    }

    pub fn next_wallet_id(&self) -> Result<i64, AgentWalletError> {
        let max_id: i64 = query_row(
            &*self.driver,
            "SELECT COALESCE(MAX(wallet_id), 0) FROM agent_wallets",
            &[],
            |row| row.get_int(0),
        )?
        .unwrap_or(0);
        Ok(max_id + 1)
    }
}

#[derive(Debug, Clone)]
pub struct WalletRow {
    pub agent: WebID,
    pub wallet_id: i64,
    pub gas_balance: i64,
    pub rjoule_balance: i64,
    pub created_at: String,
}
