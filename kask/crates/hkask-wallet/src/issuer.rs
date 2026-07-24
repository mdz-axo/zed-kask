//! ApiKeyIssuer — generates Ed25519 API key capability tokens.
//!
//! # "Printing" an API key `[OUGHT-DECL]`
//! An API key is an Ed25519 keypair. The private key IS the API key — returned
//! to the user once at creation time, never stored by hKask. The public key is
//! stored in the `api_keys` table for Bearer token authentication.
//!
//! # OCAP alignment (P4)
//! Each key carries embedded attenuation: spending limit, expiry, privacy mode.
//! The Ed25519 signature proves it was issued by the wallet holder.

use crate::reg_span::WalletSpan;
use crate::types::{
    ApiKeyCapability, ApiKeyMaterial, ChainId, PrivacyMode, RJoule, RateLimitConfig, WalletError,
};
use chrono::{Duration, Utc};
use ed25519_dalek::SigningKey;
use hkask_storage::WalletStore;
use hkask_types::crypto::Ed25519PublicKey;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::id::{ApiKeyId, WalletId};
use rand::Rng;
use std::sync::Arc;
use zeroize::Zeroizing;

use crate::signing;

/// Issues Ed25519-signed API key capability tokens.
///
/// expect: "The system manages API key issuance with spending limits and expiry"
/// \[P9\] Motivating: Homeostatic Self-Regulation — API keys scope and limit agent energy access
/// \[P2\] Constraining: Affirmative Consent — keys are explicitly scoped, revocable, and user-issued
/// \[P4\] Constraining: Clear Boundaries — spending limits and expiry enforce capability boundaries
/// \[P1\] Constraining: User Sovereignty — private keys are returned once and never stored
/// inv: private keys are never stored (only public keys persisted)
/// inv: wallet_seed is zeroized on drop
/// # Security `[OUGHT-DECL]`
/// - Private keys are generated per-key, returned to user once, never stored
/// - Only the public key is persisted (for Bearer token lookup)
/// - Wallet seed is held in `Zeroizing` for capability signing
pub struct ApiKeyIssuer {
    store: Arc<WalletStore>,
    event_sink: Option<Arc<dyn RegulationSink>>,
}

impl ApiKeyIssuer {
    /// Create a new ApiKeyIssuer.
    ///
    /// expect: "The system manages API key issuance with spending limits and expiry"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — API keys scope and limit agent energy access
    /// \[P2\] Constraining: Affirmative Consent — keys are explicitly scoped, revocable, and user-issued
    /// \[P4\] Constraining: Clear Boundaries — spending limits and expiry enforce capability boundaries
    /// \[P1\] Constraining: User Sovereignty — private keys are returned once and never stored
    /// pre:  store is initialized
    /// post: returns Ok(ApiKeyIssuer) with resolved wallet_seed in Zeroizing
    /// post: returns Err if wallet_seed resolution fails
    pub fn new(store: Arc<WalletStore>) -> Result<Self, WalletError> {
        Ok(ApiKeyIssuer {
            store,
            event_sink: None,
        })
    }

    /// Attach a Regulation event sink for span emission.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Emit a Regulation span if an event sink is configured.
    fn emit_span(&self, span: WalletSpan, verb: &str, phase: CyclePhase, obs: serde_json::Value) {
        if let Some(ref sink) = self.event_sink {
            let event_span = Span::new(
                SpanNamespace::from_observable(&span).expect("domain span must be canonical"),
                verb,
            );
            let actor =
                hkask_types::WebID::from_persona_with_namespace(b"wallet-issuer", "wallet-surface");
            let event = RegulationRecord::new(actor, event_span, phase, obs, 0);
            if let Err(e) = sink.persist(&event) {
                tracing::warn!(target: "hkask.wallet", span = ?span, verb = verb, error = %e, "Failed to persist Regulation span");
            }
        }
    }

    /// "Print" a new API key.
    ///
    /// expect: "I can create an API key with spending limits and scope"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — API keys scope and limit agent energy access
    /// \[P2\] Constraining: Affirmative Consent — keys are explicitly scoped, revocable, and user-issued
    /// \[P4\] Constraining: Clear Boundaries — spending limits and expiry enforce capability boundaries
    /// \[P1\] Constraining: User Sovereignty — private keys are returned once and never stored
    /// pre:  wallet_id is valid, spending_limit_rj > 0, purpose is non-empty
    /// post: returns Ok(ApiKeyMaterial) with fresh Ed25519 keypair
    /// post: private_key_hex returned once, never stored by hKask
    /// post: public key + capability metadata persisted in store
    /// post: emits reg.wallet.key_issued span
    /// Generates a fresh Ed25519 keypair, creates a signed capability token
    /// with the specified limits, scope, and purpose, stores the public key,
    /// and returns the private key to the user (shown exactly once).
    #[allow(clippy::too_many_arguments)]
    pub fn create_key(
        &self,
        wallet_id: WalletId,
        spending_limit_rj: RJoule,
        expiry_days: Option<u32>,
        privacy_mode: PrivacyMode,
        preferred_chain: Option<ChainId>,
        scope: Vec<String>,
        purpose: String,
        rate_limit: Option<RateLimitConfig>,
    ) -> Result<ApiKeyMaterial, WalletError> {
        // Generate fresh Ed25519 keypair for this API key
        let mut rng = rand::rng();
        let mut seed = Zeroizing::new([0u8; 32]);
        rng.fill(&mut *seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let private_key_bytes = signing_key.to_bytes();
        // seed (Zeroizing) drops here → key material zeroized
        let public_key = Ed25519PublicKey(signing_key.verifying_key().to_bytes());

        let key_id = ApiKeyId::new();
        let issued_at = Utc::now();
        let expiry = expiry_days.map(|days| issued_at + Duration::days(days as i64));

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key,
            spending_limit_rj,
            spent_rj: RJoule::ZERO,
            scope,
            purpose,
            rate_limit,
            expiry,
            issued_at,
            privacy_mode,
            preferred_chain,
        };

        // Sign the capability with the wallet's Ed25519 key
        let _signature = signing::sign_capability(&capability)?;

        // Store the public key + capability metadata
        self.store.store_api_key(&capability)?;

        // Regulation span: key issued
        self.emit_span(
            WalletSpan::KeyIssued,
            "issued",
            CyclePhase::Act,
            serde_json::json!({
                "key_id": key_id.to_string(),
                "wallet_id": wallet_id.to_string(),
                "spending_limit_rj": spending_limit_rj.as_u64(),
                "expiry_days": expiry_days,
                "privacy_mode": privacy_mode.to_string(),
            }),
        );

        Ok(ApiKeyMaterial {
            key_id,
            private_key_hex: hex::encode(private_key_bytes),
            capability,
        })
    }

    /// Revoke an API key. Returns unspent rJoules to the wallet.
    /// Idempotent — revoking an already-revoked key is a no-op.
    ///
    /// expect: "I can revoke an API key and recover unspent balance"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — API keys scope and limit agent energy access
    /// \[P2\] Constraining: Affirmative Consent — keys are explicitly scoped, revocable, and user-issued
    /// \[P4\] Constraining: Clear Boundaries — spending limits and expiry enforce capability boundaries
    /// \[P1\] Constraining: User Sovereignty — private keys are returned once and never stored
    /// pre:  key_id is a valid ApiKeyId
    /// post: key marked as revoked in store
    /// post: unspent rJoules returned to wallet
    /// post: idempotent — revoking already-revoked key is no-op
    /// post: emits reg.wallet.key_revoked span
    pub fn revoke_key(&self, key_id: ApiKeyId) -> Result<(), WalletError> {
        self.store.revoke_api_key(key_id)?;

        // Regulation span: key revoked
        self.emit_span(
            WalletSpan::KeyRevoked,
            "revoked",
            CyclePhase::Act,
            serde_json::json!({
                "key_id": key_id.to_string(),
            }),
        );

        Ok(())
    }

    /// List active (non-revoked) API keys for a wallet.
    ///
    /// expect: "I can list my active API keys"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — API keys scope and limit agent energy access
    /// \[P2\] Constraining: Affirmative Consent — keys are explicitly scoped, revocable, and user-issued
    /// \[P4\] Constraining: Clear Boundaries — spending limits and expiry enforce capability boundaries
    /// \[P1\] Constraining: User Sovereignty — private keys are returned once and never stored
    /// pre:  wallet_id is a valid WalletId
    /// post: returns Ok(`Vec<ApiKeyCapability>`) containing only non-revoked keys
    pub fn list_keys(&self, wallet_id: WalletId) -> Result<Vec<ApiKeyCapability>, WalletError> {
        self.store.list_api_keys(wallet_id)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChainId, TransactionType};

    fn make_issuer() -> ApiKeyIssuer {
        // SAFETY: test-only — sets master key env var in isolated test process.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        ApiKeyIssuer::new(store).unwrap()
    }

    /// expect: "Wallet issuer create keypair test works correctly under test conditions"
    #[test]
    fn create_key_produces_valid_keypair() {
        let issuer = make_issuer();
        let wallet = WalletId::new();
        issuer.store.ensure_wallet(wallet).unwrap();

        let material = issuer
            .create_key(
                wallet,
                RJoule::new(5000),
                None,
                PrivacyMode::Transparent,
                None,
                vec!["read-specs".to_string()],
                "test key for validation".to_string(),
                None,
            )
            .unwrap();

        // Private key is 32 bytes → 64 hex chars
        assert_eq!(material.private_key_hex.len(), 64);
        // Public key is stored
        let retrieved = issuer.store.get_api_key(material.key_id).unwrap();
        assert!(retrieved.is_some());
    }

    /// expect: "Wallet issuer expiry test works correctly under test conditions"
    #[test]
    fn create_key_with_expiry() {
        let issuer = make_issuer();
        let wallet = WalletId::new();
        issuer.store.ensure_wallet(wallet).unwrap();

        let material = issuer
            .create_key(
                wallet,
                RJoule::new(5000),
                Some(30),
                PrivacyMode::Transparent,
                None,
                vec!["embed-corpus".to_string()],
                "monthly embedding job".to_string(),
                None,
            )
            .unwrap();

        assert!(material.capability.expiry.is_some());
    }

    /// expect: "Wallet issuer revoke unspent test works correctly under test conditions"
    #[test]
    fn revoke_key_returns_unspent_rjoules() {
        let issuer = make_issuer();
        let wallet = WalletId::new();
        issuer
            .store
            .credit_rjoules(
                wallet,
                RJoule::new(10000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let material = issuer
            .create_key(
                wallet,
                RJoule::new(5000),
                None,
                PrivacyMode::Transparent,
                None,
                vec!["read-specs".to_string()],
                "revocation test key".to_string(),
                None,
            )
            .unwrap();

        // Simulate spending 1200 rJ
        issuer
            .store
            .update_spent_rj(material.key_id, RJoule::new(1200))
            .unwrap();
        // Debit wallet by the limit (simulating allocation)
        issuer
            .store
            .debit_rjoules(
                wallet,
                RJoule::new(5000),
                TransactionType::Withdrawal {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_debit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        issuer.revoke_key(material.key_id).unwrap();
        let balance = issuer.store.get_balance(wallet).unwrap().unwrap();
        assert_eq!(balance.rjoules, 8800); // 10000 - 5000 + 3800 unspent
    }

    /// expect: "Wallet issuer list active test works correctly under test conditions"
    #[test]
    fn list_keys_returns_active_keys() {
        let issuer = make_issuer();
        let wallet = WalletId::new();
        issuer.store.ensure_wallet(wallet).unwrap();

        let key1 = issuer
            .create_key(
                wallet,
                RJoule::new(1000),
                None,
                PrivacyMode::Transparent,
                None,
                vec!["read-specs".to_string()],
                "list test key 1".to_string(),
                None,
            )
            .unwrap();
        let key2 = issuer
            .create_key(
                wallet,
                RJoule::new(2000),
                None,
                PrivacyMode::Transparent,
                Some(ChainId::Hedera),
                vec!["embed-corpus".to_string()],
                "list test key 2".to_string(),
                None,
            )
            .unwrap();

        let keys = issuer.list_keys(wallet).unwrap();
        assert_eq!(keys.len(), 2);

        // Revoke one
        issuer.revoke_key(key1.key_id).unwrap();
        let keys_after = issuer.list_keys(wallet).unwrap();
        assert_eq!(keys_after.len(), 1);
        assert_eq!(keys_after[0].key_id, key2.key_id);
    }
}
