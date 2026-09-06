//! Child-local media dispatch using the MCP process's env-injected keys.
//! Chat, vision, and embedding remain on their existing shared-port paths.

use crate::config::InferenceConfig;
use crate::provider::{MediaOp, MediaProvider, ProviderRegistry};
use hkask_types::{InferenceError, MediaGenerateParams};
use std::sync::Arc;

/// Media-only router. Missing selected credentials are reported at dispatch.
pub struct MediaRouter {
    registry: ProviderRegistry,
}

impl MediaRouter {
    /// Build the media router from an `InferenceConfig`.
    ///
    /// Constructs providers lazily — a provider is only created if its
    /// configuration is valid (non-empty API key). Providers that fail to
    /// construct are not registered and emit a `reg.inference` warn.
    ///
    /// expect: "The system creates provider membranes requiring valid API keys"
    /// \[P4\] Motivating: Clear Boundaries — providers registered only with valid keys
    /// pre:  none (reads config)
    /// post: returns MediaRouter whose registry holds all constructible providers
    #[must_use]
    pub fn new(config: InferenceConfig) -> Self {
        let client = Arc::new(reqwest::Client::new());
        let mut providers: Vec<Arc<dyn MediaProvider>> = Vec::new();

        match crate::media_providers::DeepInfraMediaProvider::new(&config, client.clone()) {
            Ok(provider) => providers.push(Arc::new(provider)),
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    "DeepInfra media provider not registered"
                );
            }
        }

        match crate::media_providers::OpenRouterMediaProvider::new(&config, client) {
            Ok(provider) => providers.push(Arc::new(provider)),
            Err(e) => {
                tracing::warn!(
                    target: "reg.inference",
                    error = %e,
                    "OpenRouter media provider not registered"
                );
            }
        }

        if providers.is_empty() {
            tracing::warn!(
                target: "reg.inference",
                "no media providers configured — all media generation will fail \
                 (set DEEPINFRA_API_KEY and/or OPENROUTER_API_KEY)"
            );
        }

        Self {
            registry: ProviderRegistry::new(providers),
        }
    }

    /// Resolve a provider-qualified model and execute exactly one media provider.
    pub async fn media_generate(
        &self,
        op: &str,
        params: &MediaGenerateParams,
    ) -> Result<serde_json::Value, InferenceError> {
        self.registry.execute(op.parse::<MediaOp>()?, params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn media_generate_unknown_op_errors() {
        let router = MediaRouter::new(InferenceConfig::default());
        assert!(matches!(router.media_generate("nonsense_op", &MediaGenerateParams::default()).await,
            Err(InferenceError::Model(message)) if message.contains("unknown media op")));
    }

    /// expect: "Missing keys name the selected provider, never another provider."
    /// [P4] No cross-provider credential substitution.
    #[tokio::test]
    async fn media_generate_no_provider_errors_clearly() {
        let router = MediaRouter::new(InferenceConfig::default());
        for (model, key) in [("OpenRouter/vendor/model", "OPENROUTER_API_KEY"),
                             ("DeepInfra/vendor/model", "DEEPINFRA_API_KEY")] {
            let error = router.media_generate("generate_image", &MediaGenerateParams {
                model: Some(model.into()), ..Default::default()
            }).await.expect_err("no credentials");
            assert!(matches!(error, InferenceError::NotConfigured(message) if message.contains(key)));
        }
    }
}
