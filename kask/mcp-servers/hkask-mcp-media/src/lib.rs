#![forbid(unsafe_code)]
//! hKask MCP Media — AI media generation (image, video, voice via centralized inference router)
//!
//! Tool families:
//! - Gallery: organize, search, status
//! - Image: describe, remove_background, apply_style, create_collage
//! - Video: clip, to_gif, image_to_video, add_caption, remix, concat, from_images
//! - Generation: generate_image, transform_image, upscale_image, generate_video
//! - Voice: voice_design, generate_speech
//! - Audio: transcribe, transcribe_bundle, audio_capture, record_and_transcribe

// Pre-existing clippy lints from original bin-only codebase (addressed in separate refactoring pass).
#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only
#![allow(clippy::collapsible_if, clippy::cloned_ref_to_slice_refs)]

pub mod omc;

mod error;
mod gallery;
mod templates;
pub mod video;

pub use error::{MediaError, map_media_error};

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use gallery::GalleryState;
use gallery::vision::{self};
use hkask_inference::InferenceRouter;
use hkask_mcp_server::DaemonClient;
use hkask_mcp_server::server::{McpToolError, execute_tool, validate_tool_url};
use hkask_pods::VoiceDesign;
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::database::value::DbValue;
use hkask_storage::{Database, GalleryMode, GalleryStore, GalleryStoreError};
use hkask_types::InferencePort;

use hkask_types::{TimedWord, TranscriptBundle, TranscriptSegment};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
pub mod tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use video::FfmpegRunner;

use ab_glyph::Font;
use sha2::Digest;

// ── Model configuration ───────────────────────────────────────────────

/// Default open-weight models for media processing.
/// All can be overridden via environment variables.
pub mod models {
    /// Default TTS model: Qwen3-TTS (Apache 2.0) via fal.ai
    pub const TTS_DEFAULT: &str = "FA/qwen-3-tts";
    pub const TTS_ENV: &str = "HKASK_MEDIA_TTS_MODEL";

    /// Default STT model: fal.ai Wizper (optimized Whisper v3)
    pub const STT_DEFAULT: &str = "FA/wizper";
    pub const STT_ENV: &str = "HKASK_MEDIA_STT_MODEL";

    /// Default vision model: Qwen3-VL (Apache 2.0) via KiloCode
    pub const VISION_DEFAULT: &str = "KC/qwen/qwen3-vl-235b-a22b-instruct";
    pub const VISION_ENV: &str = "HKASK_MEDIA_VISION_MODEL";

    /// Default image generation model: FLUX.2 \[dev\] (open-source) via fal.ai
    pub const IMAGE_GEN_DEFAULT: &str = "FA/flux-2";
    pub const IMAGE_GEN_ENV: &str = "HKASK_MEDIA_IMAGE_GEN_MODEL";

    /// Resolve a model name from env var or default.
    pub fn resolve(env_key: &str, default: &str) -> String {
        std::env::var(env_key).unwrap_or_else(|_| default.to_string())
    }

    pub fn tts_model() -> String {
        resolve(TTS_ENV, TTS_DEFAULT)
    }
    pub fn stt_model() -> String {
        resolve(STT_ENV, STT_DEFAULT)
    }
    pub fn vision_model() -> String {
        resolve(VISION_ENV, VISION_DEFAULT)
    }
    pub fn image_gen_model() -> String {
        resolve(IMAGE_GEN_ENV, IMAGE_GEN_DEFAULT)
    }
}

/// Lock-free snapshot of gallery state — safe to hold across .await points.
struct GalleryAccess {
    gallery_id: String,
    image_count: u64,
    root_path: PathBuf,
}

hkask_mcp_server::mcp_server!(
    pub struct MediaServer {
        pub inference: Arc<InferenceRouter>,
        pub gallery_state: Arc<Mutex<Option<GalleryState>>>,
        pub gallery_store: Arc<GalleryStore>,
        pub template_env: minijinja::Environment<'static>,
        pub ffmpeg: FfmpegRunner,
    }
);

pub mod types;
use types::*;

/// Compute normalized Levenshtein similarity between two strings.
/// Returns 1.0 for identical strings, 0.0 for completely different.
fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 && b_len == 0 {
        return 1.0;
    }
    if a_len == 0 || b_len == 0 {
        return 0.0;
    }

    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_chars: Vec<char> = a_lower.chars().collect();
    let b_chars: Vec<char> = b_lower.chars().collect();

    // Space-optimized DP: only keep two rows
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[b_len];
    let max_len = a_len.max(b_len) as f64;
    1.0 - (distance as f64 / max_len)
}

#[cfg(test)]
mod levenshtein_tests {
    use super::*;

    #[test]
    fn identical_strings() {
        assert!((levenshtein_similarity("sunset", "sunset") - 1.0).abs() < 0.001);
    }

    #[test]
    fn completely_different() {
        let sim = levenshtein_similarity("sunset", "xyzzy");
        assert!(sim < 0.3, "expected low similarity, got {}", sim);
    }

    #[test]
    fn case_insensitive() {
        assert!((levenshtein_similarity("Sunset", "sunset") - 1.0).abs() < 0.001);
    }

    #[test]
    fn typo_tolerant() {
        let sim = levenshtein_similarity("sunset", "sunest");
        assert!(sim > 0.6, "expected high similarity for typo, got {}", sim);
    }

    #[test]
    fn empty_strings() {
        assert!((levenshtein_similarity("", "") - 1.0).abs() < 0.001);
        assert!((levenshtein_similarity("sunset", "") - 0.0).abs() < 0.001);
        assert!((levenshtein_similarity("", "sunset") - 0.0).abs() < 0.001);
    }
}

impl MediaServer {
    /// Lock the gallery and extract essential state. Drops the lock before
    /// returning, so the result is safe to hold across .await points.
    fn access_gallery(&self) -> Result<GalleryAccess, MediaError> {
        let guard = self
            .gallery_state
            .lock()
            .map_err(|e| MediaError::Io(format!("Gallery state lock error: {}", e)))?;
        let state = guard.as_ref().ok_or(MediaError::GalleryNotInitialized)?;
        let access = GalleryAccess {
            gallery_id: state
                .gallery_id
                .clone()
                .ok_or(MediaError::GalleryNotInitialized)?,
            image_count: state.image_count,
            root_path: state.path.clone(),
        };
        Ok(access)
    }

    /// Return the ffmpeg runner or an error if ffmpeg is not installed.
    fn require_ffmpeg(&self) -> Result<&FfmpegRunner, McpToolError> {
        if self.ffmpeg.available {
            Ok(&self.ffmpeg)
        } else {
            Err(McpToolError::unavailable(
                "ffmpeg not found on system PATH — video tools unavailable.",
            ))
        }
    }

    /// Return the best available vision model or an error if none is configured.
    async fn require_vision(&self) -> Result<(&'static str, &'static str), McpToolError> {
        self.resolve_vision_model().await.ok_or_else(|| {
            McpToolError::unavailable(
                "No vision-capable provider configured (set DI_API_KEY, OR_API_KEY, or TG_API_KEY)",
            )
        })
    }

    /// Render a Jinja2 prompt template with the given variables.
    fn render_prompt(&self, name: &str, vars: &HashMap<&str, &str>) -> Result<String, MediaError> {
        templates::render(&self.template_env, name, vars)
    }

    /// Resolve an image index to a base64 data URL for vision LLM calls.
    fn resolve_image_url(&self, image_index: usize) -> Result<String, MediaError> {
        let ga = self.access_gallery()?;

        let img = self
            .gallery_store
            .get_image(&ga.gallery_id, Some(image_index), None)
            .map_err(|e| {
                MediaError::ImageNotFound(format!(
                    "Image not found at index {}: {}",
                    image_index, e
                ))
            })?;

        let data = std::fs::read(&img.absolute_path)
            .map_err(|e| MediaError::Io(format!("Failed to read image: {}", e)))?;
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
        let mime = match img.format.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tiff" => "image/tiff",
            _ => "image/png",
        };
        Ok(format!("data:{};base64,{}", mime, b64))
    }

    /// Resolve an image index to a filesystem path.
    fn resolve_image_path(&self, image_index: usize) -> Result<PathBuf, MediaError> {
        let ga = self.access_gallery()?;

        let img = self
            .gallery_store
            .get_image(&ga.gallery_id, Some(image_index), None)
            .map_err(|e| {
                MediaError::ImageNotFound(format!(
                    "Image not found at index {}: {}",
                    image_index, e
                ))
            })?;

        Ok(PathBuf::from(&img.absolute_path))
    }

    /// Resolve an image index to its SQLite image ID for tag persistence.
    fn resolve_image_id(&self, image_index: usize) -> Result<String, MediaError> {
        let ga = self.access_gallery()?;

        let img = self
            .gallery_store
            .get_image(&ga.gallery_id, Some(image_index), None)
            .map_err(|e| {
                MediaError::ImageNotFound(format!(
                    "Image not found at index {}: {}",
                    image_index, e
                ))
            })?;

        Ok(img.id)
    }

    /// Resolve an image ID directly to a base64 data URL.
    ///
    /// Used by face matching where we have image IDs from tags/registry,
    /// not gallery indices.
    fn resolve_image_url_by_id(&self, image_id: &str) -> Result<String, MediaError> {
        let ga = self.access_gallery()?;

        // Look up the image's absolute path by its SQLite ID
        let rows = self
            .gallery_store
            .driver()
            .query(
                "SELECT absolute_path FROM gallery_images WHERE id = ?1 AND gallery_id = ?2",
                &[
                    DbValue::Text(image_id.to_string()),
                    DbValue::Text(ga.gallery_id.to_string()),
                ],
            )
            .map_err(|e| {
                MediaError::ImageNotFound(format!("Image not found by ID {}: {}", image_id, e))
            })?;
        let absolute_path: String = rows
            .first()
            .and_then(|r| r.get_str(0).ok())
            .ok_or_else(|| {
                MediaError::ImageNotFound(format!("Image not found by ID {}", image_id))
            })?
            .to_string();

        let data = std::fs::read(&absolute_path)
            .map_err(|e| MediaError::Io(format!("Failed to read image: {}", e)))?;
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
        let mime = if absolute_path.ends_with(".png") {
            "image/png"
        } else if absolute_path.ends_with(".jpg") || absolute_path.ends_with(".jpeg") {
            "image/jpeg"
        } else if absolute_path.ends_with(".webp") {
            "image/webp"
        } else if absolute_path.ends_with(".gif") {
            "image/gif"
        } else {
            "image/png"
        };
        Ok(format!("data:{};base64,{}", mime, b64))
    }

    /// Persist a single tag to the gallery store (best-effort, logs errors).
    fn persist_tag(
        &self,
        image_id: &str,
        tag_type: &str,
        value: &str,
        confidence: f64,
        model: &str,
    ) {
        match self
            .gallery_store
            .tag_image(image_id, tag_type, value, confidence, model)
        {
            Ok(_) => {
                tracing::debug!(target: "hkask.mcp.media.tags", image_id = %image_id, tag_type = %tag_type, value = %value, "Tag persisted")
            }
            Err(e) => {
                tracing::warn!(target: "hkask.mcp.media.tags", image_id = %image_id, tag_type = %tag_type, error = %e, "Failed to persist tag")
            }
        }
    }

    /// Shared face-registration logic used by both the `face_register` MCP tool
    /// (which takes a gallery image index) and `face_scan_folder` (which
    /// imports a reference image from a folder, then registers it).
    ///
    /// Validates the image via the `validate_face_ref` vision LLM template
    /// (unless `force` is set), then inserts a row into `face_registry`.
    /// Returns `(FaceRegistryRecord, Option<FaceValidationResult>)`.
    async fn register_face_from_url(
        &self,
        image_id: &str,
        image_url: &str,
        first_name: &str,
        last_name: &str,
        user_notes: &str,
        force: bool,
    ) -> Result<
        (
            hkask_storage::FaceRegistryRecord,
            Option<gallery::vision::FaceValidationResult>,
        ),
        McpToolError,
    > {
        let (status, notes, validation) = if force {
            (FaceStatus::Valid, user_notes.to_string(), None)
        } else {
            let (vision_model, _vision_label) = self.require_vision().await?;
            let v = gallery::vision::validate_face_reference(
                &self.inference,
                &self.template_env,
                image_url,
                Some(vision_model),
            )
            .await
            .map_err(|e| McpToolError::internal(format!("Face validation failed: {}", e)))?;
            let status = if v.valid {
                FaceStatus::Valid
            } else {
                FaceStatus::Rejected
            };
            let notes = if v.valid {
                user_notes.to_string()
            } else if user_notes.is_empty() {
                v.issues.join("; ")
            } else {
                format!("{}; {}", user_notes, v.issues.join("; "))
            };
            (status, notes, Some(v))
        };

        // Produce a 512-dim face embedding via the `embed_face` vision LLM
        // template. Stored as raw f32-le bytes in the `embedding` BLOB column
        // for fast cosine-similarity matching during gallery_refresh. Falls
        // back to None (LLM-only matching) if embedding extraction fails.
        let embedding_blob: Option<Vec<u8>> = if status.is_valid() {
            match self.require_vision().await {
                Ok((vision_model, _)) => {
                    match gallery::vision::embed_face(
                        &self.inference,
                        &self.template_env,
                        image_url,
                        Some(vision_model),
                    )
                    .await
                    {
                        Ok(result) => Some(embedding_to_blob(&result.embedding)),
                        Err(e) => {
                            tracing::warn!(
                                target: "hkask.mcp.media.face",
                                error = %e,
                                "Face embedding extraction failed — will use LLM-only matching"
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.media.face",
                        error = %e,
                        "No vision model available for embedding — will use LLM-only matching"
                    );
                    None
                }
            }
        } else {
            None
        };

        let record = self
            .gallery_store
            .register_face(
                first_name,
                last_name,
                image_id,
                embedding_blob.as_deref(),
                status.as_ref(),
                &notes,
            )
            .map_err(|e| McpToolError::internal(format!("Failed to register face: {}", e)))?;
        Ok((record, validation))
    }

    /// Import a reference image file into the current gallery (idempotent by
    /// SHA-256 hash) and return its gallery `image_id` plus a base64 data URL
    /// suitable for vision LLM calls. Used by `face_scan_folder`.
    ///
    /// If the image is already in the gallery (matched by hash), the existing
    /// record is reused — no duplicate row is inserted. The file is read from
    /// disk exactly once; the base64 URL is computed from the in-memory bytes.
    fn import_reference_image(
        &self,
        abs_path: &std::path::Path,
    ) -> Result<(String, String), MediaError> {
        let ga = self.access_gallery()?;

        let data = std::fs::read(abs_path)
            .map_err(|e| MediaError::Io(format!("Failed to read {}: {}", abs_path.display(), e)))?;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&data);
        let hash = format!("{:x}", hasher.finalize());

        // Compute the base64 data URL once from the in-memory bytes — avoids
        // re-reading the file from disk in resolve_image_url_by_id.
        let ext = abs_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            _ => "image/png",
        };
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
        let image_url = format!("data:{};base64,{}", mime, b64);

        // Reuse existing record if present (idempotent).
        if let Ok(existing) = self
            .gallery_store
            .get_image(&ga.gallery_id, None, Some(&hash))
        {
            return Ok((existing.id, image_url));
        }

        let (width, height) = image::image_dimensions(abs_path)
            .map_err(|e| MediaError::Io(format!("Failed to read dimensions: {}", e)))?;
        let relative_path = abs_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| abs_path.to_string_lossy().to_string());
        let size_bytes = data.len() as u64;

        let record = self
            .gallery_store
            .add_image(
                &ga.gallery_id,
                &relative_path,
                &abs_path.to_string_lossy(),
                &hash,
                width,
                height,
                &ext,
                size_bytes,
            )
            .map_err(|e| {
                MediaError::ImageNotFound(format!("Failed to import reference image: {}", e))
            })?;

        Ok((record.id, image_url))
    }

    /// Run face matching: for each `face` tag in the gallery, compare against
    /// every entry in the face registry. Prefers cosine similarity on stored
    /// embeddings (fast, local — produced by the `embed_face` template at
    /// registration time). Falls back to the `match_faces` vision LLM template
    /// when embeddings are missing or the cosine score is in the uncertain
    /// band [0.3, 0.5). On a match (confidence ≥ 0.5 for embeddings, 0.7 for
    /// LLM), persist a `face` tag with the person's name and registry_id.
    /// Returns `(faces_matched, errors)`.
    ///
    /// This is the composable face-matching stage called by `gallery_refresh`
    /// when `include_faces=true`.
    async fn run_face_matching(
        &self,
        ga: &GalleryAccess,
        registry: &[hkask_storage::FaceRegistryRecord],
    ) -> (u32, Vec<String>) {
        let mut faces_matched = 0u32;
        let mut errors = Vec::new();

        let all_tags = match self.gallery_store.get_all_tags(&ga.gallery_id) {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("Failed to query tags: {}", e));
                return (0, errors);
            }
        };

        let vision_model = match self.resolve_vision_model().await {
            Some((m, _label)) => m,
            None => {
                errors.push("Face matching skipped: no vision model available".to_string());
                return (0, errors);
            }
        };

        // Pre-decode registry embeddings once (avoid re-parsing the BLOB for
        // every face tag).
        let registry_embeddings: Vec<(usize, Vec<f32>)> = registry
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.embedding
                    .as_ref()
                    .and_then(|b| blob_to_embedding(b))
                    .map(|e| (i, e))
            })
            .collect();

        for (tag, _path) in &all_tags {
            if tag.tag_type != "face" {
                continue;
            }

            let face_image_id = &tag.image_id;

            // Parse the tag value once — used for both bbox extraction and
            // face_index preservation on match.
            let parsed: Option<serde_json::Value> = serde_json::from_str(&tag.value).ok();
            let face_bbox = parsed.as_ref().and_then(|v| v.get("bbox").cloned());

            let query_url = if let Some(ref bbox) = face_bbox {
                match self.crop_face_region(face_image_id, bbox) {
                    Ok(cropped_url) => cropped_url,
                    Err(_) => match self.resolve_image_url_by_id(face_image_id) {
                        Ok(url) => url,
                        Err(e) => {
                            errors.push(format!("Face tag {}: {}", tag.id, e));
                            continue;
                        }
                    },
                }
            } else {
                match self.resolve_image_url_by_id(face_image_id) {
                    Ok(url) => url,
                    Err(e) => {
                        errors.push(format!("Face tag {}: {}", tag.id, e));
                        continue;
                    }
                }
            };

            // Produce a query embedding once per face tag. Used for the fast
            // cosine path; falls back to LLM-only if extraction fails.
            let query_embedding: Option<Vec<f32>> = match gallery::vision::embed_face(
                &self.inference,
                &self.template_env,
                &query_url,
                Some(vision_model),
            )
            .await
            {
                Ok(result) => Some(result.embedding),
                Err(e) => {
                    tracing::warn!(
                        target: "hkask.mcp.media.face",
                        error = %e,
                        "Query embedding extraction failed — falling back to LLM-only matching"
                    );
                    None
                }
            };

            for (reg_idx, reg_entry) in registry.iter().enumerate() {
                // ── Fast path: cosine similarity on stored embeddings ──
                let cosine_match: Option<(f32, &str)> = (|| {
                    let q = query_embedding.as_ref()?;
                    let r = registry_embeddings
                        .iter()
                        .find(|(i, _)| *i == reg_idx)
                        .map(|(_, e)| e)?;
                    let sim = cosine_similarity(q, r);
                    if sim >= 0.5 {
                        Some((sim, "embedding_cosine"))
                    } else if sim < 0.3 {
                        // Confident non-match — skip the LLM call entirely.
                        Some((sim, "embedding_cosine_reject"))
                    } else {
                        // Uncertain band — fall through to LLM.
                        None
                    }
                })();

                if let Some((confidence, method)) = cosine_match {
                    if method == "embedding_cosine_reject" {
                        continue; // confident non-match, try next registry entry
                    }
                    // Embedding match — persist and move to next face tag.
                    let name = format!("{} {}", reg_entry.first_name, reg_entry.last_name);
                    let face_index = parsed.as_ref().and_then(|v| v["face_index"].as_u64());
                    let new_value = serde_json::json!({
                        "face_index": face_index,
                        "name": name,
                        "match_confidence": confidence,
                        "registry_id": reg_entry.id,
                        "method": method,
                    });
                    self.persist_tag(
                        &tag.image_id,
                        "face",
                        &new_value.to_string(),
                        confidence as f64,
                        vision_model,
                    );
                    faces_matched += 1;
                    break;
                }

                // ── Slow path: vision LLM `match_faces` template ──
                let ref_url = match self.resolve_image_url_by_id(&reg_entry.image_id) {
                    Ok(url) => url,
                    Err(e) => {
                        errors.push(format!("Registry entry {}: {}", reg_entry.id, e));
                        continue;
                    }
                };

                match gallery::vision::match_faces(
                    &self.inference,
                    &self.template_env,
                    &ref_url,
                    &query_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(result) => {
                        if result.is_match && result.confidence >= 0.7 {
                            let name = format!("{} {}", reg_entry.first_name, reg_entry.last_name);
                            let face_index = parsed.as_ref().and_then(|v| v["face_index"].as_u64());
                            let new_value = serde_json::json!({
                                "face_index": face_index,
                                "name": name,
                                "match_confidence": result.confidence,
                                "registry_id": reg_entry.id,
                                "method": "vision_llm",
                            });
                            self.persist_tag(
                                &tag.image_id,
                                "face",
                                &new_value.to_string(),
                                result.confidence,
                                vision_model,
                            );
                            faces_matched += 1;
                            break;
                        }
                    }
                    Err(e) => {
                        errors.push(format!("Match {} vs {}: {}", reg_entry.id, tag.id, e));
                    }
                }
            }
        }

        (faces_matched, errors)
    }

    /// Process a single reference face image: locate sidecar, parse, import,
    /// validate, embed, and register. Returns `Ok(json)` on success (the JSON
    /// summary for this one face), or `Err(message)` on failure (the error
    /// string to push into the scan's error list).
    ///
    /// Extracted from `run_face_scan_folder` for testability — this is the
    /// per-file unit of work.
    async fn register_one_face(
        &self,
        path: &std::path::Path,
        ext: &str,
        force: bool,
    ) -> Result<serde_json::Value, MediaError> {
        let fname = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let sidecar = path.with_extension(format!("{}.yaml", ext));
        let sidecar = if sidecar.is_file() {
            sidecar
        } else {
            let alt = path.with_extension(format!("{}.yml", ext));
            if alt.is_file() {
                alt
            } else {
                return Err(MediaError::SidecarNotFound(fname));
            }
        };

        let sidecar_text = std::fs::read_to_string(&sidecar)
            .map_err(|e| MediaError::Io(format!("{}: failed to read sidecar: {}", fname, e)))?;

        let parsed: FaceSidecar = serde_yaml_neo::from_str(&sidecar_text).map_err(|e| {
            MediaError::SidecarInvalid(format!(
                "{}: invalid sidecar YAML: {}",
                sidecar.display(),
                e
            ))
        })?;

        let (image_id, image_url) = self.import_reference_image(path).map_err(|e| {
            MediaError::FaceRegistration(format!("{}: import failed: {}", fname, e))
        })?;

        let (record, validation) = self
            .register_face_from_url(
                &image_id,
                &image_url,
                &parsed.first_name,
                &parsed.last_name,
                &parsed.notes,
                force,
            )
            .await
            .map_err(|e| {
                MediaError::FaceRegistration(format!("{}: registration failed: {}", fname, e))
            })?;

        Ok(serde_json::json!({
            "face_id": record.id,
            "first_name": record.first_name,
            "last_name": record.last_name,
            "status": record.status,
            "notes": record.notes,
            "validation": validation,
            "source": path.file_name().unwrap_or_default().to_string_lossy(),
        }))
    }

    /// Internal face-folder scan logic shared by the `face_scan_folder` MCP
    /// tool and the `gallery_refresh` orchestrator. Walks `folder` for image
    /// files with `.yaml` sidecars, imports each image into the gallery
    /// (idempotent by hash), validates via the vision LLM (unless `force`),
    /// and registers in `face_registry`. Returns a JSON summary.
    async fn run_face_scan_folder(
        &self,
        folder: &std::path::Path,
        force: bool,
    ) -> Result<serde_json::Value, McpToolError> {
        const IMG_EXTS: &[&str] = crate::IMAGE_EXTENSIONS;

        let mut scanned = 0u32;
        let mut registered = 0u32;
        let mut skipped = 0u32;
        let mut rejected = 0u32;
        let mut errors: Vec<String> = Vec::new();
        let mut registered_faces: Vec<serde_json::Value> = Vec::new();

        for entry in walkdir::WalkDir::new(folder)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            if !IMG_EXTS.contains(&ext.as_str()) {
                continue;
            }

            scanned += 1;

            match self.register_one_face(path, &ext, force).await {
                Ok(face_json) => {
                    if face_json["status"] == FaceStatus::Valid.as_ref() {
                        registered += 1;
                    } else {
                        rejected += 1;
                    }
                    registered_faces.push(face_json);
                }
                Err(e) => {
                    if matches!(e, MediaError::SidecarNotFound(_)) {
                        skipped += 1;
                    }
                    errors.push(e.to_string());
                }
            }
        }

        Ok(serde_json::json!({
            "folder": folder.to_string_lossy(),
            "scanned": scanned,
            "registered": registered,
            "skipped": skipped,
            "rejected": rejected,
            "faces": registered_faces,
            "errors": errors,
        }))
    }

    /// Crop a face region from an image using bounding box percentages.
    ///
    /// Returns a base64 data URL of the cropped face region, or the original
    /// image URL if cropping fails (graceful degradation).
    fn crop_face_region(
        &self,
        image_id: &str,
        bbox: &serde_json::Value,
    ) -> Result<String, MediaError> {
        let ga = self.access_gallery()?;

        let rows = self
            .gallery_store
            .driver()
            .query(
                "SELECT absolute_path FROM gallery_images WHERE id = ?1 AND gallery_id = ?2",
                &[
                    DbValue::Text(image_id.to_string()),
                    DbValue::Text(ga.gallery_id.to_string()),
                ],
            )
            .map_err(|e| MediaError::ImageNotFound(format!("Image not found: {}", e)))?;
        let absolute_path: String = rows
            .first()
            .and_then(|r| r.get_str(0).ok())
            .ok_or(MediaError::ImageNotFound("Image not found".to_string()))?
            .to_string();

        // Read and crop the image
        let img = image::open(&absolute_path)
            .map_err(|e| MediaError::Io(format!("Failed to open image: {}", e)))?;

        let x_pct = bbox["x_pct"].as_f64().unwrap_or(0.0);
        let y_pct = bbox["y_pct"].as_f64().unwrap_or(0.0);
        let w_pct = bbox["w_pct"].as_f64().unwrap_or(100.0);
        let h_pct = bbox["h_pct"].as_f64().unwrap_or(100.0);

        let (img_w, img_h) = (img.width(), img.height());
        let x = ((x_pct / 100.0) * img_w as f64).round() as u32;
        let y = ((y_pct / 100.0) * img_h as f64).round() as u32;
        let w = ((w_pct / 100.0) * img_w as f64).round() as u32;
        let h = ((h_pct / 100.0) * img_h as f64).round() as u32;

        // Clamp to image bounds
        let x = x.min(img_w.saturating_sub(1));
        let y = y.min(img_h.saturating_sub(1));
        let w = w.min(img_w - x).max(1);
        let h = h.min(img_h - y).max(1);

        let cropped = img.crop_imm(x, y, w, h);

        // Encode as base64 data URL
        let mut buf = std::io::Cursor::new(Vec::new());
        cropped
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .map_err(|e| MediaError::Io(format!("Failed to encode cropped image: {}", e)))?;
        let data = buf.into_inner();
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
        Ok(format!("data:image/jpeg;base64,{}", b64))
    }

    /// Resolve the best available vision model with fallback chain.
    /// Tries: fal.ai → DeepInfra → OpenRouter → Together AI.
    /// Returns (model_name, label) or None if no vision provider is configured.
    async fn resolve_vision_model(&self) -> Option<(&'static str, &'static str)> {
        let models = self.inference.list_vision_models().await;

        for model in &models {
            match model.provider {
                hkask_inference::ProviderId::Fal => {
                    // Qwen2.5-VL 72B — Apache 2.0 open-weight, served by fal.ai
                    return Some(("FA/Qwen/Qwen2.5-VL-72B-Instruct", "qwen2.5-vl-72b"));
                }
                hkask_inference::ProviderId::DeepInfra => {
                    return Some((
                        "DI/meta-llama/Llama-3.2-11B-Vision-Instruct",
                        "llama-3.2-vision",
                    ));
                }
                hkask_inference::ProviderId::OpenRouter => {
                    return Some(("OR/qwen/qwen-2.5-vl-72b-instruct", "qwen2.5-vl-72b"));
                }
                hkask_inference::ProviderId::Together => {
                    return Some(("TG/Qwen/Qwen2.5-VL-72B-Instruct", "qwen-vl"));
                }
                _ => continue,
            }
        }

        None
    }

    /// Re-scan an existing gallery and persist new images.
    /// Returns (gallery_id, old_image_count, images_added, total_images, persisted_count).
    /// The MutexGuard is dropped before return so callers can safely await.
    fn rescan_existing_gallery(
        &self,
        recursive: bool,
    ) -> Result<(String, u64, u32, u32, u32), MediaError> {
        // Hold the lock for the entire scan→persist operation to prevent lost-update
        // races under concurrent calls. All operations inside are synchronous I/O
        // (std::fs + GalleryStore), so holding std::sync::Mutex is safe.
        let mut guard = self
            .gallery_state
            .lock()
            .map_err(|e| MediaError::Io(format!("Gallery state lock error: {}", e)))?;
        let state = guard.as_mut().ok_or(MediaError::GalleryNotInitialized)?;

        let gallery_id = state
            .gallery_id
            .clone()
            .ok_or(MediaError::GalleryNotInitialized)?;
        let old_count = state.image_count;

        let scan_result = state.scan(recursive, None);
        let mut persisted = 0u32;
        for entry in &scan_result.entries {
            let abs_path = state.path.join(&entry.relative_path);
            if self
                .gallery_store
                .add_image(
                    &gallery_id,
                    &entry.relative_path,
                    &abs_path.to_string_lossy(),
                    &entry.checksum,
                    entry.width,
                    entry.height,
                    &entry.format,
                    entry.size_bytes,
                )
                .is_ok()
            {
                persisted += 1;
            }
        }

        Ok((
            gallery_id,
            old_count,
            scan_result.added,
            scan_result.total,
            persisted,
        ))
    }

    /// Run the analysis pipeline on a subset of gallery images.
    /// Used internally by gallery_organize auto_analyze and gallery_analyze.
    /// Returns (analyzed_count, error_messages).
    async fn run_analysis_on_indices(
        &self,
        indices: &[usize],
        pipelines: &[String],
    ) -> (u32, Vec<String>) {
        let (vision_model, vision_label) = match self.resolve_vision_model().await {
            Some(v) => v,
            None => return (0, vec!["No vision model available — configure a vision-capable provider (DeepInfra, OpenRouter, or Together AI)".to_string()]),
        };
        let mut analyzed = 0u32;
        let mut errors = Vec::new();

        let run_faces = pipelines.iter().any(|p| p == "faces");
        let run_objects = pipelines.iter().any(|p| p == "objects");
        let run_colors = pipelines.iter().any(|p| p == "colors");
        let run_composition = pipelines.iter().any(|p| p == "composition");
        let run_scene = pipelines.iter().any(|p| p == "scene");

        for idx in indices {
            let image_url = match self.resolve_image_url(*idx) {
                Ok(url) => url,
                Err(e) => {
                    errors.push(format!("image {}: {}", idx, e));
                    continue;
                }
            };
            let image_id = match self.resolve_image_id(*idx) {
                Ok(id) => id,
                Err(e) => {
                    errors.push(format!("image {}: {}", idx, e));
                    continue;
                }
            };

            if run_faces {
                match vision::detect_faces(
                    &self.inference,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(faces) => {
                        for face in &faces {
                            let value = serde_json::to_string(face).unwrap_or_default();
                            self.persist_tag(&image_id, "face", &value, 0.85, vision_label);
                        }
                    }
                    Err(e) => {
                        errors.push(format!("image {} face detection: {}", idx, e));
                    }
                }
            }

            if run_objects {
                match vision::detect_objects(
                    &self.inference,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(objects) => {
                        for obj in &objects {
                            let value = serde_json::to_string(obj).unwrap_or_default();
                            self.persist_tag(&image_id, "object", &value, 0.85, vision_label);
                        }
                    }
                    Err(e) => {
                        errors.push(format!("image {} object detection: {}", idx, e));
                    }
                }
            }

            if run_colors {
                match vision::analyze_colors(
                    &self.inference,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(parsed) => {
                        if let Some(colors) = parsed["colors"].as_array() {
                            for color in colors {
                                let value = serde_json::to_string(color).unwrap_or_default();
                                self.persist_tag(&image_id, "color", &value, 0.85, vision_label);
                            }
                        }
                        for field in &["palette_style", "temperature", "saturation"] {
                            if let Some(v) = parsed.get(*field).and_then(|v| v.as_str()) {
                                self.persist_tag(&image_id, "color", v, 0.9, vision_label);
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("image {} color analysis: {}", idx, e));
                    }
                }
            }

            if run_composition {
                match vision::analyze_composition(
                    &self.inference,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(parsed) => {
                        for field in &[
                            "focal_point",
                            "rule_of_thirds",
                            "leading_lines",
                            "depth_of_field",
                            "perspective",
                            "framing",
                            "symmetry",
                            "negative_space",
                        ] {
                            if let Some(v) = parsed.get(*field).and_then(|v| v.as_str()) {
                                self.persist_tag(&image_id, "composition", v, 0.85, vision_label);
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("image {} composition analysis: {}", idx, e));
                    }
                }
            }

            if run_scene {
                match vision::caption_scene(
                    &self.inference,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(caption) => {
                        self.persist_tag(&image_id, "caption", &caption, 0.9, vision_label);
                    }
                    Err(e) => {
                        errors.push(format!("image {} scene caption: {}", idx, e));
                    }
                }
            }

            analyzed += 1;
        }

        (analyzed, errors)
    }

    /// Extract EXIF metadata from an image file.
    /// Returns key fields as a JSON object, or null if EXIF is unavailable.
    fn extract_exif(path: &str) -> serde_json::Value {
        let exif = match nom_exif::read_exif(path) {
            Ok(e) => e,
            Err(_) => return serde_json::Value::Null,
        };

        let mut fields = serde_json::Map::new();

        // Map common EXIF tag codes to human-readable names
        let tag_map: &[(u16, &str)] = &[
            (0x010F, "camera_make"),   // Make
            (0x0110, "camera_model"),  // Model
            (0x9003, "date_taken"),    // DateTimeOriginal
            (0x829A, "exposure_time"), // ExposureTime
            (0x829D, "f_number"),      // FNumber
            (0x8827, "iso"),           // ISOSpeedRatings
            (0x920A, "focal_length"),  // FocalLength
            (0x9209, "flash"),         // Flash
            (0x010E, "description"),   // ImageDescription
            (0x013B, "artist"),        // Artist
            (0x8298, "copyright"),     // Copyright
            (0x0131, "software"),      // Software
        ];

        for (code, name) in tag_map {
            if let Some(entry) = exif.get_by_code(nom_exif::IfdIndex::MAIN, *code) {
                if let Some(value_str) = entry.as_str() {
                    fields.insert(
                        name.to_string(),
                        serde_json::Value::String(value_str.to_string()),
                    );
                }
            }
        }

        // GPS info
        if let Some(gps) = exif.gps_info() {
            fields.insert(
                "gps".to_string(),
                serde_json::Value::String(gps.to_iso6709()),
            );
        }

        if fields.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::Object(fields)
        }
    }
}

/// Load a font for meme text rendering. Tries the provided path first,
/// then common system paths, then returns an error with guidance.
fn load_meme_font(font_path: Option<&str>) -> Result<ab_glyph::FontVec, MediaError> {
    if let Some(path) = font_path {
        // Reject path traversal attempts — font_path must be a simple filename
        if path.contains('/') || path.contains('\\') || path.contains("..") {
            return Err(MediaError::Io(format!(
                "font_path must be a simple filename, not a path: '{}'",
                path
            )));
        }
        let data = std::fs::read(path)
            .map_err(|e| MediaError::Io(format!("Cannot read font at '{}': {}", path, e)))?;
        return ab_glyph::FontVec::try_from_vec(data)
            .map_err(|e| MediaError::Io(format!("Invalid font file at '{}': {:?}", path, e)));
    }

    // Try common system paths
    let candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    ];

    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            if let Ok(font) = ab_glyph::FontVec::try_from_vec(data) {
                return Ok(font);
            }
        }
    }

    Err(MediaError::Io("No system font found".to_string()))
}

/// Measure rendered text dimensions for centering.
fn measure_text(font: &ab_glyph::FontVec, scale: ab_glyph::PxScale, text: &str) -> (u32, u32) {
    let mut total_width = 0.0f32;
    for c in text.chars() {
        let glyph_id = font.glyph_id(c);
        total_width += font.h_advance_unscaled(glyph_id) * scale.x;
    }
    let height = (font.ascent_unscaled() * scale.y / font.height_unscaled()).ceil() as u32;
    (total_width.ceil() as u32, height)
}

/// Cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Convert an f32 embedding vector to raw bytes for BLOB storage.
fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Convert raw BLOB bytes back to an f32 embedding vector.
fn blob_to_embedding(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return None;
    }
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>()
        .into()
}

/// YAML sidecar format for `face_scan_folder`.
/// Maps a reference image file to a person name.
#[derive(Debug, serde::Deserialize)]
struct FaceSidecar {
    first_name: String,
    last_name: String,
    #[serde(default)]
    notes: String,
}

/// Image file extensions recognized by the media server for gallery scans
/// and face reference imports. Kept in sync with `gallery::state::DEFAULT_EXTENSIONS`.
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif"];

/// Resolve the default face reference folder: `~/.hkask/faces/`.
/// Returns `None` if `HOME` is not set.
fn default_face_folder() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".hkask").join("faces"))
}

// ── Combined tool router (P5 Essentialism — modular tool groups) ──────────

impl MediaServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::gallery_router()
            + Self::processing_router()
            + Self::audio_router()
            + Self::generation_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for MediaServer {}

/// Run the media MCP server (used by binary target).
pub async fn run(
    userpod: String,
    _daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    dotenvy::dotenv().ok();

    // Build the inference router for vision LLM tasks.
    // Backends are constructed lazily — only those with configured API keys are available.
    let inference_config = hkask_inference::InferenceConfig::from_env();
    let inference = Arc::new(InferenceRouter::new(inference_config));

    let daemon_ok = match try_daemon_flow(&userpod).await {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(target: "hkask.mcp.media", userpod = %userpod, error = %e, "Daemon unavailable — falling back to direct mode");
            false
        }
    };

    let daemon_client = if daemon_ok {
        Some(DaemonClient::new())
    } else {
        None
    };

    // Create an in-memory GalleryStore for the media server.
    // Gracefully degrade if DB initialization fails — gallery tools
    // will return errors but the server stays alive.
    // GalleryStore schema initialized by from_driver().
    let gallery_store = {
        let db = Database::in_memory().expect("in-memory DB");
        let pool = db.sqlite_pool().expect("sqlite pool");
        let driver = Arc::new(SqliteDriver::new(pool));
        tracing::info!(target: "hkask.mcp.media", "Gallery store initialized");
        Arc::new(GalleryStore::from_driver(driver))
    };

    hkask_mcp_server::run_server(
        "hkask-mcp-media",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            Ok(MediaServer::new(
                ctx.webid,
                userpod.clone(),
                daemon_client.clone(),
                inference.clone(),
                Arc::new(Mutex::new(None)),
                gallery_store.clone(),
                templates::create_env(),
                FfmpegRunner::detect(),
            ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "DI_API_KEY",
                "DeepInfra API key for vision LLMs and media generation",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "FA_API_KEY",
                "fal.ai API key for image/video generation",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "TG_API_KEY",
                "Together AI API key for vision LLMs",
            ),
        ],
    )
    .await
}

async fn try_daemon_flow(userpod: &str) -> anyhow::Result<()> {
    let client = DaemonClient::new();
    let result = hkask_mcp_server::verify_startup_gates(&client, userpod, "media", &[]).await?;
    tracing::info!(target: "hkask.mcp.media", userpod = %userpod,
        "P4 gates verified{}",
        if result.denied_tools.is_empty() { String::new() }
        else { format!(" — {} tool(s) denied: {:?}", result.denied_tools.len(), result.denied_tools) }
    );
    Ok(())
}

// ── Integration tests ────────────────────────────────────────────────────
//
// These tests exercise the GalleryStore + GalleryState pipeline and collage
// composition logic. Inference-dependent tools require a running LLM backend
// and are tested via the MCP protocol in live sessions.

#[cfg(test)]
mod integration_tests {
    use crate::gallery::GalleryState;
    use hkask_storage::{GalleryMode, GalleryStore};
    use image::{Rgb, RgbImage};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_store() -> (Arc<GalleryStore>, TempDir) {
        let temp = TempDir::new().expect("tempdir");
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(GalleryStore::from_driver(driver));
        (store, temp)
    }

    fn create_test_image(dir: &std::path::Path, name: &str, r: u8, g: u8, b: u8) {
        let img: RgbImage = RgbImage::from_pixel(64, 64, Rgb([r, g, b]));
        img.save(dir.join(name)).expect("save test image");
    }

    #[test]
    fn gallery_lifecycle_init_to_search() {
        let (store, temp) = setup_store();

        create_test_image(temp.path(), "sunset.jpg", 255, 100, 50);
        create_test_image(temp.path(), "ocean.jpg", 50, 100, 255);
        create_test_image(temp.path(), "forest.png", 34, 139, 34);

        let gallery = store
            .create(
                &temp.path().to_string_lossy(),
                hkask_storage::GalleryMode::ReadOnly,
            )
            .expect("create gallery");
        assert_eq!(gallery.image_count, 0);

        let mut state = GalleryState::new(temp.path().to_path_buf(), GalleryMode::ReadOnly);
        let scan = state.scan(false, None);
        assert_eq!(scan.added, 3);

        for entry in &scan.entries {
            store
                .add_image(
                    &gallery.id,
                    &entry.relative_path,
                    &temp.path().join(&entry.relative_path).to_string_lossy(),
                    &entry.checksum,
                    entry.width,
                    entry.height,
                    &entry.format,
                    entry.size_bytes,
                )
                .expect("add image");
        }

        let img = store
            .get_image(&gallery.id, Some(0), None)
            .expect("get image");
        assert_eq!(img.width, 64);

        store
            .tag_image(&img.id, "object", "sunset", 0.95, "test")
            .expect("tag");

        let tags = store.get_tags(&img.id).expect("get tags");
        assert_eq!(tags.len(), 1);

        let all_tags = store.get_all_tags(&gallery.id).expect("get all tags");
        assert!(!all_tags.is_empty());
        assert!(all_tags.iter().any(|(t, _)| t.value == "sunset"));
    }

    #[test]
    fn collage_compose_grid_layout() {
        let temp = TempDir::new().expect("tempdir");

        let images: Vec<image::DynamicImage> = vec![
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([255u8, 0, 0]))),
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([0, 255u8, 0]))),
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([0, 0, 255u8]))),
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([255u8, 255u8, 0]))),
        ];

        let spacing: u32 = 8;
        let canvas_w: u32 = 800;
        let canvas_h: u32 = 600;
        let cols = (images.len() as f64).sqrt().ceil() as u32;
        let rows = (images.len() as u32).div_ceil(cols);
        assert_eq!(cols, 2);
        assert_eq!(rows, 2);

        let cell_w = (canvas_w - spacing * (cols + 1)) / cols;
        let cell_h = (canvas_h - spacing * (rows + 1)) / rows;

        let mut canvas = image::DynamicImage::new_rgba8(canvas_w, canvas_h);
        let bg = image::Rgba([30u8, 30u8, 30u8, 255u8]);
        for pixel in canvas.as_mut_rgba8().unwrap().pixels_mut() {
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

        let output_path = temp.path().join("collage_test.png");
        canvas.save(&output_path).expect("save collage");
        let collage = image::open(&output_path).expect("reopen");
        assert_eq!(collage.width(), 800);
        assert_eq!(collage.height(), 600);

        let non_bg = collage
            .to_rgba8()
            .pixels()
            .filter(|p| p.0 != [30, 30, 30, 255])
            .count();
        assert!(
            non_bg > 100,
            "collage should have non-bg pixels (got {})",
            non_bg
        );
    }

    #[test]
    fn gallery_store_image_not_found() {
        let (store, temp) = setup_store();
        let gallery = store
            .create(
                &temp.path().to_string_lossy(),
                hkask_storage::GalleryMode::ReadOnly,
            )
            .expect("create gallery");

        assert!(store.get_image(&gallery.id, Some(999), None).is_err());
        assert!(
            store
                .get_image(&gallery.id, None, Some("nonexistent"))
                .is_err()
        );
    }

    #[test]
    fn gallery_three_state_policy() {
        use hkask_storage::GalleryMode;
        assert_eq!(GalleryMode::ReadOnly.as_str(), "read-only");
        assert_eq!(GalleryMode::CopyOnWrite.as_str(), "copy-on-write");
        assert_eq!(GalleryMode::Destructive.as_str(), "destructive");
        assert_ne!(
            GalleryMode::ReadOnly.as_str(),
            GalleryMode::Destructive.as_str()
        );
    }

    // ── Face recognition tests ─────────────────────────────────────────────

    #[test]
    fn face_validation_deserialize_pass() {
        let json = r#"{
            "valid": true,
            "face_count": 1,
            "face_coverage_pct": 45,
            "pose": "frontal",
            "lighting": "good",
            "occlusion": "none",
            "clarity": "sharp",
            "issues": []
        }"#;
        let result: crate::gallery::vision::FaceValidationResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(result.valid);
        assert_eq!(result.face_count, 1);
        assert_eq!(result.face_coverage_pct, 45);
        assert_eq!(result.pose, "frontal");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn face_validation_deserialize_reject() {
        let json = r#"{
            "valid": false,
            "face_count": 2,
            "face_coverage_pct": 10,
            "pose": "profile",
            "lighting": "poor",
            "occlusion": "significant",
            "clarity": "blurry",
            "issues": [
                "Multiple faces detected (2) — reference must contain exactly 1 face",
                "Face coverage too low (10%) — minimum 15% required",
                "Profile pose — frontal or near-frontal required"
            ]
        }"#;
        let result: crate::gallery::vision::FaceValidationResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(!result.valid);
        assert_eq!(result.face_count, 2);
        assert_eq!(result.issues.len(), 3);
        assert!(result.issues[0].contains("Multiple faces"));
    }

    #[test]
    fn face_match_deserialize_match() {
        let json = r#"{
            "match": true,
            "confidence": 0.94,
            "reasoning": "Same bone structure, identical eye spacing, matching nose shape."
        }"#;
        let result: crate::gallery::vision::FaceMatchResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(result.is_match);
        assert!((result.confidence - 0.94).abs() < 0.001);
        assert!(result.reasoning.contains("bone structure"));
    }

    #[test]
    fn face_match_deserialize_no_match() {
        let json = r#"{
            "match": false,
            "confidence": 0.85,
            "reasoning": "Different jawline structure and eye shape — likely different people."
        }"#;
        let result: crate::gallery::vision::FaceMatchResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(!result.is_match);
        assert!((result.confidence - 0.85).abs() < 0.001);
        assert!(result.reasoning.contains("Different"));
    }

    #[test]
    fn face_registry_lifecycle() {
        let (store, _temp) = setup_store();

        // Create a gallery and image for the face reference
        let gallery = store
            .create("/tmp/test-gallery", GalleryMode::ReadOnly)
            .expect("create gallery");
        let img = store
            .add_image(
                &gallery.id,
                "alice.jpg",
                "/tmp/test-gallery/alice.jpg",
                "hash1",
                400,
                600,
                "jpg",
                50000,
            )
            .expect("add image");

        // Register a face
        let face = store
            .register_face("Alice", "Chen", &img.id, None, "valid", "Frontal portrait")
            .expect("register face");
        assert_eq!(face.first_name, "Alice");
        assert_eq!(face.status, "valid");

        // List faces
        let faces = store.list_faces(None).expect("list faces");
        assert_eq!(faces.len(), 1);

        // Get by ID
        let retrieved = store.get_face(&face.id).expect("get face");
        assert_eq!(retrieved.last_name, "Chen");

        // Remove
        store.remove_face(&face.id).expect("remove face");
        let faces = store.list_faces(None).expect("list after remove");
        assert_eq!(faces.len(), 0);
    }

    #[test]
    fn face_registry_status_filter() {
        let (store, _temp) = setup_store();
        let gallery = store
            .create("/tmp/test-gallery", GalleryMode::ReadOnly)
            .expect("create gallery");
        let img1 = store
            .add_image(
                &gallery.id,
                "a.jpg",
                "/tmp/a.jpg",
                "h1",
                100,
                100,
                "jpg",
                1000,
            )
            .expect("add img1");
        let img2 = store
            .add_image(
                &gallery.id,
                "b.jpg",
                "/tmp/b.jpg",
                "h2",
                100,
                100,
                "jpg",
                1000,
            )
            .expect("add img2");

        store
            .register_face("Alice", "A", &img1.id, None, "valid", "")
            .unwrap();
        store
            .register_face("Bob", "B", &img2.id, None, "rejected", "Too dark")
            .unwrap();

        let valid = store.list_faces(Some("valid")).unwrap();
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].first_name, "Alice");

        let rejected = store.list_faces(Some("rejected")).unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].first_name, "Bob");

        let pending = store.list_faces(Some("pending")).unwrap();
        assert_eq!(pending.len(), 0);
    }
}
