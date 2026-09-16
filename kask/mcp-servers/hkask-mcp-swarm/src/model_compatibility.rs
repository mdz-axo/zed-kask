//! Evidence-backed model compatibility recommendations for explicit reasoning disable.

const REGISTRY_JSON: &str = include_str!("../../../registry/model-compatibility.json");

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct ModelCompatibilityRegistry {
    pub schema_version: u32,
    pub as_of: String,
    pub entries: Vec<ModelCompatibilityEntry>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub(crate) struct ModelCompatibilityEntry {
    pub provider_id: String,
    pub model_id: String,
    pub selection_name: String,
    pub display_name: String,
    pub quality_tier: String,
    pub supports_thinking_disabled: bool,
    pub transport_scope: String,
    pub capability_evidence_url: String,
    pub quality_evidence_url: String,
    pub observed_at: String,
    pub note: String,
}

pub(crate) fn registry() -> Result<ModelCompatibilityRegistry, String> {
    let registry: ModelCompatibilityRegistry = serde_json::from_str(REGISTRY_JSON)
        .map_err(|error| format!("model compatibility registry is invalid: {error}"))?;
    if registry.schema_version != 1 {
        return Err(format!(
            "unsupported model compatibility registry schema version {}",
            registry.schema_version
        ));
    }
    if registry.as_of.trim().is_empty() || registry.entries.is_empty() {
        return Err("model compatibility registry is empty".to_string());
    }
    for entry in &registry.entries {
        if entry.provider_id.trim().is_empty()
            || entry.model_id.trim().is_empty()
            || entry.selection_name.trim().is_empty()
            || entry.display_name.trim().is_empty()
            || !matches!(entry.quality_tier.as_str(), "frontier" | "near_frontier")
            || entry.transport_scope != "direct_provider"
            || entry.capability_evidence_url.trim().is_empty()
            || entry.quality_evidence_url.trim().is_empty()
            || entry.observed_at.trim().is_empty()
        {
            return Err(format!(
                "model compatibility registry entry '{}' is incomplete",
                entry.model_id
            ));
        }
    }
    Ok(registry)
}

impl ModelCompatibilityRegistry {
    pub(crate) fn recommendations(self, limit: usize) -> Vec<ModelCompatibilityEntry> {
        self.entries
            .into_iter()
            .filter(|entry| entry.supports_thinking_disabled)
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entries_are_versioned_sourced_and_direct_provider_scoped() {
        let registry = registry().expect("registry validates");
        assert_eq!(registry.schema_version, 1);
        assert_eq!(registry.as_of, "2026-09-16");
        assert!(!registry.entries.is_empty());
        for entry in registry.entries {
            assert!(entry.supports_thinking_disabled);
            assert!(matches!(
                entry.quality_tier.as_str(),
                "frontier" | "near_frontier"
            ));
            assert_eq!(entry.transport_scope, "direct_provider");
            assert!(entry.capability_evidence_url.starts_with("https://"));
            assert!(entry.quality_evidence_url.starts_with("https://"));
            assert!(!entry.note.contains("OpenRouter aliases are compatible"));
        }
    }

    #[test]
    fn recommendations_preserve_quality_order_and_limit() {
        let recommendations = registry().expect("registry validates").recommendations(2);
        let tiers = recommendations
            .iter()
            .map(|entry| entry.quality_tier.as_str())
            .collect::<Vec<_>>();
        assert_eq!(tiers, vec!["frontier", "near_frontier"]);
    }
}
