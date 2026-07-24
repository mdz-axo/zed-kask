//! Model listing and search across all configured providers.

use super::InferenceRouter;
use crate::RouterModelEntry;

/// Collect models from an optional backend, extending the entries vec.
macro_rules! collect {
    ($entries:expr, $self:expr, $field:ident) => {
        if let Some(ref backend) = $self.$field {
            $entries.extend(backend.list_models().await);
        }
    };
}

impl InferenceRouter {
    /// List all available models across all configured providers.
    ///
    /// Queries each backend and merges results with provider prefixes applied.
    /// Graceful degradation: if one provider fails, results from others are still
    /// returned. RunPod's `list_models` returns empty (its models are discovered
    /// via template configuration, not API listing) so including it is a no-op.
    ///
    /// expect: "I can discover available models across providers"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — aggregated model variety across providers
    /// pre:  backends are initialized (may be None)
    /// post: returns `Vec<RouterModelEntry>` with all available models across providers
    /// post: if a backend fails → its models are omitted (graceful degradation)
    #[must_use]
    pub async fn list_models(&self) -> Vec<RouterModelEntry> {
        let mut entries = Vec::new();

        collect!(entries, self, deepinfra);
        collect!(entries, self, fal);
        collect!(entries, self, together);
        collect!(entries, self, openrouter);
        collect!(entries, self, kilocode);
        collect!(entries, self, runpod);
        collect!(entries, self, ollama);
        collect!(entries, self, cline);

        entries
    }

    /// Search models by name across all providers (case-insensitive substring).
    ///
    /// expect: "I can discover available models across providers"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — searchable model catalog for routing
    /// pre:  query may be empty (returns all models)
    /// post: returns `Vec<RouterModelEntry>` filtered by case-insensitive substring match
    /// post: if query is empty → returns all models (delegates to list_models)
    #[must_use]
    pub async fn search_models(&self, query: &str) -> Vec<RouterModelEntry> {
        let all = self.list_models().await;
        if query.is_empty() {
            return all;
        }
        let lower = query.to_lowercase();
        all.into_iter()
            .filter(|m| m.model.to_lowercase().contains(&lower))
            .collect()
    }

    /// List only models that are likely vision-capable.
    ///
    /// Convenience filter over `list_models()` using the heuristic
    /// `supports_vision` flag. Useful for OCR model selection.
    ///
    /// expect: "I can discover available models across providers"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — vision-capable model discovery
    /// pre:  none (delegates to list_models)
    /// post: returns `Vec<RouterModelEntry>` filtered to supports_vision == Some(true)
    #[must_use]
    pub async fn list_vision_models(&self) -> Vec<RouterModelEntry> {
        self.list_models()
            .await
            .into_iter()
            .filter(|m| m.supports_vision == Some(true))
            .collect()
    }
}
