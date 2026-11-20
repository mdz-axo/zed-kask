//! Phase 2 deterministic integration suite against a real in-process
//! `cdk::mint::Mint` backed by `cdk-fake-wallet` and encrypted SQLCipher
//! `wallet.db` storage.
//!
//! Covers all Phase 2 verification gates from `wallet-cashu-lightning-design.md` §7:
//! 1. Mint (NUT-04), receive, send (declined & approved), NUT-11 P2PK send/receive
//!    (unauthorized rejection + authorized redemption), NUT-05 melt + NUT-08 change,
//!    idempotent retry refusal (T8), and unallowlisted mint refusal (T4).
//! 2. Crash recovery: mid-send interruption persisted in encrypted SQLCipher
//!    `wallet.db` → reopen fresh engine → `wallet_reconcile` recovers 100% of value.
//! 3. Deterministic seed restore (NUT-09 / NUT-13): wipe `wallet.db` completely →
//!    reopen fresh engine with same seed → `wallet_restore_from_seed` recovers 100%
//!    of unspent balance.

use std::collections::{HashMap, HashSet};
use std::str::FromStr as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use cdk::Amount;
use cdk::mint::{Mint, MintBuilder, MintInput, MintMeltLimits};
use cdk::nuts::{
    BatchCheckMintQuoteRequest, BatchMintRequest, CheckStateRequest, CheckStateResponse,
    CurrencyUnit, Id, KeySet, KeysetResponse, MeltRequest, MintInfo, MintRequest, MintResponse,
    PaymentMethod, RestoreRequest, RestoreResponse, SecretKey, SwapRequest, SwapResponse,
};
use cdk::wallet::{
    AuthWallet, LnurlPayInvoiceResponse, LnurlPayResponse, MintConnector, SendOptions,
};
use cdk_common::common::FeeReserve;
use cdk_common::nut00::KnownMethod;
use cdk_common::quote_id::QuoteId;
use cdk_common::{
    MeltQuoteCreateResponse, MeltQuoteRequest, MeltQuoteResponse, MintQuoteRequest,
    MintQuoteResponse,
};
use cdk_fake_wallet::FakeWallet;
use cdk_sql_common::SQLMintDatabase;
use hkask_mcp_wallet::approval::{ApprovalError, ApprovalPort};
use hkask_mcp_wallet::config::WalletConfig;
use hkask_mcp_wallet::engine::{ConnectorFactory, WalletEngine, WalletError};
use hkask_mcp_wallet::storage::{SqlcipherConfig, SqlcipherConnectionManager};
use hkask_types::wallet_wire::{
    WalletNwcOperationWire, WalletNwcResultWire, WalletSpendApprovalWire, WalletSpendRequestWire,
};
use hkask_wallet_core::WalletNetwork;
use tokio::sync::Mutex;
use zeroize::Zeroizing;

const TEST_MINT_URL: &str = "http://127.0.0.1:3338";

/// In-process `MintConnector` delegating directly to a real `cdk::mint::Mint`
/// backed by `cdk-fake-wallet`. Exercises 100% of Cashu NUT-00..13/19 crypto
/// without binding any TCP port.
#[derive(Debug)]
struct DirectMintConnector {
    mint: Arc<Mint>,
}

#[async_trait]
impl MintConnector for DirectMintConnector {
    async fn fetch_lnurl_pay_request(&self, _url: &str) -> Result<LnurlPayResponse, cdk::Error> {
        Err(cdk::Error::Custom(
            "LNURL not used in direct mint test".into(),
        ))
    }

    async fn fetch_lnurl_invoice(&self, _url: &str) -> Result<LnurlPayInvoiceResponse, cdk::Error> {
        Err(cdk::Error::Custom(
            "LNURL not used in direct mint test".into(),
        ))
    }

    async fn get_mint_keys(&self) -> Result<Vec<KeySet>, cdk::Error> {
        Ok(self.mint.pubkeys().keysets)
    }

    async fn get_mint_keyset(&self, keyset_id: Id) -> Result<KeySet, cdk::Error> {
        self.mint
            .keyset_pubkeys(&keyset_id)
            .map(|r| r.keysets.into_iter().next())?
            .ok_or(cdk::Error::UnknownKeySet)
    }

    async fn get_mint_keysets(&self) -> Result<KeysetResponse, cdk::Error> {
        Ok(self.mint.keysets())
    }

    async fn post_mint_quote(
        &self,
        request: MintQuoteRequest,
    ) -> Result<MintQuoteResponse<String>, cdk::Error> {
        let resp = self.mint.get_mint_quote(request).await?;
        Ok(resp.into())
    }

    async fn post_mint(
        &self,
        _method: &PaymentMethod,
        request: MintRequest<String>,
    ) -> Result<MintResponse, cdk::Error> {
        let quote_id = QuoteId::from_str(&request.quote)
            .map_err(|e| cdk::Error::Custom(format!("bad quote id: {e}")))?;
        let typed_req = MintRequest {
            quote: quote_id,
            outputs: request.outputs,
            signature: request.signature,
        };
        self.mint
            .process_mint_request(MintInput::Single(typed_req))
            .await
    }

    async fn post_batch_check_mint_quote_status(
        &self,
        _method: &PaymentMethod,
        request: BatchCheckMintQuoteRequest<String>,
    ) -> Result<Vec<MintQuoteResponse<String>>, cdk::Error> {
        let mut out = Vec::new();
        for q in request.quotes {
            let quote_id = QuoteId::from_str(&q)
                .map_err(|e| cdk::Error::Custom(format!("bad quote id: {e}")))?;
            let resp = self.mint.check_mint_quote(&quote_id).await?;
            out.push(resp.into());
        }
        Ok(out)
    }

    async fn post_batch_mint(
        &self,
        _method: &PaymentMethod,
        _request: BatchMintRequest<String>,
    ) -> Result<MintResponse, cdk::Error> {
        Err(cdk::Error::Custom("batch mint unused in test".into()))
    }

    async fn post_melt_quote(
        &self,
        request: MeltQuoteRequest,
    ) -> Result<MeltQuoteCreateResponse<String>, cdk::Error> {
        let resp = self.mint.get_melt_quote(request).await?;
        Ok(resp.into())
    }

    async fn get_mint_quote_status(
        &self,
        _method: PaymentMethod,
        quote_id: &str,
    ) -> Result<MintQuoteResponse<String>, cdk::Error> {
        let qid = QuoteId::from_str(quote_id)
            .map_err(|e| cdk::Error::Custom(format!("bad quote id: {e}")))?;
        let resp = self.mint.check_mint_quote(&qid).await?;
        Ok(resp.into())
    }

    async fn get_melt_quote_status(
        &self,
        _method: PaymentMethod,
        quote_id: &str,
    ) -> Result<MeltQuoteResponse<String>, cdk::Error> {
        let qid = QuoteId::from_str(quote_id)
            .map_err(|e| cdk::Error::Custom(format!("bad quote id: {e}")))?;
        let resp = self.mint.check_melt_quote(&qid).await?;
        Ok(resp.into())
    }

    async fn post_melt(
        &self,
        _method: &PaymentMethod,
        request: MeltRequest<String>,
    ) -> Result<MeltQuoteResponse<String>, cdk::Error> {
        let qid = QuoteId::from_str(request.quote())
            .map_err(|e| cdk::Error::Custom(format!("bad quote id: {e}")))?;
        let typed = MeltRequest::new(qid, request.inputs().clone(), request.outputs().clone());
        let pending = self.mint.melt(&typed).await?;
        let resp = pending.await?;
        Ok(resp.into())
    }

    async fn post_swap(&self, request: SwapRequest) -> Result<SwapResponse, cdk::Error> {
        self.mint.process_swap_request(request).await
    }

    async fn get_mint_info(&self) -> Result<MintInfo, cdk::Error> {
        self.mint.mint_info().await
    }

    async fn post_check_state(
        &self,
        request: CheckStateRequest,
    ) -> Result<CheckStateResponse, cdk::Error> {
        self.mint.check_state(&request).await
    }

    async fn post_restore(&self, request: RestoreRequest) -> Result<RestoreResponse, cdk::Error> {
        self.mint.restore(request).await
    }

    async fn get_auth_wallet(&self) -> Option<AuthWallet> {
        None
    }

    async fn set_auth_wallet(&self, _wallet: Option<AuthWallet>) {}
}

struct DirectConnectorFactory {
    mint: Arc<Mint>,
}

#[async_trait]
impl ConnectorFactory for DirectConnectorFactory {
    fn build(&self, _mint_url: &str) -> Result<Arc<dyn MintConnector + Send + Sync>, WalletError> {
        Ok(Arc::new(DirectMintConnector {
            mint: Arc::clone(&self.mint),
        }))
    }
}

/// System-boundary test approval port recording every spend request.
struct TestApprovalPort {
    approve: AtomicBool,
    seen: Mutex<Vec<WalletSpendRequestWire>>,
}

impl TestApprovalPort {
    fn new(approve: bool) -> Self {
        Self {
            approve: AtomicBool::new(approve),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn set_approve(&self, approve: bool) {
        self.approve.store(approve, Ordering::SeqCst);
    }
}

#[async_trait]
impl ApprovalPort for TestApprovalPort {
    async fn request_db_unlock(
        &self,
    ) -> Result<(Zeroizing<String>, Zeroizing<[u8; 64]>), ApprovalError> {
        Ok((Zeroizing::new("a".repeat(64)), Zeroizing::new([1u8; 64])))
    }

    async fn request_spend_approval(
        &self,
        request: WalletSpendRequestWire,
    ) -> Result<WalletSpendApprovalWire, ApprovalError> {
        self.seen.lock().await.push(request.clone());
        if !self.approve.load(Ordering::SeqCst) {
            return Err(ApprovalError::Declined(
                "operator declined in test".to_string(),
            ));
        }
        Ok(WalletSpendApprovalWire {
            approval_id: format!("appr-{}", uuid::Uuid::new_v4()),
            intent_id: request.intent_id,
            amount_sats: request.amount_sats,
            max_fee_sats: request.max_fee_sats,
            destination_hash: request.destination_hash,
            network: request.network,
            rail: request.rail,
            expires_at: request.created_at + 120,
            mac_hex: "00".repeat(32),
            remaining_daily_cap_sats: 40_000,
            nwc_preimage: None,
            nwc_fee_paid_sats: None,
            nwc_payment_hash: None,
        })
    }

    async fn nwc_query(
        &self,
        _operation: WalletNwcOperationWire,
    ) -> Result<WalletNwcResultWire, ApprovalError> {
        Err(ApprovalError::NotConfigured(
            "NWC unused in Cashu test".into(),
        ))
    }
}

async fn start_fake_mint() -> Arc<Mint> {
    let mint_db = Arc::new(
        SQLMintDatabase::<SqlcipherConnectionManager>::new(SqlcipherConfig::in_memory())
            .await
            .expect("mint sqlite db"),
    );
    let fake_wallet = Arc::new(FakeWallet::new(
        FeeReserve {
            min_fee_reserve: Amount::from(2),
            percent_fee_reserve: 0.01,
        },
        HashMap::new(),
        HashSet::new(),
        0,
        CurrencyUnit::Sat,
    ));

    let mut builder = MintBuilder::new(mint_db.clone());
    builder
        .add_payment_processor(
            CurrencyUnit::Sat,
            PaymentMethod::Known(KnownMethod::Bolt11),
            MintMeltLimits::new(1, 100_000),
            fake_wallet,
        )
        .await
        .expect("add fake wallet processor");
    builder = builder.with_name("test-fake-mint".to_string());

    let mint_seed = [42u8; 32];
    let mint = builder
        .build_with_seed(mint_db, &mint_seed)
        .await
        .expect("build mint");
    mint.start().await.expect("start mint");
    Arc::new(mint)
}

fn test_config(db_path: std::path::PathBuf) -> WalletConfig {
    WalletConfig {
        network: WalletNetwork::Regtest,
        mainnet_enabled: false,
        per_tx_cap_sats: 10_000,
        daily_cap_sats: 50_000,
        max_fee_sats: 100,
        allowed_mints: vec![TEST_MINT_URL.to_string()],
        default_mint: TEST_MINT_URL.to_string(),
        nwc_label: String::new(),
        db_path,
    }
}

async fn wait_for_fake_mint_paid(engine: &WalletEngine, quote_id: &str) {
    let wallet = engine.wallet_for_mint(None).await.expect("wallet");
    for _ in 0..40 {
        let q = wallet
            .check_mint_quote_status(quote_id)
            .await
            .expect("check_mint_quote_status");
        if q.state == cdk::nuts::MintQuoteState::Paid {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("fake wallet did not mark mint quote {quote_id} as Paid within 1s");
}

#[tokio::test]
async fn mint_send_receive_p2pk_melt_and_idempotent_retry() {
    let mint = start_fake_mint().await;
    let factory: Arc<dyn ConnectorFactory> = Arc::new(DirectConnectorFactory {
        mint: Arc::clone(&mint),
    });

    let sender_dir = tempfile::tempdir().expect("sender tempdir");
    let sender_approval = Arc::new(TestApprovalPort::new(false));
    let sender = WalletEngine::open_encrypted(
        test_config(sender_dir.path().join("sender.db")),
        Zeroizing::new("1".repeat(64)),
        Zeroizing::new([1u8; 64]),
        sender_approval.clone(),
        Arc::clone(&factory),
    )
    .await
    .expect("open sender");

    // 1. Mint 1,000 sats via NUT-04
    let inv = sender
        .create_invoice("cashu", 1_000, None, Some("initial funding".into()))
        .await
        .expect("create_invoice");
    assert_eq!(inv.amount_sats, 1_000);
    wait_for_fake_mint_paid(&sender, &inv.quote_id).await;
    let minted = sender
        .mint_paid_quote(None, &inv.quote_id)
        .await
        .expect("mint_paid_quote");
    assert_eq!(minted, 1_000);
    assert_eq!(
        sender.balance().await.expect("bal").total_confirmed_sats,
        1_000
    );

    // 2. Declined approval on send_ecash moves 0 sats and releases reserved proofs
    let err = sender
        .send_ecash("send-declined-1", 200, None, None, None)
        .await
        .expect_err("declined must fail");
    assert!(matches!(
        err,
        WalletError::Approval(ApprovalError::Declined(_))
    ));
    let bal_after_decline = sender.balance().await.expect("bal");
    assert_eq!(bal_after_decline.total_confirmed_sats, 1_000);
    assert_eq!(bal_after_decline.total_reserved_sats, 0);

    // 3. Approved bearer send + decode + receive by recipient
    sender_approval.set_approve(true);
    let sent = sender
        .send_ecash("send-ok-1", 200, None, None, Some("coffee".into()))
        .await
        .expect("approved send");
    assert_eq!(sent.amount_sats, 200);
    assert!(!sent.p2pk_locked);

    let decoded = sender.decode_payment(&sent.token).expect("decode token");
    assert_eq!(decoded.kind, "cashu_token");
    assert_eq!(decoded.amount_sats, Some(200));
    assert_eq!(decoded.p2pk_locked, Some(false));

    let recv_key = SecretKey::generate();
    let recv_pubkey_hex = recv_key.public_key().to_hex();
    let recv_dir = tempfile::tempdir().expect("recv tempdir");
    let receiver = WalletEngine::open_encrypted(
        test_config(recv_dir.path().join("recv.db")),
        Zeroizing::new("2".repeat(64)),
        Zeroizing::new([2u8; 64]),
        Arc::new(TestApprovalPort::new(true)),
        Arc::clone(&factory),
    )
    .await
    .expect("open receiver")
    .with_p2pk_key(recv_key);

    let received = receiver
        .receive_ecash(&sent.token)
        .await
        .expect("receive bearer ecash");
    assert_eq!(received.amount_sats, 200);
    assert_eq!(
        receiver
            .balance()
            .await
            .expect("recv bal")
            .total_confirmed_sats,
        200
    );

    // 4. NUT-11 P2PK send: unauthorized wallet fails, authorized recipient redeems
    let p2pk_sent = sender
        .send_ecash("send-p2pk-1", 150, None, Some(&recv_pubkey_hex), None)
        .await
        .expect("p2pk send");
    assert!(p2pk_sent.p2pk_locked);
    let decoded_p2pk = sender
        .decode_payment(&p2pk_sent.token)
        .expect("decode p2pk");
    assert_eq!(decoded_p2pk.p2pk_locked, Some(true));

    let intruder_dir = tempfile::tempdir().expect("intruder tempdir");
    let intruder = WalletEngine::open_encrypted(
        test_config(intruder_dir.path().join("intruder.db")),
        Zeroizing::new("3".repeat(64)),
        Zeroizing::new([3u8; 64]),
        Arc::new(TestApprovalPort::new(true)),
        Arc::clone(&factory),
    )
    .await
    .expect("open intruder");
    assert!(
        intruder.receive_ecash(&p2pk_sent.token).await.is_err(),
        "unauthorized wallet without P2PK secret key must not redeem locked proofs"
    );

    let p2pk_received = receiver
        .receive_ecash(&p2pk_sent.token)
        .await
        .expect("authorized recipient redeems P2PK token");
    assert_eq!(p2pk_received.amount_sats, 150);
    assert_eq!(
        receiver
            .balance()
            .await
            .expect("recv bal")
            .total_confirmed_sats,
        350
    );

    // 5a. Threat T6: paying a mainnet (`lnbc`) BOLT11 on a `regtest` wallet fails closed
    let external_invoice =
        cdk_fake_wallet::create_fake_invoice(100_000, "external merchant".to_string()).to_string();
    let net_mismatch = sender
        .pay_invoice("melt-wrong-net", &external_invoice, "cashu", None)
        .await
        .expect_err("mainnet bolt11 on regtest wallet must fail closed (T6)");
    assert!(matches!(net_mismatch, WalletError::PolicyDenied(_)));

    // 5b. NUT-05 melt + NUT-08 change + T8 idempotent retry refusal on matching Mainnet wallet
    // (`cdk-fake-wallet` generates `lnbc` / mainnet-currency BOLT11 invoices).
    let melt_dir = tempfile::tempdir().expect("melt tempdir");
    let mut melt_cfg = test_config(melt_dir.path().join("melt.db"));
    melt_cfg.network = WalletNetwork::Mainnet;
    melt_cfg.mainnet_enabled = true;
    melt_cfg.allowed_mints = vec!["https://mint.example.com".to_string()];
    melt_cfg.default_mint = "https://mint.example.com".to_string();
    let melter = WalletEngine::open_encrypted(
        melt_cfg,
        Zeroizing::new("5".repeat(64)),
        Zeroizing::new([5u8; 64]),
        Arc::new(TestApprovalPort::new(true)),
        Arc::clone(&factory),
    )
    .await
    .expect("open melter");
    let melt_funding = melter
        .create_invoice("cashu", 500, None, Some("melt funding".into()))
        .await
        .expect("melt funding invoice");
    wait_for_fake_mint_paid(&melter, &melt_funding.quote_id).await;
    melter
        .mint_paid_quote(None, &melt_funding.quote_id)
        .await
        .expect("mint melt funding");

    let paid = melter
        .pay_invoice("melt-1", &external_invoice, "cashu", None)
        .await
        .expect("melt bolt11");
    assert_eq!(paid.amount_sats, 100);
    assert_eq!(paid.status, "settled");
    assert!(
        paid.preimage.is_some(),
        "settled melt must record Lightning preimage"
    );
    // Remaining balance reflects 500 - 100 - fee_paid (NUT-08 change returned)
    let after_melt = melter.balance().await.expect("bal after melt");
    assert_eq!(
        after_melt.total_confirmed_sats,
        500 - 100 - paid.fee_paid_sats
    );

    // Duplicate idempotency_key is refused without double-spend (T8)
    let dup = melter
        .pay_invoice("melt-1", &external_invoice, "cashu", None)
        .await
        .expect_err("duplicate idempotency_key must fail");
    assert!(matches!(dup, WalletError::Duplicate(_)));

    // Duplicate payment_hash with a DIFFERENT idempotency_key is also refused (T8)
    let dup_hash = melter
        .pay_invoice("melt-2-same-invoice", &external_invoice, "cashu", None)
        .await
        .expect_err("duplicate payment_hash must fail");
    assert!(matches!(dup_hash, WalletError::Duplicate(_)));

    // 6. Unallowlisted mint is refused (T4)
    let strict_dir = tempfile::tempdir().expect("strict tempdir");
    let mut strict_cfg = test_config(strict_dir.path().join("strict.db"));
    strict_cfg.allowed_mints = vec!["https://other-mint.example.com".to_string()];
    strict_cfg.default_mint = "https://other-mint.example.com".to_string();
    let strict_wallet = WalletEngine::open_encrypted(
        strict_cfg,
        Zeroizing::new("4".repeat(64)),
        Zeroizing::new([4u8; 64]),
        Arc::new(TestApprovalPort::new(true)),
        Arc::clone(&factory),
    )
    .await
    .expect("open strict wallet");
    let untrusted = strict_wallet
        .receive_ecash(&sent.token)
        .await
        .expect_err("unallowlisted mint token must fail");
    assert!(matches!(untrusted, WalletError::PolicyDenied(_)));
}

#[tokio::test]
async fn crash_recovery_mid_send_recovers_100_percent_after_restart_and_reconcile() {
    let mint = start_fake_mint().await;
    let factory: Arc<dyn ConnectorFactory> = Arc::new(DirectConnectorFactory {
        mint: Arc::clone(&mint),
    });
    let dir = tempfile::tempdir().expect("crash tempdir");
    let db_path = dir.path().join("wallet.db");
    let pass = Zeroizing::new("c".repeat(64));
    let seed = Zeroizing::new([7u8; 64]);

    // Session 1: mint 500 sats, prepare a send (reserving proofs + persisting saga),
    // then simulate SIGKILL by dropping without confirm() or cancel().
    {
        let engine = WalletEngine::open_encrypted(
            test_config(db_path.clone()),
            pass.clone(),
            seed.clone(),
            Arc::new(TestApprovalPort::new(true)),
            Arc::clone(&factory),
        )
        .await
        .expect("open session 1");

        let inv = engine
            .create_invoice("cashu", 500, None, None)
            .await
            .expect("create invoice");
        wait_for_fake_mint_paid(&engine, &inv.quote_id).await;
        engine
            .mint_paid_quote(None, &inv.quote_id)
            .await
            .expect("mint 500");

        let wallet = engine.wallet_for_mint(None).await.expect("wallet");
        let prepared = wallet
            .prepare_send(Amount::from(200), SendOptions::default())
            .await
            .expect("prepare_send");
        // Verify proofs are reserved before simulated crash
        assert!(
            engine.balance().await.expect("bal").total_reserved_sats > 0,
            "proofs should be reserved mid-saga"
        );
        // Simulate process death: drop prepared + engine without confirm/cancel
        drop(prepared);
    }

    // Session 2: reopen from the same encrypted SQLCipher DB and reconcile
    let recovered_engine = WalletEngine::open_encrypted(
        test_config(db_path),
        pass,
        seed,
        Arc::new(TestApprovalPort::new(true)),
        factory,
    )
    .await
    .expect("reopen session 2");

    let report = recovered_engine.reconcile().await.expect("reconcile");
    assert_eq!(report.mints.len(), 1);
    let bal = recovered_engine.balance().await.expect("recovered balance");
    assert_eq!(
        bal.total_confirmed_sats, 500,
        "100% of value must be recovered after mid-send crash + reconcile"
    );
    assert_eq!(bal.total_reserved_sats, 0);
}

#[tokio::test]
async fn wipe_db_and_restore_from_seed_recovers_100_percent_balance() {
    let mint = start_fake_mint().await;
    let factory: Arc<dyn ConnectorFactory> = Arc::new(DirectConnectorFactory {
        mint: Arc::clone(&mint),
    });
    let dir = tempfile::tempdir().expect("restore tempdir");
    let db_path = dir.path().join("wallet.db");
    let pass = Zeroizing::new("d".repeat(64));
    let seed = Zeroizing::new([9u8; 64]);

    // 1. Mint 750 sats into encrypted DB using deterministic NUT-13 seed
    {
        let engine = WalletEngine::open_encrypted(
            test_config(db_path.clone()),
            pass.clone(),
            seed.clone(),
            Arc::new(TestApprovalPort::new(true)),
            Arc::clone(&factory),
        )
        .await
        .expect("open original wallet");

        let inv = engine
            .create_invoice("cashu", 750, None, None)
            .await
            .expect("create invoice");
        wait_for_fake_mint_paid(&engine, &inv.quote_id).await;
        engine
            .mint_paid_quote(None, &inv.quote_id)
            .await
            .expect("mint 750");
        assert_eq!(
            engine.balance().await.expect("bal").total_confirmed_sats,
            750
        );
    }

    // 2. Wipe wallet.db completely (including WAL/SHM sidecars)
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));

    // 3. Reopen fresh empty wallet with the SAME deterministic seed & run NUT-09 restore
    let restored_engine = WalletEngine::open_encrypted(
        test_config(db_path),
        pass,
        seed,
        Arc::new(TestApprovalPort::new(true)),
        factory,
    )
    .await
    .expect("open fresh wallet for restore");

    assert_eq!(
        restored_engine
            .balance()
            .await
            .expect("empty bal")
            .total_confirmed_sats,
        0
    );

    let report = restored_engine
        .restore_from_seed(None)
        .await
        .expect("NUT-09/13 restore");
    assert_eq!(report.total_restored_unspent_sats, 750);
    assert_eq!(
        restored_engine
            .balance()
            .await
            .expect("restored bal")
            .total_confirmed_sats,
        750,
        "NUT-09/13 restore must recover 100% of unspent balance"
    );
}

#[tokio::test]
async fn concurrent_duplicate_idempotency_key_allows_at_most_one_spend() {
    let mint = start_fake_mint().await;
    let factory: Arc<dyn ConnectorFactory> = Arc::new(DirectConnectorFactory {
        mint: Arc::clone(&mint),
    });
    let dir = tempfile::tempdir().expect("race tempdir");
    let engine = Arc::new(
        WalletEngine::open_encrypted(
            test_config(dir.path().join("race.db")),
            Zeroizing::new("9".repeat(64)),
            Zeroizing::new([11u8; 64]),
            Arc::new(TestApprovalPort::new(true)),
            factory,
        )
        .await
        .expect("open race engine"),
    );

    let inv = engine
        .create_invoice("cashu", 1_000, None, None)
        .await
        .expect("create invoice");
    wait_for_fake_mint_paid(&engine, &inv.quote_id).await;
    engine
        .mint_paid_quote(None, &inv.quote_id)
        .await
        .expect("mint 1000");

    // Fire 6 concurrent sends with the SAME idempotency_key
    let (r1, r2, r3, r4, r5, r6) = tokio::join!(
        engine.send_ecash("race-idem-1", 100, None, None, None),
        engine.send_ecash("race-idem-1", 100, None, None, None),
        engine.send_ecash("race-idem-1", 100, None, None, None),
        engine.send_ecash("race-idem-1", 100, None, None, None),
        engine.send_ecash("race-idem-1", 100, None, None, None),
        engine.send_ecash("race-idem-1", 100, None, None, None),
    );
    let results = [r1, r2, r3, r4, r5, r6];
    let ok_count = results.iter().filter(|r| r.is_ok()).count();
    let err_count = results.iter().filter(|r| r.is_err()).count();
    assert_eq!(
        ok_count, 1,
        "at most one concurrent spend with identical idempotency_key may succeed"
    );
    assert_eq!(err_count, 5);

    let bal = engine.balance().await.expect("final bal");
    assert_eq!(
        bal.total_confirmed_sats, 900,
        "balance must reflect exactly one 100-sat spend with zero double-spend"
    );
    assert_eq!(bal.total_reserved_sats, 0);
}
