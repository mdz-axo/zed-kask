//! Backend dispatch traits + impls — the seam between `InferenceRouter` and provider backends.
//!
//! Each provider backend implements `ChatBackend` and (if it supports
//! multimodal input) `VisionBackend`. The router holds typed `Option<Backend>`
//! fields (single source of truth — no separate map, no `Arc`, no dual storage)
//! and exposes `chat_backend`/`vision_backend` match-fns that return
//! `&dyn ChatBackend`/`&dyn VisionBackend` borrowed from those fields. Dispatch
//! is a single match-fn call instead of a per-provider `match` arm that must be
//! kept in sync across six call sites (resolve_chat, dispatch_generate,
//! dispatch_generate_stream, media::generate_vision, models::list_models,
//! new). Adding a provider = implement the trait(s) + add a field + construct it
//! in `new` + add a match arm in `chat_backend`/`vision_backend`.
//!
//! The methods are dyn-safe via explicit `Pin<Box<dyn Future/Stream + Send>>`
//! return types (the same pattern `InferencePort` already uses), rather than
//! native `async fn` in trait (which is not object-safe without `async-trait`).
//! Each impl is a thin delegating wrapper around the backend's inherent
//! `async fn` / streaming method — the real behavior stays in the backends.
//!
//! Lifetime note: `generate` and `generate_vision` tie their return lifetime `'a`
//! to `&self` AND the data args, because their futures borrow the args (the args
//! are awaited across `.await` points). `generate_stream` ties its return ONLY to
//! `&self` — the backends clone the args into the stream
//! (`stream_chat_completion` takes owned values), so the returned stream does
//! not borrow the args. Tying the args to the stream's lifetime would wrongly
//! forbid returning a stream built from function-local data.

use crate::cline_backend::ClineBackend;
use crate::deepinfra_backend::DeepInfraBackend;
use crate::fal_backend::FalBackend;
use crate::kilocode_backend::KiloCodeBackend;
use crate::ollama_backend::OllamaBackend;
use crate::openrouter_backend::OpenRouterBackend;
use crate::runpod_backend::RunpodBackend;
use crate::together_backend::TogetherBackend;
use futures_util::Stream;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceStreamChunk,
};
use std::future::Future;
use std::pin::Pin;

/// Chat-completion backend: text generation and streaming.
///
/// Implemented by every provider that serves `/chat/completions`. RunPod is
/// excluded (it is vision/OCR-only) — it implements `VisionBackend` alone.
/// Model *listing* is intentionally NOT on this trait: `InferenceRouter::list_models`
/// iterates the typed backend fields and calls each backend's inherent
/// `list_models` directly, so listing stays decoupled from chat dispatch.
pub trait ChatBackend: Send + Sync {
    /// Generate a chat completion (non-streaming). The returned future borrows
    /// the args (they are awaited across `.await`).
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>>;

    /// Stream a chat completion as SSE chunks. The returned stream borrows only
    /// `&self` — backends clone the args into the stream — so the args may be
    /// short-lived (e.g. function-local data).
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>>;

    /// Generate a chat completion from an explicit message array (multi-turn).
    ///
    /// Each message carries its own role (`"system"`, `"user"`, `"assistant"`),
    /// so the provider sees the full conversation history instead of a flattened
    /// string. Default: flattens messages to a string and delegates to `generate`.
    /// Backends that speak the OpenAI wire format override this to pass the
    /// message array directly.
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        let prompt = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        Box::pin(async move { self.generate(model, &prompt, params, tools).await })
    }
}

/// Vision/multimodal backend: image-grounded generation.
///
/// Implemented by every provider that accepts base64 images alongside a prompt.
/// RunPod implements this (it serves vision/OCR) even though it is not a
/// `ChatBackend`.
pub trait VisionBackend: Send + Sync {
    /// Generate a vision-grounded completion from base64 images.
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>>;
}

// ── ChatBackend impls (7 chat-capable providers) ─────────────────────────────
//
// Each impl delegates to the backend's inherent pub method, boxing the future
// (`generate`/`generate_vision`) or returning the inherent stream directly
// (`generate_stream` — the inherent already returns a pinned boxed stream tied
// to &self).

impl ChatBackend for DeepInfraBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

impl ChatBackend for TogetherBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

impl ChatBackend for OpenRouterBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

impl ChatBackend for KiloCodeBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

impl ChatBackend for OllamaBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

impl ChatBackend for ClineBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

impl ChatBackend for FalBackend {
    fn generate<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate(model, prompt, params, tools))
    }
    fn generate_stream<'a>(
        &'a self,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<Box<dyn Stream<Item = Result<InferenceStreamChunk, InferenceError>> + Send + 'a>> {
        self.generate_stream(model, prompt, params, tools)
    }
    fn generate_with_messages<'a>(
        &'a self,
        model: &'a str,
        messages: &'a [ChatMessage],
        params: &'a LLMParameters,
        tools: Option<&'a [ChatToolDefinition]>,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_with_messages(model, messages, params, tools))
    }
}

// ── VisionBackend impls (all 8 providers serve vision) ───────────────────────

impl VisionBackend for DeepInfraBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for TogetherBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for OpenRouterBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for KiloCodeBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for OllamaBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for ClineBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for FalBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}

impl VisionBackend for RunpodBackend {
    fn generate_vision<'a>(
        &'a self,
        model: &'a str,
        prompt: &'a str,
        images: &'a [String],
        params: &'a LLMParameters,
    ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + 'a>> {
        Box::pin(self.generate_vision(model, prompt, images, params))
    }
}
