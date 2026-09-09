//! Xaman Ek curator session — the two-step session lifecycle (create + send)
//! with a structural settlement guard.
//!
//! `swarm_xaman` cannot use `spend_gate::complete_*` because its session
//! lifecycle has custom error mapping (Auth/PaymentRequired/RateLimited →
//! specific MCP kinds) and spans two HTTP calls (session create + message
//! send). The prior inline ladder had four `auth.take().refund()` sites — a
//! manual footgun where a new failure path could forget the refund.
//!
//! `CuratorSession` owns the `Option<Settlement>` and settles it
//! on `Drop` unless `disarm()` is called on success. Settlement follows the
//! operator-ratified T05 policy (2026-09-08): every failure path classifies
//! the error — a proven pre-dispatch rejection releases the authorization,
//! an ambiguous or accepted-but-unreadable outcome holds it and surfaces
//! the uncertainty. `Drop` (reached only when `send` was never called — no
//! content dispatched) releases.

use hkask_mcp_server::server::McpToolError;

use crate::abw_client::SwarmClient;
use crate::abw_util::url_encode_segment;
use crate::consent::ConsentStore;
use crate::error::SwarmError;
use crate::spend_gate::{self, Settlement};

/// A settlement guard for a Xaman Ek curator call. Owns the consent
/// authorization and settles it on `Drop` unless `disarm()` is called on
/// success.
///
/// Created via [`CuratorSession::create`] (creates a new ABW session) or
/// [`CuratorSession::resume`] (reuses an existing session_id). Both settle
/// the reservation on construction failure per the T05 classification.
/// Call `send` to post a message; on success, `send` calls `disarm`
/// internally so the auth is consumed. On any `Err` return from
/// `create`/`send`, the guard settles (release or hold) and drops.
pub struct CuratorSession<'a> {
    client: &'a SwarmClient,
    consent: &'a ConsentStore,
    auth: Option<Settlement>,
    session_id: String,
    /// Whether the authorization has been settled (consumed on success,
    /// released on proven rejection, or held on ambiguity). When `true`,
    /// `Drop` is a no-op.
    settled: bool,
}

impl<'a> CuratorSession<'a> {
    /// Create a new Xaman Ek session and return a guard holding the auth.
    /// Takes ownership of the auth so it can settle on construction failure
    /// per the T05 classification (proven rejection releases; ambiguity
    /// holds and surfaces the uncertainty; a 2xx without a session_id is
    /// external acceptance followed by a local failure — held).
    pub(crate) async fn create(
        client: &'a SwarmClient,
        consent: &'a ConsentStore,
        mut auth: Option<Settlement>,
        session_type: &str,
    ) -> Result<Self, McpToolError> {
        let created = client
            .post(
                "/xaman/sessions",
                &serde_json::json!({ "session_type": session_type }),
            )
            .await
            .map_err(|e| Self::settle_on_error(consent, auth.take(), e))?;
        let session_id = match created
            .get("session_id")
            .and_then(|s| s.as_str())
            .map(str::to_string)
        {
            Some(id) => id,
            None => {
                return Err(Self::settle_on_error(
                    consent,
                    auth.take(),
                    SwarmError::ApiVersionMismatch(
                        "xaman session create returned no session_id".to_string(),
                    ),
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
        auth: Option<Settlement>,
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
    /// settles the auth by the T05 classification (release on proven
    /// rejection; hold + surface on ambiguity) and propagates the error.
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
                let auth = self.auth.take();
                self.settled = true;
                Self::settle_on_error(self.consent, auth, e)
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

    /// Settle the authorization for a failed curator call by the T05
    /// policy: a proven pre-dispatch rejection releases it (with the custom
    /// Xaman error mapping: Auth/PaymentRequired/RateLimited → specific
    /// kinds, everything else → `CuratorUnavailable`); an ambiguous or
    /// accepted-but-unreadable outcome holds it and surfaces the
    /// uncertainty. Returns the tool error to propagate.
    fn settle_on_error(
        consent: &ConsentStore,
        auth: Option<Settlement>,
        e: SwarmError,
    ) -> McpToolError {
        let Some(auth) = auth else {
            return SwarmError::into_tool_error(e);
        };
        if e.is_proven_rejection() {
            auth.release(consent);
            return match e {
                SwarmError::Auth(m) => McpToolError::permission_denied(m),
                SwarmError::PaymentRequired(m) => McpToolError::permission_denied(m),
                SwarmError::RateLimited(m) => McpToolError::rate_limited(m),
                other => SwarmError::CuratorUnavailable(other.to_string()).into_tool_error(),
            };
        }
        let held = auth.held_description();
        auth.hold();
        McpToolError::unavailable(format!(
            "curator dispatch outcome uncertain — the request may have reached \
             Xaman Ek and the result is unknown; {held}. Inspect the session \
             before retrying; a retry cannot reuse this authorization. ({e})"
        ))
    }
}

impl<'a> Drop for CuratorSession<'a> {
    fn drop(&mut self) {
        // Reached only when `send` was never called: every create/send
        // failure settles explicitly and send success disarms. No content
        // was dispatched, so releasing is correct regardless of
        // classification — nothing was sent.
        if !self.settled {
            if let Some(a) = self.auth.take() {
                a.release(self.consent);
            }
            self.settled = true;
        }
    }
}

/// Authorize a curator call and return the auth to hand to `CuratorSession`.
/// Thin wrapper around `spend_gate::authorize_curate` so the caller doesn't
/// need to import `spend_gate` directly.
pub fn authorize(
    client: &SwarmClient,
    consent: &ConsentStore,
    token: Option<&str>,
) -> Result<Option<Settlement>, McpToolError> {
    spend_gate::authorize_curate(client, consent, token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abw_client::test_http::{Behavior, FixtureServer, sqlite_store, test_client};

    /// The curate token is single-use with cost 0 — the reservation is the
    /// consumed token gating content sent to the third-party curator.
    fn curate_token(consent: &ConsentStore) -> String {
        consent.mint("curate", "xaman", 0).expect("mint")
    }

    /// `CuratorSession::create`'s Ok type is not Debug — extract the error
    /// without `expect_err`.
    async fn expect_create_error(
        client: &SwarmClient,
        store: &ConsentStore,
        auth: Option<Settlement>,
        session_type: &str,
    ) -> McpToolError {
        match CuratorSession::create(client, store, auth, session_type).await {
            Err(error) => error,
            Ok(_) => panic!("expected a create error"),
        }
    }

    /// T05 (Q4): an ambiguous session-create outcome HOLDS the curate token.
    /// Pre-fix, `map_create_error` refunded every error — a token that may
    /// have paid for an accepted dispatch became reusable.
    #[tokio::test]
    async fn xaman_create_ambiguous_holds_curate_token() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let token = curate_token(&store);
        let server = FixtureServer::start(vec![Behavior::Ambiguous]);
        let client = test_client(&server.base_url());
        let auth = authorize(&client, &store, Some(&token)).expect("authorized");
        let error = expect_create_error(&client, &store, auth, "free").await;
        assert!(
            error.to_string().contains("uncertain"),
            "the uncertainty must be surfaced: {error}"
        );
        assert!(
            store.consume(&token, "curate", "xaman", 0).is_err(),
            "the curate token must stay consumed"
        );
    }

    /// T05 (Q4): an ambiguous message-send outcome HOLDS the curate token.
    #[tokio::test]
    async fn xaman_send_ambiguous_holds_curate_token() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let token = curate_token(&store);
        let server = FixtureServer::start(vec![
            Behavior::Respond(200, "{\"session_id\":\"s1\"}".to_string()),
            Behavior::Ambiguous,
        ]);
        let client = test_client(&server.base_url());
        let auth = authorize(&client, &store, Some(&token)).expect("authorized");
        let mut session = CuratorSession::create(&client, &store, auth, "free")
            .await
            .expect("session created");
        let error = session
            .send("plan my swarm")
            .await
            .expect_err("ambiguous send");
        assert!(
            error.to_string().contains("uncertain"),
            "the uncertainty must be surfaced: {error}"
        );
        assert!(
            store.consume(&token, "curate", "xaman", 0).is_err(),
            "the curate token must stay consumed"
        );
    }

    /// T05: a 2xx create response without a session_id is external acceptance
    /// followed by a local failure — the token is HELD, not refunded. Pre-fix
    /// this path refunded unconditionally.
    #[tokio::test]
    async fn xaman_create_without_session_id_holds_curate_token() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let token = curate_token(&store);
        let server = FixtureServer::start(vec![Behavior::Respond(
            200,
            "{\"unexpected\":true}".to_string(),
        )]);
        let client = test_client(&server.base_url());
        let auth = authorize(&client, &store, Some(&token)).expect("authorized");
        let _ = expect_create_error(&client, &store, auth, "free").await;
        assert!(
            store.consume(&token, "curate", "xaman", 0).is_err(),
            "external acceptance followed by a local failure holds the token"
        );
    }

    /// T05 control: ABW answering with an error proves the dispatch did not
    /// land — the curate token is released and reusable.
    #[tokio::test]
    async fn xaman_rejection_releases_curate_token() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let token = curate_token(&store);
        let server = FixtureServer::start(vec![Behavior::Respond(
            402,
            "insufficient credits".to_string(),
        )]);
        let client = test_client(&server.base_url());
        let auth = authorize(&client, &store, Some(&token)).expect("authorized");
        let _ = expect_create_error(&client, &store, auth, "free").await;
        assert!(
            store.consume(&token, "curate", "xaman", 0).is_ok(),
            "a proven rejection releases the token for retry"
        );
    }

    /// T05 control: a successful two-step curator call consumes the token —
    /// single-use per successful spend.
    #[tokio::test]
    async fn xaman_success_consumes_curate_token() {
        let dir = tempfile::tempdir().expect("dir");
        let store = sqlite_store(&dir);
        let token = curate_token(&store);
        let server = FixtureServer::start(vec![
            Behavior::Respond(200, "{\"session_id\":\"s1\"}".to_string()),
            Behavior::Respond(200, "{\"response\":\"plan\"}".to_string()),
        ]);
        let client = test_client(&server.base_url());
        let auth = authorize(&client, &store, Some(&token)).expect("authorized");
        let mut session = CuratorSession::create(&client, &store, auth, "free")
            .await
            .expect("session created");
        let data = session.send("plan my swarm").await.expect("send succeeds");
        assert_eq!(
            data.get("response").and_then(|value| value.as_str()),
            Some("plan")
        );
        assert!(
            store.consume(&token, "curate", "xaman", 0).is_err(),
            "a successful spend consumes the token"
        );
    }
}
