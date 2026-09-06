//! Strict media routing: resolve the operation's model once, then dispatch to
//! exactly the named provider. Provider errors retain their type; no fallback.

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
    GenerateSpeech,
    Transcribe,
    /// LLM reasoning over audio input — chat completions with `input_audio`
    /// content parts (the OpenAI audio-chat format). Audio in + prompt,
    /// text out; the Educt speaker pass's primary source.
    ChatAudio,
    /// Chat under a strict JSON Schema — OpenRouter structured outputs
    /// (`response_format: json_schema`). The provider enforces the schema;
    /// the Educt v2 pass mode's opt-in measurement instrument.
    ChatJson,
}

/// Parse the string op name used by `InferencePort::media_generate`.
///
/// expect: "The system maps string media ops to typed registry ops"
/// pre:  op is a known media op string
/// post: returns Ok(MediaOp), or Err(Model) for an unknown op
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
            "chat_json" => Ok(Self::ChatJson),
            other => Err(InferenceError::Model(format!(
                "unknown media op: {other}"
            ))),
        }
    }
}

impl MediaOp {
    pub(crate) fn model_env(self) -> Option<&'static str> {
        match self {
            Self::GenerateImage | Self::ImageToImage => Some("HKASK_MEDIA_IMAGE_GEN_MODEL"),
            Self::GenerateSpeech => Some("HKASK_MEDIA_TTS_MODEL"),
            Self::Transcribe => Some("HKASK_MEDIA_STT_MODEL"),
            Self::GenerateVideo | Self::ImageToVideo => Some("HKASK_MEDIA_VIDEO_MODEL"),
            Self::ChatAudio => Some("HKASK_MEDIA_AUDIO_CHAT_MODEL"),
            Self::ChatJson => Some("HKASK_MEDIA_STRUCTURED_PASS_MODEL"),
            Self::RemoveBackground | Self::Upscale => None,
        }
    }

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
            Self::ChatJson => "chat_json",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoProvider(&'static str);

    impl MediaProvider for EchoProvider {
        fn id(&self) -> &'static str {
            self.0
        }
        fn supports(&self, _: MediaOp) -> bool {
            true
        }
        fn execute<'a>(
            &'a self,
            _: MediaOp,
            params: &'a MediaGenerateParams,
        ) -> Pin<Box<dyn Future<Output = Result<Value, InferenceError>> + Send + 'a>> {
            Box::pin(
                async move { Ok(serde_json::json!({"provider": self.0, "model": params.model})) },
            )
        }
    }

    /// expect: "My OpenRouter model never sends my media to DeepInfra."
    /// [P1] Motivating: honor the user's selected provider.
    /// dcterms:identifier: ProviderRegistry::execute
    #[tokio::test]
    async fn qualified_model_selects_provider_not_registration_order() {
        let registry = ProviderRegistry::new(vec![
            Arc::new(EchoProvider("deepinfra")),
            Arc::new(EchoProvider("openrouter")),
        ]);
        let result = registry
            .execute(
                MediaOp::GenerateImage,
                &MediaGenerateParams {
                    model: Some("OpenRouter/black-forest-labs/flux.2-klein-4b".into()),
                    ..Default::default()
                },
            )
            .await
            .expect("selected provider succeeds");
        assert_eq!(result["provider"], "openrouter");
        assert_eq!(result["model"], "black-forest-labs/flux.2-klein-4b");
    }

    #[test]
    fn chat_audio_op_round_trips() {
        let op: MediaOp = "chat_audio".parse().expect("chat_audio parses");
        assert_eq!(op, MediaOp::ChatAudio);
        assert_eq!(op.as_str(), "chat_audio");
    }

    #[test]
    fn chat_json_op_round_trips() {
        let op: MediaOp = "chat_json".parse().expect("chat_json parses");
        assert_eq!(op, MediaOp::ChatJson);
        assert_eq!(op.as_str(), "chat_json");
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

    /// Whether this provider can serve `op`; never authorizes another provider.
    fn supports(&self, op: MediaOp) -> bool;

    /// Execute `op` with resolved params, not user-facing configuration.
    ///
    /// Direct callers must supply a nonempty provider-local `params.model` for
    /// selectable operations (no provider prefix). Fixed operations require
    /// `model: None`. Use `ProviderRegistry::execute` for user-facing models:
    /// adapters never resolve environment defaults or strip provider prefixes.
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

/// Registered media backends, looked up by the selected provider's name.
/// Registration order never grants permission to dispatch elsewhere.
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn MediaProvider>>,
}

impl ProviderRegistry {
    #[must_use]
    pub fn new(providers: Vec<Arc<dyn MediaProvider>>) -> Self {
        Self { providers }
    }

    /// expect: "My media goes only to the provider I selected, including on failure."
    /// [P1] Motivating: preserve the user's model and provider choice.
    /// [P4] Constraining: no implicit cross-provider data transfer.
    /// pre: model override or per-operation configuration uses a full provider prefix;
    /// fixed-model operations carry no override.
    /// post: at most one provider executes, with only the provider-local model;
    /// invalid selection performs no request; provider errors return unchanged.
    pub async fn execute(
        &self,
        op: MediaOp,
        params: &MediaGenerateParams,
    ) -> Result<Value, InferenceError> {
        let mut params = params.clone();
        let (provider_name, credential) = if let Some(variable) = op.model_env() {
            let model = match params.model.as_ref() {
                Some(model) => model.clone(),
                None => std::env::var(variable).map_err(|_| {
                    InferenceError::NotConfigured(format!(
                        "set {variable} or pass a provider-qualified model for {}", op.as_str()
                    ))
                })?,
            };
            let invalid_model = || InferenceError::Model(
                "media models require OpenRouter/<model> or DeepInfra/<model>; use full provider names, a nonempty model, and no whitespace".into()
            );
            let (prefix, local_model) = model.split_once('/').ok_or_else(invalid_model)?;
            if model.chars().any(char::is_whitespace)
                || local_model.split('/').any(str::is_empty)
            {
                return Err(invalid_model());
            }
            let selected = if prefix.eq_ignore_ascii_case("OpenRouter") {
                ("openrouter", "OPENROUTER_API_KEY")
            } else if prefix.eq_ignore_ascii_case("DeepInfra") {
                ("deepinfra", "DEEPINFRA_API_KEY")
            } else {
                return Err(invalid_model());
            };
            params.model = Some(local_model.to_owned());
            selected
        } else {
            if params.model.is_some() {
                return Err(InferenceError::Model(format!(
                    "{} uses a fixed DeepInfra model; remove the model override", op.as_str()
                )));
            }
            ("deepinfra", "DEEPINFRA_API_KEY")
        };
        let mut matching = self.providers.iter()
            .filter(|provider| provider.id().eq_ignore_ascii_case(provider_name));
        let provider = matching.next().ok_or_else(|| InferenceError::NotConfigured(format!(
            "{provider_name} is not configured for {}; set {credential}", op.as_str()
        )))?;
        if matching.next().is_some() {
            return Err(InferenceError::Model(format!(
                "ambiguous media provider registration: {provider_name}"
            )));
        }
        if !provider.supports(op) {
            return Err(InferenceError::Model(format!(
                "{provider_name} does not support {}; choose a supported provider-qualified model", op.as_str()
            )));
        }
        provider.execute(op, &params).await
    }
}
