//! Face pipeline — registration, matching, and folder scanning.
//!
//! Shared by the `face_*` MCP tools and the `gallery_refresh` orchestrator.

use crate::GalleryAccess;
use crate::MediaServer;
use crate::error::{MediaError, map_media_error};
use crate::gallery::vision;
use crate::types::FaceStatus;
use hkask_mcp_server::server::McpToolError;
use hkask_storage::database::value::DbValue;
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
            hkask_storage::FaceRegistryRecord,
            Option<vision::FaceValidationResult>,
        ),
        McpToolError,
    > {
        let (status, notes, validation) = if force {
            (FaceStatus::Valid, user_notes.to_string(), None)
        } else {
            let (vision_model, _vision_label) = self.require_vision().await?;
            let v = vision::validate_face_reference(
                &self.vision_port,
                &self.template_env,
                image_url,
                Some(vision_model),
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
        // local embedding model, no local geometric matching. Full build-out
        // is deferred; no embedding is produced at registration. The store's
        // nullable `embedding` column is legacy from a removed local-cosine
        // path, is unused, and is not part of this design.
        let record = self
            .gallery_store
            .register_face(
                first_name,
                last_name,
                image_id,
                None,
                status.as_ref(),
                &notes,
            )
            .map_err(|e| map_media_error(e.into()))?;
        Ok((record, validation))
    }

    /// Import a reference image file into the current gallery (idempotent by
    /// SHA-256 hash) and return its gallery `image_id` plus a base64 data URL
    /// suitable for vision LLM calls. Used by `face_scan_folder`.
    ///
    /// If the image is already in the gallery (matched by hash), the existing
    /// record is reused — no duplicate row is inserted. The file is read from
    /// disk exactly once; the base64 URL is computed from the in-memory bytes.
    pub(crate) fn import_reference_image(
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

            for reg_entry in registry {
                // Vision LLM `match_faces` two-image comparison — the only
                // comparator. (A previous cosine fast-path on LLM-produced
                // "embeddings" was removed: LLMs cannot emit geometrically
                // consistent vectors, so those scores were noise.)
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
    pub(crate) async fn register_one_face(
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
    pub(crate) fn crop_face_region(
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
                    DbValue::Text(ga.gallery_id),
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
