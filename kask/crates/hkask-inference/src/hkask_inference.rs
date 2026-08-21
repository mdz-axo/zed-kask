#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Inference — the IPC bridge client + shared inference configuration.
//!
//! In zed-kask, MCP server child processes do not hold API keys. Inference
//! (chat, vision, embed, tool dispatch, worktree spawn, model listing) routes
//! through a Unix-socket IPC bridge back to the zed process, which resolves it
//! via zed's `LanguageModelRegistry` (with zed's configured credentials). This
//! crate provides the MCP-server side of that bridge:
//!
//! - [`InferenceIpcClient`] — `InferencePort` / `ToolDispatchPort` /
//!   `WorktreeSpawnPort` implementation over the Unix socket. Construct it via
//!   [`resolve_inference_port`] / [`resolve_tool_dispatch_port`] /
//!   [`resolve_worktree_spawn_port`], which return an unavailable stub (with a
//!   clear, socket-named error) when `HKASK_INFERENCE_SOCKET` is not set.
//! - [`InferenceConfig`] — shared configuration (base URLs, default model).
//! - [`ProviderId`] — provider-prefix routing enum.
//! - [`model_constants`] — env-overridable default model ids.
//! - [`openai_compat::sanitize_error_body`] — response-body redaction shared
//!   across inference/MCP provider error paths.
//!
//! # Model Naming
//!
//! Model ids are provider-prefixed (e.g. `OpenRouter/z-ai/glm-5.2`,
//! `ollama/nomic-embed-text`, `RunPod/kask-ocr`). The prefix selects the
//! provider in zed's `LanguageModelRegistry`; an unprefixed name uses the
//! default model (configurable, default: `OpenRouter/z-ai/glm-5.2`).

pub mod config;
pub mod inference_ipc_client;
pub mod model_constants;
pub mod openai_compat;

// Re-exports — public API
pub use config::{InferenceConfig, ProviderConfig, ProviderId};
pub use inference_ipc_client::InferenceIpcClient;

/// Resolve the best available `InferencePort` for an MCP server.
///
/// This is the canonical entry point for MCP servers at startup. It tries
/// the IPC bridge first (connecting back to zed's `LanguageModelRegistry`
/// from env-var API keys when the IPC socket is not available.
///
/// # Priority
///
/// 1. `InferenceIpcClient` — if `HKASK_INFERENCE_SOCKET` is set and the
///    socket is reachable. This routes inference through zed's
///    `LanguageModelRegistry` (with guard and zed's configured
///    API keys). Chat, vision, embed, and list_models all go through here.
///    This handles only media generation. Chat/vision/
///    embed return a clear error directing the operator to the IPC bridge.
///    Used when running standalone or when the IPC socket is not available.
///
/// # Logs
///
/// Logs which path was taken at `info` level so operators can verify the
/// inference routing from server startup logs.
///
/// pre:  none (reads env vars)
/// post: returns an `Arc<dyn InferencePort>` ready for inference calls
#[must_use]
pub async fn resolve_inference_port() -> std::sync::Arc<dyn hkask_types::InferencePort> {
    match InferenceIpcClient::from_env().await {
        Some(Ok(client)) => {
            tracing::info!(
                target: "hkask.inference",
                "MCP inference routed through zed IPC bridge (HKASK_INFERENCE_SOCKET)"
            );
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::InferencePort>
        }
        Some(Err(e)) => {
            tracing::warn!(
                target: "hkask.inference",
                error = %e,
                "IPC bridge connection failed — inference unavailable (the zed process is required)"
            );
            std::sync::Arc::new(UnavailableInference)
        }
        None => {
            tracing::info!(
                target: "hkask.inference",
                "HKASK_INFERENCE_SOCKET not set — inference unavailable (the zed process is required)"
            );
            std::sync::Arc::new(UnavailableInference)
        }
    }
}

/// Inference stub for MCP servers without the IPC bridge. Every method
/// returns a clear error naming the missing socket so callers can
/// distinguish "dispatch unavailable" from other failures.
struct UnavailableInference;

impl hkask_types::InferencePort for UnavailableInference {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &hkask_types::template::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Connection(
                "inference unavailable: HKASK_INFERENCE_SOCKET not set or IPC bridge unreachable —                  inference requires the zed process".to_string(),
            ))
        })
    }
}

/// Resolve the tool-dispatch port for an MCP server.
///
/// Returns the IPC-bridge client when the socket is available (the zed
/// process dispatches governed tools through its `McpRuntime`), or a stub
/// that returns a clear error. Unlike `resolve_inference_port` there is no
/// media fallback — tool dispatch only exists on the zed side.
///
/// Mirrors `resolve_inference_port`'s structure so MCP servers resolve both
/// ports in one startup step (e.g. `hkask-mcp-swarm`'s local delegate loop
/// reads `Arc<dyn ToolDispatchPort>`).
#[must_use]
pub async fn resolve_tool_dispatch_port() -> std::sync::Arc<dyn hkask_types::ToolDispatchPort> {
    match InferenceIpcClient::from_env().await {
        Some(Ok(client)) => {
            tracing::info!(
                target: "hkask.inference",
                "MCP tool dispatch routed through zed IPC bridge (HKASK_INFERENCE_SOCKET)"
            );
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::ToolDispatchPort>
        }
        Some(Err(e)) => {
            tracing::warn!(
                target: "hkask.inference",
                error = %e,
                "IPC bridge connection failed — tool dispatch unavailable"
            );
            std::sync::Arc::new(UnavailableToolDispatch)
        }
        None => {
            tracing::info!(
                target: "hkask.inference",
                "HKASK_INFERENCE_SOCKET not set — tool dispatch unavailable"
            );
            std::sync::Arc::new(UnavailableToolDispatch)
        }
    }
}

/// Tool-dispatch stub for MCP servers without the IPC bridge. Returns a
/// clear error naming the missing socket so the caller can distinguish
/// "dispatch unavailable" from "tool not found".
struct UnavailableToolDispatch;

impl hkask_types::ToolDispatchPort for UnavailableToolDispatch {
    fn invoke_tool<'a>(
        &'a self,
        _server: &'a str,
        _tool: &'a str,
        _args: serde_json::Value,
        _allowed: &'a [String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<serde_json::Value, hkask_types::InferenceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Connection(
                "tool dispatch unavailable: HKASK_INFERENCE_SOCKET not set or IPC bridge unreachable — \
                 MCP tool calls from delegated agents require the zed process".to_string(),
            ))
        })
    }
}

/// Resolve the worktree-spawn port from the zed IPC bridge. Returns an
/// `UnavailableWorktreeSpawn` stub when the socket is absent or unreachable —
/// the MCP server falls back to in-memory `LazyLocalSwarmRuntime::delegate()`.
pub async fn resolve_worktree_spawn_port() -> std::sync::Arc<dyn hkask_types::WorktreeSpawnPort> {
    match InferenceIpcClient::from_env().await {
        Some(Ok(client)) => {
            tracing::info!(
                target: "hkask.inference",
                "MCP worktree spawn routed through zed IPC bridge (HKASK_INFERENCE_SOCKET)"
            );
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::WorktreeSpawnPort>
        }
        Some(Err(e)) => {
            tracing::warn!(
                target: "hkask.inference",
                error = %e,
                "IPC bridge connection failed — worktree spawn unavailable, falling back to in-memory spawn"
            );
            std::sync::Arc::new(UnavailableWorktreeSpawn)
        }
        None => {
            tracing::info!(
                target: "hkask.inference",
                "HKASK_INFERENCE_SOCKET not set — worktree spawn unavailable, falling back to in-memory spawn"
            );
            std::sync::Arc::new(UnavailableWorktreeSpawn)
        }
    }
}

/// Worktree-spawn stub for MCP servers without the IPC bridge. Returns an
/// error so `kanban_task_spawn` falls back to `LazyLocalSwarmRuntime`.
pub struct UnavailableWorktreeSpawn;

impl hkask_types::WorktreeSpawnPort for UnavailableWorktreeSpawn {
    fn create_worktree_thread<'a>(
        &'a self,
        _prompt: &'a str,
        _title: &'a str,
        _worktree_name: Option<&'a str>,
        _base_ref: Option<&'a str>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<String, hkask_types::InferenceError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Connection(
                "HKASK_INFERENCE_SOCKET not set or IPC bridge unreachable — worktree spawn requires the zed process"
                    .to_string(),
            ))
        })
    }
}
