//! Pluggable Lightning rail trait and cryptographic preimage verification (D110 Phase 3).
//!
//! v1 implements [`LightningRail`] via NIP-47 NWC (`kask_bridge::nwc_rail`)
//! executed editor-side behind native spend approval. Keeping the trait here
//! leaves future rails (e.g. embedded `ldk-node`) pluggable without touching
//! `hkask-mcp-wallet` or the spend-approval seam.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::amount::Millisatoshis;

/// Lifecycle state of a Lightning invoice returned by `lookup_invoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightningInvoiceState {
    Pending,
    Settled,
    Failed,
    Expired,
}

impl LightningInvoiceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Settled => "settled",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}

/// Result of creating an inbound Lightning receive invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedLightningInvoice {
    pub bolt11: String,
    pub payment_hash: String,
    pub amount_msat: Millisatoshis,
    pub expires_at: u64,
}

/// Verified outcome of paying an outbound BOLT11 invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightningPaymentResult {
    pub preimage: String,
    pub payment_hash: String,
    pub fee_paid_msat: Millisatoshis,
}

/// Outcome of querying an invoice's state by payment hash (reconciliation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightningInvoiceLookup {
    pub payment_hash: String,
    pub state: LightningInvoiceState,
    pub preimage: Option<String>,
    pub fee_paid_msat: Option<Millisatoshis>,
}

/// Typed Lightning rail errors. Never echoes raw NWC URIs or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LightningError {
    #[error("Lightning rail is not configured")]
    NotConfigured,
    #[error("Lightning payment timed out — status unknown; run wallet_reconcile")]
    Timeout,
    #[error("Lightning preimage does not match expected payment_hash")]
    InvalidPreimage,
    #[error("Lightning service returned a malformed response: {0}")]
    MalformedResponse(String),
    #[error("Lightning payment failed: {0}")]
    PaymentFailed(String),
    #[error("Lightning transport error: {0}")]
    Transport(String),
}

/// Cryptographically verify that `SHA-256(preimage) == expected_payment_hash`.
///
/// Both inputs must be 64-character hex strings (32 bytes). Fails closed on
/// malformed hex, wrong byte length, or hash mismatch.
pub fn verify_preimage_matches_payment_hash(
    preimage_hex: &str,
    expected_payment_hash_hex: &str,
) -> Result<(), LightningError> {
    let preimage_bytes =
        hex::decode(preimage_hex.trim()).map_err(|_| LightningError::InvalidPreimage)?;
    if preimage_bytes.len() != 32 {
        return Err(LightningError::InvalidPreimage);
    }
    let expected_bytes = hex::decode(expected_payment_hash_hex.trim())
        .map_err(|_| LightningError::InvalidPreimage)?;
    if expected_bytes.len() != 32 {
        return Err(LightningError::InvalidPreimage);
    }
    let computed = Sha256::digest(&preimage_bytes);
    if computed.as_slice() != expected_bytes.as_slice() {
        return Err(LightningError::InvalidPreimage);
    }
    Ok(())
}

/// Pluggable Lightning payment backend (v1: NIP-47 NWC; future: `ldk-node`).
#[async_trait]
pub trait LightningRail: Send + Sync {
    /// Query available Lightning balance in millisatoshis.
    async fn get_balance_msat(&self) -> Result<Millisatoshis, LightningError>;

    /// Create an inbound BOLT11 invoice.
    async fn make_invoice(
        &self,
        amount_msat: Millisatoshis,
        description: Option<String>,
        expiry_secs: Option<u32>,
    ) -> Result<CreatedLightningInvoice, LightningError>;

    /// Pay a validated BOLT11 invoice and cryptographically verify the returned
    /// preimage against `expected_payment_hash`.
    async fn pay_invoice(
        &self,
        bolt11: &str,
        expected_payment_hash: &str,
    ) -> Result<LightningPaymentResult, LightningError>;

    /// Look up an invoice by payment hash (used by `wallet_reconcile`).
    async fn lookup_invoice(
        &self,
        payment_hash: &str,
    ) -> Result<LightningInvoiceLookup, LightningError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preimage_verification_accepts_exact_sha256_and_rejects_tampering() {
        let preimage = [0x42u8; 32];
        let preimage_hex = hex::encode(preimage);
        let payment_hash_hex = hex::encode(Sha256::digest(preimage));

        assert!(verify_preimage_matches_payment_hash(&preimage_hex, &payment_hash_hex).is_ok());
        assert!(
            verify_preimage_matches_payment_hash(&preimage_hex.to_uppercase(), &payment_hash_hex)
                .is_ok()
        );

        let wrong_preimage = hex::encode([0x43u8; 32]);
        assert_eq!(
            verify_preimage_matches_payment_hash(&wrong_preimage, &payment_hash_hex),
            Err(LightningError::InvalidPreimage)
        );
        assert_eq!(
            verify_preimage_matches_payment_hash("not-hex", &payment_hash_hex),
            Err(LightningError::InvalidPreimage)
        );
        assert_eq!(
            verify_preimage_matches_payment_hash("abcd", &payment_hash_hex),
            Err(LightningError::InvalidPreimage)
        );
    }
}
