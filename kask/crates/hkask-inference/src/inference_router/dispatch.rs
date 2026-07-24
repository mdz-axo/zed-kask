//! Provider dispatch — routes generate calls to resolved backends via the
//! `chat_backend` match-fn, which returns `&dyn ChatBackend` borrowed from the
//! typed fields.

use super::InferenceRouter;
use crate::config::ProviderId;
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatMessage, ChatToolDefinition, InferenceError, InferenceResult, InferenceStreamChunk,
};
use std::pin::Pin;

impl InferenceRouter {
    /// Dispatch a generate call to the resolved chat backend.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — shared dispatch for text generation
    /// pre:  provider is a resolved ProviderId with an available chat backend
    /// pre:  model, prompt, params are validated
    /// post: returns Ok(InferenceResult) on success
    /// post: returns Err(Connection) if no chat backend is configured for provider
    pub(crate) async fn dispatch_generate(
        &self,
        provider: ProviderId,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Result<InferenceResult, InferenceError> {
        let backend = self.chat_backend(provider).ok_or_else(|| {
            InferenceError::Connection(format!(
                "Provider {} is not available (check configuration)",
                provider.as_str()
            ))
        })?;
        backend.generate(model, prompt, params, tools).await
    }

    /// Dispatch a multi-turn generate call to the resolved chat backend.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — shared dispatch for multi-turn text generation
    /// pre:  provider is a resolved ProviderId with an available chat backend
    /// pre:  model, messages, params are validated
    /// post: returns Ok(InferenceResult) on success
    /// post: returns Err(Connection) if no chat backend is configured for provider
    pub(crate) async fn dispatch_generate_messages(
        &self,
        provider: ProviderId,
        model: &str,
        messages: &[ChatMessage],
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Result<InferenceResult, InferenceError> {
        let backend = self.chat_backend(provider).ok_or_else(|| {
            InferenceError::Connection(format!(
                "Provider {} is not available (check configuration)",
                provider.as_str()
            ))
        })?;
        backend
            .generate_with_messages(model, messages, params, tools)
            .await
    }

    /// Dispatch a streaming generate call to the resolved chat backend.
    ///
    /// expect: "The system dispatches regulated inference to the correct provider"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — shared dispatch for streaming
    /// pre:  provider is a resolved ProviderId with an available chat backend
    /// post: returns a stream of Ok(InferenceStreamChunk) on success
    /// post: returns a stream yielding Err(Connection) if no chat backend is configured
    pub(crate) fn dispatch_generate_stream(
        &self,
        provider: ProviderId,
        model: &str,
        prompt: &str,
        params: &LLMParameters,
        tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<
            dyn futures_util::Stream<Item = Result<InferenceStreamChunk, InferenceError>>
                + Send
                + '_,
        >,
    > {
        // No owned locals needed: `generate_stream`'s return borrows only
        // `&self` (backends clone the args), so the borrowed args from the caller
        // can be passed straight through.
        match self.chat_backend(provider) {
            Some(backend) => backend.generate_stream(model, prompt, params, tools),
            None => {
                let provider_str = provider.as_str().to_string();
                Box::pin(futures_util::stream::once(async move {
                    Err(InferenceError::Connection(format!(
                        "Provider {} is not available (check configuration)",
                        provider_str
                    )))
                }))
            }
        }
    }
}
