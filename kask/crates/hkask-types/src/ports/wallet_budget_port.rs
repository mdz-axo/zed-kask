//! WalletBudgetPort — hexagonal port for wallet energy budget operations.
//!
//! Regulation depends on this port trait instead of concrete `WalletManager`.
//! This inverts the dependency per hexagonal architecture: Regulation defines
//! the interface it needs, and `regulation::WalletManager` implements it.
//!
//! Per Conant-Ashby (Good Regulator theorem): the regulator must model
//! the system it regulates. This port IS the regulator's model of the
//! wallet — an abstract interface, not the concrete type.

use crate::id::{ApiKeyId, WalletId};
use crate::{ApiKeyCapability, Encumbrance, RJoule};

/// Errors produced by wallet budget operations.
#[derive(Debug, thiserror::Error)]
pub enum WalletBudgetError {
    #[error("wallet error: {0}")]
    Wallet(String),
    #[error("insufficient balance: wallet {wallet_id} has {available} rJ, needs {required} rJ")]
    InsufficientBalance {
        wallet_id: String,
        available: u64,
        required: u64,
    },
}

/// Port trait for wallet energy budget operations.
///
/// Regulation uses this to:
/// - Convert gas costs to rJoule amounts
/// - Check encumbrance status for API keys
/// - Verify affordability before spending
/// - Retrieve API key capabilities
/// - Read and adjust the gas→rJoule conversion rate
///
/// Implementations: `hkask_regulation::wallet_manager::WalletManager` implements this trait.
/// Regulation holds `Arc<dyn WalletBudgetPort>` instead of `Arc<WalletManager>`.
pub trait WalletBudgetPort: Send + Sync {
    /// Convert gas units to rJoule using the current conversion rate.
    fn gas_to_rjoules(&self, gas: u64) -> RJoule;

    /// Get the current encumbrance for an API key, if any.
    fn get_encumbrance(&self, key_id: ApiKeyId) -> Option<Encumbrance>;

    /// Emit a wallet key health alert (expired/exhausted).
    fn emit_key_alert(&self, key_id: ApiKeyId, exhausted: bool, expired: bool);

    /// Check if a wallet can afford a given rJoule cost.
    fn can_afford(&self, wallet_id: WalletId, cost_rj: RJoule) -> bool;

    /// Get API key capability metadata for a key ID.
    fn get_api_key(&self, key_id: ApiKeyId) -> Option<ApiKeyCapability>;

    /// Get the current wallet balance.
    fn get_balance(&self, wallet_id: WalletId) -> Result<crate::WalletBalance, WalletBudgetError>;

    /// Get the current gas→rJoule conversion rate.
    fn gas_per_rjoule(&self) -> u64;

    /// Set the gas→rJoule conversion rate (Regulation calibration).
    fn set_gas_per_rjoule(&self, rate: u64);

    /// Consume rJoules from an API key's encumbrance (atomic debit).
    /// Returns Err if the key has insufficient remaining encumbrance.
    fn consume(&self, key_id: ApiKeyId, gas_rj: RJoule) -> Result<(), WalletBudgetError>;

    /// Settle rJoules after an operation — debits actual from wallet balance.
    /// `reserved_rj` is the originally reserved amount; `actual_rj` is what was consumed.
    fn settle_rjoules(
        &self,
        wallet_id: WalletId,
        reserved_rj: RJoule,
        actual_rj: RJoule,
    ) -> Result<(), WalletBudgetError>;
}
