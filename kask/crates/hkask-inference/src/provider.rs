//! Media provider abstraction — pluggable, multi-provider media generation.
//!
//! A [`MediaProvider`] serves a subset of [`MediaOp`]s. [`ProviderRegistry`]
//! holds the registered providers in priority order and dispatches an op to
//! the first provider that supports it, falling back to the next on runtime
//! error. This is the registry that replaces the hardcoded two-field dispatch
//! in `MediaRouter`: adding a provider = implement `MediaProvider` + register
//! in `MediaRouter::new`; no dispatch edits.
//!
//! Two implementations exist from day one (`FalBackend`, `DeepInfraBackend`),
//! so this trait is not speculative generality. The registry order encodes the
//! existing policy: DeepInfra first (cheapest for background removal / TTS /
//! STT), fal.ai fallback for those three ops and sole provider for the rest.

use hkask_types::{InferenceError, MediaGenerateParams};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A media generation operation. New ops are added here; each provider
/// declares which subset it [`MediaProvider::supports`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaOp {
    GenerateImage,
    ImageToImage,
    RemoveBackground,
    Upscale,
    GenerateVideo,
    ImageToVideo,
    SegmentObject,
    GenerateSpeech,
    Transcribe,
    ExecuteWorkflow,
}

/// Parse the string op name used by `InferencePort::media_generate`.
///
/// expect: "The system maps string media ops to typed registry ops"
/// pre:  op is a known media op string
/// post: returns Ok(MediaOp), or Err(Connection) for an unknown op
impl std::str::FromStr for MediaOp {
    type Err = InferenceError;
    fn from_str(op: &str) -> Result<Self, Self::Err> {
        match op {
            "generate_image" => Ok(Self::GenerateImage),
            "image_to_image" => Ok(Self::ImageToImage),
            "remove_background" => Ok(Self::RemoveBackground),
            "upscale" => Ok(Self::Upscale),
            "generate_video" => Ok(Self::GenerateVideo),
            "image_to_video" => Ok(Self::ImageToVideo),
            "segment_object" => Ok(Self::SegmentObject),
            "generate_speech" => Ok(Self::GenerateSpeech),
            "transcribe" => Ok(Self::Transcribe),
            "execute_workflow" => Ok(Self::ExecuteWorkflow),
            other => Err(InferenceError::Connection(format!(
                "unknown media op: {other}"
            ))),
        }
    }
}

impl MediaOp {
    /// The canonical string name (matches `InferencePort::media_generate` op).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GenerateImage => "generate_image",
            Self::ImageToImage => "image_to_image",
            Self::RemoveBackground => "remove_background",
            Self::Upscale => "upscale",
            Self::GenerateVideo => "generate_video",
            Self::ImageToVideo => "image_to_video",
            Self::SegmentObject => "segment_object",
            Self::GenerateSpeech => "generate_speech",
            Self::Transcribe => "transcribe",
            Self::ExecuteWorkflow => "execute_workflow",
        }
    }
}

/// One provider behind the media membrane.
///
/// Implementations map the unified [`MediaGenerateParams`] to their
/// provider-specific call. The trait is `Send + Sync` so providers can live in
/// an `Arc<dyn MediaProvider>` behind the registry.
pub trait MediaProvider: Send + Sync {
    /// Stable provider id (e.g. `"fal.ai"`, `"deepinfra"`) for logging / audit.
    fn id(&self) -> &'static str;

    /// Whether this provider can serve `op`. The registry uses this to filter
    /// candidates before dispatch.
    fn supports(&self, op: MediaOp) -> bool;

    /// Execute `op` with the unified params.
    ///
    /// Implementations extract the fields they need from `params` (cloning
    /// owned `String`s into the future scope) and call their provider-specific
    /// method. The returned future borrows `self` and `params` for `'a`.
    fn execute<'a>(
        &'a self,
        op: MediaOp,
        params: &'a MediaGenerateParams,
    ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>>;
}

/// Ordered registry of media providers. Dispatches an op to the first
/// supporting provider, falling back on runtime error.
///
/// Order matters: register the preferred provider first. For the ops both
/// providers support (background removal / TTS / STT), DeepInfra is registered
/// first so it is preferred, with fal.ai as the runtime fallback — preserving
/// the pre-refactor `MediaRouter` policy exactly.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn MediaProvider>>,
}

impl ProviderRegistry {
    /// Build a registry from an ordered list of providers.
    #[must_use]
    pub fn new(providers: Vec<Arc<dyn MediaProvider>>) -> Self {
        Self { providers }
    }

    /// Whether at least one registered provider supports `op`.
    #[must_use]
    pub fn supports(&self, op: MediaOp) -> bool {
        self.providers.iter().any(|p| p.supports(op))
    }

    /// Number of registered providers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether the registry has no providers.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    /// Execute `op`, trying providers in priority order that support it. On
    /// error, falls back to the next supporting provider (with a `reg.inference`
    /// warn). Returns the first success, or the last error if all fail.
    ///
    /// expect: "The system routes media ops through the configured provider membrane"
    /// pre:  at least one provider supports op (otherwise returns Connection error)
    /// post: returns Ok(value) from the first succeeding provider
    /// post: if all supporting providers fail → Err(last error)
    /// post: fallback attempts emit a `reg.inference` warn naming the failed provider
    pub async fn execute(
        &self,
        op: MediaOp,
        params: &MediaGenerateParams,
    ) -> Result<Value, InferenceError> {
        let candidates: Vec<&Arc<dyn MediaProvider>> =
            self.providers.iter().filter(|p| p.supports(op)).collect();
        if candidates.is_empty() {
            return Err(InferenceError::Connection(format!(
                "no provider configured for media op: {}",
                op.as_str()
            )));
        }
        let mut last_err: Option<InferenceError> = None;
        for (idx, provider) in candidates.iter().enumerate() {
            match provider.execute(op, params).await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if idx + 1 < candidates.len() {
                        tracing::warn!(
                            target: "reg.inference",
                            provider = provider.id(),
                            op = op.as_str(),
                            error = %err,
                            "provider failed, falling back to next provider"
                        );
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            InferenceError::Connection(format!(
                "all providers failed for media op: {}",
                op.as_str()
            ))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A mock provider that supports a configurable op set and either succeeds
    /// (returns a marker value) or fails, recording call order.
    struct MockProvider {
        id: &'static str,
        supported: Vec<MediaOp>,
        fail: bool,
        calls: Arc<AtomicUsize>,
    }

    impl MockProvider {
        fn new(
            id: &'static str,
            supported: &[MediaOp],
            fail: bool,
            calls: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                id,
                supported: supported.to_vec(),
                fail,
                calls,
            }
        }
    }

    impl MediaProvider for MockProvider {
        fn id(&self) -> &'static str {
            self.id
        }
        fn supports(&self, op: MediaOp) -> bool {
            self.supported.contains(&op)
        }
        fn execute<'a>(
            &'a self,
            op: MediaOp,
            _params: &'a MediaGenerateParams,
        ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
            let calls = Arc::clone(&self.calls);
            let id = self.id;
            let fail = self.fail;
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                if fail {
                    Err(InferenceError::Connection(format!(
                        "{id} failed for {op:?}"
                    )))
                } else {
                    Ok(serde_json::json!({"provider": id, "op": op.as_str()}))
                }
            })
        }
    }

    fn empty_params() -> MediaGenerateParams {
        MediaGenerateParams::default()
    }

    #[test]
    fn media_op_roundtrips_str() {
        for op in [
            MediaOp::GenerateImage,
            MediaOp::ImageToImage,
            MediaOp::RemoveBackground,
            MediaOp::Upscale,
            MediaOp::GenerateVideo,
            MediaOp::ImageToVideo,
            MediaOp::SegmentObject,
            MediaOp::GenerateSpeech,
            MediaOp::Transcribe,
            MediaOp::ExecuteWorkflow,
        ] {
            assert_eq!(MediaOp::from_str(op.as_str()).unwrap(), op);
        }
        assert!(MediaOp::from_str("nonsense").is_err());
    }

    #[tokio::test]
    async fn registry_dispatches_to_first_supporting_provider() {
        let calls = Arc::new(AtomicUsize::new(0));
        // DeepInfra-shaped: supports RemoveBackground; fal-shaped: supports all.
        let deepinfra = MockProvider::new(
            "deepinfra",
            &[MediaOp::RemoveBackground],
            false,
            Arc::clone(&calls),
        );
        let fal = MockProvider::new(
            "fal.ai",
            &[MediaOp::GenerateImage],
            false,
            Arc::new(AtomicUsize::new(0)),
        );
        let registry = ProviderRegistry::new(vec![Arc::new(deepinfra), Arc::new(fal)]);
        let value = registry
            .execute(MediaOp::RemoveBackground, &empty_params())
            .await
            .unwrap();
        assert_eq!(value["provider"], "deepinfra");
    }

    #[tokio::test]
    async fn registry_falls_back_on_runtime_error() {
        // DeepInfra registered first and FAILS for RemoveBackground; fal.ai
        // (registered second, supports it) must be tried and succeed.
        let deepinfra = MockProvider::new(
            "deepinfra",
            &[MediaOp::RemoveBackground],
            true,
            Arc::new(AtomicUsize::new(0)),
        );
        let fal = MockProvider::new(
            "fal.ai",
            &[MediaOp::RemoveBackground],
            false,
            Arc::new(AtomicUsize::new(0)),
        );
        let registry = ProviderRegistry::new(vec![Arc::new(deepinfra), Arc::new(fal)]);
        let value = registry
            .execute(MediaOp::RemoveBackground, &empty_params())
            .await
            .unwrap();
        assert_eq!(
            value["provider"], "fal.ai",
            "must fall back to fal.ai when deepinfra fails"
        );
    }

    #[tokio::test]
    async fn registry_returns_error_when_no_provider_supports_op() {
        let deepinfra = MockProvider::new(
            "deepinfra",
            &[MediaOp::RemoveBackground],
            false,
            Arc::new(AtomicUsize::new(0)),
        );
        let registry = ProviderRegistry::new(vec![Arc::new(deepinfra)]);
        let err = registry
            .execute(MediaOp::GenerateImage, &empty_params())
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("no provider configured"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn registry_returns_last_error_when_all_candidates_fail() {
        let deepinfra = MockProvider::new(
            "deepinfra",
            &[MediaOp::Transcribe],
            true,
            Arc::new(AtomicUsize::new(0)),
        );
        let fal = MockProvider::new(
            "fal.ai",
            &[MediaOp::Transcribe],
            true,
            Arc::new(AtomicUsize::new(0)),
        );
        let registry = ProviderRegistry::new(vec![Arc::new(deepinfra), Arc::new(fal)]);
        let err = registry
            .execute(MediaOp::Transcribe, &empty_params())
            .await
            .unwrap_err();
        // Last error is from the last candidate tried (fal.ai).
        assert!(err.to_string().contains("fal.ai"), "got: {err}");
    }

    #[tokio::test]
    async fn registry_empty_errors_on_any_op() {
        let registry = ProviderRegistry::new(vec![]);
        assert!(registry.is_empty());
        let err = registry
            .execute(MediaOp::GenerateImage, &empty_params())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no provider"));
    }
}
