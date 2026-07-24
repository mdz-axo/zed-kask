//! ACP protocol types and stdio transport.
//!
//! Implements the Agent Client Protocol v1 wire format (JSON-RPC 2.0 over stdio).
//! Message types follow the specification at agentclientprotocol.com.
//!
//! When the `agent-client-protocol` crate stabilizes a simple `Agent` trait,
//! this module will be replaced by the crate's types and transport.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn};

use super::HkaskAcpAgent;

// ── JSON-RPC envelope ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcError {
    code: i32,
    message: String,
}

// ── ACP initialize ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeRequest {
    protocol_version: u32,
    client_capabilities: Option<Value>,
    client_info: Option<ClientInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientInfo {
    name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

fn initialize_response(id: Value, userpod: &str) -> JsonRpcResponse {
    let title = format!("hKask — {}", userpod);
    let result = serde_json::json!({
        "protocolVersion": 1,
        "agentCapabilities": {
            "loadSession": false,
            "promptCapabilities": {
                "image": false,
                "audio": false,
                "embeddedContext": true,
            },
            "mcpCapabilities": {
                "http": false,
                "sse": false,
            },
        },
        "agentInfo": {
            "name": "hkask-acp",
            "title": title,
            "version": env!("CARGO_PKG_VERSION"),
        },
        "authMethods": [],
    });
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

// ── ACP session/new ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewSessionRequest {
    cwd: String,
    #[serde(default)]
    mcp_servers: Vec<Value>,
}

fn new_session_response(id: Value, session_id: &str) -> JsonRpcResponse {
    let result = serde_json::json!({ "sessionId": session_id });
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

// ── ACP session/prompt ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptRequest {
    session_id: String,
    prompt: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "resource")]
    Resource { resource: ResourceContent },
    #[serde(rename = "resource_link")]
    ResourceLink { uri: String, name: Option<String> },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceContent {
    uri: String,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

fn prompt_response(id: Value, stop_reason: &str) -> JsonRpcResponse {
    let result = serde_json::json!({ "stopReason": stop_reason });
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: Some(result),
        error: None,
    }
}

// ── ACP session/cancel (notification) ──────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelNotification {
    session_id: String,
}

// ── ACP session/update (notification, agent → client) ──────────────────

pub(crate) fn agent_message_chunk(
    session_id: &str,
    message_id: &str,
    text: &str,
) -> JsonRpcNotification {
    JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: "session/update".into(),
        params: Some(serde_json::json!({
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "messageId": message_id,
                "content": {
                    "type": "text",
                    "text": text,
                },
            },
        })),
    }
}

pub(crate) fn tool_call_notification(
    session_id: &str,
    tool_call_id: &str,
    title: &str,
    kind: &str,
) -> JsonRpcNotification {
    JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: "session/update".into(),
        params: Some(serde_json::json!({
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": tool_call_id,
                "title": title,
                "kind": kind,
                "status": "pending",
            },
        })),
    }
}

pub(crate) fn tool_call_update(
    session_id: &str,
    tool_call_id: &str,
    status: &str,
    content_text: Option<&str>,
) -> JsonRpcNotification {
    let mut update = serde_json::json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": tool_call_id,
        "status": status,
    });
    if let Some(text) = content_text {
        update["content"] = serde_json::json!([{
            "type": "content",
            "content": { "type": "text", "text": text },
        }]);
    }
    JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: "session/update".into(),
        params: Some(serde_json::json!({
            "sessionId": session_id,
            "update": update,
        })),
    }
}

pub(crate) fn usage_update(session_id: &str, used: u32, size: u32) -> JsonRpcNotification {
    JsonRpcNotification {
        jsonrpc: "2.0".into(),
        method: "session/update".into(),
        params: Some(serde_json::json!({
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "usage_update",
                "used": used,
                "size": size,
            },
        })),
    }
}

fn error_response(id: Value, code: i32, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
        }),
    }
}

pub(crate) async fn write_notification(
    stdout: &mut (impl tokio::io::AsyncWrite + Unpin),
    notif: &JsonRpcNotification,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut json = serde_json::to_string(notif)?;
    json.push('\n');
    stdout.write_all(json.as_bytes()).await?;
    stdout.flush().await
}

// ── Stdio transport ────────────────────────────────────────────────────

pub struct StdioTransport {}

impl StdioTransport {
    /// expect: "The ACP userpod provides IDE agent presence"
    /// post: returns empty StdioTransport ready for serve()
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {}
    }

    /// Serve ACP JSON-RPC 2.0 over stdin/stdout.
    ///
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  agent is fully built (inference + daemon configured)
    /// post: reads JSON-RPC requests from stdin, writes responses to stdout
    /// post: runs until stdin EOF or unrecoverable error
    pub async fn serve(&mut self, agent: Arc<HkaskAcpAgent>) -> Result<(), super::AcpError> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        self.serve_impl(agent, stdin, &mut stdout).await
    }

    /// Test entry point — serves ACP over arbitrary reader/writer.
    ///
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  agent is fully built; reader implements AsyncRead; writer implements AsyncWrite
    /// post: reads JSON-RPC requests from reader, writes responses to writer
    /// post: runs until reader EOF or unrecoverable error
    pub async fn serve_with_streams<R: tokio::io::AsyncRead + Unpin>(
        &mut self,
        agent: Arc<HkaskAcpAgent>,
        reader: R,
        writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    ) -> Result<(), super::AcpError> {
        self.serve_impl(agent, reader, writer).await
    }

    async fn serve_impl<R: tokio::io::AsyncRead + Unpin>(
        &mut self,
        agent: Arc<HkaskAcpAgent>,
        reader: R,
        writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    ) -> Result<(), super::AcpError> {
        let buf_reader = BufReader::new(reader);
        let mut lines = buf_reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    warn!(target: "hkask.acp", "Failed to parse JSON-RPC: {}", e);
                    continue;
                }
            };

            let is_notification = request.id.is_none();
            let response = self.handle_request_impl(&request, &agent, writer).await;

            if is_notification {
                continue;
            }

            let mut json = serde_json::to_string(&response)?;
            json.push('\n');
            writer.write_all(json.as_bytes()).await?;
            writer.flush().await?;
        }

        Ok(())
    }

    async fn handle_request_impl(
        &mut self,
        req: &JsonRpcRequest,
        agent: &Arc<HkaskAcpAgent>,
        stdout: &mut (impl tokio::io::AsyncWrite + Unpin),
    ) -> JsonRpcResponse {
        let id = req.id.clone().unwrap_or(Value::Null);

        match req.method.as_str() {
            "initialize" => {
                info!(target: "hkask.acp", userpod = %agent.userpod, "initialize");
                if let Some(ref err) = agent.daemon_error {
                    return error_response(id, -32000, err);
                }
                initialize_response(id, &agent.userpod)
            }

            "session/new" => {
                if let Some(ref err) = agent.daemon_error {
                    return error_response(id, -32000, err);
                }
                let params: NewSessionRequest =
                    match serde_json::from_value(req.params.clone().unwrap_or(Value::Null)) {
                        Ok(p) => p,
                        Err(e) => {
                            return error_response(id, -32602, &format!("Invalid params: {}", e));
                        }
                    };

                let session_id = format!("acp-{}", uuid::Uuid::new_v4());
                let now = chrono::Utc::now().timestamp();

                {
                    let mut sessions = agent.sessions.lock().await;
                    sessions.insert(
                        session_id.clone(),
                        super::SessionState {
                            session_id: session_id.clone(),
                            cwd: params.cwd.clone(),
                            created_at: now,
                        },
                    );
                }

                info!(
                    target: "hkask.acp",
                    session_id = %session_id,
                    cwd = %params.cwd,
                    "Session created"
                );

                let count = agent.sessions.lock().await.len();
                super::reg_emit_acp(
                    "reg.acp.userpod.memory_size",
                    &agent.userpod,
                    &format!("sessions={}", count),
                );

                new_session_response(id, &session_id)
            }

            "session/prompt" => {
                if let Some(ref err) = agent.daemon_error {
                    return error_response(id, -32000, err);
                }
                let params: PromptRequest =
                    match serde_json::from_value(req.params.clone().unwrap_or(Value::Null)) {
                        Ok(p) => p,
                        Err(e) => {
                            return error_response(id, -32602, &format!("Invalid params: {}", e));
                        }
                    };

                // Build prompt from content blocks, resolving ResourceLinks to local files
                let mut parts: Vec<String> = Vec::new();
                for block in &params.prompt {
                    match block {
                        ContentBlock::Text { text } => {
                            parts.push(text.clone());
                        }
                        ContentBlock::Resource { resource } => {
                            if let Some(ref text) = resource.text {
                                parts.push(format!("File {}:\n{}", resource.uri, text));
                            }
                        }
                        ContentBlock::ResourceLink { uri, name } => {
                            if let Some(path) = uri.strip_prefix("file://") {
                                match tokio::fs::read_to_string(path).await {
                                    Ok(content) => {
                                        let label = name.as_deref().unwrap_or(path);
                                        parts.push(format!("File {}:\n{}", label, content));
                                    }
                                    Err(e) => {
                                        warn!(target: "hkask.acp", uri = %uri, error = %e, "Failed to read ResourceLink");
                                    }
                                }
                            }
                        }
                    }
                }
                let prompt_text = parts.join("\n");

                info!(
                    target: "hkask.acp",
                    session_id = %params.session_id,
                    prompt_len = prompt_text.len(),
                    "Prompt received"
                );

                if prompt_text.is_empty() {
                    return prompt_response(id, "end_turn");
                }

                // Streaming inference — notifications written inline
                match agent
                    .run_inference_stream(&prompt_text, &params.session_id, stdout)
                    .await
                {
                    Ok(stop_reason) => prompt_response(id, &stop_reason),
                    Err(e) => {
                        warn!(target: "hkask.acp", "Inference failed: {}", e);
                        prompt_response(id, "end_turn")
                    }
                }
            }

            "session/cancel" => {
                // Notification — handle but no response expected
                #[allow(clippy::collapsible_if)]
                if let Some(params) = &req.params {
                    if let Ok(cancel) = serde_json::from_value::<CancelNotification>(params.clone())
                    {
                        info!(target: "hkask.acp", session_id = %cancel.session_id, "Session cancelled");
                    }
                }
                // Notifications shouldn't reach here (id is None), but be safe
                JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id,
                    result: Some(Value::Null),
                    error: None,
                }
            }

            _ => {
                warn!(target: "hkask.acp", "Unknown method: {}", req.method);
                error_response(id, -32601, &format!("Method not found: {}", req.method))
            }
        }
    }
}
