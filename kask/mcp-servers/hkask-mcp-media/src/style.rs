//! Style preset system — prompt augmentation via named presets (Fooocus
//! pattern). Each preset appends a style suffix and/or negative prompt to
//! the user's text prompt before dispatch, so the agent gets Fooocus-style
//! "zero-tuning" quality without manual prompt engineering.
//!
//! Presets affect prompts only; they do not select providers or models.
//! Media routing resolves `MediaGenerateParams::model` or the operation's
//! configured model independently of the selected style.

use hkask_types::MediaGenerateParams;

/// A style preset that augments the generation prompt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StylePreset {
    /// Suffix appended to the user's prompt (e.g.,
    /// ", cinematic lighting, dramatic shadows").
    pub prompt_suffix: Option<String>,
    /// Negative prompt (things to avoid), appended inline.
    pub negative_prompt: Option<String>,
}

/// Look up a built-in style preset by name.
///
/// Returns `None` for unknown presets.
#[must_use]
pub fn get_preset(name: &str) -> Option<StylePreset> {
    match name {
        "default" => Some(StylePreset {
            prompt_suffix: None,
            negative_prompt: Some("low quality, blurry, distorted".into()),
        }),
        "anime" => Some(StylePreset {
            prompt_suffix: Some(", anime style, cel shading, vibrant colors".into()),
            negative_prompt: Some("realistic, photographic, 3d render".into()),
        }),
        "realistic" => Some(StylePreset {
            prompt_suffix: Some(", photorealistic, high detail, natural lighting".into()),
            negative_prompt: Some("cartoon, anime, illustration, artificial".into()),
        }),
        "cinematic" => Some(StylePreset {
            prompt_suffix: Some(", cinematic composition, dramatic lighting, film grain".into()),
            negative_prompt: Some("flat lighting, amateur, snapshot".into()),
        }),
        "minimal" => Some(StylePreset {
            prompt_suffix: Some(", minimal, clean, simple composition".into()),
            negative_prompt: Some("cluttered, busy, complex".into()),
        }),
        _ => None,
    }
}

/// Apply a style preset to a media generation request, augmenting
/// the prompt in-place. If the preset has a `prompt_suffix`, it is
/// appended to the existing prompt. If it has a `negative_prompt`,
/// it is appended on a new line prefixed with "Negative:".
pub fn apply_preset(params: &mut MediaGenerateParams, preset: &StylePreset) {
    let mut prompt = params.prompt.take().unwrap_or_default();
    if let Some(suffix) = &preset.prompt_suffix {
        prompt.push_str(suffix);
    }
    if let Some(neg) = &preset.negative_prompt {
        prompt.push_str("\nNegative: ");
        prompt.push_str(neg);
    }
    params.prompt = Some(prompt);
}

/// List all available style preset names. Consumed by the `expand_prompt`
/// tool's error message when an unknown style is requested.
pub fn available_styles() -> &'static [&'static str] {
    &["default", "anime", "realistic", "cinematic", "minimal"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_preset_augments_prompt() {
        let mut params = MediaGenerateParams {
            prompt: Some("a cat in space".into()),
            ..Default::default()
        };
        let preset = get_preset("cinematic").unwrap();
        apply_preset(&mut params, &preset);
        assert!(params.prompt.as_ref().unwrap().contains("a cat in space"));
        assert!(
            params
                .prompt
                .as_ref()
                .unwrap()
                .contains("cinematic composition")
        );
        assert!(
            params
                .prompt
                .as_ref()
                .unwrap()
                .contains("Negative: flat lighting")
        );
    }

    #[test]
    fn default_preset_adds_negative_only() {
        let mut params = MediaGenerateParams {
            prompt: Some("a mountain".into()),
            ..Default::default()
        };
        let preset = get_preset("default").unwrap();
        apply_preset(&mut params, &preset);
        assert_eq!(
            params.prompt.as_deref(),
            Some("a mountain\nNegative: low quality, blurry, distorted")
        );
    }

    #[test]
    fn unknown_style_rejected() {
        assert!(get_preset("nonexistent").is_none());
        assert!(get_preset("default").is_some());
        assert!(get_preset("anime").is_some());
        assert!(get_preset("realistic").is_some());
        assert!(get_preset("cinematic").is_some());
        assert!(get_preset("minimal").is_some());
    }

    #[test]
    fn apply_preset_with_empty_prompt() {
        let mut params = MediaGenerateParams::default();
        let preset = get_preset("anime").unwrap();
        apply_preset(&mut params, &preset);
        assert!(params.prompt.as_ref().unwrap().contains("anime style"));
        assert!(
            params
                .prompt
                .as_ref()
                .unwrap()
                .contains("Negative: realistic")
        );
    }

    #[test]
    fn available_styles_lists_all_five() {
        let styles = available_styles();
        assert_eq!(styles.len(), 5);
        assert!(styles.contains(&"default"));
        assert!(styles.contains(&"anime"));
        assert!(styles.contains(&"realistic"));
        assert!(styles.contains(&"cinematic"));
        assert!(styles.contains(&"minimal"));
    }
}
