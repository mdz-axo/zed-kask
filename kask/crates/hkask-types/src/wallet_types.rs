//! Wallet types — Hedera-only wallet value types.
//!
//! These types are needed by hkask-storage (WalletStore) which sits below
//! hkask-wallet in the dependency chain. The hkask-wallet crate re-exports
//! them so downstream code can use `hkask_wallet::RJoule` etc.
//!
//! # Epistemic frame (pragmatic-semantics)
//! - rJoule is an internal accounting unit `[OUGHT-DECL]` — not an on-chain token
//! - Every rJoule originates from a verified on-chain deposit `[IS-DECL]`
//! - API keys are Ed25519-signed OCAP capability tokens `[OUGHT-DECL]`

use crate::{ApiKeyId, Ed25519PublicKey, InfrastructureError, WalletId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// ── ChainId ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ChainId {
    #[default]
    Hedera,
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hedera")
    }
}

impl FromStr for ChainId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hedera" => Ok(ChainId::Hedera),
            other => Err(format!("unknown chain: {other}")),
        }
    }
}

// ── PrivacyMode ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PrivacyMode {
    #[default]
    Transparent,
}

impl fmt::Display for PrivacyMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "transparent")
    }
}

impl FromStr for PrivacyMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "transparent" => Ok(PrivacyMode::Transparent),
            other => Err(format!("unknown privacy mode: {other}")),
        }
    }
}

// ── DepositAddress ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositAddress {
    pub address: String,
    pub chain: ChainId,
    pub privacy_mode: PrivacyMode,
}

impl fmt::Display for DepositAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.chain, self.address)
    }
}

// ── DepositReference ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositReference {
    pub reference: String,
    pub wallet_id: WalletId,
    pub chain: ChainId,
    pub nonce: [u8; 16],
    pub expires_at: DateTime<Utc>,
}

impl fmt::Display for DepositReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dep_{} (wallet: {}, expires: {})",
            self.reference, self.wallet_id, self.expires_at
        )
    }
}

// ── Economic constants ─────────────────────────────────────────────────────────

/// Gas cycles per 1 rJoule (250,000 gas = 1 rJ = $1.00).
/// This is the authoritative conversion constant for the system.
pub const GAS_PER_RJOULE: u64 = 250_000;

// ── RJoule — stable value unit ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RJoule(pub u64);

impl RJoule {
    pub const ZERO: RJoule = RJoule(0);

    pub fn new(value: u64) -> Self {
        RJoule(value)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn saturating_add(self, other: RJoule) -> RJoule {
        RJoule(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: RJoule) -> RJoule {
        RJoule(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for RJoule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} rJ", self.0)
    }
}

// ── WalletConfig ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    pub rj_per_usdc: u64,
    pub gas_per_rjoule: u64,
    pub min_deposit_usdc_micro: u64,
    pub enabled_chains: Vec<ChainId>,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            rj_per_usdc: 1,
            gas_per_rjoule: GAS_PER_RJOULE,
            min_deposit_usdc_micro: 1_000_000,
            enabled_chains: vec![ChainId::Hedera],
        }
    }
}

// ── WalletBalance ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    pub wallet_id: WalletId,
    pub rjoules: u64,
    pub usdc_equivalent_micro: u64,
    pub gas_equivalent: u64,
}

impl fmt::Display for WalletBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} rJ  (~{:.6} USDC, ~{} gas)",
            self.rjoules,
            self.usdc_equivalent_micro as f64 / 1_000_000.0,
            self.gas_equivalent
        )
    }
}

// ── TransactionType ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Deposit {
        chain: ChainId,
        privacy: PrivacyMode,
        tx_hash: String,
        amount_usdc_micro: u64,
    },
    Withdrawal {
        chain: ChainId,
        privacy: PrivacyMode,
        tx_hash: String,
        amount_usdc_micro: u64,
    },
    Spend {
        key_id: ApiKeyId,
        tool: String,
        gas: u64,
        rj: RJoule,
    },
    Refund {
        key_id: ApiKeyId,
        reason: String,
        rj: RJoule,
    },
    Shield {
        chain: ChainId,
        tx_hash: String,
        amount_usdc_micro: u64,
    },
}

// ── WalletTransaction ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletTransaction {
    pub id: u64,
    pub wallet_id: WalletId,
    pub tx_type: TransactionType,
    pub rjoules_delta: i64,
    pub balance_after: u64,
    pub timestamp: DateTime<Utc>,
}

// ── WalletError ────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    #[error("infrastructure error: {0}")]
    Infra(InfrastructureError),

    #[error("insufficient rJoule balance: have {have}, need {need}")]
    InsufficientBalance { have: RJoule, need: RJoule },

    #[error("API key {key_id} spending limit exceeded: {spent} / {limit}")]
    SpendingLimitExceeded {
        key_id: ApiKeyId,
        spent: RJoule,
        limit: RJoule,
    },

    #[error("API key {key_id} expired at {expiry}")]
    KeyExpired {
        key_id: ApiKeyId,
        expiry: DateTime<Utc>,
    },

    #[error("API key {key_id} has been revoked")]
    KeyRevoked { key_id: ApiKeyId },

    #[error("deposit reference {reference} not found or expired")]
    DepositReferenceInvalid { reference: String },

    #[error("deposit address unresolvable: {address}")]
    DepositAddressUnresolvable { address: String },

    #[error("chain error ({chain}): {message}")]
    ChainError { chain: ChainId, message: String },

    #[error("API key {key_id} already has an active encumbrance")]
    EncumbranceAlreadyExists { key_id: ApiKeyId },

    #[error("no active encumbrance found for API key {key_id}")]
    EncumbranceNotFound { key_id: ApiKeyId },

    #[error(
        "encumbrance for key {key_id} has insufficient remaining: have {remaining}, need {need}"
    )]
    EncumbranceInsufficient {
        key_id: ApiKeyId,
        remaining: RJoule,
        need: RJoule,
    },

    #[error("settlement exceeds reservation: reserved {reserved}, actual {actual}")]
    ReservationExceeded { reserved: RJoule, actual: RJoule },
}

impl From<InfrastructureError> for WalletError {
    fn from(e: InfrastructureError) -> Self {
        WalletError::Infra(e)
    }
}

impl From<crate::DbError> for WalletError {
    fn from(e: crate::DbError) -> Self {
        WalletError::Infra(InfrastructureError::from(e))
    }
}

impl From<uuid::Error> for WalletError {
    fn from(e: uuid::Error) -> Self {
        WalletError::Infra(InfrastructureError::Serialization(e.to_string()))
    }
}

// ── RateLimitConfig ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub tokens_per_day: u64,
}

// ── ApiKeyCapability ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyCapability {
    pub wallet_id: WalletId,
    pub key_id: ApiKeyId,
    pub public_key: Ed25519PublicKey,
    pub spending_limit_rj: RJoule,
    pub spent_rj: RJoule,
    pub scope: Vec<String>,
    pub purpose: String,
    pub rate_limit: Option<RateLimitConfig>,
    pub expiry: Option<DateTime<Utc>>,
    pub issued_at: DateTime<Utc>,
    pub privacy_mode: PrivacyMode,
    #[serde(default)]
    pub preferred_chain: Option<ChainId>,
}

impl ApiKeyCapability {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expiry.is_some_and(|exp| now > exp)
    }

    pub fn remaining_rj(&self) -> RJoule {
        self.spending_limit_rj.saturating_sub(self.spent_rj)
    }
}

// ── EncumbranceStatus ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncumbranceStatus {
    Active,
    Consumed,
    Released,
}

impl EncumbranceStatus {
    /// Check whether a transition from `self` to `target` is valid.
    ///
    /// Encumbrance lifecycle:
    ///   Active → Consumed  (reservation was exercised)
    ///   Active → Released  (reservation was cancelled)
    /// Both Consumed and Released are terminal states.
    pub fn can_transition_to(&self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Active, Self::Consumed) | (Self::Active, Self::Released)
        )
    }

    /// Returns true if this is a terminal state (no further transitions possible).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Consumed | Self::Released)
    }
}

impl fmt::Display for EncumbranceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Consumed => write!(f, "consumed"),
            Self::Released => write!(f, "released"),
        }
    }
}

impl FromStr for EncumbranceStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "consumed" => Ok(Self::Consumed),
            "released" => Ok(Self::Released),
            other => Err(format!("unknown encumbrance status: {other}")),
        }
    }
}

// ── Encumbrance ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encumbrance {
    pub key_id: ApiKeyId,
    pub wallet_id: WalletId,
    pub amount_rj: u64,
    pub consumed_rj: u64,
    pub status: EncumbranceStatus,
    pub created_at: String,
    pub released_at: Option<String>,
}

impl Encumbrance {
    pub fn remaining_rj(&self) -> u64 {
        self.amount_rj.saturating_sub(self.consumed_rj)
    }

    pub fn is_active(&self) -> bool {
        self.status == EncumbranceStatus::Active
    }
}
