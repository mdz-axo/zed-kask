//! Media provider abstraction — pluggable, multi-provider media generation.
//!
//! A [`MediaProvider`] serves a subset of [`MediaOp`]s. [`ProviderRegistry`]
//! holds the registered providers in priority order and dispatches an op to
//! the first provider that supports it, falling back to the next on runtime
//! error. This is the registry that replaces the hardcoded two-field dispatch
//! in `MediaRouter`: adding a provider = implement `MediaProvider` + register
//! in `MediaRouter::new`; no dispatch edits.
//!
//! No implementations are currently registered (the former media backends
//! were removed); the trait + registry remain the generic dispatch
//! infrastructure for providers added in the future.

use hkask_types::{InferenceError, MediaGenerateParams};
use serde_json::Value;
use std::cmp::Ordering;
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
    GenerateSpeech,
    Transcribe,
    /// LLM reasoning over audio input — chat completions with `input_audio`
    /// content parts (the OpenAI audio-chat format). Audio in + prompt,
    /// text out; the Educt speaker pass's primary source.
    ChatAudio,
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
            "generate_speech" => Ok(Self::GenerateSpeech),
            "transcribe" => Ok(Self::Transcribe),
            "chat_audio" => Ok(Self::ChatAudio),
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
            Self::GenerateSpeech => "generate_speech",
            Self::Transcribe => "transcribe",
            Self::ChatAudio => "chat_audio",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_audio_op_round_trips() {
        let op: MediaOp = "chat_audio".parse().expect("chat_audio parses");
        assert_eq!(op, MediaOp::ChatAudio);
        assert_eq!(op.as_str(), "chat_audio");
    }
}

/// One provider behind the media membrane.
///
/// Implementations map the unified [`MediaGenerateParams`] to their
/// provider-specific call. The trait is `Send + Sync` so providers can live in
/// an `Arc<dyn MediaProvider>` behind the registry.
pub trait MediaProvider: Send + Sync {
    /// Stable provider id (e.g. `"openrouter"`) for logging / audit.
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
/// Order matters: register the preferred provider first.
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

    /// Slice of all registered providers (for scored selection / iteration).
    #[must_use]
    pub fn providers(&self) -> &[Arc<dyn MediaProvider>] {
        &self.providers
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
    /// warn). Returns the first success, or an error listing every provider
    /// failure in attempt order.
    ///
    /// When multiple providers can serve `op`, the primary is chosen via the
    /// 7-dimension scored engine (`scoring::select_scored`), which emits the
    /// `reg.media.select` span, and the fallback chain is ordered by descending
    /// weighted score. With a single candidate there is no selection to make —
    /// the lone provider is used directly.
    ///
    /// expect: "The system routes media ops through the configured provider membrane"
    /// pre:  at least one provider supports op (otherwise returns Connection error)
    /// post: returns Ok(value) from the first succeeding provider
    /// post: if all supporting providers fail → Err listing every provider
    ///       failure in attempt order (the primary's error is not masked by
    ///       the fallback's)
    /// post: fallback attempts emit a `reg.inference` warn naming the failed provider
    /// post: multi-provider ops emit a `reg.media.select` span with candidate scores
    pub async fn execute(
        &self,
        op: MediaOp,
        params: &MediaGenerateParams,
    ) -> Result<Value, InferenceError> {
        let candidates: Vec<Arc<dyn MediaProvider>> = self
            .providers
            .iter()
            .filter(|p| p.supports(op))
            .map(Arc::clone)
            .collect();
        if candidates.is_empty() {
            return Err(InferenceError::NotConfigured(format!(
                "no provider configured for media op: {}",
                op.as_str()
            )));
        }

        // When multiple providers can serve the op, select the primary via the
        // 7-dimension scored engine (which emits the `reg.media.select` span)
        // and order the fallback chain by descending weighted score. With a
        // single candidate there is no selection to make — use it directly
        // so single-provider ops don't emit a spurious selection span.
        let ordered: Vec<Arc<dyn MediaProvider>> = if candidates.len() > 1 {
            let (chosen, scores) = crate::scoring::select_scored(self, op)?;
            let chosen_id = chosen.id();
            let mut by_score: Vec<Arc<dyn MediaProvider>> = Vec::with_capacity(candidates.len());
            by_score.push(chosen);
            // Remaining candidates, best weighted score first.
            let mut remaining: Vec<&crate::scoring::ScoredProvider> =
                scores.iter().filter(|s| s.id != chosen_id).collect();
            remaining.sort_by(|a, b| {
                b.weighted
                    .partial_cmp(&a.weighted)
                    .unwrap_or(Ordering::Equal)
            });
            for s in remaining {
                if let Some(p) = self.providers.iter().find(|p| p.id() == s.id.as_str()) {
                    by_score.push(Arc::clone(p));
                }
            }
            by_score
        } else {
            candidates
        };

        let mut failures: Vec<String> = Vec::new();
        for (idx, provider) in ordered.iter().enumerate() {
            match provider.execute(op, params).await {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if idx + 1 < ordered.len() {
                        tracing::warn!(
                            target: "reg.inference",
                            provider = provider.id(),
                            op = op.as_str(),
                            error = %err,
                            "provider failed, falling back to next provider"
                        );
                    }
                    failures.push(format!("{}: {err}", provider.id()));
                }
            }
        }
        // Every provider failure is listed in attempt order — returning only
        // the last error masked the primary's (a 401 "invalid key" was hidden
        // behind the fallback's "invalid model" 400, sending the operator
        // debugging the wrong layer).
        Err(InferenceError::Connection(format!(
            "all providers failed for media op: {} — {}",
            op.as_str(),
            failures.join("; ")
        )))
    }
}
