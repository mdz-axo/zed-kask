//! ChainPort — abstract interface for blockchain deposit monitoring and withdrawal.
//!
//! # Design (idiomatic-rust: composition over inheritance)
//! `ChainPort` is a trait for capability, not a base class. Each implementation
//! is a standalone struct with its own RPC connection and state.
//!
//! # Implementations
//! - `HederaPort` — HTS USDC on Hedera (feature-gated: "hedera")

use crate::types::{ChainId, TxHash, WalletError};
use chrono::{DateTime, Utc};
use hkask_types::WebID;

/// A confirmed on-chain deposit detected by a ChainPort implementation.
///
/// # Invariant `[OUGHT-DECL]`
/// `amount_usdc_micro > 0` — zero-amount deposits are rejected at construction.
#[derive(Debug, Clone)]
pub struct DepositEvent {
    pub tx_hash: TxHash,
    pub from_address: String,
    pub to_address: String,
    /// Amount in micro-USDC (1 = 0.000001 USDC)
    pub amount_usdc_micro: u64,
    pub confirmations: u64,
    pub block_time: DateTime<Utc>,
}

impl DepositEvent {
    /// Create a validated deposit event. Rejects zero-amount deposits.
    pub fn new(
        tx_hash: TxHash,
        from_address: String,
        to_address: String,
        amount_usdc_micro: u64,
        confirmations: u64,
        block_time: DateTime<Utc>,
    ) -> Result<Self, WalletError> {
        if amount_usdc_micro == 0 {
            return Err(WalletError::Infra(
                hkask_types::InfrastructureError::database(
                    "deposit amount must be greater than zero".to_string(),
                ),
            ));
        }
        Ok(DepositEvent {
            tx_hash,
            from_address,
            to_address,
            amount_usdc_micro,
            confirmations,
            block_time,
        })
    }
}

/// Abstract interface for blockchain deposit monitoring and withdrawal.
///
/// # Security `[OUGHT-DECL]`
/// Implementations do NOT hold treasury keys. Signing is delegated to the
/// isolated `signing.rs` module. ChainPort only constructs unsigned transactions.

#[async_trait::async_trait]
pub trait ChainPort: Send + Sync {
    /// Which chain this port serves.
    fn chain_id(&self) -> ChainId;

    /// Derive a new deposit address from the treasury seed + index.
    /// Deterministic: same seed + index → same address.
    fn derive_deposit_address(&self, index: u64) -> Result<String, WalletError>;

    /// Poll the blockchain for deposits to the given addresses.
    /// Returns confirmed deposits not yet recorded.
    async fn monitor_deposits(
        &self,
        actor: &WebID,
        addresses: &[String],
    ) -> Result<Vec<DepositEvent>, WalletError>;

    /// Build an unsigned withdrawal transaction.
    /// Signing happens in the isolated `signing.rs` module.
    fn build_withdrawal_tx(
        &self,
        to_address: &str,
        amount_usdc_micro: u64,
    ) -> Result<Vec<u8>, WalletError>;

    /// Submit a signed transaction to the blockchain.
    async fn submit_signed_tx(
        &self,
        actor: &WebID,
        signed_tx_bytes: &[u8],
    ) -> Result<TxHash, WalletError>;

    /// Get the number of confirmations for a transaction.
    async fn confirmations(&self, actor: &WebID, tx_hash: &TxHash) -> Result<u64, WalletError>;
}
