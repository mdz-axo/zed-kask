//! API key authentication middleware — Ed25519 Bearer token verification.
//!
//! Authenticates requests using hKask-issued API keys (Ed25519 keypairs).
//! The private key IS the API key — presented as a hex-encoded Bearer token.
//!
//! # Flow
//! 1. Extract Bearer token from Authorization header
//! 2. Parse as Ed25519 private key (hex-encoded 32 bytes → 64 hex chars)
//! 3. Derive public key from private key
//! 4. Look up ApiKeyCapability by public key in WalletStore
//! 5. Verify: not revoked, not expired, spending limit not exceeded
//! 6. Attach WalletContext to request extensions
//!
//! # OCAP alignment (P4)
//! The API key IS a capability token. The middleware verifies the capability
//! and extracts its attenuation (spending limit). Downstream handlers use
//! the attached wallet context for gas→rJoule billing.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ed25519_dalek::SigningKey;
use hkask_services_wallet::WalletService;
use hkask_storage::WalletStore;
use hkask_types::WebID;
use hkask_types::event::RegulationSink;
use hkask_types::id::{ApiKeyId, WalletId};
use hkask_wallet::{Encumbrance, RJoule};
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Wallet context attached to authenticated requests.
#[derive(Debug, Clone)]
pub struct WalletContext {
    pub wallet_id: WalletId,
    pub key_id: ApiKeyId,
    pub spending_limit_rj: RJoule,
    pub spent_rj: RJoule,
}

/// Middleware state for API key authentication.
#[derive(Clone)]
pub struct ApiKeyAuthService {
    wallet_store: Arc<WalletStore>,
    wallet_service: Arc<WalletService>,
    /// API rate limiter — when present, per-key rate limits are enforced.
    api_meter: Option<Arc<std::sync::RwLock<hkask_regulation::ApiMeter>>>,
    /// Regulation event sink — when present, a `reg.api.request` span is emitted
    /// for every authenticated request after the rate limit check.
    event_sink: Option<Arc<dyn RegulationSink>>,
}

impl ApiKeyAuthService {
    /// Create a new API key auth service backed by a WalletStore and WalletService.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  wallet_store and wallet_service are valid Arcs
    /// post: returns ApiKeyAuthService ready for middleware use
    pub fn new(wallet_store: Arc<WalletStore>, wallet_service: Arc<WalletService>) -> Self {
        Self {
            wallet_store,
            wallet_service,
            api_meter: None,
            event_sink: None,
        }
    }

    /// Attach an API rate limiter. When set, per-key rate limits are enforced
    /// in the middleware after authentication succeeds.
    #[must_use]
    pub fn with_api_meter(
        mut self,
        meter: Arc<std::sync::RwLock<hkask_regulation::ApiMeter>>,
    ) -> Self {
        self.api_meter = Some(meter);
        self
    }

    /// Attach a Regulation event sink. When set, a `reg.api.request` span is emitted
    /// for every authenticated request after the rate limit check.
    #[must_use]
    pub fn with_event_sink(mut self, sink: Arc<dyn RegulationSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    /// Deterministically derive a per-key budget principal.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  key_id is a valid ApiKeyId
    /// post: returns a deterministic WebID unique to that key_id within this namespace
    fn budget_principal_for_key(key_id: ApiKeyId) -> WebID {
        let persona = format!("api-key-budget:{}", key_id);
        WebID::from_persona_with_namespace(persona.as_bytes(), "wallet-api-key-budget")
    }

    /// Authenticate a request using an Ed25519 API key Bearer token.
    ///
    /// Returns the wallet context and the active encumbrance (if any) so
    /// callers can emit metering spans with allocation data.
    fn authenticate(
        &self,
        request: &Request<Body>,
    ) -> Result<(WalletContext, Option<Encumbrance>), ApiKeyAuthError> {
        let header = request
            .headers()
            .get("Authorization")
            .ok_or(ApiKeyAuthError::MissingAuthorization)?;

        let header_str = header
            .to_str()
            .map_err(|_| ApiKeyAuthError::InvalidAuthorizationFormat)?;

        let token = header_str
            .strip_prefix("Bearer ")
            .ok_or(ApiKeyAuthError::InvalidAuthorizationFormat)?;

        // Parse hex-encoded Ed25519 private key (64 hex chars = 32 bytes)
        let private_key_bytes =
            hex::decode(token).map_err(|_| ApiKeyAuthError::InvalidKeyFormat)?;

        if private_key_bytes.len() != 32 {
            return Err(ApiKeyAuthError::InvalidKeyFormat);
        }

        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(&private_key_bytes);

        let signing_key = SigningKey::from_bytes(&key_arr);
        let public_key_bytes = signing_key.verifying_key().to_bytes();

        // Look up the capability by public key.
        // The store query already filters `WHERE revoked_at IS NULL`,
        // so revoked keys are never returned.
        let capability = self
            .wallet_store
            .get_api_key_by_public_key(&public_key_bytes)
            .map_err(|_| ApiKeyAuthError::StoreError)?
            .ok_or(ApiKeyAuthError::UnknownApiKey)?;

        // The DB query already matches by public_key, but a constant-time comparison
        // protects against hypothetical DB corruption or timing side-channels.
        if !bool::from(capability.public_key.as_bytes().ct_eq(&public_key_bytes)) {
            return Err(ApiKeyAuthError::UnknownApiKey);
        }

        // Verify key is not expired
        if let Some(expiry) = capability.expiry
            && chrono::Utc::now() > expiry
        {
            self.wallet_service
                .manager()
                .emit_key_alert(capability.key_id, false, true);
            return Err(ApiKeyAuthError::KeyExpired);
        }

        // Verify spending limit not exceeded
        if capability.spent_rj.as_u64() >= capability.spending_limit_rj.as_u64() {
            self.wallet_service
                .manager()
                .emit_key_alert(capability.key_id, true, false);
            return Err(ApiKeyAuthError::SpendingLimitExceeded);
        }

        // Verify encumbrance: the key must have rJoules allocated
        let encumbrance = self
            .wallet_store
            .get_encumbrance(capability.key_id)
            .map_err(|_| ApiKeyAuthError::StoreError)?;

        match &encumbrance {
            Some(enc) if enc.is_active() && enc.remaining_rj() > 0 => {
                // Key has allocated rJoules — proceed
            }
            Some(enc) if enc.is_active() => {
                // Encumbrance exists but is exhausted
                self.wallet_service
                    .manager()
                    .emit_key_alert(capability.key_id, true, false);
                return Err(ApiKeyAuthError::PaymentRequired(
                    "API key encumbrance exhausted — allocate more rJoules".into(),
                ));
            }
            Some(_) => {
                // Encumbrance exists but is consumed/released
                self.wallet_service
                    .manager()
                    .emit_key_alert(capability.key_id, true, false);
                return Err(ApiKeyAuthError::PaymentRequired(
                    "API key encumbrance is not active — re-encumber rJoules".into(),
                ));
            }
            None => {
                // No encumbrance at all — key has no allocated rJoules
                return Err(ApiKeyAuthError::PaymentRequired(
                    "API key has no rJoules allocated — use `kask wallet encumber` first".into(),
                ));
            }
        }

        // Verify scope: the request path must match the key's declared scope.
        // An empty scope means unrestricted access. Otherwise, at least one
        // scope entry must be a prefix of the request URI path.
        if !capability.scope.is_empty() {
            let path = request.uri().path();
            let scope_matched = capability.scope.iter().any(|s| path.starts_with(s));
            if !scope_matched {
                return Err(ApiKeyAuthError::ScopeViolation {
                    path: path.to_string(),
                    allowed_scopes: capability.scope.clone(),
                });
            }
        }

        Ok((
            WalletContext {
                wallet_id: capability.wallet_id,
                key_id: capability.key_id,
                spending_limit_rj: capability.spending_limit_rj,
                spent_rj: capability.spent_rj,
            },
            encumbrance,
        ))
    }
}

/// Errors returned by API key authentication.
#[derive(Debug, thiserror::Error)]
pub enum ApiKeyAuthError {
    #[error("Missing Authorization header")]
    MissingAuthorization,
    #[error("Invalid Authorization header format (expected: Bearer <hex-key>)")]
    InvalidAuthorizationFormat,
    #[error("Invalid API key format (expected: 64-character hex string)")]
    InvalidKeyFormat,
    #[error("Unknown API key")]
    UnknownApiKey,
    #[error("API key has expired")]
    KeyExpired,
    #[error("API key spending limit exceeded")]
    SpendingLimitExceeded,
    #[error("Payment required: {0}")]
    PaymentRequired(String),
    #[error("Scope violation: {path} not in allowed scopes {allowed_scopes:?}")]
    ScopeViolation {
        path: String,
        allowed_scopes: Vec<String>,
    },
    #[error("Wallet store error")]
    StoreError,
    #[error("Rate limit exceeded: {0}")]
    RateLimited(String),
}

impl IntoResponse for ApiKeyAuthError {
    fn into_response(self) -> Response {
        let (status, message): (StatusCode, String) = match self {
            ApiKeyAuthError::MissingAuthorization => (
                StatusCode::UNAUTHORIZED,
                "Missing Authorization header".into(),
            ),
            ApiKeyAuthError::InvalidAuthorizationFormat => (
                StatusCode::UNAUTHORIZED,
                "Invalid Authorization header format".into(),
            ),
            ApiKeyAuthError::InvalidKeyFormat => {
                (StatusCode::UNAUTHORIZED, "Invalid API key format".into())
            }
            ApiKeyAuthError::UnknownApiKey => (StatusCode::UNAUTHORIZED, "Unknown API key".into()),
            ApiKeyAuthError::KeyExpired => (StatusCode::FORBIDDEN, "API key has expired".into()),
            ApiKeyAuthError::SpendingLimitExceeded => (
                StatusCode::FORBIDDEN,
                "API key spending limit exceeded".into(),
            ),
            ApiKeyAuthError::PaymentRequired(msg) => (StatusCode::PAYMENT_REQUIRED, msg),
            ApiKeyAuthError::ScopeViolation {
                path,
                allowed_scopes,
            } => (
                StatusCode::FORBIDDEN,
                format!(
                    "Scope violation: '{}' not in allowed scopes {:?}",
                    path, allowed_scopes
                ),
            ),
            ApiKeyAuthError::StoreError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal authentication error".into(),
            ),
            ApiKeyAuthError::RateLimited(msg) => (StatusCode::TOO_MANY_REQUESTS, msg),
        };
        (status, message).into_response()
    }
}

/// Axum middleware function for API key authentication.
///
/// Pass-through: if no `Authorization: Bearer` header is present, the request
/// proceeds without API key auth (capability token auth still applies from the
/// global `auth_middleware`). This allows wallet and non-wallet routes to coexist.
///
/// When a valid Bearer token is present, registers a wallet-backed energy budget
/// in the Regulation so that subsequent tool/inference calls consume rJoules from the
/// key's encumbrance.
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  auth is a valid ApiKeyAuthService
/// post: if no Bearer header → pass-through (next.run)
/// post: if valid Bearer token → WalletContext injected, budget registered
/// post: if invalid Bearer token → Err(ApiKeyAuthError)
pub async fn api_key_auth_middleware(
    State(auth): State<Arc<ApiKeyAuthService>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiKeyAuthError> {
    // Pass-through: if no Bearer token, skip API key auth.
    // Capability token auth is handled by the global auth_middleware.
    let has_bearer = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.starts_with("Bearer "))
        .unwrap_or(false);

    if !has_bearer {
        return Ok(next.run(request).await);
    }

    let (ctx, encumbrance) = auth.authenticate(&request)?;

    // ── Rate limit check (if API meter is configured) ──
    let rate_limit_status = if let Some(ref meter) = auth.api_meter {
        let path = request.uri().path();
        let weight = hkask_regulation::api_metering::endpoint_weight(path).0 as u64;
        let estimated_tokens = weight * 100;
        let status = meter
            .write()
            .map(|mut m| m.check_and_record(ctx.key_id, estimated_tokens))
            .unwrap_or(hkask_regulation::api_metering::RateLimitStatus::Ok);
        if status != hkask_regulation::api_metering::RateLimitStatus::Ok {
            return Err(ApiKeyAuthError::RateLimited(format!(
                "{} — retry later",
                status.as_str()
            )));
        }
        status
    } else {
        hkask_regulation::api_metering::RateLimitStatus::Ok
    };

    // ── Emit reg.api.request span (if event sink is configured) ──
    if let Some(ref sink) = auth.event_sink {
        let span = hkask_regulation::api_metering::ApiRequestSpan::new(
            &ctx.key_id.to_string(),
            request.uri().path(),
            true, // scope was verified by authenticate() above
            0,    // gas consumed is settled downstream by the governed McpRuntime
            encumbrance.as_ref(),
            rate_limit_status,
        );
        span.emit_to(sink.as_ref(), &WebID::default());
    }

    // Register wallet-backed budget so the governed McpRuntime
    // debits from this key's encumbrance during the request.
    let budget_principal = ApiKeyAuthService::budget_principal_for_key(ctx.key_id);
    let _ = auth
        .wallet_service
        .register_wallet_budget_for_key(
            budget_principal,
            ctx.wallet_id,
            ctx.key_id,
            ctx.spending_limit_rj,
        )
        .await;

    // Attach wallet context to request extensions for downstream handlers
    let mut request = request;
    request.extensions_mut().insert(ctx);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_services_wallet::WalletService;
    use hkask_storage::WalletStore;
    use hkask_types::crypto::Ed25519PublicKey;
    use hkask_wallet::{ApiKeyCapability, ChainId, PrivacyMode, TransactionType, WalletConfig};
    use hkask_wallet::{ApiKeyIssuer, StaticPriceFeed, WalletManager};

    fn make_auth_service_with_key(spent_rj: u64, limit_rj: u64) -> (ApiKeyAuthService, String) {
        // SAFETY: test-only setup for deterministic wallet manager construction.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx",
            );
        }

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );",
            )
            .unwrap();
        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                Default::default(),
                Arc::new(StaticPriceFeed::new()),
            )
            .unwrap(),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).unwrap());
        let wallet_service = Arc::new(WalletService::new(manager, issuer));

        let wallet_id = WalletId::new();
        store.ensure_wallet(wallet_id).unwrap();
        let key_id = ApiKeyId::new();
        let private_key = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey(public_key),
            spending_limit_rj: RJoule::new(limit_rj),
            spent_rj: RJoule::new(spent_rj),
            scope: vec![],
            purpose: "middleware test key".into(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();

        let auth = ApiKeyAuthService::new(store, wallet_service);
        (auth, hex::encode(private_key))
    }

    #[test]
    fn budget_principal_is_deterministic_for_same_key() {
        let key_id = ApiKeyId::new();
        let p1 = ApiKeyAuthService::budget_principal_for_key(key_id);
        let p2 = ApiKeyAuthService::budget_principal_for_key(key_id);
        assert_eq!(p1, p2);
    }

    #[test]
    fn budget_principal_is_distinct_across_keys() {
        let k1 = ApiKeyId::new();
        let k2 = ApiKeyId::new();
        let p1 = ApiKeyAuthService::budget_principal_for_key(k1);
        let p2 = ApiKeyAuthService::budget_principal_for_key(k2);
        assert_ne!(p1, p2);
    }

    #[test]
    fn authenticate_rejects_exhausted_key() {
        let (auth, token) = make_auth_service_with_key(1_000, 1_000);
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::SpendingLimitExceeded),
            "expected SpendingLimitExceeded, got {err:?}"
        );
    }

    #[test]
    fn authenticate_rejects_consumed_encumbrance() {
        // SAFETY: test-only setup for deterministic wallet manager construction.
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx",
            );
        }

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );",
            )
            .unwrap();
        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                Default::default(),
                Arc::new(StaticPriceFeed::new()),
            )
            .unwrap(),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).unwrap());
        let wallet_service = Arc::new(WalletService::new(manager, issuer));

        let wallet_id = WalletId::new();
        store.ensure_wallet(wallet_id).unwrap();
        store
            .credit_rjoules(
                wallet_id,
                RJoule::new(2_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let key_id = ApiKeyId::new();
        let private_key = [99u8; 32];
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey(public_key),
            spending_limit_rj: RJoule::new(1_000),
            spent_rj: RJoule::new(100),
            scope: vec![],
            purpose: "middleware consumed encumbrance test key".into(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();

        store
            .encumber_rjoules(wallet_id, key_id, RJoule::new(300))
            .unwrap();
        store.consume_encumbrance(key_id, RJoule::new(300)).unwrap();

        let auth = ApiKeyAuthService::new(store, wallet_service);
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .header(
                "Authorization",
                format!("Bearer {}", hex::encode(private_key)),
            )
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::PaymentRequired(ref msg) if msg.contains("not active")),
            "expected PaymentRequired for inactive/consumed encumbrance, got {err:?}"
        );
    }

    #[test]
    fn authenticate_valid_key_succeeds() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );",
            )
            .unwrap();
        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                Default::default(),
                Arc::new(StaticPriceFeed::new()),
            )
            .unwrap(),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).unwrap());
        let wallet_service = Arc::new(WalletService::new(manager, issuer));

        let wallet_id = WalletId::new();
        store.ensure_wallet(wallet_id).unwrap();
        store
            .credit_rjoules(
                wallet_id,
                RJoule::new(5_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let key_id = ApiKeyId::new();
        let private_key = [77u8; 32];
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey(public_key),
            spending_limit_rj: RJoule::new(1_000),
            spent_rj: RJoule::ZERO,
            scope: vec![],
            purpose: "valid key test".into(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();
        // Encumber rJoules so the key has spendable balance
        store
            .encumber_rjoules(wallet_id, key_id, RJoule::new(500))
            .unwrap();

        let auth = ApiKeyAuthService::new(store, wallet_service);
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .header(
                "Authorization",
                format!("Bearer {}", hex::encode(private_key)),
            )
            .body(Body::empty())
            .unwrap();

        let (ctx, encumbrance) = auth.authenticate(&request).unwrap();
        assert_eq!(ctx.wallet_id, wallet_id);
        assert_eq!(ctx.key_id, key_id);
        assert_eq!(ctx.spending_limit_rj.as_u64(), 1_000);
        assert_eq!(ctx.spent_rj.as_u64(), 0);
        // Encumbrance is returned for metering span emission
        assert!(encumbrance.is_some());
        assert_eq!(encumbrance.unwrap().remaining_rj(), 500);
    }

    #[test]
    fn authenticate_rejects_expired_key() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );",
            )
            .unwrap();
        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                Default::default(),
                Arc::new(StaticPriceFeed::new()),
            )
            .unwrap(),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).unwrap());
        let wallet_service = Arc::new(WalletService::new(manager, issuer));

        let wallet_id = WalletId::new();
        store.ensure_wallet(wallet_id).unwrap();

        let key_id = ApiKeyId::new();
        let private_key = [88u8; 32];
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey(public_key),
            spending_limit_rj: RJoule::new(1_000),
            spent_rj: RJoule::ZERO,
            scope: vec![],
            purpose: "expired key test".into(),
            rate_limit: None,
            expiry: Some(chrono::Utc::now() - chrono::Duration::days(1)), // yesterday
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();

        let auth = ApiKeyAuthService::new(store, wallet_service);
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .header(
                "Authorization",
                format!("Bearer {}", hex::encode(private_key)),
            )
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::KeyExpired),
            "expected KeyExpired, got {err:?}"
        );
    }

    #[test]
    fn authenticate_rejects_revoked_key() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );",
            )
            .unwrap();
        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                Default::default(),
                Arc::new(StaticPriceFeed::new()),
            )
            .unwrap(),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).unwrap());
        let wallet_service = Arc::new(WalletService::new(manager, issuer));

        let wallet_id = WalletId::new();
        store.ensure_wallet(wallet_id).unwrap();

        let key_id = ApiKeyId::new();
        let private_key = [99u8; 32];
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey(public_key),
            spending_limit_rj: RJoule::new(1_000),
            spent_rj: RJoule::ZERO,
            scope: vec![],
            purpose: "revoked key test".into(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();
        store.revoke_api_key(key_id).unwrap();

        let auth = ApiKeyAuthService::new(store, wallet_service);
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .header(
                "Authorization",
                format!("Bearer {}", hex::encode(private_key)),
            )
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::UnknownApiKey),
            "expected UnknownApiKey for revoked key, got {err:?}"
        );
    }

    #[test]
    fn authenticate_rejects_missing_authorization() {
        let (auth, _token) = make_auth_service_with_key(0, 1_000);
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::MissingAuthorization),
            "expected MissingAuthorization, got {err:?}"
        );
    }

    #[test]
    fn authenticate_rejects_invalid_token_format() {
        let (auth, _token) = make_auth_service_with_key(0, 1_000);
        // Not hex-encoded
        let request = Request::builder()
            .uri("/api/wallet/balance")
            .header("Authorization", "Bearer not-hex-data!!!")
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::InvalidKeyFormat),
            "expected InvalidKeyFormat, got {err:?}"
        );
    }

    #[test]
    fn authenticate_rejects_scope_violation() {
        // SAFETY: test-only
        unsafe {
            std::env::set_var(
                "HKASK_MASTER_KEY",
                "xXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXxXx",
            );
        }
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(WalletStore::from_driver(driver));
        store
            .driver()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS wallet_balances (
                wallet_id TEXT PRIMARY KEY NOT NULL,
                balance_rj INTEGER NOT NULL DEFAULT 0,
                usdc_equivalent_micro INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_id TEXT NOT NULL,
                tx_type TEXT NOT NULL,
                tx_subtype TEXT,
                chain TEXT,
                on_chain_tx_hash TEXT,
                amount_rj INTEGER NOT NULL,
                balance_after_rj INTEGER NOT NULL,
                key_id TEXT,
                tool_name TEXT,
                gas_units INTEGER,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS api_keys (
                key_id TEXT PRIMARY KEY,
                wallet_id TEXT NOT NULL,
                public_key BLOB NOT NULL,
                spending_limit_rj INTEGER NOT NULL,
                spent_rj INTEGER NOT NULL DEFAULT 0,
                scope TEXT NOT NULL,
                purpose TEXT,
                rate_limit_json TEXT,
                privacy_mode TEXT NOT NULL,
                preferred_chain TEXT,
                expires_at TEXT,
                issued_at TEXT NOT NULL,
                revoked_at TEXT
            );
            CREATE TABLE IF NOT EXISTS encumbrances (
                key_id TEXT NOT NULL,
                wallet_id TEXT NOT NULL,
                amount_rj INTEGER NOT NULL,
                consumed_rj INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                released_at TEXT
            );",
            )
            .unwrap();
        let manager = Arc::new(
            WalletManager::build(
                WalletConfig::default(),
                Arc::clone(&store),
                Default::default(),
                Arc::new(StaticPriceFeed::new()),
            )
            .unwrap(),
        );
        let issuer = Arc::new(ApiKeyIssuer::new(Arc::clone(&store)).unwrap());
        let wallet_service = Arc::new(WalletService::new(manager, issuer));

        let wallet_id = WalletId::new();
        store.ensure_wallet(wallet_id).unwrap();
        store
            .credit_rjoules(
                wallet_id,
                RJoule::new(5_000),
                TransactionType::Deposit {
                    chain: ChainId::default(),
                    privacy: PrivacyMode::default(),
                    tx_hash: "test_credit".to_string(),
                    amount_usdc_micro: 0,
                },
            )
            .unwrap();

        let key_id = ApiKeyId::new();
        let private_key = [55u8; 32];
        let signing_key = SigningKey::from_bytes(&private_key);
        let public_key = signing_key.verifying_key().to_bytes();

        let capability = ApiKeyCapability {
            wallet_id,
            key_id,
            public_key: Ed25519PublicKey(public_key),
            spending_limit_rj: RJoule::new(1_000),
            spent_rj: RJoule::ZERO,
            scope: vec!["/api/specs".to_string()], // only specs endpoint
            purpose: "scope test key".into(),
            rate_limit: None,
            expiry: None,
            issued_at: chrono::Utc::now(),
            privacy_mode: PrivacyMode::Transparent,
            preferred_chain: None,
        };
        store.store_api_key(&capability).unwrap();
        store
            .encumber_rjoules(wallet_id, key_id, RJoule::new(500))
            .unwrap();

        let auth = ApiKeyAuthService::new(store, wallet_service);
        // Request a path outside the key's scope
        let request = Request::builder()
            .uri("/api/wallet/balance") // not in "/api/specs" scope
            .header(
                "Authorization",
                format!("Bearer {}", hex::encode(private_key)),
            )
            .body(Body::empty())
            .unwrap();

        let err = auth.authenticate(&request).unwrap_err();
        assert!(
            matches!(err, ApiKeyAuthError::ScopeViolation { .. }),
            "expected ScopeViolation, got {err:?}"
        );
    }
}
