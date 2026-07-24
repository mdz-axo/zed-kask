//! Wallet types for hKask — rJoule payments, multi-chain deposits, API key capabilities.
//!
//! # Epistemic frame (pragmatic-semantics)
//! - rJoule is an internal accounting unit `[OUGHT-DECL]` — not an on-chain token
//! - Every rJoule originates from a verified on-chain deposit `[IS-DECL]`
//! - API keys are Ed25519-signed OCAP capability tokens `[OUGHT-DECL]`

pub mod chain;
pub mod error;
// Re-exports from hkask_wallet_types (canonical source for these types)
pub use hkask_types::{
    ApiKeyCapability, ApiKeyMaterial, ChainId, DepositAddress, DepositReference, Encumbrance,
    EncumbranceStatus, GAS_PER_RJOULE, PriceFeedConfig, PrivacyMode, RJ_PER_USDC, RJoule,
    RateLimitConfig, TransactionType, TxHash, WalletBalance, WalletConfig, WalletError,
    WalletTransaction,
};
