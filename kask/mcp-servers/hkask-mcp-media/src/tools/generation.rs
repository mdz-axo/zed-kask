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
        }): Parameters<GenerateImageRequest>,
    ) -> String {
        execute_tool(self, "generate_image", async {
            if prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument("prompt must not be empty"));
            }
            let size = image_size.clone();
            self.inference
                .generate_image(&prompt, size.as_deref(), num_images)
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
            self.inference
                .image_to_image(&image_url, &prompt, strength)
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
            self.inference
                .upscale(&image_url, scale)
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
            self.inference
                .generate_video(&prompt, duration)
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
            self.inference
                .execute_workflow(&workflow_json)
                .await
                .map(|wr| {
                    serde_json::json!({
                        "output_urls": wr.output_urls,
                        "output_fields": wr.output_fields,
                        "elapsed_seconds": wr.elapsed_seconds,
                    })
                })
                .map_err(|e| McpToolError::unavailable(format!("Workflow execution failed: {e}")))
        })
        .await
    }
}
