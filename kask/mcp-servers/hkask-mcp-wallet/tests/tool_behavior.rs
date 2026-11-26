//! Tool-behavior contract tests for `hkask-mcp-wallet` through the public
//! `Parameters<T>` MCP tool seam (required by `kask/scripts/check-mcp-tool-tests.sh`).

use std::sync::Arc;

use hkask_mcp_test_support::parse_envelope;
use hkask_mcp_wallet::approval::UnavailableApprovalPort;
use hkask_mcp_wallet::config::WalletConfig;
use hkask_mcp_wallet::engine::{HttpConnectorFactory, WalletEngine};
use hkask_mcp_wallet::server::{
    WalletBalanceParams, WalletCreateInvoiceParams, WalletDecodePaymentParams,
    WalletPayInvoiceParams, WalletSendEcashParams, WalletServer, WalletStatusParams,
    WalletTransactionsParams,
};
use hkask_types::WebID;
use hkask_wallet_core::WalletNetwork;
use rmcp::handler::server::wrapper::Parameters;
use zeroize::Zeroizing;

async fn test_server() -> WalletServer {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = WalletConfig {
        network: WalletNetwork::Regtest,
        mainnet_enabled: false,
        per_tx_cap_sats: 10_000,
        daily_cap_sats: 50_000,
        max_fee_sats: 100,
        allowed_mints: vec![],
        default_mint: String::new(),
        nwc_label: String::new(),
        db_path: dir.path().join("wallet.db"),
    };
    let engine = WalletEngine::open_in_memory(
        cfg,
        Zeroizing::new([1u8; 64]),
        Arc::new(UnavailableApprovalPort),
        Arc::new(HttpConnectorFactory::new(true)),
    )
    .await
    .expect("in-memory engine");
    WalletServer::new(WebID::new(), Some(Arc::new(engine)), None)
}

#[tokio::test]
async fn wallet_status_and_balance_and_transactions_return_annotated_json() {
    let server = test_server().await;

    let status_json = server
        .wallet_status(Parameters(WalletStatusParams {}))
        .await
        .expect("wallet_status");
    let status = parse_envelope(&status_json);
    assert_eq!(status["network"], "regtest");
    assert_eq!(status["mainnet_enabled"], false);
    assert_eq!(status["db_unlocked"], true);

    let bal_json = server
        .wallet_balance(Parameters(WalletBalanceParams {}))
        .await
        .expect("wallet_balance");
    let bal = parse_envelope(&bal_json);
    assert_eq!(bal["total_confirmed_sats"], 0);

    let tx_json = server
        .wallet_transactions(Parameters(WalletTransactionsParams::default()))
        .await
        .expect("wallet_transactions");
    let tx = parse_envelope(&tx_json);
    assert_eq!(tx["count"], 0);
}

#[tokio::test]
async fn wallet_decode_payment_and_invalid_inputs_fail_with_typed_errors() {
    let server = test_server().await;

    let lnaddr_json = server
        .wallet_decode_payment(Parameters(WalletDecodePaymentParams {
            input: "alice@example.com".to_string(),
        }))
        .await
        .expect("decode lightning address");
    let lnaddr = parse_envelope(&lnaddr_json);
    assert_eq!(lnaddr["kind"], "lightning_address");

    // Empty input is rejected
    assert!(
        server
            .wallet_decode_payment(Parameters(WalletDecodePaymentParams {
                input: "   ".to_string(),
            }))
            .await
            .is_err()
    );

    // Zero-amount create_invoice is rejected
    assert!(
        server
            .wallet_create_invoice(Parameters(WalletCreateInvoiceParams {
                rail: "cashu".to_string(),
                amount_sats: 0,
                mint_url: None,
                description: None,
            }))
            .await
            .is_err()
    );

    // Spend without approval port fails closed (T1)
    assert!(
        server
            .wallet_send_ecash(Parameters(WalletSendEcashParams {
                idempotency_key: "idem-1".to_string(),
                amount_sats: 100,
                mint_url: None,
                p2pk_pubkey: None,
                memo: None,
            }))
            .await
            .is_err()
    );
    assert!(
        server
            .wallet_pay_invoice(Parameters(WalletPayInvoiceParams {
                idempotency_key: "idem-2".to_string(),
                bolt11: "not-a-bolt11".to_string(),
                rail: "cashu".to_string(),
                mint_url: None,
            }))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn locked_wallet_server_reports_locked_status_and_refuses_balance() {
    let server = WalletServer::new(WebID::new(), None, Some("keychain locked".to_string()));
    let status_json = server
        .wallet_status(Parameters(WalletStatusParams {}))
        .await
        .expect("locked status succeeds");
    let status = parse_envelope(&status_json);
    assert_eq!(status["db_unlocked"], false);

    assert!(
        server
            .wallet_balance(Parameters(WalletBalanceParams {}))
            .await
            .is_err(),
        "locked server must refuse wallet_balance"
    );

    // Pure-local decode works even while the wallet DB is locked
    let decoded_json = server
        .wallet_decode_payment(Parameters(WalletDecodePaymentParams {
            input: "alice@example.com".to_string(),
        }))
        .await
        .expect("pure-local decode must work while locked");
    let decoded = parse_envelope(&decoded_json);
    assert_eq!(decoded["kind"], "lightning_address");
}

/// A7 (`exp_a2be8e6e7445464585ad131f4519eed4`) task 2: `acn_commit_escrow_approval_gate`.
///
/// expect: an unattended ACN escrow spend through `wallet_send_ecash` without
/// native editor `ApprovalPort::request_spend_approval` approval deterministically
/// fails closed with zero sats spent and zero settled transactions.
#[tokio::test]
async fn acn_commit_escrow_approval_gate() {
    let server = test_server().await;

    let send_err = server
        .wallet_send_ecash(Parameters(WalletSendEcashParams {
            idempotency_key: "a7-acn-escrow-001".to_string(),
            amount_sats: 100,
            mint_url: None,
            memo: Some("ACN commit escrow lock: a7-loopback-ask-001".to_string()),
            p2pk_pubkey: None,
        }))
        .await
        .expect_err("unattended ACN escrow spend must be refused without ApprovalPort");
    assert!(
        !send_err.to_string().is_empty(),
        "refusal carries a diagnostic reason"
    );

    let pay_err = server
        .wallet_pay_invoice(Parameters(WalletPayInvoiceParams {
            idempotency_key: "a7-acn-pay-001".to_string(),
            rail: "nwc".to_string(),
            bolt11: "lnbcrt100n1p0test".to_string(),
            mint_url: None,
        }))
        .await
        .expect_err("unattended NWC settlement must be refused without ApprovalPort");
    assert!(
        !pay_err.to_string().is_empty(),
        "NWC refusal carries a diagnostic reason"
    );

    let tx_json = server
        .wallet_transactions(Parameters(WalletTransactionsParams::default()))
        .await
        .expect("wallet_transactions");
    let tx = parse_envelope(&tx_json);
    assert_eq!(
        tx["count"], 0,
        "zero transactions settle when ApprovalPort is absent"
    );
}
