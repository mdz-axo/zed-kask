#![cfg_attr(not(test), forbid(unsafe_code))]
//! hKask Wallet — rJoule payments, Hedera self-custody deposits, API key issuance.
//!
//! # Self-custody `[OUGHT-DECL]` (P1 — User Sovereignty)

// Used via derive macros (serde/thiserror/async_trait) — invisible to unused_crate_dependencies lint
#![allow(unused_crate_dependencies)]
//! hKask derives treasury keys from the user's master key via HKDF. No third
//! party holds the keys. The user controls their funds at all times.
//! Chain port (`hedera.rs`) interacts directly with blockchain endpoints.
//!
//! # Security `[OUGHT-DECL]`
//! - `signing.rs` — isolated security boundary for all key operations
//! - Per-operation key loading: keys derived via HKDF, used, zeroized immediately
//! - No long-lived treasury key material
//! - API key private keys returned to user once, never stored by hKask
//! - `Zeroizing` wrappers on all secret key material
//!
//! # Crate Map
//! - `chain.rs` — `ChainPort` trait + `DepositEvent`
//! - `signing.rs` — Isolated signing module (security boundary)
//! - `manager.rs` — `WalletManager` + deposit reference logic
//! - `issuer.rs` — `ApiKeyIssuer` + `ApiKeyMaterial`
//! - `price_feed.rs` — `PriceFeed` trait + fee estimation
//! - `hedera.rs` — `HederaPort` (feature-gated: "hedera")

pub mod chain;
pub mod issuer;
pub mod manager;
pub mod price_feed;
pub mod reg_span;
pub mod signing;
pub mod types;

#[cfg(feature = "hedera")]
pub mod hedera;

pub use chain::{ChainPort, DepositEvent};
pub use issuer::ApiKeyIssuer;
pub use manager::WalletManager;
pub use price_feed::{
    CoinGeckoPriceFeed, CompositePriceFeed, EodhdPriceFeed, ExchangeRate, PriceFeed,
    StaticPriceFeed, WithdrawalFee, estimate_withdrawal_fee, resolve_price_feed,
};
pub use signing::{sign_capability, sign_withdrawal};

pub use types::{
    ApiKeyCapability, ApiKeyMaterial, ChainId, DepositAddress, DepositReference, Encumbrance,
    EncumbranceStatus, GAS_PER_RJOULE, PriceFeedConfig, PrivacyMode, RJ_PER_USDC, RJoule,
    RateLimitConfig, TransactionType, TxHash, WalletBalance, WalletConfig, WalletError,
    WalletTransaction,
};
