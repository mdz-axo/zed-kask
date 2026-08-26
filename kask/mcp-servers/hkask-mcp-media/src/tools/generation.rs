//! Generation tools — generate images, transform images, upscale, generate video, execute workflows.
use crate::*;

#[tool_router(router = generation_router, vis = "pub")]
impl MediaServer {
    // ── Generation tools ────────────────────────────────────────────────────

    #[tool(description = "Generate an image from a text prompt. Describe what you want to see.")]
    pub async fn generate_image(
        &self,
        Parameters(GenerateImageRequest {
            prompt,
            image_size,
            num_images,
            style,
        }): Parameters<GenerateImageRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "generate_image",
            Self::ontology_anchor("generate_image"),
            async {
                if prompt.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("prompt must not be empty"));
                }
                let size = image_size.clone();
                let mut media_params = hkask_types::MediaGenerateParams {
                    prompt: Some(prompt.clone()),
                    size: size.clone(),
                    count: num_images,
                    ..Default::default()
                };
                if let Some(style_name) = &style
                    && let Some(preset) = crate::style::get_preset(style_name)
                {
                    crate::style::apply_preset(&mut media_params, &preset);
                }
                self.charge_budget("generate_image", &media_params).await?;
                let result = self
                    .vision_port
                    .media_generate("generate_image", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Image generation failed", e))?;
                // Attach an OMC-tagged, provenance-carrying display hint so the
                // media widget can dispatch the OMC-driven "Explain" affordance and
                // compose-back the "I disagree" gesture.
                let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
                Ok(crate::media_block::enrich_with_omc_and_provenance(
                    result,
                    "generate_image",
                    "image",
                    args,
                    None,
                ))
            },
        )
        .await
    }

    #[tool(
        description = "Transform an existing image with a text prompt. Describe the change you want."
    )]
    pub async fn transform_image(
        &self,
        Parameters(TransformImageRequest {
            prompt,
            image_url,
            strength,
            style,
        }): Parameters<TransformImageRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "transform_image",
            Self::ontology_anchor("transform_image"),
            async {
                validate_tool_url_with_dns(&image_url).await?;
                if let Some(s) = strength
                    && !(0.0..=1.0).contains(&s)
                {
                    return Err(McpToolError::invalid_argument(
                        "strength must be between 0.0 and 1.0",
                    ));
                }
                let mut media_params = hkask_types::MediaGenerateParams {
                    image_url: Some(image_url.clone()),
                    prompt: Some(prompt.clone()),
                    strength,
                    ..Default::default()
                };
                if let Some(style_name) = &style
                    && let Some(preset) = crate::style::get_preset(style_name)
                {
                    crate::style::apply_preset(&mut media_params, &preset);
                }
                self.charge_budget("image_to_image", &media_params).await?;
                let result = self
                    .vision_port
                    .media_generate("image_to_image", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Image transform failed", e))?;
                let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
                Ok(crate::media_block::enrich_with_omc_and_provenance(
                    result,
                    "transform_image",
                    "image",
                    args,
                    None,
                ))
            },
        )
        .await
    }

    #[tool(description = "Upscale an image to higher resolution.")]
    pub async fn upscale_image(
        &self,
        Parameters(UpscaleImageRequest { image_url, scale }): Parameters<UpscaleImageRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "upscale_image",
            Self::ontology_anchor("upscale_image"),
            async {
                validate_tool_url_with_dns(&image_url).await?;
                let media_params = hkask_types::MediaGenerateParams {
                    image_url: Some(image_url.clone()),
                    scale,
                    ..Default::default()
                };
                self.charge_budget("upscale", &media_params).await?;
                let result = self
                    .vision_port
                    .media_generate("upscale", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Upscale failed", e))?;
                let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
                Ok(crate::media_block::enrich_with_omc_and_provenance(
                    result,
                    "upscale_image",
                    "image",
                    args,
                    None,
                ))
            },
        )
        .await
    }

    #[tool(
        description = "Generate a short video from a text prompt. Describe the scene you want to see in motion."
    )]
    pub async fn generate_video(
        &self,
        Parameters(GenerateVideoRequest {
            prompt,
            duration,
            style,
        }): Parameters<GenerateVideoRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "generate_video",
            Self::ontology_anchor("generate_video"),
            async {
                if prompt.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("prompt must not be empty"));
                }
                let mut media_params = hkask_types::MediaGenerateParams {
                    prompt: Some(prompt.clone()),
                    duration,
                    ..Default::default()
                };
                if let Some(style_name) = &style
                    && let Some(preset) = crate::style::get_preset(style_name)
                {
                    crate::style::apply_preset(&mut media_params, &preset);
                }
                self.charge_budget("generate_video", &media_params).await?;
                let result = self
                    .vision_port
                    .media_generate("generate_video", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Video generation failed", e))?;
                let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
                Ok(crate::media_block::enrich_with_omc_and_provenance(
                    result,
                    "generate_video",
                    "video",
                    args,
                    None,
                ))
            },
        )
        .await
    }

    #[tool(
        description = "Expand a short media prompt into a rich, detailed prompt using a vision LLM (Fooocus 'V2' pattern). The user writes 'a cat in space' and the system expands it to include lighting, composition, style, atmosphere, and quality modifiers. Optionally apply a style preset (default, anime, realistic, cinematic, minimal) to the expanded prompt."
    )]
    pub async fn expand_prompt(
        &self,
        Parameters(ExpandPromptRequest { prompt, style }): Parameters<ExpandPromptRequest>,
    ) -> String {
        execute_tool_semantic(self, "expand_prompt", Self::ontology_anchor("expand_prompt"), async {
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }

            // Build the expansion instruction for the vision LLM.
            let expansion_instruction = format!(
                "Expand this short media prompt into a rich, detailed prompt for image/video generation. \
                 Add specific details about lighting, composition, style, atmosphere, and quality. \
                 Keep the original intent. Do not add quotes or explanations. Output only the expanded prompt. \
                 Original prompt: {prompt}"
            );

            // Call the vision LLM via the IPC bridge.
            let llm_params = hkask_types::template::LLMParameters::default();
            let result = self
                .vision_port
                .generate_vision(&expansion_instruction, &[], &llm_params, None)
                .await
                .map_err(|e| {
                    classify_inference_error(
                        "Prompt expansion failed (requires vision LLM via IPC bridge)",
                        e,
                    )
                })?;

            let expanded = result.text.trim().to_string();

            // Apply style preset if set.
            let final_prompt = if let Some(style_name) = &style {
                let preset = crate::style::get_preset(style_name).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "Unknown style: {style_name}. Available: {}",
                        crate::style::available_styles().join(", ")
                    ))
                })?;
                let mut params = hkask_types::MediaGenerateParams {
                    prompt: Some(expanded.clone()),
                    ..Default::default()
                };
                crate::style::apply_preset(&mut params, &preset);
                params.prompt.unwrap_or(expanded)
            } else {
                expanded
            };

            Ok(serde_json::json!({
                "original_prompt": prompt,
                "expanded_prompt": final_prompt,
                "style": style,
            }))
        })
        .await
    }
}
