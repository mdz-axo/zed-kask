//! Face pipeline — registration, matching, and folder scanning.
//!
//! Shared by the `face_*` MCP tools and the `gallery_refresh` orchestrator.

use crate::GalleryAccess;
use crate::MediaServer;
use crate::error::{MediaError, map_media_error};
use crate::gallery::vision;
use crate::types::FaceStatus;
use hkask_mcp_server::server::McpToolError;
use sha2::Digest;

#[derive(Debug, serde::Deserialize)]
struct FaceSidecar {
    first_name: String,
    last_name: String,
    #[serde(default)]
    notes: String,
}

impl MediaServer {
    /// Shared face-registration logic used by both the `face_register` MCP tool
    /// (which takes a gallery image index) and `face_scan_folder` (which
    /// imports a reference image from a folder, then registers it).
    ///
    /// Validates the image via the `validate_face_ref` vision LLM template
    /// (unless `force` is set), then inserts a row into `face_registry`.
    /// Returns `(FaceRegistryRecord, Option<FaceValidationResult>)`.
    pub(crate) async fn register_face_from_url(
        &self,
        image_id: &str,
        image_url: &str,
        first_name: &str,
        last_name: &str,
        user_notes: &str,
        force: bool,
    ) -> Result<
        (
            crate::gallery::FaceRegistryRecord,
            Option<vision::FaceValidationResult>,
        ),
        McpToolError,
    > {
        let (status, notes, validation) = if force {
            (FaceStatus::Valid, user_notes.to_string(), None)
        } else {
            let (vision_model, _vision_label) = self.require_vision()?;
            let v = vision::validate_face_reference(
                &self.vision_port,
                &self.template_env,
                image_url,
                Some(vision_model.as_str()),
            )
            .await
            .map_err(map_media_error)?;
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

        // Design decision (2026-08-29): face recognition relies on vision-LLM
        // calls, not local code. The implementation surface is the minijinja
        // (j2) prompt templates — `validate_face_ref` here, `match_faces` in
        // `run_face_matching` — dispatched through the inference port, the
        // same pattern as every other vision capability in this server. No
        // local embedding model, no local geometric matching; a previous
        // LLM-produced-"embedding" cosine path was removed because LLMs
        // cannot emit geometrically consistent vectors, and its store
        // column went with it. Full build-out of the face-recognition
        // feature is deferred — the current templates are the working core.
        let record = self
            .gallery_store
            .register_face(first_name, last_name, image_id, status.as_ref(), &notes)
            .map_err(|e| map_media_error(e.into()))?;
        Ok((record, validation))
    }

    /// Import a reference image into the gallery that owns its file (`home` is
    /// the file's canonical folder; see `assets::owning_gallery_id`), idempotent
    /// by canonical path, and return its gallery `image_id` plus a base64 data
    /// URL for vision LLM calls. Used by `face_scan_folder` and frame extraction.
    ///
    /// If the image is already in the gallery (matched by canonical path), the existing
    /// record is reused — no duplicate row is inserted. The file is read from
    /// disk exactly once; the base64 URL is computed from the in-memory bytes.
    pub(crate) fn import_reference_image(
        &self,
        abs_path: &std::path::Path,
        home: &std::path::Path,
    ) -> Result<(String, String), MediaError> {
        let abs_path = abs_path.canonicalize()?;
        let gallery_id = crate::assets::owning_gallery_id(&self.gallery_store, &abs_path, home)?;
        let data = std::fs::read(&abs_path)
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

        let image =
            image::load_from_memory(&data).map_err(|error| MediaError::Io(error.to_string()))?;
        let (width, height) = (image.width(), image.height());
        let size_bytes = data.len() as u64;

        let record = self
            .gallery_store
            .add_image(
                &gallery_id,
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
    /// every entry in the face registry using the `match_faces` vision LLM
    /// template (a two-image same-person comparison). On a match
    /// (confidence ≥ 0.7), persist a `face` tag with the person's name and
    /// registry_id. Returns `(faces_matched, errors)`.
    ///
    /// This is the composable face-matching stage called by `gallery_refresh`
    /// when `include_faces=true`.
    pub(crate) async fn run_face_matching(
        &self,
        ga: &GalleryAccess,
        registry: &[crate::gallery::FaceRegistryRecord],
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

        let vision_model = match self.resolve_vision_model() {
            Some((m, _label)) => m,
            None => {
                errors.push(format!(
                    "Face matching skipped: no vision model configured (set {})",
                    crate::models::VISION_ENV
                ));
                return (0, errors);
            }
        };

        // Group registry entries by person `(first_name, last_name)` in
        // first-seen order so multiple reference portraits for the same person
        // act as a best-of reference set (early-exiting within a person's
        // references when confidence >= 0.85).
        let mut person_groups: Vec<((String, String), Vec<&crate::gallery::FaceRegistryRecord>)> =
            Vec::new();
        for reg_entry in registry {
            let key = (reg_entry.first_name.clone(), reg_entry.last_name.clone());
            if let Some((_, entries)) = person_groups.iter_mut().find(|(k, _)| *k == key) {
                entries.push(reg_entry);
            } else {
                person_groups.push((key, vec![reg_entry]));
            }
        }

        for (tag, _path) in &all_tags {
            if tag.tag_type != "face" {
                continue;
            }

            // Parse the tag value once — used for already-matched skip, bbox
            // extraction, and face_index/bbox preservation on match.
            let parsed: Option<serde_json::Value> = serde_json::from_str(&tag.value).ok();
            let already_named = parsed
                .as_ref()
                .and_then(|v| v.get("name").or_else(|| v.get("registry_id")))
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.trim().is_empty());
            if already_named {
                continue;
            }

            let face_image_id = &tag.image_id;
            let face_bbox = parsed.as_ref().and_then(|v| v.get("bbox").cloned());

            let query_url = if let Some(ref bbox) = face_bbox {
                match self.crop_face_region(face_image_id, bbox) {
                    Ok(cropped_url) => cropped_url,
                    Err(e) => {
                        errors.push(format!("Face tag {} crop failed: {}", tag.id, e));
                        continue;
                    }
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

            for ((first_name, last_name), entries) in &person_groups {
                let mut best_match: Option<(&crate::gallery::FaceRegistryRecord, f64)> = None;
                for reg_entry in entries {
                    // Vision LLM `match_faces` two-image comparison — the only
                    // comparator.
                    let ref_url = match self.resolve_image_url_by_id(&reg_entry.image_id) {
                        Ok(url) => url,
                        Err(e) => {
                            errors.push(format!("Registry entry {}: {}", reg_entry.id, e));
                            continue;
                        }
                    };

                    match vision::match_faces(
                        &self.vision_port,
                        &self.template_env,
                        &ref_url,
                        &query_url,
                        Some(vision_model.as_str()),
                    )
                    .await
                    {
                        Ok(result) => {
                            if result.is_match && result.confidence >= 0.7 {
                                let replace = best_match
                                    .is_none_or(|(_, best_conf)| result.confidence > best_conf);
                                if replace {
                                    best_match = Some((*reg_entry, result.confidence));
                                }
                                if result.confidence >= 0.85 {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            errors.push(format!("Match {} vs {}: {}", reg_entry.id, tag.id, e));
                        }
                    }
                }

                if let Some((matched_entry, confidence)) = best_match {
                    let name = format!("{first_name} {last_name}");
                    let face_index = parsed.as_ref().and_then(|v| v["face_index"].as_u64());
                    let mut new_value = serde_json::json!({
                        "face_index": face_index,
                        "name": name,
                        "match_confidence": confidence,
                        "registry_id": matched_entry.id,
                        "method": "vision_llm",
                    });
                    if let Some(bbox) = face_bbox.clone() {
                        new_value["bbox"] = bbox;
                    }
                    match self.gallery_store.replace_tag(
                        &tag.id,
                        &tag.image_id,
                        "face",
                        &new_value.to_string(),
                        confidence,
                        vision_model.as_str(),
                    ) {
                        Ok(_) => faces_matched += 1,
                        Err(error) => errors.push(format!(
                            "Match {} vs {} could not persist: {}",
                            matched_entry.id, tag.id, error
                        )),
                    }
                    break;
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
    pub(crate) async fn register_one_face(
        &self,
        path: &std::path::Path,
        folder: &std::path::Path,
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

        if std::fs::symlink_metadata(&sidecar)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
        {
            return Err(MediaError::SidecarInvalid(format!(
                "{}: refusing symlinked sidecar: {}",
                fname,
                sidecar.display()
            )));
        }

        let sidecar_text = std::fs::read_to_string(&sidecar)
            .map_err(|e| MediaError::Io(format!("{}: failed to read sidecar: {}", fname, e)))?;

        let parsed: FaceSidecar = serde_yaml_neo::from_str(&sidecar_text).map_err(|e| {
            MediaError::SidecarInvalid(format!(
                "{}: invalid sidecar YAML: {}",
                sidecar.display(),
                e
            ))
        })?;

        let (image_id, image_url) = self.import_reference_image(path, folder).map_err(|e| {
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
    /// files with `.yaml` sidecars, imports each image into its owning gallery
    /// (idempotent by canonical path), validates via the vision LLM (unless `force`),
    /// and registers in `face_registry`. Returns a JSON summary.
    pub(crate) async fn run_face_scan_folder(
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
            if entry.file_type().is_symlink() {
                errors.push(format!("Symlink not scanned: {}", path.display()));
                continue;
            }
            if !entry.file_type().is_file() {
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

            match self.register_one_face(path, folder, &ext, force).await {
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
    pub(crate) fn crop_face_region(
        &self,
        image_id: &str,
        bbox: &serde_json::Value,
    ) -> Result<String, MediaError> {
        let record = self.gallery_store.get_by_id(image_id)?;
        if record.missing {
            return Err(MediaError::ImageNotFound(image_id.into()));
        }
        let bytes = crate::read_image_capped(&record.absolute_path)?;
        if format!("{:x}", sha2::Sha256::digest(&bytes)) != record.hash {
            return Err(MediaError::Io(
                "Face image changed; rescan before matching".into(),
            ));
        }
        let img =
            image::load_from_memory(&bytes).map_err(|error| MediaError::Io(error.to_string()))?;

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
}

/// Resolve the default face reference folder: `{kask_data_dir}/mcp/media/faces/`.
pub(crate) fn default_face_folder() -> Option<std::path::PathBuf> {
    let dir =
        hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new("mcp/media/faces"));
    if let Some(parent) = dir.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                target: "hkask.mcp.media",
                path = %parent.display(),
                %error,
                "Failed to create face folder — the subsequent scan will surface the failure"
            );
        }
    }
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gallery::{GalleryMode, GalleryStore};
    use hkask_storage::database::sqlite::SqliteDriver;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct CountingMatchPort {
        calls: AtomicUsize,
        responses: Vec<&'static str>,
    }

    impl CountingMatchPort {
        fn new(responses: Vec<&'static str>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                responses,
            }
        }
    }

    crate::test_support::media_inference_stub! {
        CountingMatchPort,
        generate(self, _prompt): {
            panic!("face matching must not call text generate")
        },
        generate_vision(self, _prompt, _images): {
            let idx = self.calls.fetch_add(1, Ordering::SeqCst);
            let text = self
                .responses
                .get(idx)
                .copied()
                .unwrap_or(r#"{"match":false,"confidence":0.1,"reasoning":"no","matching_features":[],"differing_features":[]}"#)
                .to_string();
            Box::pin(async move {
                Ok(hkask_types::InferenceResult {
                    text,
                    model: "test-vision".into(),
                    usage: Default::default(),
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn run_face_matching_aggregates_multi_ref_replaces_in_place_and_skips_matched() {
        unsafe { std::env::set_var(crate::models::VISION_ENV, "OpenRouter/test-vision") };
        let dir = tempfile::tempdir().expect("tempdir");
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(GalleryStore::from_driver(driver).expect("store"));
        let gallery = store
            .open(&dir.path().to_string_lossy(), GalleryMode::ReadOnly)
            .expect("open gallery");

        let write_png = |name: &str, color: u8| -> std::path::PathBuf {
            let path = dir.path().join(name);
            image::RgbImage::from_pixel(4, 4, image::Rgb([color, 0, 0]))
                .save(&path)
                .expect("save png");
            path
        };
        let compute_hash = |path: &std::path::Path| -> String {
            let bytes = std::fs::read(path).expect("read png");
            format!("{:x}", sha2::Sha256::digest(&bytes))
        };

        let ref1_path = write_png("alice_ref1.png", 10);
        let ref2_path = write_png("alice_ref2.png", 20);
        let query_path = write_png("group_photo.png", 30);

        let ref1_img = store
            .add_image(
                &gallery.id,
                &ref1_path.to_string_lossy(),
                &compute_hash(&ref1_path),
                4,
                4,
                "png",
                64,
            )
            .expect("add ref1");
        let ref2_img = store
            .add_image(
                &gallery.id,
                &ref2_path.to_string_lossy(),
                &compute_hash(&ref2_path),
                4,
                4,
                "png",
                64,
            )
            .expect("add ref2");
        let query_img = store
            .add_image(
                &gallery.id,
                &query_path.to_string_lossy(),
                &compute_hash(&query_path),
                4,
                4,
                "png",
                64,
            )
            .expect("add query");

        let _reg1 = store
            .register_face("Alice", "Lovelace", &ref1_img.id, "valid", "profile")
            .expect("reg1");
        let reg2 = store
            .register_face("Alice", "Lovelace", &ref2_img.id, "valid", "frontal")
            .expect("reg2");

        // Detected face tag with bbox and no name yet.
        let initial_tag = serde_json::json!({
            "face_index": 0,
            "bbox": {"x_pct": 10.0, "y_pct": 10.0, "w_pct": 50.0, "h_pct": 50.0}
        });
        store
            .tag_image(
                &query_img.id,
                "face",
                &initial_tag.to_string(),
                0.85,
                "detector",
            )
            .expect("tag_image");

        // First reference evaluated scores 0.55 (below 0.7 threshold), second scores 0.82 (match).
        let port = Arc::new(CountingMatchPort::new(vec![
            r#"{"match":true,"confidence":0.55,"reasoning":"partial","matching_features":[],"differing_features":["angle"]}"#,
            r#"{"match":true,"confidence":0.82,"reasoning":"strong","matching_features":["eyes","jawline"],"differing_features":[]}"#,
        ]));
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            port.clone(),
            Arc::new(Mutex::new(None)),
            store.clone(),
            crate::templates::create_env().expect("templates"),
            crate::video::FfmpegRunner::detect(),
            crate::video::YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            Default::default(),
        );
        let ga = crate::GalleryAccess {
            gallery_id: gallery.id.clone(),
            image_count: 3,
            root_path: dir.path().to_path_buf(),
            mode: "read-only".to_string(),
            total_size_bytes: 192,
        };
        let registry = store.list_faces(Some("valid")).expect("list faces");

        let (matched, errors) = server.run_face_matching(&ga, &registry).await;
        assert_eq!(matched, 1, "errors: {errors:?}");
        assert!(errors.is_empty());
        assert_eq!(port.calls.load(Ordering::SeqCst), 2);

        // Exactly one face tag remains on query_img (replaced in place, no duplicate),
        // carrying the matched name, reg2's registry_id, and preserved bbox.
        let tags = store.get_tags(&query_img.id).expect("get_tags");
        assert_eq!(
            tags.len(),
            1,
            "unnamed tag must be replaced in place, not duplicated"
        );
        let matched_json: serde_json::Value = serde_json::from_str(&tags[0].value).expect("json");
        assert_eq!(matched_json["name"], "Alice Lovelace");
        assert_eq!(matched_json["match_confidence"], 0.82);
        assert!(
            matched_json["registry_id"] == reg2.id || matched_json["registry_id"] == _reg1.id,
            "matched registry_id must belong to Alice Lovelace"
        );
        assert!(
            matched_json.get("bbox").is_some(),
            "bbox must be preserved on match"
        );

        // A second pass skips the already-matched face tag with zero vision calls.
        let (matched_again, errors_again) = server.run_face_matching(&ga, &registry).await;
        assert_eq!(matched_again, 0);
        assert!(errors_again.is_empty());
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            2,
            "already-matched face tag must not trigger additional vision calls"
        );
    }

    #[tokio::test]
    async fn run_face_matching_surfaces_crop_failure_without_full_image_fallback() {
        unsafe { std::env::set_var(crate::models::VISION_ENV, "OpenRouter/test-vision") };
        let dir = tempfile::tempdir().expect("tempdir");
        let driver = SqliteDriver::in_memory_driver();
        let store = Arc::new(GalleryStore::from_driver(driver).expect("store"));
        let gallery = store
            .open(&dir.path().to_string_lossy(), GalleryMode::ReadOnly)
            .expect("open gallery");

        let ref_path = dir.path().join("ref.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([10, 0, 0]))
            .save(&ref_path)
            .expect("save ref");
        let ref_bytes = std::fs::read(&ref_path).expect("read ref");
        let ref_hash = format!("{:x}", sha2::Sha256::digest(&ref_bytes));
        let ref_img = store
            .add_image(
                &gallery.id,
                &ref_path.to_string_lossy(),
                &ref_hash,
                4,
                4,
                "png",
                64,
            )
            .expect("add ref");
        store
            .register_face("Alice", "Lovelace", &ref_img.id, "valid", "")
            .expect("register face");

        // Query image has a stale hash on disk so `crop_face_region` fails with a
        // revision mismatch — `run_face_matching` must surface the crop error and
        // NOT fall back to sending the uncropped image.
        let query_path = dir.path().join("query.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([20, 0, 0]))
            .save(&query_path)
            .expect("save query");
        let query_img = store
            .add_image(
                &gallery.id,
                &query_path.to_string_lossy(),
                "stale-hash",
                4,
                4,
                "png",
                64,
            )
            .expect("add query");
        let tag_val = serde_json::json!({
            "face_index": 0,
            "bbox": {"x_pct": 0.0, "y_pct": 0.0, "w_pct": 50.0, "h_pct": 50.0}
        });
        store
            .tag_image(
                &query_img.id,
                "face",
                &tag_val.to_string(),
                0.85,
                "detector",
            )
            .expect("tag query");

        let port = Arc::new(CountingMatchPort::new(vec![]));
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            port.clone(),
            Arc::new(Mutex::new(None)),
            store.clone(),
            crate::templates::create_env().expect("templates"),
            crate::video::FfmpegRunner::detect(),
            crate::video::YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            Default::default(),
        );
        let ga = crate::GalleryAccess {
            gallery_id: gallery.id.clone(),
            image_count: 2,
            root_path: dir.path().to_path_buf(),
            mode: "read-only".to_string(),
            total_size_bytes: 128,
        };
        let registry = store.list_faces(Some("valid")).expect("list faces");

        let (matched, errors) = server.run_face_matching(&ga, &registry).await;
        assert_eq!(matched, 0);
        assert_eq!(errors.len(), 1, "crop failure must be surfaced: {errors:?}");
        assert!(
            errors[0].contains("crop failed"),
            "unexpected error: {}",
            errors[0]
        );
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            0,
            "crop failure must not fall back to a full-image vision call"
        );
    }
}
