//! Shared D110 wallet wire contract — what `hkask-mcp-wallet`, `kask_bridge`,
//! and UI surfaces agree on without pulling `secp256k1` or Cashu CDK into
//! `hkask-types`.
//!
//! Mirrors `nostr_wire.rs`:
//! - `WALLET_SERVER_ID` — matches `id` in `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS`.
//! - `WALLET_TOOL_NAMES` — pinned against `hkask-mcp-wallet`'s router by
//!   `tool_surface_pin!`.
//! - Typed IPC wire payloads for spend approval and SQLCipher DB unlock.

use serde::{Deserialize, Serialize};

/// The wallet MCP server's id — MUST match its `id` in
/// `kask_bridge::mcp_servers::BUILT_IN_MCP_SERVERS` and
/// `settings_content::DEFAULT_PRIVATE_THREAD_DISABLED_SERVERS`.
pub const WALLET_SERVER_ID: &str = "wallet";

/// The 10 tools exposed by `hkask-mcp-wallet` (sorted alphabetically).
/// Pinned against the server's router by `tool_surface_pin!` in Phase 2.
pub const WALLET_TOOL_NAMES: &[&str] = &[
    "wallet_balance",
    "wallet_create_invoice",
    "wallet_decode_payment",
    "wallet_pay_invoice",
    "wallet_receive_ecash",
    "wallet_reconcile",
    "wallet_restore_from_seed",
    "wallet_send_ecash",
    "wallet_status",
    "wallet_transactions",
];

/// Wire representation of a single bounded payment intent sent from
/// `hkask-mcp-wallet` to the editor over `HKASK_INFERENCE_SOCKET` for
/// mandatory native GPUI approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletSpendRequestWire {
    /// Unique intent identifier (UUID).
    pub intent_id: String,
    /// Caller-supplied idempotency key for duplicate-payment protection (T8).
    pub idempotency_key: String,
    /// Payment rail: `"cashu"` or `"nwc"`.
    pub rail: String,
    /// Target network: `"regtest"`, `"signet"`, or `"mainnet"`.
    pub network: String,
    /// Exact payment amount in whole satoshis.
    pub amount_sats: u64,
    /// Maximum fee in whole satoshis.
    pub max_fee_sats: u64,
    /// Cashu mint URL (required when `rail == "cashu"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mint_url: Option<String>,
    /// Human-readable destination shown in the native approval modal.
    pub destination_summary: String,
    /// Lowercase 64-char hex SHA-256 of the destination/invoice/recipient.
    pub destination_hash: String,
    /// Optional BOLT11 invoice string (for Lightning/NWC or Cashu melt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bolt11: Option<String>,
    /// Creation timestamp (Unix seconds).
    pub created_at: u64,
}

/// Wire representation of an editor-approved spend returned to
/// `hkask-mcp-wallet` over `HKASK_INFERENCE_SOCKET`.
///
/// For Cashu sends/melts, carries the short-lived HMAC approval receipt.
/// For editor-side NWC execution (Phase 3), also carries the settled preimage
/// and paid fee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletSpendApprovalWire {
    /// Editor-assigned approval ID.
    pub approval_id: String,
    /// Echoed intent ID bound into the receipt MAC.
    pub intent_id: String,
    /// Approved amount in satoshis.
    pub amount_sats: u64,
    /// Approved max fee in satoshis.
    pub max_fee_sats: u64,
    /// Lowercase 64-char hex SHA-256 of the destination.
    pub destination_hash: String,
    /// Approved network (`"regtest"`, `"signet"`, `"mainnet"`).
    pub network: String,
    /// Approved rail (`"cashu"`, `"nwc"`).
    pub rail: String,
    /// Receipt expiry (Unix seconds).
    pub expires_at: u64,
    /// Lowercase hex HMAC-SHA256 receipt tag.
    pub mac_hex: String,
    /// Remaining 24-hour spend cap in satoshis after this reservation.
    pub remaining_daily_cap_sats: u64,
    /// Optional Lightning payment preimage when executed editor-side via NWC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nwc_preimage: Option<String>,
    /// Optional actual fee paid in satoshis when executed editor-side via NWC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nwc_fee_paid_sats: Option<u64>,
    /// Optional Lightning payment hash when executed editor-side via NWC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nwc_payment_hash: Option<String>,
}

/// Non-spending NIP-47 NWC query executed editor-side so `wallet_nwc_uri`
/// never enters `hkask-mcp-wallet` (D110 Phase 3).
///
/// Note: `pay_invoice` is intentionally NOT part of this enum — paying an
/// invoice is only reachable through `InferenceMethod::WalletSpendApprove`
/// behind mandatory native GPUI confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WalletNwcOperationWire {
    /// Query NWC wallet balance (`get_balance`).
    GetBalance,
    /// Create an inbound Lightning receive invoice (`make_invoice`).
    MakeInvoice {
        amount_sats: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// Look up an invoice by payment hash (`lookup_invoice`) for reconciliation.
    LookupInvoice { payment_hash: String },
}

/// Outcome of a non-spending editor-side NWC query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WalletNwcResultWire {
    Balance {
        balance_sats: u64,
    },
    Invoice {
        bolt11: String,
        payment_hash: String,
        amount_sats: u64,
        expiry: u64,
    },
    Lookup {
        payment_hash: String,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preimage: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fee_paid_sats: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_tool_names_has_all_ten_tools_in_sorted_order() {
        assert_eq!(WALLET_TOOL_NAMES.len(), 10);
        let mut sorted = WALLET_TOOL_NAMES.to_vec();
        sorted.sort_unstable();
        assert_eq!(WALLET_TOOL_NAMES, sorted.as_slice());
        assert!(
            WALLET_TOOL_NAMES
                .iter()
                .all(|name| name.starts_with("wallet_")),
            "every wallet tool uses the wallet_ prefix"
        );
    }

    #[test]
    fn wallet_spend_request_and_approval_round_trip() {
        let req = WalletSpendRequestWire {
            intent_id: "intent-1".into(),
            idempotency_key: "idem-1".into(),
            rail: "cashu".into(),
            network: "regtest".into(),
            amount_sats: 1_000,
            max_fee_sats: 10,
            mint_url: Some("http://127.0.0.1:3338".into()),
            destination_summary: "lnbcrt10u...".into(),
            destination_hash: "aa".repeat(32),
            bolt11: None,
            created_at: 1_700_000_000,
        };
        let json = serde_json::to_string(&req).expect("serialize req");
        let back: WalletSpendRequestWire = serde_json::from_str(&json).expect("deserialize req");
        assert_eq!(back, req);

        let appr = WalletSpendApprovalWire {
            approval_id: "appr-1".into(),
            intent_id: "intent-1".into(),
            amount_sats: 1_000,
            max_fee_sats: 10,
            destination_hash: "aa".repeat(32),
            network: "regtest".into(),
            rail: "cashu".into(),
            expires_at: 1_700_000_120,
            mac_hex: "bb".repeat(32),
            remaining_daily_cap_sats: 48_990,
            nwc_preimage: None,
            nwc_fee_paid_sats: None,
            nwc_payment_hash: None,
        };
        let json = serde_json::to_string(&appr).expect("serialize approval");
        let back: WalletSpendApprovalWire =
            serde_json::from_str(&json).expect("deserialize approval");
        assert_eq!(back, appr);
    }
}
