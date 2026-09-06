//! Gallery image helpers — resolution, tagging, analysis pipeline, and EXIF.

use crate::error::MediaError;
use crate::gallery::vision;
use crate::{MediaServer, read_image_capped};
use hkask_storage::database::value::DbValue;
use std::path::PathBuf;

impl MediaServer {
    /// Resolve an image index to a base64 data URL for vision LLM calls.
    pub(crate) fn resolve_image_url(&self, image_index: usize) -> Result<String, MediaError> {
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

        let data = read_image_capped(&img.absolute_path)?;
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
    pub(crate) fn resolve_image_path(&self, image_index: usize) -> Result<PathBuf, MediaError> {
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
    pub(crate) fn resolve_image_id(&self, image_index: usize) -> Result<String, MediaError> {
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
    pub(crate) fn resolve_image_url_by_id(&self, image_id: &str) -> Result<String, MediaError> {
        let ga = self.access_gallery()?;

        // Look up the image's absolute path by its SQLite ID
        let rows = self
            .gallery_store
            .driver()
            .query(
                "SELECT absolute_path FROM gallery_images WHERE id = ?1 AND gallery_id = ?2",
                &[
                    DbValue::Text(image_id.to_string()),
                    DbValue::Text(ga.gallery_id),
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

        let data = read_image_capped(&absolute_path)?;
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
    pub(crate) fn persist_tag(
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
}

/// Split a provider-prefixed model name into (prefixed_name, label) — the
/// label (everything after the first `/`) is for logs and tool results.
fn split_model_label(prefixed: String) -> (String, String) {
    let label = prefixed
        .split_once('/')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| prefixed.clone());
    (prefixed, label)
}

/// Pure core of the env-configured vision model resolution: `Some(value)`
/// resolves to (prefixed, label), `None`/blank values resolve to `None`
/// (the registry heuristic then picks).
fn configured_vision_model_from(env_value: Option<String>) -> Option<(String, String)> {
    env_value
        .filter(|m| !m.trim().is_empty())
        .map(split_model_label)
}

/// The env-configured vision model (`HKASK_MEDIA_VISION_MODEL`, injected
/// from the settings default or an operator override), or `None` when
/// unset.
fn configured_vision_model() -> Option<(String, String)> {
    configured_vision_model_from(crate::models::vision_model())
}

impl MediaServer {
    /// Resolve the vision model for the tagging pipelines.
    ///
    /// The env-configured model (settings default or operator override,
    /// injected as `HKASK_MEDIA_VISION_MODEL`) wins — deterministic. The
    /// registry heuristic below is the fallback for direct-CLI runs without
    /// injected env: it picks the first OpenRouter vision model the registry
    /// reports, which is whatever the catalog lists first — the live gap
    /// 2026-09-04 resolved to a reasoning-mandatory model that rejected
    /// every non-reasoning tagging call ("Reasoning is mandatory for this
    /// endpoint and cannot be disabled").
    pub(crate) async fn resolve_vision_model(&self) -> Option<(String, String)> {
        if let Some(configured) = configured_vision_model() {
            return Some(configured);
        }

        let models = match self.vision_port.list_vision_models().await {
            Ok(models) => models,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.media",
                    error = %e,
                    "list_vision_models failed — inference port unavailable, returning None"
                );
                return None;
            }
        };

        for model in &models {
            // Match case-insensitively: the IPC model list uses zed provider
            // ids such as "openrouter", not the display name "OpenRouter".
            let prefix = model.prefixed_name.split('/').next().unwrap_or("");
            if prefix.eq_ignore_ascii_case("openrouter") {
                return Some(split_model_label(model.prefixed_name.clone()));
            }
        }

        None
    }

    /// Re-scan an existing gallery and persist new images.
    /// Returns (gallery_id, old_image_count, images_added, total_images, persisted_count).
    /// The MutexGuard is dropped before return so callers can safely await.
    pub(crate) fn rescan_existing_gallery(
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
    pub(crate) async fn run_analysis_on_indices(
        &self,
        indices: &[usize],
        pipelines: &[String],
    ) -> (u32, Vec<String>) {
        let (vision_model, vision_label) = match self.resolve_vision_model().await {
            Some(v) => v,
            None => {
                return (
                    0,
                    vec![
                    "No vision model available — configure a vision-capable provider (OpenRouter)"
                        .to_string(),
                ],
                );
            }
        };
        // Shadow to &str so the per-pipeline call sites below (which take
        // Option<&str> / &str) work unchanged.
        let vision_model = vision_model.as_str();
        let vision_label = vision_label.as_str();
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
                    &self.vision_port,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(faces) => {
                        for face in &faces {
                            match serde_json::to_string(face) {
                                Ok(value) => {
                                    self.persist_tag(&image_id, "face", &value, 0.85, vision_label)
                                }
                                Err(e) => errors
                                    .push(format!("image {} face tag serialization: {}", idx, e)),
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("image {} face detection: {}", idx, e));
                    }
                }
            }

            if run_objects {
                match vision::detect_objects(
                    &self.vision_port,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(objects) => {
                        for obj in &objects {
                            match serde_json::to_string(obj) {
                                Ok(value) => self.persist_tag(
                                    &image_id,
                                    "object",
                                    &value,
                                    0.85,
                                    vision_label,
                                ),
                                Err(e) => errors
                                    .push(format!("image {} object tag serialization: {}", idx, e)),
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("image {} object detection: {}", idx, e));
                    }
                }
            }

            if run_colors {
                match vision::analyze_colors(
                    &self.vision_port,
                    &self.template_env,
                    &image_url,
                    Some(vision_model),
                )
                .await
                {
                    Ok(parsed) => {
                        if let Some(colors) = parsed["colors"].as_array() {
                            for color in colors {
                                match serde_json::to_string(color) {
                                    Ok(value) => self.persist_tag(
                                        &image_id,
                                        "color",
                                        &value,
                                        0.85,
                                        vision_label,
                                    ),
                                    Err(e) => errors.push(format!(
                                        "image {} color tag serialization: {}",
                                        idx, e
                                    )),
                                }
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
                    &self.vision_port,
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
                    &self.vision_port,
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
    pub(crate) fn extract_exif(path: &str) -> serde_json::Value {
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
            if let Some(entry) = exif.get_by_code(nom_exif::IfdIndex::MAIN, *code)
                && let Some(value_str) = entry.as_str()
            {
                fields.insert(
                    name.to_string(),
                    serde_json::Value::String(value_str.to_string()),
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The configured model wins over the registry heuristic and splits
    /// into (prefixed, label) — the label feeds logs and tool results.
    /// Pins the deterministic-resolution fix: before it, the registry
    /// heuristic picked the catalog's first vision model, which was
    /// reasoning-mandatory and rejected every tagging call.
    #[test]
    fn configured_vision_model_splits_prefix_and_label() {
        let (name, label) =
            configured_vision_model_from(Some("OpenRouter/openai/gpt-4o-mini".to_string()))
                .expect("configured model resolves");
        assert_eq!(name, "OpenRouter/openai/gpt-4o-mini");
        assert_eq!(label, "openai/gpt-4o-mini");
    }

    #[test]
    fn configured_vision_model_none_when_unset_or_blank() {
        assert!(configured_vision_model_from(None).is_none());
        assert!(configured_vision_model_from(Some(String::new())).is_none());
        assert!(configured_vision_model_from(Some("  ".to_string())).is_none());
    }
}
