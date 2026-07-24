//! hkask-acp — ACP (Agent Client Protocol) userpod binary
//!
//! Presents hKask coding agents in IDEs (Zed, VS Code, etc.) via the
//! Agent Client Protocol (agentclientprotocol.com). The userpod:
//!
//! 1. Connects to the hKask daemon for auth, capability, and memory
//! 2. Implements ACP JSON-RPC 2.0 over stdio
//! 3. Routes prompts through hKask's inference router
//! 4. Encodes interactions as episodic memory h_mems
//! 5. Registers Regulation spans for observability
//!
//! The ACP wire protocol is JSON-RPC 2.0 over stdin/stdout. Message types
//! follow the Agent Client Protocol specification v1. When the
//! `agent-client-protocol` crate's simpler `Agent` trait ships, this
//! manual implementation will be replaced by the trait impl.
//!
//! # Startup
//!
//! ```text
//! HKASK_MCP_HOST=<name> hkask-acp
//! ```

pub mod protocol;

use hkask_inference::{InferenceConfig, InferenceRouter, model_constants};
use hkask_mcp_server::daemon::DaemonClient;
use hkask_mcp_server::startup::verify_startup_gates;
use hkask_types::template::LLMParameters;
use hkask_types::{InferencePort, InferenceStreamChunk};
use protocol::*;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::cloud::CloudClient;

const ENV_USERPOD: &str = "HKASK_MCP_HOST";

/// Error type for hkask-acp library operations.
#[derive(Debug, Error)]
pub enum AcpError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Inference(String),
}

pub struct SessionState {
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  session_id is a non-empty UUID string
    /// post: holds session identifier for request routing
    pub session_id: String,
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  cwd is a valid filesystem path
    /// post: holds working directory for the session
    pub cwd: String,
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  created_at is a valid Unix timestamp
    /// post: holds session creation time
    pub created_at: i64,
}

pub struct HkaskAcpAgent {
    userpod: String,
    daemon: Option<DaemonClient>,
    /// Cloud gateway client — used when HKASK_CLOUD_GATEWAY is configured.
    cloud: Option<CloudClient>,
    /// Human-readable status message if daemon connection failed.
    daemon_error: Option<String>,
    inference: Arc<dyn InferencePort>,
    default_model: String,
    pub sessions: Mutex<HashMap<String, SessionState>>,
}

impl HkaskAcpAgent {
    /// Production constructor — connects to daemon. Never fails.
    /// If the daemon is unreachable, the agent starts in degraded mode
    /// and returns actionable errors to the IDE on each request.
    async fn build() -> Self {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "hkask.acp=info".into()),
            )
            .init();

        let userpod = std::env::var(ENV_USERPOD).unwrap_or_else(|_| {
            warn!("HKASK_MCP_HOST not set, using default 'acp-userpod'");
            "acp-userpod".to_string()
        });

        // Try cloud gateway first (HKASK_CLOUD_GATEWAY set)
        let cloud = match CloudClient::from_env() {
            Ok(Some(c)) => {
                info!(
                    target: "hkask.acp",
                    userpod = %userpod,
                    "Connected via cloud gateway"
                );
                Some(c)
            }
            Ok(None) => None,
            Err(e) => {
                warn!(target: "hkask.acp", userpod = %userpod, error = %e, "Cloud gateway config error — falling back to local daemon");
                None
            }
        };

        // Fall back to local daemon if no cloud gateway
        let (daemon, daemon_error) = if cloud.is_some() {
            (None, None)
        } else {
            let daemon_client = DaemonClient::new();
            match verify_startup_gates(&daemon_client, &userpod, "acp", &["inference:call"]).await {
                Ok(gate_result) => {
                    info!(
                        target: "hkask.acp",
                        userpod = %userpod,
                        "P4 gates verified — {} tool(s) denied: {:?}",
                        gate_result.denied_tools.len(),
                        gate_result.denied_tools
                    );
                    reg_emit_acp("reg.acp.ide.connection_state", &userpod, "connected");
                    (Some(daemon_client), None)
                }
                Err(e) => {
                    let msg = format!(
                        "hKask daemon unavailable: {}. Start it with: kask daemon start",
                        e
                    );
                    warn!(target: "hkask.acp", userpod = %userpod, error = %msg);
                    reg_emit_acp("reg.acp.ide.connection_state", &userpod, "degraded");
                    (None, Some(msg))
                }
            }
        };

        let inference: Arc<dyn InferencePort> = Arc::new(hkask_guard::GuardedInferencePort::new(
            Arc::new(InferenceRouter::new(InferenceConfig::from_env())) as Arc<dyn InferencePort>,
            hkask_guard::ContentGuard::mandatory(&hkask_guard::GuardConfig::from_env()),
        ));

        // P9: Fusion cost-safety — log the configuration on startup.
        // With the plugin-based approach (explicit panel + judge models),
        // there is no risk of OpenRouter defaulting to ALL models.
        // Fusion is transparently active when OpenRouter is configured.
        {
            let config = InferenceConfig::from_env();
            if let Some(ref fusion) = config.fusion {
                tracing::info!(
                    target: "reg.inference",
                    fusion_judge = %fusion.judge,
                    fusion_panel = ?fusion.panel,
                    "ACP: fusion configured"
                );
            }
        }

        let default_model = std::env::var("HKASK_ACP_MODEL")
            .unwrap_or_else(|_| model_constants::DEFAULT_FALLBACK_MODEL.to_string());

        Self {
            userpod,
            daemon,
            cloud,
            daemon_error,
            inference,
            default_model,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Test constructor — uses provided inference port, no daemon.
    ///
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  inference is a valid `Arc<dyn InferencePort>`
    /// post: returns HkaskAcpAgent in test mode with no daemon connection
    pub fn for_testing(inference: Arc<dyn InferencePort>) -> Self {
        Self {
            userpod: "test-userpod".into(),
            daemon: None,
            cloud: None,
            daemon_error: None,
            inference,
            default_model: "test-model".into(),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Set the default model for inference.
    ///
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  model is a non-empty model name string
    /// post: default_model set; returns Self for builder chaining
    pub fn with_model(mut self, model: &str) -> Self {
        self.default_model = model.to_string();
        self
    }

    /// Whether the daemon is connected and ready.
    fn daemon_ready(&self) -> bool {
        self.daemon.is_some()
    }

    /// Run inference stream — process prompt through LLM, dispatch tool calls, emit to stdout.
    ///
    /// expect: "The ACP userpod provides IDE agent presence"
    /// pre:  prompt is non-empty; session_id is valid; stdout is writable
    /// post: returns Ok(stop_reason) on completion; streams ACP JSON notifications to stdout
    /// post: encodes prompt + response as episodic memory h_mems if daemon is connected
    pub async fn run_inference_stream(
        &self,
        prompt: &str,
        session_id: &str,
        stdout: &mut (impl tokio::io::AsyncWrite + Unpin),
    ) -> Result<String, AcpError> {
        use futures_util::StreamExt;

        let params = LLMParameters {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            max_tokens: 4096,
            ..Default::default()
        };

        let port: Arc<dyn InferencePort> = Arc::clone(&self.inference) as Arc<dyn InferencePort>;
        let start = std::time::Instant::now();
        let mut stream =
            port.generate_stream_with_model(prompt, &params, Some(&self.default_model), None);

        let mut total_text = String::new();
        let mut finish_reason = String::from("end_turn");
        let mut total_tokens = 0u32;
        let message_id = format!("msg-{}", uuid::Uuid::new_v4());
        let mut tool_call_counter = 0u32;

        while let Some(chunk_result) = stream.next().await {
            let chunk: InferenceStreamChunk =
                chunk_result.map_err(|e| AcpError::Inference(format!("Stream error: {}", e)))?;

            // Tool calls in this chunk — dispatch via daemon or report only
            for tc in &chunk.tool_calls {
                tool_call_counter += 1;
                let tc_id = tc
                    .call_id
                    .as_deref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("tc-{}-{}", session_id, tool_call_counter));
                let kind = map_tool_kind(tc);
                let title = format!("{} {}", tc.server, tc.tool);

                // Notify: pending
                let notif = tool_call_notification(session_id, &tc_id, &title, &kind);
                write_notification(stdout, &notif).await?;

                // Notify: in_progress
                let update = tool_call_update(session_id, &tc_id, "in_progress", None);
                write_notification(stdout, &update).await?;

                // Dispatch tool call via cloud gateway or daemon
                let result_text = if let Some(ref cloud) = self.cloud {
                    let tool_name = format!("{}:{}", tc.server, tc.tool);
                    match cloud.dispatch_tool(&tool_name, &tc.args).await {
                        Ok((true, Some(ref out), _)) => format!("{}", out),
                        Ok((false, _, Some(ref err))) => format!("Error: {}", err),
                        Err(e) => format!("Cloud error: {e}"),
                        _ => "Unexpected response".into(),
                    }
                } else if let Some(ref daemon) = self.daemon {
                    let tool_name = format!("{}:{}", tc.server, tc.tool);
                    match daemon
                        .tool_dispatch(&self.userpod, &tool_name, &tc.args)
                        .await
                    {
                        Ok(hkask_mcp_server::daemon::DaemonResponse::ToolDispatchResponse {
                            ok: true,
                            output: Some(ref out),
                            ..
                        }) => format!("{}", out),
                        Ok(hkask_mcp_server::daemon::DaemonResponse::ToolDispatchResponse {
                            ok: false,
                            error: Some(ref err),
                            ..
                        }) => format!("Error: {}", err),
                        Err(e) => format!("Dispatch error: {}", e),
                        _ => "Unexpected response".into(),
                    }
                } else {
                    format!("Tool call: {} {} (no daemon)", tc.server, tc.tool)
                };

                // Notify: completed with result
                let update = tool_call_update(session_id, &tc_id, "completed", Some(&result_text));
                write_notification(stdout, &update).await?;
            }

            // Text content
            if !chunk.text_delta.is_empty() {
                total_text.push_str(&chunk.text_delta);
                let notif = agent_message_chunk(session_id, &message_id, &chunk.text_delta);
                write_notification(stdout, &notif).await?;
            }

            // Track usage and finish reason from final chunk
            if let Some(ref usage) = chunk.usage {
                total_tokens = usage.total_tokens;
            }
            if let Some(ref fr) = chunk.finish_reason {
                finish_reason = fr.clone();
            }
        }

        let latency_ms = start.elapsed().as_millis() as u64;

        // Usage update notification
        let usage_notif = usage_update(session_id, total_tokens, total_tokens);
        write_notification(stdout, &usage_notif).await?;

        // Empty response guard — tell the user something happened
        if total_text.is_empty() && tool_call_counter == 0 {
            let notif = agent_message_chunk(
                session_id,
                &format!("empty-{}", uuid::Uuid::new_v4()),
                "[No output produced — the model returned an empty response.]",
            );
            write_notification(stdout, &notif).await?;
        }

        info!(
            target: "hkask.acp",
            session_id = %session_id,
            latency_ms = %latency_ms,
            tokens = %total_tokens,
            finish_reason = %finish_reason,
            text_len = total_text.len(),
            tool_calls = tool_call_counter,
            "Inference stream complete"
        );

        // Encode memory — full content, not just metadata (same as REPL)
        if let Some(ref daemon) = self.daemon {
            // Store prompt text
            let _ = daemon
                .store_experience(
                    &self.userpod,
                    &format!("session:{}:prompt", session_id),
                    "text",
                    &serde_json::json!(prompt),
                    Some(0.9),
                )
                .await;

            // Store response text
            let _ = daemon
                .store_experience(
                    &self.userpod,
                    &format!("session:{}:response", session_id),
                    "text",
                    &serde_json::json!(total_text),
                    Some(0.9),
                )
                .await;

            // Store response metadata
            let _ = daemon
                .store_experience(
                    &self.userpod,
                    &format!("session:{}:metadata", session_id),
                    "stats",
                    &serde_json::json!({
                        "tokens": total_tokens,
                        "model": &self.default_model,
                        "finish": &finish_reason,
                        "tool_calls": tool_call_counter,
                    }),
                    Some(0.95),
                )
                .await;
        }

        // Map finish_reason to ACP StopReason
        let stop_reason = match finish_reason.as_str() {
            "stop" | "end_turn" => "end_turn",
            "length" => "max_tokens",
            "tool_calls" => "end_turn",
            _ => "end_turn",
        };

        Ok(stop_reason.to_string())
    }

    // run_inference removed — replaced by run_inference_stream
}

/// Map a StructuredToolCall to an ACP tool kind string.
fn map_tool_kind(tc: &hkask_types::inference_types::StructuredToolCall) -> String {
    match tc.tool.as_str() {
        "web_search" | "brave_search" | "tavily_search" => "search".into(),
        "web_extract" | "fetch" | "scrape" => "fetch".into(),
        "execute" | "run" | "shell" => "execute".into(),
        "read" | "cat" => "read".into(),
        "write" | "edit" | "patch" => "edit".into(),
        "delete" | "rm" => "delete".into(),
        "think" | "reason" | "plan" => "think".into(),
        _ => "other".into(),
    }
}

fn reg_emit_acp(span: &str, userpod: &str, detail: &str) {
    info!(
        target: "hkask.acp",
        reg_span = %span,
        userpod = %userpod,
        detail = %detail,
        "REG"
    );
}

/// Entry point — build agent, serve ACP over stdio until disconnect.
///
/// expect: "The ACP userpod provides IDE agent presence"
/// pre:  HKASK_MCP_HOST env var may be set; cargo build must have succeeded
/// post: ACP JSON-RPC server runs over stdin/stdout until EOF or error
/// post: emits reg.acp.ide.connection_state span on connect and disconnect
pub async fn run() -> Result<(), AcpError> {
    let agent = Arc::new(HkaskAcpAgent::build().await);
    info!(target: "hkask.acp", userpod = %agent.userpod, daemon_ok = agent.daemon_ready(), "ACP userpod starting");

    let mut transport = protocol::StdioTransport::new();
    transport.serve(Arc::clone(&agent)).await?;

    reg_emit_acp(
        "reg.acp.ide.connection_state",
        &agent.userpod,
        "disconnected",
    );
    Ok(())
}
