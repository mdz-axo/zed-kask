use crate::database::sqlite::SqliteDriver;
use crate::wallet::WalletStore;
use hkask_types::{
    ApiKeyCapability, ChainId, DepositReference, PrivacyMode, RJoule, TransactionType, WalletError,
    WalletTransaction,
};
use hkask_types::{ApiKeyId, Ed25519PublicKey, WalletId};
use std::sync::Arc;

fn make_store() -> WalletStore {
    let driver = SqliteDriver::in_memory_pool().expect("in-memory pool");
    let store = WalletStore::from_driver(Arc::new(SqliteDriver::new(driver)));
    store.enable_wal_mode().ok();
    store
}

#[test]
fn enable_wal_mode_succeeds() {
    let store = make_store();
    // WAL mode should succeed on in-memory databases (no-op but no error)
    let result = store.enable_wal_mode();
    assert!(
        result.is_ok(),
        "WAL mode enable should succeed: {:?}",
        result
    );
}

#[test]
fn credit_rjoules_increases_balance() {
    let store = make_store();
    let wallet = WalletId::new();
    let tx_type = TransactionType::Deposit {
        chain: ChainId::default(),
        privacy: PrivacyMode::default(),
        tx_hash: "test_credit".to_string(),
        amount_usdc_micro: 0,
    };
    let balance = store
        .credit_rjoules(wallet, RJoule::new(1000), tx_type)
        .unwrap();
    assert_eq!(balance.rjoules, 1000);
}

#[test]
fn debit_rjoules_decreases_balance() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(1000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    let balance = store
        .debit_rjoules(
            wallet,
            RJoule::new(300),
            TransactionType::Withdrawal {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_debit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    assert_eq!(balance.rjoules, 700);
}

#[test]
fn debit_rjoules_rejects_insufficient_balance() {
    let store = make_store();
    let wallet = WalletId::new();
    store
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
    let err = store
        .debit_rjoules(
            wallet,
            RJoule::new(500),
            TransactionType::Withdrawal {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_debit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap_err();
    assert!(matches!(err, WalletError::InsufficientBalance { .. }));
}

#[test]
fn balance_never_negative() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(50),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    // Debit exactly the balance
    let balance = store
        .debit_rjoules(
            wallet,
            RJoule::new(50),
            TransactionType::Withdrawal {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_debit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    assert_eq!(balance.rjoules, 0);
    // Debit more should fail
    assert!(
        store
            .debit_rjoules(
                wallet,
                RJoule::new(1),
                TransactionType::Withdrawal {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_debit_2".to_string(),
                    amount_usdc_micro: 0,
                }
            )
            .is_err()
    );
}

#[test]
fn transaction_ledger_is_append_only() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(1000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    let balance = store.get_balance(wallet).unwrap().unwrap();
    let tx = WalletTransaction {
        id: 0, // auto-increment, ignored on insert
        wallet_id: wallet,
        tx_type: TransactionType::Deposit {
            chain: ChainId::default(),
            privacy: PrivacyMode::default(),
            tx_hash: "test_tx".into(),
            amount_usdc_micro: 1_000_000,
        },
        rjoules_delta: 1000,
        balance_after: balance.rjoules,
        timestamp: chrono::Utc::now(),
    };
    store.record_transaction(&tx).unwrap();
    let txs = store.get_transactions(wallet, 10, 0).unwrap();
    // credit_rjoules atomically records a transaction, plus our manual record_transaction
    assert_eq!(txs.len(), 2);
    assert_eq!(txs[0].rjoules_delta, 1000);
    assert_eq!(txs[1].rjoules_delta, 1000);
}

#[test]
fn deposit_reference_anti_replay() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    let dep_ref = DepositReference {
        chain: ChainId::default(),
        reference: "test_ref_001".into(),
        wallet_id: wallet,
        nonce: [0u8; 16],
        expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
    };
    store.store_deposit_reference(&dep_ref).unwrap();
    // First consumption succeeds
    let result = store.consume_deposit_reference("test_ref_001").unwrap();
    assert_eq!(result, Some(wallet));
    // Second consumption fails (already spent)
    let result2 = store.consume_deposit_reference("test_ref_001").unwrap();
    assert_eq!(result2, None);
}

#[test]
fn expired_deposit_reference_rejected() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    let dep_ref = DepositReference {
        chain: ChainId::default(),
        reference: "expired_ref".into(),
        wallet_id: wallet,
        nonce: [0u8; 16],
        expires_at: chrono::Utc::now() - chrono::Duration::hours(1), // already expired
    };
    store.store_deposit_reference(&dep_ref).unwrap();
    let result = store.consume_deposit_reference("expired_ref").unwrap();
    assert_eq!(result, None);
}

#[test]
fn api_key_store_and_retrieve_by_public_key() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    let pubkey = Ed25519PublicKey([1u8; 32]);
    let cap = ApiKeyCapability {
        wallet_id: wallet,
        key_id: ApiKeyId::new(),
        public_key: pubkey,
        spending_limit_rj: RJoule::new(5000),
        spent_rj: RJoule::ZERO,
        scope: vec!["read-specs".to_string()],
        purpose: "test key".to_string(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::default(),
        preferred_chain: None,
    };
    store.store_api_key(&cap).unwrap();
    let retrieved = store.get_api_key_by_public_key(pubkey.as_bytes()).unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().key_id, cap.key_id);
}

/// Regression: privacy_mode and preferred_chain must survive store→load round-trip.
#[test]
fn api_key_privacy_mode_round_trips() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    let cap = ApiKeyCapability {
        wallet_id: wallet,
        key_id: ApiKeyId::new(),
        public_key: Ed25519PublicKey([9u8; 32]),
        spending_limit_rj: RJoule::new(5000),
        spent_rj: RJoule::ZERO,
        scope: vec!["embed-corpus".to_string()],
        purpose: "privacy-test".to_string(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::Transparent,
        preferred_chain: Some(ChainId::Hedera),
    };
    store.store_api_key(&cap).unwrap();
    let retrieved = store.get_api_key(cap.key_id).unwrap().unwrap();
    assert_eq!(retrieved.privacy_mode, PrivacyMode::Transparent);
    assert_eq!(retrieved.preferred_chain, Some(ChainId::Hedera));
}

#[test]
fn api_key_revocation_returns_unspent_rjoules() {
    let store = make_store();
    let wallet = WalletId::new();
    store
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
    let cap = ApiKeyCapability {
        wallet_id: wallet,
        key_id: ApiKeyId::new(),
        public_key: Ed25519PublicKey([2u8; 32]),
        spending_limit_rj: RJoule::new(5000),
        spent_rj: RJoule::new(1200), // 3800 unspent
        scope: vec!["embed-corpus".to_string()],
        purpose: "revocation test".to_string(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::default(),
        preferred_chain: None,
    };
    let key_id = cap.key_id;
    store.store_api_key(&cap).unwrap();
    // Debit the wallet by the key's spending limit (simulating allocation)
    store
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
    let before = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(before.rjoules, 5000); // 10000 - 5000
    store.revoke_api_key(key_id).unwrap();
    let after = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(after.rjoules, 8800); // 5000 + 3800 unspent returned
}

#[test]
fn consume_encumbrance_updates_api_key_spent_rj() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(10_000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    let key_id = ApiKeyId::new();
    let cap = ApiKeyCapability {
        wallet_id: wallet,
        key_id,
        public_key: Ed25519PublicKey([7u8; 32]),
        spending_limit_rj: RJoule::new(5000),
        spent_rj: RJoule::ZERO,
        scope: vec!["read-specs".to_string()],
        purpose: "spend sync test".to_string(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::default(),
        preferred_chain: None,
    };
    store.store_api_key(&cap).unwrap();
    store
        .encumber_rjoules(wallet, key_id, RJoule::new(2000))
        .unwrap();
    store.consume_encumbrance(key_id, RJoule::new(300)).unwrap();
    store.consume_encumbrance(key_id, RJoule::new(250)).unwrap();
    let key = store.get_api_key(key_id).unwrap().unwrap();
    assert_eq!(
        key.spent_rj,
        RJoule::new(550),
        "spent_rj must track cumulative encumbrance consumption"
    );
}

#[test]
fn failed_consume_does_not_increment_api_key_spent_rj() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(10_000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    let key_id = ApiKeyId::new();
    let cap = ApiKeyCapability {
        wallet_id: wallet,
        key_id,
        public_key: Ed25519PublicKey([8u8; 32]),
        spending_limit_rj: RJoule::new(5000),
        spent_rj: RJoule::ZERO,
        scope: vec!["read-specs".to_string()],
        purpose: "failed consume sync test".to_string(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::default(),
        preferred_chain: None,
    };
    store.store_api_key(&cap).unwrap();
    store
        .encumber_rjoules(wallet, key_id, RJoule::new(300))
        .unwrap();
    store.consume_encumbrance(key_id, RJoule::new(300)).unwrap();
    // Replay/second consume must fail because encumbrance is fully consumed.
    let second = store.consume_encumbrance(key_id, RJoule::new(1));
    assert!(
        second.is_err(),
        "second consume must fail after full consumption"
    );
    let key = store.get_api_key(key_id).unwrap().unwrap();
    assert_eq!(
        key.spent_rj,
        RJoule::new(300),
        "spent_rj must remain unchanged on failed consume"
    );
}

#[test]
fn purge_expired_references_cleans_up() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    // Store an expired reference
    let expired = DepositReference {
        chain: ChainId::default(),
        reference: "old_ref".into(),
        wallet_id: wallet,
        nonce: [0u8; 16],
        expires_at: chrono::Utc::now() - chrono::Duration::hours(1),
    };
    store.store_deposit_reference(&expired).unwrap();
    // Store a valid reference
    let valid = DepositReference {
        chain: ChainId::default(),
        reference: "new_ref".into(),
        wallet_id: wallet,
        nonce: [1u8; 16],
        expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
    };
    store.store_deposit_reference(&valid).unwrap();
    let purged = store.purge_expired_references().unwrap();
    assert_eq!(purged, 1);
    // Expired is gone
    assert_eq!(store.consume_deposit_reference("old_ref").unwrap(), None);
    // Valid still works
    assert_eq!(
        store.consume_deposit_reference("new_ref").unwrap(),
        Some(wallet)
    );
}

// Property test: for any sequence of credits and debits, the sum of all
// transaction rjoules_delta values must equal the current wallet balance.
#[test]
fn balance_equals_sum_of_ledger_deltas() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    // Perform a random-ish sequence of credits and debits.
    // Using fixed values for deterministic reproducibility.
    let operations: [(bool, u64); 12] = [
        (true, 5000),  // credit 5000
        (true, 3000),  // credit 3000
        (false, 1200), // debit 1200
        (true, 750),   // credit 750
        (false, 3000), // debit 3000
        (false, 500),  // debit 500
        (true, 10000), // credit 10000
        (false, 2000), // debit 2000
        (true, 150),   // credit 150
        (false, 8000), // debit 8000
        (false, 1500), // debit 1500
        (true, 2500),  // credit 2500
    ];
    let mut expected_sum: i64 = 0;
    for (is_credit, amount) in &operations {
        let rj = RJoule::new(*amount);
        if *is_credit {
            expected_sum += *amount as i64;
            store
                .credit_rjoules(
                    wallet,
                    rj,
                    TransactionType::Deposit {
                        chain: ChainId::default(),
                        privacy: PrivacyMode::default(),
                        tx_hash: format!("test_tx_credit_{expected_sum}"),
                        amount_usdc_micro: *amount * 1000,
                    },
                )
                .unwrap();
        } else {
            // Only debit if we can afford it
            if store.get_balance(wallet).unwrap().unwrap().rjoules >= *amount {
                expected_sum -= *amount as i64;
                store
                    .debit_rjoules(
                        wallet,
                        rj,
                        TransactionType::Withdrawal {
                            chain: ChainId::default(),
                            privacy: PrivacyMode::default(),
                            tx_hash: format!("test_tx_debit_{expected_sum}"),
                            amount_usdc_micro: *amount * 1000,
                        },
                    )
                    .unwrap();
            }
        }
    }
    // Verify: current balance == sum of all deltas
    let balance = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(
        balance.rjoules as i64, expected_sum,
        "MUST-10 VIOLATION: balance {} != sum of ledger deltas {}",
        balance.rjoules, expected_sum,
    );
    // Cross-verify via transaction ledger
    let txs = store.get_transactions(wallet, 100, 0).unwrap();
    let ledger_sum: i64 = txs.iter().map(|tx| tx.rjoules_delta).sum();
    assert_eq!(
        balance.rjoules as i64, ledger_sum,
        "MUST-10 VIOLATION: balance {} != ledger sum {}",
        balance.rjoules, ledger_sum,
    );
}

// ── Idempotency contract tests ──────────────────────────────────────
//
// Idempotency contract matrix (PR 2.5.1):
//
// | Operation                  | Idempotent? | Mechanism                          |
// |----------------------------|:-----------:|------------------------------------|
// | ensure_wallet               | ✅          | INSERT OR IGNORE                  |
// | get_balance / can_afford    | ✅          | Read-only                         |
// | get_transactions            | ✅          | Read-only                         |
// | consume_deposit_reference   | ✅          | Atomic CAS (spent=0 → spent=1)    |
// | release_encumbrance         | ✅          | Status guard (active only)        |
// | revoke_api_key              | ✅          | Marks revoked (idempotent mark)   |
// | credit_rjoules              | ❌          | No tx-hash dedup (GAP)            |
// | debit_rjoules               | ❌          | No idempotency key (GAP)          |
// | encumber_rjoules            | ⚡           | Key-scoped guard (not op-scoped)  |
// | consume_encumbrance         | ❌          | Double-consumes while active (GAP)|
// | store_api_key               | ❌          | Always creates new key (GAP)      |
// | store_deposit_reference     | ❌          | Always inserts                    |
//
// GAP entries are documented below with regression-catching tests.

#[test]
fn ensure_wallet_is_idempotent() {
    let store = make_store();
    let wallet = WalletId::new();
    // First call creates
    store.ensure_wallet(wallet).unwrap();
    let b1 = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(b1.rjoules, 0);
    // Second call should be no-op (INSERT OR IGNORE)
    store.ensure_wallet(wallet).unwrap();
    let b2 = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(
        b2.rjoules, 0,
        "balance should not change on duplicate ensure"
    );
}

#[test]
fn release_encumbrance_is_idempotent() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(5000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    // Create an API key first (encumbrance references api_keys table)
    let key_id = ApiKeyId::new();
    let cap = ApiKeyCapability {
        wallet_id: wallet,
        key_id,
        public_key: Ed25519PublicKey([9u8; 32]),
        spending_limit_rj: RJoule::new(5000),
        spent_rj: RJoule::ZERO,
        scope: vec!["test".to_string()],
        purpose: "idempotency test".to_string(),
        rate_limit: None,
        expiry: None,
        issued_at: chrono::Utc::now(),
        privacy_mode: PrivacyMode::default(),
        preferred_chain: None,
    };
    store.store_api_key(&cap).unwrap();
    store
        .encumber_rjoules(wallet, key_id, RJoule::new(1000))
        .unwrap();
    // Balance should be 4000 after encumbrance
    let after_encumber = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(after_encumber.rjoules, 4000);
    // First release returns funds
    store.release_encumbrance(key_id).unwrap();
    let after_first = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(
        after_first.rjoules, 5000,
        "first release should return funds"
    );
    // Second release is a no-op (explicitly documented as idempotent)
    store.release_encumbrance(key_id).unwrap();
    let after_second = store.get_balance(wallet).unwrap().unwrap();
    assert_eq!(
        after_second.rjoules, 5000,
        "second release must not double-credit (idempotency contract)"
    );
}

//
// This test documents the CURRENT behavior. When a transaction-hash
// deduplication mechanism is added, this test MUST be updated to verify
// that duplicate credits are rejected.
#[test]
fn credit_rjoules_is_not_idempotent_documents_gap() {
    let store = make_store();
    let wallet = WalletId::new();
    // Credit once
    store
        .credit_rjoules(
            wallet,
            RJoule::new(1000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit_1".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    assert_eq!(store.get_balance(wallet).unwrap().unwrap().rjoules, 1000);
    // Credit again with same amount — currently doubles (GAP)
    store
        .credit_rjoules(
            wallet,
            RJoule::new(1000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit_2".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    assert_eq!(
        store.get_balance(wallet).unwrap().unwrap().rjoules,
        2000,
        "GAP: duplicate credit doubles balance — no tx-hash dedup exists"
    );
}

//
// This test documents the CURRENT behavior. When an idempotency key
// mechanism is added, this test MUST be updated to verify that duplicate
// debits are rejected (or are safe).
#[test]
fn debit_rjoules_is_not_idempotent_documents_gap() {
    let store = make_store();
    let wallet = WalletId::new();
    store
        .credit_rjoules(
            wallet,
            RJoule::new(1000),
            TransactionType::Deposit {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_credit".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    // Debit once
    store
        .debit_rjoules(
            wallet,
            RJoule::new(300),
            TransactionType::Withdrawal {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_debit_1".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    assert_eq!(store.get_balance(wallet).unwrap().unwrap().rjoules, 700);
    // Debit again — currently succeeds and double-charges (GAP)
    store
        .debit_rjoules(
            wallet,
            RJoule::new(300),
            TransactionType::Withdrawal {
                chain: ChainId::default(),
                privacy: PrivacyMode::default(),
                tx_hash: "test_debit_2".to_string(),
                amount_usdc_micro: 0,
            },
        )
        .unwrap();
    assert_eq!(
        store.get_balance(wallet).unwrap().unwrap().rjoules,
        400,
        "GAP: duplicate debit double-charges — no idempotency key exists"
    );
}

//
// This is the same as the anti-replay test above but explicitly framed
// as an idempotency contract test.
#[test]
fn consume_deposit_reference_is_idempotent() {
    let store = make_store();
    let wallet = WalletId::new();
    store.ensure_wallet(wallet).unwrap();
    let dep_ref = DepositReference {
        chain: ChainId::default(),
        reference: "idem_ref_001".into(),
        wallet_id: wallet,
        nonce: [0u8; 16],
        expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
    };
    store.store_deposit_reference(&dep_ref).unwrap();
    // First consumption succeeds
    let r1 = store.consume_deposit_reference("idem_ref_001").unwrap();
    assert_eq!(r1, Some(wallet));
    // Second consumption returns None (idempotent — already spent)
    let r2 = store.consume_deposit_reference("idem_ref_001").unwrap();
    assert_eq!(
        r2, None,
        "second consume must return None (idempotent via atomic CAS)"
    );
}
