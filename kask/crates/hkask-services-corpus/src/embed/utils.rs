//! Utility helpers for the embedding pipeline.

/// Strip a recognized 2-letter provider prefix from a model name.
///
/// "DI/Qwen/Qwen3-235B-A22B-Instruct-2507" → "Qwen/Qwen3-235B-A22B-Instruct-2507"
/// "qwen/qwen3.5-35b-a3b"           → "qwen/qwen3.5-35b-a3b" (no recognized prefix)
///
/// Used before sending model IDs to classifier base URLs, which determine
/// the provider independently of the model string. Note: this only strips
/// the router prefix; it does NOT lowercase or normalize the id.
pub(crate) fn strip_provider_prefix(model: &str) -> &str {
    for prefix in ["DI/", "KC/", "FA/", "TG/", "OR/", "RP/", "BT/"] {
        if let Some(rest) = model.strip_prefix(prefix) {
            return rest;
        }
    }
    model
}
