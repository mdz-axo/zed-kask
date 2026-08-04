//! A2A HTTP server — `tiny_http`-based loopback HTTP server that exposes
//! local agents via the A2A (Agent2Agent) protocol.
//!
//! Runs in a dedicated OS thread (completely outside the tokio runtime).
//! Each A2A HTTP request is handled synchronously in that thread, then
//! dispatches to the async `LocalSwarmRuntime::delegate` via
//! `tokio::runtime::Handle::block_on`.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use crate::a2a;
use crate::error::SwarmError;
use crate::local_registry::LocalAgentRegistry;
use crate::local_runtime::LazyLocalSwarmRuntime;

/// A minimal A2A Message for HTTP deserialization. Wire-compatible with
/// the A2A `Message` type but avoids importing the private `a2a::Message`.
#[derive(Debug, Clone, serde::Deserialize)]
struct A2aHttpMessage {
    #[serde(default)]
    parts: Vec<A2aHttpPart>,
    #[serde(default)]
    context_id: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct A2aHttpPart {
    #[serde(default)]
    text: Option<String>,
}

pub(crate) struct A2aHttpServer {
    port: u16,
}

impl A2aHttpServer {
    pub fn start(
        runtime: Arc<LazyLocalSwarmRuntime>,
        registry: Arc<LocalAgentRegistry>,
        tokio_handle: tokio::runtime::Handle,
        max_credits_per_dispatch: u32,
    ) -> Result<Self, String> {
        let server = tiny_http::Server::http("127.0.0.1:0")
            .map_err(|e| format!("failed to bind A2A HTTP server: {e}"))?;

        let port = match server.server_addr() {
            tiny_http::ListenAddr::IP(addr) => addr.port(),
            tiny_http::ListenAddr::Unix(_) => {
                return Err("A2A HTTP server bound to Unix socket, not TCP".to_string());
            }
        };

        let base_url = format!("http://127.0.0.1:{port}");
        std::thread::spawn(move || {
            run_server(
                server,
                runtime,
                registry,
                tokio_handle,
                base_url,
                max_credits_per_dispatch,
            );
        });

        tracing::info!(target: "hkask.mcp.swarm", port, "A2A HTTP server on 127.0.0.1:{port}");
        Ok(Self { port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

fn run_server(
    server: tiny_http::Server,
    runtime: Arc<LazyLocalSwarmRuntime>,
    registry: Arc<LocalAgentRegistry>,
    tokio_handle: tokio::runtime::Handle,
    base_url: String,
    max_credits_per_dispatch: u32,
) {
    loop {
        match server.recv_timeout(Duration::from_secs(1)) {
            Ok(Some(request)) => {
                handle_request(
                    request,
                    &runtime,
                    &registry,
                    &tokio_handle,
                    &base_url,
                    max_credits_per_dispatch,
                );
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(target: "hkask.mcp.swarm", error = %e, "A2A HTTP recv error");
            }
        }
    }
}

fn handle_request(
    mut request: tiny_http::Request,
    runtime: &Arc<LazyLocalSwarmRuntime>,
    registry: &Arc<LocalAgentRegistry>,
    tokio_handle: &tokio::runtime::Handle,
    base_url: &str,
    max_credits_per_dispatch: u32,
) {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();

    let response: tiny_http::Response<Cursor<Vec<u8>>> = match (method.as_str(), url.as_str()) {
        ("GET", "/.well-known/agent-card.json") => {
            let cards = registry.list();
            let a2a_cards: Vec<_> = cards
                .iter()
                .map(|c| a2a::to_a2a_card(c, base_url))
                .collect();
            json_response(
                200,
                &serde_json::json!({"agent_cards": a2a_cards, "count": a2a_cards.len()}),
            )
        }

        ("GET", path) if path.ends_with("/.well-known/agent-card.json") => {
            let agent_id = extract_agent_id(path, "/.well-known/agent-card.json");
            match registry.get(&agent_id) {
                Some(card) => json_response(200, &a2a::to_a2a_card(&card, base_url)),
                None => json_response(
                    404,
                    &serde_json::json!({"error": "agent not found", "agent_id": agent_id}),
                ),
            }
        }

        ("POST", path) if path.ends_with("/message:send") => {
            let agent_id = extract_agent_id(path, "/message:send");

            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                let _ = request.respond(json_response(
                    400,
                    &serde_json::json!({"error": format!("failed to read body: {e}")}),
                ));
                return;
            }

            let message: A2aHttpMessage = match serde_json::from_str(&body) {
                Ok(m) => m,
                Err(e) => {
                    let _ = request.respond(json_response(
                        400,
                        &serde_json::json!({"error": format!("invalid A2A Message: {e}")}),
                    ));
                    return;
                }
            };

            let message_text = message
                .parts
                .iter()
                .filter_map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join("\n");
            if message_text.is_empty() {
                let _ = request.respond(json_response(
                    400,
                    &serde_json::json!({"error": "message has no text parts"}),
                ));
                return;
            }

            let agent = match registry.get(&agent_id) {
                Some(card) => card,
                None => {
                    let _ = request.respond(json_response(
                        404,
                        &serde_json::json!({"error": "agent not found", "agent_id": agent_id}),
                    ));
                    return;
                }
            };

            let credits = 20u32;
            let result = tokio_handle.block_on(async {
                let runtime = runtime
                    .get_or_init()
                    .await
                    .map_err(|e| SwarmError::Unavailable(e))?;
                runtime
                    .delegate(&agent, &message_text, credits, max_credits_per_dispatch)
                    .await
            });

            match result {
                Ok(delegate_result) => {
                    let task = a2a::task_from_response(
                        &delegate_result.response,
                        message.context_id,
                        &delegate_result.model,
                        delegate_result.tokens_used,
                        delegate_result.cost,
                    );
                    json_response(200, &task)
                }
                Err(e) => {
                    let status = match &e {
                        SwarmError::PaymentRequired(_) => 402,
                        _ => 500,
                    };
                    json_response(status, &serde_json::json!({"error": e.to_string()}))
                }
            }
        }

        ("GET", "/healthz") => json_response(200, &serde_json::json!({"status": "ok"})),

        _ => json_response(
            404,
            &serde_json::json!({"error": "not found", "method": method, "path": url}),
        ),
    };

    let _ = request.respond(response);
}

fn extract_agent_id(path: &str, suffix: &str) -> String {
    let without_suffix = path.strip_suffix(suffix).unwrap_or(path);
    without_suffix
        .strip_prefix("/agents/")
        .unwrap_or(without_suffix)
        .to_string()
}

fn json_response(
    status: u16,
    body: &impl serde::Serialize,
) -> tiny_http::Response<Cursor<Vec<u8>>> {
    let json = serde_json::to_vec(body)
        .unwrap_or_else(|_| Vec::from(&b"{\"error\":\"serialization failed\"}"[..]));
    tiny_http::Response::from_data(json)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(b"Content-Type", b"application/json")
                .expect("valid header"),
        )
}
