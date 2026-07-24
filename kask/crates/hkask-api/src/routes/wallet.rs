//! Wallet API routes — balance, deposits, withdrawals, API keys.
//!
//! All endpoints require `ApiState` with an attached `WalletService`.
//! Routes return 503 Service Unavailable if the wallet service is not configured.
//!
//! Authentication: wallet routes accept both capability tokens (system auth)
//! and API key Bearer tokens (user auth). The `api_key_auth_middleware` runs
//! after the global `auth_middleware`, so either auth method works.

use axum::Extension;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::ApiState;
use crate::middleware::api_key_auth::WalletContext;
use hkask_types::WebID;
use hkask_types::id::WalletId;
use hkask_wallet::{ChainId, PrivacyMode, RJoule};

/// Create wallet router.
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  none
/// post: returns OpenApi`Router<ApiState>` with wallet routes registered
pub fn wallet_router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_balance))
        .routes(routes!(get_fee_estimate))
        .routes(routes!(get_deposit_address))
        .routes(routes!(create_deposit_reference))
        .routes(routes!(get_transactions))
        .routes(routes!(create_key, list_keys, revoke_key))
        .routes(routes!(withdraw))
}

// ── Response types ───────────────────────────────────────────────────────────

/// Wallet balance response — current rJoule balance and fiat equivalents.
///
/// `rjoules` is the canonical energy unit for inference and memory operations.
/// `usdc_equivalent` and `gas_equivalent` are approximate fiat conversions
/// based on the current calibrated gas-per-rjoule rate (P9 homeostatic calibration).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WalletBalanceResponse {
    /// Wallet ID (UUID)
    pub wallet_id: String,
    /// Balance in rJoules — the canonical energy unit for inference operations
    pub rjoules: u64,
    /// Approximate USD Coin equivalent at current calibration rate
    pub usdc_equivalent: f64,
    /// Approximate native gas units equivalent (chain-dependent)
    pub gas_equivalent: u64,
}

/// Withdrawal fee estimate — cost of withdrawing rJoules to a target chain.
///
/// `rjoules` is the fee in the canonical energy unit.
/// `native_units` and `usdc_equivalent` are approximate conversions.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WithdrawalFeeEstimateResponse {
    /// Target blockchain: "hinkal" or "hedera"
    pub chain: String,
    /// Fee in rJoules
    pub rjoules: u64,
    /// Fee in native chain units (e.g., SOL, HBAR)
    pub native_units: f64,
    /// Approximate USD Coin equivalent
    pub usdc_equivalent: f64,
}

/// Deposit address response — a generated deposit address for a specific chain.
///
/// `privacy` is "shielded" (private, zk-protected) or "transparent" (public).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DepositAddressResponse {
    /// On-chain deposit address
    pub address: String,
    /// Blockchain: "hinkal" or "hedera"
    pub chain: String,
    /// Privacy mode: "shielded" or "transparent"
    pub privacy: String,
}

/// Deposit reference request — create a time-limited deposit reference for tracking.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DepositReferenceRequest {
    pub chain: String,
    /// Wallet ID (UUID). Defaults to system wallet if omitted.
    pub wallet_id: Option<String>,
}

/// Deposit reference response — a time-limited reference code for an incoming deposit.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DepositReferenceResponse {
    /// Unique deposit reference code
    pub reference: String,
    /// Blockchain this reference is for
    pub chain: String,
    /// ISO 8601 expiration timestamp
    pub expires_at: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct TransactionQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Wallet ID (UUID). Defaults to system wallet if omitted.
    pub wallet_id: Option<String>,
}

/// Transaction response — a single wallet transaction.
///
/// `rjoules_delta` is positive for deposits, negative for withdrawals.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TransactionResponse {
    /// Change in rJoules (positive = deposit, negative = withdrawal)
    pub rjoules_delta: i64,
    /// Balance after this transaction
    pub balance_after: u64,
    /// ISO 8601 timestamp
    pub timestamp: String,
}

/// Transaction list response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TransactionListResponse {
    /// Wallet transactions, newest first
    pub transactions: Vec<TransactionResponse>,
}

/// Create API key request.
///
/// API keys carry a spending limit in rJoules and an optional expiry.
/// `private` controls shielded (default: true) vs transparent mode.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateKeyRequest {
    /// Spending limit in rJoules
    pub limit_rj: u64,
    /// Key expiry in days from creation (None = no expiry)
    pub expiry_days: Option<u32>,
    /// Privacy: true = shielded, false = transparent (default: true)
    pub private: Option<bool>,
    /// Preferred chain: "hinkal" or "hedera" (default: "hinkal")
    pub chain: Option<String>,
    /// Wallet ID (UUID). Defaults to system wallet if omitted.
    pub wallet_id: Option<String>,
}

/// API key created response.
///
/// **Security:** `private_key_hex` is shown only once. Store it securely.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiKeyCreatedResponse {
    /// Unique key identifier
    pub key_id: String,
    /// Private key in hex (shown only once — store securely)
    pub private_key_hex: String,
    /// Spending limit in rJoules
    pub spending_limit_rj: u64,
    /// ISO 8601 expiry timestamp (None = no expiry)
    pub expires_at: Option<String>,
    /// Privacy mode: "shielded" or "transparent"
    pub privacy_mode: String,
    /// Preferred blockchain for settlement
    pub preferred_chain: Option<String>,
}

/// API key entry — status of an existing API key.
///
/// `status` is "active", "revoked", "exhausted", or "expired".
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiKeyEntry {
    /// Unique key identifier
    pub key_id: String,
    /// Cumulative rJoules spent via this key
    pub spent_rj: u64,
    /// Spending limit in rJoules
    pub limit_rj: u64,
    /// Status: "active", "revoked", "exhausted", or "expired"
    pub status: String,
    /// Privacy mode: "shielded" or "transparent"
    pub privacy_mode: String,
    /// ISO 8601 expiry timestamp (None = no expiry)
    pub expires_at: Option<String>,
    /// Preferred blockchain for settlement
    pub preferred_chain: Option<String>,
}

/// API key list response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiKeyListResponse {
    /// All API keys for the authenticated wallet
    pub keys: Vec<ApiKeyEntry>,
}

/// API key revoked response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiKeyRevokedResponse {
    /// Key ID that was revoked
    pub key_id: String,
    /// Always true on success
    pub revoked: bool,
}

/// Withdraw request — withdraw rJoules to an on-chain address.
///
/// `chain` selects the target blockchain (default: "hinkal").
/// `private` controls shielded vs transparent mode (default: true).
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct WithdrawRequest {
    /// Amount to withdraw in rJoules
    pub amount_rj: u64,
    /// Destination on-chain address
    pub to_address: String,
    /// Target chain: "hinkal" or "hedera" (default: "hinkal")
    pub chain: Option<String>,
    /// Privacy: true = shielded, false = transparent (default: true)
    pub private: Option<bool>,
    /// Wallet ID (UUID). Defaults to system wallet if omitted.
    pub wallet_id: Option<String>,
}

/// Withdrawal response — confirmation of a completed on-chain withdrawal.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct WithdrawalResponse {
    /// On-chain transaction hash
    pub tx_hash: String,
    /// Amount withdrawn in rJoules
    pub amount_rj: u64,
    /// Settlement chain
    pub chain: String,
    /// Privacy mode used: "shielded" or "transparent"
    pub privacy: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn wallet_err(status: StatusCode, msg: &str) -> Response<Body> {
    (status, Json(serde_json::json!({"error": msg}))).into_response()
}

fn get_wallet(state: &ApiState) -> Result<&hkask_services_wallet::WalletService, StatusCode> {
    state
        .wallet_service
        .as_ref()
        .map(|arc| arc.as_ref())
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

fn parse_chain(s: Option<&str>) -> Result<ChainId, &'static str> {
    match s {
        None => Ok(ChainId::Hedera),
        Some("hedera") => Ok(ChainId::Hedera),
        _ => Err("Invalid chain (expected 'hedera')"),
    }
}

fn resolve_privacy_mode(_private: Option<bool>) -> PrivacyMode {
    PrivacyMode::default()
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct FeeEstimateQuery {
    pub chain: Option<String>,
}

/// Estimate current network withdrawal fee using configured price feed.
#[utoipa::path(
    get,
    path = "/api/wallet/fee",
    params(FeeEstimateQuery),
    responses(
        (status = 200, body = WithdrawalFeeEstimateResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn get_fee_estimate(
    State(state): State<ApiState>,
    Query(q): Query<FeeEstimateQuery>,
    wallet_ctx: Option<Extension<WalletContext>>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };

    let chain = match parse_chain(q.chain.as_deref()) {
        Ok(c) => c,
        Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
    };

    let webid = wallet_ctx
        .as_ref()
        .map(|ctx| {
            WebID::from_persona_with_namespace(ctx.wallet_id.to_string().as_bytes(), "wallet-owner")
        })
        .unwrap_or_else(|| WebID::from_persona_with_namespace(b"api-wallet-fee", "wallet-owner"));

    match svc.manager().estimate_withdrawal_fee(&webid, chain).await {
        Ok(fee) => (
            StatusCode::OK,
            Json(WithdrawalFeeEstimateResponse {
                chain: chain.to_string(),
                rjoules: fee.rjoules,
                native_units: fee.native_units,
                usdc_equivalent: fee.usdc_micro as f64 / 1_000_000.0,
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

fn resolve_wallet_id(
    wallet_arg: Option<&str>,
    ctx: Option<&WalletContext>,
) -> Result<WalletId, &'static str> {
    // If request is API-key authenticated, the wallet is fixed by the key.
    if let Some(wc) = ctx {
        if let Some(s) = wallet_arg {
            let requested: WalletId = s
                .parse()
                .map_err(|_| "Invalid wallet_id format (expected UUID)")?;
            if requested != wc.wallet_id {
                return Err("wallet_id does not match authenticated API key wallet");
            }
        }
        return Ok(wc.wallet_id);
    }

    // Unauthenticated/system path: optional explicit wallet_id, else default.
    if let Some(s) = wallet_arg {
        let parsed: WalletId = s
            .parse()
            .map_err(|_| "Invalid wallet_id format (expected UUID)")?;
        return Ok(parsed);
    }

    Ok(WalletId::default())
}

fn key_belongs_to_authenticated_wallet(ctx: &WalletContext, key_wallet_id: WalletId) -> bool {
    key_wallet_id == ctx.wallet_id
}

// ── GET /api/wallet/balance ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct WalletIdQuery {
    /// Wallet ID (UUID). Defaults to system wallet if omitted.
    pub wallet_id: Option<String>,
}

/// Get current wallet balance.
#[utoipa::path(
    get,
    path = "/api/wallet/balance",
    responses(
        (status = 200, body = WalletBalanceResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn get_balance(
    State(state): State<ApiState>,
    Query(q): Query<WalletIdQuery>,
    wallet_ctx: Option<Extension<WalletContext>>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id =
        match resolve_wallet_id(q.wallet_id.as_deref(), wallet_ctx.as_ref().map(|e| &e.0)) {
            Ok(id) => id,
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        };
    match svc.manager().get_balance(wallet_id) {
        Ok(balance) => (
            StatusCode::OK,
            Json(WalletBalanceResponse {
                wallet_id: wallet_id.to_string(),
                rjoules: balance.rjoules,
                usdc_equivalent: balance.usdc_equivalent_micro as f64 / 1_000_000.0,
                gas_equivalent: balance.gas_equivalent,
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── GET /api/wallet/deposit-address ──────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DepositAddressQuery {
    pub chain: Option<String>,
    pub private: Option<bool>,
    /// Wallet ID (UUID). Defaults to system wallet if omitted.
    pub wallet_id: Option<String>,
}

/// Get a deposit address for receiving USDC.
#[utoipa::path(
    get,
    path = "/api/wallet/deposit-address",
    params(DepositAddressQuery),
    responses(
        (status = 200, body = DepositAddressResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn get_deposit_address(
    State(state): State<ApiState>,
    Query(q): Query<DepositAddressQuery>,
    wallet_ctx: Option<Extension<WalletContext>>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id =
        match resolve_wallet_id(q.wallet_id.as_deref(), wallet_ctx.as_ref().map(|e| &e.0)) {
            Ok(id) => id,
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        };
    let chain = match parse_chain(q.chain.as_deref()) {
        Ok(c) => c,
        Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
    };
    let privacy = resolve_privacy_mode(q.private);

    match svc.manager().get_deposit_address(wallet_id, chain, privacy) {
        Ok(addr) => (
            StatusCode::OK,
            Json(DepositAddressResponse {
                address: addr.address,
                chain: chain.to_string(),
                privacy: privacy.to_string(),
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── POST /api/wallet/deposit-reference ──────────────────────────────────────

/// Generate a one-time deposit reference for shielded deposits.
#[utoipa::path(
    post,
    path = "/api/wallet/deposit-reference",
    request_body = DepositReferenceRequest,
    responses(
        (status = 200, body = DepositReferenceResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn create_deposit_reference(
    State(state): State<ApiState>,
    wallet_ctx: Option<Extension<WalletContext>>,
    Json(req): Json<DepositReferenceRequest>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id =
        match resolve_wallet_id(req.wallet_id.as_deref(), wallet_ctx.as_ref().map(|e| &e.0)) {
            Ok(id) => id,
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        };
    let chain = match parse_chain(Some(&req.chain)) {
        Ok(c) => c,
        Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
    };

    let duration = chrono::Duration::hours(24);
    match svc
        .manager()
        .generate_deposit_reference(wallet_id, chain, duration)
    {
        Ok(dep_ref) => (
            StatusCode::OK,
            Json(DepositReferenceResponse {
                reference: dep_ref.reference,
                chain: chain.to_string(),
                expires_at: dep_ref.expires_at.to_rfc3339(),
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── GET /api/wallet/transactions ────────────────────────────────────────────

/// Get paginated transaction history.
#[utoipa::path(
    get,
    path = "/api/wallet/transactions",
    params(TransactionQuery),
    responses(
        (status = 200, body = TransactionListResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn get_transactions(
    State(state): State<ApiState>,
    Query(q): Query<TransactionQuery>,
    wallet_ctx: Option<Extension<WalletContext>>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id =
        match resolve_wallet_id(q.wallet_id.as_deref(), wallet_ctx.as_ref().map(|e| &e.0)) {
            Ok(id) => id,
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        };
    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);

    match svc.manager().get_transactions(wallet_id, limit, offset) {
        Ok(txs) => (
            StatusCode::OK,
            Json(TransactionListResponse {
                transactions: txs
                    .iter()
                    .map(|tx| TransactionResponse {
                        rjoules_delta: tx.rjoules_delta,
                        balance_after: tx.balance_after,
                        timestamp: tx.timestamp.to_rfc3339(),
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── POST /api/wallet/keys ───────────────────────────────────────────────────

/// Create a new API key.
#[utoipa::path(
    post,
    path = "/api/wallet/keys",
    request_body = CreateKeyRequest,
    responses(
        (status = 201, body = ApiKeyCreatedResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn create_key(
    State(state): State<ApiState>,
    wallet_ctx: Option<Extension<WalletContext>>,
    Json(req): Json<CreateKeyRequest>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id =
        match resolve_wallet_id(req.wallet_id.as_deref(), wallet_ctx.as_ref().map(|e| &e.0)) {
            Ok(id) => id,
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        };
    let privacy = resolve_privacy_mode(req.private);
    let preferred_chain = match req.chain.as_deref() {
        Some(c) => match parse_chain(Some(c)) {
            Ok(chain) => Some(chain),
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        },
        None => Some(ChainId::Hedera),
    };

    // Ensure wallet exists
    if let Err(e) = svc.manager().ensure_wallet(wallet_id) {
        return wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
    }

    match svc.issuer().create_key(
        wallet_id,
        RJoule::new(req.limit_rj),
        req.expiry_days,
        privacy,
        preferred_chain,
        vec![],
        String::new(),
        None,
    ) {
        Ok(material) => (
            StatusCode::CREATED,
            Json(ApiKeyCreatedResponse {
                key_id: material.key_id.to_string(),
                private_key_hex: material.private_key_hex,
                spending_limit_rj: material.capability.spending_limit_rj.as_u64(),
                expires_at: material.capability.expiry.map(|e| e.to_rfc3339()),
                privacy_mode: material.capability.privacy_mode.to_string(),
                preferred_chain: material.capability.preferred_chain.map(|c| c.to_string()),
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── GET /api/wallet/keys ────────────────────────────────────────────────────

/// List active API keys.
#[utoipa::path(
    get,
    path = "/api/wallet/keys",
    responses(
        (status = 200, body = ApiKeyListResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn list_keys(
    State(state): State<ApiState>,
    Query(q): Query<WalletIdQuery>,
    wallet_ctx: Option<Extension<WalletContext>>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id =
        match resolve_wallet_id(q.wallet_id.as_deref(), wallet_ctx.as_ref().map(|e| &e.0)) {
            Ok(id) => id,
            Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
        };

    match svc.issuer().list_keys(wallet_id) {
        Ok(keys) => (
            StatusCode::OK,
            Json(ApiKeyListResponse {
                keys: keys
                    .iter()
                    .map(|key| {
                        let status = if key.spent_rj.as_u64() >= key.spending_limit_rj.as_u64() {
                            "exhausted"
                        } else if key.expiry.is_some_and(|exp| chrono::Utc::now() > exp) {
                            "expired"
                        } else {
                            "active"
                        };
                        ApiKeyEntry {
                            key_id: key.key_id.to_string(),
                            spent_rj: key.spent_rj.as_u64(),
                            limit_rj: key.spending_limit_rj.as_u64(),
                            status: status.to_string(),
                            privacy_mode: key.privacy_mode.to_string(),
                            expires_at: key.expiry.map(|e| e.to_rfc3339()),
                            preferred_chain: key.preferred_chain.map(|c| c.to_string()),
                        }
                    })
                    .collect(),
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── DELETE /api/wallet/keys/{key_id} ────────────────────────────────────────

/// Revoke an API key.
#[utoipa::path(
    delete,
    path = "/api/wallet/keys/{key_id}",
    responses(
        (status = 200, body = ApiKeyRevokedResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn revoke_key(
    State(state): State<ApiState>,
    wallet_ctx: Option<Extension<WalletContext>>,
    Path(key_id_str): Path<String>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };

    let key_id = match key_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            return wallet_err(StatusCode::BAD_REQUEST, "Invalid key ID format").into_response();
        }
    };

    if let Some(ctx) = wallet_ctx.as_ref().map(|e| &e.0) {
        match svc.manager().get_api_key(key_id) {
            Ok(Some(cap)) => {
                if !key_belongs_to_authenticated_wallet(ctx, cap.wallet_id) {
                    return wallet_err(
                        StatusCode::FORBIDDEN,
                        "key_id does not belong to authenticated API key wallet",
                    )
                    .into_response();
                }
            }
            Ok(None) => {
                return wallet_err(StatusCode::NOT_FOUND, "API key not found").into_response();
            }
            Err(e) => {
                return wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
                    .into_response();
            }
        }
    }

    match svc.issuer().revoke_key(key_id) {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiKeyRevokedResponse {
                key_id: key_id_str,
                revoked: true,
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── POST /api/wallet/withdraw ───────────────────────────────────────────────

/// Withdraw rJoules as USDC to an external address.
#[utoipa::path(
    post,
    path = "/api/wallet/withdraw",
    request_body = WithdrawRequest,
    responses(
        (status = 200, body = WithdrawalResponse),
        (status = 503, description = "Wallet service not configured")
    )
)]
async fn withdraw(
    State(state): State<ApiState>,
    wallet_ctx: Option<Extension<WalletContext>>,
    Json(req): Json<WithdrawRequest>,
) -> impl IntoResponse {
    let svc = match get_wallet(&state) {
        Ok(s) => s,
        Err(status) => return wallet_err(status, "Wallet service not configured"),
    };
    let wallet_id = match resolve_wallet_id(req.wallet_id.as_deref(), wallet_ctx.as_deref()) {
        Ok(id) => id,
        Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
    };
    let chain = match parse_chain(req.chain.as_deref()) {
        Ok(c) => c,
        Err(msg) => return wallet_err(StatusCode::BAD_REQUEST, msg),
    };
    let privacy = resolve_privacy_mode(req.private);

    // Derive a WebID for the consent check from the wallet context or wallet_id.
    // When authenticated via API key, the wallet_id identifies the owning user.
    let webid = wallet_ctx
        .as_ref()
        .map(|ctx| {
            WebID::from_persona_with_namespace(ctx.wallet_id.to_string().as_bytes(), "wallet-owner")
        })
        .unwrap_or_else(|| {
            WebID::from_persona_with_namespace(wallet_id.to_string().as_bytes(), "wallet-owner")
        });

    match svc
        .withdraw(
            &webid,
            wallet_id,
            RJoule::new(req.amount_rj),
            &req.to_address,
            chain,
            privacy,
        )
        .await
    {
        Ok(tx_hash) => (
            StatusCode::OK,
            Json(WithdrawalResponse {
                tx_hash: tx_hash.0,
                amount_rj: req.amount_rj,
                chain: chain.to_string(),
                privacy: privacy.to_string(),
            }),
        )
            .into_response(),
        Err(e) => wallet_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::id::ApiKeyId;

    fn wallet_ctx(wallet_id: WalletId) -> WalletContext {
        WalletContext {
            wallet_id,
            key_id: ApiKeyId::new(),
            spending_limit_rj: RJoule::new(1000),
            spent_rj: RJoule::ZERO,
        }
    }

    #[test]
    fn resolve_wallet_id_rejects_mismatched_wallet_for_authenticated_request() {
        let authed_wallet = WalletId::new();
        let other_wallet = WalletId::new();
        let ctx = wallet_ctx(authed_wallet);

        let result = resolve_wallet_id(Some(&other_wallet.to_string()), Some(&ctx));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_wallet_id_accepts_matching_wallet_for_authenticated_request() {
        let authed_wallet = WalletId::new();
        let ctx = wallet_ctx(authed_wallet);

        let result = resolve_wallet_id(Some(&authed_wallet.to_string()), Some(&ctx));
        assert_eq!(result.unwrap(), authed_wallet);
    }

    #[test]
    fn key_belongs_to_authenticated_wallet_rejects_mismatched_wallet() {
        let authed_wallet = WalletId::new();
        let key_wallet = WalletId::new();
        let ctx = wallet_ctx(authed_wallet);

        assert!(!key_belongs_to_authenticated_wallet(&ctx, key_wallet));
    }

    #[test]
    fn key_belongs_to_authenticated_wallet_accepts_matching_wallet() {
        let authed_wallet = WalletId::new();
        let ctx = wallet_ctx(authed_wallet);

        assert!(key_belongs_to_authenticated_wallet(&ctx, authed_wallet));
    }

    #[test]
    fn parse_chain_rejects_invalid_value() {
        let result = parse_chain(Some("bitcoin"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_chain_defaults_to_hedera() {
        let result = parse_chain(None).unwrap();
        assert_eq!(result, ChainId::Hedera);
    }

    #[test]
    fn parse_chain_accepts_hedera() {
        let result = parse_chain(Some("hedera")).unwrap();
        assert_eq!(result, ChainId::Hedera);
    }

    #[test]
    fn resolve_privacy_mode_defaults_to_transparent() {
        assert_eq!(resolve_privacy_mode(None), PrivacyMode::Transparent);
    }

    #[test]
    fn resolve_privacy_mode_allows_explicit_transparent_opt_out() {
        assert_eq!(resolve_privacy_mode(Some(false)), PrivacyMode::Transparent);
    }
}
