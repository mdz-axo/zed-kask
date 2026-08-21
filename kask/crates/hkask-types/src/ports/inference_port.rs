use super::EmbeddingGenerationError;
use super::inference_types::InferenceStreamChunk;
use super::inference_types::{ChatMessage, ChatToolDefinition, InferenceError, InferenceResult};
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

/// LLM invocation boundary. Uses ``Pin<Box<dyn Future>>`` (not `async_trait`) for object-safety.
/// A model available from an inference provider.
///
/// Returned by `InferencePort::list_models`; lives in `hkask-types` so the
/// `InferencePort` trait can return it without depending on `hkask-inference`.
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
    /// `ollama/nomic-embed-text`). The implementation strips the
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
}
