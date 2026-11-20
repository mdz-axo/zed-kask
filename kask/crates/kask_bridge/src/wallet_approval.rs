//! Native editor-side wallet spend approval & DB unlock seam (D110).
//!
//! Mirrors `nostr_signing.rs` with strictly stronger financial invariants:
//! - **No `autopublish`-equivalent bypass.** Every value-moving spend
//!   (`wallet_pay_invoice`, `wallet_send_ecash`, `nostr_zap`, `nostr_nutzap`)
//!   MUST display a native GPUI modal showing rail, network, exact sats, max
//!   fee, destination, and remaining 24h cap before any secret is read or any
//!   budget is reserved.
//! - **No window = refusal.** If no active editor window is available, or the
//!   operator declines/dismisses the modal, the request fails closed without
//!   touching the OS keychain or rolling spend ledger.
//! - **Hard caps enforced editor-side.** Network match, `mainnet_enabled`,
//!   mint allowlist, `per_tx_cap_sats`, `max_fee_sats`, and rolling 24h
//!   `daily_cap_sats` are checked before opening any modal and re-checked
//!   atomically on reservation.
//! - **Mainnet consent belt (T10).** Mainnet spends additionally verify the
//!   keychain-backed `WALLET_MAINNET_CONSENT_CREDENTIAL_URL` flag after
//!   operator approval so a `settings.json`-only edit cannot activate mainnet.

use std::str::FromStr as _;
use std::sync::{Arc, LazyLock, Mutex};

use credentials_provider::CredentialsProvider;
use gpui::{AppContext as _, AsyncApp, PromptLevel};
use hkask_types::inference_ipc::InferenceErrorPayload;
use hkask_types::wallet_wire::{
    WalletNwcOperationWire, WalletNwcResultWire, WalletSpendApprovalWire, WalletSpendRequestWire,
};
use hkask_wallet_core::{
    LightningError, LightningRail, PaymentRail, PolicyRefusal, RollingSpendLedger, Satoshis,
    SpendIntent, WalletNetwork, decode_bolt11, generate_session_hmac_key, mint_approval_receipt,
    policy::APPROVAL_RECEIPT_TTL_SECS,
};
use settings::Settings as _;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use crate::settings::KaskSettings;

/// Bounded queue capacity for wallet approval/unlock requests between tokio
/// IPC dispatch and the GPUI main thread.
pub(crate) const WALLET_APPROVAL_QUEUE_CAPACITY: usize = 8;

/// Negative button label on the native spend approval modal.
pub const DECLINE_LABEL: &str = "Decline";

/// Format the affirmative button label with exact satoshis.
pub fn approve_button_label(amount_sats: u64) -> String {
    format!("Approve Payment ({amount_sats} sats)")
}

/// Process-global rolling 24h spend ledger owned exclusively by the editor.
static SPEND_LEDGER: LazyLock<Mutex<RollingSpendLedger>> =
    LazyLock::new(|| Mutex::new(RollingSpendLedger::new()));

/// Process-ephemeral 32-byte HMAC key used to sign short-lived approval receipts.
static SESSION_HMAC_KEY: LazyLock<Zeroizing<[u8; 32]>> = LazyLock::new(|| {
    generate_session_hmac_key().unwrap_or_else(|_| {
        // Fallback only if OS CSPRNG fails; process-unique UUID bytes ensure
        // non-zero entropy without panicking.
        let mut fallback = Zeroizing::new([0u8; 32]);
        let a = *uuid::Uuid::new_v4().as_bytes();
        let b = *uuid::Uuid::new_v4().as_bytes();
        fallback[..16].copy_from_slice(&a);
        fallback[16..].copy_from_slice(&b);
        fallback
    })
});

/// What an admitted wallet IPC request asks the editor to perform.
#[derive(Debug)]
pub(crate) enum WalletApprovalOperation {
    /// Return the keychain-held SQLCipher DB passphrase for `hkask-mcp-wallet`.
    DbUnlock,
    /// Validate policy/caps, obtain native GPUI confirmation, reserve budget,
    /// and return a signed approval receipt (plus editor-side NWC execution for
    /// Lightning spends).
    SpendApprove(SpendIntent),
    /// Execute a non-spending editor-side NWC query (`get_balance`,
    /// `make_invoice`, `lookup_invoice`) without exposing `wallet_nwc_uri`.
    NwcQuery(WalletNwcOperationWire),
}

/// Outcome returned over the oneshot channel to `inference_ipc_server`.
#[derive(Debug)]
pub(crate) enum WalletApprovalResponse {
    DbKey {
        passphrase: Zeroizing<String>,
        seed_hex: Zeroizing<String>,
    },
    SpendApproved(WalletSpendApprovalWire),
    NwcResult(WalletNwcResultWire),
}

/// Admitted request carried from tokio dispatch to the GPUI handler.
pub(crate) struct WalletApprovalRequest {
    pub(crate) operation: WalletApprovalOperation,
    pub(crate) grant: String,
    pub(crate) reply: oneshot::Sender<Result<WalletApprovalResponse, InferenceErrorPayload>>,
}

fn refusal(code: &str, message: impl Into<String>) -> InferenceErrorPayload {
    InferenceErrorPayload {
        code: code.into(),
        message: message.into(),
        status: None,
    }
}

fn policy_refusal_to_ipc(err: PolicyRefusal) -> InferenceErrorPayload {
    let code = match &err {
        PolicyRefusal::MainnetDisabled
        | PolicyRefusal::MintNotAllowlisted
        | PolicyRefusal::PerTxCapExceeded { .. }
        | PolicyRefusal::FeeCapExceeded { .. }
        | PolicyRefusal::DailyCapExceeded { .. } => "PolicyDenied",
        PolicyRefusal::DuplicateIntent => "Conflict",
        _ => "InvalidArgument",
    };
    refusal(code, err.to_string())
}

fn parse_rail(raw: &str) -> Result<PaymentRail, InferenceErrorPayload> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "cashu" => Ok(PaymentRail::Cashu),
        "nwc" => Ok(PaymentRail::Nwc),
        _ => Err(refusal(
            "InvalidArgument",
            "unsupported payment rail (expected cashu or nwc)",
        )),
    }
}

/// Pure admission check for `InferenceMethod::WalletDbUnlock` (tokio side).
pub(crate) fn admit_db_unlock(
    grant: Option<&str>,
) -> Result<WalletApprovalOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::WALLET_DB_UNLOCK) {
        return Err(refusal(
            "Auth",
            "Unlocking the wallet database requires the parent-granted host/wallet_db_unlock \
             authority (kask.mcp.delegated_tools for this server)",
        ));
    }
    Ok(WalletApprovalOperation::DbUnlock)
}

/// Pure admission check for `InferenceMethod::WalletNwcQuery` (tokio side).
pub(crate) fn admit_nwc_query(
    grant: Option<&str>,
    query: Option<&WalletNwcOperationWire>,
) -> Result<WalletApprovalOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::WALLET_DB_UNLOCK) {
        return Err(refusal(
            "Auth",
            "NWC wallet queries require the parent-granted host/wallet_db_unlock authority \
             (kask.mcp.delegated_tools for this server)",
        ));
    }
    let Some(query) = query else {
        return Err(refusal(
            "InvalidArgument",
            "wallet_nwc_query payload is required",
        ));
    };
    if let WalletNwcOperationWire::MakeInvoice { amount_sats, .. } = query {
        if *amount_sats == 0 {
            return Err(refusal(
                "InvalidArgument",
                "make_invoice amount_sats must be greater than zero",
            ));
        }
    }
    if let WalletNwcOperationWire::LookupInvoice { payment_hash } = query {
        if payment_hash.trim().len() != 64
            || !payment_hash.trim().bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(refusal(
                "InvalidArgument",
                "lookup_invoice payment_hash must be 64 hex characters",
            ));
        }
    }
    Ok(WalletApprovalOperation::NwcQuery(query.clone()))
}

/// Pure admission check for `InferenceMethod::WalletSpendApprove` (tokio side).
/// Validates the delegation grant and converts + validates the wire intent shape.
pub(crate) fn admit_spend(
    grant: Option<&str>,
    wire: Option<&WalletSpendRequestWire>,
) -> Result<WalletApprovalOperation, InferenceErrorPayload> {
    if !crate::delegation_grants::grant_holds(grant, crate::delegation_grants::WALLET_SPEND_APPROVE)
    {
        return Err(refusal(
            "Auth",
            "Approving a wallet spend requires the parent-granted host/wallet_spend_approve \
             authority (kask.mcp.delegated_tools for this server)",
        ));
    }
    let Some(wire) = wire else {
        return Err(refusal(
            "InvalidArgument",
            "wallet_spend_request payload is required",
        ));
    };
    let rail = parse_rail(&wire.rail)?;
    let network = WalletNetwork::from_str(&wire.network)
        .map_err(|e| refusal("InvalidArgument", e.to_string()))?;

    let intent = SpendIntent {
        intent_id: wire.intent_id.clone(),
        idempotency_key: wire.idempotency_key.clone(),
        rail,
        network,
        amount: Satoshis::new(wire.amount_sats),
        max_fee: Satoshis::new(wire.max_fee_sats),
        mint_url: wire.mint_url.clone(),
        destination_summary: wire.destination_summary.clone(),
        destination_hash: wire.destination_hash.clone(),
        bolt11: wire.bolt11.clone(),
        created_at: wire.created_at,
    };
    intent.validate().map_err(policy_refusal_to_ipc)?;

    Ok(WalletApprovalOperation::SpendApprove(intent))
}

/// Headline message shown on the native GPUI spend approval modal.
pub(crate) fn spend_approval_message(intent: &SpendIntent) -> String {
    if intent.network.is_mainnet() {
        format!(
            "[MAINNET — REAL FUNDS] Approve {} payment of {}?",
            intent.rail, intent.amount
        )
    } else {
        format!(
            "[{}] Approve {} payment of {}?",
            intent.network.display_upper(),
            intent.rail,
            intent.amount
        )
    }
}

/// Full breakdown shown in the modal detail body so what is approved is exact.
pub(crate) fn spend_approval_detail(
    intent: &SpendIntent,
    remaining_after: Satoshis,
) -> Result<String, InferenceErrorPayload> {
    let total = intent
        .total_commitment()
        .map_err(|e| refusal("InvalidArgument", e.to_string()))?;
    let rail_line = match intent.rail {
        PaymentRail::Cashu => format!(
            "Rail: Cashu ({})",
            intent.mint_url.as_deref().unwrap_or("<unspecified>")
        ),
        PaymentRail::Nwc => "Rail: Lightning (NWC)".to_string(),
    };
    let network_line = if intent.network.is_mainnet() {
        "Network: MAINNET (WARNING: real Bitcoin/satoshis — irreversible)"
    } else {
        match intent.network {
            WalletNetwork::Regtest => "Network: REGTEST",
            WalletNetwork::Signet => "Network: SIGNET",
            WalletNetwork::Mainnet => unreachable!(),
        }
    };
    Ok(format!(
        "{rail_line}\n\
         {network_line}\n\
         Amount: {}\n\
         Max Fee: {}\n\
         Total Commitment: {}\n\
         Destination: {}\n\
         Remaining 24h Budget After Tx: {}\n\
         Intent ID: {}",
        intent.amount,
        intent.max_fee,
        total,
        intent.destination_summary,
        remaining_after,
        intent.intent_id
    ))
}

/// Only button index 0 (`Approve Payment (<N> sats)`) approves.
/// Button index 1 (`Decline`) or a dismissed/closed window never approves.
pub(crate) fn answer_approves<E>(answer: Result<usize, E>) -> bool {
    matches!(answer, Ok(0))
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn read_keychain_slot(
    url: &'static str,
    missing_message: &'static str,
    cx: &AsyncApp,
) -> Result<Zeroizing<String>, InferenceErrorPayload> {
    let provider: Arc<dyn CredentialsProvider> =
        cx.update(|cx| zed_credentials_provider::global(cx));
    match provider.read_credentials(url, cx).await {
        Ok(Some((_, bytes))) if !bytes.is_empty() => {
            Ok(Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()))
        }
        Ok(_) => Err(refusal("NotConfigured", missing_message)),
        Err(error) => Err(refusal(
            "Unavailable",
            format!("the keychain could not be read: {error}"),
        )),
    }
}

/// Handle one admitted wallet request on the GPUI side.
pub(crate) async fn handle(request: WalletApprovalRequest, cx: &mut AsyncApp) {
    handle_with_state(
        request,
        &SPEND_LEDGER,
        &SESSION_HMAC_KEY[..],
        current_unix_secs(),
        cx,
    )
    .await;
}

/// Testable core of the GPUI-side wallet approval handler.
async fn handle_with_state(
    request: WalletApprovalRequest,
    ledger: &Mutex<RollingSpendLedger>,
    hmac_key: &[u8],
    now_secs: u64,
    cx: &mut AsyncApp,
) {
    if request.reply.is_closed() {
        return;
    }
    let WalletApprovalRequest {
        operation,
        grant,
        reply,
    } = request;

    match operation {
        WalletApprovalOperation::DbUnlock => {
            if !crate::delegation_grants::grant_holds(
                Some(&grant),
                crate::delegation_grants::WALLET_DB_UNLOCK,
            ) {
                let _ = reply.send(Err(refusal(
                    "Auth",
                    "the wallet_db_unlock grant was revoked before execution",
                )));
                return;
            }
            let passphrase = match read_keychain_slot(
                crate::WALLET_DB_PASSPHRASE_CREDENTIAL_URL,
                "wallet database passphrase is not initialized; configure the wallet in \
                 Settings > Kask > Wallet",
                cx,
            )
            .await
            {
                Ok(p) => p,
                Err(err) => {
                    let _ = reply.send(Err(err));
                    return;
                }
            };
            let mnemonic = match read_keychain_slot(
                crate::WALLET_SEED_MNEMONIC_CREDENTIAL_URL,
                "wallet BIP-39 seed mnemonic is not initialized; configure the wallet in \
                 Settings > Kask > Wallet",
                cx,
            )
            .await
            {
                Ok(m) => m,
                Err(err) => {
                    let _ = reply.send(Err(err));
                    return;
                }
            };
            let seed_hex = match hkask_wallet_core::derive_nut13_seed_hex_from_mnemonic(&mnemonic) {
                Ok(s) => s,
                Err(err) => {
                    let _ = reply.send(Err(refusal("InvalidArgument", err.to_string())));
                    return;
                }
            };
            let _ = reply.send(Ok(WalletApprovalResponse::DbKey {
                passphrase,
                seed_hex,
            }));
        }
        WalletApprovalOperation::NwcQuery(query) => {
            if !crate::delegation_grants::grant_holds(
                Some(&grant),
                crate::delegation_grants::WALLET_DB_UNLOCK,
            ) {
                let _ = reply.send(Err(refusal(
                    "Auth",
                    "the wallet_db_unlock grant was revoked before NWC query execution",
                )));
                return;
            }
            let nwc_uri = match read_keychain_slot(
                crate::WALLET_NWC_URI_CREDENTIAL_URL,
                "NWC connection URI is not configured; set it in Settings > Kask > Wallet",
                cx,
            )
            .await
            {
                Ok(u) => u,
                Err(err) => {
                    let _ = reply.send(Err(err));
                    return;
                }
            };
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let rail = crate::nwc_rail::NwcRail::new(nwc_uri);
                    crate::nwc_rail::execute_nwc_query(&rail, &query)
                        .await
                        .map(WalletApprovalResponse::NwcResult)
                        .map_err(lightning_err_to_ipc)
                })
                .await;
            let _ = reply.send(outcome);
        }
        WalletApprovalOperation::SpendApprove(intent) => {
            if !crate::delegation_grants::grant_holds(
                Some(&grant),
                crate::delegation_grants::WALLET_SPEND_APPROVE,
            ) {
                let _ = reply.send(Err(refusal(
                    "Auth",
                    "the wallet_spend_approve grant was revoked before execution",
                )));
                return;
            }

            let policy = cx.update(|cx| KaskSettings::get_global(cx).wallet.to_spend_cap_policy());

            // 1. Pre-modal policy + rolling 24h cap check (pure — no keychain read).
            let remaining_after = {
                let Ok(guard) = ledger.lock() else {
                    let _ = reply.send(Err(refusal(
                        "Unavailable",
                        "wallet spend ledger mutex poisoned; failing closed",
                    )));
                    return;
                };
                match guard.check_policy(&policy, &intent, now_secs) {
                    Ok(remaining) => remaining,
                    Err(err) => {
                        let _ = reply.send(Err(policy_refusal_to_ipc(err)));
                        return;
                    }
                }
            };

            // 2. Mandatory active editor window — no window = refusal, zero keychain reads.
            let Some(window) = cx.update(|cx| {
                cx.active_window()
                    .or_else(|| cx.windows().into_iter().next())
            }) else {
                let _ = reply.send(Err(refusal(
                    "Unavailable",
                    "wallet spending requires an open editor window for native operator approval",
                )));
                return;
            };

            let message = spend_approval_message(&intent);
            let detail = match spend_approval_detail(&intent, remaining_after) {
                Ok(d) => d,
                Err(err) => {
                    let _ = reply.send(Err(err));
                    return;
                }
            };
            let approve_label = approve_button_label(intent.amount.as_u64());
            let buttons = [approve_label.as_str(), DECLINE_LABEL];

            // 3. Mandatory native GPUI confirmation modal (never bypassed).
            let answer = cx.update_window(window, |_, window, cx| {
                window.prompt(PromptLevel::Warning, &message, Some(&detail), &buttons, cx)
            });
            let approved = match answer {
                Ok(receiver) => answer_approves(receiver.await),
                Err(_) => false,
            };
            if !approved {
                let _ = reply.send(Err(refusal(
                    "Cancelled",
                    "the operator declined the payment; no funds moved",
                )));
                return;
            }

            // 4. Only AFTER explicit operator approval: verify mainnet consent belt (T10),
            //    reserve budget atomically, and mint the short-lived HMAC receipt.
            let outcome =
                finalize_after_approval(&intent, &policy, ledger, hmac_key, now_secs, cx).await;
            let _ = reply.send(outcome);
        }
    }
}

/// Post-approval finalization: verifies keychain-backed mainnet consent (T10),
/// atomically reserves against the rolling 24h spend ledger, and mints a
/// short-lived HMAC-SHA256 approval receipt.
async fn finalize_after_approval(
    intent: &SpendIntent,
    policy: &hkask_wallet_core::SpendCapPolicy,
    ledger: &Mutex<RollingSpendLedger>,
    hmac_key: &[u8],
    now_secs: u64,
    cx: &AsyncApp,
) -> Result<WalletApprovalResponse, InferenceErrorPayload> {
    if policy.network.is_mainnet() {
        let consent = read_keychain_slot(
            crate::WALLET_MAINNET_CONSENT_CREDENTIAL_URL,
            "mainnet spending requires operator consent recorded via Settings > Kask > Wallet",
            cx,
        )
        .await?;
        if consent.trim() != "enabled" {
            return Err(refusal(
                "PolicyDenied",
                "mainnet consent flag in keychain is not active; refusing spend",
            ));
        }
    }

    let remaining_after = {
        let mut guard = ledger.lock().map_err(|_| {
            refusal(
                "Unavailable",
                "wallet spend ledger mutex poisoned; failing closed",
            )
        })?;
        guard
            .reserve(policy, intent, now_secs)
            .map_err(policy_refusal_to_ipc)?
    };

    let approval_id = uuid::Uuid::new_v4().to_string();
    let expires_at = now_secs.saturating_add(APPROVAL_RECEIPT_TTL_SECS);
    let receipt = mint_approval_receipt(hmac_key, approval_id, intent, expires_at)
        .map_err(policy_refusal_to_ipc)?;

    // For NWC Lightning spends, execute `pay_invoice` editor-side using the
    // keychain-held `wallet_nwc_uri` so the secret never enters the MCP child.
    let (nwc_preimage, nwc_fee_paid_sats, nwc_payment_hash) = if intent.rail == PaymentRail::Nwc {
        let bolt11 = intent
            .bolt11
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                if let Ok(mut guard) = ledger.lock() {
                    guard.release(&intent.intent_id);
                }
                refusal(
                    "InvalidArgument",
                    "NWC spend approval requires intent.bolt11",
                )
            })?
            .to_string();
        let decoded = decode_bolt11(&bolt11).map_err(|e| {
            if let Ok(mut guard) = ledger.lock() {
                guard.release(&intent.intent_id);
            }
            refusal("InvalidArgument", e.to_string())
        })?;
        let nwc_uri = match read_keychain_slot(
            crate::WALLET_NWC_URI_CREDENTIAL_URL,
            "NWC connection URI is not configured; set it in Settings > Kask > Wallet",
            cx,
        )
        .await
        {
            Ok(uri) => uri,
            Err(err) => {
                if let Ok(mut guard) = ledger.lock() {
                    guard.release(&intent.intent_id);
                }
                return Err(err);
            }
        };
        let expected_hash = decoded.payment_hash;
        let pay_res = cx
            .background_executor()
            .spawn(async move {
                let rail = crate::nwc_rail::NwcRail::new(nwc_uri);
                rail.pay_invoice(&bolt11, &expected_hash).await
            })
            .await;
        match pay_res {
            Ok(paid) => {
                let fee_sats = paid
                    .fee_paid_msat
                    .to_satoshis_ceil()
                    .map(Satoshis::as_u64)
                    .unwrap_or(0);
                (Some(paid.preimage), Some(fee_sats), Some(paid.payment_hash))
            }
            Err(err) => {
                // Release the 24h budget reservation only on explicit failure;
                // on Timeout or InvalidPreimage keep the reservation pending
                // reconciliation so we never under-count in-flight spends.
                if matches!(err, LightningError::PaymentFailed(_)) {
                    if let Ok(mut guard) = ledger.lock() {
                        guard.release(&intent.intent_id);
                    }
                }
                return Err(lightning_err_to_ipc(err));
            }
        }
    } else {
        (None, None, None)
    };

    Ok(WalletApprovalResponse::SpendApproved(
        WalletSpendApprovalWire {
            approval_id: receipt.approval_id,
            intent_id: receipt.intent_id,
            amount_sats: receipt.amount_sats,
            max_fee_sats: receipt.max_fee_sats,
            destination_hash: receipt.destination_hash,
            network: receipt.network.as_str().to_string(),
            rail: receipt.rail.as_str().to_string(),
            expires_at: receipt.expires_at,
            mac_hex: receipt.mac_hex,
            remaining_daily_cap_sats: remaining_after.as_u64(),
            nwc_preimage,
            nwc_fee_paid_sats,
            nwc_payment_hash,
        },
    ))
}

fn lightning_err_to_ipc(err: LightningError) -> InferenceErrorPayload {
    let code = match &err {
        LightningError::NotConfigured => "NotConfigured",
        LightningError::Timeout => "Timeout",
        LightningError::InvalidPreimage | LightningError::MalformedResponse(_) => "InvalidResponse",
        LightningError::PaymentFailed(_) => "PaymentFailed",
        LightningError::Transport(_) => "Unavailable",
    };
    refusal(code, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegation_grants::{
        NOSTR_SIGN, WALLET_DB_UNLOCK, WALLET_SPEND_APPROVE, grant_for_server,
        revoke_delegation_grant,
    };

    fn sample_wire(network: &str, amount_sats: u64, max_fee_sats: u64) -> WalletSpendRequestWire {
        WalletSpendRequestWire {
            intent_id: format!("intent-{}", uuid::Uuid::new_v4()),
            idempotency_key: format!("idem-{}", uuid::Uuid::new_v4()),
            rail: "cashu".into(),
            network: network.into(),
            amount_sats,
            max_fee_sats,
            mint_url: Some("http://127.0.0.1:3338".into()),
            destination_summary: "lnbcrt10u...".into(),
            destination_hash: "ab".repeat(32),
            bolt11: None,
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn admission_refuses_without_wallet_grants_or_valid_shape() {
        let server = format!("test-wallet-admit-{}", uuid::Uuid::new_v4());
        let wire = sample_wire("regtest", 500, 10);

        // No grant
        assert_eq!(admit_db_unlock(None).unwrap_err().code, "Auth");
        assert_eq!(admit_spend(None, Some(&wire)).unwrap_err().code, "Auth");

        // Wrong grant (NOSTR_SIGN does not admit wallet ops)
        let nostr = grant_for_server(&server, &[NOSTR_SIGN.to_string()]).expect("grant");
        assert_eq!(admit_db_unlock(Some(&nostr)).unwrap_err().code, "Auth");
        assert_eq!(
            admit_spend(Some(&nostr), Some(&wire)).unwrap_err().code,
            "Auth"
        );

        // Right grants
        let wallet = grant_for_server(
            &server,
            &[
                WALLET_DB_UNLOCK.to_string(),
                WALLET_SPEND_APPROVE.to_string(),
            ],
        )
        .expect("grant");
        assert!(admit_db_unlock(Some(&wallet)).is_ok());
        assert!(admit_spend(Some(&wallet), Some(&wire)).is_ok());
        assert_eq!(
            admit_spend(Some(&wallet), None).unwrap_err().code,
            "InvalidArgument"
        );

        let mut zero_wire = wire;
        zero_wire.amount_sats = 0;
        assert_eq!(
            admit_spend(Some(&wallet), Some(&zero_wire))
                .unwrap_err()
                .code,
            "InvalidArgument"
        );

        revoke_delegation_grant(&server);
    }

    #[test]
    fn only_button_zero_approves_spend() {
        assert!(answer_approves::<()>(Ok(0)));
        assert!(!answer_approves::<()>(Ok(1)));
        assert!(!answer_approves::<()>(Ok(2)));
        assert!(!answer_approves(Err(())));
    }

    #[gpui::test]
    async fn missing_window_and_policy_refusals_never_touch_keychain(
        cx: &mut gpui::TestAppContext,
    ) {
        let fake = cx.update(zed_credentials_provider::set_fake_credentials_provider);
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
            KaskSettings::register(cx);
        });
        let server = format!("test-wallet-gpui-{}", uuid::Uuid::new_v4());
        let grant = grant_for_server(
            &server,
            &[
                WALLET_DB_UNLOCK.to_string(),
                WALLET_SPEND_APPROVE.to_string(),
            ],
        )
        .expect("grant");

        let ledger = Mutex::new(RollingSpendLedger::new());
        let hmac_key = [7u8; 32];
        let mut async_cx = cx.to_async();

        // 1. Unallowlisted mint fails closed before any window or keychain read
        let wire = sample_wire("regtest", 500, 10);
        let op = admit_spend(Some(&grant), Some(&wire)).expect("admitted");
        let (tx, rx) = oneshot::channel();
        handle_with_state(
            WalletApprovalRequest {
                operation: op,
                grant: grant.clone(),
                reply: tx,
            },
            &ledger,
            &hmac_key,
            1_700_000_000,
            &mut async_cx,
        )
        .await;
        let err = rx.await.expect("reply").unwrap_err();
        assert_eq!(err.code, "PolicyDenied");
        assert!(
            fake.take_reads().is_empty(),
            "policy denial must never read keychain"
        );

        // 2. Allowlist the mint, but with NO open window -> Unavailable, zero keychain reads
        cx.update(|cx| {
            let mut settings = KaskSettings::default();
            settings.wallet.allowed_mints = vec!["http://127.0.0.1:3338".to_string()];
            cx.global_mut::<settings::SettingsStore>()
                .override_global(settings);
        });
        let op = admit_spend(Some(&grant), Some(&wire)).expect("admitted");
        let (tx, rx) = oneshot::channel();
        handle_with_state(
            WalletApprovalRequest {
                operation: op,
                grant: grant.clone(),
                reply: tx,
            },
            &ledger,
            &hmac_key,
            1_700_000_000,
            &mut async_cx,
        )
        .await;
        let err = rx.await.expect("reply").unwrap_err();
        assert_eq!(err.code, "Unavailable");
        assert!(
            fake.take_reads().is_empty(),
            "missing-window refusal must never read keychain"
        );

        // 3. DbUnlock reads only WALLET_DB_PASSPHRASE_CREDENTIAL_URL
        let (tx, rx) = oneshot::channel();
        handle_with_state(
            WalletApprovalRequest {
                operation: WalletApprovalOperation::DbUnlock,
                grant: grant.clone(),
                reply: tx,
            },
            &ledger,
            &hmac_key,
            1_700_000_000,
            &mut async_cx,
        )
        .await;
        let err = rx.await.expect("reply").unwrap_err();
        assert_eq!(err.code, "NotConfigured");
        assert_eq!(
            fake.take_reads(),
            vec![crate::WALLET_DB_PASSPHRASE_CREDENTIAL_URL.to_string()]
        );

        revoke_delegation_grant(&server);
    }

    #[gpui::test]
    async fn mainnet_finalization_requires_keychain_consent_and_mints_verifiable_receipt(
        cx: &mut gpui::TestAppContext,
    ) {
        let fake = cx.update(zed_credentials_provider::set_fake_credentials_provider);
        let async_cx = cx.to_async();

        let policy = hkask_wallet_core::SpendCapPolicy {
            network: WalletNetwork::Mainnet,
            mainnet_enabled: true,
            per_tx_cap_sats: Satoshis::new(10_000),
            daily_cap_sats: Satoshis::new(50_000),
            max_fee_sats: Satoshis::new(100),
            allowed_mints: vec!["https://mint.example.com".to_string()],
        };
        let intent = SpendIntent {
            intent_id: "intent-mainnet-1".into(),
            idempotency_key: "idem-mainnet-1".into(),
            rail: PaymentRail::Cashu,
            network: WalletNetwork::Mainnet,
            amount: Satoshis::new(1_000),
            max_fee: Satoshis::new(10),
            mint_url: Some("https://mint.example.com".into()),
            destination_summary: "lnbc10u...".into(),
            destination_hash: "cd".repeat(32),
            bolt11: None,
            created_at: 1_700_000_000,
        };
        let ledger = Mutex::new(RollingSpendLedger::new());
        let hmac_key = [9u8; 32];

        // Without keychain consent flag -> fails closed (T10)
        let err = finalize_after_approval(
            &intent,
            &policy,
            &ledger,
            &hmac_key,
            1_700_000_000,
            &async_cx,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, "NotConfigured");
        assert_eq!(
            ledger
                .lock()
                .expect("lock")
                .spent_in_window(1_700_000_000)
                .expect("sum"),
            Satoshis::ZERO,
            "failed mainnet consent must not reserve budget"
        );

        // Insert mainnet consent flag into fake keychain -> succeeds and reserves budget
        fake.state.lock().keys.insert(
            crate::WALLET_MAINNET_CONSENT_CREDENTIAL_URL.to_string(),
            b"enabled".to_vec(),
        );
        let resp = finalize_after_approval(
            &intent,
            &policy,
            &ledger,
            &hmac_key,
            1_700_000_000,
            &async_cx,
        )
        .await
        .expect("approved with consent");
        match resp {
            WalletApprovalResponse::SpendApproved(wire) => {
                assert_eq!(wire.amount_sats, 1_000);
                assert_eq!(wire.remaining_daily_cap_sats, 48_990);
                let receipt = hkask_wallet_core::ApprovalReceipt {
                    approval_id: wire.approval_id,
                    intent_id: wire.intent_id,
                    amount_sats: wire.amount_sats,
                    max_fee_sats: wire.max_fee_sats,
                    destination_hash: wire.destination_hash,
                    network: WalletNetwork::Mainnet,
                    rail: PaymentRail::Cashu,
                    expires_at: wire.expires_at,
                    mac_hex: wire.mac_hex,
                };
                assert!(
                    hkask_wallet_core::verify_approval_receipt(
                        &hmac_key,
                        &receipt,
                        &intent,
                        1_700_000_050
                    )
                    .is_ok()
                );
            }
            other => panic!("expected SpendApproved, got {other:?}"),
        }
    }
}
