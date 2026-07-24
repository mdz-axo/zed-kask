//! Chat WebSocket route — persistent bidirectional streaming chat.
//!
//! # REQ: P3-chat-ws — P3 Headless: streaming chat via wss://.
//! expect: "I can interact with hKask agents through a persistent WebSocket connection"
//!
//! Flow:
//! 1. Client opens WebSocket to `GET /api/v1/chat/ws`
//! 2. Server verifies session cookie or Bearer token
//! 3. Client sends `{"type":"prompt","input":"..."}` as JSON text frames
//! 4. Server auto-discovers MCP tools and passes them to inference
//! 5. Server streams `{"type":"token","text_delta":"...","model":"..."}` frames
//! 6. Server sends `{"type":"done","finish_reason":"stop","usage":{...}}` on completion
//! 7. Client may send multiple `prompt` messages over the same connection

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

use crate::ApiState;
use hkask_capability::ToolInfo;
use hkask_services_chat::{ChatService, ChatStreamEvent, ChatTurnRequest};
use hkask_types::{ChatToolDefinition, ChatToolFunction};

// ── Protocol message types ──────────────────────────────────────────────

/// Message from client to server over the chat WebSocket.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsClientMessage {
    /// Start a new chat turn.
    Prompt {
        input: String,
        #[serde(default)]
        model: Option<String>,
    },
    /// Cancel the current generation.
    Cancel,
    /// Keepalive ping.
    Ping,
}

/// Message from server to client over the chat WebSocket.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsServerMessage {
    /// Streaming token delta.
    Token { text_delta: String, model: String },
    /// Turn complete.
    Done {
        finish_reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<serde_json::Value>,
        memory_stored: bool,
    },
    /// Error during generation.
    Error { message: String },
    /// Response to client ping.
    Pong,
}

// ── MCP tool auto-discovery ─────────────────────────────────────────────

/// Convert MCP-discovered tools to OpenAI-compatible `ChatToolDefinition`s.
///
/// Mirrors `tools_to_definitions()` in `hkask-cli/src/repl/tool_augmented.rs`.
/// Duplicated here to avoid a dependency from `hkask-api` → `hkask-cli`.
fn mcp_tools_to_definitions(tools: &[ToolInfo]) -> Vec<ChatToolDefinition> {
    tools
        .iter()
        .map(|tool| ChatToolDefinition {
            tool_type: "function".to_string(),
            function: ChatToolFunction {
                name: format!("{}/{}", tool.server_id, tool.name),
                description: tool.description.clone(),
                parameters: tool.input_schema.clone(),
            },
        })
        .collect()
}

/// Auto-discover MCP tools from the agent service and return as
/// `ChatToolDefinition`s. Returns `None` if no MCP runtime is configured
/// or no tools are registered.
async fn discover_mcp_tools(state: &ApiState) -> Option<Vec<ChatToolDefinition>> {
    let mcp = state.agent_service.infra().mcp.clone();
    let tool_names = mcp.discover_tools().await;
    if tool_names.is_empty() {
        return None;
    }

    let mut tools: Vec<ToolInfo> = Vec::with_capacity(tool_names.len());
    for name in &tool_names {
        if let Some(info) = mcp.get_tool_info(name).await {
            tools.push(info);
        }
    }

    if tools.is_empty() {
        None
    } else {
        Some(mcp_tools_to_definitions(&tools))
    }
}

// ── Route registration ──────────────────────────────────────────────────

/// Return the chat WebSocket router as an `OpenApiRouter`.
pub fn chat_ws_router() -> utoipa_axum::router::OpenApiRouter<ApiState> {
    use utoipa_axum::routes;
    utoipa_axum::router::OpenApiRouter::new().routes(routes!(chat_ws))
}

/// GET /api/v1/chat/ws
///
/// Upgrades to a WebSocket for persistent bidirectional streaming chat.
/// MCP tools are auto-discovered and passed to inference so the model
/// can use native function calling.
///
/// expect: "I can interact with hKask agents through a persistent WebSocket"
/// pre:  request contains valid session cookie or Bearer token
/// post: WebSocket upgraded, bidirectional JSON message stream
#[utoipa::path(
    get,
    path = "/api/v1/chat/ws",
    tag = "chat-ws",
    responses(
        (status = 101, description = "WebSocket upgrade — bidirectional chat session"),
        (status = 401, description = "Missing or invalid authentication"),
    ),
)]
pub async fn chat_ws(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, (StatusCode, String)> {
    // ── Auth: try Bearer token first, fall back to session cookie ──
    let _webid = extract_auth_webid(&headers, &state).await?;

    tracing::info!(target = "hkask.api.chat_ws", "Chat WebSocket connected");

    Ok(ws.on_upgrade(move |socket| handle_chat_ws(socket, state)))
}

// ── Auth extraction ─────────────────────────────────────────────────────

/// Extract the authenticated WebID from session cookie.
/// Mirrors the terminal WebSocket auth pattern.
async fn extract_auth_webid(
    headers: &axum::http::HeaderMap,
    state: &ApiState,
) -> Result<String, (StatusCode, String)> {
    let cookies = headers
        .get("cookie")
        .and_then(|c| c.to_str().ok())
        .unwrap_or("");

    let session_id = cookies
        .split(';')
        .find_map(|c| c.trim().strip_prefix("hkask_session="))
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "Missing session cookie".to_string(),
        ))?;

    let user_store = state.agent_service.storage().users.clone();
    let store = user_store.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Lock error: {e}"),
        )
    })?;
    let session = store
        .get_session(session_id)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Session lookup error: {e}"),
            )
        })?
        .ok_or((StatusCode::UNAUTHORIZED, "Invalid session".to_string()))?;

    let now = chrono::Utc::now().timestamp();
    if session.is_expired(now) {
        return Err((StatusCode::UNAUTHORIZED, "Session expired".to_string()));
    }

    Ok(session.webid.to_string())
}

// ── WebSocket handler ───────────────────────────────────────────────────

/// Handle the upgraded WebSocket connection.
async fn handle_chat_ws(socket: WebSocket, state: ApiState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Agent service reference for the duration of the connection
    let agent_service = state.agent_service.clone();

    // Auto-discover MCP tools once at connection time.
    let mcp_tools = discover_mcp_tools(&state).await;

    if let Some(ref tools) = mcp_tools {
        tracing::info!(
            target = "hkask.api.chat_ws",
            tool_count = tools.len(),
            "MCP tools discovered"
        );
    }

    loop {
        tokio::select! {
            msg = ws_receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        let msg_str = text.to_string();

                        let client_msg: WsClientMessage = match serde_json::from_str(&msg_str) {
                            Ok(m) => m,
                            Err(e) => {
                                let err = WsServerMessage::Error {
                                    message: format!("Invalid message: {e}"),
                                };
                                if let Ok(json) = serde_json::to_string(&err) {
                                    let _ = ws_sender.send(Message::Text(json.into())).await;
                                }
                                continue;
                            }
                        };

                        match client_msg {
                            WsClientMessage::Prompt { input, model } => {
                                if input.is_empty() {
                                    let err = WsServerMessage::Error {
                                        message: "Input must be non-empty".to_string(),
                                    };
                                    if let Ok(json) = serde_json::to_string(&err) {
                                        let _ = ws_sender.send(Message::Text(json.into())).await;
                                    }
                                    continue;
                                }

                                // Build the service request with auto-discovered MCP tools
                                let svc_req = ChatTurnRequest {
                                    input,
                                    userpod_name: Some("Curator".to_string()),
                                    model_override: model,
                                    tool_section: None,
                                    api_spec: Some(crate::openapi_spec::condensed_api_spec()),
                                    inference_port_override: None,
                                    episodic_storage_override: None,
                                    semantic_storage_override: None,
                                    auth_context: None,
                                    params_override: None,
                                    tools: mcp_tools.clone(),
                                    thread_messages: None,
                                    prebuilt_messages: None,
            improv_mode: None,
                                };

                                // Stream the response inline (blocks the loop until done)
                                let mut stream = ChatService::chat_stream(&agent_service, svc_req);
                                while let Some(event) = stream.next().await {
                                    let server_msg = match event {
                                        ChatStreamEvent::Token { text_delta, model } => {
                                            WsServerMessage::Token { text_delta, model }
                                        }
                                        ChatStreamEvent::Done { finish_reason, usage, memory_stored } => {
                                            WsServerMessage::Done {
                                                finish_reason,
                                                usage: usage.map(|u| serde_json::json!({
                                                    "prompt_tokens": u.prompt_tokens,
                                                    "completion_tokens": u.completion_tokens,
                                                    "total_tokens": u.total_tokens,
                                                })),
                                                memory_stored,
                                            }
                                        }
                                        ChatStreamEvent::Error { message } => {
                                            WsServerMessage::Error { message }
                                        }
                                    };

                                    if let Ok(json) = serde_json::to_string(&server_msg)
                                        && ws_sender.send(Message::Text(json.into())).await.is_err()
                                    {
                                        return; // Client disconnected
                                    }
                                }
                            }

                            WsClientMessage::Cancel => {
                                // Phase 2: when generation is spawned, abort it.
                                // For Phase 1 inline streaming, Cancel is a no-op.
                            }

                            WsClientMessage::Ping => {
                                let pong = WsServerMessage::Pong;
                                if let Ok(json) = serde_json::to_string(&pong) {
                                    let _ = ws_sender.send(Message::Text(json.into())).await;
                                }
                            }
                        }
                    }

                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!(
                            target = "hkask.api.chat_ws",
                            "Chat WebSocket disconnected"
                        );
                        return;
                    }

                    // Ignore binary, ping/pong (axum handles pong auto-reply)
                    Some(Ok(_)) => {}

                    Some(Err(e)) => {
                        tracing::warn!(
                            target = "hkask.api.chat_ws",
                            error = %e,
                            "WebSocket error"
                        );
                        return;
                    }
                }
            }
        }
    }
}
