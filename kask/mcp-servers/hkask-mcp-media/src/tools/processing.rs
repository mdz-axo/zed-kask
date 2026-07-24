//! Processing tools — background removal, style transfer, collage, video editing, memes.
use crate::*;

#[tool_router(router = processing_router, vis = "pub")]
impl MediaServer {
    // ── Derivation tools ─────────────────────────────────────────────────────

    #[tool(
        description = "Remove background from a gallery image. Delegates to DeepInfra Bria RMBG 2.0."
    )]
    pub async fn image_remove_background(
        &self,
        Parameters(RemoveBackgroundRequest {
            image_index,
            new_bg_color: _new_bg_color,
        }): Parameters<RemoveBackgroundRequest>,
    ) -> String {
        execute_tool(self, "image_remove_background", async {
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            self.inference
                .remove_background(&image_url)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Background removal failed: {}", e)))
        })
        .await
    }

    #[tool(
        description = "Apply style transfer to a gallery image. Delegates to fal.ai Flux dev img2img."
    )]
    pub async fn image_apply_style(
        &self,
        Parameters(ApplyStyleRequest {
            image_index,
            style_prompt,
            strength,
        }): Parameters<ApplyStyleRequest>,
    ) -> String {
        execute_tool(self, "image_apply_style", async {
            if style_prompt.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "style_prompt must not be empty",
                ));
            }
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            self.inference
                .image_to_image(&image_url, &style_prompt, strength)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Style transfer failed: {}", e)))
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
    ) -> String {
        execute_tool(self, "image_create_collage", async {
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
                    .map_err(|e| McpToolError::internal(format!("Failed to query tags: {}", e)))?;

                let mut image_scores: HashMap<String, f64> = HashMap::new();
                for (tag, relative_path) in &all_tags {
                    for term in terms {
                        let sim = levenshtein_similarity(term, &tag.value);
                        if sim >= 0.3 {
                            let weighted = sim * tag.confidence;
                            let entry = image_scores.entry(relative_path.clone()).or_insert(0.0);
                            *entry = entry.max(weighted);
                        }
                    }
                }

                let mut ranked: Vec<(String, f64)> = image_scores.into_iter().collect();
                ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                ranked.truncate(max_items);

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
                    .map_err(|e| McpToolError::internal(format!("Failed to get reference tags: {}", e)))?;

                let all_tags = self
                    .gallery_store
                    .get_all_tags(&ga.gallery_id)
                    .map_err(|e| McpToolError::internal(format!("Failed to query tags: {}", e)))?;

                let mut image_scores: HashMap<String, f64> = HashMap::new();
                for (tag, relative_path) in &all_tags {
                    let abs_path = ga.root_path.join(relative_path);
                    if abs_path == ref_path {
                        continue;
                    }
                    for ref_tag in &ref_tags {
                        let sim = levenshtein_similarity(&ref_tag.value, &tag.value);
                        if sim >= 0.3 {
                            let weighted = sim * tag.confidence;
                            let entry = image_scores.entry(relative_path.clone()).or_insert(0.0);
                            *entry = entry.max(weighted);
                        }
                    }
                }

                let mut ranked: Vec<(String, f64)> = image_scores.into_iter().collect();
                ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                ranked.truncate(max_items.saturating_sub(1));

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
                images.push(
                    image::open(path)
                        .map_err(|e| McpToolError::internal(format!("Failed to open {}: {}", path.display(), e)))?,
                );
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
            let _ = std::fs::create_dir_all(&temp_dir);
            let output_path = temp_dir.join(format!("collage_{}.png", uuid::Uuid::new_v4()));

            canvas
                .save(&output_path)
                .map_err(|e| McpToolError::internal(format!("Failed to save collage: {}", e)))?;

            Ok(serde_json::json!({
                "status": "created",
                "image_count": images.len(),
                "layout": layout,
                "cols": cols,
                "rows": rows,
                "canvas_width": canvas_w,
                "canvas_height": canvas_h,
                "spacing": spacing,
                "output": output_path.display().to_string(),
            }))
        })
        .await
    }

    // ── Video tools ──────────────────────────────────────────────────────────

    #[tool(description = "Trim a video to specified start/end times using local ffmpeg.")]
    pub async fn video_clip(
        &self,
        Parameters(VideoClipRequest {
            video_url,
            start_sec,
            end_sec,
        }): Parameters<VideoClipRequest>,
    ) -> String {
        execute_tool(self, "video_clip", async {
            validate_tool_url(&video_url)?;

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

            self.require_ffmpeg()?;

            let output = self
                .ffmpeg
                .clip(&video_url, start_sec, end_sec)
                .await
                .map_err(map_media_error)?;

            Ok(serde_json::json!({
                "status": "clipped",
                "source": video_url,
                "start_sec": start_sec,
                "end_sec": end_sec,
                "duration": end_sec - start_sec,
                "output": output.display().to_string(),
            }))
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
    ) -> String {
        execute_tool(self, "video_to_gif", async {
            validate_tool_url(&video_url)?;

            self.require_ffmpeg()?;

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

            let output = self
                .ffmpeg
                .to_gif(&video_url, start, dur, w, f)
                .await
                .map_err(map_media_error)?;

            Ok(serde_json::json!({
                "status": "converted",
                "source": video_url,
                "start_sec": start,
                "duration_sec": dur,
                "width": w,
                "fps": f,
                "output": output.display().to_string(),
            }))
        })
        .await
    }

    #[tool(
        description = "Animate a gallery image into a short video clip. Delegates to fal.ai Seedance 2.0."
    )]
    pub async fn image_to_video(
        &self,
        Parameters(ImageToVideoRequest {
            image_index,
            prompt,
            duration,
            model: _model,
        }): Parameters<ImageToVideoRequest>,
    ) -> String {
        execute_tool(self, "image_to_video", async {
            if let Some(d) = duration {
                if d <= 0.0 {
                    return Err(McpToolError::invalid_argument("duration must be positive"));
                }
            }
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            self.inference
                .image_to_video(&image_url, prompt.as_deref(), duration)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Image-to-video failed: {}", e)))
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
    ) -> String {
        execute_tool(self, "video_add_caption", async {
            validate_tool_url(&video_url)?;

            self.require_ffmpeg()?;

            let pos = position.as_deref().unwrap_or("bottom");
            let size = font_size.unwrap_or(24);
            if size == 0 {
                return Err(McpToolError::invalid_argument(
                    "font_size must be greater than 0",
                ));
            }

            let output = self
                .ffmpeg
                .add_caption(&video_url, &text, pos, size)
                .await
                .map_err(map_media_error)?;

            Ok(serde_json::json!({
                "status": "captioned",
                "source": video_url,
                "text": text,
                "position": pos,
                "font_size": size,
                "output": output.display().to_string(),
            }))
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
    ) -> String {
        execute_tool(self, "video_remix", async {
            validate_tool_url(&video_url)?;

            if start_sec >= end_sec {
                return Err(McpToolError::invalid_argument(
                    "start_sec must be less than end_sec.",
                ));
            }

            self.require_ffmpeg()?;

            let clipped = self
                .ffmpeg
                .clip(&video_url, start_sec, end_sec)
                .await
                .map_err(|e| McpToolError::internal(format!("Clip step failed: {}", e)))?;

            let captioned = if let Some(ref cap) = caption_text {
                self.ffmpeg
                    .add_caption(&clipped.to_string_lossy(), cap, "bottom", 24)
                    .await
                    .map_err(|e| McpToolError::internal(format!("Caption step failed: {}", e)))?
            } else {
                clipped.clone()
            };

            let gif_result = self
                .ffmpeg
                .to_gif(
                    &captioned.to_string_lossy(),
                    0.0,
                    end_sec - start_sec,
                    480,
                    10,
                )
                .await;

            // Always clean up temp files regardless of outcome
            let _ = std::fs::remove_file(&clipped);
            if caption_text.is_some() {
                let _ = std::fs::remove_file(&captioned);
            }

            let gif = gif_result
                .map_err(|e| McpToolError::internal(format!("GIF step failed: {}", e)))?;

            Ok(serde_json::json!({
                "status": "remixed",
                "source": video_url,
                "start_sec": start_sec,
                "end_sec": end_sec,
                "caption": caption_text,
                "output": gif.display().to_string(),
            }))
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
    ) -> String {
        execute_tool(self, "video_from_images", async {
            if image_indices.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "At least one image index is required.",
                ));
            }

            self.require_ffmpeg()?;

            let mut paths = Vec::new();
            for idx in &image_indices {
                paths.push(self.resolve_image_path(*idx).map_err(map_media_error)?);
            }

            let fps = fps.unwrap_or(24);
            let fmt = format.as_deref().unwrap_or("mp4");

            if fps == 0 {
                return Err(McpToolError::invalid_argument("fps must be greater than 0"));
            }

            let output = self
                .ffmpeg
                .images_to_video(&paths, fps, fmt)
                .await
                .map_err(map_media_error)?;

            Ok(serde_json::json!({
                "status": "created",
                "frame_count": paths.len(),
                "fps": fps,
                "format": fmt,
                "output": output.display().to_string(),
            }))
        })
        .await
    }

    #[tool(description = "Concatenate multiple video clips into one using ffmpeg.")]
    pub async fn video_concat(
        &self,
        Parameters(VideoConcatRequest { video_urls }): Parameters<VideoConcatRequest>,
    ) -> String {
        execute_tool(self, "video_concat", async {
            if video_urls.len() < 2 {
                return Err(McpToolError::invalid_argument(
                    "At least 2 video URLs are required.",
                ));
            }

            for url in &video_urls {
                validate_tool_url(url)?;
            }

            self.require_ffmpeg()?;

            let output = self
                .ffmpeg
                .concat(&video_urls)
                .await
                .map_err(map_media_error)?;

            Ok(serde_json::json!({
                "status": "concatenated",
                "clip_count": video_urls.len(),
                "output": output.display().to_string(),
            }))
        })
        .await
    }

    #[tool(
        description = "Generate a description of video content by extracting keyframes and analyzing them with a vision LLM."
    )]
    pub async fn video_caption(
        &self,
        Parameters(VideoCaptionRequest { video_url, style }): Parameters<VideoCaptionRequest>,
    ) -> String {
        execute_tool(self, "video_caption", async {
            validate_tool_url(&video_url)?;

            let style_str = style.as_deref().unwrap_or("descriptive");
            self.require_ffmpeg()?;

            let frames = self
                .ffmpeg
                .extract_keyframes(&video_url, 2.0, 10)
                .await
                .map_err(|e| {
                    McpToolError::internal(format!("Keyframe extraction failed: {}", e))
                })?;

            if frames.is_empty() {
                return Err(McpToolError::internal(
                    "No keyframes extracted from video.",
                ));
            }

            let mut image_urls = Vec::new();
            for frame in &frames {
                match std::fs::read(frame) {
                    Ok(data) => {
                        let b64 = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &data,
                        );
                        image_urls.push(format!("data:image/jpeg;base64,{}", b64));
                    }
                    Err(e) => {
                        tracing::warn!(target: "hkask.mcp.media", frame = %frame.display(), error = %e, "Failed to read keyframe");
                    }
                }
            }

            let mut vars = HashMap::new();
            vars.insert("style", style_str);
            let prompt = self
                .render_prompt("video_caption", &vars)
                .map_err(|e| McpToolError::internal(format!("Template render failed: {}", e)))?;

            let (vision_model, _vision_label) = self.require_vision().await?;
            let params = hkask_types::template::LLMParameters::default();
            let result = self
                .inference
                .generate_vision(&prompt, &image_urls, &params, Some(vision_model))
                .await;

            for frame in &frames {
                let _ = std::fs::remove_file(frame);
            }

            match result {
                Ok(r) => Ok(serde_json::json!({
                    "caption": r.text.trim(),
                    "style": style_str,
                    "frames_analyzed": image_urls.len(),
                })),
                Err(e) => Err(McpToolError::unavailable(format!(
                    "Vision inference failed: {}",
                    e
                ))),
            }
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
    ) -> String {
        execute_tool(self, "video_meme", async {
            let image_path = self
                .resolve_image_path(image_index)
                .map_err(map_media_error)?;

            let mut img =
                image::open(&image_path).map_err(|e| McpToolError::internal(format!("Failed to open image: {}", e)))?;

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
                    imageproc::drawing::draw_text_mut(&mut img, black, x + dx, y + dy, scale, &font, &text_upper);
                }
                imageproc::drawing::draw_text_mut(&mut img, white, x, y, scale, &font, &text_upper);
            }

            if let Some(ref text) = bottom_text {
                let text_upper: String = text.to_uppercase();
                let (tw, th) = measure_text(&font, scale, &text_upper);
                let x = ((img_w as i32 - tw as i32) / 2).max(0);
                let y = (img_h as i32 - th as i32 - (img_h as f32 * 0.05) as i32).max(0);
                for &(dx, dy) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    imageproc::drawing::draw_text_mut(&mut img, black, x + dx, y + dy, scale, &font, &text_upper);
                }
                imageproc::drawing::draw_text_mut(&mut img, white, x, y, scale, &font, &text_upper);
            }

            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png)
                .map_err(|e| McpToolError::internal(format!("Failed to encode composited image: {}", e)))?;
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, buf.get_ref());
            let data_uri = format!("data:image/png;base64,{}", b64);

            let motion_prompt = if motion.is_empty() {
                "slow zoom in".to_string()
            } else {
                motion.clone()
            };
            self.inference
                .image_to_video(&data_uri, Some(&motion_prompt), duration)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Image-to-video failed: {}", e)))
        })
        .await
    }
}
