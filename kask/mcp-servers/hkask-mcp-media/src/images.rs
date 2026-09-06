//! Gallery image helpers — resolution, tagging, analysis pipeline, and EXIF.

use crate::error::MediaError;
use crate::gallery::vision;
use crate::{MediaServer, read_image_capped};
use std::path::PathBuf;

/// Encode the captured revision, refusing a path whose contents changed since observation.
fn image_record_url(record: &hkask_storage::gallery::ImageRecord) -> Result<String, MediaError> {
    use sha2::Digest;
    let bytes = read_image_capped(&record.absolute_path)?;
    let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
    if record.missing || hash != record.hash {
        return Err(MediaError::Io(format!("Asset {} changed or is missing; rescan before analyzing", record.id)));
    }
    let format = image::guess_format(&bytes).map_err(|error| MediaError::Io(error.to_string()))?;
    let base64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok(format!("data:{};base64,{base64}", format.to_mime_type()))
}

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
    pub(crate) fn resolve_image_url_by_id(&self, gallery_id: &str, image_id: &str) -> Result<String, MediaError> {
        let record = self.gallery_store.get_by_id(gallery_id, image_id)?;
        image_record_url(&record)
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

    /// Scan a captured gallery, not whichever gallery becomes active later.
    pub(crate) fn rescan_gallery(
        &self, gallery: &crate::GalleryAccess, recursive: bool,
    ) -> Result<(hkask_storage::gallery::GalleryScan, hkask_storage::gallery::ReconcileResult), MediaError> {
        let state = crate::GalleryState::new(gallery.root_path.clone(), gallery.mode.parse()?);
        let scan = state.scan(recursive, None);
        let result = self.gallery_store.reconcile(&gallery.gallery_id, &scan)?;
        Ok((scan, result))
    }

    /// Run the analysis pipeline on a subset of gallery images.
    /// Used internally by gallery_organize auto_analyze and gallery_analyze.
    /// Returns (analyzed_count, error_messages).
    pub(crate) async fn run_analysis_on_indices(
        &self,
        gallery: &crate::GalleryAccess,
        indices: &[usize],
        pipelines: &[String],
    ) -> (u32, Vec<String>) {
        let records = indices.iter().map(|index| self.gallery_store.get_image(&gallery.gallery_id, Some(*index), None))
            .collect::<Result<Vec<_>, _>>();
        match records {
            Ok(records) => self.run_analysis_on_assets(&records, pipelines).await,
            Err(error) => (0, vec![error.to_string()]),
        }
    }

    pub(crate) async fn run_analysis_on_assets(
        &self, records: &[hkask_storage::gallery::ImageRecord], pipelines: &[String],
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

        for record in records {
            let idx = &record.id;
            let image_id = &record.id;
            let before_errors = errors.len();
            let mut tags: Vec<(String, String, f64)> = Vec::new();
            let image_url = match image_record_url(record) {
                Ok(url) => url,
                Err(error) => { errors.push(format!("image {idx}: {error}")); continue; }
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
                                    tags.push(("face".into(), value, 0.85))
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
                                Ok(value) => tags.push(("object".into(), value, 0.85)),
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
                                    Ok(value) => tags.push(("color".into(), value, 0.85)),
                                    Err(e) => errors.push(format!(
                                        "image {} color tag serialization: {}",
                                        idx, e
                                    )),
                                }
                            }
                        }
                        for field in &["palette_style", "temperature", "saturation"] {
                            if let Some(v) = parsed.get(*field).and_then(|v| v.as_str()) {
                                tags.push(("color".into(), v.into(), 0.9));
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
                                tags.push(("composition".into(), v.into(), 0.85));
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
                        tags.push(("caption".into(), caption, 0.9));
                    }
                    Err(e) => {
                        errors.push(format!("image {} scene caption: {}", idx, e));
                    }
                }
            }

            // A subset cannot certify all retained annotations. Face metadata also
            // requires the face pipeline when such annotations already exist.
            let has_faces = match self.gallery_store.get_tags(image_id) {
                Ok(tags) => tags.iter().any(|tag| tag.tag_type == "face"),
                Err(error) => { errors.push(error.to_string()); continue; }
            };
            let complete = errors.len() == before_errors && run_objects && run_colors
                && run_composition && run_scene && (run_faces || !has_faces);
            // Detect source edits even when no rescan has updated the durable hash yet.
            if let Err(error) = image_record_url(record) {
                errors.push(format!("image {idx} changed during analysis: {error}"));
                continue;
            }
            match self.gallery_store.persist_analysis(record, &tags, vision_label, complete) {
                Ok(true) if errors.len() == before_errors => analyzed += 1,
                Ok(true) => {},
                Ok(false) => errors.push(format!("image {idx} revision changed during analysis")),
                Err(error) => errors.push(format!("image {idx} metadata persistence: {error}")),
            }
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
