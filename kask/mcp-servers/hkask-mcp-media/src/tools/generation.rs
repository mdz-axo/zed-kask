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
        execute_tool(self, "generate_image", async {
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
            if let Some(style_name) = &style {
                if let Some(preset) = crate::style::get_preset(style_name) {
                    crate::style::apply_preset(&mut media_params, &preset);
                }
            }
            self.vision_port
                .media_generate("generate_image", &media_params)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Image generation failed: {}", e)))
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
        }): Parameters<TransformImageRequest>,
    ) -> String {
        execute_tool(self, "transform_image", async {
            validate_tool_url(&image_url)?;
            if let Some(s) = strength {
                if !(0.0..=1.0).contains(&s) {
                    return Err(McpToolError::invalid_argument(
                        "strength must be between 0.0 and 1.0",
                    ));
                }
            }
            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                prompt: Some(prompt.clone()),
                strength,
                ..Default::default()
            };
            self.vision_port
                .media_generate("image_to_image", &media_params)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Image transform failed: {}", e)))
        })
        .await
    }

    #[tool(description = "Upscale an image to higher resolution.")]
    pub async fn upscale_image(
        &self,
        Parameters(UpscaleImageRequest { image_url, scale }): Parameters<UpscaleImageRequest>,
    ) -> String {
        execute_tool(self, "upscale_image", async {
            validate_tool_url(&image_url)?;
            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                scale,
                ..Default::default()
            };
            self.vision_port
                .media_generate("upscale", &media_params)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Upscale failed: {}", e)))
        })
        .await
    }

    #[tool(
        description = "Generate a short video from a text prompt. Describe the scene you want to see in motion."
    )]
    pub async fn generate_video(
        &self,
        Parameters(GenerateVideoRequest { prompt, duration }): Parameters<GenerateVideoRequest>,
    ) -> String {
        execute_tool(self, "generate_video", async {
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            let media_params = hkask_types::MediaGenerateParams {
                prompt: Some(prompt.clone()),
                duration,
                ..Default::default()
            };
            self.vision_port
                .media_generate("generate_video", &media_params)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Video generation failed: {}", e)))
        })
        .await
    }

    // ── Workflow execution ─────────────────────────────────────────────────

    #[tool(
        description = "Execute a multi-step Fal media workflow. Provide a JSON string with a DAG of nodes (input, run, display types). Run nodes accept 'mode': 'sync' (default, via fal.run) or 'queue' (via queue.fal.run with polling) for long-running models like video generation and upscaling. Nodes execute in dependency order with $reference resolution between them. Returns output URLs and metadata."
    )]
    pub async fn execute_workflow(
        &self,
        Parameters(ExecuteWorkflowRequest { workflow }): Parameters<ExecuteWorkflowRequest>,
    ) -> String {
        execute_tool(self, "execute_workflow", async {
            let workflow_json: serde_json::Value =
                serde_json::from_str(&workflow).map_err(|e| {
                    McpToolError::invalid_argument(format!("Invalid workflow JSON: {e}"))
                })?;
            let media_params = hkask_types::MediaGenerateParams {
                workflow: Some(workflow_json.clone()),
                ..Default::default()
            };
            self.vision_port
                .media_generate("execute_workflow", &media_params)
                .await
                .map(|wr| {
                    // The IPC bridge serializes `WorkflowResult` to JSON for
                    // transport; extract the same fields the old direct call
                    // returned.
                    serde_json::json!({
                        "output_urls": wr.get("output_urls").cloned().unwrap_or(serde_json::Value::Array(vec![])),
                        "output_fields": wr.get("output_fields").cloned().unwrap_or(serde_json::Value::Null),
                        "elapsed_seconds": wr.get("elapsed_seconds").cloned().unwrap_or(serde_json::Value::from(0.0)),
                    })
                })
                .map_err(|e| McpToolError::unavailable(format!("Workflow execution failed: {e}")))
        })
        .await
    }
}
