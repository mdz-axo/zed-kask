//! Shared test-support doubles for hKask MCP servers.
//!
//! `#[doc(hidden)]` keeps this out of the public docs while server
//! integration tests reach it through the lib (the kata-kanban
//! `test_support` precedent). Not part of the server API surface.
//!
//! The `NoopInferencePort` replaces the per-server inline mock each
//! server's end-to-end tests hand-rolled (~24 lines each; the
//! duplication ratchet measured the cross-server copies as clusters
//! after the A-3 rollout gave every server the same e2e test shape).

use std::pin::Pin;

use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatToolDefinition, EmbedWithIdentityFuture, EmbeddingGenerationError, InferenceError,
    InferencePort, InferenceResult,
};

/// An inference port that always fails with a typed Connection error —
/// the "not configured for this test" double.
///
/// Local operations that need no inference still work; every
/// inference-dependent operation surfaces a structured error instead of
/// succeeding silently. The embed arm fails the same way (a test that
/// needs embeddings provides its own port).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopInferencePort;

impl InferencePort for NoopInferencePort {
    fn generate(
        &self,
        _prompt: &str,
        _parameters: &LLMParameters,
        _tools: Option<&[ChatToolDefinition]>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>,
    > {
        Box::pin(async {
            Err(InferenceError::Connection(
                "noop inference port — not configured for this test".into(),
            ))
        })
    }

    fn embed_with_dimensions<'a>(
        &'a self,
        _model: &str,
        _texts: &[String],
        _dimensions: Option<u32>,
    ) -> EmbedWithIdentityFuture<'a> {
        Box::pin(async {
            Err(EmbeddingGenerationError::Connection(
                "noop inference port — not configured for this test".into(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generate_fails_with_connection_error() {
        let err = NoopInferencePort
            .generate("hi", &LLMParameters::default(), None)
            .await
            .expect_err("the noop port always fails");
        assert!(
            matches!(err, InferenceError::Connection(_)),
            "typed Connection error, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn embed_fails_with_connection_error() {
        let err = NoopInferencePort
            .embed_with_dimensions("m", &["t".into()], None)
            .await
            .expect_err("the noop port always fails");
        assert!(
            matches!(err, EmbeddingGenerationError::Connection(_)),
            "typed Connection error, got: {err:?}"
        );
    }
}
