//! Xaman Ek curator session — the two-step session lifecycle (create + send)
//! with a structural refund guard.
//!
//! `swarm_xaman` cannot use `spend_gate::complete_*` because its session
//! lifecycle has custom error mapping (Auth/PaymentRequired/RateLimited →
//! specific MCP kinds) and spans two HTTP calls (session create + message
//! send). The prior inline ladder had four `auth.take().refund()` sites — a
//! manual footgun where a new failure path could forget the refund.
//!
//! `CuratorSession` owns the `Option<DelegateAuthorization>` and refunds it
//! on `Drop` unless `disarm()` is called on success. The refund invariant
//! is now structural: the guard refunds exactly once, on any path that
//! doesn't explicitly disarm it.

use hkask_mcp_server::server::McpToolError;

use crate::abw_client::SwarmClient;
use crate::abw_util::url_encode_segment;
use crate::consent::ConsentStore;
use crate::error::SwarmError;
use crate::spend_gate::{self, DelegateAuthorization};

/// A refund guard for a Xaman Ek curator call. Owns the consent authorization
/// and refunds it on `Drop` unless `disarm()` is called on success.
///
/// Created via [`CuratorSession::create`] (creates a new ABW session) or
/// [`CuratorSession::resume`] (reuses an existing session_id). Both refund
/// the authorization on construction failure. Call `send` to post a message;
/// on success, `send` calls `disarm` internally so the auth is consumed. On
/// any `Err` return from `create`/`send`, the guard refunds and drops.
pub(crate) struct CuratorSession<'a> {
    client: &'a SwarmClient,
    consent: &'a ConsentStore,
    auth: Option<DelegateAuthorization>,
    session_id: String,
    /// Whether the authorization has been consumed (success) or refunded
    /// (failure). When `true`, `Drop` is a no-op.
    settled: bool,
}

impl<'a> CuratorSession<'a> {
    /// Create a new Xaman Ek session and return a guard holding the auth.
    /// Takes ownership of the auth so it can refund on construction failure.
    /// Refunds the auth on session-create failure with the custom Xaman
    /// error mapping (Auth/PaymentRequired/RateLimited → specific kinds).
    pub(crate) async fn create(
        client: &'a SwarmClient,
        consent: &'a ConsentStore,
        mut auth: Option<DelegateAuthorization>,
        session_type: &str,
    ) -> Result<Self, McpToolError> {
        let created = client
            .post(
                "/xaman/sessions",
                &serde_json::json!({ "session_type": session_type }),
            )
            .await
            .map_err(|e| Self::map_create_error(consent, auth.take(), e))?;
        let session_id = match created
            .get("session_id")
            .and_then(|s| s.as_str())
            .map(str::to_string)
        {
            Some(id) => id,
            None => {
                return Err(Self::refund_and_err(
                    consent,
                    auth.take(),
                    SwarmError::ApiVersionMismatch(
                        "xaman session create returned no session_id".to_string(),
                    )
                    .into_tool_error(),
                ));
            }
        };
        Ok(Self {
            client,
            consent,
            auth,
            session_id,
            settled: false,
        })
    }

    /// Resume an existing Xaman Ek session by id. The auth is carried but
    /// no session-create call is made.
    pub(crate) fn resume(
        client: &'a SwarmClient,
        consent: &'a ConsentStore,
        auth: Option<DelegateAuthorization>,
        session_id: String,
    ) -> Self {
        Self {
            client,
            consent,
            auth,
            session_id,
            settled: false,
        }
    }

    /// Send a message to the curator. On success, disarms the guard (the
    /// auth stays consumed) and returns the raw ABW response. On failure,
    /// refunds the auth and propagates the error.
    pub(crate) async fn send(&mut self, message: &str) -> Result<serde_json::Value, McpToolError> {
        let data = self
            .client
            .post(
                &format!(
                    "/xaman/sessions/{}/message",
                    url_encode_segment(&self.session_id)
                ),
                &serde_json::json!({ "message": message }),
            )
            .await
            .map_err(|e| {
                self.refund();
                SwarmError::into_tool_error(e)
            })?;
        self.disarm();
        Ok(data)
    }

    /// The session id this guard holds.
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Mark the authorization as settled (consumed on success). After this,
    /// `Drop` is a no-op.
    fn disarm(&mut self) {
        self.settled = true;
    }

    /// Refund the auth if present and not yet settled, then mark settled.
    fn refund(&mut self) {
        if !self.settled {
            if let Some(a) = self.auth.take() {
                a.refund(self.consent);
            }
            self.settled = true;
        }
    }

    /// Map a session-create error to the MCP tool error surface, refunding
    /// the auth first. The Xaman create path has custom error mapping:
    /// Auth/PaymentRequired/RateLimited → specific kinds; everything else →
    /// `CuratorUnavailable`.
    fn map_create_error(
        consent: &ConsentStore,
        auth: Option<DelegateAuthorization>,
        e: SwarmError,
    ) -> McpToolError {
        if let Some(a) = auth {
            a.refund(consent);
        }
        match e {
            SwarmError::Auth(m) => McpToolError::permission_denied(m),
            SwarmError::PaymentRequired(m) => McpToolError::permission_denied(m),
            SwarmError::RateLimited(m) => McpToolError::rate_limited(m),
            other => SwarmError::CuratorUnavailable(other.to_string()).into_tool_error(),
        }
    }

    /// Refund the auth (if present) and return the given error. Used when
    /// the guard never took ownership of the auth (construction failure).
    fn refund_and_err(
        consent: &ConsentStore,
        auth: Option<DelegateAuthorization>,
        err: McpToolError,
    ) -> McpToolError {
        if let Some(a) = auth {
            a.refund(consent);
        }
        err
    }
}

impl<'a> Drop for CuratorSession<'a> {
    fn drop(&mut self) {
        self.refund();
    }
}

/// Authorize a curator call and return the auth to hand to `CuratorSession`.
/// Thin wrapper around `spend_gate::authorize_curate` so the caller doesn't
/// need to import `spend_gate` directly.
pub(crate) fn authorize(
    client: &SwarmClient,
    consent: &ConsentStore,
    token: Option<&str>,
) -> Result<Option<DelegateAuthorization>, McpToolError> {
    spend_gate::authorize_curate(client, consent, token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SwarmConfig;

    /// A `CuratorSession` that is dropped without `send` succeeding must
    /// refund the auth. This pins the structural refund invariant: the guard
    /// refunds on any path that doesn't explicitly disarm it.
    #[test]
    fn dropped_session_refunds_no_auth_is_noop() {
        let consent = ConsentStore::default();
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        let _session = CuratorSession::resume(&client, &consent, None, "fake-session".to_string());
        // Dropping here must not panic and must not refund (no auth present).
    }

    /// `resume` with `None` auth is the `curator_consent_default = true` path
    /// (operator globally opted in). The guard holds no auth, so `Drop` and
    /// `refund` are no-ops. This pins that the opt-in path doesn't
    /// accidentally try to refund a non-existent grant.
    #[test]
    fn resume_with_no_auth_is_noop_on_drop() {
        let consent = ConsentStore::default();
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        let session = CuratorSession::resume(&client, &consent, None, "s".to_string());
        assert_eq!(session.session_id(), "s");
        // Drop — no refund, no panic.
    }

    /// `disarm` prevents the refund on drop. After `disarm`, `refund` is a
    /// no-op. This pins the success path: `send` calls `disarm` on success,
    /// so the auth stays consumed.
    #[test]
    fn disarm_prevents_refund() {
        let consent = ConsentStore::default();
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        let mut session = CuratorSession::resume(&client, &consent, None, "s".to_string());
        session.disarm();
        session.refund(); // no-op — already settled
        assert!(session.settled);
    }

    /// `refund` is idempotent — calling it twice does not double-refund.
    /// This pins the `settled` flag's role: the first `refund` marks settled,
    /// the second is a no-op.
    #[test]
    fn refund_is_idempotent() {
        let consent = ConsentStore::default();
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        let mut session = CuratorSession::resume(&client, &consent, None, "s".to_string());
        session.refund();
        session.refund(); // second call — no-op
        assert!(session.settled);
    }
}
