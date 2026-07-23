// Integration tests for the gas → rJoule → budget → replenish feedback loop.
//
// These tests exercise the public seams of `hkask-regulation` together with
// `hkask-wallet` and `hkask-storage` to verify end-to-end behavior:
//
// 1. WalletBackedBudget reserve/settle through GasBudgetManager.
// 2. WalletEnergyEstimator calibration propagates into WalletBackedBudget costs.
// 3. GasBudget replenishment after partial settlement refunds.

use hkask_regulation::WalletEnergyEstimator;
use hkask_regulation::energy::{GasBudget, GasCost};
use hkask_regulation::energy_budget_management::GasBudgetManager;
use hkask_regulation::wallet_budget::WalletBackedBudget;
use hkask_storage::WalletStore;
use hkask_types::crypto::Ed25519PublicKey;
use hkask_types::id::{ApiKeyId, WalletId};
use hkask_wallet::WalletManager;
use hkask_wallet::price_feed::StaticPriceFeed;
use hkask_wallet::{
    ApiKeyCapability, ChainId, GAS_PER_RJOULE, PrivacyMode, RJoule, TransactionType, WalletConfig,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Deterministic master key used only in tests.
const TEST_MASTER_KEY: &str = "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX";

/// Set the master key that `WalletManager` resolves. All tests share the same
/// deterministic key; this is safe because each test uses its own in-memory DB.
fn ensure_test_master_key() {
    // SAFETY: test-only env mutation; value is deterministic and shared.
    unsafe {
        std::env::set_var("HKASK_MASTER_KEY", TEST_MASTER_KEY);
    }
}

fn make_wallet_manager_with_store(
    config: WalletConfig,
    store: Arc<WalletStore>,
) -> Arc<WalletManager> {
    ensure_test_master_key();
    Arc::new(
        WalletManager::build(config, store, HashMap::new(), Arc::new(StaticPriceFeed))
            .expect("build wallet manager"),
    )
}

fn make_wallet_budget_with_key(
    gas_per_rjoule: u64,
    credit_rj: u64,
    encumber_rj: u64,
    limit_rj: u64,
) -> (WalletId, ApiKeyId, Arc<WalletManager>, WalletBackedBudget) {
    let config = WalletConfig {
        gas_per_rjoule,
        ..Default::default()
    };

    let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
    let store = Arc::new(WalletStore::from_driver(driver));

    let wallet_id = WalletId::new();
    let key_id = ApiKeyId::new();

    store
        .credit_rjoules(
            wallet_id,
            RJoule::new(credit_rj),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .expect("credit wallet");

    let capability = ApiKeyCapability {
        wallet_id,
        key_id,
        public_key: Ed25519PublicKey([11u8; 32]),
        spending_limit_rj: RJoule::new(limit_rj),
        spent_rj: RJoule::new(0),
        scope: vec![],
        purpose: "integration test".into(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::default(),
        preferred_chain: None,
    };
    store.store_api_key(&capability).expect("store key");
    store
        .encumber_rjoules(wallet_id, key_id, RJoule::new(encumber_rj))
        .expect("encumber rjoules");

    let manager = make_wallet_manager_with_store(config, Arc::clone(&store));

    let budget = WalletBackedBudget::new(
        wallet_id,
        Arc::clone(&manager) as Arc<dyn hkask_types::WalletBudgetPort>,
    )
    .with_api_key(key_id, RJoule::new(limit_rj));

    (wallet_id, key_id, manager, budget)
}

// to WalletBackedBudget and debits the key encumbrance.
#[tokio::test]
async fn manager_wallet_budget_reserve_settle_debits_encumbrance() {
    let (_wallet_id, key_id, manager, budget) =
        make_wallet_budget_with_key(GAS_PER_RJOULE, 10_000, 2_000, 5_000);

    let agent = hkask_types::WebID::new();
    let mgr = GasBudgetManager::new();
    mgr.register_wallet_budget(agent, budget).await;

    // gas_per_rjoule = GAS_PER_RJOULE → GAS_PER_RJOULE gas = 1 rJ. Encumbrance has 2000 rJ.
    let reserved = mgr
        .reserve_gas(&agent, GasCost(GAS_PER_RJOULE))
        .await
        .unwrap();
    assert_eq!(reserved.0, GAS_PER_RJOULE);

    let settled = mgr
        .settle_gas(&agent, GasCost(GAS_PER_RJOULE), GasCost(GAS_PER_RJOULE))
        .await
        .unwrap();
    assert_eq!(settled.0, GAS_PER_RJOULE);

    let enc = manager.get_encumbrance(key_id).unwrap().unwrap();
    assert_eq!(enc.remaining_rj(), 1_999, "1 rJ consumed from encumbrance");
}

// back into WalletBackedBudget via the configured gas_per_rjoule rate.
#[tokio::test]
async fn calibrated_gas_per_rjoule_changes_budget_cost() {
    // Calibrate an estimator: initial GAS_PER_RJOULE, observed ratio 2.0 → rate doubled.
    let mut estimator = WalletEnergyEstimator::new(GAS_PER_RJOULE);
    let adjusted = estimator.calibrate(2.0);
    assert!(adjusted, "ratio 2.0 should adjust gas_per_rjoule");
    assert_eq!(estimator.gas_per_rjoule, GAS_PER_RJOULE * 2);

    // Build a wallet budget using the calibrated rate.
    let (_wallet_id, key_id, manager, budget) =
        make_wallet_budget_with_key(estimator.gas_per_rjoule, 10_000, 2_000, 5_000);

    let agent = hkask_types::WebID::new();
    let mgr = GasBudgetManager::new();
    mgr.register_wallet_budget(agent, budget).await;

    // At doubled rate, GAS_PER_RJOULE*2 gas = 1 rJ.
    mgr.reserve_gas(&agent, GasCost(GAS_PER_RJOULE * 2))
        .await
        .unwrap();
    mgr.settle_gas(
        &agent,
        GasCost(GAS_PER_RJOULE * 2),
        GasCost(GAS_PER_RJOULE * 2),
    )
    .await
    .unwrap();

    let enc = manager.get_encumbrance(key_id).unwrap().unwrap();
    assert_eq!(
        enc.remaining_rj(),
        1_999,
        "doubled rate should consume 1 rJ for GAS_PER_RJOULE*2 gas"
    );
}

// reservation and replenishes by the configured rate.
#[tokio::test]
async fn energy_budget_refunds_and_replenishes_after_settlement() {
    let agent = hkask_types::WebID::new();
    let mgr = GasBudgetManager::new();

    // Cap 100, default replenish_rate = cap / 10 = 10.
    let budget = GasBudget::new(GasCost(100));
    mgr.register_gas_budget(agent, budget).await;

    // Reserve 100, settle only 50 → implicit refund of 50.
    mgr.reserve_gas(&agent, GasCost(100)).await.unwrap();
    mgr.settle_gas(&agent, GasCost(100), GasCost(50))
        .await
        .unwrap();

    let status_after_settle = mgr.agent_gas_status(&agent).await.unwrap();
    assert_eq!(
        status_after_settle.remaining,
        GasCost(50),
        "remaining should be 50 after settling half the reservation"
    );

    // Replenish once: 50 + 10 = 60.
    mgr.replenish_all_budgets().await;
    let status_after_replenish = mgr.agent_gas_status(&agent).await.unwrap();
    assert_eq!(
        status_after_replenish.remaining,
        GasCost(60),
        "remaining should increase by replenish_rate"
    );

    // Replenish up to cap: should stop at 100.
    for _ in 0..10 {
        mgr.replenish_all_budgets().await;
    }
    let status_at_cap = mgr.agent_gas_status(&agent).await.unwrap();
    assert_eq!(
        status_at_cap.remaining,
        GasCost(100),
        "remaining should never exceed cap"
    );
}
