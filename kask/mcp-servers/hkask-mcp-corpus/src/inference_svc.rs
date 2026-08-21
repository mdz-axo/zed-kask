use std::sync::Arc;

use hkask_types::InferencePort;

use hkask_services_core::ServiceError;

/// Inference context for the corpus server.
///
/// The `shared_port` is the IPC client that routes through zed's
/// `LanguageModelRegistry`. When available, all generation calls go through
/// it (with `model_override` for non-default models). The `inference_config`
/// is kept only for model listing (which the IPC client returns empty for —
/// model discovery needs the direct provider API).
pub(crate) struct InferenceContext {
    pub shared_port: Option<Arc<dyn InferencePort>>,
    pub default_model: String,
}

impl InferenceContext {
    #[must_use]
    pub fn from_parts(
        shared_port: Option<Arc<dyn InferencePort>>,
        default_model: impl Into<String>,
    ) -> Self {
        Self {
            shared_port,
            default_model: default_model.into(),
        }
    }
}

pub(crate) struct InferenceService;

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
}
