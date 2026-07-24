// ── Tests ──────────────────────────────────────────────────────────────────────

use super::WalletService;
use hkask_services_core::{DomainKind, ServiceError};
use hkask_storage::WalletStore;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, RegulationSink, Span, SpanNamespace};
use hkask_types::id::WalletId;
use hkask_types::regulation::{RegulationSpan, ToolSubsystem};
use hkask_wallet::GAS_PER_RJOULE;
use hkask_wallet::price_feed::StaticPriceFeed;
use hkask_wallet::{ApiKeyIssuer, PriceFeed, WalletManager};
use hkask_wallet::{
    ChainId, ChainPort, DepositEvent, ExchangeRate, PrivacyMode, RJoule, TxHash, WalletConfig,
    WalletError,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

mod test_support {
    use super::*;

    const TEST_MASTER_KEY: &str =
        "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX";

    #[derive(Default)]
    pub(super) struct CaptureSink {
        pub(super) events: Mutex<Vec<RegulationRecord>>,
    }

    impl RegulationSink for CaptureSink {
        fn persist(
            &self,
            event: &RegulationRecord,
        ) -> Result<(), hkask_types::InfrastructureError> {
            self.events.lock().expect("lock").push(event.clone());
            Ok(())
        }
    }

    pub(super) fn set_test_master_key() {
        // SAFETY: test-only env var set in isolated test process.
        unsafe {
            std::env::set_var("HKASK_MASTER_KEY", TEST_MASTER_KEY);
        }
    }

    pub(super) fn build_service_with_harness(
        sink: Arc<CaptureSink>,
        chains: HashMap<ChainId, Arc<dyn ChainPort>>,
        price_feed: Arc<dyn PriceFeed>,
    ) -> WalletService {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));

        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                chains,
                price_feed,
            )
            .expect("build manager")
            .with_event_sink(Arc::clone(&sink) as Arc<dyn RegulationSink>),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).expect("issuer"));
        WalletService::new(manager, issuer)
    }

    pub(super) fn assert_event_actor(sink: &CaptureSink, operation: &str, actor: &WebID) {
        let events = sink.events.lock().expect("lock");
        let event = events
            .iter()
            .find(|e| e.observation.get("operation") == Some(&serde_json::json!(operation)))
            .unwrap_or_else(|| panic!("event for operation '{operation}' must be emitted"));
        assert_eq!(event.observer_webid.to_string(), actor.to_string());
    }

    pub(super) struct FailingActorChain {
        pub(super) sink: Arc<dyn RegulationSink>,
    }

    pub(super) struct FailingPriceFeed;

    #[async_trait::async_trait]
    impl ChainPort for FailingActorChain {
        fn chain_id(&self) -> ChainId {
            ChainId::Hedera
        }

        fn derive_deposit_address(&self, _index: u64) -> Result<String, WalletError> {
            Ok("mock_addr".into())
        }

        async fn monitor_deposits(
            &self,
            _actor: &WebID,
            _addresses: &[String],
        ) -> Result<Vec<DepositEvent>, WalletError> {
            Ok(vec![])
        }

        fn build_withdrawal_tx(
            &self,
            _to_address: &str,
            _amount_usdc_micro: u64,
        ) -> Result<Vec<u8>, WalletError> {
            Ok(b"mock-withdraw-payload".to_vec())
        }

        async fn submit_signed_tx(
            &self,
            actor: &WebID,
            _signed_tx_bytes: &[u8],
        ) -> Result<TxHash, WalletError> {
            let event = RegulationRecord::new(
                *actor,
                Span::new(
                    SpanNamespace::try_from(RegulationSpan::Tool {
                        subsystem: ToolSubsystem::Wallet,
                    })
                    .unwrap(),
                    "error",
                ),
                CyclePhase::Sense,
                serde_json::json!({
                    "chain": "hedera",
                    "operation": "submit_signed_tx",
                    "error": "forced adapter failure"
                }),
                0,
            );
            let _ = self.sink.persist(&event);
            Err(WalletError::ChainError {
                chain: ChainId::Hedera,
                message: "forced adapter failure".into(),
            })
        }

        async fn confirmations(
            &self,
            _actor: &WebID,
            _tx_hash: &TxHash,
        ) -> Result<u64, WalletError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl PriceFeed for FailingPriceFeed {
        async fn get_rate(&self, _chain: ChainId) -> Result<ExchangeRate, WalletError> {
            Err(WalletError::Infra(
                hkask_types::InfrastructureError::database("forced price feed failure"),
            ))
        }
    }
}

use test_support::*;

fn make_service() -> WalletService {
    set_test_master_key();
    build_service_with_harness(
        Arc::new(CaptureSink::default()),
        Default::default(),
        Arc::new(StaticPriceFeed),
    )
}

#[test]
fn get_balance_returns_zero_for_new_wallet() {
    let svc = make_service();
    let wallet = WalletId::new();
    let balance = svc.manager().get_balance(wallet).unwrap();
    assert_eq!(balance.rjoules, 0);
}

#[test]
fn gas_to_rjoules_conversion() {
    let svc = make_service();
    assert_eq!(svc.manager().gas_to_rjoules(0).as_u64(), 0);
    assert_eq!(svc.manager().gas_to_rjoules(GAS_PER_RJOULE / 2).as_u64(), 1);
    assert_eq!(svc.manager().gas_to_rjoules(GAS_PER_RJOULE * 2).as_u64(), 2);
}

#[test]
fn rjoules_to_gas_conversion() {
    let svc = make_service();
    assert_eq!(svc.manager().rjoules_to_gas(RJoule::new(0)), 0);
    assert_eq!(
        svc.manager().rjoules_to_gas(RJoule::new(5)),
        5 * GAS_PER_RJOULE
    );
}

#[tokio::test]
async fn estimate_withdrawal_fee_returns_positive_fee() {
    let svc = make_service();
    let actor = WebID::from_persona(b"wallet-service-test");
    let fee = svc
        .manager()
        .estimate_withdrawal_fee(&actor, ChainId::Hedera)
        .await
        .expect("fee estimate");
    assert!(fee.rjoules > 0);
    assert!(fee.usdc_micro > 0);
    assert!(fee.native_units > 0.0);
}

#[tokio::test]
async fn withdraw_propagates_actor_into_adapter_chain_error_span() {
    set_test_master_key();
    let sink = Arc::new(CaptureSink::default());

    let mut chains: HashMap<ChainId, Arc<dyn ChainPort>> = HashMap::new();
    chains.insert(
        ChainId::Hedera,
        Arc::new(FailingActorChain {
            sink: Arc::clone(&sink) as Arc<dyn RegulationSink>,
        }),
    );

    let svc = build_service_with_harness(Arc::clone(&sink), chains, Arc::new(StaticPriceFeed));

    let wallet = WalletId::new();
    svc.manager().ensure_wallet(wallet).expect("ensure wallet");

    let actor = WebID::from_persona(b"svc-wallet-actor");
    let err = svc
        .withdraw(
            &actor,
            wallet,
            RJoule::ZERO,
            "some_destination",
            ChainId::Hedera,
            PrivacyMode::Transparent,
        )
        .await
        .expect_err("forced adapter failure should bubble up");
    assert!(matches!(
        err,
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            ..
        }
    ));

    assert_event_actor(&sink, "submit_signed_tx", &actor);
}

#[tokio::test]
async fn estimate_fee_error_span_preserves_request_actor() {
    set_test_master_key();
    let sink = Arc::new(CaptureSink::default());
    let svc = build_service_with_harness(
        Arc::clone(&sink),
        Default::default(),
        Arc::new(FailingPriceFeed),
    );

    let actor = WebID::from_persona(b"svc-fee-actor");
    let err: ServiceError = svc
        .manager()
        .estimate_withdrawal_fee(&actor, ChainId::Hedera)
        .await
        .expect_err("forced price feed failure should bubble up")
        .into();
    assert!(matches!(
        err,
        ServiceError::Domain {
            domain: DomainKind::Wallet,
            ..
        }
    ));

    assert_event_actor(&sink, "estimate_withdrawal_fee", &actor);
}

#[test]
fn create_key_produces_valid_material() {
    let svc = make_service();
    let wallet = WalletId::new();
    svc.manager().ensure_wallet(wallet).expect("ensure_wallet");

    let material = svc
        .issuer()
        .create_key(
            wallet,
            RJoule::new(5000),
            None,
            PrivacyMode::Transparent,
            None,
            vec!["read-specs".to_string()],
            "test key".to_string(),
            None,
        )
        .unwrap();
    assert_eq!(material.private_key_hex.len(), 64);
    assert!(material.capability.spending_limit_rj.as_u64() == 5000);
}

#[test]
fn list_keys_returns_created_keys() {
    let svc = make_service();
    let wallet = WalletId::new();
    svc.manager().ensure_wallet(wallet).expect("ensure_wallet");

    svc.issuer()
        .create_key(
            wallet,
            RJoule::new(1000),
            None,
            PrivacyMode::Transparent,
            None,
            vec!["read-specs".to_string()],
            "list test 1".to_string(),
            None,
        )
        .unwrap();
    svc.issuer()
        .create_key(
            wallet,
            RJoule::new(2000),
            None,
            PrivacyMode::Transparent,
            Some(ChainId::Hedera),
            vec!["embed-corpus".to_string()],
            "list test 2".to_string(),
            None,
        )
        .unwrap();

    let keys = svc.issuer().list_keys(wallet).unwrap();
    assert_eq!(keys.len(), 2);
}

#[test]
fn revoke_key_removes_from_active_list() {
    let svc = make_service();
    let wallet = WalletId::new();
    svc.manager().ensure_wallet(wallet).expect("ensure_wallet");

    let material = svc
        .issuer()
        .create_key(
            wallet,
            RJoule::new(1000),
            None,
            PrivacyMode::Transparent,
            None,
            vec!["read-specs".to_string()],
            "revoke test".to_string(),
            None,
        )
        .unwrap();

    assert_eq!(svc.issuer().list_keys(wallet).unwrap().len(), 1);
    svc.issuer().revoke_key(material.key_id).unwrap();
    assert_eq!(svc.issuer().list_keys(wallet).unwrap().len(), 0);
}
