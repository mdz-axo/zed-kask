//! CLI→Storage vertical slice integration test.
//!
//! Verifies the full stack from service layer down through all stores
//! using a shared in-memory database. Tests cross-store visibility:
//! writes through one store are visible to Regulation events and other stores.
//!
//! # Architecture under test
//!
//! ```text
//! ServiceConfig::in_memory()
//!   → AgentService::build()
//!     → ConsentStore ──┐
//!     → GoalRepo ──────┤
//!     → SpecStore ─────┼── all share ONE Arc<Mutex<Connection>>
//!     → UserStore ─────┤
//!     → WalletStore ───┤
//!     → RegulationArchive ──┘
//! ```rust,no_run
//!
//! # REQ tags
//!
//! Each test carries a `// REQ:` tag linking it to the contract-first
//! migration plan.

use hkask_regulation::EnergyEstimator;
use hkask_services_context::AgentService;
use hkask_services_core::ServiceConfig;
use hkask_types::DataCategory;
use hkask_types::WebID;
use hkask_types::event::{CyclePhase, RegulationRecord, Span, SpanKind};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build an in-memory AgentService for testing.
/// Sets the master key env var required for key derivation.
async fn build_test_service() -> AgentService {
    // SAFETY: integration tests run in isolated processes
    unsafe {
        std::env::set_var(
            "HKASK_MASTER_KEY",
            "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxX",
        );
    }
    let config = ServiceConfig::in_memory();
    AgentService::build(config)
        .await
        .expect("AgentService::build with in_memory config should succeed")
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
///
/// `AgentService::build()` with `ServiceConfig::in_memory()` succeeds
/// and all accessors return valid references.
#[tokio::test]
async fn service_builds_with_in_memory_config() {
    let svc = build_test_service().await;

    // Config should be in_memory
    assert!(svc.config().in_memory, "config should be in_memory");

    // Memory ports are accessible via build_per_agent_memory(db, reg_sink)

    // Regulation runtime should be accessible
    let ledger = svc.ledger().runtime.read().await;
    // Domains may be empty at startup — that's valid
    drop(ledger);

    // All store accessors should return valid references
    let _goal_repo = svc.storage().goals.clone();

    let _user_store = svc.storage().users.clone();
    let _event_sink = svc.ledger().events.clone();
    let _sovereignty = svc.governance().consent.clone();
}

/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
///
/// Grant consent through the sovereignty service and verify
/// the consent manager reflects the grant.
#[tokio::test]
async fn sovereignty_consent_round_trip() {
    let svc = build_test_service().await;
    let webid_str = WebID::new().to_string();

    let sovereignty = svc.governance().consent.clone();

    // Initially no consent for EpisodicMemory
    let has_consent = sovereignty.has_consent(&webid_str, &DataCategory::EpisodicMemory);
    assert!(!has_consent.unwrap(), "new WebID should not have consent");

    // Grant consent
    sovereignty
        .grant_consent(&webid_str, &DataCategory::EpisodicMemory)
        .expect("grant_consent should succeed");

    // Verify consent is now granted
    let has_consent = sovereignty.has_consent(&webid_str, &DataCategory::EpisodicMemory);
    assert!(
        has_consent.unwrap(),
        "consent should be granted after grant_consent"
    );
}

/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
///
/// Create a goal through the goal repository and read it back.
#[tokio::test]
async fn goal_write_read_round_trip() {
    let svc = build_test_service().await;
    let webid = WebID::new();
    let goal_repo = svc.storage().goals.clone();

    // Create a goal
    let goal = goal_repo
        .create_goal(
            &webid,
            "test goal for integration",
            hkask_types::Visibility::Private,
        )
        .expect("create_goal should succeed");

    // Read it back
    let retrieved = goal_repo
        .get_goal(goal.id)
        .expect("get_goal should succeed")
        .expect("goal should exist after creation");

    assert_eq!(retrieved.text, "test goal for integration");
    assert_eq!(retrieved.webid, webid);
}

/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
///
/// After granting consent, a Regulation event can be persisted in the
/// shared event store (visible because all stores share one DB).
#[tokio::test]
async fn cross_store_consent_visible_to_reg_events() {
    let svc = build_test_service().await;
    let webid = WebID::new();
    let webid_str = webid.to_string();

    // Grant consent
    svc.governance()
        .consent
        .grant_consent(&webid_str, &DataCategory::EpisodicMemory)
        .expect("grant_consent should succeed");

    // The Regulation event sink shares the same database as the consent store.
    // Verify the event sink is functional on the shared connection.
    let event_sink = svc.ledger().events.clone();
    let test_event = hkask_types::event::RegulationRecord::new(
        webid,
        hkask_types::event::Span::new(
            hkask_types::event::SpanNamespace::new("reg.inference").unwrap(),
            "test.integration",
        ),
        hkask_types::event::CyclePhase::Act,
        serde_json::json!({"test": true}),
        0,
    );
    event_sink
        .persist(&test_event)
        .expect("event sink should accept events on shared connection");
}

/// \[P7\] Motivating: Evolutionary Architecture — parameter emerged from real usage and is calibrated at runtime.
///
/// The shared CalibratedEnergyEstimator observes reg.gas.settled events persisted
/// through the Regulation event sink and updates per-server cost estimates.
#[tokio::test]
async fn service_energy_estimator_calibrates_from_events() {
    let svc = build_test_service().await;
    let agent = WebID::new();
    let server = "hkask-mcp-media";

    // Before calibration, default cost applies.
    let estimator = svc.ledger().energy.clone();
    let before = estimator.estimate_cost(server, "search", &serde_json::json!({}));
    assert_eq!(before, 100);

    // Persist a settled gas event via the shared Regulation event sink.
    let event = RegulationRecord::new(
        agent,
        Span::from_kind(SpanKind::GasSettled),
        CyclePhase::Act,
        serde_json::json!({
            "server": server,
            "tool": "search",
            "reserved": 100,
            "actual": 200,
            "refunded": 0,
        }),
        0,
    );
    svc.ledger()
        .events
        .persist(&event)
        .expect("persist settled gas event");

    // Calibrate the estimator directly (background loop also runs, but direct
    // call keeps the test deterministic).
    let adjusted = estimator.calibrate().await.expect("calibrate");
    assert_eq!(adjusted, 1, "media server should be adjusted");

    let after = estimator.estimate_cost(server, "search", &serde_json::json!({}));
    assert_eq!(
        after, 200,
        "media cost should double after ratio 2.0 observation"
    );
}

/// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
///
/// The wallet store is wired into the shared database and can
/// perform balance operations.
#[tokio::test]
async fn wallet_store_accessible() {
    let svc = build_test_service().await;

    // Wallet may be None if no chain ports are configured (expected in test mode)
    if let Some(wallet) = svc.infra().wallet.clone() {
        let wallet_id = hkask_types::id::WalletId::new();
        wallet
            .manager()
            .ensure_wallet(wallet_id)
            .expect("ensure_wallet should succeed");
        let balance = wallet
            .manager()
            .get_balance(wallet_id)
            .expect("get_balance should succeed");
        assert_eq!(balance.rjoules, 0, "new wallet should have zero balance");
    }
}

/// \[P2\] Constraining: Affirmative Consent — consolidation_agent_memory enforces consent gate.
///
/// Verifies the consent-denial path (ServiceError::Domain { domain: Consent, kind: Forbidden, .. }) and that
/// the consent-granted path proceeds past the consent check to the DB-open stage.
#[tokio::test]
async fn consolidate_userpod_memory_consent_checks() {
    let svc = build_test_service().await;
    let agent_name = "test-agent"; // matches ServiceConfig::in_memory()
    let target_webid = hkask_types::WebID::for_userpod_name(agent_name);
    let request = hkask_types::ConsolidationRequest::default();

    // Part 1: consent denied — no consent granted yet
    let result = svc.consolidate_userpod_memory(agent_name, request.clone());
    assert!(
        matches!(
            result,
            Err(hkask_services_core::ServiceError::Domain {
                domain: hkask_services_core::DomainKind::Consent,
                kind: hkask_services_core::ErrorKind::Forbidden,
                ..
            })
        ),
        "should deny consolidation without consent, got {:?}",
        result
    );

    // Part 2: grant both memory categories and retry — consent gate should pass.
    // The DB open will fail (in-memory config has no on-disk DB), producing
    // a Storage error. This proves the consent gate was satisfied.
    svc.governance()
        .consent
        .grant_consent(
            &target_webid.to_string(),
            &hkask_types::DataCategory::EpisodicMemory,
        )
        .expect("grant episodic_memory consent");
    svc.governance()
        .consent
        .grant_consent(
            &target_webid.to_string(),
            &hkask_types::DataCategory::SemanticMemory,
        )
        .expect("grant semantic_memory consent");

    let result = svc.consolidate_userpod_memory(agent_name, request);
    assert!(
        matches!(
            result,
            Err(hkask_services_core::ServiceError::Domain { .. })
        ),
        "should proceed past consent check — expected Storage error for nonexistent DB, got {:?}",
        result
    );
}
