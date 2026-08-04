//! A2A HTTP gateway — JSON-RPC 2.0 over HTTP, exposing local agents to
//! external A2A clients (Agent2Agent protocol, https://github.com/a2aproject/A2A).
//!
//! One loopback HTTP server (one port) fronts every local agent. Routing is by
//! the JSON-RPC `tenant` field (the A2A spec's opaque routing identifier): the
//! gateway `AgentCard` advertises one `AgentInterface` per local agent with
//! `tenant = agent_id`, and an external client sends `SendMessage` with
//! `tenant = <agent_id>` to reach that agent. The card is regenerated from
//! `LocalAgentRegistry` on every request, so agents created at runtime appear
//! without a server restart.
//!
//! The local ledger (`swarm_fund_local` / `LocalSwarmRuntime::delegate`) is the
//! only spend gate — no consent tokens on this path, consistent with the local
//! model. Streaming, push notifications, and task cancellation are not supported
//! in v1 (the card declares `streaming=false`, `push_notifications=false`);
//! `delegate` is synchronous and returns a completed `Task` inline, so there is
//! no task store — `GetTask` reports not-found and `ListTasks` returns empty.
//!
//! Runs in a dedicated OS thread (outside the tokio runtime); each `SendMessage`
//! dispatches to the async `LocalSwarmRuntime::delegate` via
//! `tokio::runtime::Handle::block_on`. This is the swarm MCP server binary (not
//! GPUI), so `block_on` on a dedicated thread is the correct pattern.

use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use crate::error::LocalSwarmError;
use crate::error::SwarmError;
use crate::local_registry::LocalAgentRegistry;
use crate::local_runtime::LazyLocalSwarmRuntime;

use a2a::errors::error_code;
use a2a::jsonrpc::{JsonRpcError, JsonRpcId, JsonRpcRequest, JsonRpcResponse, methods};
use a2a::{
    AgentCapabilities, AgentCard, AgentInterface, AgentSkill, ListTasksResponse,
    SendMessageRequest, SendMessageResponse,
};

/// Loopback credits authorized per external `SendMessage`. External A2A clients
/// do not carry a budget; the local ledger still gates the actual spend, and
/// the per-dispatch ceiling (`max_credits_per_dispatch`) clamps this.
const A2A_HTTP_CREDITS: u32 = 20;

pub(crate) struct A2aHttpServer {
    port: u16,
}

impl A2aHttpServer {
    /// Start the A2A HTTP gateway on an ephemeral loopback port. Spawns a
    /// dedicated OS thread; returns the bound port. The server reflects the
    /// current `LocalAgentRegistry` on every request (no restart needed for
    /// agents added/removed at runtime).
    pub fn start(
        runtime: Arc<LazyLocalSwarmRuntime>,
        registry: Arc<LocalAgentRegistry>,
        tokio_handle: tokio::runtime::Handle,
        max_credits_per_dispatch: u32,
    ) -> Result<Self, LocalSwarmError> {
        let server = tiny_http::Server::http("127.0.0.1:0")
            .map_err(|e| LocalSwarmError::Io(format!("failed to bind A2A HTTP server: {e}")))?;
        let port = match server.server_addr() {
            tiny_http::ListenAddr::IP(addr) => addr.port(),
            tiny_http::ListenAddr::Unix(_) => {
                return Err(LocalSwarmError::InvalidInput(
                    "A2A HTTP server bound to Unix socket, not TCP".to_string(),
                ));
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
            )
        });
        tracing::info!(
            target: "hkask.mcp.swarm",
            port,
            "A2A HTTP gateway on 127.0.0.1:{port} (JSON-RPC over POST /)"
        );
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
            let card = build_gateway_card(registry, base_url);
            json_response(200, &card)
        }

        ("POST", "/") => {
            let mut body = String::new();
            if let Err(e) = request.as_reader().read_to_string(&mut body) {
                let err = JsonRpcResponse::error(
                    JsonRpcId::Null,
                    JsonRpcError {
                        code: error_code::INVALID_REQUEST,
                        message: format!("failed to read body: {e}"),
                        data: None,
                    },
                );
                let _ = request.respond(json_rpc_raw_response(err));
                return;
            }
            let resp = handle_jsonrpc(
                &body,
                runtime,
                registry,
                tokio_handle,
                max_credits_per_dispatch,
            );
            json_rpc_raw_response(resp)
        }

        ("GET", "/healthz") => json_response(200, &serde_json::json!({"status": "ok"})),

        _ => json_response(
            404,
            &serde_json::json!({"error": "not found", "method": method, "path": url}),
        ),
    };

    let _ = request.respond(response);
}

/// Build the gateway `AgentCard` from the current registry. One
/// `AgentInterface` per local agent (same gateway URL, `tenant = agent_id`),
/// so an external client selects an agent by its interface's `tenant` and
/// sends `SendMessage` with that `tenant`. Regenerated per request → runtime
/// agent changes appear without a restart. With zero agents, a single
/// tenant-less interface keeps the card valid (`supported_interfaces` is
/// required and must be non-empty).
fn build_gateway_card(registry: &LocalAgentRegistry, base_url: &str) -> AgentCard {
    let agents = registry.list();
    let mut interfaces: Vec<AgentInterface> = agents
        .iter()
        .map(|card| {
            let mut iface = AgentInterface::new(base_url, a2a::TRANSPORT_PROTOCOL_JSONRPC);
            iface.tenant = Some(card.agent_id.clone());
            iface
        })
        .collect();
    if interfaces.is_empty() {
        interfaces.push(AgentInterface::new(
            base_url,
            a2a::TRANSPORT_PROTOCOL_JSONRPC,
        ));
    }

    let skills: Vec<AgentSkill> = agents
        .iter()
        .map(|card| AgentSkill {
            id: card.agent_id.clone(),
            name: card.agent_id.clone(),
            description: if card.description.is_empty() {
                format!("Local agent: {}", card.agent_id)
            } else {
                card.description.clone()
            },
            tags: vec![card.agent_type.clone()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        })
        .collect();

    AgentCard {
        name: "hKask Local Swarm Gateway".to_string(),
        description: format!(
            "A2A gateway fronting {} local agent(s). Select an agent by its interface's \
             `tenant` (the agent id) and send `SendMessage` with that `tenant`.",
            agents.len()
        ),
        version: "1.0.0".to_string(),
        supported_interfaces: interfaces,
        capabilities: AgentCapabilities {
            streaming: Some(false),
            push_notifications: Some(false),
            extensions: None,
            extended_agent_card: Some(false),
        },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills,
        provider: None,
        documentation_url: None,
        icon_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}

/// Dispatch a JSON-RPC 2.0 request body to the A2A method handlers.
fn handle_jsonrpc(
    body: &str,
    runtime: &Arc<LazyLocalSwarmRuntime>,
    registry: &Arc<LocalAgentRegistry>,
    tokio_handle: &tokio::runtime::Handle,
    max_credits_per_dispatch: u32,
) -> JsonRpcResponse {
    let req: JsonRpcRequest = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => {
            return JsonRpcResponse::error(
                JsonRpcId::Null,
                JsonRpcError {
                    code: error_code::PARSE_ERROR,
                    message: format!("invalid JSON-RPC request: {e}"),
                    data: None,
                },
            );
        }
    };

    match req.method.as_str() {
        methods::SEND_MESSAGE => {
            let params = match req.params {
                Some(p) => p,
                None => {
                    return err(
                        &req.id,
                        error_code::INVALID_PARAMS,
                        "SendMessage requires params",
                    );
                }
            };
            let sm_req: SendMessageRequest = match serde_json::from_value(params) {
                Ok(r) => r,
                Err(e) => {
                    return err(
                        &req.id,
                        error_code::INVALID_PARAMS,
                        format!("invalid SendMessage params: {e}"),
                    );
                }
            };
            // Route by `tenant` (the A2A opaque routing identifier = agent id).
            let agent_id = match sm_req.tenant.as_deref().filter(|s| !s.is_empty()) {
                Some(id) => id.to_string(),
                None => {
                    return err(
                        &req.id,
                        error_code::INVALID_PARAMS,
                        "SendMessage requires `tenant` set to the target local agent id",
                    );
                }
            };
            let agent = match registry.get(&agent_id) {
                Some(c) => c,
                None => {
                    return err(
                        &req.id,
                        error_code::INVALID_PARAMS,
                        format!("local agent '{agent_id}' not found"),
                    );
                }
            };
            // Extract text from the message parts (concatenate text parts; v1
            // ignores non-text parts). An empty text is a client error, not a
            // zero-cost dispatch.
            let text: String = sm_req
                .message
                .parts
                .iter()
                .filter_map(|p| p.as_text().map(|s| s.to_string()))
                .collect::<Vec<_>>()
                .join("\n");
            if text.trim().is_empty() {
                return err(
                    &req.id,
                    error_code::INVALID_PARAMS,
                    "message has no text parts",
                );
            }
            let context_id = sm_req.message.context_id;
            let result = tokio_handle.block_on(async {
                let runtime = runtime
                    .get_or_init()
                    .await
                    .map_err(|e| SwarmError::Unavailable(e))?;
                runtime
                    .delegate(&agent, &text, A2A_HTTP_CREDITS, max_credits_per_dispatch)
                    .await
            });
            match result {
                Ok(delegate_result) => {
                    let task = crate::a2a::task_from_response(
                        &delegate_result.response,
                        context_id,
                        &delegate_result.model,
                        delegate_result.tokens_used,
                        delegate_result.cost,
                    );
                    let resp = SendMessageResponse::Task(task);
                    let value = serde_json::to_value(&resp).unwrap_or_else(
                        |_| serde_json::json!({"error": "failed to serialize A2A task"}),
                    );
                    JsonRpcResponse::success(req.id, value)
                }
                Err(e) => err(&req.id, error_code::INTERNAL_ERROR, e.to_string()),
            }
        }

        methods::GET_TASK => JsonRpcResponse::error(
            req.id,
            JsonRpcError {
                code: error_code::TASK_NOT_FOUND,
                message: "no task store: delegate is synchronous and returns the completed Task \
                          inline; GetTask has no persisted task to return"
                    .to_string(),
                data: None,
            },
        ),

        methods::LIST_TASKS => {
            let empty = ListTasksResponse {
                tasks: Vec::new(),
                next_page_token: String::new(),
                page_size: 0,
                total_size: 0,
            };
            match serde_json::to_value(&empty) {
                Ok(v) => JsonRpcResponse::success(req.id, v),
                Err(e) => err(&req.id, error_code::INTERNAL_ERROR, e.to_string()),
            }
        }

        methods::GET_EXTENDED_AGENT_CARD => {
            // No authenticated extended card in v1 — return the same gateway
            // card. `registry` is an `Arc`; deref-borrow for the builder.
            let card = build_gateway_card(registry, "");
            match serde_json::to_value(&card) {
                Ok(v) => JsonRpcResponse::success(req.id, v),
                Err(e) => err(&req.id, error_code::INTERNAL_ERROR, e.to_string()),
            }
        }

        // v1 does not support streaming, cancellation, or push notifications
        // (the card declares streaming=false, push_notifications=false).
        methods::SEND_STREAMING_MESSAGE
        | methods::SUBSCRIBE_TO_TASK
        | methods::CANCEL_TASK
        | methods::CREATE_PUSH_CONFIG
        | methods::GET_PUSH_CONFIG
        | methods::LIST_PUSH_CONFIGS
        | methods::DELETE_PUSH_CONFIG => err(
            &req.id,
            error_code::UNSUPPORTED_OPERATION,
            format!(
                "{} is not supported by the local A2A gateway in v1",
                req.method
            ),
        ),

        _ => err(
            &req.id,
            error_code::METHOD_NOT_FOUND,
            format!("unknown A2A method: {}", req.method),
        ),
    }
}

fn err(id: &JsonRpcId, code: i32, message: impl Into<String>) -> JsonRpcResponse {
    JsonRpcResponse::error(
        id.clone(),
        JsonRpcError {
            code,
            message: message.into(),
            data: None,
        },
    )
}

/// Serialize a `JsonRpcResponse` into a `200` tiny_http JSON response.
fn json_rpc_raw_response(resp: JsonRpcResponse) -> tiny_http::Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&resp)
        .unwrap_or_else(|_| Vec::from(&b"{\"error\":\"rpc serialization failed\"}"[..]));
    tiny_http::Response::from_data(body)
        .with_status_code(200)
        .with_header(
            tiny_http::Header::from_bytes(b"Content-Type", b"application/json")
                .expect("valid header"),
        )
}

/// Serialize an arbitrary JSON body into a tiny_http response with a status code.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_registry::{LocalAgentCapabilities, LocalAgentCard, LocalAgentRegistry};
    use crate::local_runtime::LazyLocalSwarmRuntime;
    use a2a::jsonrpc::methods;

    fn temp_dir(label: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "hkask-a2a-tests-{}-{}",
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().to_string()
    }

    fn registry_with_agent(dir: &str, agent_id: &str) -> Arc<LocalAgentRegistry> {
        let registry = LocalAgentRegistry::new(dir);
        registry
            .write_card(&LocalAgentCard {
                agent_id: agent_id.to_string(),
                agent_type: "test".to_string(),
                description: "a test agent".to_string(),
                accepts: vec![],
                produces: vec![],
                dependencies: Default::default(),
                capabilities: LocalAgentCapabilities::default(),
                cloud_id: None,
            })
            .unwrap();
        Arc::new(registry)
    }

    #[test]
    fn gateway_card_empty_registry_has_placeholder_interface() {
        let dir = temp_dir("empty");
        let registry = LocalAgentRegistry::new(&dir);
        let card = build_gateway_card(&registry, "http://127.0.0.1:9999");
        assert_eq!(card.name, "hKask Local Swarm Gateway");
        assert_eq!(card.supported_interfaces.len(), 1);
        assert!(card.supported_interfaces[0].tenant.is_none());
        assert!(card.skills.is_empty());
        assert_eq!(card.capabilities.streaming, Some(false));
        assert_eq!(card.capabilities.push_notifications, Some(false));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gateway_card_advertises_one_interface_per_agent_with_tenant() {
        let dir = temp_dir("agents");
        let registry = registry_with_agent(&dir, "agent_a");
        let card = build_gateway_card(&registry, "http://127.0.0.1:9999");
        assert_eq!(card.supported_interfaces.len(), 1);
        assert_eq!(
            card.supported_interfaces[0].tenant.as_deref(),
            Some("agent_a")
        );
        assert_eq!(card.supported_interfaces[0].url, "http://127.0.0.1:9999");
        assert_eq!(card.skills.len(), 1);
        assert_eq!(card.skills[0].id, "agent_a");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Build a JSON-RPC request body string for the given method + params JSON.
    fn rpc_body(id: &str, method: &str, params: Option<&str>) -> String {
        let params = match params {
            Some(p) => format!(",\"params\":{p}"),
            None => String::new(),
        };
        format!("{{\"jsonrpc\":\"2.0\",\"id\":\"{id}\",\"method\":\"{method}\"{params}}}")
    }

    fn dispatch(body: &str, registry: &Arc<LocalAgentRegistry>) -> JsonRpcResponse {
        let runtime = Arc::new(LazyLocalSwarmRuntime::lazy(
            "/tmp/hkask-a2a-test-unused-ledger.db".to_string(),
        ));
        let handle = tokio::runtime::Runtime::new().unwrap();
        let handle = handle.handle().clone();
        handle_jsonrpc(body, &runtime, registry, &handle, 50)
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let dir = temp_dir("unknown");
        let registry = Arc::new(LocalAgentRegistry::new(&dir));
        let resp = dispatch(&rpc_body("1", "NopeMethod", None), &registry);
        let err = resp.error.expect("expected an error");
        assert_eq!(err.code, error_code::METHOD_NOT_FOUND);
        assert_eq!(resp.id, JsonRpcId::String("1".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn get_task_returns_task_not_found() {
        let dir = temp_dir("gettask");
        let registry = Arc::new(LocalAgentRegistry::new(&dir));
        let resp = dispatch(
            &rpc_body("2", methods::GET_TASK, Some(r#"{"id":"t-1"}"#)),
            &registry,
        );
        let err = resp.error.expect("expected an error");
        assert_eq!(err.code, error_code::TASK_NOT_FOUND);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_tasks_returns_empty_success() {
        let dir = temp_dir("listtasks");
        let registry = Arc::new(LocalAgentRegistry::new(&dir));
        let resp = dispatch(&rpc_body("3", methods::LIST_TASKS, Some("{}")), &registry);
        assert!(resp.error.is_none());
        let result = resp.result.expect("expected a result");
        assert_eq!(result["tasks"].as_array().unwrap().len(), 0);
        assert_eq!(result["totalSize"].as_i64(), Some(0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn send_message_without_tenant_is_invalid_params() {
        let dir = temp_dir("notenant");
        let registry = registry_with_agent(&dir, "agent_a");
        // SendMessage with a text message but no `tenant`.
        let params = r#"{"message":{"messageId":"m1","role":"user","parts":[{"text":"hi"}]}}"#;
        let resp = dispatch(
            &rpc_body("4", methods::SEND_MESSAGE, Some(params)),
            &registry,
        );
        let err = resp.error.expect("expected an error");
        assert_eq!(err.code, error_code::INVALID_PARAMS);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn send_message_unknown_agent_is_invalid_params() {
        let dir = temp_dir("unknownagent");
        let registry = registry_with_agent(&dir, "agent_a");
        let params = r#"{"message":{"messageId":"m1","role":"user","parts":[{"text":"hi"}]},"tenant":"nope"}"#;
        let resp = dispatch(
            &rpc_body("5", methods::SEND_MESSAGE, Some(params)),
            &registry,
        );
        let err = resp.error.expect("expected an error");
        assert_eq!(err.code, error_code::INVALID_PARAMS);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn streaming_and_cancel_are_unsupported() {
        let dir = temp_dir("unsupported");
        let registry = Arc::new(LocalAgentRegistry::new(&dir));
        for method in [
            methods::SEND_STREAMING_MESSAGE,
            methods::SUBSCRIBE_TO_TASK,
            methods::CANCEL_TASK,
            methods::CREATE_PUSH_CONFIG,
        ] {
            let resp = dispatch(&rpc_body("6", method, None), &registry);
            let err = resp.error.expect("expected an error");
            assert_eq!(
                err.code,
                error_code::UNSUPPORTED_OPERATION,
                "method {method}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
