//! Model browser tools — enumerate available media generation models.
//!
//! Fills the OMC `Participant` concept: the model/provider is a participant in
//! the creation task. The model list is constructed from `model_constants`
//! defaults (resolvable via env vars) and their known `MediaOp` capabilities.
use crate::types::{MediaModelInfo, ModelInfoRequest, ModelListRequest};
use crate::*;

/// Build the full list of configured media models from `model_constants` defaults.
///
/// Each model is annotated with its provider (parsed from the prefixed name),
/// modality, and capabilities. The `is_default` flag is true for all entries —
/// these are the configured defaults. Future dynamic enumeration will add
/// non-default models from provider APIs.
fn build_model_list() -> Vec<MediaModelInfo> {
    use crate::models;

    let image_model = models::image_gen_model();
    let video_model = hkask_inference::model_constants::resolve(
        "HKASK_MEDIA_VIDEO_MODEL",
        hkask_inference::model_constants::DEFAULT_VIDEO_MODEL,
    );
    let tts_model = models::tts_model();
    let stt_model = models::stt_model();
    let vision_model = models::vision_model();

    vec![
        MediaModelInfo {
            id: image_model.clone(),
            name: strip_provider_prefix(&image_model).to_string(),
            provider: parse_provider(&image_model),
            modality: "image".to_string(),
            capabilities: vec!["generate_image".to_string(), "image_to_image".to_string()],
            is_default: true,
            description: Some("Image generation and transformation".to_string()),
        },
        MediaModelInfo {
            id: video_model.clone(),
            name: strip_provider_prefix(&video_model).to_string(),
            provider: parse_provider(&video_model),
            modality: "video".to_string(),
            capabilities: vec!["generate_video".to_string(), "image_to_video".to_string()],
            is_default: true,
            description: Some("Text-to-video and image-to-video generation".to_string()),
        },
        MediaModelInfo {
            id: tts_model.clone(),
            name: strip_provider_prefix(&tts_model).to_string(),
            provider: parse_provider(&tts_model),
            modality: "audio".to_string(),
            capabilities: vec!["generate_speech".to_string()],
            is_default: true,
            description: Some("Text-to-speech voice synthesis".to_string()),
        },
        MediaModelInfo {
            id: stt_model.clone(),
            name: strip_provider_prefix(&stt_model).to_string(),
            provider: parse_provider(&stt_model),
            modality: "audio".to_string(),
            capabilities: vec!["transcribe".to_string()],
            is_default: true,
            description: Some("Speech-to-text transcription".to_string()),
        },
        MediaModelInfo {
            id: vision_model.clone(),
            name: strip_provider_prefix(&vision_model).to_string(),
            provider: parse_provider(&vision_model),
            modality: "vision".to_string(),
            capabilities: vec![
                "describe_image".to_string(),
                "gallery_analyze".to_string(),
                "video_caption".to_string(),
                "expand_prompt".to_string(),
            ],
            is_default: true,
            description: Some(
                "Vision LLM for image analysis, scene description, and prompt expansion"
                    .to_string(),
            ),
        },
    ]
}

/// Extract the provider name from a prefixed model id (e.g. "DeepInfra/..." → "deepinfra").
fn parse_provider(prefixed: &str) -> String {
    if let Some((provider, _)) = prefixed.split_once('/') {
        provider.to_lowercase()
    } else {
        "unknown".to_string()
    }
}

/// Strip the provider prefix from a model id (e.g. "DeepInfra/black-forest-labs/FLUX-2-klein-4b" → "black-forest-labs/FLUX-2-klein-4b").
fn strip_provider_prefix(prefixed: &str) -> &str {
    if let Some((_, rest)) = prefixed.split_once('/') {
        rest
    } else {
        prefixed
    }
}

#[tool_router(router = models_router, vis = "pub")]
impl MediaServer {
    /// List available media generation models with their provider, modality,
    /// capabilities, and default status. Optionally filter by provider.
    #[tool(
        description = "List available media generation models with their provider, modality, capabilities, and default status. Optionally filter by provider (e.g. 'deepinfra', 'openrouter')."
    )]
    pub async fn model_list(
        &self,
        Parameters(ModelListRequest { provider }): Parameters<ModelListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "model_list",
            async {
                let mut models = build_model_list();
                if let Some(filter) = provider
                    && !filter.is_empty()
                {
                    let filter_lower = filter.to_lowercase();
                    models.retain(|m| m.provider == filter_lower);
                }
                serde_json::to_value(&models)
                    .map_err(|e| McpToolError::internal(format!("encode model list: {e}")))
            },
        )
        .await
    }

    /// Get detailed information about a specific media model by its id.
    #[tool(description = "Get detailed information about a specific media model by its id.")]
    pub async fn model_info(
        &self,
        Parameters(ModelInfoRequest { model_id }): Parameters<ModelInfoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(
            self,
            "model_info",
            async {
                let models = build_model_list();
                let model = models
                    .into_iter()
                    .find(|m| m.id == model_id)
                    .ok_or_else(|| {
                        McpToolError::not_found(format!(
                            "Model not found: {model_id}. Call model_list to see available models."
                        ))
                    })?;
                serde_json::to_value(&model)
                    .map_err(|e| McpToolError::internal(format!("encode model info: {e}")))
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_provider_extracts_lowercase_provider() {
        assert_eq!(
            parse_provider("DeepInfra/black-forest-labs/FLUX-2-klein-4b"),
            "deepinfra"
        );
        assert_eq!(parse_provider("OpenRouter/openai/dall-e-3"), "openrouter");
    }

    #[test]
    fn parse_provider_handles_unknown() {
        assert_eq!(parse_provider("no-prefix"), "unknown");
    }

    #[test]
    fn strip_provider_prefix_removes_first_segment() {
        assert_eq!(
            strip_provider_prefix("DeepInfra/black-forest-labs/FLUX-2-klein-4b"),
            "black-forest-labs/FLUX-2-klein-4b"
        );
        assert_eq!(
            strip_provider_prefix("OpenRouter/openai/dall-e-3"),
            "openai/dall-e-3"
        );
    }

    #[test]
    fn strip_provider_prefix_passthrough_no_prefix() {
        assert_eq!(strip_provider_prefix("no-prefix"), "no-prefix");
    }

    #[test]
    fn build_model_list_returns_at_least_five_models() {
        let models = build_model_list();
        assert!(
            models.len() >= 5,
            "expected ≥5 models, got {}",
            models.len()
        );
    }

    #[test]
    fn build_model_list_has_correct_modalities() {
        let models = build_model_list();
        let modalities: Vec<&str> = models.iter().map(|m| m.modality.as_str()).collect();
        assert!(modalities.contains(&"image"), "missing image modality");
        assert!(modalities.contains(&"video"), "missing video modality");
        assert!(modalities.contains(&"audio"), "missing audio modality");
        assert!(modalities.contains(&"vision"), "missing vision modality");
    }

    #[test]
    fn build_model_list_all_have_non_empty_capabilities() {
        let models = build_model_list();
        for model in &models {
            assert!(
                !model.capabilities.is_empty(),
                "model {} has empty capabilities",
                model.id
            );
        }
    }

    #[test]
    fn build_model_list_all_marked_as_default() {
        let models = build_model_list();
        for model in &models {
            assert!(model.is_default, "model {} not marked as default", model.id);
        }
    }

    #[test]
    fn build_model_list_all_have_provider() {
        let models = build_model_list();
        for model in &models {
            assert!(
                !model.provider.is_empty() && model.provider != "unknown",
                "model {} has unknown provider",
                model.id
            );
        }
    }
}
