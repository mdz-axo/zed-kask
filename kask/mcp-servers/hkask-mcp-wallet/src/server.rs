//! MCP tool router for `hkask-mcp-wallet` (all 10 D110 tools).

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::engine::{WalletEngine, WalletError};

hkask_mcp_server::mcp_server!(
    pub struct WalletServer {
        pub engine: Option<Arc<WalletEngine>>,
        pub locked_reason: Option<String>,
    } = crate::ontology::annotate
);

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletStatusParams {}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletBalanceParams {}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletTransactionsParams {
    /// Optional rail filter: `"cashu"` or `"nwc"`.
    #[serde(default)]
    pub rail: Option<String>,
    /// Optional status filter (e.g. `"settled"`, `"pending"`, `"reconciling"`, `"declined"`).
    #[serde(default)]
    pub status: Option<String>,
    /// Optional Unix timestamp lower bound.
    #[serde(default)]
    pub since: Option<u64>,
    /// Maximum rows to return (default 50, capped at 500).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletDecodePaymentParams {
    /// BOLT11 invoice, Lightning Address (`user@domain`), or Cashu token (`cashuA`/`cashuB`).
    pub input: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletReconcileParams {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletCreateInvoiceParams {
    /// Receive rail: `"cashu"` or `"nwc"`.
    pub rail: String,
    /// Exact receive amount in whole satoshis.
    pub amount_sats: u64,
    /// Optional allowlisted Cashu mint URL (defaults to `kask.wallet.default_mint`).
    #[serde(default)]
    pub mint_url: Option<String>,
    /// Optional invoice memo/description.
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletReceiveEcashParams {
    /// Encoded Cashu token (`cashuA...` or `cashuB...`).
    pub token: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletPayInvoiceParams {
    /// Caller-supplied idempotency key preventing duplicate payments on retry (T8).
    pub idempotency_key: String,
    /// BOLT11 invoice to pay.
    pub bolt11: String,
    /// Payment rail: `"cashu"` or `"nwc"`.
    pub rail: String,
    /// Optional allowlisted Cashu mint URL when `rail == "cashu"`.
    #[serde(default)]
    pub mint_url: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletSendEcashParams {
    /// Caller-supplied idempotency key preventing duplicate sends on retry (T8).
    pub idempotency_key: String,
    /// Exact amount to send in whole satoshis.
    pub amount_sats: u64,
    /// Optional allowlisted Cashu mint URL (defaults to `kask.wallet.default_mint`).
    #[serde(default)]
    pub mint_url: Option<String>,
    /// Optional compressed secp256k1 hex public key for NUT-11 P2PK lock.
    #[serde(default)]
    pub p2pk_pubkey: Option<String>,
    /// Optional human-readable token memo.
    #[serde(default)]
    pub memo: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalletRestoreFromSeedParams {
    /// Optional allowlisted mint URL to restore; omit to restore all allowlisted mints.
    #[serde(default)]
    pub mint_url: Option<String>,
}

fn map_wallet_err(err: WalletError) -> McpToolError {
    match err {
        WalletError::InvalidArgument(msg) => McpToolError::invalid_argument(msg),
        WalletError::PolicyDenied(msg) | WalletError::Duplicate(msg) => {
            McpToolError::invalid_argument(msg)
        }
        WalletError::Locked(_) | WalletError::Approval(_) | WalletError::NwcNotYetImplemented => {
            McpToolError::new(hkask_types::McpErrorKind::Unavailable, err.to_string())
        }
        WalletError::Cashu(msg) => McpToolError::internal(msg),
        WalletError::Ledger(e) => McpToolError::internal(e.to_string()),
    }
}

impl WalletServer {
    fn require_engine(&self) -> Result<&Arc<WalletEngine>, McpToolError> {
        self.engine.as_ref().ok_or_else(|| {
            McpToolError::new(
                hkask_types::McpErrorKind::Unavailable,
                self.locked_reason.clone().unwrap_or_else(|| {
                    "wallet is locked; configure in Settings > Kask > Wallet".to_string()
                }),
            )
        })
    }
}

#[tool_router(router = wallet_router, vis = "pub")]
impl WalletServer {
    #[tool(
        description = "Report wallet network, mainnet gate, configured mints/NWC label, spending caps, DB unlock state, and count of pending items needing reconciliation."
    )]
    pub async fn wallet_status(
        &self,
        Parameters(_params): Parameters<WalletStatusParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_status", async {
            let report = match &self.engine {
                Some(engine) => engine.status().map_err(map_wallet_err)?,
                None => {
                    let cfg = crate::config::WalletConfig::from_env();
                    crate::engine::StatusReport {
                        network: cfg.network.as_str().to_string(),
                        mainnet_enabled: cfg.mainnet_enabled,
                        db_unlocked: false,
                        per_tx_cap_sats: cfg.per_tx_cap_sats,
                        daily_cap_sats: cfg.daily_cap_sats,
                        max_fee_sats: cfg.max_fee_sats,
                        allowed_mints: cfg.allowed_mints,
                        default_mint: cfg.default_mint,
                        nwc_label: cfg.nwc_label,
                        pending_reconciliation_count: 0,
                    }
                }
            };
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Report per-mint Cashu balances (confirmed, pending, reserved) and NWC Lightning balance when configured."
    )]
    pub async fn wallet_balance(
        &self,
        Parameters(_params): Parameters<WalletBalanceParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_balance", async {
            let engine = self.require_engine()?;
            let report = engine.balance().await.map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "List paginated local wallet audit ledger entries filtered by rail, status, or timestamp. Never emits raw tokens or secrets."
    )]
    pub async fn wallet_transactions(
        &self,
        Parameters(params): Parameters<WalletTransactionsParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_transactions", async {
            let engine = self.require_engine()?;
            let rows = engine
                .transactions(
                    params.rail.as_deref(),
                    params.status.as_deref(),
                    params.since,
                    params.limit,
                    params.offset,
                )
                .map_err(map_wallet_err)?;
            Ok(json!({ "transactions": rows, "count": rows.len() }))
        })
        .await
    }

    #[tool(
        description = "Decode a BOLT11 invoice, Lightning Address, or Cashu token locally without contacting any network service."
    )]
    pub async fn wallet_decode_payment(
        &self,
        Parameters(params): Parameters<WalletDecodePaymentParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_decode_payment", async {
            let report =
                crate::engine::decode_payment_input(&params.input).map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Run NUT-07 proof state checks, incomplete saga recovery, and paid mint-quote issuance across allowlisted mints."
    )]
    pub async fn wallet_reconcile(
        &self,
        Parameters(_params): Parameters<WalletReconcileParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_reconcile", async {
            let engine = self.require_engine()?;
            let report = engine.reconcile().await.map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Create a BOLT11 receive invoice via Cashu NUT-04 mint quote (allowlisted mint) or NWC make_invoice."
    )]
    pub async fn wallet_create_invoice(
        &self,
        Parameters(params): Parameters<WalletCreateInvoiceParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_create_invoice", async {
            let engine = self.require_engine()?;
            let report = engine
                .create_invoice(
                    &params.rail,
                    params.amount_sats,
                    params.mint_url.as_deref(),
                    params.description,
                )
                .await
                .map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Validate an encoded Cashu token against the mint allowlist, verify DLEQ (NUT-12), swap proofs (including NUT-11 P2PK redemption), and persist fresh proofs."
    )]
    pub async fn wallet_receive_ecash(
        &self,
        Parameters(params): Parameters<WalletReceiveEcashParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_receive_ecash", async {
            let engine = self.require_engine()?;
            let report = engine
                .receive_ecash(&params.token)
                .await
                .map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Pay a BOLT11 invoice via Cashu NUT-05 melt (with NUT-08 fee change) or NWC. Requires mandatory native editor approval and an idempotency_key."
    )]
    pub async fn wallet_pay_invoice(
        &self,
        Parameters(params): Parameters<WalletPayInvoiceParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_pay_invoice", async {
            let engine = self.require_engine()?;
            let report = engine
                .pay_invoice(
                    &params.idempotency_key,
                    &params.bolt11,
                    &params.rail,
                    params.mint_url.as_deref(),
                )
                .await
                .map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Create a Cashu ecash token (optionally P2PK-locked via NUT-11). Requires mandatory native editor approval and an idempotency_key."
    )]
    pub async fn wallet_send_ecash(
        &self,
        Parameters(params): Parameters<WalletSendEcashParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_send_ecash", async {
            let engine = self.require_engine()?;
            let report = engine
                .send_ecash(
                    &params.idempotency_key,
                    params.amount_sats,
                    params.mint_url.as_deref(),
                    params.p2pk_pubkey.as_deref(),
                    params.memo,
                )
                .await
                .map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }

    #[tool(
        description = "Run NUT-09 / NUT-13 deterministic seed restore across allowlisted mints after explicit native operator confirmation."
    )]
    pub async fn wallet_restore_from_seed(
        &self,
        Parameters(params): Parameters<WalletRestoreFromSeedParams>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "wallet_restore_from_seed", async {
            let engine = self.require_engine()?;
            let report = engine
                .restore_from_seed(params.mint_url.as_deref())
                .await
                .map_err(map_wallet_err)?;
            serde_json::to_value(report).map_err(|e| McpToolError::internal(e.to_string()))
        })
        .await
    }
}

#[tool_handler(router = Self::wallet_router())]
impl rmcp::ServerHandler for WalletServer {
    fn get_info(&self) -> rmcp::model::ServerConfig {
        rmcp::model::ServerConfig::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new(
            "hkask-mcp-wallet",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "All spending operations (wallet_pay_invoice, wallet_send_ecash, wallet_restore_from_seed) require mandatory native operator confirmation in Zed. Use wallet_decode_payment first to inspect invoices or tokens before proposing any spend.",
        )
    }
}
