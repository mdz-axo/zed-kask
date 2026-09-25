//! Processing tools — background removal, style transfer, collage, video editing, memes.
use crate::*;

const REMIX_CAPTION_POSITION: &str = "bottom";
const REMIX_CAPTION_FONT_SIZE: u32 = 24;
const REMIX_GIF_START_SEC: f32 = 0.0;
const REMIX_GIF_WIDTH: u32 = 480;
const REMIX_GIF_FPS: u32 = 10;

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct LocalVideoValidationGate {
    pub(crate) entered: std::sync::Arc<tokio::sync::Notify>,
    pub(crate) resume: std::sync::Arc<tokio::sync::Notify>,
    pub(crate) bypass_dns: bool,
}

#[cfg(test)]
static LOCAL_VIDEO_VALIDATION_GATE: std::sync::LazyLock<
    std::sync::Mutex<Option<LocalVideoValidationGate>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

#[cfg(test)]
pub(crate) struct LocalVideoValidationGateGuard;

#[cfg(test)]
impl Drop for LocalVideoValidationGateGuard {
    fn drop(&mut self) {
        match LOCAL_VIDEO_VALIDATION_GATE.lock() {
            Ok(mut gate) => *gate = None,
            Err(error) => tracing::warn!(
                target: "hkask.mcp.media",
                %error,
                "Failed to clear local-video validation test gate"
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn install_local_video_validation_gate(
    gate: LocalVideoValidationGate,
) -> Result<LocalVideoValidationGateGuard, MediaError> {
    let mut installed = LOCAL_VIDEO_VALIDATION_GATE
        .lock()
        .map_err(|error| MediaError::Io(format!("local-video validation gate lock: {error}")))?;
    *installed = Some(gate);
    Ok(LocalVideoValidationGateGuard)
}

#[cfg(test)]
async fn pause_after_local_video_admission() -> Result<bool, McpToolError> {
    let gate = LOCAL_VIDEO_VALIDATION_GATE
        .lock()
        .map_err(|error| {
            McpToolError::internal(format!("local-video validation gate lock: {error}"))
        })?
        .clone();
    let Some(gate) = gate else {
        return Ok(false);
    };
    gate.entered.notify_one();
    gate.resume.notified().await;
    Ok(gate.bypass_dns)
}

#[derive(serde::Serialize)]
struct ClipEffectiveParams<'a> {
    source: &'a str,
    start_sec: f32,
    end_sec: f32,
    duration_sec: f32,
}

#[derive(serde::Serialize)]
struct GifEffectiveParams<'a> {
    source: &'a str,
    start_sec: f32,
    duration_sec: f32,
    width: u32,
    fps: u32,
}

#[derive(serde::Serialize)]
struct CaptionEffectiveParams<'a> {
    source: &'a str,
    text: &'a str,
    position: &'a str,
    font_size: u32,
}

#[derive(serde::Serialize)]
struct RemixEffectiveParams<'a> {
    source: &'a str,
    start_sec: f32,
    end_sec: f32,
    caption_text: Option<&'a str>,
    caption_position: &'static str,
    caption_font_size: u32,
    gif_start_sec: f32,
    gif_duration_sec: f32,
    gif_width: u32,
    gif_fps: u32,
}

#[derive(serde::Serialize)]
struct ImagesEffectiveParams<'a> {
    sources: &'a [String],
    image_indices: &'a [usize],
    fps: u32,
}

#[derive(serde::Serialize)]
struct ConcatEffectiveParams<'a> {
    sources: &'a [String],
}

#[derive(serde::Serialize)]
#[serde(untagged)]
enum LocalVideoEffectiveParams<'a> {
    Clip(ClipEffectiveParams<'a>),
    Gif(GifEffectiveParams<'a>),
    Caption(CaptionEffectiveParams<'a>),
    Remix(RemixEffectiveParams<'a>),
    ImagesMp4(ImagesEffectiveParams<'a>),
    ImagesGif(ImagesEffectiveParams<'a>),
    Concat(ConcatEffectiveParams<'a>),
}

impl LocalVideoEffectiveParams<'_> {
    const fn op(&self) -> &'static str {
        match self {
            Self::Clip(_) => "video_clip",
            Self::Gif(_) => "video_to_gif",
            Self::Caption(_) => "video_add_caption",
            Self::Remix(_) => "video_remix",
            Self::ImagesMp4(_) | Self::ImagesGif(_) => "video_from_images",
            Self::Concat(_) => "video_concat",
        }
    }

    const fn status(&self) -> &'static str {
        match self {
            Self::Clip(_) => "clipped",
            Self::Gif(_) => "converted",
            Self::Caption(_) => "captioned",
            Self::Remix(_) => "remixed",
            Self::ImagesMp4(_) | Self::ImagesGif(_) => "created",
            Self::Concat(_) => "concatenated",
        }
    }

    const fn format(&self) -> crate::assets::LocalMediaFormat {
        match self {
            Self::Gif(_) | Self::Remix(_) | Self::ImagesGif(_) => {
                crate::assets::LocalMediaFormat::Gif
            }
            Self::Clip(_) | Self::Caption(_) | Self::ImagesMp4(_) | Self::Concat(_) => {
                crate::assets::LocalMediaFormat::Mp4
            }
        }
    }
}

pub(crate) fn resolve_image_path_in_gallery(
    server: &MediaServer,
    gallery: &GalleryAccess,
    image_index: usize,
) -> Result<std::path::PathBuf, MediaError> {
    let gallery_id = &gallery.gallery_id;
    let image = server
        .gallery_store
        .get_image(gallery_id, Some(image_index), None)
        .map_err(|error| {
            MediaError::ImageNotFound(format!(
                "Image not found at index {image_index} in the active gallery {gallery_id}: {error}"
            ))
        })?;
    Ok(std::path::PathBuf::from(image.absolute_path))
}

struct LocalVideoIntermediates {
    paths: Vec<std::path::PathBuf>,
}

impl LocalVideoIntermediates {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn track(&mut self, path: std::path::PathBuf) {
        self.paths.push(path);
    }

    fn cleanup(&mut self) -> Result<(), MediaError> {
        let mut first_error = None;
        let mut remaining = Vec::new();
        for path in std::mem::take(&mut self.paths) {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(MediaError::Io(format!(
                            "delete video remix intermediate {}: {error}",
                            path.display()
                        )));
                    }
                    remaining.push(path);
                }
            }
        }
        self.paths = remaining;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for LocalVideoIntermediates {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(
                target: "hkask.mcp.media",
                %error,
                "Failed to clean up video remix intermediates"
            );
        }
    }
}

#[tool_router(router = processing_router, vis = "pub")]
impl MediaServer {
    // ── Derivation tools ─────────────────────────────────────────────────────

    #[tool(
        description = "Remove background from a gallery image. Delegates to the configured background-removal provider."
    )]
    pub async fn image_remove_background(
        &self,
        Parameters(RemoveBackgroundRequest {
            image_index,
            new_bg_color: _new_bg_color,
        }): Parameters<RemoveBackgroundRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "image_remove_background", async {
            let _admission = self.admit_heavy_operation()?;
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                ..Default::default()
            };
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            let result = self
                .vision_port
                .media_generate("remove_background", &media_params)
                .await
                .map_err(|e| classify_inference_error("Background removal failed", e))?;
            // Persist the payload and compose the slim result (the provider's
            // base64 payload never enters the model's context).
            persist_slim_and_enrich(
                &self.gallery_store,
                &result,
                "image_remove_background",
                "image",
                args,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Apply style transfer to a gallery image via the configured image-to-image provider (DeepInfra or OpenRouter)."
    )]
    pub async fn image_apply_style(
        &self,
        Parameters(ApplyStyleRequest {
            image_index,
            style_prompt,
            strength,
        }): Parameters<ApplyStyleRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "image_apply_style", async {
            let _admission = self.admit_heavy_operation()?;
            if style_prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "style_prompt must not be empty",
                ));
            }
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                prompt: Some(style_prompt.clone()),
                strength,
                ..Default::default()
            };
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            let result = self
                .vision_port
                .media_generate("image_to_image", &media_params)
                .await
                .map_err(|e| classify_inference_error("Style transfer failed", e))?;
            // Persist the payload and compose the slim result (the provider's
            // base64 payload never enters the model's context). Previously this
            // tool returned the raw provider response unpersisted.
            persist_slim_and_enrich(
                &self.gallery_store,
                &result,
                "image_apply_style",
                "image",
                args,
            )
            .await
        })
        .await
    }

    #[tool(
        description = "Create a collage from multiple gallery images. Local composition using image crate. Three modes: search_terms (semantic tag search), similar_to_index (visually similar images), or image_indices (explicit list)."
    )]
    pub async fn image_create_collage(
        &self,
        Parameters(CreateCollageRequest {
            search_terms,
            similar_to_index,
            image_indices,
            max_items,
            layout,
            spacing,
            canvas_size,
        }): Parameters<CreateCollageRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "image_create_collage", async {
            let _admission = self.admit_heavy_operation()?;
            let mode_count =
                search_terms.is_some() as u8 + similar_to_index.is_some() as u8 + image_indices.is_some() as u8;
            if mode_count == 0 {
                return Err(McpToolError::invalid_argument(
                    "Must specify one of: search_terms, similar_to_index, or image_indices.",
                ));
            }
            if mode_count > 1 {
                return Err(McpToolError::invalid_argument(
                    "search_terms, similar_to_index, and image_indices are mutually exclusive. Choose one.",
                ));
            }

            let ga = self.access_gallery().map_err(map_media_error)?;

            let mut paths = Vec::new();

            if let Some(ref terms) = search_terms {
                let all_tags = self
                    .gallery_store
                    .get_all_tags(&ga.gallery_id)
                    .map_err(map_gallery_store_error)?;

                let ranked = crate::tools::gallery::rank_images_by_tag_similarity(
                    &all_tags,
                    terms,
                    0.3,
                    None,
                )
                .into_iter()
                .take(max_items)
                .collect::<Vec<_>>();

                for (rel_path, _score) in &ranked {
                    paths.push(ga.root_path.join(rel_path));
                }
            } else if let Some(ref_idx) = similar_to_index {
                let ref_path = self
                    .resolve_image_path(ref_idx)
                    .map_err(map_media_error)?;
                let ref_image_id = self.resolve_image_id(ref_idx).map_err(map_media_error)?;
                let ref_tags = self
                    .gallery_store
                    .get_tags(&ref_image_id)
                    .map_err(map_gallery_store_error)?;

                let all_tags = self
                    .gallery_store
                    .get_all_tags(&ga.gallery_id)
                    .map_err(map_gallery_store_error)?;

                let ref_rel_path = ref_path
                    .strip_prefix(&ga.root_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ref_path.to_string_lossy().to_string());
                let terms: Vec<String> = ref_tags.iter().map(|t| t.value.clone()).collect();
                let ranked = crate::tools::gallery::rank_images_by_tag_similarity(
                    &all_tags,
                    &terms,
                    0.3,
                    Some(&ref_rel_path),
                )
                .into_iter()
                .take(max_items.saturating_sub(1))
                .collect::<Vec<_>>();

                paths.push(ref_path);
                for (rel_path, _score) in &ranked {
                    paths.push(ga.root_path.join(rel_path));
                }
            } else if let Some(ref indices) = image_indices {
                if indices.is_empty() {
                    return Err(McpToolError::invalid_argument("At least one image index is required."));
                }
                if indices.len() > 9 {
                    return Err(McpToolError::invalid_argument(
                        "Maximum 9 images supported for collage.",
                    ));
                }
                let limit = indices.len().min(max_items);
                for idx in indices.iter().take(limit) {
                    paths.push(self.resolve_image_path(*idx).map_err(map_media_error)?);
                }
            }

            if paths.is_empty() {
                return Err(McpToolError::invalid_argument("No images found for collage."));
            }

            let mut images = Vec::new();
            for path in &paths {
                images.push(image::open(path).map_err(|e| map_image_open_error(path, e))?);
            }

            if images.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "At least one image is required for collage composition",
                ));
            }

            let cols = match layout.as_str() {
                "horizontal" => images.len() as u32,
                "vertical" => 1u32,
                "masonry" => 3u32.min(images.len() as u32),
                _ => (images.len() as f64).sqrt().ceil() as u32,
            };
            let rows = (images.len() as u32).div_ceil(cols);

            let parts: Vec<&str> = canvas_size.split('x').collect();
            let canvas_w: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1200);
            let canvas_h: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(900);

            let cell_w = (canvas_w - spacing * (cols + 1)) / cols;
            let cell_h = (canvas_h - spacing * (rows + 1)) / rows;

            let mut canvas = image::DynamicImage::new_rgba8(canvas_w, canvas_h);
            let bg = image::Rgba([30u8, 30u8, 30u8, 255u8]);
            for pixel in canvas.as_mut_rgba8().expect("canvas was created as RGBA8").pixels_mut() {
                *pixel = bg;
            }

            for (i, img) in images.iter().enumerate() {
                let col = i as u32 % cols;
                let row = i as u32 / cols;

                let scaled = img.resize_exact(
                    cell_w.saturating_sub(spacing),
                    cell_h.saturating_sub(spacing),
                    image::imageops::FilterType::Lanczos3,
                );

                let x = spacing + col * (cell_w + spacing) + (cell_w.saturating_sub(spacing) - scaled.width()) / 2;
                let y = spacing + row * (cell_h + spacing) + (cell_h.saturating_sub(spacing) - scaled.height()) / 2;

                image::imageops::overlay(&mut canvas, &scaled, x as i64, y as i64);
            }

            let temp_dir = std::env::temp_dir().join("hkask-media");
            if let Err(error) = std::fs::create_dir_all(&temp_dir) {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    path = %temp_dir.display(),
                    %error,
                    "Failed to create collage temp directory — the subsequent write will surface the failure"
                );
            }
            let output_path = temp_dir.join(format!("collage_{}.png", uuid::Uuid::new_v4()));

            canvas
                .save(&output_path)
                .map_err(|e| map_image_open_error(&output_path, e))?;

            let result = serde_json::json!({
                "status": "created",
                "image_count": images.len(),
                "layout": layout,
                "cols": cols,
                "rows": rows,
                "canvas_width": canvas_w,
                "canvas_height": canvas_h,
                "spacing": spacing,
                "output": output_path.display().to_string(),
            });
            let args = serde_json::json!({
                "layout": layout,
                "image_count": images.len(),
            });
            Ok(crate::media_block::enrich_with_omc_and_provenance(
                result,
                "image_create_collage",
                "image",
                args,
                None,
            ))
        })
        .await
    }

    // ── Video tools ──────────────────────────────────────────────────────────

    /// expect: My clipped video remains available in the admitted gallery under one stable identity.
    /// [P1] Motivating: user work survives processor and server teardown.
    /// pre: an active gallery exists and `0 <= start_sec < end_sec`.
    /// post: the durable output, gallery row, lineage, result id, and media-block id agree.
    /// [P1] Constraining: any publication failure removes the output and coupled rows.
    #[tool(description = "Trim a video to specified start/end times using local ffmpeg.")]
    pub async fn video_clip(
        &self,
        Parameters(VideoClipRequest {
            video_url,
            start_sec,
            end_sec,
        }): Parameters<VideoClipRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_clip", async {
            let _admission = self.admit_heavy_operation()?;
            if start_sec < 0.0 || end_sec <= 0.0 {
                return Err(McpToolError::invalid_argument(
                    "timestamps must be non-negative",
                ));
            }

            if start_sec >= end_sec {
                return Err(McpToolError::invalid_argument(
                    "start_sec must be less than end_sec.",
                ));
            }

            #[cfg(test)]
            let bypass_dns = pause_after_local_video_admission().await?;
            #[cfg(not(test))]
            let bypass_dns = false;
            if !crate::is_local_media_path(&video_url) && !bypass_dns {
                validate_tool_url_with_dns(&video_url).await?;
            }
            self.require_ffmpeg()?;

            let params = ClipEffectiveParams {
                source: &video_url,
                start_sec,
                end_sec,
                duration_sec: end_sec - start_sec,
            };
            let output = self
                .ffmpeg
                .clip(params.source, params.start_sec, params.end_sec)
                .await
                .map_err(map_media_error)?;
            let effective_params = LocalVideoEffectiveParams::Clip(params);
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                effective_params.op(),
                effective_params.status(),
                effective_params.format(),
                &effective_params,
            )
        })
        .await
    }

    #[tool(description = "Convert a video segment to GIF format using local ffmpeg.")]
    pub async fn video_to_gif(
        &self,
        Parameters(VideoToGifRequest {
            video_url,
            start_sec,
            duration_sec,
            width,
            fps,
        }): Parameters<VideoToGifRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_to_gif", async {
            let _admission = self.admit_heavy_operation()?;
            let start = start_sec.unwrap_or(0.0);
            let dur = duration_sec.unwrap_or(5.0);
            let w = width.unwrap_or(480);
            let f = fps.unwrap_or(10);

            if start < 0.0 || dur <= 0.0 {
                return Err(McpToolError::invalid_argument(
                    "timestamps must be non-negative",
                ));
            }
            if w == 0 {
                return Err(McpToolError::invalid_argument(
                    "width must be greater than 0",
                ));
            }
            if f == 0 {
                return Err(McpToolError::invalid_argument("fps must be greater than 0"));
            }

            #[cfg(test)]
            let bypass_dns = pause_after_local_video_admission().await?;
            #[cfg(not(test))]
            let bypass_dns = false;
            if !crate::is_local_media_path(&video_url) && !bypass_dns {
                validate_tool_url_with_dns(&video_url).await?;
            }
            self.require_ffmpeg()?;
            let params = GifEffectiveParams {
                source: &video_url,
                start_sec: start,
                duration_sec: dur,
                width: w,
                fps: f,
            };
            let output = self
                .ffmpeg
                .to_gif(
                    params.source,
                    params.start_sec,
                    params.duration_sec,
                    params.width,
                    params.fps,
                )
                .await
                .map_err(map_media_error)?;
            let effective_params = LocalVideoEffectiveParams::Gif(params);
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                effective_params.op(),
                effective_params.status(),
                effective_params.format(),
                &effective_params,
            )
        })
        .await
    }

    #[tool(
        description = "Animate a gallery image into a short video clip via the configured image-to-video provider (DeepInfra or OpenRouter)."
    )]
    pub async fn image_to_video(
        &self,
        Parameters(ImageToVideoRequest {
            image_index,
            prompt,
            duration,
            model,
        }): Parameters<ImageToVideoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "image_to_video", async {
            let _admission = self.admit_heavy_operation()?;
            if let Some(d) = duration
                && d <= 0.0
            {
                return Err(McpToolError::invalid_argument("duration must be positive"));
            }
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(image_url.clone()),
                prompt: prompt.clone(),
                duration,
                model,
                ..Default::default()
            };
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            let result = self
                .vision_port
                .media_generate("image_to_video", &media_params)
                .await
                .map_err(|e| classify_inference_error("Image-to-video failed", e))?;
            // Persist the video payload and compose the slim result (the
            // payload never enters the model's context). Previously this
            // tool returned the raw provider response unpersisted.
            persist_slim_and_enrich(
                &self.gallery_store,
                &result,
                "image_to_video",
                "video",
                args,
            )
            .await
        })
        .await
    }

    #[tool(description = "Add text caption overlay to a video using local ffmpeg.")]
    pub async fn video_add_caption(
        &self,
        Parameters(VideoAddCaptionRequest {
            video_url,
            text,
            position,
            font_size,
        }): Parameters<VideoAddCaptionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_add_caption", async {
            let _admission = self.admit_heavy_operation()?;
            let pos = position.as_deref().unwrap_or("bottom");
            let size = font_size.unwrap_or(24);
            if size == 0 {
                return Err(McpToolError::invalid_argument(
                    "font_size must be greater than 0",
                ));
            }

            #[cfg(test)]
            let bypass_dns = pause_after_local_video_admission().await?;
            #[cfg(not(test))]
            let bypass_dns = false;
            if !crate::is_local_media_path(&video_url) && !bypass_dns {
                validate_tool_url_with_dns(&video_url).await?;
            }
            self.require_ffmpeg()?;
            let params = CaptionEffectiveParams {
                source: &video_url,
                text: &text,
                position: pos,
                font_size: size,
            };
            let output = self
                .ffmpeg
                .add_caption(
                    params.source,
                    params.text,
                    params.position,
                    params.font_size,
                )
                .await
                .map_err(map_media_error)?;
            let effective_params = LocalVideoEffectiveParams::Caption(params);
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                effective_params.op(),
                effective_params.status(),
                effective_params.format(),
                &effective_params,
            )
        })
        .await
    }

    #[tool(description = "Generate a video remix: clip, add caption, convert to GIF.")]
    pub async fn video_remix(
        &self,
        Parameters(VideoRemixRequest {
            video_url,
            start_sec,
            end_sec,
            caption_text,
        }): Parameters<VideoRemixRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_remix", async {
            let _admission = self.admit_heavy_operation()?;
            if start_sec >= end_sec {
                return Err(McpToolError::invalid_argument(
                    "start_sec must be less than end_sec.",
                ));
            }

            #[cfg(test)]
            let bypass_dns = pause_after_local_video_admission().await?;
            #[cfg(not(test))]
            let bypass_dns = false;
            if !crate::is_local_media_path(&video_url) && !bypass_dns {
                validate_tool_url_with_dns(&video_url).await?;
            }
            self.require_ffmpeg()?;

            let params = RemixEffectiveParams {
                source: &video_url,
                start_sec,
                end_sec,
                caption_text: caption_text.as_deref(),
                caption_position: REMIX_CAPTION_POSITION,
                caption_font_size: REMIX_CAPTION_FONT_SIZE,
                gif_start_sec: REMIX_GIF_START_SEC,
                gif_duration_sec: end_sec - start_sec,
                gif_width: REMIX_GIF_WIDTH,
                gif_fps: REMIX_GIF_FPS,
            };
            let mut intermediates = LocalVideoIntermediates::new();
            let clipped = self
                .ffmpeg
                .clip(params.source, params.start_sec, params.end_sec)
                .await
                .map_err(map_media_error)?;
            intermediates.track(clipped.clone());

            let captioned = if let Some(cap) = params.caption_text {
                let captioned = self
                    .ffmpeg
                    .add_caption(
                        &clipped.to_string_lossy(),
                        cap,
                        params.caption_position,
                        params.caption_font_size,
                    )
                    .await
                    .map_err(map_media_error)?;
                intermediates.track(captioned.clone());
                captioned
            } else {
                clipped.clone()
            };

            let gif = self
                .ffmpeg
                .to_gif(
                    &captioned.to_string_lossy(),
                    params.gif_start_sec,
                    params.gif_duration_sec,
                    params.gif_width,
                    params.gif_fps,
                )
                .await
                .map_err(map_media_error)?;
            let mut final_cleanup = LocalVideoIntermediates::new();
            final_cleanup.track(gif.clone());
            intermediates.cleanup().map_err(map_media_error)?;

            let effective_params = LocalVideoEffectiveParams::Remix(params);
            crate::assets::publish_local_media(
                &self.gallery_store,
                &gif,
                effective_params.op(),
                effective_params.status(),
                effective_params.format(),
                &effective_params,
            )
        })
        .await
    }

    #[tool(description = "Create a video or GIF from a sequence of gallery images using ffmpeg.")]
    pub async fn video_from_images(
        &self,
        Parameters(VideoFromImagesRequest {
            image_indices,
            fps,
            format,
        }): Parameters<VideoFromImagesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_from_images", async {
            let _admission = self.admit_heavy_operation()?;
            validate_item_count(
                "image_indices",
                image_indices.len(),
                1,
                hkask_types::media_limits::MAX_IMAGE_SEQUENCE_ITEMS,
            )?;

            let fps = fps.unwrap_or(24);
            if fps == 0 {
                return Err(McpToolError::invalid_argument("fps must be greater than 0"));
            }
            let requested_format = format.as_deref().unwrap_or("mp4");
            let format =
                crate::assets::LocalVideoFormat::parse(requested_format).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "Unsupported video format {requested_format:?}; expected mp4 or gif"
                    ))
                })?;
            let gallery = self.access_gallery().map_err(map_media_error)?;
            #[cfg(test)]
            pause_after_local_video_admission().await?;
            self.require_ffmpeg()?;

            let mut paths = Vec::new();
            for idx in &image_indices {
                paths.push(
                    resolve_image_path_in_gallery(self, &gallery, *idx).map_err(map_media_error)?,
                );
            }
            let sources = paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>();

            let params = ImagesEffectiveParams {
                sources: &sources,
                image_indices: &image_indices,
                fps,
            };
            let effective_params = match format {
                crate::assets::LocalVideoFormat::Mp4 => {
                    LocalVideoEffectiveParams::ImagesMp4(params)
                }
                crate::assets::LocalVideoFormat::Gif => {
                    LocalVideoEffectiveParams::ImagesGif(params)
                }
            };
            let output = self
                .ffmpeg
                .images_to_video(&paths, fps, format)
                .await
                .map_err(map_media_error)?;

            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                effective_params.op(),
                effective_params.status(),
                effective_params.format(),
                &effective_params,
            )
        })
        .await
    }

    #[tool(description = "Concatenate multiple video clips into one using ffmpeg.")]
    pub async fn video_concat(
        &self,
        Parameters(VideoConcatRequest { video_urls }): Parameters<VideoConcatRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_concat", async {
            let _admission = self.admit_heavy_operation()?;
            validate_item_count(
                "video_urls",
                video_urls.len(),
                2,
                hkask_types::media_limits::MAX_CONCAT_ITEMS,
            )?;

            #[cfg(test)]
            let bypass_dns = pause_after_local_video_admission().await?;
            #[cfg(not(test))]
            let bypass_dns = false;
            for url in &video_urls {
                if !crate::is_local_media_path(url) && !bypass_dns {
                    validate_tool_url_with_dns(url).await?;
                }
            }
            self.require_ffmpeg()?;

            let params = ConcatEffectiveParams {
                sources: &video_urls,
            };
            let output = self
                .ffmpeg
                .concat(params.sources)
                .await
                .map_err(map_media_error)?;
            let effective_params = LocalVideoEffectiveParams::Concat(params);
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                effective_params.op(),
                effective_params.status(),
                effective_params.format(),
                &effective_params,
            )
        })
        .await
    }

    #[tool(
        description = "Generate a description of video content by extracting keyframes and analyzing them with a vision LLM."
    )]
    pub async fn video_caption(
        &self,
        Parameters(VideoCaptionRequest { video_url, style }): Parameters<VideoCaptionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_caption", async {
            let _admission = self.admit_heavy_operation()?;
            if !crate::is_local_media_path(&video_url) {
                validate_tool_url_with_dns(&video_url).await?;
            }

            let style_str = style.as_deref().unwrap_or("descriptive");
            self.require_ffmpeg()?;

            let frames = self
                .ffmpeg
                .extract_keyframes(&video_url, 2.0, 10)
                .await
                .map_err(map_media_error)?;

            if frames.is_empty() {
                return Err(McpToolError::internal("No keyframes extracted from video."));
            }

            let mut image_urls = Vec::new();
            let mut frame_read_failures = Vec::new();
            for frame in &frames {
                match std::fs::read(frame) {
                    Ok(data) => {
                        let b64 = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &data,
                        );
                        image_urls.push(format!("data:image/jpeg;base64,{b64}"));
                    }
                    Err(error) => frame_read_failures.push(format!("{}: {error}", frame.display())),
                }
            }
            if !frame_read_failures.is_empty() {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    failure_count = frame_read_failures.len(),
                    causes = %frame_read_failures.join("; "),
                    "Some extracted keyframes could not be read"
                );
            }

            let mut vars = HashMap::new();
            vars.insert("style", style_str);
            let prompt = self
                .render_prompt("video_caption", &vars)
                .map_err(|e| McpToolError::internal(format!("Template render failed: {}", e)))?;

            let (vision_model, _vision_label) = self.require_vision().await?;
            let params = hkask_types::template::LLMParameters::default();
            let mut image_b64s = Vec::with_capacity(image_urls.len());
            for url in &image_urls {
                image_b64s.push(
                    crate::gallery::vision::load_image_as_png_base64(url)
                        .await
                        .map_err(crate::error::map_media_error)?,
                );
            }
            let result = self
                .vision_port
                .generate_vision(&prompt, &image_b64s, &params, Some(vision_model.as_str()))
                .await;

            match result {
                Ok(r) => Ok(serde_json::json!({
                    "caption": r.text.trim(),
                    "style": style_str,
                    "frames_analyzed": image_urls.len(),
                })),
                Err(e) => Err(classify_inference_error("Vision inference failed", e)),
            }
        })
        .await
    }

    #[tool(
        description = "Extract keyframes from a video as gallery assets. Each frame becomes a searchable gallery image with its own lineage. Returns the gallery indices of the imported frames."
    )]
    pub async fn video_extract_frames(
        &self,
        Parameters(VideoExtractFramesRequest {
            video_url,
            interval_sec,
            max_frames,
        }): Parameters<VideoExtractFramesRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_extract_frames", async {
            let _admission = self.admit_heavy_operation()?;
            if !interval_sec.is_finite() || interval_sec <= 0.0 {
                return Err(McpToolError::invalid_argument(
                    "interval_sec must be finite and greater than 0",
                ));
            }
            validate_item_count(
                "max_frames",
                max_frames as usize,
                1,
                hkask_types::media_limits::MAX_EXTRACTED_FRAMES as usize,
            )?;
            if !crate::is_local_media_path(&video_url) {
                validate_tool_url_with_dns(&video_url).await?;
            }
            self.require_ffmpeg()?;

            let frames = self
                .ffmpeg
                .extract_keyframes(&video_url, interval_sec, max_frames)
                .await
                .map_err(map_media_error)?;

            if frames.is_empty() {
                return Err(McpToolError::internal("No keyframes extracted from video."));
            }

            // Promote each scratch frame into a durable gallery asset. The
            // extraction batch owns all scratch cleanup; failed durable copies
            // are removed here while successful imports remain published.
            let durable_directory = crate::assets::generated_assets_dir();
            std::fs::create_dir_all(&durable_directory).map_err(|error| {
                map_media_error(MediaError::Io(format!(
                    "Failed to create generated assets directory {}: {error}",
                    durable_directory.display()
                )))
            })?;
            let mut imported = Vec::new();
            let mut failures = Vec::new();
            for (frame_index, frame) in (&frames).into_iter().enumerate() {
                let durable_path = durable_directory.join(format!("{}.jpg", uuid::Uuid::new_v4()));
                let import_result = std::fs::copy(frame, &durable_path)
                    .map_err(|error| MediaError::Io(format!("durable copy failed: {error}")))
                    .and_then(|_| self.import_reference_image(&durable_path, &durable_directory));
                match import_result {
                    Ok((image_id, image_url)) => imported.push(serde_json::json!({
                        "frame_index": frame_index,
                        "image_id": image_id,
                        "image_url": image_url,
                    })),
                    Err(error) => {
                        let mut cause = error.to_string();
                        match std::fs::remove_file(&durable_path) {
                            Ok(()) => {}
                            Err(cleanup_error)
                                if cleanup_error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(cleanup_error) => {
                                cause.push_str(&format!(
                                    "; failed durable-copy cleanup: {cleanup_error}"
                                ));
                            }
                        }
                        failures.push(serde_json::json!({
                            "frame_index": frame_index,
                            "scratch_file": frame.file_name().and_then(|name| name.to_str()),
                            "cause": cause,
                        }));
                    }
                }
            }

            if !failures.is_empty() {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    extracted = frames.len(),
                    imported = imported.len(),
                    failed = failures.len(),
                    "Some extracted keyframes could not be imported"
                );
            }
            let status = if failures.is_empty() {
                "completed"
            } else if imported.is_empty() {
                "failed"
            } else {
                "partial"
            };

            Ok(serde_json::json!({
                "status": status,
                "frames_extracted": frames.len(),
                "frames_imported": imported.len(),
                "frames": imported,
                "failures": failures,
            }))
        })
        .await
    }

    #[tool(
        description = "Create a meme video from a gallery image with text overlay and camera motion. Composes text rendering + AI motion generation. Perfect for 'WHEN YOU SEE IT' style memes."
    )]
    pub async fn video_meme(
        &self,
        Parameters(VideoMemeRequest {
            image_index,
            top_text,
            bottom_text,
            motion,
            duration,
            font_path,
        }): Parameters<VideoMemeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_meme", async {
            let _admission = self.admit_heavy_operation()?;
            let image_path = self
                .resolve_image_path(image_index)
                .map_err(map_media_error)?;

            let mut img = image::open(&image_path).map_err(|e| map_image_open_error(&image_path, e))?;

            let font = load_meme_font(font_path.as_deref()).map_err(|e| {
                McpToolError::unavailable(format!(
                    "No font available for text rendering: {}. Install fonts-dejavu-core or provide --font_path.",
                    e
                ))
            })?;

            let img_w = img.width();
            let img_h = img.height();
            let scale = ab_glyph::PxScale::from(img_h as f32 * 0.10);
            let white = image::Rgba([255u8, 255u8, 255u8, 255u8]);
            let black = image::Rgba([0u8, 0u8, 0u8, 255u8]);

            if let Some(ref text) = top_text {
                let text_upper: String = text.to_uppercase();
                let (tw, _th) = measure_text(&font, scale, &text_upper);
                let x = ((img_w as i32 - tw as i32) / 2).max(0);
                let y = (img_h as f32 * 0.05) as i32;
                for &(dx, dy) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    draw_text_mut(&mut img, black, x + dx, y + dy, scale, &font, &text_upper);
                }
                draw_text_mut(&mut img, white, x, y, scale, &font, &text_upper);
            }

            if let Some(ref text) = bottom_text {
                let text_upper: String = text.to_uppercase();
                let (tw, th) = measure_text(&font, scale, &text_upper);
                let x = ((img_w as i32 - tw as i32) / 2).max(0);
                let y = (img_h as i32 - th as i32 - (img_h as f32 * 0.05) as i32).max(0);
                for &(dx, dy) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    draw_text_mut(&mut img, black, x + dx, y + dy, scale, &font, &text_upper);
                }
                draw_text_mut(&mut img, white, x, y, scale, &font, &text_upper);
            }

            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| map_image_open_error(std::path::Path::new("<meme composite>"), e))?;
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, buf.get_ref());
            let data_uri = format!("data:image/png;base64,{}", b64);

            let motion_prompt = if motion.is_empty() {
                "slow zoom in".to_string()
            } else {
                motion.clone()
            };
            let media_params = hkask_types::MediaGenerateParams {
                image_url: Some(data_uri.clone()),
                prompt: Some(motion_prompt.clone()),
                duration,
                ..Default::default()
            };
            let args = serde_json::to_value(&media_params)
                .unwrap_or(serde_json::Value::Null);
            let result = self
                .vision_port
                .media_generate("image_to_video", &media_params)
                .await
                .map_err(|e| classify_inference_error("Image-to-video failed", e))?;
            // Persist the video payload and compose the slim result (the
            // payload never enters the model's context). Previously this
            // tool returned the raw provider response unpersisted.
            persist_slim_and_enrich(
                &self.gallery_store,
                &result,
                "video_meme",
                "video",
                args,
            )
            .await
        })
        .await
    }

    /// Probe a video file for metadata — duration, dimensions, codec, fps,
    /// bit rate. Uses `ffprobe` (bundled with ffmpeg). The timeline-editor
    /// data source: the UI needs duration to render the timeline strip and
    /// fps to compute frame positions.
    #[tool(
        description = "Probe a video file for metadata — duration, dimensions, codec, fps, bit rate. Uses ffprobe (bundled with ffmpeg)."
    )]
    pub async fn video_info(
        &self,
        Parameters(VideoInfoRequest { video_url }): Parameters<VideoInfoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_info", async {
            let _admission = self.admit_heavy_operation()?;
            if !crate::is_local_media_path(&video_url) {
                validate_tool_url_with_dns(&video_url).await?;
            }
            let ffmpeg = self.require_ffmpeg()?;
            let info = ffmpeg.probe(&video_url).await.map_err(map_media_error)?;
            serde_json::to_value(&info)
                .map_err(|e| McpToolError::internal(format!("encode video info: {e}")))
        })
        .await
    }

    /// Download a video from a URL (YouTube, Vimeo, direct file, etc.) to
    /// local storage, index it in the gallery, and return a media block for
    /// immediate viewing. Uses `yt-dlp` for platform URLs. Complements the
    /// streaming widget (W1): stream for immediate viewing, fetch for
    /// persistence.
    #[tool(
        description = "Download a video from a URL (YouTube, Vimeo, direct file) to local storage, index it in the gallery, and return a media block for viewing. Requires yt-dlp for platform URLs."
    )]
    pub async fn video_fetch(
        &self,
        Parameters(VideoFetchRequest { url }): Parameters<VideoFetchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_fetch", async {
            let _admission = self.admit_heavy_operation()?;
            if url.trim().is_empty() {
                return Err(McpToolError::invalid_argument("url must not be empty"));
            }

            validate_tool_url_with_dns(&url).await?;
            let ytdlp = self.require_yt_dlp()?;
            let scratch = tempfile::Builder::new()
                .prefix(".video-fetch-")
                .tempdir_in(crate::assets::generated_assets_dir())
                .map_err(|error| {
                    McpToolError::internal(format!("create video_fetch scratch directory: {error}"))
                })?;
            let output_path = scratch.path().join("download.mp4");

            let fetch = ytdlp
                .fetch(&url, &output_path)
                .await
                .map_err(map_media_error)?;
            let metadata = std::fs::metadata(&output_path).map_err(|error| {
                McpToolError::internal(format!(
                    "yt-dlp completed without a readable output file {}: {error}",
                    output_path.display()
                ))
            })?;
            if !metadata.is_file() || metadata.len() == 0 {
                let cleanup = std::fs::remove_file(&output_path);
                let detail = match cleanup {
                    Ok(()) => String::new(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(error) => format!("; cleanup failed: {error}"),
                };
                return Err(McpToolError::internal(format!(
                    "yt-dlp produced no complete media file{}",
                    detail
                )));
            }

            let mut result = crate::assets::publish_local_media(
                &self.gallery_store,
                &output_path,
                "video_fetch",
                "fetched",
                crate::assets::LocalMediaFormat::Mp4,
                &serde_json::json!({ "source_url": url }),
            )?;
            if let Some(warning) = fetch.warning
                && let Some(object) = result.as_object_mut()
            {
                object.insert("warning".to_string(), serde_json::Value::String(warning));
            }
            Ok(result)
        })
        .await
    }
}
