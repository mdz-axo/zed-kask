//! ABW HTTP client — thin reqwest wrapper isolating ABW assumptions.
//!
//! Extracted from the swarm server root. Encapsulates the base URL, auth
//! header, and error mapping (status AND body — ABW buries upstream LLM
//! errors in 200 envelopes). The panel, settings, and tool handlers never
//! construct raw requests; they go through this seam.

use crate::abw_util::{detect_embedded_error, extract_quoted};
use crate::config::SwarmConfig;
use crate::error::SwarmError;

/// Thin reqwest wrapper isolating every ABW-specific assumption (base URL,
/// auth header, error mapping) behind one seam. The panel, settings, and
/// tools never construct raw requests.
pub(crate) struct SwarmClient {
    http: reqwest::Client,
    config: SwarmConfig,
}

impl SwarmClient {
    pub(crate) fn new(http: reqwest::Client, config: SwarmConfig) -> Self {
        Self { http, config }
    }

    /// Read-only access to the resolved config (for budget-gate checks).
    pub(crate) fn config(&self) -> &SwarmConfig {
        &self.config
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!(
            "{}/api{}",
            self.config.api_base_url.trim_end_matches('/'),
            path
        )
    }

    /// True when an API key is configured. Read tools that need auth check
    /// this first and fail with a remediation message rather than a raw 401.
    pub(crate) fn is_authenticated(&self) -> bool {
        self.config.api_key.is_some()
    }

    pub(crate) fn require_auth(&self) -> Result<&str, SwarmError> {
        self.config
            .api_key
            .as_deref()
            .ok_or_else(|| SwarmError::Auth("no API key configured".to_string()))
    }

    /// Send a request, attaching the bearer token when present, and map the
    /// response (status AND body) into `Result<Value, SwarmError>`.
    pub(crate) async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<serde_json::Value, SwarmError> {
        let builder = match &self.config.api_key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        };
        let resp = builder
            .send()
            .await
            .map_err(|e| SwarmError::Unavailable(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        match status.as_u16() {
            200..=299 => {
                // DELETE endpoints and other no-content responses return an
                // empty body — treat that as a successful null result rather
                // than a parse failure.
                if body.trim().is_empty() {
                    return Ok(serde_json::Value::Null);
                }
                let value: serde_json::Value = serde_json::from_str(&body)
                    .map_err(|e| SwarmError::ApiVersionMismatch(format!("parse error: {e}")))?;
                // ABW wraps upstream LLM errors into 200 envelopes. Detect the
                // pattern ("I encountered an error" / "credit balance is too low")
                // so callers get a typed error instead of a success-looking payload.
                if let Some(err) = detect_embedded_error(&value) {
                    return Err(err);
                }
                Ok(value)
            }
            401 | 403 => Err(SwarmError::Auth(body.trim().to_string())),
            402 => Err(SwarmError::PaymentRequired(body.trim().to_string())),
            429 => Err(SwarmError::RateLimited(body.trim().to_string())),
            500 if body.contains("not funded") => {
                let agent = extract_quoted(&body).unwrap_or_default();
                Err(SwarmError::AgentNotFunded {
                    agent,
                    message: body.trim().to_string(),
                })
            }
            _ => Err(SwarmError::Unavailable(format!(
                "HTTP {status}: {}",
                body.trim()
            ))),
        }
    }

    pub(crate) async fn get(&self, path: &str) -> Result<serde_json::Value, SwarmError> {
        self.send(self.http.get(self.url(path))).await
    }

    pub(crate) async fn post(
        &self,
        path: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, SwarmError> {
        self.send(self.http.post(self.url(path)).json(payload))
            .await
    }

    /// Send a DELETE request (fire, workspace/agent teardown). Empty 2xx
    /// bodies are mapped to `null` by `send`.
    pub(crate) async fn delete(&self, path: &str) -> Result<serde_json::Value, SwarmError> {
        self.send(self.http.delete(self.url(path))).await
    }

    /// Generalized request carrying a query string and an optional JSON body.
    /// The verb helpers (`get`/`post`/`delete`) take only a path; ABW endpoints
    /// added after fermi v0.10.15/v0.10.26 take query parameters
    /// (`/agents/{id}/knowledge/search?q=`, `/agents/{id}/publish?force=…&reason=…`)
    /// that the helpers cannot carry. This is the deep path for those — it keeps
    /// the auth/timeout/error-mapping behavior of `send` without spawning a
    /// one-liner per query-string verb. Existing call sites keep the helpers.
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, SwarmError> {
        let mut builder = self.http.request(method, self.url(path));
        if !query.is_empty() {
            builder = builder.query(query);
        }
        if let Some(payload) = body {
            builder = builder.json(payload);
        }
        self.send(builder).await
    }

    /// Send a PATCH request. The workspace-update endpoint is 405 on ABW
    /// (verified live 2026-08-02 — no PATCH /workspaces/{id}); this exists
    /// only for the live probe that pins that fact.
    #[cfg(test)]
    #[expect(dead_code)]
    pub(crate) async fn patch(
        &self,
        path: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, SwarmError> {
        self.send(self.http.patch(self.url(path)).json(payload))
            .await
    }

    /// Fetch the operator's current wallet balance (the algedonic sense input).
    /// Returns `None` when unauthenticated (catalogue-only mode). A query
    /// failure emits a warning and returns `None` rather than fabricating a
    /// balance — the `.rules` trap about `unwrap_or(0)` on regulation signals:
    /// a failed measurement must be distinguishable from a measured zero.
    pub(crate) async fn wallet_balance(&self) -> Option<i64> {
        if !self.is_authenticated() {
            return None;
        }
        match self.get("/wallet").await {
            Ok(v) => v.get("balance").and_then(|b| b.as_i64()),
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.swarm",
                    "wallet balance query failed ({e}) — treating signal as stale, not zero"
                );
                None
            }
        }
    }

    /// Attach the current wallet balance to a tool response under a `wallet`
    /// key, so the algedonic signal rides every tool's return path instead of
    /// requiring a separate poll. No-op when unauthenticated or the balance
    /// query fails (the response is still useful without it).
    pub(crate) async fn with_wallet(&self, mut value: serde_json::Value) -> serde_json::Value {
        if let Some(balance) = self.wallet_balance().await
            && let Some(obj) = value.as_object_mut()
        {
            obj.insert(
                "wallet".to_string(),
                serde_json::json!({ "balance": balance }),
            );
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The algedonic wallet signal must never be fabricated. When the server is
    // unauthenticated, `wallet_balance` returns `None` (no key → no wallet),
    // and `with_wallet` leaves the response untouched rather than inserting a
    // zero. This pins the `.rules` trap: a missing measurement is
    // distinguishable from a measured zero balance.
    #[tokio::test]
    async fn wallet_envelope_absent_when_unauthenticated() {
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        assert!(client.wallet_balance().await.is_none());
        let out = client.with_wallet(serde_json::json!({"ok": true})).await;
        assert!(out.get("wallet").is_none());
        assert_eq!(out.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn client_url_joins_apex_and_path() {
        let client = SwarmClient::new(reqwest::Client::new(), SwarmConfig::default());
        assert_eq!(
            client.url("/agents"),
            "https://agent-bestiary.world/api/agents"
        );
    }
}
