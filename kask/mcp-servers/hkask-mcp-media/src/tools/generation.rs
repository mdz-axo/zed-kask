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
            let _admission = self.admit_heavy_operation()?;
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            let count = num_images.unwrap_or(1);
            validate_item_count(
                "num_images",
                count as usize,
                1,
                hkask_types::media_limits::MAX_GENERATION_VARIANTS as usize,
            )?;
            // Admission-time gallery capture: the gallery active when the
            // operation is admitted is the gallery every variant is indexed
            // into, even if the active root switches mid-inference.
            let gallery = self.capture_gallery();
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
            let args = serde_json::to_value(&media_params).map_err(|error| {
                McpToolError::internal(format!("encode image generation parameters: {error}"))
            })?;

            if count == 1 {
                let result = self
                    .vision_port
                    .media_generate("generate_image", &media_params)
                    .await
                    .map_err(|error| classify_inference_error("Image generation failed", error))?;
                // Persist the payload and compose the slim result (path +
                // metadata + display hint — the base64 payload never enters
                // the model's context).
                return persist_slim_and_enrich(
                    gallery.as_ref(),
                    &self.gallery_store,
                    &result,
                    "generate_image",
                    "image",
                    args,
                )
                .await;
            }

            // A requested variant is complete only when it is either durably
            // persisted or represented by a causal failure. Valid siblings
            // are never rolled back when another variant fails.
            let requested = count as usize;
            let mut variants = Vec::with_capacity(requested);
            let mut failures = Vec::new();
            let mut attempts = 0usize;
            while variants.len() + failures.len() < requested && attempts < requested {
                attempts += 1;
                let result = match self
                    .vision_port
                    .media_generate("generate_image", &media_params)
                    .await
                {
                    Ok(result) => result,
                    Err(error) => {
                        let classified = classify_inference_error("Image generation failed", error);
                        failures.push(serde_json::json!({
                            "variant_index": variants.len() + failures.len(),
                            "stage": "provider",
                            "cause": classified.message,
                        }));
                        continue;
                    }
                };
                let single_results = match result.get("data").and_then(|data| data.as_array()) {
                    Some(data) if data.is_empty() => {
                        failures.push(serde_json::json!({
                            "variant_index": variants.len() + failures.len(),
                            "stage": "provider",
                            "cause": "provider returned an empty data array",
                        }));
                        continue;
                    }
                    Some(data) => data
                        .iter()
                        .map(|item| serde_json::json!({ "data": [item] }))
                        .collect::<Vec<_>>(),
                    None => vec![result],
                };
                for single_result in single_results {
                    if variants.len() + failures.len() >= requested {
                        break;
                    }
                    let variant_index = variants.len() + failures.len();
                    match persist_slim_and_enrich(
                        gallery.as_ref(),
                        &self.gallery_store,
                        &single_result,
                        "generate_image",
                        "image",
                        args.clone(),
                    )
                    .await
                    {
                        Ok(variant) => variants.push(variant),
                        Err(error) => failures.push(serde_json::json!({
                            "variant_index": variant_index,
                            "stage": "persistence",
                            "cause": error.message,
                        })),
                    }
                }
            }

            let status = if failures.is_empty() {
                "completed"
            } else if variants.is_empty() {
                "failed"
            } else {
                "partial"
            };
            // Top-level display_hints (one fenced media block per valid
            // variant) follows the gallery-search display contract.
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
                "status": status,
                "prompt": prompt,
                "count_requested": count,
                "count_returned": variants.len(),
                "variants": variants,
                "failures": failures,
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
            let _admission = self.admit_heavy_operation()?;
            // Admission-time gallery capture — before every await (DNS
            // validation included): the gallery active when the operation is
            // admitted is the gallery the output is indexed into.
            let gallery = self.capture_gallery();
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
                gallery.as_ref(),
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
            let _admission = self.admit_heavy_operation()?;
            let gallery = self.capture_gallery();
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
                gallery.as_ref(),
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
            let _admission = self.admit_heavy_operation()?;
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            let gallery = self.capture_gallery();
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
                gallery.as_ref(),
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
            let _admission = self.admit_heavy_operation()?;
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
            let _admission = self.admit_heavy_operation()?;
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
            let gallery = self.capture_gallery();
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
                gallery.as_ref(),
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
