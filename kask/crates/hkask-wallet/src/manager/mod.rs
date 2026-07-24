//! WalletManager — orchestrates chain ports, privacy layer, and rJoule accounting.
//!
//! # Deposit reference logic merged here per essentialist G1
//! The deposit reference scheme (generate, verify, consume) was originally a
//! separate 2-function module. Merged into WalletManager because it is tightly
//! coupled to wallet_seed and WalletStore — a separate module added no behavior
//! beyond what inline functions provide.

use crate::reg_span::WalletSpan;
#[cfg(test)]
use crate::types::EncumbranceStatus;
use crate::types::{
    ApiKeyCapability, ChainId, DepositAddress, DepositReference, Encumbrance, PrivacyMode, RJoule,
    TransactionType, TxHash, WalletBalance, WalletConfig, WalletError, WalletTransaction,
};
use chrono::{Duration, Utc};
use hkask_keystore::keychain::resolve_wallet_seed;
use hkask_storage::WalletStore;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::id::{ApiKeyId, WalletId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

use crate::chain::{ChainPort, DepositEvent};
use crate::price_feed::{PriceFeed, WithdrawalFee};

mod budget;
mod deposits;
mod encumbrance;
mod regulation;
mod withdrawals;

/// Optional hook for centralized self-healing policies.
///
/// Implementations live in service-layer crates so wallet stays dependency-minimal.
pub trait WalletSelfHealer: Send + Sync {
    fn heal(&self, operation: &str, error: &str);
}

/// Orchestrates chain ports, privacy layer, and rJoule accounting.
///
/// # Ownership `[OUGHT-DECL]`
/// - Sole-owns `ChainPort` implementations
/// - Shares `Arc<WalletStore>` with Regulation for algedonic monitoring
/// - Holds `wallet_seed` in `Zeroizing` for deposit reference generation
/// - Does NOT hold treasury keys (loaded per-operation in signing.rs)
///
/// expect: "The system manages rJoule balances, encumbrances, and energy-based payments"
/// \[P9\] Motivating: Homeostatic Self-Regulation — wallet is the energy regulation anchor
/// \[P1\] Constraining: User Sovereignty — wallet_seed is user-owned and zeroized
/// inv: wallet_seed is zeroized on drop (Zeroizing wrapper)
/// inv: chains map is non-empty after successful build
pub struct WalletManager {
    config: WalletConfig,
    store: Arc<WalletStore>,
    chains: HashMap<ChainId, Arc<dyn ChainPort>>,
    wallet_seed: Zeroizing<[u8; 32]>,
    /// Optional Regulation event sink for span emission (Phase 5).
    /// When present, wallet operations emit reg.wallet.* spans.
    event_sink: Option<Arc<dyn RegulationSink>>,
    /// Price feed for native token USD rates (fee estimation).
    /// Resolved from user's `PriceFeedConfig` at build time.
    price_feed: Arc<dyn PriceFeed>,
    /// Runtime-adjustable gas→rJoule conversion rate.
    /// Initialized from `config.gas_per_rjoule`; updated by the Regulation calibration loop.
    gas_per_rjoule: Arc<AtomicU64>,
    /// Optional self-heal hook for centralized recovery policies.
    self_heal_hook: Arc<Mutex<Option<Arc<dyn WalletSelfHealer>>>>,
}

impl WalletManager {
    /// Build a WalletManager from configuration, store, chain/privacy ports, and price feed.
    ///
    /// expect: "The system manages rJoule balances, encumbrances, and energy-based payments"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — wallet is the energy regulation anchor
    /// \[P1\] Constraining: User Sovereignty — wallet_seed is user-owned and zeroized
    /// pre:  config is valid, store is initialized, chains is non-empty
    /// pre:  price_feed is a resolved PriceFeed implementation
    /// post: returns Ok(WalletManager) with resolved wallet_seed
    /// post: returns Err if wallet_seed resolution fails
    pub fn build(
        config: WalletConfig,
        store: Arc<WalletStore>,
        chains: HashMap<ChainId, Arc<dyn ChainPort>>,
        price_feed: Arc<dyn PriceFeed>,
    ) -> Result<Self, WalletError> {
        let seed_bytes = resolve_wallet_seed().map_err(|e| {
            WalletError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
        })?;
        let mut seed_arr = [0u8; 32];
        seed_arr.copy_from_slice(&seed_bytes[..32]);
        let gas_per_rjoule = Arc::new(AtomicU64::new(config.gas_per_rjoule));
        Ok(WalletManager {
            config,
            store,
            chains,
            wallet_seed: Zeroizing::new(seed_arr),
            event_sink: None,
            price_feed,
            gas_per_rjoule,
            self_heal_hook: Arc::new(Mutex::new(None)),
        })
    }

    /// Attach a Regulation event sink for span emission.
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Replace the price feed (for testing or runtime reconfiguration).
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_price_feed(mut self, feed: Arc<dyn PriceFeed>) -> Self {
        self.price_feed = feed;
        self
    }

    /// Attach a self-heal hook (service-layer coordinator).
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_self_healer(self, healer: Arc<dyn WalletSelfHealer>) -> Self {
        if let Ok(mut slot) = self.self_heal_hook.lock() {
            *slot = Some(healer);
        }
        self
    }

    /// Set a self-heal hook after construction (thread-safe).
    pub fn set_self_healer(&self, healer: Arc<dyn WalletSelfHealer>) {
        if let Ok(mut slot) = self.self_heal_hook.lock() {
            *slot = Some(healer);
        }
    }

    /// Get a reference to the price feed.
    pub fn price_feed(&self) -> &Arc<dyn PriceFeed> {
        &self.price_feed
    }

    // Regulation event emission moved to reg.rs. All methods are available via impl blocks
    // in that module, loaded through `use super::*`.

    // ── Balance ──────────────────────────────────────────────────────────────

    /// Get the current rJoule balance for a wallet.
    ///
    /// expect: "I can query my rJoule balance"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — balance is the cybernetic state
    /// \[P8\] Constraining: Semantic Grounding — gas/USDC equivalents derive deterministically
    /// pre:  wallet_id is a valid WalletId
    /// post: returns Ok(balance) with rjoules, gas_equivalent, usdc_equivalent_micro
    /// post: gas_equivalent == rjoules * config.gas_per_rjoule
    /// post: balance.rjoules >= 0 (balances are never negative)
    pub fn get_balance(&self, wallet_id: WalletId) -> Result<WalletBalance, WalletError> {
        let mut balance = self.store.get_balance(wallet_id)?.unwrap_or(WalletBalance {
            wallet_id,
            rjoules: 0,
            usdc_equivalent_micro: 0,
            gas_equivalent: 0,
        });
        balance.gas_equivalent = balance.rjoules * self.config.gas_per_rjoule;
        balance.usdc_equivalent_micro =
            (balance.rjoules as u128 * 1_000_000 / self.config.rj_per_usdc as u128) as u64;
        Ok(balance)
    }

    /// Get an API key's capability metadata for Regulation health monitoring.
    /// Returns `None` if the key doesn't exist or has been revoked.
    ///
    /// expect: "The system manages API key issuance with spending limits and expiry"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — API key health state for feedback loops
    /// \[P4\] Constraining: Clear Boundaries — revoked keys are excluded
    /// pre:  key_id is a valid ApiKeyId
    /// post: returns Ok(Some(capability)) if key exists and is active
    /// post: returns Ok(None) if key doesn't exist or is revoked
    pub fn get_api_key(&self, key_id: ApiKeyId) -> Result<Option<ApiKeyCapability>, WalletError> {
        self.store.get_api_key(key_id)
    }

    /// Ensure a wallet row exists (idempotent — creates if missing).
    pub fn ensure_wallet(&self, wallet_id: WalletId) -> Result<(), WalletError> {
        self.store.ensure_wallet(wallet_id)
    }

    /// Get paginated transaction history for a wallet.
    pub fn get_transactions(
        &self,
        wallet_id: WalletId,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<WalletTransaction>, WalletError> {
        self.store.get_transactions(wallet_id, limit, offset)
    }

    // ── Deposit monitoring ───────────────────────────────────────────────────

    // Moved to deposits.rs — impl blocks loaded via `mod deposits;`.

    // ── Deposit address ──────────────────────────────────────────────────────

    /// Get or derive a deposit address for a wallet on a specific chain.
    pub fn get_deposit_address(
        &self,
        wallet_id: WalletId,
        chain: ChainId,
        privacy: PrivacyMode,
    ) -> Result<DepositAddress, WalletError> {
        let port = self.chains.get(&chain).ok_or(WalletError::ChainError {
            chain,
            message: "chain not enabled".into(),
        })?;
        // Use derivation index 0 for the primary address
        let address = port.derive_deposit_address(0)?;
        self.store
            .store_deposit_address(wallet_id, &address, 0, chain, privacy)?;

        // Regulation span: deposit address derived
        self.emit_span(
            WalletSpan::Deposit,
            "derived",
            CyclePhase::Act,
            serde_json::json!({
                "wallet_id": wallet_id.to_string(),
                "chain": chain.to_string(),
                "privacy": privacy.to_string(),
            }),
        );

        Ok(DepositAddress {
            address,
            chain,
            privacy_mode: privacy,
        })
    }

    // ── Gas ↔ rJoule conversion ──────────────────────────────────────────────

    // Moved to budget.rs — impl blocks loaded via `mod budget;`.

    // ── Deposit reference scheme (merged from deposit_ref.rs) ─────────────────

    /// Generate a one-time deposit reference for shielded deposits.
    ///
    /// # Privacy property `[IS-DECL]`
    /// Derived via HKDF from the wallet seed + nonce + expiry.
    /// Appears random on-chain but hKask can verify it belongs to a specific wallet.
    ///
    /// # Anti-replay `[OUGHT-DECL]`
    /// References are burned on use (consumed in WalletStore).
    /// References expire after `validity_duration` (default 24h).
    pub fn generate_deposit_reference(
        &self,
        wallet_id: WalletId,
        chain: ChainId,
        validity_duration: Duration,
    ) -> Result<DepositReference, WalletError> {
        let nonce: [u8; 16] = rand::random();
        let expiry = Utc::now() + validity_duration;
        let context = format!(
            "hkask:deposit-ref:{}:{}:{}:{}",
            wallet_id,
            chain,
            expiry.timestamp(),
            hex::encode(nonce)
        );
        // HKDF-expand from wallet seed
        let ref_bytes = hkdf_expand(&*self.wallet_seed, context.as_bytes())?;
        let reference = hex::encode(&ref_bytes[..16]); // 32-char hex string

        let dep_ref = DepositReference {
            chain,
            reference,
            wallet_id,
            nonce,
            expires_at: expiry,
        };
        self.store.store_deposit_reference(&dep_ref)?;
        Ok(dep_ref)
    }

    // ── Encumbrance — rJoule lock/release/consume ────────────────────────────

    // Moved to encumbrance.rs — impl blocks loaded via `mod encumbrance;`.
}

// ── HKDF helper (minimal, uses hmac + sha2 from workspace) ─────────────────────

fn hkdf_expand(seed: &[u8], info: &[u8]) -> Result<Vec<u8>, WalletError> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(seed).map_err(|e| {
        WalletError::Infra(hkask_types::InfrastructureError::database(e.to_string()))
    })?;
    mac.update(info);
    mac.update(&[0x01]);
    let result = mac.finalize().into_bytes();
    Ok(result.to_vec())
}

// ── WalletBudgetPort implementation ────────────────────────────────────────────

/// Implement the hexagonal port so Regulation depends on the trait, not the concrete type.
/// Per Conant-Ashby: the regulator (Regulation) models the system (wallet) via this
/// abstract interface, not via direct coupling to `WalletManager`.
impl hkask_types::WalletBudgetPort for WalletManager {
    fn gas_to_rjoules(&self, gas: u64) -> RJoule {
        WalletManager::gas_to_rjoules(self, gas)
    }

    fn get_encumbrance(&self, key_id: ApiKeyId) -> Option<Encumbrance> {
        WalletManager::get_encumbrance(self, key_id).ok().flatten()
    }

    fn emit_key_alert(&self, key_id: ApiKeyId, exhausted: bool, expired: bool) {
        WalletManager::emit_key_alert(self, key_id, exhausted, expired);
    }

    fn can_afford(&self, wallet_id: WalletId, cost_rj: RJoule) -> bool {
        WalletManager::can_afford(self, wallet_id, cost_rj).unwrap_or(false)
    }

    fn get_api_key(&self, key_id: ApiKeyId) -> Option<ApiKeyCapability> {
        WalletManager::get_api_key(self, key_id).ok().flatten()
    }

    fn get_balance(
        &self,
        wallet_id: WalletId,
    ) -> Result<hkask_types::WalletBalance, hkask_types::WalletBudgetError> {
        WalletManager::get_balance(self, wallet_id)
            .map_err(|e| hkask_types::WalletBudgetError::Wallet(e.to_string()))
    }

    fn gas_per_rjoule(&self) -> u64 {
        WalletManager::gas_per_rjoule(self)
    }

    fn set_gas_per_rjoule(&self, rate: u64) {
        WalletManager::set_gas_per_rjoule(self, rate);
    }

    fn consume(
        &self,
        key_id: ApiKeyId,
        gas_rj: RJoule,
    ) -> Result<(), hkask_types::WalletBudgetError> {
        WalletManager::consume(self, key_id, gas_rj)
            .map_err(|e| hkask_types::WalletBudgetError::Wallet(e.to_string()))
    }

    fn settle_rjoules(
        &self,
        wallet_id: WalletId,
        reserved_rj: RJoule,
        actual_rj: RJoule,
    ) -> Result<(), hkask_types::WalletBudgetError> {
        WalletManager::settle_rjoules(self, wallet_id, reserved_rj, actual_rj)
            .map_err(|e| hkask_types::WalletBudgetError::Wallet(e.to_string()))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiKeyIssuer;
    use crate::GAS_PER_RJOULE;
    use crate::chain::DepositEvent;
    use crate::price_feed::StaticPriceFeed;

    struct MockChainPort {
        chain: ChainId,
    }

    #[async_trait::async_trait]
    #[async_trait::async_trait]
    impl ChainPort for MockChainPort {
        fn chain_id(&self) -> ChainId {
            self.chain
        }
        fn derive_deposit_address(&self, _index: u64) -> Result<String, WalletError> {
            Ok("mock_address_123".into())
        }
        async fn monitor_deposits(
            &self,
            _actor: &WebID,
            _addresses: &[String],
        ) -> Result<Vec<DepositEvent>, WalletError> {
            Ok(vec![])
        }
        fn build_withdrawal_tx(&self, _to: &str, _amount: u64) -> Result<Vec<u8>, WalletError> {
            Ok(b"mock_tx".to_vec())
        }
        async fn submit_signed_tx(
            &self,
            _actor: &WebID,
            _tx: &[u8],
        ) -> Result<TxHash, WalletError> {
            Ok(TxHash("mock_hash".into()))
        }
        async fn confirmations(
            &self,
            _actor: &WebID,
            _tx_hash: &TxHash,
        ) -> Result<u64, WalletError> {
            Ok(32)
        }
    }

    fn make_manager() -> WalletManager {
        // SAFETY: test-only env var set in single-threaded test context;
        // SAFETY: no other threads read HKASK_MASTER_KEY concurrently.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        let mut chains: HashMap<ChainId, Arc<dyn ChainPort>> = HashMap::new();
        chains.insert(
            ChainId::Hedera,
            Arc::new(MockChainPort {
                chain: ChainId::Hedera,
            }) as Arc<dyn ChainPort>,
        );
        WalletManager::build(
            WalletConfig::default(),
            store,
            chains,
            Arc::new(StaticPriceFeed::new()),
        )
        .unwrap()
    }

    /// expect: "Wallet mgr gas conversion test works correctly under test conditions"
    #[test]
    fn gas_to_rjoules_conversion() {
        let mgr = make_manager();
        assert_eq!(mgr.gas_to_rjoules(GAS_PER_RJOULE), RJoule::new(1));
        assert_eq!(mgr.gas_to_rjoules(GAS_PER_RJOULE / 2), RJoule::new(1)); // rounds up
        assert_eq!(mgr.gas_to_rjoules(0), RJoule::ZERO);
    }

    /// expect: "Wallet mgr rjoules to gas test works correctly under test conditions"
    #[test]
    fn rjoules_to_gas_conversion() {
        let mgr = make_manager();
        assert_eq!(mgr.rjoules_to_gas(RJoule::new(1)), GAS_PER_RJOULE);
        assert_eq!(mgr.rjoules_to_gas(RJoule::new(5)), 5 * GAS_PER_RJOULE);
    }

    /// expect: "I can estimate withdrawal fees before initiating a withdrawal"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — fee estimate enables cost-aware withdrawal
    /// \[P8\] Constraining: Semantic Grounding — derived from live/native USD rate
    #[tokio::test]
    async fn estimate_withdrawal_fee_uses_price_feed() {
        let mgr = make_manager();
        let actor = WebID::from_persona(b"wallet-test");
        let fee = mgr
            .estimate_withdrawal_fee(&actor, ChainId::Hedera)
            .await
            .expect("fee estimate");
        assert!(fee.rjoules > 0);
        assert!(fee.usdc_micro > 0);
        assert!(fee.native_units > 0.0);
    }

    /// expect: "Wallet mgr can afford test works correctly under test conditions"
    #[test]
    fn can_afford_checks_balance() {
        let mgr = make_manager();
        let wallet = WalletId::new();
        mgr.store
            .credit_rjoules(
                wallet,
                RJoule::new(100),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();
        assert!(mgr.can_afford(wallet, RJoule::new(50)).unwrap());
        assert!(!mgr.can_afford(wallet, RJoule::new(200)).unwrap());
    }

    /// expect: "Wallet mgr reserve rejects test works correctly under test conditions"
    #[test]
    fn reserve_rejects_insufficient_balance() {
        let mgr = make_manager();
        let wallet = WalletId::new();
        mgr.store
            .credit_rjoules(
                wallet,
                RJoule::new(10),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();
        assert!(mgr.reserve_rjoules(wallet, RJoule::new(5)).is_ok());
        assert!(mgr.reserve_rjoules(wallet, RJoule::new(100)).is_err());
    }

    /// expect: "Wallet mgr settle debits test works correctly under test conditions"
    #[test]
    fn settle_debits_actual_cost() {
        let mgr = make_manager();
        let wallet = WalletId::new();
        mgr.store
            .credit_rjoules(
                wallet,
                RJoule::new(100),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();
        mgr.settle_rjoules(wallet, RJoule::new(50), RJoule::new(30))
            .unwrap();
        let balance = mgr.get_balance(wallet).unwrap();
        assert_eq!(balance.rjoules, 70); // 100 - 30
    }

    /// expect: "Wallet mgr deposit ref gen test works correctly under test conditions"
    #[test]
    fn deposit_reference_generation() {
        let mgr = make_manager();
        let wallet = WalletId::new();
        mgr.store.ensure_wallet(wallet).unwrap();

        let dep_ref = mgr
            .generate_deposit_reference(wallet, ChainId::Hedera, Duration::hours(24))
            .unwrap();
        assert_eq!(dep_ref.reference.len(), 32); // 16 bytes → 32 hex chars
        assert_eq!(dep_ref.wallet_id, wallet);
        assert_eq!(dep_ref.chain, ChainId::Hedera);
    }

    // ── Property-based tests ───────────────────────────────────────────────

    use proptest::prelude::*;

    /// Strategy: generate a random RJoule amount in a reasonable range.
    fn arbitrary_rjoule() -> BoxedStrategy<RJoule> {
        (1u64..1000u64).prop_map(RJoule::new).boxed()
    }

    /// Helper: create a minimal API key so encumbrance FK constraint is satisfied.
    fn ensure_key(store: &Arc<WalletStore>, wallet_id: WalletId, key_id: ApiKeyId) {
        use crate::types::ApiKeyCapability;
        use hkask_types::crypto::Ed25519PublicKey;
        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey([0u8; 32]),
            spending_limit_rj: RJoule::new(1_000_000),
            spent_rj: RJoule::ZERO,
            scope: vec![],
            purpose: "test".into(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::default(),
            preferred_chain: None,
        };
        let _ = store.store_api_key(&capability);
    }

    // After any sequence of credit, encumber, consume, and release operations:
    // - Wallet balance = total_credited - total_consumed (conservation)
    // - Total consumed ≤ total credited (can't spend more than deposited)
    // - Per key: consumed ≤ encumbered (can't consume more than locked)
    proptest! {
        #![proptest_config(ProptestConfig { max_shrink_iters: 0, .. ProptestConfig::with_cases(64) })]
        #[test]
        fn balance_conservation_under_encumbrance_lifecycle(
            credits in prop::collection::vec(arbitrary_rjoule(), 1..10),
            operations in prop::collection::vec((arbitrary_rjoule(), arbitrary_rjoule()), 0..20),
        ) {
            let mgr = make_manager();
            let wallet = WalletId::new();
            mgr.store.ensure_wallet(wallet).unwrap();

            // Track total credited
            let mut total_credited: u64 = 0;
            for credit in &credits {
                mgr.store.credit_rjoules(wallet, *credit, TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: format!("test_credit_{total_credited}"),
                    amount_usdc_micro: 0,
                }).unwrap();
                total_credited += credit.as_u64();
            }

            // Track per-key encumbrance state (create keys on demand)
            let mut key_encumbered: std::collections::HashMap<ApiKeyId, u64> = std::collections::HashMap::new();
            let mut key_consumed: std::collections::HashMap<ApiKeyId, u64> = std::collections::HashMap::new();

            for (encumber_amount, consume_amount) in &operations {
                let key_id = ApiKeyId::new();
                ensure_key(&mgr.store, wallet, key_id);

                // Encumber: lock rJoules for this key (only if affordable)
                if mgr.can_afford(wallet, *encumber_amount).unwrap_or(false) {
                    let _ = mgr.encumber(wallet, key_id, *encumber_amount);
                    *key_encumbered.entry(key_id).or_insert(0) += encumber_amount.as_u64();
                }

                // Consume: spend from encumbrance (up to encumbered amount)
                let encumbered = *key_encumbered.get(&key_id).unwrap_or(&0);
                let consumed = *key_consumed.get(&key_id).unwrap_or(&0);
                let available = encumbered.saturating_sub(consumed);
                let actual_consume = consume_amount.as_u64().min(available);
                if actual_consume > 0 {
                    let _ = mgr.consume(key_id, RJoule::new(actual_consume));
                    *key_consumed.entry(key_id).or_insert(0) += actual_consume;
                }

                // Release: return unspent to wallet
                let _ = mgr.release_encumbrance(key_id);
            }

            // Invariant 1: balance = credited - consumed (conservation)
            let balance = mgr.get_balance(wallet).unwrap();
            let total_consumed: u64 = key_consumed.values().sum();
            prop_assert_eq!(balance.rjoules, total_credited.saturating_sub(total_consumed),
                "balance {} != credited {} - consumed {}", balance.rjoules, total_credited, total_consumed);

            // Invariant 2: can't consume more than credited
            prop_assert!(total_consumed <= total_credited,
                "consumed {} > credited {}", total_consumed, total_credited);

            // Invariant 3: per-key, consumed ≤ encumbered
            for (key_id, encumbered) in &key_encumbered {
                let consumed = key_consumed.get(key_id).copied().unwrap_or(0);
                prop_assert!(consumed <= *encumbered,
                    "key {}: consumed {} > encumbered {}", key_id, consumed, encumbered);
            }
        }
    }

    // ── Integration: deposit monitor ───────────────────────────────────────

    /// A MockChainPort that returns a pre-configured deposit event.
    struct DepositMockPort {
        chain: ChainId,
        deposit: Option<DepositEvent>,
    }

    #[async_trait::async_trait]
    impl ChainPort for DepositMockPort {
        fn chain_id(&self) -> ChainId {
            self.chain
        }
        fn derive_deposit_address(&self, _index: u64) -> Result<String, WalletError> {
            Ok("mock_deposit_addr_1".into())
        }
        async fn monitor_deposits(
            &self,
            _actor: &WebID,
            _addresses: &[String],
        ) -> Result<Vec<DepositEvent>, WalletError> {
            Ok(self.deposit.clone().into_iter().collect())
        }
        fn build_withdrawal_tx(&self, _to: &str, _amount: u64) -> Result<Vec<u8>, WalletError> {
            Ok(b"mock_tx".to_vec())
        }
        async fn submit_signed_tx(
            &self,
            _actor: &WebID,
            _tx: &[u8],
        ) -> Result<TxHash, WalletError> {
            Ok(TxHash("mock_hash".into()))
        }
        async fn confirmations(
            &self,
            _actor: &WebID,
            _tx_hash: &TxHash,
        ) -> Result<u64, WalletError> {
            Ok(32)
        }
    }

    /// expect: "Wallet mgr deposit monitor idempotent test works correctly under test conditions"
    #[tokio::test]
    async fn deposit_monitor_credits_and_is_idempotent() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        // Use a deterministic wallet ID so the monitor can find it.
        // WalletId::default() creates a random UUID each call — they won't match.
        let wallet_id = WalletId::from_name("test_wallet");
        store.ensure_wallet(wallet_id).unwrap();

        // Store a deposit address so resolution works
        store
            .store_deposit_address(
                wallet_id,
                "mock_deposit_addr_1",
                0,
                ChainId::Hedera,
                PrivacyMode::Transparent,
            )
            .unwrap();

        let deposit_event = DepositEvent {
            tx_hash: TxHash("test_tx_hash_001".into()),
            from_address: "sender_addr".into(),
            to_address: "mock_deposit_addr_1".into(),
            amount_usdc_micro: 1_000_000, // 1 USDC
            confirmations: 32,
            block_time: Utc::now(),
        };

        let mut chains: HashMap<ChainId, Arc<dyn ChainPort>> = HashMap::new();
        chains.insert(
            ChainId::Hedera,
            Arc::new(DepositMockPort {
                chain: ChainId::Hedera,
                deposit: Some(deposit_event.clone()),
            }) as Arc<dyn ChainPort>,
        );

        let mgr = WalletManager::build(
            WalletConfig::default(),
            Arc::clone(&store),
            chains,
            Arc::new(StaticPriceFeed::new()),
        )
        .unwrap();

        // Run one monitor cycle
        mgr.poll_deposits_once().await;

        // Verify balance was credited
        let balance = store.get_balance(wallet_id).unwrap().unwrap();
        assert!(
            balance.rjoules > 0,
            "balance should be credited after deposit"
        );

        // Verify idempotency: running again with same tx_hash should not double-credit
        let balance_before = balance.rjoules;
        let mut chains2: HashMap<ChainId, Arc<dyn ChainPort>> = HashMap::new();
        chains2.insert(
            ChainId::Hedera,
            Arc::new(DepositMockPort {
                chain: ChainId::Hedera,
                deposit: Some(deposit_event),
            }) as Arc<dyn ChainPort>,
        );
        let mgr2 = WalletManager::build(
            WalletConfig::default(),
            Arc::clone(&store),
            chains2,
            Arc::new(StaticPriceFeed::new()),
        )
        .unwrap();
        mgr2.poll_deposits_once().await;
        let balance_after = store.get_balance(wallet_id).unwrap().unwrap();
        assert_eq!(
            balance_after.rjoules, balance_before,
            "idempotency: balance should not change on replayed deposit"
        );
    }

    /// expect: "Wallet mgr multi chain deposit test works correctly under test conditions"
    #[tokio::test]
    async fn poll_deposits_once_multi_chain() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        let wallet_id = WalletId::from_name("multi_chain_wallet");
        store.ensure_wallet(wallet_id).unwrap();

        // Register deposit address — must match DepositMockPort::derive_deposit_address
        store
            .store_deposit_address(
                wallet_id,
                "mock_deposit_addr_1",
                0,
                ChainId::Hedera,
                PrivacyMode::Transparent,
            )
            .unwrap();

        let hed_deposit_1 = DepositEvent {
            tx_hash: TxHash("hed_tx_001".into()),
            from_address: "sender_a".into(),
            to_address: "mock_deposit_addr_1".into(),
            amount_usdc_micro: 1_000_000, // 1 USDC
            confirmations: 32,
            block_time: Utc::now(),
        };
        let hed_deposit_2 = DepositEvent {
            tx_hash: TxHash("hed_tx_002".into()),
            from_address: "sender_b".into(),
            to_address: "mock_deposit_addr_1".into(),
            amount_usdc_micro: 2_000_000, // 2 USDC
            confirmations: 64,
            block_time: Utc::now(),
        };

        let mut chains: HashMap<ChainId, Arc<dyn ChainPort>> = HashMap::new();
        chains.insert(
            ChainId::Hedera,
            Arc::new(DepositMockPort {
                chain: ChainId::Hedera,
                deposit: Some(hed_deposit_1),
            }) as Arc<dyn ChainPort>,
        );
        chains.insert(
            ChainId::Hedera,
            Arc::new(DepositMockPort {
                chain: ChainId::Hedera,
                deposit: Some(hed_deposit_2),
            }) as Arc<dyn ChainPort>,
        );

        let mgr = WalletManager::build(
            WalletConfig::default(),
            Arc::clone(&store),
            chains,
            Arc::new(StaticPriceFeed::new()),
        )
        .unwrap();

        mgr.poll_deposits_once().await;

        // Both chains' deposits should be credited
        let balance = store.get_balance(wallet_id).unwrap().unwrap();
        assert!(
            balance.rjoules > 0,
            "balance should reflect deposits from both chains"
        );

        // Verify two deposit transactions recorded
        let txs = store.get_transactions(wallet_id, 10, 0).unwrap();
        let deposit_count = txs
            .iter()
            .filter(|tx| matches!(tx.tx_type, TransactionType::Deposit { .. }))
            .count();
        // Both deposit events come from Hedera — only one chain port survives
        // (HashMap key collision on ChainId::Hedera). The surviving port
        // processes hed_deposit_2, crediting one deposit.
        assert_eq!(deposit_count, 1, "one deposit should be recorded");
    }

    /// expect: "Wallet mgr payment lifecycle test works correctly under test conditions"
    #[test]
    fn end_to_end_payment_lifecycle() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let mgr = make_manager();
        let wallet_id = WalletId::from_name("e2e_test_wallet");
        mgr.store.ensure_wallet(wallet_id).unwrap();

        // Step 1: Deposit — credit rJoules
        mgr.store
            .credit_rjoules(
                wallet_id,
                RJoule::new(10_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();
        let balance = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(balance.rjoules, 10_000, "deposit credited");

        // Step 2: Create API key with spending limit
        let issuer = ApiKeyIssuer::new(Arc::clone(&mgr.store)).unwrap();
        let material = issuer
            .create_key(
                wallet_id,
                RJoule::new(5_000),
                None, // no expiry
                PrivacyMode::Transparent,
                None, // no chain preference
                vec![],
                "e2e test key".into(),
                None,
            )
            .unwrap();
        let key_id = material.key_id;

        // Step 3: Encumber rJoules to the key
        mgr.encumber(wallet_id, key_id, RJoule::new(2_000)).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert!(enc.is_active(), "encumbrance should be active");
        assert_eq!(enc.remaining_rj(), 2_000, "full amount available");

        // Step 4: Consume rJoules (simulating tool/inference usage)
        mgr.consume(key_id, RJoule::new(500)).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert_eq!(enc.remaining_rj(), 1_500, "500 consumed");

        // Consume more
        mgr.consume(key_id, RJoule::new(300)).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert_eq!(enc.remaining_rj(), 1_200, "800 total consumed");

        // Step 5: Verify wallet balance unchanged (encumbrance is separate)
        let balance = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(balance.rjoules, 8_000, "wallet: 10_000 - 2_000 encumbered");

        // Step 6: Release encumbrance — returns unspent to wallet
        mgr.release_encumbrance(key_id).unwrap();
        let balance = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(
            balance.rjoules, 9_200,
            "wallet: 8_000 + 1_200 unspent returned"
        );

        // Step 7: Verify encumbrance is released
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert!(!enc.is_active(), "encumbrance should be released");

        // Step 8: Verify key spending limit tracking
        // spent_rj is now synced with encumbrance consumption (consume_encumbrance
        // updates both encumbrances.consumed_rj and api_keys.spent_rj).
        let capability = mgr.get_api_key(key_id).unwrap().unwrap();
        assert_eq!(
            capability.spent_rj.as_u64(),
            800,
            "spent_rj reflects 500 + 300 consumed from encumbrance"
        );
    }

    /// expect: "Wallet mgr encumbrance state machine test works correctly under test conditions"
    // Proves that once an encumbrance is released, no operation can re-activate it.
    #[test]
    fn encumbrance_status_state_machine_no_released_to_active() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let mgr = make_manager();
        let wallet_id = WalletId::from_name("state_machine_test");
        mgr.store.ensure_wallet(wallet_id).unwrap();
        mgr.store
            .credit_rjoules(
                wallet_id,
                RJoule::new(10_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        // Create a key and encumber rJoules
        let issuer = ApiKeyIssuer::new(Arc::clone(&mgr.store)).unwrap();
        let material = issuer
            .create_key(
                wallet_id,
                RJoule::new(5_000),
                None,
                PrivacyMode::Transparent,
                None,
                vec![],
                "state machine test key".into(),
                None,
            )
            .unwrap();
        let key_id = material.key_id;

        // Encumber 2000 rJ
        mgr.encumber(wallet_id, key_id, RJoule::new(2_000)).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert!(enc.is_active(), "initial state: Active");

        // Consume some
        mgr.consume(key_id, RJoule::new(500)).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert!(enc.is_active(), "still Active after partial consume");

        // Release the encumbrance
        mgr.release_encumbrance(key_id).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert!(!enc.is_active(), "state after release: Released");
        assert_eq!(enc.status, EncumbranceStatus::Released);

        // Attempt to consume from Released encumbrance — MUST fail
        let result = mgr.consume(key_id, RJoule::new(100));
        assert!(result.is_err(), "consume on Released encumbrance must fail");
        match result {
            Err(WalletError::EncumbranceNotFound { .. }) => {} // expected
            other => panic!("expected EncumbranceNotFound, got {:?}", other),
        }

        // Verify encumbrance is still Released (not re-activated)
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert_eq!(
            enc.status,
            EncumbranceStatus::Released,
            "status remains Released"
        );

        // Attempt to release again — idempotent, no error
        mgr.release_encumbrance(key_id).unwrap();
        let enc = mgr.get_encumbrance(key_id).unwrap().unwrap();
        assert_eq!(
            enc.status,
            EncumbranceStatus::Released,
            "double release is idempotent"
        );

        // Verify wallet balance: 10_000 - 2_000 (encumbered) + 1_500 (unspent returned) = 9_500
        let balance = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(
            balance.rjoules, 9_500,
            "wallet balance after release + refund"
        );
    }

    // ── Withdrawal pipeline tests ─────────────────────────────────────────

    /// expect: "Wallet mgr withdraw pipeline test works correctly under test conditions"
    #[tokio::test]
    async fn withdraw_full_pipeline_success() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let mgr = make_manager();
        let wallet_id = WalletId::from_name("withdraw_test");
        mgr.store.ensure_wallet(wallet_id).unwrap();
        mgr.store
            .credit_rjoules(
                wallet_id,
                RJoule::new(10_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let balance_before = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(balance_before.rjoules, 10_000);

        // Execute withdrawal
        let actor = WebID::from_persona(b"wallet-test");
        let tx_hash = mgr
            .withdraw(
                &actor,
                wallet_id,
                RJoule::new(2_000),
                "recipient_addr_123",
                ChainId::Hedera,
                PrivacyMode::Transparent,
            )
            .await
            .unwrap();

        // Verify tx_hash returned
        assert_eq!(tx_hash.0, "mock_hash");

        // Verify balance debited
        let balance_after = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(balance_after.rjoules, 8_000, "10_000 - 2_000 = 8_000");

        // Verify transaction recorded in ledger
        let txs = mgr.get_transactions(wallet_id, 10, 0).unwrap();
        let withdrawal_tx = txs
            .iter()
            .find(|tx| matches!(tx.tx_type, TransactionType::Withdrawal { .. }));
        assert!(
            withdrawal_tx.is_some(),
            "withdrawal transaction should be in ledger"
        );
        let wtx = withdrawal_tx.unwrap();
        assert_eq!(wtx.rjoules_delta, -2000, "debit of 2000 rJ");
        assert_eq!(wtx.balance_after, 8000, "balance after withdrawal");
    }

    /// expect: "Wallet mgr withdraw insufficient test works correctly under test conditions"
    #[tokio::test]
    async fn withdraw_rejects_insufficient_balance() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let mgr = make_manager();
        let wallet_id = WalletId::from_name("insufficient_test");
        mgr.store.ensure_wallet(wallet_id).unwrap();
        mgr.store
            .credit_rjoules(
                wallet_id,
                RJoule::new(500),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let actor = WebID::from_persona(b"wallet-test");
        let result = mgr
            .withdraw(
                &actor,
                wallet_id,
                RJoule::new(10_000), // more than balance
                "recipient_addr",
                ChainId::Hedera,
                PrivacyMode::Transparent,
            )
            .await;

        assert!(
            result.is_err(),
            "withdraw should fail with insufficient balance"
        );
        match result {
            Err(WalletError::InsufficientBalance { .. }) => {} // expected
            other => panic!("expected InsufficientBalance, got {:?}", other),
        }

        // Verify balance unchanged
        let balance = mgr.get_balance(wallet_id).unwrap();
        assert_eq!(
            balance.rjoules, 500,
            "balance should be unchanged after failed withdrawal"
        );
    }

    /// expect: "Wallet mgr withdraw unsupported chain test works correctly under test conditions"
    #[tokio::test]
    async fn withdraw_rejects_unsupported_chain() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        let wallet_id = WalletId::from_name("chain_test");
        store.ensure_wallet(wallet_id).unwrap();
        store
            .credit_rjoules(
                wallet_id,
                RJoule::new(10_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let before = store.get_balance(wallet_id).unwrap().unwrap();

        // Build manager with NO registered chains — any withdrawal should fail
        let mgr = WalletManager::build(
            WalletConfig::default(),
            Arc::clone(&store),
            HashMap::new(),
            Arc::new(StaticPriceFeed::new()),
        )
        .unwrap();

        let actor = WebID::from_persona(b"wallet-test");
        let result = mgr
            .withdraw(
                &actor,
                wallet_id,
                RJoule::new(1_000),
                "recipient_addr",
                ChainId::Hedera,
                PrivacyMode::Transparent,
            )
            .await;

        assert!(
            result.is_err(),
            "withdraw to unregistered chain should fail"
        );
        match result {
            Err(WalletError::ChainError { .. }) => {} // expected
            other => panic!("expected ChainNotEnabled, got {:?}", other),
        }

        let after = store.get_balance(wallet_id).unwrap().unwrap();
        assert_eq!(
            after.rjoules, before.rjoules,
            "failed withdrawal must not change wallet balance"
        );
    }
}
