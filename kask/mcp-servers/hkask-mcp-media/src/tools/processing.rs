//! Source-linked processing and presentation tools.
use crate::*;

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
struct ConcatEffectiveParams<'a> {
    sources: &'a [String],
}

#[derive(serde::Serialize)]
struct CollageEffectiveParams {
    layout: String,
    image_count: usize,
    cols: u32,
    rows: u32,
    canvas_width: u32,
    canvas_height: u32,
    spacing: u32,
}

#[derive(serde::Serialize)]
#[serde(untagged)]
enum LocalVideoEffectiveParams<'a> {
    Clip(ClipEffectiveParams<'a>),
    Gif(GifEffectiveParams<'a>),
    Caption(CaptionEffectiveParams<'a>),
    Concat(ConcatEffectiveParams<'a>),
}

impl LocalVideoEffectiveParams<'_> {
    const fn op(&self) -> &'static str {
        match self {
            Self::Clip(_) => "video_clip",
            Self::Gif(_) => "video_to_gif",
            Self::Caption(_) => "video_add_caption",
            Self::Concat(_) => "video_concat",
        }
    }

    const fn status(&self) -> &'static str {
        match self {
            Self::Clip(_) => "clipped",
            Self::Gif(_) => "converted",
            Self::Caption(_) => "captioned",
            Self::Concat(_) => "concatenated",
        }
    }

    const fn format(&self) -> crate::assets::LocalMediaFormat {
        match self {
            Self::Gif(_) => crate::assets::LocalMediaFormat::Gif,
            Self::Clip(_) | Self::Caption(_) | Self::Concat(_) => {
                crate::assets::LocalMediaFormat::Mp4
            }
        }
    }
}

/// Compose a collage grid: dark canvas, cells sized by `cols`/`rows` and
/// `spacing`, each image scaled to its cell (Lanczos3) and centered.
/// Shared by the collage tool and its composition test — the grid math is
/// the tested contract, so it lives once.
pub(crate) fn compose_collage_grid(
    images: &[image::DynamicImage],
    cols: u32,
    rows: u32,
    canvas_w: u32,
    canvas_h: u32,
    spacing: u32,
) -> image::DynamicImage {
    let cell_w = (canvas_w - spacing * (cols + 1)) / cols;
    let cell_h = (canvas_h - spacing * (rows + 1)) / rows;

    let mut canvas = image::DynamicImage::new_rgba8(canvas_w, canvas_h);
    let bg = image::Rgba([30u8, 30u8, 30u8, 255u8]);
    for pixel in canvas
        .as_mut_rgba8()
        .expect("canvas was created as RGBA8")
        .pixels_mut()
    {
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

        let x = spacing
            + col * (cell_w + spacing)
            + (cell_w.saturating_sub(spacing) - scaled.width()) / 2;
        let y = spacing
            + row * (cell_h + spacing)
            + (cell_h.saturating_sub(spacing) - scaled.height()) / 2;

        image::imageops::overlay(&mut canvas, &scaled, x as i64, y as i64);
    }
    canvas
}

/// Captioning is explicitly bounded visual sampling, not whole-recording analysis.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SampledVideoCaptionRequest {
    pub video_url: String,
    pub style: Option<String>,
    /// Minimum separation between selected source frames in seconds.
    #[serde(default = "caption_interval")]
    pub interval_sec: f32,
    /// Maximum number of sampled frames sent for visual analysis.
    #[serde(default = "caption_frame_limit")]
    pub max_frames: u32,
}

fn caption_interval() -> f32 {
    2.0
}
fn caption_frame_limit() -> u32 {
    10
}

fn validate_sampling(interval_sec: f32, max_frames: u32) -> Result<(), McpToolError> {
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
    )
}

/// Hash and decode the same private snapshot, never a mutable original or URL.
/// `pub(crate)`: the media layer passes reuse the capture/stamping machinery
/// (source_sha256 identity + publish params) for their sampled frames.
pub(crate) struct SourceSnapshot {
    file: tempfile::NamedTempFile,
    pub(crate) identity: serde_json::Value,
}

impl SourceSnapshot {
    pub(crate) async fn capture(store: &GalleryStore, source: &str) -> Result<Self, McpToolError> {
        use sha2::Digest;
        use std::io::{Read, Write};
        let mut file = tempfile::NamedTempFile::new()
            .map_err(MediaError::from)
            .map_err(map_media_error)?;
        let canonical = if crate::is_local_media_path(source) {
            let path = std::path::Path::new(source.strip_prefix("file://").unwrap_or(source))
                .canonicalize()
                .map_err(MediaError::from)
                .map_err(map_media_error)?;
            let mut original = std::fs::File::open(&path)
                .map_err(MediaError::from)
                .map_err(map_media_error)?;
            std::io::copy(&mut original, file.as_file_mut())
                .map_err(MediaError::from)
                .map_err(map_media_error)?;
            Some(path)
        } else {
            let bytes = crate::assets::download_asset(source)
                .await
                .map_err(map_media_error)?;
            file.write_all(&bytes)
                .map_err(MediaError::from)
                .map_err(map_media_error)?;
            None
        };
        let mut hasher = sha2::Sha256::new();
        let mut snapshot = file
            .reopen()
            .map_err(MediaError::from)
            .map_err(map_media_error)?;
        let mut buffer = [0_u8; 65536];
        loop {
            let read = snapshot
                .read(&mut buffer)
                .map_err(MediaError::from)
                .map_err(map_media_error)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let hash = format!("{:x}", hasher.finalize());
        let mut parent_id = None;
        if let Some(path) = &canonical {
            if let Some(gallery) = store
                .containing(path)
                .map_err(crate::error::map_gallery_store_error)?
            {
                let count = store
                    .count_assets(&gallery.id)
                    .map_err(crate::error::map_gallery_store_error)?;
                for offset in (0..count).step_by(100) {
                    for record in store
                        .list_assets(&gallery.id, offset as usize, 100)
                        .map_err(crate::error::map_gallery_store_error)?
                    {
                        if std::path::Path::new(&record.absolute_path) == path
                            && !record.missing
                            && record.hash == hash
                        {
                            parent_id = Some(record.id);
                        }
                    }
                }
            }
        }
        Ok(Self {
            file,
            identity: serde_json::json!({
                "source": source, "source_id": format!("urn:sha256:{hash}"),
                "source_sha256": hash, "parent_image_id": parent_id,
            }),
        })
    }

    pub(crate) fn path(&self) -> String {
        self.file.path().to_string_lossy().into_owned()
    }
}

fn sampled_coverage(interval_sec: f32, max_frames: u32, timestamps: &[f64]) -> serde_json::Value {
    serde_json::json!({
        "method": "original_frames_minimum_interval_showinfo_pts",
        "requested": { "interval_sec": interval_sec, "max_frames": max_frames,
            "nominal_start_offset_sec": 0.0,
            "nominal_last_sample_offset_sec": f64::from(interval_sec) * f64::from(max_frames - 1) },
        "observed": { "source_pts_sec": timestamps, "first_source_pts_sec": timestamps.first(),
            "last_source_pts_sec": timestamps.last() },
        "limitation": "Sampled visual frames only; intervening and later events, audio, speech, and whole-recording coverage are not assessed. Requested offsets are a sampling plan, not observed timestamps."
    })
}

#[tool_router(router = processing_router, vis = "pub")]
impl MediaServer {
    /// Admission choreography shared by the local-media tools: local
    /// inputs must exist, the test gate observes admission (test builds
    /// only), remote inputs pass DNS validation, and ffmpeg must be
    /// available — in that order, once per tool call. Video tools and the
    /// kind-agnostic clip both pass through here.
    async fn admit_local_media_inputs(&self, urls: &[String]) -> Result<(), McpToolError> {
        for url in urls {
            ensure_local_input_exists(url)?;
        }

        #[cfg(test)]
        let bypass_dns = pause_after_local_video_admission().await?;
        #[cfg(not(test))]
        let bypass_dns = false;
        for url in urls {
            if !crate::is_local_media_path(url) && !bypass_dns {
                validate_tool_url_with_dns(url).await?;
            }
        }
        self.require_ffmpeg()?;
        Ok(())
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

            let canvas = compose_collage_grid(&images, cols, rows, canvas_w, canvas_h, spacing);

            // Scratch write consumed by the canonical publication below —
            // the publish path stages the bytes durably, removes the scratch
            // (rollback-armed on every failure path), and returns the durable
            // asset's path with its gallery identity (PR-M2: the collage is no
            // longer the one asset-producing tool outside the storage
            // contract — no bare temp path, no orphan subject to OS cleanup).
            let temp_dir = std::env::temp_dir().join("hkask-media");
            if let Err(error) = std::fs::create_dir_all(&temp_dir) {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    path = %temp_dir.display(),
                    %error,
                    "Failed to create collage scratch directory — the subsequent write will surface the failure"
                );
            }
            let scratch_path = temp_dir.join(format!("collage_{}.png", uuid::Uuid::new_v4()));

            canvas
                .save(&scratch_path)
                .map_err(|e| map_image_open_error(&scratch_path, e))?;

            let effective_params = CollageEffectiveParams {
                layout: layout.clone(),
                image_count: images.len(),
                cols,
                rows,
                canvas_width: canvas_w,
                canvas_height: canvas_h,
                spacing,
            };
            crate::assets::publish_local_media(
                &self.gallery_store,
                &scratch_path,
                "image_create_collage",
                "created",
                crate::assets::LocalMediaFormat::Png,
                &effective_params,
            )
        })
        .await
    }

    // ── Video tools ──────────────────────────────────────────────────────────

    /// expect: My clipped video remains available in the admitted gallery under one stable identity.
    /// [P1] Motivating: user work survives processor and server teardown.
    /// pre: an active gallery exists and `0 <= start_sec < end_sec`.
    /// post: the durable output, gallery row, lineage, result id, and media-block id agree.
    /// [P1] Constraining: any publication failure removes the output and coupled rows.
    #[tool(
        description = "Trim media to specified start/end times using local ffmpeg — video inputs stream-copy losslessly to mp4, audio inputs decode to wav; the published asset kind follows the input."
    )]
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
            self.admit_local_media_inputs(std::slice::from_ref(&video_url))
                .await?;

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
            // The publication format follows the real output (2026-10-27):
            // audio inputs decode to wav — publishing them as the op's
            // default Mp4 would mislabel the asset's kind and display hint.
            let format = if output.extension().and_then(|ext| ext.to_str()) == Some("wav") {
                crate::assets::LocalMediaFormat::Wav
            } else {
                effective_params.format()
            };
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                effective_params.op(),
                effective_params.status(),
                format,
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
            self.admit_local_media_inputs(std::slice::from_ref(&video_url))
                .await?;
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
            self.admit_local_media_inputs(std::slice::from_ref(&video_url))
                .await?;
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
            self.admit_local_media_inputs(&video_urls).await?;

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
        description = "Analyze sampled video frames with a vision model. Returns requested sampling coverage, actual source-frame timestamps, and explicit visual-only limitations; not whole-recording analysis."
    )]
    pub async fn video_caption(
        &self,
        Parameters(SampledVideoCaptionRequest {
            video_url,
            style,
            interval_sec,
            max_frames,
        }): Parameters<SampledVideoCaptionRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_caption", async {
            let _admission = self.admit_heavy_operation()?;
            ensure_local_input_exists(&video_url)?;
            if !crate::is_local_media_path(&video_url) {
                validate_tool_url_with_dns(&video_url).await?;
            }

            validate_sampling(interval_sec, max_frames)?;
            let style_str = style.as_deref().unwrap_or("descriptive");
            self.require_ffmpeg()?;
            let source = SourceSnapshot::capture(&self.gallery_store, &video_url).await?;

            let frames = self
                .ffmpeg
                .extract_keyframes(&source.path(), interval_sec, max_frames)
                .await
                .map_err(map_media_error)?;

            if frames.is_empty() {
                return Err(McpToolError::failed_precondition("No keyframes extracted from video."));
            }

            let mut image_urls = Vec::new();
            let mut frame_read_failures = Vec::new();
            let mut observed_timestamps = Vec::new();
            for (frame, timestamp) in (&frames).into_iter().zip(&frames.timestamps_sec) {
                match std::fs::read(frame) {
                    Ok(data) => {
                        let b64 = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &data,
                        );
                        image_urls.push(format!("data:image/jpeg;base64,{b64}"));
                        observed_timestamps.push(*timestamp);
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

            if image_urls.is_empty() {
                return Err(McpToolError::internal(format!("No sampled frames readable: {}", frame_read_failures.join("; "))));
            }
            let coverage = sampled_coverage(interval_sec, max_frames, &observed_timestamps);
            let mut vars = HashMap::new();
            vars.insert("style", style_str);
            let prompt = self
                .render_prompt("video_caption", &vars)
                .map_err(|e| McpToolError::internal(format!("Template render failed: {}", e)))?;

            let prompt = format!("{prompt}\nDescribe ONLY the supplied sampled visual frames. Do not infer audio, speech, unseen events or whole-recording coverage. Source frame timestamps in image order (seconds): {observed_timestamps:?}. State the sampled-only limitation in the description.");
            let (vision_model, _vision_label) = self.require_vision()?;
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
                    "source": source.identity,
                    "sampling": coverage,
                    "frame_read_failures": frame_read_failures,
                })),
                Err(e) => Err(classify_inference_error("Vision inference failed", e)),
            }
        })
        .await
    }

    #[tool(
        description = "Sample original video frames as durable gallery assets with source hash identity and actual source PTS. Returns stable asset IDs, source mappings, and per-frame publication failures."
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
            validate_sampling(interval_sec, max_frames)?;
            ensure_local_input_exists(&video_url)?;
            if !crate::is_local_media_path(&video_url) {
                validate_tool_url_with_dns(&video_url).await?;
            }
            self.require_ffmpeg()?;

            let source = SourceSnapshot::capture(&self.gallery_store, &video_url).await?;
            let frames = self
                .ffmpeg
                .extract_keyframes(&source.path(), interval_sec, max_frames)
                .await
                .map_err(map_media_error)?;

            if frames.is_empty() {
                return Err(McpToolError::failed_precondition(
                    "No keyframes extracted from video.",
                ));
            }

            // Only extracted scratch outputs are consumed by the atomic publisher.
            let mut imported = Vec::new();
            let mut failures = Vec::new();
            for (frame_index, (frame, timestamp)) in (&frames)
                .into_iter()
                .zip(&frames.timestamps_sec)
                .enumerate()
            {
                let mut params = source.identity.clone();
                params["source_pts_sec"] = serde_json::json!(timestamp);
                params["frame_index"] = serde_json::json!(frame_index);
                params["sampling_interval_sec"] = serde_json::json!(interval_sec);
                params["timestamp_basis"] =
                    serde_json::json!("selected_original_frame_showinfo_pts");
                let publication = image::image_dimensions(frame)
                    .map_err(|error| {
                        McpToolError::internal(format!("Invalid extracted image: {error}"))
                    })
                    .and_then(|_| {
                        crate::assets::publish_local_media(
                            &self.gallery_store,
                            frame,
                            "video_extract_frames",
                            "extracted",
                            crate::assets::LocalMediaFormat::Jpg,
                            &params,
                        )
                    });
                match publication {
                    Ok(result) => imported.push(result),
                    Err(error) => failures.push(serde_json::json!({
                        "frame_index": frame_index, "source_pts_sec": timestamp,
                        "source": source.identity, "cause": error.to_string(),
                    })),
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
                "source": source.identity,
                "sampling": sampled_coverage(interval_sec, max_frames, &frames.timestamps_sec),
                "frames_extracted": frames.len(),
                "frames_imported": imported.len(),
                "frames": imported,
                "failures": failures,
            }))
        })
        .await
    }

    #[tool(
        description = "Probe a video file for metadata — duration, dimensions, codec, fps, bit rate. Uses ffprobe (bundled with ffmpeg)."
    )]
    pub async fn video_info(
        &self,
        Parameters(VideoInfoRequest { video_url }): Parameters<VideoInfoRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "video_info", async {
            let _admission = self.admit_heavy_operation()?;
            ensure_local_input_exists(&video_url)?;
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

#[cfg(test)]
mod curation_tests {
    use super::*;
    use sha2::Digest;

    struct NoProvider;
    crate::test_support::media_inference_stub! {
        NoProvider,
        generate(self, _prompt): {
            Box::pin(async { panic!("curation test must not call providers") })
        }
    }

    fn store() -> Arc<GalleryStore> {
        Arc::new(
            GalleryStore::from_driver(
                hkask_storage::database::sqlite::SqliteDriver::in_memory_driver(),
            )
            .expect("in-memory gallery"),
        )
    }

    fn server(store: Arc<GalleryStore>, ffmpeg: FfmpegRunner) -> MediaServer {
        MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoProvider),
            Arc::new(Mutex::new(None)),
            store,
            templates::create_env().expect("media templates"),
            ffmpeg,
            YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        )
    }

    #[test]
    fn sampled_visual_coverage_distinguishes_plan_from_observation() {
        let coverage = sampled_coverage(2.0, 3, &[0.37, 2.42]);
        assert_eq!(coverage["requested"]["nominal_last_sample_offset_sec"], 4.0);
        assert_eq!(
            coverage["observed"]["source_pts_sec"],
            serde_json::json!([0.37, 2.42])
        );
        assert!(
            coverage["limitation"]
                .as_str()
                .expect("limitation")
                .contains("whole-recording coverage are not assessed")
        );
        assert!(validate_sampling(f32::NAN, 3).is_err());
        assert!(validate_sampling(2.0, 0).is_err());
        assert!(validate_sampling(2.0, 3).is_ok());
        let request: SampledVideoCaptionRequest = serde_json::from_value(serde_json::json!({
            "video_url": "source.mp4", "interval_sec": 7.5, "max_frames": 4,
        }))
        .expect("sampling controls deserialize");
        assert_eq!((request.interval_sec, request.max_frames), (7.5, 4));
    }

    #[tokio::test]
    async fn missing_source_is_not_found_before_processing() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("missing.mp4").display().to_string();
        let server = server(store(), FfmpegRunner::detect());
        let extract = server
            .video_extract_frames(Parameters(VideoExtractFramesRequest {
                video_url: path.clone(),
                interval_sec: 2.0,
                max_frames: 1,
            }))
            .await
            .expect_err("missing source must fail");
        assert!(extract.to_string().contains("not_found"));
        let caption = server
            .video_caption(Parameters(SampledVideoCaptionRequest {
                video_url: path,
                style: None,
                interval_sec: 2.0,
                max_frames: 1,
            }))
            .await
            .expect_err("caption missing source must fail");
        assert!(caption.to_string().contains("not_found"));
    }

    struct EnvironmentGuard(&'static str, Option<std::ffi::OsString>);
    impl EnvironmentGuard {
        fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
            let guard = Self(key, std::env::var_os(key));
            unsafe { std::env::set_var(key, value) };
            guard
        }
    }
    impl Drop for EnvironmentGuard {
        fn drop(&mut self) {
            match self.1.take() {
                Some(value) => unsafe { std::env::set_var(self.0, value) },
                None => unsafe { std::env::remove_var(self.0) },
            }
        }
    }

    struct SampledVision;
    crate::test_support::media_inference_stub! {
        SampledVision,
        generate(self, _prompt): {
            Box::pin(async { panic!("caption must use the visual port") })
        },
        generate_vision(self, prompt, images): {
            assert_eq!(images.len(), 1);
            assert!(prompt.contains("3.7"));
            assert!(prompt.contains("Do not infer audio, speech, unseen events or whole-recording coverage"));
            Box::pin(async { Ok(hkask_types::InferenceResult {
                text: "One sampled visual frame; unobserved events are not assessed.".into(),
                model: "mock".into(),
                usage: hkask_types::InferenceUsage { prompt_tokens: 1, completion_tokens: 1,
                    total_tokens: 2, reported: true },
                finish_reason: "stop".into(), tool_calls: Vec::new(), reasoning: None, cost_usd: None,
            }) })
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn caption_controls_and_sampled_limitations_roundtrip_without_provider()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let _model = EnvironmentGuard::set(
            crate::models::VISION_ENV,
            std::ffi::OsStr::new("mock/visual"),
        );
        let root = tempfile::tempdir()?;
        let source = root.path().join("original.mp4");
        std::fs::write(&source, b"original bytes")?;
        let image = root.path().join("sample.jpg");
        image::RgbImage::new(2, 2).save(&image)?;
        let script = root.path().join("sample.sh");
        let fixture = image.to_string_lossy().replace('\'', "'\\''");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor arg do pattern=$arg; done\noutput=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\ncp '{fixture}' \"$output\"\nprintf '[Parsed_showinfo_1] n: 0 pts_time:3.7\\n' >&2\n"
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut server = server(
            store(),
            FfmpegRunner::with_binary(script.display().to_string(), root.path().join("scratch")),
        );
        server.vision_port = Arc::new(SampledVision);
        let response = server
            .video_caption(Parameters(SampledVideoCaptionRequest {
                video_url: source.display().to_string(),
                style: None,
                interval_sec: 7.5,
                max_frames: 4,
            }))
            .await?;
        let envelope: serde_json::Value = serde_json::from_str(&response)?;
        let payload = &envelope["content"];
        assert_eq!(payload["frames_analyzed"], 1);
        assert_eq!(payload["sampling"]["requested"]["interval_sec"], 7.5);
        assert_eq!(payload["sampling"]["requested"]["max_frames"], 4);
        assert_eq!(
            payload["sampling"]["observed"]["source_pts_sec"],
            serde_json::json!([3.7])
        );
        assert!(
            payload["sampling"]["limitation"]
                .as_str()
                .ok_or("missing limitation")?
                .contains("Sampled visual frames only")
        );
        assert_eq!(std::fs::read(source)?, b"original bytes");
        Ok(())
    }

    /// The tool must persist selected-frame PTS and the exact decoded source
    /// revision while retaining useful frames when a sibling is invalid.
    #[cfg(unix)]
    #[tokio::test]
    async fn extracted_frame_provenance_roundtrip_preserves_source_and_partial_failures()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let _lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let root = tempfile::tempdir()?;
        let _guard = EnvironmentGuard::set("HKASK_ARTIFACTS_DIR", root.path().as_os_str());
        let original = root.path().join("original.mp4");
        let original_bytes = b"immutable source revision";
        std::fs::write(&original, original_bytes)?;
        let fixture = root.path().join("fixture.jpg");
        image::RgbImage::new(2, 2).save(&fixture)?;
        let script = root.path().join("extract.sh");
        let fixture_path = fixture.to_string_lossy().replace('\'', "'\\''");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor arg do pattern=$arg; done\nfirst=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nsecond=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/002/')\ncp '{fixture_path}' \"$first\"\nprintf invalid > \"$second\"\nprintf '[Parsed_showinfo_1] n: 0 pts_time:0.37\\n[Parsed_showinfo_1] n: 1 pts_time:4.9\\n' >&2\n"
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let store = store();
        let gallery = store.open(&root.path().display().to_string(), GalleryMode::ReadOnly)?;
        let hash = format!("{:x}", sha2::Sha256::digest(original_bytes));
        let parent = store.add_media(
            &gallery.id,
            &original.display().to_string(),
            &hash,
            0,
            0,
            "mp4",
            original_bytes.len() as u64,
            "video",
        )?;
        let scratch = root.path().join("scratch");
        let server = server(
            store.clone(),
            FfmpegRunner::with_binary(script.display().to_string(), scratch.clone()),
        );
        let response = server
            .video_extract_frames(Parameters(VideoExtractFramesRequest {
                video_url: original.display().to_string(),
                interval_sec: 2.0,
                max_frames: 2,
            }))
            .await?;
        let envelope: serde_json::Value = serde_json::from_str(&response)?;
        let payload = &envelope["content"];
        assert_eq!(payload["status"], "partial");
        assert_eq!(payload["frames_imported"], 1);
        assert_eq!(payload["failures"][0]["source_pts_sec"], 4.9);
        let frame = &payload["frames"][0];
        assert_eq!(frame["source_pts_sec"], 0.37);
        assert_eq!(frame["source_sha256"], hash);
        assert_eq!(frame["source_id"], format!("urn:sha256:{hash}"));
        let id = frame["gallery_asset_id"]
            .as_str()
            .ok_or("missing gallery asset id")?;
        let lineage = store.get_generation(id)?.ok_or("missing lineage")?;
        assert_eq!(lineage.parent_image_id.as_deref(), Some(parent.id.as_str()));
        let params: serde_json::Value =
            serde_json::from_str(lineage.params.as_deref().ok_or("missing params")?)?;
        assert_eq!(params["source_sha256"], hash);
        assert_eq!(params["source_pts_sec"], 0.37);
        assert!(store.get_omc_creation_graph(id)?.is_some());
        let durable = store.get_by_id(id)?;
        assert!(std::path::Path::new(&durable.absolute_path).is_file());
        drop(server);
        assert_eq!(std::fs::read(&original)?, original_bytes);
        assert!(std::path::Path::new(&durable.absolute_path).is_file());
        assert!(!scratch.exists());
        Ok(())
    }
}
