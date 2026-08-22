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
//! `DEFAULT_EMBEDDING_MODEL`, `RunPod/kask-ocr`). The prefix selects the
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
/// `HKASK_INFERENCE_SOCKET` is set and reachable. When the bridge is
/// unavailable, falls back to [`DirectOllamaEmbeddingPort`] — a direct HTTP
/// client that talks to Ollama's OpenAI-compatible `/v1/embeddings` endpoint.
/// This allows the corpus pipeline's embedding stage to proceed without the
/// IPC bridge, which is critical for standalone MCP server execution (e.g.
/// when the corpus server runs outside zed's governed launch). Generation,
/// vision, and tool-dispatch methods on the fallback port return clear errors
/// — only `embed` is functional.
///
/// `pre`: none (reads env vars). `post`: an `Arc<dyn InferencePort>` ready for
/// inference calls.
#[must_use]
pub async fn resolve_inference_port() -> std::sync::Arc<dyn hkask_types::InferencePort> {
    match connect_bridge("MCP inference").await {
        Some(client) => {
            std::sync::Arc::new(client) as std::sync::Arc<dyn hkask_types::InferencePort>
        }
        None => {
            // IPC bridge unavailable — attempt direct Ollama embedding fallback.
            // This keeps the corpus pipeline's embedding stage functional when
            // the MCP server runs without zed's governed launch (no IPC socket).
            // Generation/vision/tool-dispatch remain unavailable.
            let embedding_model = model_constants::embedding_model();
            if let Some(port) = DirectOllamaEmbeddingPort::try_new(&embedding_model) {
                tracing::info!(
                    target: "hkask.inference",
                    model = %embedding_model,
                    "IPC bridge unavailable — falling back to direct Ollama embedding "
                );
                std::sync::Arc::new(port) as std::sync::Arc<dyn hkask_types::InferencePort>
            } else {
                std::sync::Arc::new(UnavailableInference)
            }
        }
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

/// Direct HTTP embedding port for Ollama's OpenAI-compatible endpoint.
///
/// When the IPC bridge is unavailable (e.g. the corpus MCP server runs
/// outside zed's governed launch), this port provides a direct fallback for
/// embedding calls only. It talks to Ollama at `http://localhost:11434/v1`
/// (configurable via `OLLAMA_API_URL` env var) using `reqwest`. Generation,
/// vision, and tool-dispatch methods return clear errors — only `embed` is
/// functional.
///
/// The port is constructed only when the embedding model has an `ollama/`
/// prefix (checked by [`DirectOllamaEmbeddingPort::try_new`]). For non-Ollama
/// embedding models, `try_new` returns `None` and the caller falls back to
/// `UnavailableInference`.
struct DirectOllamaEmbeddingPort {
    /// The base URL for Ollama's OpenAI-compatible API, e.g.
    /// `http://localhost:11434/v1`.
    api_url: String,
    /// Reusable reqwest client.
    client: reqwest::Client,
}

impl DirectOllamaEmbeddingPort {
    /// Attempt to construct the port for an `ollama/`-prefixed embedding model.
    ///
    /// Returns `None` if:
    /// - The model string has no `ollama/` prefix (not an Ollama model).
    /// - The reqwest client cannot be constructed (TLS backend failure).
    ///
    /// The API URL is resolved from `OLLAMA_API_URL` env var, falling back to
    /// `http://localhost:11434/v1` (Ollama's default OpenAI-compatible endpoint).
    #[must_use]
    fn try_new(embedding_model: &str) -> Option<Self> {
        // Only applies to ollama-prefixed models.
        let model_id = embedding_model.strip_prefix("ollama/")?;
        if model_id.is_empty() {
            return None;
        }

        let api_url = std::env::var("OLLAMA_API_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_string());

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| {
                tracing::warn!(
                    target: "hkask.inference",
                    error = %e,
                    "Failed to construct reqwest client for direct Ollama embedding fallback"
                );
            })
            .ok()?;

        Some(Self {
            api_url,
            client,
        })
    }
}

impl hkask_types::InferencePort for DirectOllamaEmbeddingPort {
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
                "generation unavailable on direct Ollama embedding fallback \
                 — only embed is supported without the IPC bridge".to_string(),
            ))
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
            Err(hkask_types::InferenceError::Connection(
                "vision inference unavailable on direct Ollama embedding fallback".to_string(),
            ))
        })
    }

    fn embed<'a>(&'a self, model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        let model_id = model.strip_prefix("ollama/").unwrap_or(model).to_string();
        let api_url = self.api_url.clone();
        let client = self.client.clone();
        let texts = texts.to_vec();
        Box::pin(async move {
            if texts.is_empty() {
                return Err(hkask_types::EmbeddingGenerationError::EmptyResponse);
            }

            let uri = format!("{api_url}/embeddings");
            let body = serde_json::json!({
                "model": model_id,
                "input": texts,
            });

            let response = client
                .post(&uri)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    hkask_types::EmbeddingGenerationError::Connection(format!(
                        "direct Ollama embedding request failed: {e}"
                    ))
                })?;

            let status = response.status();
            if !status.is_success() {
                let body_text = response.text().await.unwrap_or_default();
                return Err(hkask_types::EmbeddingGenerationError::Api(
                    status.as_u16(),
                    body_text,
                ));
            }

            #[derive(serde::Deserialize)]
            struct EmbeddingData {
                embedding: Vec<f32>,
            }
            #[derive(serde::Deserialize)]
            struct EmbeddingResponse {
                data: Vec<EmbeddingData>,
            }

            let parsed: EmbeddingResponse = response.json().await.map_err(|e| {
                hkask_types::EmbeddingGenerationError::Json(format!(
                    "failed to parse Ollama embedding response: {e}"
                ))
            })?;

            let embeddings: Vec<Vec<f32>> =
                parsed.data.into_iter().map(|d| d.embedding).collect();

            if embeddings.is_empty() {
                return Err(hkask_types::EmbeddingGenerationError::EmptyResponse);
            }

            Ok(embeddings)
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
            Err(hkask_types::InferenceError::Connection(
                "list_models unavailable on direct Ollama embedding fallback".to_string(),
            ))
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
