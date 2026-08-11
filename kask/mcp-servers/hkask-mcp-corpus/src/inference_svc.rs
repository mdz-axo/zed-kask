use std::sync::Arc;

use hkask_inference::{ProviderId, RouterModelEntry};
use hkask_types::InferencePort;

use hkask_services_core::ServiceError;

/// Inference context for the corpus server.
///
/// The `shared_port` is the IPC client that routes through zed's
/// `LanguageModelRegistry`. When available, all generation calls go through
/// it (with `model_override` for non-default models). The `inference_config`
/// is kept only for model listing (which the IPC client returns empty for —
/// model discovery needs the direct provider API).
pub struct InferenceContext {
    pub shared_port: Option<Arc<dyn InferencePort>>,
    pub default_model: String,
    pub inference_config: hkask_inference::InferenceConfig,
}

impl InferenceContext {
    #[must_use]
    pub fn from_parts(
        shared_port: Option<Arc<dyn InferencePort>>,
        default_model: impl Into<String>,
        inference_config: hkask_inference::InferenceConfig,
    ) -> Self {
        Self {
            shared_port,
            default_model: default_model.into(),
            inference_config,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub provider: ProviderId,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
    pub size_bytes: Option<u64>,
}

impl From<RouterModelEntry> for ModelInfo {
    fn from(entry: RouterModelEntry) -> Self {
        Self {
            name: entry.prefixed_name,
            provider: entry.provider,
            family: entry.family,
            parameter_size: entry.parameter_size,
            quantization_level: entry.quantization_level,
            size_bytes: entry.size_bytes,
        }
    }
}

impl From<hkask_types::ModelEntry> for ModelInfo {
    fn from(entry: hkask_types::ModelEntry) -> Self {
        // Parse the provider from the prefixed name (e.g. "deepinfra/qwen/..." → DeepInfra).
        let provider_str = entry
            .prefixed_name
            .split('/')
            .next()
            .unwrap_or("openrouter");
        let provider = ProviderId::from_prefix_segment(provider_str);
        Self {
            name: entry.prefixed_name,
            provider,
            family: None,
            parameter_size: None,
            quantization_level: None,
            size_bytes: None,
        }
    }
}

pub struct InferenceService;

impl InferenceService {
    #[must_use = "result must be used"]
    pub fn resolve_port(
        ctx: &InferenceContext,
        model: &str,
    ) -> Result<Arc<dyn InferencePort>, ServiceError> {
        tracing::info!(target: "hkask.inference_svc", operation = "resolve_port", model = %model, has_shared = ctx.shared_port.is_some(), "REG");

        if let Some(ref port) = ctx.shared_port {
            // The shared port (InferenceIpcClient) routes through zed's
            // LanguageModelRegistry. It supports `generate_with_model` with
            // a `model_override`, so any model can be routed through zed's
            // configured providers — no standalone MediaRouter fallback needed.
            return Ok(Arc::clone(port));
        }

        // No shared port — the IPC bridge isn't configured. This means
        // the MCP server wasn't launched by zed (or the socket is down).
        // Return an error rather than silently falling back to a standalone
        // MediaRouter with env-var credentials.
        Err(ServiceError::Domain {
            domain: hkask_services_core::DomainKind::Wallet,
            kind: hkask_services_core::ErrorKind::ServiceUnavailable,
            source: None,
            message: format!(
                "No inference port available — the zed IPC bridge is not configured. \
                 The MCP server must be launched by zed (set HKASK_INFERENCE_SOCKET). \
                 Requested model: {model}"
            ),
        })
    }

    #[must_use = "result must be used"]
    pub async fn list_models(ctx: &InferenceContext) -> Result<Vec<ModelInfo>, ServiceError> {
        tracing::info!(target: "hkask.inference_svc", operation = "list_models", "REG");
        // Lazy TTL cache: first call fetches live (the "start-up" update),
        // subsequent calls within the TTL return cached. See `model_cache`.
        crate::model_cache::ModelCache::list_models(ctx).await
    }

    #[must_use = "result must be used"]
    pub async fn search_models(
        ctx: &InferenceContext,
        query: &str,
    ) -> Result<Vec<ModelInfo>, ServiceError> {
        tracing::info!(target: "hkask.inference_svc", operation = "search_models", query = %query, "REG");
        // Search is a filter over the cached full list — one cache, filtered in-memory.
        let all = crate::model_cache::ModelCache::list_models(ctx).await?;
        if query.is_empty() {
            return Ok(all);
        }
        let lower = query.to_lowercase();
        Ok(all
            .into_iter()
            .filter(|m| m.name.to_lowercase().contains(&lower))
            .collect())
    }
}
