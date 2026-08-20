#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask Inference — media generation + IPC bridge client.
//!
//! In zed-kask, chat inference routes through the zed IPC bridge
//! (`InferenceIpcClient` → `LanguageModelRegistry`). This crate provides:
//!   `MediaProvider` backends, not covered by zed's `LanguageModel` abstraction.
//! - `InferenceIpcClient` — the IPC bridge client used by MCP servers to route
//!   chat/vision/embed through zed's `LanguageModelRegistry`.
//! - `InferenceConfig` — shared configuration (base URLs, API keys, default model).
//! - `ProviderId` — provider routing enum used by the training adapter router.
//!
//! # Architecture
//!
//! ```text
//!   └── ProviderRegistry — dispatches to registered MediaProvider backends
//!       they are added back)
//!
//! InferenceIpcClient (implements InferencePort — chat/vision/embed via zed)
//!   └── Unix socket → zed LanguageModelRegistry
//!
//! ```rust,no_run
//!
//! # Model Naming
//!
//! - `OpenRouter/openai/gpt-4o` → OpenRouter (via IPC bridge)
//! - No prefix → default model (configurable, default: OpenRouter/z-ai/glm-5.2)

pub mod chat_protocol;
pub mod config;
pub mod inference_ipc_client;
pub mod model_constants;
pub mod openai_compat;

// Re-exports — public API
pub use config::{InferenceConfig, ProviderConfig, ProviderId};
pub use inference_ipc_client::InferenceIpcClient;

/// Unified model entry from any provider, with provider prefix applied.
#[derive(Debug, Clone)]
pub struct RouterModelEntry {
    /// Full model name with provider prefix (e.g., "ollama/qwen3:8b")
    pub prefixed_name: String,
    /// Provider this model belongs to
    pub provider: ProviderId,
    /// Raw model name without prefix
    pub model: String,
    /// Model family (e.g., "llama", "qwen2")
    pub family: Option<String>,
    /// Parameter count (e.g., "8B", "70B")
    pub parameter_size: Option<String>,
    /// Quantization level (e.g., "Q4_0")
    pub quantization_level: Option<String>,
    /// Model size in bytes (if available)
    pub size_bytes: Option<u64>,
    /// Whether the model supports vision/multimodal input.
    /// Populated via heuristic on model family name (not runtime probing).
    pub supports_vision: Option<bool>,
}

impl RouterModelEntry {
    /// Construct a RouterModelEntry from a provider and model id.
    ///
    /// expect: "The system heuristically routes multimodal models"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — canonical model entry construction
    /// pre:  model_id is non-empty
    /// post: returns RouterModelEntry with prefixed name, provider, and inferred vision support
    pub fn from_model_entry(provider: ProviderId, model_id: &str) -> Self {
        Self {
            prefixed_name: provider.prefix_model(model_id),
            provider,
            model: model_id.to_string(),
            supports_vision: Self::infer_vision_support(model_id, None),
            family: None,
            parameter_size: None,
            quantization_level: None,
            size_bytes: None,
        }
    }

    /// Heuristic: known vision-capable model families.
    ///
    /// Checks model name and family against a compiled-in allowlist
    /// plus any models listed in the `HKASK_VISION_FAMILIES` env var
    /// (comma-separated). Runtime-addition avoids recompiles.
    #[must_use]
    pub fn infer_vision_support(model: &str, family: Option<&str>) -> Option<bool> {
        const DEFAULT_VISION_FAMILIES: &[&str] = &[
            "llava",
            "bakllava",
            "minicpm-v",
            "gemma3",
            "llama3.2-vision",
            "cogvlm",
            "moondream",
            "pixtral",
            "florence",
            "paligemma",
            "qwen2-vl",
            "qwen2.5-vl",
            "qwen3-vl",
            "qwen-vl",
            "internvl",
            "phi-3-vision",
            "lighton",
            "paddleocr",
            "nemotron-parse",
            "olmocr",
            "deepseek-ocr",
        ];

        let model_lower = model.to_lowercase();
        let family_lower = family.map(|f| f.to_lowercase());

        // Check compiled-in families
        for vf in DEFAULT_VISION_FAMILIES {
            if model_lower.contains(vf) {
                return Some(true);
            }
            if let Some(ref fam) = family_lower
                && fam.contains(vf)
            {
                return Some(true);
            }
        }

        // Check env-configured families
        if let Ok(extra) = std::env::var("HKASK_VISION_FAMILIES") {
            for vf in extra.split(',').map(|s| s.trim().to_lowercase()) {
                if !vf.is_empty() && model_lower.contains(&vf) {
                    return Some(true);
                }
                if let Some(ref fam) = family_lower
                    && !vf.is_empty()
                    && fam.contains(&vf)
                {
                    return Some(true);
                }
            }
        }

        None
    }
}

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
            dyn std::future::Future<Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>>
                + Send
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

/// Resolve the skill-execution port for an MCP server.
///
/// Returns the IPC-bridge client when the socket is available (the zed
/// process runs the cascade through its global `ManifestExecutor`), or a
/// stub that returns a clear error. Mirrors `resolve_tool_dispatch_port`.
#[must_use]
pub async fn resolve_skill_exec_port() -> std::sync::Arc<dyn hkask_types::SkillExecPort> {
    match InferenceIpcClient::from_env().await {
        Some(Ok(client)) => {
            tracing::info!(
                target: "hkask.inference",
                "MCP skill execution routed through zed IPC bridge (HKASK_INFERENCE_SOCKET)"
            );
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::SkillExecPort>
        }
        Some(Err(e)) => {
            tracing::warn!(
                target: "hkask.inference",
                error = %e,
                "IPC bridge connection failed — skill execution unavailable"
            );
            std::sync::Arc::new(UnavailableSkillExec)
        }
        None => {
            tracing::info!(
                target: "hkask.inference",
                "HKASK_INFERENCE_SOCKET not set — skill execution unavailable"
            );
            std::sync::Arc::new(UnavailableSkillExec)
        }
    }
}

/// Skill-execution stub for MCP servers without the IPC bridge.
struct UnavailableSkillExec;

impl hkask_types::SkillExecPort for UnavailableSkillExec {
    fn execute_skill<'a>(
        &'a self,
        _name: &'a str,
        _task: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<String, hkask_types::SkillExecError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(hkask_types::SkillExecError::Unavailable(
                "HKASK_INFERENCE_SOCKET not set or IPC bridge unreachable — running a declared skill requires the zed process"
                    .to_string(),
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
