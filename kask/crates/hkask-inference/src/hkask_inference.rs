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
pub use config::{InferenceConfig, ProviderId};
pub use inference_ipc_client::InferenceIpcClient;

/// The shared "no IPC bridge" reason. Every stub method names the socket so the
/// operator can distinguish "not configured" from "configured but broken" — a
/// missing socket is never reported as an empty success (the `.rules`
/// broken-feedback-loop trap: `unwrap_or(0)`-class, where a broken bridge would
/// read as "no models" / "no results").
const IPC_BRIDGE_UNAVAILABLE: &str =
    "HKASK_INFERENCE_SOCKET not set or IPC bridge unreachable — the zed process is required";

/// One IPC-bridge connection attempt, with the result logged once. The match
/// and logging live here so the three `resolve_*_port` entry points do not
/// triplicate them.
///
/// Returns `Some(client)` when the bridge connected, `None` when the socket is
/// unset or unreachable. The `None` carries no payload — every caller returns its
/// own socket-named stub — so `Option` is the exact type; a bespoke enum would be
/// speculative generality (`Unavailable` had no fields).
async fn connect_bridge(label: &str) -> Option<InferenceIpcClient> {
    match InferenceIpcClient::from_env().await {
        Some(Ok(client)) => {
            tracing::info!(
                target: "hkask.inference",
                "{label} routed through zed IPC bridge (HKASK_INFERENCE_SOCKET)"
            );
            Some(client)
        }
        Some(Err(e)) => {
            tracing::warn!(
                target: "hkask.inference",
                error = %e,
                "IPC bridge connection failed — {label} unavailable ({IPC_BRIDGE_UNAVAILABLE})"
            );
            None
        }
        None => {
            tracing::info!(
                target: "hkask.inference",
                "{label} unavailable ({IPC_BRIDGE_UNAVAILABLE})"
            );
            None
        }
    }
}

/// Resolve the best available `InferencePort` for an MCP server.
///
/// Routes through the zed IPC bridge ([`InferenceIpcClient`]) when
/// `HKASK_INFERENCE_SOCKET` is set and reachable; otherwise returns an
/// [`UnavailableInference`] stub whose **every** method returns a clear,
/// socket-named error — never an empty success. In particular `list_models`
/// returns `Err` (not `Ok(Vec::new())`) so a missing bridge is not misread as
/// an empty model registry.
///
/// `pre`: none (reads env vars). `post`: an `Arc<dyn InferencePort>` ready for
/// inference calls.
#[must_use]
pub async fn resolve_inference_port() -> std::sync::Arc<dyn hkask_types::InferencePort> {
    match connect_bridge("MCP inference").await {
        Some(client) => {
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::InferencePort>
        }
        None => std::sync::Arc::new(UnavailableInference),
    }
}

/// Inference stub for MCP servers without the IPC bridge. Every method returns
/// a clear, socket-named error so callers can distinguish "dispatch
/// unavailable" from "no models" / "backend doesn't implement vision" / other
/// failures. The trait's default impls are overridden for `generate_vision`,
/// `embed`, and `list_models` specifically because their defaults are **not**
/// socket-named: `list_models` defaults to `Ok(Vec::new())` (a broken bridge
/// read as an empty registry), `generate_vision` to a generic
/// `VisionUnsupported`, and `embed` to a generic `Connection`. Overriding them
/// keeps the "every method names the missing socket" contract honest.
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
            Err(hkask_types::InferenceError::Connection(format!(
                "inference unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
    }

    fn generate_vision(
        &self,
        _prompt: &str,
        _images: &[String],
        _parameters: &hkask_types::template::LLMParameters,
        _model_override: Option<&str>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Connection(format!(
                "vision inference unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
    }

    fn embed<'a>(&'a self, _model: &str, _texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        Box::pin(async {
            Err(hkask_types::EmbeddingGenerationError::Connection(format!(
                "embed unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
    }

    fn list_models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Vec<hkask_types::ModelEntry>, hkask_types::InferenceError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::InferenceError::Connection(format!(
                "list_models unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
    }
}

/// Resolve the tool-dispatch port for an MCP server.
///
/// Returns the IPC-bridge client when the socket is available (the zed process
/// dispatches governed tools through its `McpRuntime`), or a stub that returns
/// a clear, socket-named error. Tool dispatch only exists on the zed side —
/// there is no standalone fallback.
#[must_use]
pub async fn resolve_tool_dispatch_port() -> std::sync::Arc<dyn hkask_types::ToolDispatchPort> {
    match connect_bridge("MCP tool dispatch").await {
        Some(client) => {
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::ToolDispatchPort>
        }
        None => std::sync::Arc::new(UnavailableToolDispatch),
    }
}

/// Tool-dispatch stub for MCP servers without the IPC bridge. Returns a clear
/// error naming the missing socket so the caller can distinguish "dispatch
/// unavailable" from "tool not found".
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
            Err(hkask_types::InferenceError::Connection(format!(
                "tool dispatch unavailable: {IPC_BRIDGE_UNAVAILABLE} — \
                 MCP tool calls from delegated agents require the zed process"
            )))
        })
    }
}

/// Resolve the worktree-spawn port from the zed IPC bridge. Returns an
/// `UnavailableWorktreeSpawn` stub when the socket is absent or unreachable —
/// the MCP server falls back to in-memory `LazyLocalSwarmRuntime::delegate()`.
pub async fn resolve_worktree_spawn_port() -> std::sync::Arc<dyn hkask_types::WorktreeSpawnPort> {
    match connect_bridge("MCP worktree spawn").await {
        Some(client) => {
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::WorktreeSpawnPort>
        }
        None => std::sync::Arc::new(UnavailableWorktreeSpawn),
    }
}

/// Worktree-spawn stub for MCP servers without the IPC bridge. Returns an
/// error so `kanban_task_spawn` falls back to `LazyLocalSwarmRuntime`.
pub(crate) struct UnavailableWorktreeSpawn;

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
            Err(hkask_types::InferenceError::Connection(format!(
                "worktree spawn unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
    }
}
