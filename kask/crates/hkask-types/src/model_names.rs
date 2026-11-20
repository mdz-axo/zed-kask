//! Model-id name handling shared across the kask families.
//!
//! [`strip_provider_prefix`] is the one implementation of provider-prefix
//! stripping for model ids. It was previously duplicated between
//! hkask-inference's IPC client and hkask-mcp-media's model discovery with
//! identical semantics but independent copies — a drift class the inference
//! copy's own test history documents (`split('/').nth(1)` once truncated
//! `OpenRouter/z-ai/glm-5.2` to `z-ai`); a second copy could re-grow that
//! bug on its own. Unified by the 2026-11-16 kask audit.

/// Strip the provider prefix (the first `/`-segment) from a model id.
///
/// `"OpenRouter/z-ai/glm-5.2"` → `"z-ai/glm-5.2"`;
/// `"OpenRouter/qwen/qwen3-embedding-8b"` → `"qwen/qwen3-embedding-8b"`;
/// `"no-slash"` → `"no-slash"`. Used by `list_models`
/// to produce the `ModelEntry.model` ("raw model name without prefix") from
/// the bridge's `ModelListEntry.name` ("full name with provider prefix").
///
/// Strips only the first segment: a model id may itself contain a slash
/// (OpenRouter's `vendor/model` convention), so `split_once` is correct where
/// the prior `split('/').nth(1)` truncated `OpenRouter/z-ai/glm-5.2` to `z-ai`.
#[must_use]
pub fn strip_provider_prefix(name: &str) -> &str {
    match name.split_once('/') {
        Some((_, rest)) => rest,
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_first_segment_only() {
        // The bug at the old inline `split('/').nth(1)`: this returned "z-ai".
        assert_eq!(
            strip_provider_prefix("OpenRouter/z-ai/glm-5.2"),
            "z-ai/glm-5.2"
        );
        assert_eq!(
            strip_provider_prefix("OpenRouter/qwen/qwen3-embedding-8b"),
            "qwen/qwen3-embedding-8b"
        );
        assert_eq!(
            strip_provider_prefix("Provider/family/model"),
            "family/model"
        );
    }

    #[test]
    fn no_slash_and_edge_slashes() {
        assert_eq!(strip_provider_prefix("no-slash"), "no-slash");
        assert_eq!(strip_provider_prefix("/leading-slash"), "leading-slash");
        assert_eq!(strip_provider_prefix("trailing-slash/"), "");
    }
}
