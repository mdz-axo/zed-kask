//! Phase 3 Lightning NWC (NIP-47) integration tests against a mock NIP-47
//! wallet service boundary (D110 §7 Phase 3 verification gate).
//!
//! Covers:
//! 1. Successful `wallet_balance`, `wallet_create_invoice` (`rail = "nwc"`),
//!    and `wallet_pay_invoice` (`rail = "nwc"`) with cryptographic SHA-256
//!    preimage verification.
//! 2. Timeout during `pay_invoice` → row transitions to `reconciling` →
//!    `wallet_reconcile` (`lookup_invoice`) transitions row to `settled`
//!    (after verifying preimage).
//! 3. Malformed / mismatched preimage response fails closed and marks
//!    `reconciling` (never `settled`).
//! 4. Duplicate `idempotency_key` and duplicate `payment_hash` rejection (T8).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use hkask_mcp_wallet::approval::{ApprovalError, ApprovalPort};
use hkask_mcp_wallet::config::WalletConfig;
use hkask_mcp_wallet::engine::{HttpConnectorFactory, WalletEngine, WalletError};
use hkask_ipc_wire::wallet_wire::{
    WalletNwcOperationWire, WalletNwcResultWire, WalletSpendApprovalWire, WalletSpendRequestWire,
};
use hkask_wallet_core::{WalletNetwork, decode_bolt11, sha256_hex};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Clone)]
enum PayBehavior {
    Settle { preimage: String, fee_sats: u64 },
    Timeout,
    BadPreimage,
}

struct MockNwcApprovalPort {
    balance_sats: u64,
    pay_behavior: Mutex<PayBehavior>,
    lookup_responses: Mutex<HashMap<String, WalletNwcResultWire>>,
}

impl MockNwcApprovalPort {
    fn new(balance_sats: u64, initial_pay: PayBehavior) -> Self {
        Self {
            balance_sats,
            pay_behavior: Mutex::new(initial_pay),
            lookup_responses: Mutex::new(HashMap::new()),
        }
    }

    async fn set_pay_behavior(&self, behavior: PayBehavior) {
        *self.pay_behavior.lock().await = behavior;
    }

    async fn register_lookup(&self, payment_hash: String, resp: WalletNwcResultWire) {
        self.lookup_responses
            .lock()
            .await
            .insert(payment_hash, resp);
    }
}

#[async_trait]
impl ApprovalPort for MockNwcApprovalPort {
    async fn request_db_unlock(
        &self,
    ) -> Result<(Zeroizing<String>, Zeroizing<[u8; 64]>), ApprovalError> {
        Ok((Zeroizing::new("e".repeat(64)), Zeroizing::new([3u8; 64])))
    }

    async fn request_spend_approval(
        &self,
        request: WalletSpendRequestWire,
    ) -> Result<WalletSpendApprovalWire, ApprovalError> {
        let behavior = self.pay_behavior.lock().await.clone();
        match behavior {
            PayBehavior::Settle { preimage, fee_sats } => Ok(WalletSpendApprovalWire {
                approval_id: format!("appr-{}", uuid::Uuid::new_v4()),
                intent_id: request.intent_id,
                amount_sats: request.amount_sats,
                max_fee_sats: request.max_fee_sats,
                destination_hash: request.destination_hash,
                network: request.network,
                rail: request.rail,
                expires_at: request.created_at + 120,
                mac_hex: "00".repeat(32),
                remaining_daily_cap_sats: 45_000,
                nwc_preimage: Some(preimage),
                nwc_fee_paid_sats: Some(fee_sats),
                nwc_payment_hash: request
                    .bolt11
                    .as_deref()
                    .and_then(|b| decode_bolt11(b).ok())
                    .map(|d| d.payment_hash),
            }),
            PayBehavior::Timeout => Err(ApprovalError::Timeout(
                "NWC relay timed out awaiting pay_invoice response".to_string(),
            )),
            PayBehavior::BadPreimage => Ok(WalletSpendApprovalWire {
                approval_id: format!("appr-{}", uuid::Uuid::new_v4()),
                intent_id: request.intent_id,
                amount_sats: request.amount_sats,
                max_fee_sats: request.max_fee_sats,
                destination_hash: request.destination_hash,
                network: request.network,
                rail: request.rail,
                expires_at: request.created_at + 120,
                mac_hex: "00".repeat(32),
                remaining_daily_cap_sats: 45_000,
                nwc_preimage: Some("ff".repeat(32)),
                nwc_fee_paid_sats: Some(1),
                nwc_payment_hash: None,
            }),
        }
    }

    async fn nwc_query(
        &self,
        operation: WalletNwcOperationWire,
    ) -> Result<WalletNwcResultWire, ApprovalError> {
        match operation {
            WalletNwcOperationWire::GetBalance => Ok(WalletNwcResultWire::Balance {
                balance_sats: self.balance_sats,
            }),
            WalletNwcOperationWire::MakeInvoice {
                amount_sats,
                description,
            } => {
                let inv = cdk_fake_wallet::create_fake_invoice(
                    amount_sats * 1_000,
                    description.unwrap_or_else(|| "nwc receive".to_string()),
                );
                let bolt11 = inv.to_string();
                let decoded = decode_bolt11(&bolt11).expect("decode fake invoice");
                Ok(WalletNwcResultWire::Invoice {
                    bolt11,
                    payment_hash: decoded.payment_hash,
                    amount_sats,
                    expiry: decoded.expires_at_secs,
                })
            }
            WalletNwcOperationWire::LookupInvoice { payment_hash } => self
                .lookup_responses
                .lock()
                .await
                .get(&payment_hash)
                .cloned()
                .ok_or_else(|| ApprovalError::Transport("unknown invoice".to_string())),
        }
    }
}

/// Build a signed BOLT11 invoice with a known preimage so SHA-256(preimage) == payment_hash.
fn build_signed_bolt11_with_known_preimage(
    amount_msat: u64,
    preimage_byte: u8,
) -> (String, String, String) {
    use cdk::lightning_invoice::{Currency, InvoiceBuilder};
    use cdk_common::bitcoin::hashes::{Hash as _, sha256};
    use cdk_common::bitcoin::secp256k1::{Secp256k1, SecretKey};

    let preimage = [preimage_byte; 32];
    let preimage_hex = hex::encode(preimage);
    let payment_hash_hex = sha256_hex(&preimage);
    let payment_hash = sha256::Hash::hash(&preimage);
    let secp = Secp256k1::new();
    let privkey = SecretKey::from_slice(&[0x11u8; 32]).expect("valid privkey");

    let invoice = InvoiceBuilder::new(Currency::Bitcoin)
        .description("nwc test payment".to_string())
        .payment_hash(payment_hash)
        .payment_secret(cdk::lightning_invoice::PaymentSecret([0x22u8; 32]))
        .amount_milli_satoshis(amount_msat)
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .build_signed(|hash| secp.sign_ecdsa_recoverable(hash, &privkey))
        .expect("build signed bolt11");

    (invoice.to_string(), preimage_hex, payment_hash_hex)
}

fn nwc_config(db_path: std::path::PathBuf) -> WalletConfig {
    WalletConfig {
        network: WalletNetwork::Mainnet,
        mainnet_enabled: true,
        per_tx_cap_sats: 10_000,
        daily_cap_sats: 50_000,
        max_fee_sats: 100,
        allowed_mints: Vec::new(),
        default_mint: String::new(),
        nwc_label: "wss://relay.getalby.com/v1".to_string(),
        db_path,
    }
}

#[tokio::test]
async fn nwc_pay_preimage_verify_timeout_reconcile_bad_preimage_and_idempotency() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bolt11_ok, preimage_ok, hash_ok) = build_signed_bolt11_with_known_preimage(250_000, 0xAA);
    let port = Arc::new(MockNwcApprovalPort::new(
        123_456,
        PayBehavior::Settle {
            preimage: preimage_ok.clone(),
            fee_sats: 3,
        },
    ));
    let engine = WalletEngine::open_encrypted(
        nwc_config(dir.path().join("nwc.db")),
        Zeroizing::new("f".repeat(64)),
        Zeroizing::new([6u8; 64]),
        port.clone(),
        Arc::new(HttpConnectorFactory::new(false)),
    )
    .await
    .expect("open nwc engine");

    // 1. wallet_balance surfaces NWC balance via editor IPC
    let bal = engine.balance().await.expect("balance");
    assert!(bal.nwc_configured);
    assert_eq!(bal.nwc_balance_sats, Some(123_456));

    // 2. wallet_create_invoice over NWC
    let created = engine
        .create_invoice("nwc", 500, None, Some("tip".into()))
        .await
        .expect("nwc make_invoice");
    assert_eq!(created.rail, "nwc");
    assert_eq!(created.amount_sats, 500);

    // 3. Successful NWC pay_invoice verifies SHA-256(preimage) == payment_hash
    let paid = engine
        .pay_invoice("nwc-idem-1", &bolt11_ok, "nwc", None)
        .await
        .expect("nwc pay_invoice");
    assert_eq!(paid.status, "settled");
    assert_eq!(paid.fee_paid_sats, 3);
    assert_eq!(paid.payment_hash, hash_ok);
    assert_eq!(paid.preimage.as_deref(), Some(preimage_ok.as_str()));

    // 4. Duplicate idempotency_key and duplicate payment_hash are refused (T8)
    assert!(matches!(
        engine
            .pay_invoice("nwc-idem-1", &bolt11_ok, "nwc", None)
            .await,
        Err(WalletError::Duplicate(_))
    ));
    assert!(matches!(
        engine
            .pay_invoice("nwc-idem-other", &bolt11_ok, "nwc", None)
            .await,
        Err(WalletError::Duplicate(_))
    ));

    // 5. Timeout -> ledger row marked `reconciling` -> wallet_reconcile (`lookup_invoice`) -> `settled`
    let (bolt11_to, preimage_to, hash_to) = build_signed_bolt11_with_known_preimage(300_000, 0xBB);
    port.set_pay_behavior(PayBehavior::Timeout).await;
    let err = engine
        .pay_invoice("nwc-idem-timeout", &bolt11_to, "nwc", None)
        .await
        .expect_err("timeout must error");
    assert!(matches!(
        err,
        WalletError::Approval(ApprovalError::Timeout(_))
    ));
    assert_eq!(
        engine
            .status()
            .expect("status")
            .pending_reconciliation_count,
        1
    );

    port.register_lookup(
        hash_to.clone(),
        WalletNwcResultWire::Lookup {
            payment_hash: hash_to.clone(),
            state: "settled".to_string(),
            preimage: Some(preimage_to),
            fee_paid_sats: Some(2),
        },
    )
    .await;

    let rec = engine.reconcile().await.expect("reconcile");
    assert_eq!(rec.nwc_reconciled_settled, 1);
    assert_eq!(rec.pending_ledger_rows_remaining, 0);

    // 6. Malformed / mismatched preimage fails closed and marks `reconciling` (never `settled`)
    let (bolt11_bad, _preimage_bad, _hash_bad) =
        build_signed_bolt11_with_known_preimage(150_000, 0xCC);
    port.set_pay_behavior(PayBehavior::BadPreimage).await;
    let err = engine
        .pay_invoice("nwc-idem-bad-preimage", &bolt11_bad, "nwc", None)
        .await
        .expect_err("mismatched preimage must fail closed");
    assert!(matches!(
        err,
        WalletError::Approval(ApprovalError::InvalidResponse(_))
    ));
    assert_eq!(
        engine
            .status()
            .expect("status")
            .pending_reconciliation_count,
        1
    );
}
