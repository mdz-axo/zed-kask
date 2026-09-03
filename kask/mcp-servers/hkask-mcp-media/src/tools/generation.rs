//! Generation tools — generate images, transform images, upscale, generate video, execute workflows.
use crate::*;

#[tool_router(router = generation_router, vis = "pub")]
impl MediaServer {
    // ── Generation tools ────────────────────────────────────────────────────

    #[tool(
        description = "Generate an image (or several variants) from a text prompt. Describe what you want to see. num_images > 1 generates that many variants — each is persisted individually with its own gallery entry, and the result carries one display hint per variant for grid display. Providers that return one image per call are called repeatedly until the count is collected."
    )]
    pub async fn generate_image(
        &self,
        Parameters(GenerateImageRequest {
            prompt,
            image_size,
            num_images,
            style,
        }): Parameters<GenerateImageRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "generate_image", async {
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            let count = num_images.unwrap_or(1).clamp(1, 10);
            let size = image_size.clone();
            let mut media_params = hkask_types::MediaGenerateParams {
                prompt: Some(prompt.clone()),
                size: size.clone(),
                count: Some(count),
                ..Default::default()
            };
            if let Some(style_name) = &style {
                let preset = crate::style::get_preset(style_name).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "unknown style '{}'; available styles: {}",
                        style_name,
                        crate::style::available_styles().join(", ")
                    ))
                })?;
                crate::style::apply_preset(&mut media_params, &preset);
            }
            let result = self
                .vision_port
                .media_generate("generate_image", &media_params)
                .await
                .map_err(|e| classify_inference_error("Image generation failed", e))?;

            if count == 1 {
                // Persist the payload and compose the slim result (path +
                // metadata + display hint — the base64 payload never enters
                // the model's context).
                let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
                return persist_slim_and_enrich(
                    &self.gallery_state,
                    &self.gallery_store,
                    &result,
                    "generate_image",
                    "image",
                    args,
                )
                .await;
            }

            // Multi-variant path (the former generate_variants tool, folded
            // in): the provider may return multiple images in data[].
            // Extract each one, persist it, and build a media block for it.
            // When the provider returns a single image per call (no data[]
            // array), issue additional calls until `count` variants are
            // collected (capped at `count` total provider calls).
            let mut variants: Vec<serde_json::Value> = Vec::with_capacity(count as usize);
            let mut pending_result = Some(result);
            let mut attempts = 0;
            while variants.len() < count as usize && attempts < count as usize {
                attempts += 1;
                let result = match pending_result.take() {
                    Some(first) => first,
                    None => self
                        .vision_port
                        .media_generate("generate_image", &media_params)
                        .await
                        .map_err(|e| classify_inference_error("Image generation failed", e))?,
                };
                let single_results: Vec<serde_json::Value> =
                    match result.get("data").and_then(|d| d.as_array()) {
                        Some(data) => data
                            .iter()
                            .map(|item| serde_json::json!({ "data": [item] }))
                            .collect(),
                        // Provider returned a single-image response — use it as-is.
                        None => vec![result],
                    };
                for single_result in single_results {
                    if variants.len() >= count as usize {
                        break;
                    }
                    // Persist each variant and compose its slim result (the
                    // base64 payload never enters the model's context).
                    variants.push(
                        persist_slim_and_enrich(
                            &self.gallery_state,
                            &self.gallery_store,
                            &single_result,
                            "generate_image",
                            "image",
                            serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null),
                        )
                        .await?,
                    );
                }
            }
            // Top-level display_hints (one fenced media block per variant)
            // follows the system-prompt contract used by gallery_search, so
            // the model can copy each block into its reply for grid display.
            // The per-variant detail (with its own display_hint) stays in
            // `variants`.
            let display_hints: Vec<String> = variants
                .iter()
                .filter_map(|variant| {
                    variant
                        .get("display_hint")
                        .and_then(|hint| hint.as_str())
                        .map(str::to_string)
                })
                .collect();
            Ok(serde_json::json!({
                "prompt": prompt,
                "count_requested": count,
                "count_returned": variants.len(),
                "variants": variants,
                "display_hints": display_hints,
            }))
        })
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "transform_image", async {
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
            if let Some(style_name) = &style {
                let preset = crate::style::get_preset(style_name).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "unknown style '{}'; available styles: {}",
                        style_name,
                        crate::style::available_styles().join(", ")
                    ))
                })?;
                crate::style::apply_preset(&mut media_params, &preset);
            }
            let result = self
                .vision_port
                .media_generate("image_to_image", &media_params)
                .await
                .map_err(|e| classify_inference_error("Image transform failed", e))?;
            // Persist the payload and compose the slim result (the provider's
            // base64 payload never enters the model's context).
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            persist_slim_and_enrich(
                &self.gallery_state,
                &self.gallery_store,
                &result,
                "transform_image",
                "image",
                args,
            )
            .await
        })
        .await
    }

    #[tool(description = "Upscale an image to higher resolution.")]
    pub async fn upscale_image(
        &self,
        Parameters(UpscaleImageRequest { image_url, scale }): Parameters<UpscaleImageRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "upscale_image", async {
            validate_tool_url_with_dns(&image_url).await?;
            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                scale,
                ..Default::default()
            };
            let result = self
                .vision_port
                .media_generate("upscale", &media_params)
                .await
                .map_err(|e| classify_inference_error("Upscale failed", e))?;
            // Persist the payload and compose the slim result (the provider's
            // base64 payload never enters the model's context).
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            persist_slim_and_enrich(
                &self.gallery_state,
                &self.gallery_store,
                &result,
                "upscale_image",
                "image",
                args,
            )
            .await
        })
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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "generate_video", async {
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            let mut media_params = hkask_types::MediaGenerateParams {
                prompt: Some(prompt.clone()),
                duration,
                ..Default::default()
            };
            if let Some(style_name) = &style {
                let preset = crate::style::get_preset(style_name).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "unknown style '{}'; available styles: {}",
                        style_name,
                        crate::style::available_styles().join(", ")
                    ))
                })?;
                crate::style::apply_preset(&mut media_params, &preset);
            }
            let result = self
                .vision_port
                .media_generate("generate_video", &media_params)
                .await
                .map_err(|e| classify_inference_error("Video generation failed", e))?;
            // Persist the payload and compose the slim result (the video
            // payload never enters the model's context).
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            persist_slim_and_enrich(
                &self.gallery_state,
                &self.gallery_store,
                &result,
                "generate_video",
                "video",
                args,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Expand a short media prompt into a rich, detailed prompt using a vision LLM (Fooocus 'V2' pattern). The user writes 'a cat in space' and the system expands it to include lighting, composition, style, atmosphere, and quality modifiers. Optionally apply a style preset (default, anime, realistic, cinematic, minimal) to the expanded prompt."
    )]
    pub async fn expand_prompt(
        &self,
        Parameters(ExpandPromptRequest { prompt, style }): Parameters<ExpandPromptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "expand_prompt", async {
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

    /// Apply a transform to a region of an image (inpainting). The mask
    /// defines which regions are edited (white) and which are preserved
    /// (black). The prompt describes the desired edit.
    #[tool(
        description = "Apply a region-selective edit to an image (inpainting). Provide a mask (base64 data URI, white = edit, black = preserve) and a prompt describing the edit."
    )]
    pub async fn image_edit_region(
        &self,
        Parameters(ImageEditRegionRequest {
            image_url,
            mask,
            prompt,
            strength,
        }): Parameters<ImageEditRegionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "image_edit_region", async {
            validate_tool_url_with_dns(&image_url).await?;
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            if mask.trim().is_empty() {
                return Err(McpToolError::invalid_argument("mask must not be empty"));
            }
            let strength = strength.unwrap_or(0.85);
            if !(0.0..=1.0).contains(&strength) {
                return Err(McpToolError::invalid_argument(
                    "strength must be between 0.0 and 1.0",
                ));
            }
            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                prompt: Some(prompt.clone()),
                strength: Some(strength),
                mask: Some(mask.clone()),
                ..Default::default()
            };
            let result = self
                .vision_port
                .media_generate("image_to_image", &media_params)
                .await
                .map_err(|e| classify_inference_error("Region edit failed", e))?;
            // Persist the payload and compose the slim result (the provider's
            // base64 payload never enters the model's context).
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            persist_slim_and_enrich(
                &self.gallery_state,
                &self.gallery_store,
                &result,
                "image_edit_region",
                "image",
                args,
            )
            .await
        })
        .await
    }
}
