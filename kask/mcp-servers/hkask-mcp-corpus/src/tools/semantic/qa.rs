//! Model resolution and inference-error mapping for prepared QA generation.

/// Preserve configuration failures at the QA tool boundary instead of labelling
/// every failure a retryable provider outage.
pub(crate) fn map_qa_inference_error(error: hkask_types::InferenceError) -> crate::McpToolError {
    use hkask_types::InferenceError;
    let message = error.to_string();
    match error {
        InferenceError::Model(_) => crate::McpToolError::invalid_argument(message),
        InferenceError::NotConfigured(_) | InferenceError::Auth(_) => {
            crate::McpToolError::permission_denied(message)
        }
        InferenceError::Connection(_)
        | InferenceError::Overloaded(_)
        | InferenceError::CircuitOpen(_) => crate::McpToolError::unavailable(message),
        InferenceError::Timeout(_) => crate::McpToolError::unavailable(message),
        InferenceError::Generation(_)
        | InferenceError::Json(_)
        | InferenceError::VisionUnsupported(_) => crate::McpToolError::internal(message),
    }
}

/// Legacy consolidation selection only. QA generation uses the dedicated
/// `hkask_inference::model_constants::resolve_qa_generation_model` resolver.
pub(crate) fn configured_qa_model(requested_model: Option<String>) -> Option<String> {
    if let Some(m) = requested_model {
        return Some(m);
    }
    std::env::var("HKASK_QA_MODEL")
        .ok()
        .or_else(|| std::env::var("HKASK_DEFAULT_MODEL").ok())
}
