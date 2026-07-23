//! Wallet — Per-agent gas/rJoule balance store.
//!
//! Backed by SQLite via AgentWalletStore. Wallets are created by the Curator daemon
//! on userpod registration. Agents spend from wallets via WalletBackedBudget.

use crate::agent_wallet_store::{AgentWalletError, WalletStore};
use crate::energy::{GasCost, GasError};
use crate::well::WellManager;
use hkask_types::WebID;
use std::sync::Arc;
use tokio::sync::RwLock;

impl From<AgentWalletError> for GasError {
    fn from(e: AgentWalletError) -> Self {
        GasError::Persistence(e.to_string())
    }
}

/// Current balance of an agent's wallet.
#[derive(Debug, Clone, Copy)]
pub struct WalletBalance {
    pub gas: u64,
    pub rjoule: u64,
}

/// Manages agent wallets — creation, spending, balance queries.
/// Uses SQLite via WalletStore for persistence.
pub struct WalletManager {
    store: Option<Arc<WalletStore>>,
    well_manager: Option<Arc<RwLock<WellManager>>>,
}

impl WalletManager {
    /// Create a WalletManager with no persistence (in-memory only — for tests).
    pub fn new_in_memory() -> Self {
        Self {
            store: None,
            well_manager: None,
        }
    }

    pub fn new(store: Arc<WalletStore>) -> Self {
        Self {
            store: Some(store),
            well_manager: None,
        }
    }

    /// Attach a WellManager for auto-draw on low balance.
    pub fn with_well(mut self, well: Arc<RwLock<WellManager>>) -> Self {
        self.well_manager = Some(well);
        self
    }

    fn store(&self) -> Result<&WalletStore, GasError> {
        self.store
            .as_ref()
            .map(|s| s.as_ref())
            .ok_or_else(|| GasError::Persistence("No wallet store configured".into()))
    }

    /// Create a wallet for an agent. Returns the wallet ID.
    pub async fn create_wallet(
        &self,
        agent: WebID,
        initial_gas: GasCost,
        initial_rjoule: u64,
    ) -> Result<i64, GasError> {
        let store = self.store()?;
        if store.has_wallet(&agent)? {
            return Err(GasError::Persistence(format!(
                "Wallet already exists for agent {agent}"
            )));
        }

        // B: Draw initial balance from Well if available
        let (actual_gas, actual_rj) = if let Some(ref well) = self.well_manager {
            let mut w = well.write().await;
            w.draw_from_default(initial_gas, initial_rjoule)
                .unwrap_or((GasCost(0), 0))
        } else {
            (initial_gas, initial_rjoule)
        };

        let id = store.next_wallet_id()?;
        store.insert_wallet(&agent, id, actual_gas.0 as i64, actual_rj as i64)?;
        Ok(id)
    }

    /// Spend gas from an agent's wallet. Auto-draws from Well if balance is low.
    pub async fn spend(&self, agent: &WebID, amount: GasCost) -> Result<GasCost, GasError> {
        let store = self.store()?;
        let row = store
            .get_wallet(agent)?
            .ok_or_else(|| GasError::Persistence(format!("No wallet for agent {agent}")))?;

        let balance = row.gas_balance as u64;
        let effective_balance = if balance < amount.0 {
            // A: Auto-draw from Well when balance is insufficient
            if let Some(ref well) = self.well_manager {
                let draw_amount = GasCost(amount.0 - balance);
                let mut w = well.write().await;
                if let Ok((drawn, _)) = w.draw_from_default(draw_amount, 0) {
                    let new_balance = (balance + drawn.0) as i64;
                    store.update_gas_balance(agent, new_balance)?;
                    balance + drawn.0
                } else {
                    balance
                }
            } else {
                balance
            }
        } else {
            balance
        };

        let spent = amount.0.min(effective_balance);
        let new_balance = effective_balance.saturating_sub(spent) as i64;
        store.update_gas_balance(agent, new_balance)?;
        Ok(GasCost(spent))
    }

    /// Query an agent's wallet balance.
    pub async fn balance(&self, agent: &WebID) -> Result<WalletBalance, GasError> {
        let store = self.store()?;
        let row = store
            .get_wallet(agent)?
            .ok_or_else(|| GasError::Persistence(format!("No wallet for agent {agent}")))?;
        Ok(WalletBalance {
            gas: row.gas_balance as u64,
            rjoule: row.rjoule_balance as u64,
        })
    }

    /// Deposit gas into an agent's wallet.
    pub async fn deposit_gas(&self, agent: &WebID, amount: GasCost) -> Result<GasCost, GasError> {
        let store = self.store()?;
        let row = store
            .get_wallet(agent)?
            .ok_or_else(|| GasError::Persistence(format!("No wallet for agent {agent}")))?;

        let new_balance = (row.gas_balance as u64).saturating_add(amount.0) as i64;
        store.update_gas_balance(agent, new_balance)?;
        Ok(amount)
    }

    /// Check if a wallet has sufficient gas.
    pub async fn can_proceed(&self, agent: &WebID, amount: GasCost) -> bool {
        match self.balance(agent).await {
            Ok(b) => b.gas >= amount.0,
            Err(_) => false,
        }
    }

    /// Check if a wallet exists for the agent.
    pub async fn has_wallet(&self, agent: &WebID) -> bool {
        if let Ok(store) = self.store() {
            store.has_wallet(agent).unwrap_or(false)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_storage::database::sqlite::SqliteDriver;
    use std::sync::Arc;

    fn make_manager() -> WalletManager {
        let pool = SqliteDriver::in_memory_pool().expect("in-memory pool");
        let store = Arc::new(WalletStore::from_driver(Arc::new(SqliteDriver::new(pool))));
        WalletManager::new(store)
    }

    #[tokio::test]
    async fn create_and_query_wallet() {
        let mgr = make_manager();
        let agent = WebID::from_persona(b"test-agent");
        let id = mgr.create_wallet(agent, GasCost(1000), 500).await.unwrap();
        assert!(id > 0);

        let balance = mgr.balance(&agent).await.unwrap();
        assert_eq!(balance.gas, 1000);
        assert_eq!(balance.rjoule, 500);
    }

    #[tokio::test]
    async fn spend_reduces_balance() {
        let mgr = make_manager();
        let agent = WebID::from_persona(b"test-agent");
        mgr.create_wallet(agent, GasCost(1000), 0).await.unwrap();

        let spent = mgr.spend(&agent, GasCost(300)).await.unwrap();
        assert_eq!(spent.0, 300);

        let balance = mgr.balance(&agent).await.unwrap();
        assert_eq!(balance.gas, 700);
    }

    #[tokio::test]
    async fn spend_capped_at_balance() {
        let mgr = make_manager();
        let agent = WebID::from_persona(b"test-agent");
        mgr.create_wallet(agent, GasCost(100), 0).await.unwrap();

        let spent = mgr.spend(&agent, GasCost(500)).await.unwrap();
        assert_eq!(spent.0, 100);
        assert!(!mgr.can_proceed(&agent, GasCost(1)).await);
    }

    #[tokio::test]
    async fn deposit_adds_gas() {
        let mgr = make_manager();
        let agent = WebID::from_persona(b"test-agent");
        mgr.create_wallet(agent, GasCost(100), 0).await.unwrap();
        mgr.deposit_gas(&agent, GasCost(200)).await.unwrap();

        let balance = mgr.balance(&agent).await.unwrap();
        assert_eq!(balance.gas, 300);
    }

    #[tokio::test]
    async fn duplicate_wallet_rejected() {
        let mgr = make_manager();
        let agent = WebID::from_persona(b"test-agent");
        mgr.create_wallet(agent, GasCost(100), 0).await.unwrap();
        let result = mgr.create_wallet(agent, GasCost(200), 0).await;
        assert!(result.is_err());
    }
}
