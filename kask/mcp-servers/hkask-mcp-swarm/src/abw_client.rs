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
pub struct SwarmClient {
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
        self.config.api_key.as_deref().ok_or_else(|| {
            // The env var must be named in the message (.rules: a missing
            // credential is permission_denied naming the env var, so the
            // operator can distinguish "not configured" from "configured
            // but broken").
            SwarmError::Auth(
                "no ABW API key configured — set HKASK_ABW_API_KEY (zed keychain or env)"
                    .to_string(),
            )
        })
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
        let resp = builder.send().await.map_err(|e| {
            // Settlement classification (operator-ratified T05 policy,
            // 2026-09-08): connection-phase and construction failures prove
            // the request never left — releasable. Everything else (timeout
            // mid-request, connection reset after send) leaves the external
            // outcome unknown — the reservation must be held.
            if e.is_connect() || e.is_builder() {
                SwarmError::DispatchNotSent(e.to_string())
            } else {
                SwarmError::DispatchAmbiguous(e.to_string())
            }
        })?;
        let status = resp.status();
        // A failed body read must not become a fake success — an empty body
        // on a 200 is treated as a legitimate null result below, so a
        // network failure mid-body would silently masquerade as one.
        let body = resp.text().await.map_err(|e| {
            if status.is_success() {
                // ABW accepted the request (2xx) but the body was lost —
                // external acceptance followed by a transport failure:
                // the dispatch landed, the response content is unknown.
                // Hold, never release.
                SwarmError::DispatchAmbiguous(format!(
                    "accepted (HTTP {status}) but the response body was lost: {e}"
                ))
            } else {
                // The error status already arrived — ABW answered and
                // rejected the request; only the body text is missing.
                // Proven rejection.
                SwarmError::Unavailable(format!("HTTP {status} (response body lost): {e}"))
            }
        })?;

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
    /// added after fermi v0.16.1 take query parameters
    /// (`/agents/{id}/publish?force=…&reason=…`, `/agents/{id}/kg/rules?active_only=true`)
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
    /// (verified live 2026-08-13 — no PATCH /workspaces/{id}); this exists
    /// only for the live probe that pins that fact.
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

/// Bounded local HTTP fixture for spend-gate settlement tests — the
/// "barrier-controlled bounded HTTP fixture" the reliability plan requires.
/// Serves a fixed sequence of behaviors, one per connection, then stops.
/// Never contacts any real provider. Also hosts the shared settlement-test
/// fixtures: a keyless `SwarmClient` constructor and a SQLite `ConsentStore`
/// opener, so the spend-gate and curator suites share one construction.
#[cfg(test)]
pub(crate) mod test_http {
    use super::SwarmClient;
    use crate::config::SwarmConfig;
    use crate::consent::ConsentStore;
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// What the fixture does with one request.
    #[derive(Debug, Clone)]
    pub(crate) enum Behavior {
        /// Respond with the given status and body.
        Respond(u16, String),
        /// Read the request, then close WITHOUT responding — the client sees
        /// a sent request and no answer (the ambiguous-outcome trigger).
        Ambiguous,
    }

    /// Read the full request (headers + content-length body) so the server
    /// can respond (or close) without leaving unread data that would turn a
    /// clean close into a connection reset.
    fn read_request(stream: &mut TcpStream) {
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let mut buf = [0u8; 8192];
        let mut received: Vec<u8> = Vec::new();
        let header_end = loop {
            if let Some(pos) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(n) => received.extend_from_slice(&buf[..n]),
            }
        };
        let headers = String::from_utf8_lossy(&received[..header_end]).to_lowercase();
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while received.len() < header_end + content_length {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => received.extend_from_slice(&buf[..n]),
            }
        }
    }

    pub(crate) struct FixtureServer {
        port: u16,
        shutdown: Arc<std::sync::atomic::AtomicBool>,
        requests_served: Arc<AtomicUsize>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl FixtureServer {
        /// Start a server serving the given behaviors, one per connection;
        /// it stops accepting once the queue is empty.
        pub(crate) fn start(behaviors: Vec<Behavior>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
            let port = listener.local_addr().expect("fixture addr").port();
            let behaviors: Arc<Mutex<VecDeque<Behavior>>> =
                Arc::new(Mutex::new(VecDeque::from(behaviors)));
            let requests_served = Arc::new(AtomicUsize::new(0));
            let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let queue = Arc::clone(&behaviors);
            let served = Arc::clone(&requests_served);
            let shutdown_flag = Arc::clone(&shutdown);
            let handle = std::thread::spawn(move || {
                for stream in listener.incoming() {
                    // Checked AFTER accept returns: a Drop-time wake-up
                    // connection must not consume a queued behavior (a test
                    // that fails before serving all behaviors would
                    // otherwise hang teardown instead of failing cleanly).
                    if shutdown_flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(mut stream) = stream else { break };
                    let behavior = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    let Some(behavior) = behavior else { break };
                    served.fetch_add(1, Ordering::SeqCst);
                    match behavior {
                        Behavior::Respond(status, body) => {
                            read_request(&mut stream);
                            let response = format!(
                                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.flush();
                        }
                        Behavior::Ambiguous => {
                            // Read the request fully, then close WITHOUT a
                            // response — reqwest sees a completed send and a
                            // connection that ended before any answer.
                            read_request(&mut stream);
                        }
                    }
                }
            });
            Self {
                port,
                shutdown,
                requests_served,
                handle: Some(handle),
            }
        }

        /// The base URL a test `SwarmClient` should use.
        pub(crate) fn base_url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }

        /// How many requests the fixture actually served.
        pub(crate) fn requests_served(&self) -> usize {
            self.requests_served.load(Ordering::SeqCst)
        }
    }

    impl Drop for FixtureServer {
        fn drop(&mut self) {
            // Set the shutdown flag, then wake a blocked accept with a
            // self-connect. The flag (checked after accept returns) stops the
            // loop WITHOUT consuming a queued behavior, so a test that ends
            // early — failure or otherwise — cannot wedge teardown.
            self.shutdown.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(("127.0.0.1", self.port));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// A keyless `SwarmClient` pointed at `base_url` with a short timeout —
    /// the shared construction for settlement tests.
    pub(crate) fn test_client(base_url: &str) -> SwarmClient {
        SwarmClient::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("test client"),
            SwarmConfig {
                api_base_url: base_url.to_string(),
                api_key: None,
                ..SwarmConfig::default()
            },
        )
    }

    /// A SQLite-backed consent store in `dir`. Two stores opened on one
    /// file model two server instances sharing the consent DB (the panel
    /// flow and the tool flow).
    pub(crate) fn sqlite_store(dir: &tempfile::TempDir) -> ConsentStore {
        ConsentStore::open_sqlite(dir.path().join("consent.db").to_str().expect("path"))
            .expect("consent store")
    }
}
