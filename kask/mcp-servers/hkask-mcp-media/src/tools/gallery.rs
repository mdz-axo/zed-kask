//! Gallery tools — organize, search, find-similar, refresh, describe, analyze, faces, timeline.
use crate::*;

#[tool_router(router = gallery_router, vis = "pub")]
impl MediaServer {
    // ── Gallery tools ────────────────────────────────────────────────────────

    #[tool(
        description = "Organize a photo gallery. Point at a folder — the system creates the index, scans for images, and returns status. Use gallery_search to find photos by content."
    )]
    pub async fn gallery_organize(
        &self,
        Parameters(GalleryOrganizeRequest {
            path,
            mode,
            recursive,
            auto_analyze,
        }): Parameters<GalleryOrganizeRequest>,
    ) -> String {
        execute_tool(self, "gallery_organize", async {
            let gallery_mode = match mode.as_str() {
                "read-only" => GalleryMode::ReadOnly,
                "copy-on-write" => GalleryMode::CopyOnWrite,
                "destructive" => GalleryMode::Destructive,
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "Invalid mode '{}': must be read-only, copy-on-write, or destructive",
                        other
                    )));
                }
            };

            // Create gallery in SQLite
            let record = match self.gallery_store.create(&path, gallery_mode.clone()) {
                Ok(r) => r,
                Err(GalleryStoreError::AlreadyExists(_)) => {
                    // Re-scan existing gallery
                    match self.rescan_existing_gallery(recursive) {
                        Ok((gid, old_count, added, total, persisted)) => {
                            let result = serde_json::json!({
                                "status": "rescanned",
                                "gallery_id": gid,
                                "root_path": path,
                                "mode": mode,
                                "images_added": added,
                                "total_images": total,
                                "persisted": persisted,
                            });

                            if auto_analyze && added > 0 {
                                let new_indices: Vec<usize> = (old_count as usize
                                    ..(old_count as usize + added as usize))
                                    .collect();
                                let pipelines: Vec<String> =
                                    vec!["faces", "objects", "colors", "composition", "scene"]
                                        .into_iter()
                                        .map(|s| s.to_string())
                                        .collect();
                                let (analyzed, analyze_errors) =
                                    self.run_analysis_on_indices(&new_indices, &pipelines).await;
                                let mut r = result;
                                r["auto_analyzed"] = serde_json::json!(analyzed);
                                if !analyze_errors.is_empty() {
                                    r["analyze_errors"] = serde_json::json!(analyze_errors);
                                }
                                return Ok(r);
                            }

                            return Ok(result);
                        }
                        Err(e) => {
                            return Ok(serde_json::json!({
                                "status": "already_exists",
                                "message": e.to_string(),
                            }));
                        }
                    }
                }
                Err(e) => {
                    return Err(McpToolError::internal(format!(
                        "Failed to create gallery: {}",
                        e
                    )));
                }
            };

            // Set up in-memory GalleryState
            let mut state = GalleryState::new(PathBuf::from(&path), gallery_mode.clone());
            state.validate().map_err(map_media_error)?;
            state.ensure_meta_dir().map_err(map_media_error)?;
            state.gallery_id = Some(record.id.clone());

            // Scan for images
            let scan_result = state.scan(recursive, None);
            let mut persisted = 0u32;
            for entry in &scan_result.entries {
                let abs_path = state.path.join(&entry.relative_path);
                if self
                    .gallery_store
                    .add_image(
                        &record.id,
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

            {
                let mut guard = self.gallery_state.lock().map_err(|e| {
                    McpToolError::internal(format!("Gallery state lock error: {}", e))
                })?;
                *guard = Some(state);
            }

            let result = serde_json::json!({
                "status": "organized",
                "gallery_id": record.id,
                "root_path": record.root_path,
                "mode": record.mode,
                "images_found": scan_result.added,
                "total_images": scan_result.total,
                "persisted": persisted,
                "message": "Gallery ready. Use gallery_search to find photos by content."
            });

            if auto_analyze && scan_result.added > 0 {
                let new_indices: Vec<usize> = (0..scan_result.added as usize).collect();
                let pipelines: Vec<String> =
                    vec!["faces", "objects", "colors", "composition", "scene"]
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();
                let (analyzed, analyze_errors) =
                    self.run_analysis_on_indices(&new_indices, &pipelines).await;
                let mut r = result;
                r["auto_analyzed"] = serde_json::json!(analyzed);
                if !analyze_errors.is_empty() {
                    r["analyze_errors"] = serde_json::json!(analyze_errors);
                }
                Ok(r)
            } else {
                Ok(result)
            }
        })
        .await
    }

    #[tool(description = "Get gallery status: path, mode, image count, and total size.")]
    pub async fn gallery_status(&self) -> String {
        execute_tool(self, "gallery_status", async {
            match self.access_gallery() {
                Ok(ga) => Ok(serde_json::json!({
                    "gallery_id": ga.gallery_id,
                    "image_count": ga.image_count,
                    "root_path": ga.root_path.display().to_string(),
                })),
                Err(e) => Ok(serde_json::json!({
                    "status": "no_gallery",
                    "message": e.to_string(),
                })),
            }
        })
        .await
    }

    #[tool(
        description = "Search your gallery by describing what you're looking for. Fuzzy-matches against AI-generated tags (objects, faces, colors, composition)."
    )]
    pub async fn gallery_search(
        &self,
        Parameters(GallerySearchRequest {
            query,
            limit,
            tag_types,
            min_similarity,
        }): Parameters<GallerySearchRequest>,
    ) -> String {
        execute_tool(self, "gallery_search", async {
            if query.trim().is_empty() {
                return Err(McpToolError::invalid_argument("query must not be empty"));
            }
            let ga = self.access_gallery().map_err(map_media_error)?;

            let all_tags = self
                .gallery_store
                .get_all_tags(&ga.gallery_id)
                .map_err(|e| McpToolError::internal(format!("Failed to query tags: {}", e)))?;

            let limit = limit.unwrap_or(10);
            let min_sim = min_similarity.unwrap_or(0.3);
            let type_filter: Option<Vec<String>> =
                tag_types.map(|t| t.into_iter().map(|s| s.to_lowercase()).collect());

            let mut image_scores: std::collections::HashMap<String, (f64, Vec<serde_json::Value>)> =
                std::collections::HashMap::new();

            for (tag, relative_path) in &all_tags {
                if let Some(ref filter) = type_filter {
                    if !filter.contains(&tag.tag_type.to_lowercase()) {
                        continue;
                    }
                }

                let sim = levenshtein_similarity(&query, &tag.value);
                if sim < min_sim {
                    continue;
                }

                let weighted_sim = sim * tag.confidence;
                let entry = image_scores
                    .entry(relative_path.clone())
                    .or_insert((0.0, Vec::new()));
                entry.0 = entry.0.max(weighted_sim);
                entry.1.push(serde_json::json!({
                    "tag_type": tag.tag_type,
                    "value": tag.value,
                    "similarity": sim,
                    "confidence": tag.confidence,
                }));
            }

            let mut ranked: Vec<(String, f64, Vec<serde_json::Value>)> = image_scores
                .into_iter()
                .map(|(path, (score, matches))| (path, score, matches))
                .collect();
            ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            ranked.truncate(limit);

            let results: Vec<serde_json::Value> = ranked
                .into_iter()
                .map(|(path, score, matches)| {
                    serde_json::json!({
                        "image": path,
                        "score": score,
                        "matching_tags": matches,
                    })
                })
                .collect();

            Ok(serde_json::json!({
                "query": query,
                "results": results,
                "total_matches": results.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Find gallery images similar to a text description or to another image. Uses AI caption embeddings for semantic similarity (requires gallery_analyze to have been run first). Different from gallery_search which matches tags — this matches visual descriptions."
    )]
    pub async fn gallery_find_similar(
        &self,
        Parameters(GalleryFindSimilarRequest {
            text,
            image_index,
            limit,
            min_similarity,
        }): Parameters<GalleryFindSimilarRequest>,
    ) -> String {
        execute_tool(self, "gallery_find_similar", async {
            let query_label = text
                .clone()
                .unwrap_or_else(|| format!("image_index={}", image_index.unwrap_or(0)));

            if text.is_none() && image_index.is_none() {
                return Err(McpToolError::invalid_argument(
                    "Provide either 'text' or 'image_index' (not both).",
                ));
            }

            // Determine the query embedding
            let query_embedding: Vec<f32> = if let Some(ref query_text) = text {
                self.inference.embed_text(query_text, None).await.map_err(|e| {
                    McpToolError::unavailable(format!(
                        "Embedding model unavailable: {}. Configure a cloud provider.",
                        e
                    ))
                })?
            } else if let Some(idx) = image_index {
                let image_id = self.resolve_image_id(idx).map_err(map_media_error)?;
                let tags = self
                    .gallery_store
                    .get_tags(&image_id)
                    .map_err(|e| McpToolError::internal(format!("Failed to query tags: {}", e)))?;
                let captions: Vec<&str> = tags
                    .iter()
                    .filter(|t| t.tag_type == "caption")
                    .map(|t| t.value.as_str())
                    .collect();
                if captions.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "Image has no caption. Run gallery_analyze first to generate scene descriptions.",
                    ));
                }
                let caption_text = captions.join(" ");
                self.inference
                    .embed_text(&caption_text, None)
                    .await
                    .map_err(|e| McpToolError::unavailable(format!("Embedding model unavailable: {}", e)))?
            } else {
                unreachable!();
            };

            // Collect captions for all images in the gallery
            let ga = self.access_gallery().map_err(map_media_error)?;

            let all_tags = self
                .gallery_store
                .get_all_tags(&ga.gallery_id)
                .map_err(|e| McpToolError::internal(format!("Failed to query tags: {}", e)))?;

            // Group captions by image path and embed them
            let mut candidates: Vec<(String, String)> = Vec::new();
            let mut current_path = String::new();
            let mut current_captions: Vec<String> = Vec::new();
            for (tag, path) in &all_tags {
                if tag.tag_type != "caption" {
                    continue;
                }
                if path != &current_path {
                    if !current_captions.is_empty() {
                        candidates.push((std::mem::take(&mut current_path), current_captions.join(" ")));
                        current_captions.clear();
                    }
                    current_path = path.clone();
                }
                current_captions.push(tag.value.clone());
            }
            if !current_captions.is_empty() {
                candidates.push((current_path, current_captions.join(" ")));
            }

            if candidates.is_empty() {
                return Ok(serde_json::json!({
                    "query": query_label,
                    "results": [],
                    "message": "No captions found. Run gallery_analyze first.",
                }));
            }

            // Embed candidate captions individually and compute similarity
            let candidate_texts: Vec<&str> = candidates.iter().map(|(_, c)| c.as_str()).collect();
            let mut candidate_embeddings = Vec::new();
            for ct in &candidate_texts {
                match self.inference.embed_text(ct, None).await {
                    Ok(v) => candidate_embeddings.push(v),
                    Err(_) => candidate_embeddings.push(vec![]),
                }
            }

            // Compute cosine similarity and rank
            let mut scored: Vec<(String, f32)> = candidates
                .iter()
                .zip(candidate_embeddings.iter())
                .filter_map(|((path, _), emb)| {
                    if emb.is_empty() {
                        return None;
                    }
                    let sim = cosine_similarity(&query_embedding, emb);
                    if sim >= min_similarity {
                        Some((path.clone(), sim))
                    } else {
                        None
                    }
                })
                .collect();

            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);

            let results: Vec<serde_json::Value> = scored
                .into_iter()
                .map(|(path, score)| serde_json::json!({"image": path, "similarity": score}))
                .collect();

            Ok(serde_json::json!({
                "query": query_label,
                "results": results,
            }))
        })
        .await
    }

    #[tool(
        description = "Refresh the gallery: scan for new/removed images, then update all AI metadata (objects, colors, composition, scene descriptions). Face detection is OFF by default. When include_faces=true, also scans the face reference folder (~/.hkask/faces/ by default) for new reference faces, then auto-matches detected faces against the face_registry — named faces get person names instead of face_group numbers."
    )]
    pub async fn gallery_refresh(
        &self,
        Parameters(GalleryRefreshRequest {
            recursive,
            include_faces,
            max_images,
        }): Parameters<GalleryRefreshRequest>,
    ) -> String {
        execute_tool(self, "gallery_refresh", async {
            let (gid, _old_count, added, total, persisted) = self
                .rescan_existing_gallery(recursive)
                .map_err(map_media_error)?;

            let mut pipeline_names = vec!["objects", "colors", "composition", "scene"];
            if include_faces {
                pipeline_names.push("faces");
            }
            let pipelines: Vec<String> =
                pipeline_names.into_iter().map(|s| s.to_string()).collect();

            let all_indices: Vec<usize> = (0..total as usize).take(max_images).collect();
            let (analyzed, analyze_errors) =
                self.run_analysis_on_indices(&all_indices, &pipelines).await;

            let mut faces_matched = 0u32;
            let mut registry_count = 0usize;
            let mut match_errors: Vec<String> = Vec::new();
            let mut face_scan_errors: Vec<String> = Vec::new();
            let mut face_scan = serde_json::json!(null);
            if include_faces {
                // Stage 1: scan the face reference folder for new reference
                // faces. Default folder: ~/.hkask/faces/. Skipped silently if
                // the folder does not exist.
                if let Some(folder) = crate::default_face_folder() {
                    if folder.is_dir() {
                        match self.run_face_scan_folder(&folder, false).await {
                            Ok(result) => {
                                face_scan = result;
                            }
                            Err(e) => {
                                face_scan_errors.push(format!("face_scan_folder: {}", e));
                            }
                        }
                    }
                }

                // Stage 2: match detected faces against the registry.
                let ga = match self.access_gallery() {
                    Ok(ga) => ga,
                    Err(e) => {
                        return Ok(serde_json::json!({
                            "status": "refreshed",
                            "gallery_id": gid,
                            "scan": {
                                "images_added": added,
                                "total_images": total,
                                "persisted": persisted,
                            },
                            "analysis": {
                                "images_analyzed": analyzed,
                                "pipelines": pipelines,
                            },
                            "face_matching": {
                                "error": format!("{} — cannot match faces", e)
                            },
                            "errors": {
                                "analysis": analyze_errors,
                                "matching": serde_json::json!([]),
                            },
                        }));
                    }
                };

                let registry = match self.gallery_store.list_faces(Some("valid")) {
                    Ok(faces) => faces,
                    Err(e) => {
                        match_errors.push(format!("Failed to query face registry: {}", e));
                        Vec::new()
                    }
                };
                registry_count = registry.len();

                if !registry.is_empty() {
                    let (matched, errs) = self.run_face_matching(&ga, &registry).await;
                    faces_matched = matched;
                    match_errors.extend(errs);
                }
            }

            Ok(serde_json::json!({
                "status": "refreshed",
                "gallery_id": gid,
                "scan": {
                    "images_added": added,
                    "total_images": total,
                    "persisted": persisted,
                },
                "analysis": {
                    "images_analyzed": analyzed,
                    "pipelines": pipelines,
                },
                "face_scan_folder": face_scan,
                "face_matching": {
                    "faces_matched": faces_matched,
                    "registry_entries": registry_count,
                },
                "errors": {
                    "analysis": analyze_errors,
                    "face_scan": face_scan_errors,
                    "matching": match_errors,
                },
            }))
        })
        .await
    }

    // ── Image tools ──────────────────────────────────────────────────────────

    #[tool(
        description = "Describe an image in detail. Choose a style: descriptive (full scene), artistic (poetic), technical (photographic analysis), or alt_text (accessibility)."
    )]
    pub async fn describe_image(
        &self,
        Parameters(DescribeImageRequest { image_url, style }): Parameters<DescribeImageRequest>,
    ) -> String {
        execute_tool(self, "describe_image", async {
            validate_tool_url(&image_url)?;

            let style_str = style.as_deref().unwrap_or("descriptive");
            let mut vars = HashMap::new();
            vars.insert("style", style_str);
            let prompt = self
                .render_prompt("caption", &vars)
                .map_err(|e| McpToolError::internal(format!("Template render failed: {}", e)))?;

            let (vision_model, _vision_label) = self.require_vision().await?;
            let params = hkask_types::template::LLMParameters::default();
            let r = self
                .inference
                .generate_vision(&prompt, &[image_url], &params, Some(vision_model))
                .await
                .map_err(|e| {
                    McpToolError::unavailable(format!("Vision inference failed: {}", e))
                })?;

            Ok(serde_json::json!({"description": r.text.trim(), "style": style_str}))
        })
        .await
    }

    // ── Analysis tools ──────────────────────────────────────────────────────

    #[tool(
        description = "Analyze gallery images with AI: detect faces, objects, colors, composition, and generate scene descriptions. Tags are persisted and become searchable."
    )]
    pub async fn gallery_analyze(
        &self,
        Parameters(GalleryAnalyzeRequest {
            mode,
            image_indices,
            pipelines,
            max_images,
        }): Parameters<GalleryAnalyzeRequest>,
    ) -> String {
        execute_tool(self, "gallery_analyze", async {
            let ga = self.access_gallery().map_err(map_media_error)?;

            // NOTE: A benign race exists between access_gallery() snapshot and the loop below.
            // If images are added/removed concurrently, resolve_image_id may fail for an index
            // that was valid at snapshot time. These failures are silently skipped (continue),
            // producing graceful degradation: at worst, a newly-added image is missed or a
            // removed image is skipped. Holding the lock across the full analysis would block
            // concurrent operations, so we accept this trade-off.
            let indices: Vec<usize> = match mode.as_str() {
                "selection" => image_indices.unwrap_or_default(),
                "all" => (0..ga.image_count as usize).collect(),
                _ => {
                    let mut untagged = Vec::new();
                    for i in 0..ga.image_count as usize {
                        if let Ok(image_id) = self.resolve_image_id(i) {
                            match self.gallery_store.get_tags(&image_id) {
                                Ok(tags) if tags.is_empty() => untagged.push(i),
                                Ok(_) => continue,
                                Err(_) => untagged.push(i),
                            }
                        }
                    }
                    untagged
                }
            };

            let indices: Vec<usize> = indices.into_iter().take(max_images).collect();
            if indices.is_empty() {
                return Ok(serde_json::json!({
                    "status": "nothing_to_analyze",
                    "message": "No images to analyze."
                }));
            }

            let all_pipelines: Vec<String> =
                vec!["faces", "objects", "colors", "composition", "scene"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect();
            let pipelines = pipelines.unwrap_or(all_pipelines);

            let (analyzed, errors) = self.run_analysis_on_indices(&indices, &pipelines).await;

            let vision_label = self
                .resolve_vision_model()
                .await
                .map(|(_, label)| label)
                .unwrap_or("none");

            Ok(serde_json::json!({
                "status": "complete",
                "images_analyzed": analyzed,
                "total_images": indices.len(),
                "pipelines_run": pipelines,
                "model": vision_label,
                "errors": errors,
            }))
        })
        .await
    }

    #[tool(
        description = "Name a face group from gallery_analyze. Provide either a free-text 'name' or a 'face_id' from the face registry (which auto-resolves to 'First Last'). After naming, gallery_search can find photos of that person by name."
    )]
    pub async fn gallery_name_face(
        &self,
        Parameters(GalleryNameFaceRequest {
            face_group,
            name,
            face_id,
        }): Parameters<GalleryNameFaceRequest>,
    ) -> String {
        execute_tool(self, "gallery_name_face", async {
            let resolved_name = if let Some(ref fid) = face_id {
                self.gallery_store
                    .get_face(fid)
                    .map(|face| format!("{} {}", face.first_name, face.last_name))
                    .map_err(|e| {
                        McpToolError::invalid_argument(format!("Face registry ID not found: {}", e))
                    })?
            } else {
                match name {
                    Some(n) if !n.trim().is_empty() => n,
                    _ => {
                        return Err(McpToolError::invalid_argument(
                            "Either 'name' or 'face_id' must be provided.",
                        ));
                    }
                }
            };

            let ga = self.access_gallery().map_err(map_media_error)?;

            let all_tags = self
                .gallery_store
                .get_all_tags(&ga.gallery_id)
                .map_err(|e| McpToolError::internal(format!("Failed to query tags: {}", e)))?;

            let mut renamed = 0u32;
            for (tag, _path) in &all_tags {
                if tag.tag_type != "face" {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&tag.value) {
                    if parsed["face_index"].as_u64() == Some(face_group as u64) {
                        let new_value = serde_json::json!({
                            "face_index": face_group,
                            "name": resolved_name,
                        });
                        self.persist_tag(
                            &tag.image_id,
                            "face",
                            &new_value.to_string(),
                            1.0,
                            "user",
                        );
                        renamed += 1;
                    }
                }
            }

            Ok(serde_json::json!({
                "status": "named",
                "face_group": face_group,
                "name": resolved_name,
                "images_updated": renamed,
            }))
        })
        .await
    }

    // ── Face registry tools ─────────────────────────────────────────────────

    #[tool(
        description = "Validate a gallery image as a face reference for facial recognition. Checks: exactly 1 face, face coverage ≥15%, frontal pose, good lighting, no occlusion, sharp focus. Returns structured pass/fail with specific reasons."
    )]
    pub async fn face_validate(
        &self,
        Parameters(FaceValidateRequest { image_index }): Parameters<FaceValidateRequest>,
    ) -> String {
        execute_tool(self, "face_validate", async {
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            let (vision_model, _vision_label) = self.require_vision().await?;

            let validation = vision::validate_face_reference(
                &self.inference,
                &self.template_env,
                &image_url,
                Some(vision_model),
            )
            .await
            .map_err(|e| McpToolError::internal(format!("Face validation failed: {}", e)))?;

            Ok(serde_json::json!(validation))
        })
        .await
    }

    #[tool(
        description = "Register a face reference with a person's name. Auto-validates the image against 6 criteria (face count, coverage, pose, lighting, occlusion, clarity). Pass --force to skip validation and register directly as valid. Stores in the face_registry table for automatic matching during gallery_refresh."
    )]
    pub async fn face_register(
        &self,
        Parameters(FaceRegisterRequest {
            image_index,
            first_name,
            last_name,
            force,
        }): Parameters<FaceRegisterRequest>,
    ) -> String {
        execute_tool(self, "face_register", async {
            let image_id = self
                .resolve_image_id(image_index)
                .map_err(map_media_error)?;
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            let (record, validation) = self
                .register_face_from_url(&image_id, &image_url, &first_name, &last_name, "", force)
                .await?;

            Ok(serde_json::json!({
                "face_id": record.id,
                "first_name": record.first_name,
                "last_name": record.last_name,
                "status": record.status,
                "validation": validation,
                "notes": record.notes,
            }))
        })
        .await
    }

    #[tool(
        description = "Scan a folder of reference face images (with YAML sidecars) and register each in face_registry. Default: ~/.hkask/faces/"
    )]
    pub async fn face_scan_folder(
        &self,
        Parameters(FaceScanFolderRequest { folder_path, force }): Parameters<FaceScanFolderRequest>,
    ) -> String {
        execute_tool(self, "face_scan_folder", async {
            let folder = if let Some(p) = folder_path {
                std::path::PathBuf::from(p)
            } else {
                crate::default_face_folder().ok_or_else(|| {
                    McpToolError::invalid_argument(
                        "HOME not set and no folder_path provided".to_string(),
                    )
                })?
            };

            if !folder.is_dir() {
                return Err(McpToolError::invalid_argument(format!(
                    "Face folder does not exist: {}",
                    folder.display()
                )));
            }

            self.run_face_scan_folder(&folder, force).await
        })
        .await
    }

    #[tool(
        description = "List all registered faces in the face registry. Optionally filter by status: 'valid', 'rejected', or 'pending'."
    )]
    pub async fn face_list(
        &self,
        Parameters(FaceListRequest { status }): Parameters<FaceListRequest>,
    ) -> String {
        execute_tool(self, "face_list", async {
            let faces = self
                .gallery_store
                .list_faces(status.as_deref())
                .map_err(|e| McpToolError::internal(format!("Failed to list faces: {}", e)))?;

            Ok(serde_json::json!({
                "count": faces.len(),
                "faces": faces,
            }))
        })
        .await
    }

    #[tool(
        description = "Remove a face from the registry by its ID (returned by face_register or face_list)."
    )]
    pub async fn face_remove(
        &self,
        Parameters(FaceRemoveRequest { face_id }): Parameters<FaceRemoveRequest>,
    ) -> String {
        execute_tool(self, "face_remove", async {
            self.gallery_store
                .remove_face(&face_id)
                .map_err(|e| McpToolError::invalid_argument(format!("Face not found: {}", e)))?;
            Ok(serde_json::json!({
                "status": "removed",
                "face_id": face_id,
            }))
        })
        .await
    }

    #[tool(
        description = "Extract a specific object from an image using AI segmentation. Returns the isolated object as a new image."
    )]
    pub async fn extract_object(
        &self,
        Parameters(ExtractObjectRequest {
            image_index,
            object_description,
        }): Parameters<ExtractObjectRequest>,
    ) -> String {
        execute_tool(self, "extract_object", async {
            if object_description.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "object_description must not be empty",
                ));
            }
            let image_url = self
                .resolve_image_url(image_index)
                .map_err(map_media_error)?;

            self.inference
                .segment_object(&image_url, &object_description)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Object extraction failed: {}", e)))
        })
        .await
    }

    #[tool(
        description = "Organize gallery images by time period using EXIF dates. Returns images grouped by year, month, or decade."
    )]
    pub async fn gallery_timeline(
        &self,
        Parameters(GalleryTimelineRequest {
            period,
            count,
            per_period,
            search_terms,
        }): Parameters<GalleryTimelineRequest>,
    ) -> String {
        execute_tool(self, "gallery_timeline", async {
            let ga = self.access_gallery().map_err(map_media_error)?;

            let mut dated_images: Vec<(String, String)> = Vec::new();
            for idx in 0..ga.image_count as usize {
                let img = match self
                    .gallery_store
                    .get_image(&ga.gallery_id, Some(idx), None)
                {
                    Ok(i) => i,
                    Err(_) => continue,
                };

                if let Some(ref terms) = search_terms {
                    let tags = self.gallery_store.get_tags(&img.id).unwrap_or_default();
                    let matches = terms.iter().any(|term| {
                        tags.iter()
                            .any(|t| t.value.to_lowercase().contains(&term.to_lowercase()))
                    });
                    if !matches {
                        continue;
                    }
                }

                let exif = Self::extract_exif(&img.absolute_path);
                let date_str = exif
                    .get("date_taken")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                let period_key = match period.as_str() {
                    "month" => date_str.chars().take(7).collect(),
                    "decade" => date_str
                        .get(..3)
                        .map(|s| format!("{}0s", s))
                        .unwrap_or_else(|| "unknown".to_string()),
                    _ => date_str.chars().take(4).collect(),
                };

                dated_images.push((period_key, img.relative_path));
            }

            let mut periods: std::collections::BTreeMap<String, Vec<String>> =
                std::collections::BTreeMap::new();
            for (key, path) in &dated_images {
                periods.entry(key.clone()).or_default().push(path.clone());
            }

            let mut result_periods: Vec<serde_json::Value> = Vec::new();
            for (key, images) in periods.iter().rev().take(count) {
                let selected: Vec<&String> = images.iter().take(per_period).collect();
                result_periods.push(serde_json::json!({
                    "period": key,
                    "total_images": images.len(),
                    "images": selected,
                }));
            }

            Ok(serde_json::json!({
                "period_type": period,
                "periods": result_periods,
            }))
        })
        .await
    }
}
