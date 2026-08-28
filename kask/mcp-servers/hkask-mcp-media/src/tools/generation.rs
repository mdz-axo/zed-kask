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
                let result = self
                    .vision_port
                    .media_generate("generate_image", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Image generation failed", e))?;
                // Persist the generated image to {data_dir}/mcp/media/generated/
                // and add it to the gallery index.
                match persist_generated_asset(self, &result, "image").await {
                    Ok(path) => {
                        tracing::info!(
                            target: "hkask.mcp.media",
                            path = %path.display(),
                            "Generated image persisted to data directory"
                        );
                    }
                    Err(error) => tracing::warn!(
                        target: "hkask.mcp.media",
                        %error,
                        "Failed to persist generated asset (tool result still carries the provider URL)"
                    ),
                }
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
                let result = self
                    .vision_port
                    .media_generate("image_to_image", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Image transform failed", e))?;
                match persist_generated_asset(self, &result, "image").await {
                    Ok(path) => {
                        tracing::info!(
                            target: "hkask.mcp.media",
                            path = %path.display(),
                            "Transformed image persisted"
                        );
                    }
                    Err(error) => tracing::warn!(
                        target: "hkask.mcp.media",
                        %error,
                        "Failed to persist generated asset (tool result still carries the provider URL)"
                    ),
                }
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
                let result = self
                    .vision_port
                    .media_generate("upscale", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Upscale failed", e))?;
                match persist_generated_asset(self, &result, "image").await {
                    Ok(path) => {
                        tracing::info!(
                            target: "hkask.mcp.media",
                            path = %path.display(),
                            "Upscaled image persisted"
                        );
                    }
                    Err(error) => tracing::warn!(
                        target: "hkask.mcp.media",
                        %error,
                        "Failed to persist generated asset (tool result still carries the provider URL)"
                    ),
                }
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
                let result = self
                    .vision_port
                    .media_generate("generate_video", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Video generation failed", e))?;
                match persist_generated_asset(self, &result, "video").await {
                    Ok(path) => {
                        tracing::info!(
                            target: "hkask.mcp.media",
                            path = %path.display(),
                            "Generated video persisted"
                        );
                    }
                    Err(error) => tracing::warn!(
                        target: "hkask.mcp.media",
                        %error,
                        "Failed to persist generated asset (tool result still carries the provider URL)"
                    ),
                }
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

    /// Generate N image variants from a single prompt. Each variant is
    /// persisted individually to the gallery with its own entry. Returns an
    /// array of media blocks — one per variant — for grid display.
    #[tool(
        description = "Generate multiple image variants from a single prompt. Each variant is persisted individually to the gallery. Returns an array of media blocks for grid display. Default count: 4."
    )]
    pub async fn generate_variants(
        &self,
        Parameters(GenerateVariantsRequest {
            prompt,
            count,
            image_size,
            style,
        }): Parameters<GenerateVariantsRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "generate_variants",
            Self::ontology_anchor("generate_variants"),
            async {
                if prompt.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("prompt must not be empty"));
                }
                let count = count.clamp(1, 10);
                let mut media_params = hkask_types::MediaGenerateParams {
                    prompt: Some(prompt.clone()),
                    size: image_size.clone(),
                    count: Some(count),
                    ..Default::default()
                };
                if let Some(style_name) = &style
                    && let Some(preset) = crate::style::get_preset(style_name)
                {
                    crate::style::apply_preset(&mut media_params, &preset);
                }
                let result = self
                    .vision_port
                    .media_generate("generate_image", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Variant generation failed", e))?;

                // The provider may return multiple images in data[]. Extract
                // each one, persist it, and build a media block for it. When the
                // provider returns a single image per call (no data[] array),
                // issue additional calls until `count` variants are collected
                // (capped at `count` total provider calls).
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
                            .map_err(|e| {
                                classify_inference_error("Variant generation failed", e)
                            })?,
                    };
                    let single_results: Vec<serde_json::Value> = match result
                        .get("data")
                        .and_then(|d| d.as_array())
                    {
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
                        match persist_generated_asset(self, &single_result, "image").await {
                            Ok(path) => {
                                tracing::info!(
                                    target: "hkask.mcp.media",
                                    path = %path.display(),
                                    "Variant persisted to data directory"
                                );
                            }
                            Err(error) => tracing::warn!(
                                target: "hkask.mcp.media",
                                %error,
                                "Failed to persist variant (tool result still carries the provider URL)"
                            ),
                        }
                        variants.push(crate::media_block::enrich_with_omc_and_provenance(
                            single_result,
                            "generate_variants",
                            "image",
                            serde_json::to_value(&media_params)
                                .unwrap_or(serde_json::Value::Null),
                            None,
                        ));
                    }
                }
                // Top-level display_hints (one fenced media block per variant)
                // follows the system-prompt contract used by gallery_search /
                // gallery_find_similar, so the model can copy each block into its
                // reply for grid display. The per-variant detail (with its own
                // display_hint) stays in `variants`.
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
            },
        )
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
    ) -> String {
        execute_tool_semantic(
            self,
            "image_edit_region",
            Self::ontology_anchor("image_edit_region"),
            async {
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
                        "strength must be between 0.0 and 1.0"
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
                match persist_generated_asset(self, &result, "image").await {
                    Ok(path) => {
                        tracing::info!(
                            target: "hkask.mcp.media",
                            path = %path.display(),
                            "Region-edited image persisted"
                        );
                    }
                    Err(error) => tracing::warn!(
                        target: "hkask.mcp.media",
                        %error,
                        "Failed to persist region-edited image"
                    ),
                }
                let args = serde_json::to_value(&media_params)
                    .unwrap_or(serde_json::Value::Null);
                Ok(crate::media_block::enrich_with_omc_and_provenance(
                    result,
                    "image_edit_region",
                    "image",
                    args,
                    None,
                ))
            },
        )
        .await
    }
}
