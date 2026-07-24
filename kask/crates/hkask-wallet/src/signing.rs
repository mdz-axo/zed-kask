//! Signing module — isolated security boundary for all key operations.
//!
//! # Security Boundary `[OUGHT-DECL]`
//! This is the ONLY module where treasury key material is loaded, used for
//! signing, and zeroized. No un-zeroized key material ever leaves this module.
//! All functions accept transaction bytes and return signatures — keys are
//! loaded, used, and dropped within each call.
//!
//! # Audit Surface
//! This is the single module a security auditor must review for key handling
//! correctness. All other modules operate on already-signed data or public keys.
//!
//! # Specialized sub-wallet scope `[OUGHT-DECL]`
//! hKask wallet is a specialized sub-wallet — one of several wallets the user
//! holds. It only signs two things:
//! 1. Withdrawal transactions (USDC from treasury → user's primary wallet)
//! 2. API key capability tokens (Ed25519 signatures proving wallet ownership)
//!
//! It does NOT sign deposit transactions (user's primary wallet signs those).

use crate::types::{ApiKeyCapability, ChainId, WalletError};
use ed25519_dalek::Signer;
use hkask_keystore::keychain::resolve_treasury_key;
use hkask_types::secret::derivation_contexts;
use zeroize::Zeroizing;

/// Loaded signing key — exists only within signing.rs.
///
/// # Security `[OUGHT-DECL]`
/// - Wrapped in `Zeroizing<[u8; 32]>` for automatic zeroize on drop
/// - `Debug` impl redacts key material (P2-wallet-signing-debug-redact)
/// - Never leaves this module in un-zeroized form (P2-wallet-signing-key-boundary)
struct LoadedKey {
    bytes: Zeroizing<[u8; 32]>,
}

impl LoadedKey {
    fn from_zeroizing(key: Zeroizing<Vec<u8>>) -> Result<Self, WalletError> {
        let arr: [u8; 32] = key[..32].try_into().map_err(|_| {
            WalletError::Infra(hkask_types::InfrastructureError::database(
                "treasury key must be 32 bytes".to_string(),
            ))
        })?;
        Ok(LoadedKey {
            bytes: Zeroizing::new(arr),
        })
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl std::fmt::Debug for LoadedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

/// Sign a withdrawal transaction for a specific chain.
///
/// expect: "The system signs withdrawal transactions with my treasury key"
/// \[P9\] Motivating: Homeostatic Self-Regulation — signing authorizes energy outflow
/// \[P1\] Constraining: User Sovereignty — treasury key derived from user master key
/// \[P4\] Constraining: Clear Boundaries — key material never leaves this module
/// pre:  chain is a valid ChainId, tx_bytes is non-empty
/// post: returns Ok(signature) — 64-byte Ed25519 signature
/// post: treasury key loaded, used, and zeroized within this call
/// post: no key material returned to caller — only the signature
///
/// Loads the chain-specific treasury key via HKDF, signs the transaction bytes,
/// and zeroizes the key on drop. Key material exists in memory only for the
/// duration of this function call.
///
/// # Security
/// - Treasury key is ``Zeroizing<Vec<u8>>`` — automatically zeroed on drop
/// - No key material is returned to the caller — only the signature
/// - Per-operation key loading: key derived fresh each call, not held long-term
pub fn sign_withdrawal(chain: ChainId, tx_bytes: &[u8]) -> Result<Vec<u8>, WalletError> {
    sign_bytes(chain_to_context(&chain), tx_bytes)
}

/// Sign an arbitrary message with the Hinkal treasury key.
///
/// expect: "The system signs Hinkal privacy messages proving treasury origin"
/// \[P9\] Motivating: Homeostatic Self-Regulation — Hinkal session signing authorizes privacy-layer flow
/// \[P4\] Constraining: Clear Boundaries — message is opaque bytes; signature proves treasury origin
/// pre:  message is any byte slice (including empty)
/// post: returns Ok(signature) — 64-byte Ed25519 signature
/// post: treasury key loaded, used, and zeroized within this call
pub fn sign_message(message: &[u8]) -> Result<Vec<u8>, WalletError> {
    sign_bytes(derivation_contexts::TREASURY_HEDERA, message)
}

/// Map ChainId to its HKDF derivation context string.
fn chain_to_context(chain: &ChainId) -> &'static str {
    match chain {
        ChainId::Hedera => derivation_contexts::TREASURY_HEDERA,
    }
}

fn sign_bytes(context: &str, bytes: &[u8]) -> Result<Vec<u8>, WalletError> {
    let treasury_key: Zeroizing<Vec<u8>> = resolve_treasury_key(context).map_err(|e| {
        WalletError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
    })?;

    let loaded = LoadedKey::from_zeroizing(treasury_key)?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(loaded.as_bytes());
    let signature = signing_key.sign(bytes);
    Ok(signature.to_bytes().to_vec())
    // loaded (Secret) drops here → key material zeroed
    // treasury_key (Zeroizing) already dropped at from_zeroizing
}

/// Sign an API key capability token with the wallet's Ed25519 key.
///
/// expect: "The system signs capability tokens for API key delegation"
/// \[P9\] Motivating: Homeostatic Self-Regulation — signing authorizes API key capability
/// \[P1\] Constraining: User Sovereignty — treasury key derived from user master key
/// \[P4\] Constraining: Clear Boundaries — key material never leaves this module
/// pre:  capability is a valid, fully-populated ApiKeyCapability
/// post: returns Ok(hex_signature) — 128-char hex-encoded Ed25519 signature
/// post: delegates to hkask_keystore::sign_api_key_capability (isolated boundary)
///
/// Delegates to `hkask_keystore::sign_api_key_capability` which handles
/// wallet seed derivation, canonical JSON serialization, signing, and
/// zeroizing internally.
///
/// # Returns
/// 64-byte Ed25519 signature as a hex-encoded string (128 hex chars).
pub fn sign_capability(capability: &ApiKeyCapability) -> Result<String, WalletError> {
    let canonical_bytes = serde_json::to_vec(capability).map_err(|e| {
        WalletError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
    })?;
    hkask_keystore::keychain::sign_wallet_bytes(&canonical_bytes)
        .map_err(|e| WalletError::Infra(hkask_types::InfrastructureError::database(e.to_string())))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChainId, PrivacyMode, RJoule};
    use hkask_types::crypto::Ed25519PublicKey;
    use hkask_types::id::{ApiKeyId, WalletId};

    fn set_test_master_key() {
        // SAFETY: test-only — sets master key env var in isolated test process.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
    }

    /// expect: "Wallet sign withdrawal signature test works correctly under test conditions"
    #[test]
    fn sign_withdrawal_produces_signature() {
        set_test_master_key();
        let tx_bytes = b"test withdrawal transaction";
        let sig = sign_withdrawal(ChainId::Hedera, tx_bytes).unwrap();
        assert_eq!(sig.len(), 64); // Ed25519 signature is 64 bytes
    }

    /// expect: "Wallet sign withdrawal per chain test works correctly under test conditions"
    #[test]
    fn sign_withdrawal_differs_per_chain() {
        set_test_master_key();
        // Signatures differ per message — verifies signing is not degenerate
        let sig_a = sign_withdrawal(ChainId::Hedera, b"tx a").unwrap();
        let sig_b = sign_withdrawal(ChainId::Hedera, b"tx b").unwrap();
        assert_ne!(sig_a, sig_b);
    }

    /// expect: "Wallet sign capability hex test works correctly under test conditions"
    #[test]
    fn sign_capability_produces_hex_signature() {
        set_test_master_key();
        let cap = ApiKeyCapability {
            wallet_id: WalletId::new(),
            key_id: ApiKeyId::new(),
            public_key: Ed25519PublicKey([0u8; 32]),
            spending_limit_rj: RJoule::new(5000),
            spent_rj: RJoule::ZERO,
            scope: vec!["read-specs".to_string()],
            purpose: "signing test".to_string(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        let sig = sign_capability(&cap).unwrap();
        assert_eq!(sig.len(), 128); // 64 bytes → 128 hex chars
    }

    /// expect: "Wallet sign withdrawal all chains test works correctly under test conditions"
    #[test]
    fn sign_withdrawal_all_chains() {
        set_test_master_key();
        let tx_bytes = b"test transaction";
        // Ed25519 signatures are 64 bytes
        let sig = sign_withdrawal(ChainId::Hedera, tx_bytes).unwrap();
        assert_eq!(
            sig.len(),
            64,
            "chain Hedera produced wrong signature length"
        );
    }

    /// expect: "Wallet sign withdrawal empty test works correctly under test conditions"
    #[test]
    fn sign_withdrawal_empty_tx_bytes() {
        set_test_master_key();
        // Ed25519 signs any byte sequence, including empty — should not panic
        let sig = sign_withdrawal(ChainId::Hedera, b"").unwrap();
        assert_eq!(
            sig.len(),
            64,
            "empty tx_bytes should still produce valid signature"
        );
    }

    /// expect: "Wallet sign hinkal message signature test works correctly under test conditions"
    #[test]
    fn sign_message_produces_signature() {
        set_test_master_key();
        let msg = b"Authorize Hinkal session\nSession ID: abc";
        let sig = sign_message(msg).unwrap();
        assert_eq!(sig.len(), 64);
    }

    /// expect: "Wallet sign capability tamper test works correctly under test conditions"
    #[test]
    fn sign_capability_tampered_produces_different_signature() {
        set_test_master_key();
        let mut cap = ApiKeyCapability {
            wallet_id: WalletId::new(),
            key_id: ApiKeyId::new(),
            public_key: Ed25519PublicKey([0u8; 32]),
            spending_limit_rj: RJoule::new(5000),
            spent_rj: RJoule::ZERO,
            scope: vec!["read-specs".to_string()],
            purpose: "signing test".to_string(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        let sig1 = sign_capability(&cap).unwrap();
        // Tamper with spending limit — signature must change
        cap.spending_limit_rj = RJoule::new(9999);
        let sig2 = sign_capability(&cap).unwrap();
        assert_ne!(
            sig1, sig2,
            "tampered capability must produce different signature"
        );
    }
}
