//! API authentication middleware — Capability token verification
//!
//! Extracts `Authorization: Bearer <token>` from incoming requests,
//! verifies the Ed25519 signature using the token's embedded public key,
//! checks expiry, and attaches the validated `DelegationToken` and `WebID`
//! to request extensions.
//!
//! Routes that don't require auth (health checks, model listing) are
//! excluded from authentication.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use hkask_capability::{CapabilityChecker, DelegationToken, SYSTEM_MAX_ATTENUATION};
use hkask_types::Ed25519PublicKey;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

/// Routes that bypass authentication (health checks, model listing).
const PUBLIC_PATHS: &[&str] = &[
    "/api/regulation/health",
    "/api/models",
    "/api/models/search",
    "/api/v1/auth",
    "/api/v1/terminal",
    "/terminal",
    "/",
];

/// Service for capability token verification and revocation tracking.
#[derive(Debug, Clone)]
pub struct AuthService {
    /// Revoked capability token IDs (sync RwLock for use in sync verify_token)
    revoked_tokens: Arc<RwLock<HashSet<String>>>,
    /// Trusted issuer public keys. A bearer `DelegationToken` is only accepted if
    /// its embedded public key is one of these (the system OCAP authority derived
    /// from the master key). Empty ⇒ reject all bearer tokens (fail closed).
    trusted_roots: Arc<Vec<Ed25519PublicKey>>,
}

impl AuthService {
    /// Create an `AuthService` from a ServiceConfig.
    ///
    /// Resolves the system OCAP authority public key from the keystore (derived
    /// from the master key) and trusts it as the sole bearer-token issuer. If the
    /// master key is unavailable, the trusted-root set is empty and **all** bearer
    /// `DelegationToken`s are rejected — sovereignty fails closed (P2 / P4).
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  _config is a valid ServiceConfig
    /// post: returns AuthService with empty revocation set and the system OCAP
    ///       public key as its sole trusted root (or none if unavailable)
    pub fn from_config(_config: &hkask_services_core::ServiceConfig) -> Self {
        let trusted_roots = match hkask_keystore::keychain::get_or_create_ocap_secret() {
            Ok(secret) => {
                let sk = hkask_capability::derive_signing_key(secret.as_slice());
                vec![Ed25519PublicKey(sk.verifying_key().to_bytes())]
            }
            Err(e) => {
                tracing::error!(
                    target: "hkask.api",
                    error = %e,
                    "OCAP authority key unavailable — bearer DelegationToken auth fails closed (all rejected)"
                );
                Vec::new()
            }
        };
        Self {
            revoked_tokens: Arc::new(RwLock::new(HashSet::new())),
            trusted_roots: Arc::new(trusted_roots),
        }
    }

    /// Construct an `AuthService` with an explicit trusted-root set (test/wiring).
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// post: returns AuthService trusting exactly `trusted_roots`
    pub fn with_trusted_roots(trusted_roots: Vec<Ed25519PublicKey>) -> Self {
        Self {
            revoked_tokens: Arc::new(RwLock::new(HashSet::new())),
            trusted_roots: Arc::new(trusted_roots),
        }
    }

    /// Revoke a capability token by its ID.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  token_id is a valid token identifier string
    /// post: token_id is added to the revocation set (best-effort, RwLock write may fail silently)
    pub fn revoke_token(&self, token_id: String) {
        if let Ok(mut revoked) = self.revoked_tokens.write() {
            revoked.insert(token_id);
        }
    }

    /// Check whether a capability token has been revoked.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  token_id is a valid token identifier string
    /// post: returns true iff token_id is in the revocation set
    /// post: returns false if RwLock read fails (conservative: assume not revoked)
    pub fn is_token_revoked(&self, token_id: &str) -> bool {
        self.revoked_tokens
            .read()
            .map(|set| set.contains(token_id))
            .unwrap_or(false)
    }

    /// Verify a capability token cryptographically and check expiry.
    ///
    /// expect: "API endpoints enforce OCAP boundaries"
    /// pre:  token is a valid DelegationToken
    /// post: returns TokenVerification::Invalid if signature or attenuation chain fails
    /// post: returns TokenVerification::Expired if token is past its expiry
    /// post: returns TokenVerification::Revoked if token_id is in revocation set
    /// post: returns TokenVerification::Valid iff all checks pass
    pub fn verify_token(&self, token: &DelegationToken) -> TokenVerification {
        // 1. Verify Ed25519 signature AND that the issuer is a trusted root.
        //    A self-signed token whose key is not a trusted root is a forgery:
        //    anyone can mint one with a fresh keypair. Anchoring trust in the
        //    system OCAP authority closes that bypass (C1). Fails closed when no
        //    root is configured (empty trusted_roots ⇒ reject all).
        let checker = CapabilityChecker::with_trusted_roots((*self.trusted_roots).clone());
        if !checker.verify(token) {
            return TokenVerification::Invalid;
        }

        // 2. Check expiry
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        if token.is_expired(current_time) {
            return TokenVerification::Expired;
        }

        // 3. Verify attenuation chain (root nonce matches, level valid)
        if !token.verify_attenuation_chain(token.root_context_nonce(), SYSTEM_MAX_ATTENUATION) {
            return TokenVerification::Invalid;
        }

        // 4. Check revocation
        if self.is_token_revoked(&token.id) {
            return TokenVerification::Revoked;
        }

        TokenVerification::Valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_capability::{DelegationAction, DelegationResource, derive_signing_key};
    use hkask_types::WebID;

    fn token_signed_by(secret: &[u8]) -> DelegationToken {
        let sk = derive_signing_key(secret);
        let who = WebID::from_persona(b"holder");
        DelegationToken::new(
            DelegationResource::Tool,
            "tool".into(),
            DelegationAction::Execute,
            who,
            who,
            &sk,
        )
    }

    /// \[C1 regression\] The bearer path must reject a token minted with an
    /// attacker-controlled keypair, even though its self-signature is valid.
    #[test]
    fn verify_token_rejects_forged_issuer() {
        let system_sk = derive_signing_key(b"system-ocap-root");
        let svc = AuthService::with_trusted_roots(vec![Ed25519PublicKey(
            system_sk.verifying_key().to_bytes(),
        )]);
        let forged = token_signed_by(b"attacker-secret");
        assert!(forged.verify_cryptographic(), "self-signature is valid");
        assert_eq!(svc.verify_token(&forged), TokenVerification::Invalid);
    }

    /// A token signed by the trusted system authority is accepted.
    #[test]
    fn verify_token_accepts_trusted_issuer() {
        let system_sk = derive_signing_key(b"system-ocap-root");
        let svc = AuthService::with_trusted_roots(vec![Ed25519PublicKey(
            system_sk.verifying_key().to_bytes(),
        )]);
        let legit = token_signed_by(b"system-ocap-root");
        assert_eq!(svc.verify_token(&legit), TokenVerification::Valid);
    }

    /// \[C1 regression\] With no trusted root configured, all bearer tokens are
    /// rejected (fail closed).
    #[test]
    fn verify_token_fails_closed_without_root() {
        let svc = AuthService::with_trusted_roots(vec![]);
        let any = token_signed_by(b"whatever");
        assert_eq!(svc.verify_token(&any), TokenVerification::Invalid);
    }
}

/// Result of token verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenVerification {
    /// Token is valid and not expired.
    Valid,
    /// Signature is invalid or attenuation chain is broken.
    Invalid,
    /// Signature is valid but token has expired.
    Expired,
    /// Token has been revoked.
    Revoked,
}

/// Extracted auth context attached to validated requests.
///
/// This is a type alias for the domain-level `AuthContext` in `hkask_types`,
/// which carries the verified capability token and the caller's WebID.
/// Both API (middleware) and CLI (keystore) paths produce the same type.
pub type AuthContext = hkask_capability::AuthContext;

/// Build a response safely, falling back to a minimal status-only response if
/// the body cannot be constructed (e.g., header size overflow).
///
/// This avoids `.unwrap()` panics on `Response::builder().body(...)` which can
/// fail in edge cases (e.g., very large status codes or header values).
fn build_response(status: StatusCode, body: Body) -> Response {
    Response::builder()
        .status(status)
        .body(body)
        .unwrap_or_else(|_| {
            // Minimal fallback: status line only, no body
            Response::new(Body::empty())
        })
}

/// Middleware function that performs capability token authentication.
///
/// Returns:
/// - `401 Unauthorized` for missing or invalid tokens
/// - `403 Forbidden` for expired tokens
/// - Passes through for routes listed in `PUBLIC_PATHS`
///
/// expect: "API endpoints enforce OCAP boundaries"
/// pre:  service is a valid AuthService
/// post: if path in PUBLIC_PATHS → pass-through (next.run)
/// post: if missing Authorization header → 401
/// post: if invalid/expired/revoked token → 401 or 403
/// post: if valid token → AuthContext injected, next.run
pub async fn auth_middleware(
    State(service): State<Arc<AuthService>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Allow public routes without authentication
    if PUBLIC_PATHS.iter().any(|prefix| path.starts_with(prefix)) {
        return next.run(req).await;
    }

    // If session middleware already injected AuthContext, skip capability token check
    if req.extensions().get::<AuthContext>().is_some() {
        return next.run(req).await;
    }

    // Extract Authorization header
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token_str = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        Some(_) | None => {
            return build_response(
                StatusCode::UNAUTHORIZED,
                Body::from("Missing or malformed Authorization header"),
            );
        }
    };

    // Base64-decode the token
    let token = match DelegationToken::from_base64(token_str) {
        Ok(t) => t,
        Err(_) => {
            return build_response(
                StatusCode::UNAUTHORIZED,
                Body::from("Invalid token encoding"),
            );
        }
    };

    // Verify the token
    match service.verify_token(&token) {
        TokenVerification::Valid => {
            // Double-check revocation via async-safe method
            if service.is_token_revoked(&token.id) {
                return build_response(
                    StatusCode::UNAUTHORIZED,
                    Body::from("Token has been revoked"),
                );
            }

            let webid = token.holder();

            // Attach auth context to request extensions
            let mut req = req;
            req.extensions_mut()
                .insert(AuthContext::from_token(token, webid));

            next.run(req).await
        }
        TokenVerification::Expired => {
            build_response(StatusCode::FORBIDDEN, Body::from("Token expired"))
        }
        TokenVerification::Invalid => build_response(
            StatusCode::UNAUTHORIZED,
            Body::from("Invalid capability token"),
        ),
        TokenVerification::Revoked => build_response(
            StatusCode::UNAUTHORIZED,
            Body::from("Token has been revoked"),
        ),
    }
}
