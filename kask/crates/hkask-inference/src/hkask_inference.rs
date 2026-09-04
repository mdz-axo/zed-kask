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
pub mod media_providers;
pub mod media_router;
pub mod model_constants;
pub mod openai_compat;
pub mod provider;
pub mod rerank;
pub mod scoring;

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
    std::sync::Arc::new(LazyInferencePort::new()) as std::sync::Arc<dyn hkask_types::InferencePort>
}

/// A lazy inference port that tries the IPC bridge on each call and
/// falls back to `DirectEmbeddingPort` when the socket is unavailable.
/// Carries NO stored model — every direct-path construction resolves the
/// provider from the model actually being called (the operator's
/// no-hidden-models spec: no stored/default model may decide the endpoint).
struct LazyInferencePort {}

/// Process-local media router backing `LazyInferencePort::media_generate`.
///
/// Media generation is child-local: the media APIs (image, video, TTS, STT)
/// are not LanguageModel calls, so they gain nothing from the IPC bridge.
/// The router reads its keys from this process's env (`DEEPINFRA_API_KEY` /
/// `OPENROUTER_API_KEY`, injected by `build_mcp_server_env` from the
/// keychain). Constructed once per process so the "no media providers
/// configured" warning fires once, not on every call.
static LOCAL_MEDIA_ROUTER: std::sync::OnceLock<crate::media_router::MediaRouter> =
    std::sync::OnceLock::new();

impl LazyInferencePort {
    fn new() -> Self {
        Self {}
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
        prompt: &str,
        images: &[String],
        parameters: &hkask_types::template::LLMParameters,
        model_override: Option<&str>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                > + Send
                + '_,
        >,
    > {
        let prompt = prompt.to_string();
        let images = images.to_vec();
        let params = parameters.clone();
        let model_override = model_override.map(|s| s.to_string());
        Box::pin(async move {
            // Try the IPC bridge first — re-attempt on each call. Vision
            // inference (face detection, object detection, scene captioning,
            // etc.) routes through zed's LanguageModelRegistry via the bridge,
            // same as every other inference method on this port.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client
                    .generate_vision(&prompt, &images, &params, model_override.as_deref())
                    .await;
            }
            // No direct fallback — vision requires a multimodal model that
            // `DirectEmbeddingPort` cannot provide. The error names the
            // missing socket so callers can distinguish "not configured"
            // from "configured but broken."
            Err(hkask_types::InferenceError::Connection(format!(
                "vision inference unavailable: {IPC_BRIDGE_UNAVAILABLE}"
            )))
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
            // Fall back to direct HTTP. Resolve the model FIRST (the
            // visible chain: explicit override → `kask.models.default_model`
            // → typed error — never a code constant), then construct the
            // port FROM that model so the provider endpoint always matches
            // the model actually called. The prior code built the port from
            // a separate stored embedding model — the endpoint could
            // mismatch the per-call model, and the stored model was itself
            // a hidden default.
            let model_str = match model_override {
                Some(model) => model.to_string(),
                None => {
                    let configured = crate::config::InferenceConfig::from_env().default_model;
                    if configured.trim().is_empty() {
                        return Err(hkask_types::InferenceError::NotConfigured(
                            "no default model configured — set \
                             kask.models.default_model (injected as \
                             HKASK_DEFAULT_MODEL) or pass an explicit model; \
                             kask never falls back to a hidden code constant"
                                .to_string(),
                        ));
                    }
                    configured
                }
            };
            let port = DirectEmbeddingPort::try_new(&model_str).ok_or_else(|| {
                hkask_types::InferenceError::Connection(format!(
                    "model '{model_str}': no provider prefix matched and no \
                     provider credentials resolved — use a provider-prefixed \
                     model or configure the provider"
                ))
            })?;
            port.generate_with_model(&prompt, &params, Some(model_str.as_str()), tools.as_deref())
                .await
        })
    }

    fn generate_with_messages(
        &self,
        messages: &[hkask_types::ChatMessage],
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
        let messages = messages.to_vec();
        let params = parameters.clone();
        let model_override = model_override.map(|s| s.to_string());
        let tools = tools.map(|t| t.to_vec());
        Box::pin(async move {
            // Try the IPC bridge first — re-attempt on each call. The IPC
            // client passes the message array directly to the provider
            // (role-tagged), preserving multi-turn conversation structure.
            // Without this override, the trait default flattens messages to
            // a single string and calls generate_with_model — the provider
            // sees "system: ...\n\nuser: ..." instead of proper role-tagged
            // messages, causing the "you responding to yourself" defect.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client
                    .generate_with_messages(
                        &messages,
                        &params,
                        model_override.as_deref(),
                        tools.as_deref(),
                    )
                    .await;
            }
            // Fall back to the trait default: flatten to string and delegate
            // to generate_with_model (which tries DirectEmbeddingPort).
            let prompt = messages
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            self.generate_with_model(
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
        Box::pin(async move {
            // Try the IPC bridge first.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client.embed(&model, &texts).await;
            }
            // Fall back to direct HTTP: construct the port FROM the per-call
            // model so the provider endpoint always matches the model
            // actually embedded (the prior code built it from a separate
            // stored embedding model — a hidden default that could also
            // mismatch the endpoint).
            let port = DirectEmbeddingPort::try_new(&model).ok_or_else(|| {
                hkask_types::EmbeddingGenerationError::Connection(format!(
                    "embed model '{model}': no provider prefix matched and no \
                     provider credentials resolved — use a provider-prefixed \
                     model or run under the zed bridge"
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

    fn rerank<'a>(
        &'a self,
        model: &str,
        query: &str,
        documents: &[String],
    ) -> hkask_types::RerankFuture<'a> {
        let model = model.to_string();
        let query = query.to_string();
        let documents = documents.to_vec();
        Box::pin(async move {
            // Rerank requires the IPC bridge — no direct fallback. The
            // OpenRouter key lives on the zed side by design (the MCP
            // server never sees it), so a direct HTTP rerank is impossible
            // from here. Without this override the trait default returns
            // NotConfigured("rerank not supported") and the research
            // server's deep-strategy rerank can never reach the bridge.
            if let Some(Ok(client)) = InferenceIpcClient::from_env().await {
                return client.rerank_documents(&model, &query, &documents).await;
            }
            Err(hkask_types::InferenceError::Connection(format!(
                "rerank unavailable: {IPC_BRIDGE_UNAVAILABLE}"
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

    fn media_generate<'a>(
        &'a self,
        op: &str,
        params: &hkask_types::MediaGenerateParams,
    ) -> hkask_types::MediaFuture<'a> {
        // Child-local dispatch — no IPC round-trip. The former zed-side
        // MediaRouter was built from the zed process env, which never
        // contains the keys (they are injected only into child processes),
        // so every IPC-routed media call failed with "no provider
        // configured" even with keys installed.
        let op = op.to_string();
        let params = params.clone();
        Box::pin(async move {
            let router = LOCAL_MEDIA_ROUTER.get_or_init(|| {
                crate::media_router::MediaRouter::new(crate::config::InferenceConfig::from_env())
            });
            router.media_generate(&op, &params).await
        })
    }
}
// `LazyInferencePort` overrides the trait defaults for `generate_vision`,
// `embed`, `list_models`, and `generate_batch` so every method tries the
// IPC bridge first and names the missing socket in its fallback error.
// `media_generate` is the exception: it is child-local (see
// `LOCAL_MEDIA_ROUTER`) because media APIs are not LanguageModel calls.
// The trait defaults are not socket-named: `list_models`
// defaults to `Ok(Vec::new())` (a broken bridge read as an empty registry),
// `generate_vision` to a generic `VisionUnsupported`, and `embed` to a
// generic `Connection`. Overriding them keeps the "every method names the
// missing socket" contract honest.

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
        // No hidden default: route through the same visible chain as
        // `generate_with_model(None)` — the configured default
        // (`kask.models.default_model` / `HKASK_DEFAULT_MODEL`), else a
        // typed error. Never a code constant. (Also passes `tools`
        // through — the prior impl dropped them.)
        self.generate_with_model(prompt, parameters, None, _tools)
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
        // Resolve the model — the visible chain only (the operator's
        // spec: no hidden code constant may be the effective inference
        // model):
        // 1. the explicit per-call override,
        // 2. the configured default (`kask.models.default_model`, injected
        //    as `HKASK_DEFAULT_MODEL`),
        // 3. unset → a typed error naming the setting (fail-visible — the
        //    operator is told what to set, never silently run on a hidden
        //    model).
        let model_str = match model_override {
            Some(model) => model.to_string(),
            None => {
                let configured = crate::config::InferenceConfig::from_env().default_model;
                if configured.trim().is_empty() {
                    return Box::pin(futures_util::future::ready(Err(
                        hkask_types::InferenceError::NotConfigured(
                            "no default model configured — set kask.models.default_model \
                             (injected as HKASK_DEFAULT_MODEL) or pass an explicit model; \
                             kask never falls back to a hidden code constant"
                                .to_string(),
                        ),
                    )));
                }
                configured
            }
        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::inference_ipc::INFERENCE_SOCKET_ENV;

    /// The research server resolves its inference port via
    /// `resolve_inference_port()` and routes the deep-strategy rerank through
    /// it. Two layers once silently dropped the capability, each inheriting
    /// the trait default (`NotConfigured("rerank not supported by this
    /// InferencePort")`): `LazyInferencePort` had no `rerank` override, and a
    /// now-deleted `Arc<dyn InferencePort>` forwarding impl (a hand-maintained
    /// mirror of the trait whose advertised consumer `InferenceLoop` never
    /// existed) had no `rerank` forwarder — so a `.rerank()` call on the Arc
    /// resolved to the mirror's default instead of the inner port. The mirror
    /// is deleted; this pin exercises the REAL consumer path —
    /// `resolve_inference_port()` + `.rerank()` on the Arc (auto-deref →
    /// vtable) — and fails on the trait-default error, guarding against both
    /// a missing override and any reintroduced wrapper that forgets to
    /// forward.
    ///
    /// Hermetic by construction: the socket env var is pointed at a
    /// guaranteed-nonexistent path for the duration, so the bridge-down
    /// branch runs deterministically regardless of whether a live zed
    /// process (possibly a stale build) is on the other end of the ambient
    /// socket. No other test in this binary reads this env var.
    #[tokio::test]
    async fn lazy_inference_port_overrides_rerank() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let dead_socket = temp_dir.path().join("nonexistent.sock");
        let prior = std::env::var(INFERENCE_SOCKET_ENV).ok();
        // Edition 2024: env mutation is unsafe; safe here because no other
        // test in this binary reads INFERENCE_SOCKET_ENV.
        unsafe {
            std::env::set_var(INFERENCE_SOCKET_ENV, &dead_socket);
        }
        let outcome = {
            let port = resolve_inference_port().await;
            let outcome = port
                .rerank(
                    "OpenRouter/qwen/qwen3-reranker-8b",
                    "test query",
                    &["test document".to_string()],
                )
                .await;
            // Restore before asserting so a panic cannot leak the dead path.
            match prior {
                Some(value) => unsafe { std::env::set_var(INFERENCE_SOCKET_ENV, value) },
                None => unsafe { std::env::remove_var(INFERENCE_SOCKET_ENV) },
            }
            outcome
        };
        match outcome {
            Ok(_) => panic!(
                "a nonexistent socket must not produce rerank scores — the override \
                 dispatched somewhere unexpected"
            ),
            Err(hkask_types::InferenceError::NotConfigured(message)) => {
                panic!(
                    "LazyInferencePort must override rerank — got the trait-default \
                     NotConfigured error: {message}"
                );
            }
            Err(hkask_types::InferenceError::Connection(message)) => {
                assert!(
                    message.contains("HKASK_INFERENCE_SOCKET"),
                    "a down bridge must name the missing socket, got: {message}"
                );
            }
            Err(other) => {
                panic!("unexpected rerank error variant from the lazy port: {other:?}");
            }
        }
    }
}
