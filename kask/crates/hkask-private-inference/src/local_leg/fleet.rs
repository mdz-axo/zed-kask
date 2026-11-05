//! The served fleet — the shipped model briefs (M9's fleet layer).
//!
//! The fleet is the SET of briefs the private local leg serves: which
//! models ship with the binary, projected into model cards for the M3
//! `openai_compatible` settings entry. It is the model-side data
//! counterpart of the engine manifest's surface pin: the list changes
//! when models join, and a model joins the fleet by a brief TOML under
//! `kask/registry/model-briefs/` plus one entry in
//! [`SHIPPED_BRIEF_SOURCES`] — the D1 template-shipping precedent
//! (registry data ships with the binary, so a missing brief is a
//! compile error, not a runtime hole).

use super::model_brief::{ModelBrief, ModelBriefError};

/// The shipped brief sources — one `(model_id, TOML source)` pair per
/// fleet model, embedded at compile time from the registry. The pair
/// carries the id alongside the source so a decode failure can name
/// the intended brief, and so the drift pin (`model_id` inside the
/// TOML must equal the entry's name) can catch a renamed file or a
/// copied brief.
pub const SHIPPED_BRIEF_SOURCES: &[(&str, &str)] = &[
    (
        "qwen3-8-flash-next",
        include_str!("../../../../registry/model-briefs/qwen3-8-flash-next.toml"),
    ),
    (
        "gemma-4-31b",
        include_str!("../../../../registry/model-briefs/gemma-4-31b.toml"),
    ),
];

/// Fleet projection failures — per-variant, never a blanket string
/// (the string-errors discipline).
#[derive(Debug, thiserror::Error)]
pub enum FleetError {
    /// A shipped brief failed to decode. The whole projection fails
    /// closed — the caller surfaces the error and emits nothing, never
    /// a partial fleet.
    #[error("shipped model brief `{model_id}` failed to decode: {source}")]
    ShippedDecode {
        model_id: String,
        source: ModelBriefError,
    },
}

/// Decode every shipped brief through the one decode home
/// (`ModelBrief::decode`).
pub fn shipped_model_briefs() -> Result<Vec<ModelBrief>, FleetError> {
    SHIPPED_BRIEF_SOURCES
        .iter()
        .map(|(model_id, source)| {
            ModelBrief::decode(source).map_err(|source| FleetError::ShippedDecode {
                model_id: (*model_id).to_string(),
                source,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_briefs_decode_and_carry_card_data() {
        let fleet = shipped_model_briefs().expect("every shipped brief decodes");
        assert_eq!(fleet.len(), SHIPPED_BRIEF_SOURCES.len());
        for (brief, (model_id, _)) in fleet.iter().zip(SHIPPED_BRIEF_SOURCES) {
            // The drift pin: the brief's own id must equal the
            // registry entry's name — a renamed file or a copied brief
            // surfaces here, not as a mismatched model card.
            assert_eq!(&brief.model_id, model_id);
            // Card data: the model card renders display_name and
            // max_tokens (the recommended context length) — a brief
            // without them cannot produce a card.
            assert!(!brief.display_name.is_empty());
            assert!(brief.recommendations.context_length > 0);
        }
    }

    #[test]
    fn fleet_ids_are_unique() {
        let fleet = shipped_model_briefs().expect("fleet decodes");
        let mut ids: Vec<&str> = fleet.iter().map(|b| b.model_id.as_str()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate model_id in the shipped fleet");
    }
}
