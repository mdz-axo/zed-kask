use super::EmbeddingGenerationError;
use super::inference_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceUsage,
    StructuredToolCall,
};
use crate::template::LLMParameters;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Future returned by [`InferencePort::embed`].
///
/// Extracted as a named alias so the trait signature stays under clippy's
/// `type_complexity` threshold; the boxed-async-future shape is inherent to
/// the object-safe `InferencePort` trait (we avoid `async_trait` deliberately —
/// see the trait-level comment).
pub type EmbedFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>, EmbeddingGenerationError>> + Send + 'a>>;

/// Future returned by [`InferencePort::media_generate`].
///
/// Same rationale as `EmbedFuture` — keeps the trait signature under
/// clippy's `type_complexity` threshold.
pub type MediaFuture<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, InferenceError>> + Send + 'a>>;

/// Parameters for [`InferencePort::media_generate`].
///
/// Carries the media-generation fields (image/video/speech/transcription)
/// that the IPC bridge forwards to the MediaRouter. Grouped into a struct
/// so the trait method signature doesn't grow 12+ optional parameters.
///
/// The `op` string (e.g. "generate_image", "transcribe") is passed as the
/// first argument to `media_generate`, not as a field here — it selects the
/// backend method. The remaining fields are op-specific; the server-side
/// dispatch reads only the fields relevant to each op.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaGenerateParams {
    /// Text prompt for image/video generation.
    pub prompt: Option<String>,
    /// Image URL for image-to-image, image-to-video, upscale, etc.
    pub image_url: Option<String>,
    /// Audio URL for transcription.
    pub audio_url: Option<String>,
    /// Text for speech synthesis.
    pub text: Option<String>,
    /// Voice name for speech synthesis.
    pub voice: Option<String>,
    /// Image size for image generation.
    pub size: Option<String>,
    /// Number of images to generate.
    pub count: Option<u32>,
    /// Strength for image-to-image.
    pub strength: Option<f32>,
    /// Scale factor for upscaling.
    pub scale: Option<u32>,
    /// Duration for video generation.
    pub duration: Option<f32>,
    /// Language hint for transcription.
    pub language: Option<String>,
}

/// LLM invocation boundary. Uses ``Pin<Box<dyn Future>>`` (not `async_trait`) for object-safety.
/// Impls: `MediaRouter` and `InferenceIpcClient` (hkask-inference), `Arc<dyn InferencePort>` (blanket).
/// A model available from an inference provider.
///
/// Simplified version of `hkask_inference::RouterModelEntry` that lives in
/// `hkask-types` so the `InferencePort` trait can return it without depending
/// on `hkask-inference`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Full model name with provider prefix (e.g., "OpenRouter/z-ai/glm-5.2")
    pub prefixed_name: String,
    /// Raw model name without prefix
    pub model: String,
    /// Whether the model supports vision/multimodal input.
    pub supports_vision: bool,
}

/// MCP-server-side tool dispatch boundary.
///
/// Lets a child MCP server process (e.g. `hkask-mcp-swarm`'s local delegate
/// loop) invoke governed MCP tools that live in the zed process's
/// `McpRuntime`. The zed side mints the OCAP panel token — the child never
/// sees or holds token material. `InferenceIpcClient` implements this over
/// the `InferenceMethod::ToolInvoke` IPC method; backends without a bridge
/// (MediaRouter fallback) return a clear error.
///
/// Two implementors: the IPC client (real dispatch) and the fallback stub
/// (clear error) — the swarm delegate loop reads it, so it is not
/// speculative generality.
pub trait ToolDispatchPort: Send + Sync {
    /// Invoke a tool on a governed MCP server via the zed process.
    ///
    /// `allowed` is the caller's declared `server/tool` allowlist (the
    /// delegated agent's `mcp_tools`). The zed-side dispatch refuses any
    /// tool outside it before minting the panel token — the allowlist is
    /// enforced at the dispatch boundary, not only inside the child.
    ///
    /// pre:  `server` is a registered MCP server id; `tool` exists on it;
    ///       `server/tool` is in `allowed`
    /// post: returns the tool's JSON output, or an error (tool not found,
    ///       not in allowlist, capability denied, dispatch unavailable)
    fn invoke_tool<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: serde_json::Value,
        allowed: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, InferenceError>> + Send + 'a>>;
}

impl ToolDispatchPort for Arc<dyn ToolDispatchPort> {
    fn invoke_tool<'a>(
        &'a self,
        server: &'a str,
        tool: &'a str,
        args: serde_json::Value,
        allowed: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, InferenceError>> + Send + 'a>> {
        self.as_ref().invoke_tool(server, tool, args, allowed)
    }
}

/// Error returned by [`SkillExecPort::execute_skill`].
///
/// Replaces the previous `Result<String, String>` return, which erased the
/// error category and forced every impl to flatten structured errors (e.g.
/// [`InferenceError`]) into a string. The D1-seam-constrained
/// `agent::SkillManifestExecutor` impl (`kask_bridge::BridgeManifestExecutor`)
/// still returns `Result<String, String>` because the upstream Zed trait
/// requires it — the [`From<String>`] conversion lets that impl bridge into
/// this typed error with `.map_err(Into::into)` without an upstream change.
#[derive(Debug, thiserror::Error)]
pub enum SkillExecError {
    /// Skill execution is unavailable (no IPC socket, no manifest executor
    /// wired). The message names the missing dependency so an operator can
    /// remediate.
    #[error("skill execution unavailable: {0}")]
    Unavailable(String),
    /// An inference-layer failure while running the cascade (connection,
    /// model, JSON, circuit). Carries the underlying [`InferenceError`].
    #[error(transparent)]
    Inference(#[from] InferenceError),
    /// The cascade itself failed (no manifest, manifest load error, step
    /// failure, task join error). The string is the message produced by the
    /// manifest executor — kept as a string because the upstream
    /// `agent::SkillManifestExecutor` trait (D1 seam) returns
    /// `Result<String, String>`.
    #[error("skill cascade failed: {0}")]
    Failed(String),
}

impl From<String> for SkillExecError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// MCP-server-side skill-execution boundary.
///
/// Lets a child MCP server process (e.g. `hkask-mcp-swarm`'s local delegate)
/// run an hKask skill cascade that lives in the zed process (the global
/// `ManifestExecutor`). `InferenceIpcClient` implements this over the
/// `InferenceMethod::SkillExecute` IPC method; backends without a bridge
/// return a clear error. The cascade runs with the executor's own
/// enforcement on the zed side — the child never holds token material.
///
/// Two implementors: the IPC client (real execution) and the fallback stub
/// (clear error) — the swarm delegate loop reads it, so it is not
/// speculative generality.
pub trait SkillExecPort: Send + Sync {
    /// Execute a skill cascade by name against `task`. Returns the cascade's
    /// final output as text. `Err` when the skill has no manifest or the
    /// cascade failed.
    fn execute_skill<'a>(
        &'a self,
        name: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, SkillExecError>> + Send + 'a>>;
}

impl SkillExecPort for Arc<dyn SkillExecPort> {
    fn execute_skill<'a>(
        &'a self,
        name: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String, SkillExecError>> + Send + 'a>> {
        self.as_ref().execute_skill(name, task)
    }
}

/// Port for spawning worktree-backed agent threads via the zed IPC bridge.
/// Used by `kanban_task_spawn` to isolate spawned agents in a separate git
/// worktree (P1: worktree/terminal model). When the port is unavailable (no
/// IPC socket, no active workspace), the MCP server falls back to the
/// in-memory `LazyLocalSwarmRuntime::delegate()` path.
pub trait WorktreeSpawnPort: Send + Sync {
    /// Create a worktree-backed agent thread. Returns a confirmation message
    /// on success, or an error when the worktree spawner is not configured.
    fn create_worktree_thread<'a>(
        &'a self,
        prompt: &'a str,
        title: &'a str,
        worktree_name: Option<&'a str>,
        base_ref: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<String, InferenceError>> + Send + 'a>>;
}

pub trait InferencePort: Send + Sync {
    fn generate(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>;

    /// Falls back to `generate()` when `model_override` is `None`.
    fn generate_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        _model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.generate(prompt, parameters, tools)
    }

    /// Multi-turn inference with an explicit message array.
    ///
    /// This is the correct path for chat/REPL: each message carries its own
    /// `role` ("system", "user", "assistant"), so the provider sees the
    /// conversation as `[system, user, assistant, user, ...]` — not a single
    /// flattened string. This eliminates the "you responding to yourself"
    /// defect where previous assistant responses were embedded inside a
    /// `user` role message.
    ///
    /// Default: flattens messages to a string and delegates to
    /// `generate_with_model`. Backends that speak the OpenAI wire format
    /// override this to pass the message array directly.
    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        let prompt = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        self.generate_with_model(&prompt, parameters, model_override, tools)
    }

    /// Streaming variant of `generate_with_messages`.
    ///
    /// Default: yields a single chunk from `generate_with_messages`.
    fn generate_stream_with_messages(
        &self,
        messages: &[ChatMessage],
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        let future = self.generate_with_messages(messages, parameters, model_override, tools);
        Box::pin(futures_util::stream::once(async move {
            Ok(InferenceStreamChunk::from(future.await?))
        }))
    }

    fn generate_n(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        n: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<InferenceResult>, InferenceError>> + Send + '_>>
    {
        use futures_util::future::join_all;
        let futures: Vec<_> = (0..n)
            .map(|_| self.generate(prompt, parameters, None))
            .collect();
        Box::pin(async move {
            let results = join_all(futures).await;
            results.into_iter().collect()
        })
    }

    /// Stream inference chunks. Default: yields single chunk from `generate()`. Override for SSE/streaming backends.
    fn generate_stream(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        let future = self.generate(prompt, parameters, tools);
        Box::pin(futures_util::stream::once(async move {
            Ok(InferenceStreamChunk::from(future.await?))
        }))
    }

    /// Stream with optional model override. Falls back to `generate_stream()` when `model_override` is `None`.
    fn generate_stream_with_model(
        &self,
        prompt: &str,
        parameters: &LLMParameters,
        model_override: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        if model_override.is_some() {
            let future = self.generate_with_model(prompt, parameters, model_override, tools);
            Box::pin(futures_util::stream::once(async move {
                Ok(InferenceStreamChunk::from(future.await?))
            }))
        } else {
            self.generate_stream(prompt, parameters, tools)
        }
    }

    /// Vision inference — send base64-encoded images to a multimodal model.
    ///
    /// The default rejects the request so an implementation cannot silently drop images.
    fn generate_vision(
        &self,
        _prompt: &str,
        _images: &[String],
        _parameters: &LLMParameters,
        _model_override: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        Box::pin(async {
            Err(InferenceError::VisionUnsupported(
                "backend does not implement vision inference".to_string(),
            ))
        })
    }

    /// Generate embeddings for a batch of texts.
    ///
    /// `model` is the provider-prefixed model string (e.g.
    /// `DeepInfra/Qwen/Qwen3-Embedding-0.6B`). The implementation strips the
    /// prefix and resolves credentials from the appropriate provider.
    ///
    /// Default: returns an error. `InferenceIpcClient` overrides this to
    /// route through zed's `LanguageModelEmbeddingPort` via the IPC bridge.
    fn embed<'a>(&'a self, _model: &str, _texts: &[String]) -> EmbedFuture<'a> {
        Box::pin(async {
            Err(EmbeddingGenerationError::Connection(
                "embed not supported by this InferencePort".into(),
            ))
        })
    }

    /// List available models across all configured providers.
    ///
    /// Default: returns an empty vec. `InferenceIpcClient` overrides this to
    /// enumerate models from zed's `LanguageModelRegistry` via the IPC bridge.
    /// `MediaRouter` returns empty (media-only; model listing is not available
    /// when running standalone without the IPC bridge).
    ///
    /// Returns `Err` when the underlying provider is unreachable (IPC bridge
    /// down, registry query failed) so callers can distinguish "no models
    /// configured" from "the bridge is broken." Previously this collapsed both
    /// to an empty vec, making a broken IPC indistinguishable from an empty
    /// registry (F9 — variety-deficit: the regulator can act on "broken" but
    /// callers couldn't tell).
    #[must_use]
    fn list_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelEntry>, InferenceError>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    /// List only vision-capable models.
    ///
    /// Default: delegates to `list_models()` and filters by the vision flag.
    /// Returns empty when `list_models()` returns empty. Propagates errors
    /// from `list_models()` so a broken IPC is not masked as "no vision models."
    #[must_use]
    fn list_vision_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelEntry>, InferenceError>> + Send + 'a>> {
        Box::pin(async {
            let models = self.list_models().await?;
            Ok(models.into_iter().filter(|m| m.supports_vision).collect())
        })
    }

    /// Generate media (image, video, speech, transcription) via the MediaRouter.
    ///
    /// `op` selects the backend method (see `MediaGenerateParams::op`). The
    /// default returns an error — `InferenceIpcClient` overrides this to route
    /// through zed's IPC bridge, which dispatches to the hKask `MediaRouter`
    /// held by the zed process. The media MCP server calls this through its
    /// `Arc<dyn InferencePort>` so it no longer needs its own `MediaRouter`.
    fn media_generate<'a>(&'a self, _op: &str, _params: &MediaGenerateParams) -> MediaFuture<'a> {
        let op = _op.to_string();
        Box::pin(async move {
            Err(InferenceError::Connection(format!(
                "media_generate not supported by this InferencePort (op: {op})"
            )))
        })
    }
}

/// A single chunk of streaming inference output. Final chunk has `finish_reason` + `usage`.
#[derive(Debug, Clone)]
pub struct InferenceStreamChunk {
    pub text_delta: String,
    /// Thinking-mode reasoning delta (Qwen3/GLM-5.2 `reasoning_content`,
    /// Ollama `delta.reasoning`). Empty when the provider emits no thinking.
    pub reasoning_delta: String,
    pub model: String,
    pub finish_reason: Option<String>,
    pub usage: Option<InferenceUsage>,
    pub tool_calls: Vec<StructuredToolCall>,
    /// USD cost of this inference call. Populated on the final chunk (or the
    /// single chunk from the default `generate_stream` impl). `None` for
    /// intermediate streaming chunks (cost is only known when the provider
    /// completes the response).
    pub cost_usd: Option<f64>,
}

impl From<InferenceResult> for InferenceStreamChunk {
    fn from(r: InferenceResult) -> Self {
        Self {
            text_delta: r.text,
            reasoning_delta: r.reasoning.unwrap_or_default(),
            model: r.model,
            finish_reason: Some(r.finish_reason),
            usage: Some(r.usage),
            tool_calls: r.tool_calls,
            cost_usd: r.cost_usd,
        }
    }
}

/// Blanket impl — enables `InferenceLoop<Arc<dyn InferencePort>>` default type param.
/// Vtable dispatch only at construction; hot path uses static dispatch.
impl InferencePort for Arc<dyn InferencePort> {
    fn generate(
        &self,
        p: &str,
        pa: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.as_ref().generate(p, pa, tools)
    }
    fn generate_with_model(
        &self,
        p: &str,
        pa: &LLMParameters,
        m: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.as_ref().generate_with_model(p, pa, m, tools)
    }
    fn generate_with_messages(
        &self,
        messages: &[ChatMessage],
        pa: &LLMParameters,
        m: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.as_ref().generate_with_messages(messages, pa, m, tools)
    }
    fn generate_stream_with_messages(
        &self,
        messages: &[ChatMessage],
        pa: &LLMParameters,
        m: Option<&str>,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        self.as_ref()
            .generate_stream_with_messages(messages, pa, m, tools)
    }
    fn generate_n(
        &self,
        p: &str,
        pa: &LLMParameters,
        n: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<InferenceResult>, InferenceError>> + Send + '_>>
    {
        self.as_ref().generate_n(p, pa, n)
    }
    fn generate_stream(
        &self,
        p: &str,
        pa: &LLMParameters,
        t: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        self.as_ref().generate_stream(p, pa, t)
    }
    fn generate_stream_with_model(
        &self,
        p: &str,
        pa: &LLMParameters,
        m: Option<&str>,
        t: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + '_>> {
        self.as_ref().generate_stream_with_model(p, pa, m, t)
    }
    fn generate_vision(
        &self,
        p: &str,
        imgs: &[String],
        pa: &LLMParameters,
        m: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>> {
        self.as_ref().generate_vision(p, imgs, pa, m)
    }
    fn embed<'a>(
        &'a self,
        model: &str,
        texts: &[String],
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Vec<f32>>, EmbeddingGenerationError>> + Send + 'a>>
    {
        self.as_ref().embed(model, texts)
    }
    fn list_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelEntry>, InferenceError>> + Send + 'a>> {
        self.as_ref().list_models()
    }
    fn list_vision_models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ModelEntry>, InferenceError>> + Send + 'a>> {
        self.as_ref().list_vision_models()
    }
    fn media_generate<'a>(&'a self, op: &str, params: &MediaGenerateParams) -> MediaFuture<'a> {
        self.as_ref().media_generate(op, params)
    }
}
