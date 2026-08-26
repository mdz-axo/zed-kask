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

pub mod batch;
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

/// Resolve the inference port for an MCP server.
///
/// Returns a [`LazyInferencePort`] that tries the IPC bridge on each call
/// and falls back to [`DirectEmbeddingPort`] when the socket is unavailable.
/// This eliminates the resolve-once-at-startup problem where the corpus
/// MCP server starts before the IPC socket exists and never gets
/// restarted with it.
#[must_use]
pub async fn resolve_inference_port() -> std::sync::Arc<dyn hkask_types::InferencePort> {
    let embedding_model = model_constants::embedding_model();
    std::sync::Arc::new(LazyInferencePort::new(&embedding_model))
        as std::sync::Arc<dyn hkask_types::InferencePort>
}

/// A lazy inference port that tries the IPC bridge on each call and
/// falls back to `DirectEmbeddingPort` when the socket is unavailable.
struct LazyInferencePort {
    embedding_model: String,
}

impl LazyInferencePort {
    fn new(embedding_model: &str) -> Self {
        Self {
            embedding_model: embedding_model.to_string(),
        }
    }
}

impl hkask_types::InferencePort for LazyInferencePort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &hkask_types::template::LLMParameters,
        tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        self.generate_with_model(prompt, parameters, None, tools)
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
                "vision inference unavailable on lazy inference port".to_string(),
            ))
        })
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &hkask_types::template::LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        let prompt = prompt.to_string();
        let params = parameters.clone();
        let model_override = model_override.map(|s| s.to_string());
        let tools = tools.map(|t| t.to_vec());
        Box::pin(async move {
            // Try the IPC bridge first — re-attempt on each call.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client
                    .generate_with_model(
                        &prompt,
                        &params,
                        model_override.as_deref(),
                        tools.as_deref(),
                    )
                    .await;
            }
            // Fall back to direct HTTP.
            let port = DirectEmbeddingPort::try_new(&self.embedding_model).ok_or_else(|| {
                hkask_types::InferenceError::Connection(format!(
                    "No inference available: {IPC_BRIDGE_UNAVAILABLE} \
                         and direct fallback failed (no API key or provider)"
                ))
            })?;
            port.generate_with_model(
                &prompt,
                &params,
                model_override.as_deref(),
                tools.as_deref(),
            )
            .await
        })
    }

    fn embed<'a>(&'a self, model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        let model = model.to_string();
        let texts = texts.to_vec();
        let embedding_model = self.embedding_model.clone();
        Box::pin(async move {
            // Try the IPC bridge first.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client.embed(&model, &texts).await;
            }
            // Fall back to direct HTTP.
            let port = DirectEmbeddingPort::try_new(&embedding_model).ok_or_else(|| {
                hkask_types::EmbeddingGenerationError::Connection(format!(
                    "embed unavailable: {IPC_BRIDGE_UNAVAILABLE}"
                ))
            })?;
            port.embed(&model, &texts).await
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
            // Try the IPC bridge first.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client.list_models().await;
            }
            Err(hkask_types::InferenceError::Connection(format!(
                "list_models unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
    }

    fn generate_batch<'a>(
        &'a self,
        model: &str,
        prompts: &[hkask_types::inference_ipc::BatchPromptEntry],
        max_tokens: u32,
        temperature: f32,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Vec<hkask_types::inference_ipc::BatchResultEntry>,
                        hkask_types::InferenceError,
                    >,
                > + Send
                + 'a,
        >,
    > {
        let model = model.to_string();
        let prompts = prompts.to_vec();
        Box::pin(async move {
            // Batch API requires the IPC bridge — no direct fallback.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client
                    .call_generate_batch(&model, &prompts, max_tokens, temperature)
                    .await;
            }
            Err(hkask_types::InferenceError::Connection(format!(
                "batch inference unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
        })
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
/// Direct HTTP embedding port for Ollama's OpenAI-compatible endpoint.
///
/// When the IPC bridge is unavailable (e.g. the corpus MCP server runs
/// outside zed's governed launch), this port provides a direct fallback for
/// embedding calls only. It talks to Ollama at `http://localhost:11434/v1`
/// (configurable via `OLLAMA_API_URL` env var) using `reqwest`. Generation,
/// vision, and tool-dispatch methods return clear errors — only `embed` is
/// functional.
/// A direct HTTP inference port for any OpenAI-compatible provider.
///
/// Used as a fallback when the zed IPC bridge is unavailable. Resolves the
/// provider prefix from the model string (e.g. `DeepInfra/`, `ollama/`,
/// `OpenRouter/`) against a static table to get the API URL and env var name,
/// then calls the OpenAI-compatible `/v1/embeddings` and `/chat/completions`
/// endpoints directly.
///
/// For Ollama (local, no API key), the key is empty. For cloud providers,
/// the key is read from the env var named in the provider descriptor.
struct DirectEmbeddingPort {
    /// The base URL for the provider's OpenAI-compatible API, e.g.
    /// `https://api.deepinfra.com/v1/openai` or `http://localhost:11434/v1`.
    api_url: String,
    /// The API key for the provider (empty for local providers like Ollama).
    api_key: String,
    /// Reusable reqwest client.
    client: reqwest::Client,
}

/// A minimal provider descriptor for the direct embedding fallback.
/// Maps a provider prefix to its API URL and env var name.
struct DirectEmbeddingProvider {
    id: &'static str,
    api_url: &'static str,
    env_var: &'static str,
}

/// The provider table for the direct embedding fallback. Mirrors the
/// `INFERENCE_PROVIDERS` table in `kask_bridge::inference_providers` —
/// duplicated because `hkask-inference` cannot depend on `kask_bridge`
/// (that would invert the D8 seam). Keep in sync when adding providers.
static DIRECT_EMBEDDING_PROVIDERS: &[DirectEmbeddingProvider] = &[
    DirectEmbeddingProvider {
        id: "DeepInfra",
        api_url: "https://api.deepinfra.com/v1/openai",
        env_var: "DEEPINFRA_API_KEY",
    },
    DirectEmbeddingProvider {
        id: "OpenRouter",
        api_url: "https://openrouter.ai/api/v1",
        env_var: "OPENROUTER_API_KEY",
    },
    DirectEmbeddingProvider {
        id: "ollama",
        api_url: "http://localhost:11434/v1",
        env_var: "",
    },
];

impl DirectEmbeddingPort {
    /// Attempt to construct the port for a provider-prefixed embedding model.
    ///
    /// Returns `None` if:
    /// - The model string has no recognized provider prefix.
    /// - The provider requires an API key but the env var is not set.
    /// - The reqwest client cannot be constructed (TLS backend failure).
    ///
    /// For Ollama (empty `env_var`), no key is needed — the key is empty.
    #[must_use]
    fn try_new(embedding_model: &str) -> Option<Self> {
        // Find the provider by matching the prefix (case-insensitive).
        let provider = DIRECT_EMBEDDING_PROVIDERS.iter().find(|p| {
            let prefix = format!("{}/", p.id);
            embedding_model.len() >= prefix.len()
                && embedding_model[..prefix.len()].eq_ignore_ascii_case(&prefix)
        })?;

        // Resolve the API key. Local providers (empty env_var) need no key.
        let api_key = if provider.env_var.is_empty() {
            String::new()
        } else {
            std::env::var(provider.env_var).ok().or_else(|| {
                tracing::warn!(
                    target: "hkask.inference",
                    provider = provider.id,
                    env_var = provider.env_var,
                    "Direct embedding fallback: env var not set — \
                     embedding will not work without the IPC bridge"
                );
                None
            })?
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| {
                tracing::warn!(
                    target: "hkask.inference",
                    error = %e,
                    "Failed to construct reqwest client for direct embedding fallback"
                );
            })
            .ok()?;

        Some(Self {
            api_url: provider.api_url.to_string(),
            api_key,
            client,
        })
    }
}

impl hkask_types::InferencePort for DirectEmbeddingPort {
    fn generate(
        &self,
        prompt: &str,
        parameters: &hkask_types::template::LLMParameters,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        let default_model = model_constants::DEFAULT_FALLBACK_MODEL.to_string();
        self.generate_with_model(prompt, parameters, Some(&default_model), None)
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
                "vision inference unavailable on direct inference fallback".to_string(),
            ))
        })
    }

    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &hkask_types::template::LLMParameters,
        model_override: Option<&str>,
        _tools: Option<&[hkask_types::ChatToolDefinition]>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        // Resolve the model: strip the provider prefix for the API call.
        let model_str = model_override
            .unwrap_or(model_constants::DEFAULT_FALLBACK_MODEL)
            .to_string();
        let model_id = model_str
            .split_once('/')
            .map(|(_, rest)| rest)
            .unwrap_or(&model_str)
            .to_string();

        // Resolve the API URL + key for this model's provider.
        let (api_url, api_key) = match DIRECT_EMBEDDING_PROVIDERS.iter().find(|p| {
            let prefix = format!("{}/", p.id);
            model_str.len() >= prefix.len()
                && model_str[..prefix.len()].eq_ignore_ascii_case(&prefix)
        }) {
            Some(provider) => {
                let key = if provider.env_var.is_empty() {
                    String::new()
                } else {
                    std::env::var(provider.env_var).unwrap_or_default()
                };
                (provider.api_url.to_string(), key)
            }
            None => (self.api_url.clone(), self.api_key.clone()),
        };

        let client = self.client.clone();
        let temperature = parameters.temperature;
        let top_p = parameters.top_p;
        let thinking_allowed = parameters.thinking_allowed;
        let prompt = prompt.to_string();

        Box::pin(async move {
            let uri = format!("{api_url}/chat/completions");
            let mut body = serde_json::json!({
                "model": model_id,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": temperature,
                "top_p": top_p,
            });
            if !thinking_allowed {
                body["reasoning"] = serde_json::json!({"effort": "none"});
            }

            let mut request = client
                .post(&uri)
                .header("Content-Type", "application/json")
                .json(&body);

            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {api_key}"));
            }

            let response = request.send().await.map_err(|e| {
                hkask_types::InferenceError::Connection(format!(
                    "direct inference request failed: {e}"
                ))
            })?;

            let status = response.status();
            if !status.is_success() {
                let body_text = response.text().await.unwrap_or_default();
                return Err(hkask_types::InferenceError::Connection(format!(
                    "chat API error: status {status}: {body_text}"
                )));
            }

            #[derive(serde::Deserialize)]
            struct ChatChoice {
                message: ChatMessageContent,
                finish_reason: Option<String>,
            }
            #[derive(serde::Deserialize)]
            struct ChatMessageContent {
                content: Option<String>,
                reasoning: Option<String>,
            }
            #[derive(serde::Deserialize)]
            struct ChatUsage {
                prompt_tokens: Option<u32>,
                completion_tokens: Option<u32>,
                total_tokens: Option<u32>,
            }
            #[derive(serde::Deserialize)]
            struct ChatResponse {
                choices: Vec<ChatChoice>,
                usage: Option<ChatUsage>,
                model: Option<String>,
            }

            let parsed: ChatResponse = response.json().await.map_err(|e| {
                hkask_types::InferenceError::Connection(format!(
                    "failed to parse chat response: {e}"
                ))
            })?;

            let choice = parsed.choices.into_iter().next().ok_or_else(|| {
                hkask_types::InferenceError::Connection("chat response has no choices".to_string())
            })?;

            let text = choice.message.content.unwrap_or_default();
            let model = parsed.model.unwrap_or_default();
            let usage = hkask_types::InferenceUsage {
                prompt_tokens: parsed
                    .usage
                    .as_ref()
                    .and_then(|u| u.prompt_tokens)
                    .unwrap_or(0),
                completion_tokens: parsed
                    .usage
                    .as_ref()
                    .and_then(|u| u.completion_tokens)
                    .unwrap_or(0),
                total_tokens: parsed
                    .usage
                    .as_ref()
                    .and_then(|u| u.total_tokens)
                    .unwrap_or(0),
            };

            Ok(hkask_types::InferenceResult {
                text,
                model,
                usage,
                finish_reason: choice.finish_reason.unwrap_or_default(),
                tool_calls: Vec::new(),
                reasoning: choice.message.reasoning,
                cost_usd: None,
            })
        })
    }

    fn embed<'a>(&'a self, model: &str, texts: &[String]) -> hkask_types::EmbedFuture<'a> {
        // Strip any provider prefix — the API expects the bare model id.
        let model_id = model
            .split_once('/')
            .map(|(_, rest)| rest)
            .unwrap_or(model)
            .to_string();
        let api_url = self.api_url.clone();
        let api_key = self.api_key.clone();
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

            let mut request = client
                .post(&uri)
                .header("Content-Type", "application/json")
                .json(&body);

            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {api_key}"));
            }

            let response = request.send().await.map_err(|e| {
                hkask_types::EmbeddingGenerationError::Connection(format!(
                    "direct embedding request failed: {e}"
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
                    "failed to parse embedding response: {e}"
                ))
            })?;

            let embeddings: Vec<Vec<f32>> = parsed.data.into_iter().map(|d| d.embedding).collect();

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
                "list_models unavailable on direct inference fallback".to_string(),
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
