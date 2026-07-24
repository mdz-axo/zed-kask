//! Document processing tools — convert, OCR, chunk.
use crate::ocr::pipeline::{self, OcrExecutor};
use crate::*;
use schemars::JsonSchema;
use serde::Deserialize;

#[tool_router(router = document_router, vis = "pub")]
impl DocProcServer {
    #[tool(
        description = "Extract text from a document or directory. Detects format and automatically falls back to OCR for scanned PDFs. Directory conversion requires an output directory, persists one .txt file per supported source, and resumes non-empty outputs."
    )]
    pub async fn docproc_convert(
        &self,
        Parameters(ConvertRequest {
            path,
            output,
            force_ocr,
            target_pages,
        }): Parameters<ConvertRequest>,
    ) -> String {
        if std::path::Path::new(&path).is_dir() {
            return self
                .convert_directory(&path, output.as_deref(), force_ocr)
                .await;
        }

        execute_tool(self, "docproc_convert", async {
            let path_clone = path.clone();
            hkask_mcp_server::validate_path("path", &path, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;

            let (format, _, _) = convert::detect_format(&path);

            // Parse target_pages (PDF only) into a 1-based page set. Parsed once
            // here so both the force_ocr and text-extraction paths can use it.
            let target_set: Option<std::collections::HashSet<usize>> =
                if format == "pdf" {
                    target_pages.as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| -> Result<std::collections::HashSet<usize>, McpToolError> {
                            Ok(crate::ocr::triage::parse_target_pages(s)
                                .map_err(|e| McpToolError::invalid_argument(e.to_string()))?
                                .into_iter().collect())
                        })
                        .transpose()?
                } else { None };
            // 0-based page indices for decimation, derived from target_set.
            let target_indices = |ts: &std::collections::HashSet<usize>| -> Vec<usize> {
                let mut v: Vec<usize> = ts.iter().map(|p| p - 1).collect();
                v.sort();
                v
            };

            // Read the file
            let file_bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    return Err(McpToolError::internal(format!(
                        "Failed to read file '{}': {}",
                        path, e
                    )));
                }
            };

            if file_bytes.is_empty() {
                return Err(McpToolError::invalid_argument(format!(
                    "File '{}' is empty",
                    path
                )));
            }

            // When force_ocr is set, skip text extraction entirely.
            if force_ocr {
                if let Ok(image) = image::load_from_memory(&file_bytes) {
                    let model = match self.resolve_ocr_model(None).await {
                        Ok(m) => m,
                        Err(guidance) => {
                            return Err(McpToolError::failed_precondition(guidance.to_string()));
                        }
                    };

                    let (text, word_count, outcome) = self.run_ocr_pipeline(vec![image], &model).await;
                    let result = serde_json::json!({
                        "format": format, "path": path, "method": "ocr_pipeline",
                        "model": model, "text": text, "word_count": word_count,
                        "verification_passed": outcome.report.passed,
                        "page_count_match": outcome.report.page_count_match,
                        "empty_pages": outcome.report.empty_pages,
                        "error_count": outcome.errors.len(),
                    });
                    self.record_experience("docproc_convert", &path_clone, "success", result.clone());
                    return Ok(result);
                }

                // Not an image — try decimation + pipeline for PDFs (72 DPI JPEG to stay within 128K token limit)
                if format == "pdf" {
                    let imgs_res = if let Some(ref ts) = target_set {
                        decimation::pdf_to_images_for_pages(std::path::Path::new(&path), 72, &target_indices(ts)).await
                    } else {
                        decimation::pdf_to_images(std::path::Path::new(&path), 72).await
                    };
                    match imgs_res {
                        Ok(page_images) => {
                            let model = match self.resolve_ocr_model(None).await {
                                Ok(m) => m,
                                Err(guidance) => {
                                    return Err(McpToolError::failed_precondition(guidance.to_string()));
                                }
                            };
                            let expected = page_images.len();
                            let emb = self.embedding_router.as_ref().map(|r| {
                                (r, default_embedding_model())
                            });
                            let outcome = pipeline::run_pipeline(
                                page_images,
                                expected,
                                Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                                &self.ocr_thresholds,
                                Some(&model),
                                emb,
                                Some(ocr_concurrency()),
                            )
                            .await;
                            self.persist_pipeline_outcome(&outcome).await;
                            let text = outcome
                                .results
                                .iter()
                                .map(|r| r.text.as_str())
                                .collect::<Vec<_>>()
                                .join("\n\n");
                            let structure = crate::backend::markdown_pages_to_structure(
                                outcome.results.iter().map(|r| (r.page_index + 1, r.text.clone())),
                                "pdf",
                            );
                            let result = serde_json::json!({
                                "format": format, "path": path, "method": "ocr_pipeline",
                                "model": model, "text": text,
                                "word_count": text.split_whitespace().count(),
                                "pages": expected,
                                "block_count": structure.pages.iter().map(|p| p.blocks.len()).sum::<usize>(),
                                "structure": serde_json::to_value(&structure).unwrap_or(serde_json::Value::Null),
                                "verification_passed": outcome.report.passed,
                                "page_count_match": outcome.report.page_count_match,
                                "empty_pages": outcome.report.empty_pages,
                                "error_count": outcome.errors.len(),
                            });
                            self.record_experience(
                                "docproc_convert",
                                &path_clone,
                                "success",
                                result.clone(),
                            );
                            return Ok(result);
                        }
                        Err(e) => {
                            tracing::warn!(target: "hkask.docproc", error = %e, "Decimation failed — falling back to raw bytes OCR");
                        }
                    }
                }

                // Final fallback: raw bytes OCR
                match self.resolve_ocr_model(None).await {
                    Ok(model) => match self
                        .do_ocr(&file_bytes, &model, default_ocr_max_tokens())
                        .await
                    {
                        Ok(text) => {
                            let result = serde_json::json!({
                                "format": format,
                                "path": path,
                                "method": "ocr",
                                "model": model,
                                "text": text,
                                "word_count": text.split_whitespace().count(),
                            });
                            self.record_experience(
                                "docproc_convert",
                                &path_clone,
                                "success",
                                result.clone(),
                            );
                            return Ok(result);
                        }
                        Err(e) => {
                            return Err(McpToolError::unavailable(e.to_string()));
                        }
                    },
                    Err(guidance) => {
                        return Err(McpToolError::failed_precondition(guidance.to_string()));
                    }
                }
            }

            // ── Text extraction path ──
            // GAP-10/C6: Try fast text extraction first for PDFs before the expensive
            // typed OCR pipeline. For text-native PDFs (searchable, well-formed),
            // this returns in ~50ms instead of ~45s for a 300-page document.
            // Only fall back to the pipeline when text extraction is insufficient.
            //
            // `pdf_extract_result` caches the first extraction to avoid calling
            // extract_text() twice on the slow path (B1 audit fix).
            let mut pdf_extract_result: Option<ExtractOutcome> = None;
            if format == "pdf" {
                let mut quick_result = extract_text(&path).await?;
                if let Some(ref ts) = target_set {
                    quick_result = filter_outcome_to_pages(quick_result, ts);
                }
                let quick_result = quick_result;
                if let ExtractOutcome::Success { ref text, word_count, .. } = quick_result
                    && word_count >= OCR_FALLBACK_WORD_THRESHOLD
                {
                    let result = serde_json::json!({
                        "format": format, "path": path,
                        "method": "text_extraction", "text": text, "word_count": word_count,
                    });
                    self.record_experience("docproc_convert", &path_clone, "success", result.clone());
                    return Ok(result);
                }

                // Per-page triage found a mix of text-native + OCR-needing pages.
                // Selective OCR (Tier 1): decimate only the flagged pages, run the
                // pipeline on those, and interleave with native text in page
                // order. Avoids re-OCRing text-native pages.
                if let ExtractOutcome::PartialOcr {
                    ref page_texts,
                    ref ocr_pages,
                    ref verdicts,
                    ..
                } = quick_result
                    && !ocr_pages.is_empty()
                    && self.has_ocr()
                    && let Ok(model) = self.resolve_ocr_model(None).await
                {
                    match decimation::pdf_to_images_for_pages(
                        std::path::Path::new(&path),
                        72,
                        ocr_pages,
                    )
                    .await
                    {
                        Ok(page_images) if !page_images.is_empty() => {
                            let expected = page_images.len();
                            let emb = self.embedding_router.as_ref().map(|r| {
                                (r, default_embedding_model())
                            });
                            let outcome = pipeline::run_pipeline(
                                page_images, expected,
                                Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                                &self.ocr_thresholds, Some(&model), emb,
                                Some(ocr_concurrency()),
                            ).await;
                            self.persist_pipeline_outcome(&outcome).await;
                            let mut per_page: Vec<String> = page_texts.clone();
                            for (k, result) in outcome.results.iter().enumerate() {
                                if let Some(&page_idx) = ocr_pages.get(k)
                                    && page_idx < per_page.len()
                                {
                                    per_page[page_idx] = result.text.clone();
                                }
                            }
                            let text = per_page.join("\n\n");
                            let word_count = text.split_whitespace().count();
                            let structure = crate::backend::markdown_pages_to_structure(
                                per_page.iter().enumerate().map(|(i, t)| (i + 1, t.clone())),
                                "pdf",
                            );
                            let triage_summary: Vec<serde_json::Value> = verdicts
                                .iter()
                                .filter(|v| v.needs_ocr)
                                .map(|v| serde_json::json!({
                                    "page": v.page_number,
                                    "reasons": v.reasons.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
                                }))
                                .collect();
                            let result = serde_json::json!({
                                "format": format, "path": path, "method": "selective_ocr",
                                "model": model, "text": text, "word_count": word_count,
                                "pages": page_texts.len(),
                                "ocr_pages": ocr_pages.len(),
                                "block_count": structure.pages.iter().map(|p| p.blocks.len()).sum::<usize>(),
                                "structure": serde_json::to_value(&structure).unwrap_or(serde_json::Value::Null),
                                "triage": triage_summary,
                                "verification_passed": outcome.report.passed,
                                "page_count_match": outcome.report.page_count_match,
                                "empty_pages": outcome.report.empty_pages,
                                "error_count": outcome.errors.len(),
                            });
                            self.record_experience(
                                "docproc_convert",
                                &path_clone,
                                "success",
                                result.clone(),
                            );
                            return Ok(result);
                        }
                        Ok(_) => {
                            tracing::warn!(
                                target: "hkask.docproc",
                                "selective OCR rendered no pages — falling back to whole-doc path"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "hkask.docproc",
                                error = %e,
                                "selective decimation failed — falling back to whole-doc pipeline"
                            );
                        }
                    }
                }

                // Insufficient text — try the typed OCR pipeline (72 DPI JPEG to stay within 128K token limit)
                if self.has_ocr()
                    && let Ok(model) = self.resolve_ocr_model(None).await
                {
                    let imgs_res = if let Some(ref ts) = target_set {
                        decimation::pdf_to_images_for_pages(std::path::Path::new(&path), 72, &target_indices(ts)).await
                    } else {
                        decimation::pdf_to_images(std::path::Path::new(&path), 72).await
                    };
                    match imgs_res {
                        Ok(page_images) => {
                        let expected = page_images.len();
                        let emb = self.embedding_router.as_ref().map(|r| {
                            (r, default_embedding_model())
                        });
                        let outcome = pipeline::run_pipeline(
                            page_images, expected,
                            Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                            &self.ocr_thresholds, Some(&model), emb,
                            Some(ocr_concurrency()),
                        ).await;
                        self.persist_pipeline_outcome(&outcome).await;
                        let text = outcome.results.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n\n");
                        let word_count = text.split_whitespace().count();
                        let structure = crate::backend::markdown_pages_to_structure(
                            outcome.results.iter().map(|r| (r.page_index + 1, r.text.clone())),
                            "pdf",
                        );
                        let result = serde_json::json!({
                            "format": format, "path": path, "method": "ocr_pipeline",
                            "model": model, "text": text, "word_count": word_count,
                            "block_count": structure.pages.iter().map(|p| p.blocks.len()).sum::<usize>(),
                            "structure": serde_json::to_value(&structure).unwrap_or(serde_json::Value::Null),
                            "pages": expected,
                            "verification_passed": outcome.report.passed,
                            "page_count_match": outcome.report.page_count_match,
                            "empty_pages": outcome.report.empty_pages,
                            "error_count": outcome.errors.len(),
                            "cross_validations": outcome.cross_validations.len(),
                        });
                    self.record_experience("docproc_convert", &path_clone, "success", result.clone());
                    return Ok(result);
                        }
                        Err(e) => {
                            tracing::warn!(target: "hkask.docproc", error = %e, "Decimation failed — falling back to generic OCR");
                        }
                    }
                }

                // Pipeline unavailable or failed — reuse the cached extraction result
                pdf_extract_result = Some(quick_result);
            }

            let extract_result = if let Some(cached) = pdf_extract_result {
                cached
            } else {
                extract_text(&path).await?
            };

            match extract_result {
                ExtractOutcome::Success { text, word_count, structure } => {
                    let mut result = serde_json::json!({
                        "format": format,
                        "path": path,
                        "method": "text_extraction",
                        "text": text,
                        "word_count": word_count,
                    });
                    if let Some(doc_structure) = structure {
                        result["structure"] = serde_json::to_value(&doc_structure)
                            .unwrap_or(serde_json::Value::Null);
                        result["block_count"] = serde_json::json!(
                            doc_structure.pages.iter().map(|p| p.blocks.len()).sum::<usize>()
                        );
                    }
                    self.record_experience("docproc_convert", &path_clone, "success", result.clone());
                    Ok(result)
                }
                ExtractOutcome::NeedsOcr {
                    partial_text,
                    word_count,
                } => {
                    // Fall back to OCR — re-read file bytes for do_ocr
                    let file_bytes = std::fs::read(&path).map_err(|e| {
                        McpToolError::internal(format!("Failed to read file '{}' for OCR: {}", path, e))
                    })?;
                    match self.resolve_ocr_model(None).await {
                        Ok(model) => {
                            match self
                                .do_ocr(&file_bytes, &model, default_ocr_max_tokens())
                                .await
                            {
                                Ok(ocr_text) => {
                                    let ocr_word_count = ocr_text.split_whitespace().count();
                                    let (final_text, final_word_count, method) =
                                        if ocr_word_count > word_count {
                                            (ocr_text, ocr_word_count, "ocr")
                                        } else {
                                            (
                                                partial_text,
                                                word_count,
                                                "text_extraction_ocr_fallback_insufficient",
                                            )
                                        };
                                    let result = serde_json::json!({
                                        "format": format,
                                        "path": path,
                                        "method": method,
                                        "model": model,
                                        "text": final_text,
                                        "word_count": final_word_count,
                                        "extraction_word_count": word_count,
                                    });
                                    self.record_experience(
                                        "docproc_convert",
                                        &path_clone,
                                        "success",
                                        result.clone(),
                                    );
                                    Ok(result)
                                }
                                Err(e) => {
                                    if word_count > 0 {
                                        Ok(serde_json::json!({
                                            "format": format,
                                            "path": path,
                                            "method": "text_extraction_ocr_failed",
                                            "text": partial_text,
                                            "word_count": word_count,
                                            "ocr_error": e.to_string(),
                                        }))
                                    } else {
                                        Err(McpToolError::unavailable(format!(
                                            "Text extraction returned near-empty result and OCR failed: {}",
                                            e
                                        )))
                                    }
                                }
                            }
                        }
                        Err(guidance) => {
                            if word_count > 0 {
                                Ok(serde_json::json!({
                                    "format": format,
                                    "path": path,
                                    "method": "text_extraction_no_ocr_available",
                                    "text": partial_text,
                                    "word_count": word_count,
                                    "ocr_available": false,
                                    "ocr_guidance": guidance.to_string(),
                                }))
                            } else {
                                Err(McpToolError::failed_precondition(format!(
                                    "PDF text extraction returned no text and no OCR model is configured. {}",
                                    guidance
                                )))
                            }
                        }
                    }
                }
                ExtractOutcome::PartialOcr {
                    page_texts,
                    word_count,
                    ocr_pages,
                    verdicts: _,
                } => {
                    // Selective OCR was unavailable or decimation failed; OCR
                    // is also unavailable here. Return the native text of the
                    // text-native pages only, explicitly flagging that the OCR
                    // pages were skipped (no silent loss — the caller sees the
                    // gap).
                    let native_text = page_texts.join("\n\n");
                    Ok(serde_json::json!({
                        "format": format,
                        "path": path,
                        "method": "text_extraction_partial",
                        "text": native_text,
                        "word_count": word_count,
                        "pages": page_texts.len(),
                        "ocr_pages_skipped": ocr_pages.len(),
                        "ocr_available": false,
                    }))
                }
            }
        })
        .await
    }

    #[tool(
        description = "OCR a document using a local vision model. Requires HKASK_OCR_MODEL env var or explicit model parameter. The model must be a vision-capable model available in the inference catalog."
    )]
    pub async fn docproc_ocr(
        &self,
        Parameters(OcrRequest {
            path,
            model,
            max_tokens,
        }): Parameters<OcrRequest>,
    ) -> String {
        execute_tool(self, "docproc_ocr", async {
            let path_clone = path.clone();
            hkask_mcp_server::validate_path("path", &path, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;

            let model = match self.resolve_ocr_model(model.as_deref()).await {
                Ok(m) => m,
                Err(guidance) => {
                    return Err(McpToolError::failed_precondition(guidance.to_string()));
                }
            };

            let file_bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    return Err(McpToolError::internal(format!(
                        "Failed to read file '{}': {}",
                        path, e
                    )));
                }
            };

            match self.do_ocr(&file_bytes, &model, max_tokens).await {
                Ok(text) => {
                    let result = serde_json::json!({
                        "path": path,
                        "model": model,
                        "text": text,
                        "word_count": text.split_whitespace().count(),
                    });
                    self.record_experience("docproc_ocr", &path_clone, "success", result.clone());
                    Ok(result)
                }
                Err(e) => Err(McpToolError::unavailable(e.to_string())),
            }
        })
        .await
    }

    /// Cheaply check whether a PDF needs OCR or heavier parsing, *before*
    /// committing to a full parse. Runs a text-layer pass (`pdftotext`) plus an
    /// image inventory (`pdfimages`) -- no page rendering -- and classifies each
    /// page as text-native or needing OCR, with typed reasons.
    ///
    /// The docproc-native analogue of LiteParse's `lit is-complex`. Use it to
    /// route, reject, or estimate cost before calling `docproc_convert`.
    /// Emits `reg.pipeline.triage` spans. PDF only.
    #[tool(
        description = "Check whether a PDF needs OCR before a full parse. Returns per-page triage verdicts with typed reasons (scanned, no-text, sparse-text, embedded-images). PDF only. No page rendering -- cheap text-layer + image-inventory pass."
    )]
    pub async fn docproc_is_complex(
        &self,
        Parameters(IsComplexRequest { path, target_pages }): Parameters<IsComplexRequest>,
    ) -> String {
        execute_tool(self, "docproc_is_complex", async {
            let path_clone = path.clone();
            hkask_mcp_server::validate_path("path", &path, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
            let (format, _, _) = convert::detect_format(&path);
            if format != "pdf" {
                return Err(McpToolError::invalid_argument(
                    "docproc_is_complex supports PDF only",
                ));
            }
            let cfg = crate::ocr::TriageConfig::from_env();
            let mut verdicts = crate::ocr::triage::triage_pdf(std::path::Path::new(&path), &cfg)
                .await
                .map_err(|e| McpToolError::internal(format!("triage failed: {e}")))?;

            if let Some(spec) = target_pages.as_deref().filter(|s| !s.trim().is_empty()) {
                let target: std::collections::HashSet<usize> =
                    crate::ocr::triage::parse_target_pages(spec)
                        .map_err(|e| McpToolError::invalid_argument(e.to_string()))?
                        .into_iter()
                        .collect();
                verdicts.retain(|v| target.contains(&v.page_number));
            }

            let needs_ocr = verdicts.iter().any(|v| v.needs_ocr);
            let ocr_page_count = verdicts.iter().filter(|v| v.needs_ocr).count();
            let pages: Vec<serde_json::Value> = verdicts
                .iter()
                .map(|v| {
                    serde_json::json!({
                        "page": v.page_number,
                        "word_count": v.word_count,
                        "needs_ocr": v.needs_ocr,
                        "reasons": v.reasons.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
                    })
                })
                .collect();
            tracing::info!(
                target: "reg.pipeline.triage",
                path = path,
                pages = verdicts.len(),
                ocr_pages = ocr_page_count,
                needs_ocr,
                "is-complex triage complete"
            );
            let result = serde_json::json!({
                "path": path,
                "pages": pages,
                "page_count": verdicts.len(),
                "ocr_pages": ocr_page_count,
                "needs_ocr": needs_ocr,
            });
            self.record_experience("docproc_is_complex", &path_clone, "success", result.clone());
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Chunk text into passages at configurable token granularity. Accepts raw text or a file path (extracts text from PDF/MD/HTML/TXT with OCR fallback for scanned PDFs). Supports single-tier or multi-tier (coarse/medium/fine) output."
    )]
    pub async fn docproc_chunk(
        &self,
        Parameters(ChunkRequest {
            text,
            path,
            input_dir,
            output,
            entity_ref_prefix,
            max_tokens,
            overlap_tokens,
            strip_gutenberg,
            multi_tier,
            coarse_max_tokens,
            medium_max_tokens,
            fine_max_tokens,
            index,
            target_pages,
        }): Parameters<ChunkRequest>,
    ) -> String {
        if let Some(input_dir) = input_dir {
            return self
                .chunk_directory(
                    &input_dir,
                    output.as_deref(),
                    &entity_ref_prefix,
                    max_tokens,
                    overlap_tokens,
                    strip_gutenberg,
                    index,
                )
                .await;
        }

        execute_tool(self, "docproc_chunk", async {
            // Exactly one of text or path must be provided
            let has_text = text.as_ref().is_some_and(|t| !t.is_empty());
            let has_path = path.as_ref().is_some_and(|p| !p.is_empty());
            if has_text == has_path {
                return Err(McpToolError::invalid_argument(
                    "Exactly one of 'text' or 'path' must be provided",
                ));
            }

            if entity_ref_prefix.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "entity_ref_prefix must not be empty",
                ));
            }
            hkask_mcp_server::validate_identifier("entity_ref_prefix", &entity_ref_prefix, 256)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;

            // Resolve the source text
            let source_text: String;
            let source_label: String;
            // Structure from office-format backends — enables section-aware chunking.
            let mut source_structure: Option<hkask_types::document::DocStructure> = None;

            if let Some(ref raw_text) = text
                && !raw_text.is_empty()
            {
                source_text = raw_text.clone();
                source_label = entity_ref_prefix.clone();
            } else if let Some(ref file_path) = path
                && !file_path.is_empty()
            {
                // Use shared extract_text for format detection + text extraction
                let mut extract_outcome = extract_text(file_path).await?;
                if let Some(spec) = target_pages.as_deref().filter(|s| !s.trim().is_empty()) {
                    let target: std::collections::HashSet<usize> =
                        crate::ocr::triage::parse_target_pages(spec)
                            .map_err(|e| McpToolError::invalid_argument(e.to_string()))?
                            .into_iter()
                            .collect();
                    extract_outcome = filter_outcome_to_pages(extract_outcome, &target);
                }
                match extract_outcome {
                    ExtractOutcome::Success {
                        text: extracted,
                        structure: Some(doc_structure),
                        ..
                    } => {
                        // Preserve structure for section-aware chunking later.
                        let structure_text = doc_structure.text();
                        source_text = if structure_text.split_whitespace().count()
                            >= extracted.split_whitespace().count()
                        {
                            structure_text
                        } else {
                            extracted
                        };
                        source_structure = Some(doc_structure);
                    }
                    ExtractOutcome::Success {
                        text: extracted, ..
                    } => {
                        source_text = extracted;
                    }
                    ExtractOutcome::NeedsOcr {
                        partial_text,
                        word_count: _,
                    } => {
                        // Try OCR fallback; use partial_text if OCR unavailable/fails
                        if let Ok(model) = self.resolve_ocr_model(None).await {
                            let file_bytes = std::fs::read(file_path).map_err(|e| {
                                McpToolError::internal(format!(
                                    "Failed to read '{}': {}",
                                    file_path, e
                                ))
                            })?;
                            match self
                                .do_ocr(&file_bytes, &model, default_ocr_max_tokens())
                                .await
                            {
                                Ok(ocr_text) if !ocr_text.is_empty() => {
                                    source_structure = Some(crate::backend::markdown_to_structure(
                                        &ocr_text, "pdf",
                                    ));
                                    source_text = ocr_text;
                                }
                                _ => {
                                    source_text = partial_text;
                                }
                            }
                        } else {
                            source_text = partial_text;
                        }
                    }
                    ExtractOutcome::PartialOcr {
                        page_texts,
                        ocr_pages,
                        ..
                    } => {
                        // Mixed PDF: some pages text-native, some need OCR.
                        // Chunk's selective-OCR optimization is deferred; for
                        // now, fall back to whole-doc OCR (like NeedsOcr) so no
                        // page is silently lost. The joined native text is the
                        // fallback if OCR is unavailable.
                        let partial = page_texts.join("\n\x0c");
                        if !ocr_pages.is_empty()
                            && let Ok(model) = self.resolve_ocr_model(None).await
                        {
                            let file_bytes = std::fs::read(file_path).map_err(|e| {
                                McpToolError::internal(format!(
                                    "Failed to read '{}': {}",
                                    file_path, e
                                ))
                            })?;
                            match self
                                .do_ocr(&file_bytes, &model, default_ocr_max_tokens())
                                .await
                            {
                                Ok(ocr_text) if !ocr_text.is_empty() => {
                                    source_structure = Some(crate::backend::markdown_to_structure(
                                        &ocr_text, "pdf",
                                    ));
                                    source_text = ocr_text;
                                }
                                _ => source_text = partial,
                            }
                        } else {
                            source_text = partial;
                        }
                    }
                }
                source_label = file_path.replace(['/', '\\', '.', ' '], "_");
            } else {
                return Err(McpToolError::invalid_argument("No text or path provided"));
            }

            // Apply Gutenberg stripping if requested
            let processed = if strip_gutenberg.unwrap_or(false) {
                SemanticMemory::strip_gutenberg_headers(&source_text)
            } else {
                source_text
            };
            let processed = sanitize_links(&processed);
            let processed = crate::convert::decode_html_entities(&processed);
            let processed = crate::convert::strip_html_comments(&processed);

            let boundary = ".!? ";

            if multi_tier.unwrap_or(false) {
                // Multi-tier: coarse / medium / fine
                let chunk_tier = |tier: &str, max_tok: Option<usize>, default: usize| -> Vec<_> {
                    let w = tokens_to_words(max_tok.unwrap_or(default));
                    SemanticMemory::chunk_text(
                        &processed,
                        &format!("{source_label}:{tier}"),
                        w / 4,
                        w,
                        boundary,
                    )
                };

                let coarse = chunk_tier("coarse", coarse_max_tokens, 2048);
                let medium = chunk_tier("medium", medium_max_tokens, 512);
                let fine = chunk_tier("fine", fine_max_tokens, 128);

                let result = json!({
                    "source": source_label,
                    "multi_tier": true,
                    "coarse_max_tokens": coarse_max_tokens.unwrap_or(2048),
                    "medium_max_tokens": medium_max_tokens.unwrap_or(512),
                    "fine_max_tokens": fine_max_tokens.unwrap_or(128),
                    "coarse": serialize_passages(&coarse),
                    "medium": serialize_passages(&medium),
                    "fine": serialize_passages(&fine),
                });

                // Auto-index if requested
                let indexed = if index {
                    let all: Vec<_> = coarse.into_iter().chain(medium).chain(fine).collect();
                    self.index_passages(&all, &source_label).await
                } else {
                    0
                };

                let mut result = result;
                result["indexed"] = json!(indexed);
                self.record_experience("docproc_chunk", &source_label, "success", result.clone());
                Ok(result)
            } else {
                // Single-tier
                let (max_words, min_words) = chunk_word_bounds(max_tokens, overlap_tokens);

                // Use structure-aware chunking when a DocStructure is available
                // (office formats). Falls back to flat chunk_text otherwise.
                let passages = if let Some(ref structure) = source_structure {
                    chunk_structure(
                        structure,
                        &entity_ref_prefix,
                        min_words,
                        max_words,
                        boundary,
                    )
                } else {
                    SemanticMemory::chunk_text(
                        &processed,
                        &entity_ref_prefix,
                        min_words,
                        max_words,
                        boundary,
                    )
                };

                let total_passages = passages.len();
                let serialized = serialize_passages(&passages);

                // Auto-index if requested
                let indexed = if index {
                    self.index_passages(&passages, &source_label).await
                } else {
                    0
                };

                let result = json!({
                    "source": source_label,
                    "multi_tier": false,
                    "total_passages": total_passages,
                    "passages": serialized,
                    "max_tokens": max_tokens.unwrap_or(512),
                    "overlap_tokens": overlap_tokens.unwrap_or(64),
                    "max_words": max_words,
                    "min_words": min_words,
                    "sentence_boundary": boundary,
                    "stripped_gutenberg": strip_gutenberg.unwrap_or(false),
                    "indexed": indexed,
                });
                self.record_experience("docproc_chunk", &source_label, "success", result.clone());
                Ok(result)
            }
        })
        .await
    }
}

impl DocProcServer {
    /// expect: "The corpus pipeline uses hKask MCP servers, not external scripts."
    /// [P7] Motivating: Composable Systems — one MCP call executes the manifest's directory conversion step.
    /// pre: `path` names a readable directory and `output` names its destination directory
    /// post: each supported source has a non-empty `.txt` output or an entry in `failures`
    /// inv: existing outputs larger than 50 bytes are preserved unchanged
    /// [P3] Constraining: Generative Space — batch progress and failures remain visible in the tool result.
    async fn convert_directory(&self, path: &str, output: Option<&str>, force_ocr: bool) -> String {
        execute_tool(self, "docproc_convert", async {
            hkask_mcp_server::validate_path("path", path, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
            let output = output.ok_or_else(|| {
                McpToolError::invalid_argument(
                    "'output' directory is required when 'path' is a directory",
                )
            })?;
            hkask_mcp_server::validate_path("output", output, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;

            let output_dir = std::path::Path::new(output);
            std::fs::create_dir_all(output_dir).map_err(|e| {
                McpToolError::internal(format!(
                    "Failed to create output directory '{}': {}",
                    output, e
                ))
            })?;

            let mut sources = std::fs::read_dir(path)
                .map_err(|e| {
                    McpToolError::internal(format!("Failed to read directory '{}': {}", path, e))
                })?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|entry| entry.is_file() && is_supported_document(entry))
                .collect::<Vec<_>>();
            sources.sort();

            if sources.is_empty() {
                return Err(McpToolError::invalid_argument(format!(
                    "Directory '{}' contains no supported documents",
                    path
                )));
            }

            let mut extracted = 0usize;
            let mut skipped = 0usize;
            let mut failures = Vec::new();

            for source in &sources {
                let Some(file_name) = source.file_name() else {
                    continue;
                };
                let output_path = output_dir.join(format!("{}.txt", file_name.to_string_lossy()));
                if output_path
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() > 50)
                {
                    skipped += 1;
                    continue;
                }

                let response = Box::pin(self.docproc_convert(Parameters(ConvertRequest {
                    path: source.to_string_lossy().into_owned(),
                    output: None,
                    force_ocr,
                    target_pages: None,
                })))
                .await;

                let content = serde_json::from_str::<serde_json::Value>(&response)
                    .ok()
                    .and_then(|value| value.get("content").cloned());
                let text = content
                    .as_ref()
                    .and_then(|value| value.get("text"))
                    .and_then(serde_json::Value::as_str);

                match text {
                    Some(text) if !text.trim().is_empty() => {
                        if let Err(e) = std::fs::write(&output_path, text) {
                            failures.push(json!({
                                "path": source,
                                "error": format!("Failed to write '{}': {}", output_path.display(), e),
                            }));
                        } else {
                            extracted += 1;
                        }
                    }
                    _ => failures.push(json!({
                        "path": source,
                        "error": content
                            .unwrap_or_else(|| json!({"response": response})),
                    })),
                }
            }

            Ok(json!({
                "path": path,
                "output": output,
                "source_documents": sources.len(),
                "total_documents": extracted + skipped,
                "extracted": extracted,
                "skipped": skipped,
                "failed": failures.len(),
                "failures": failures,
            }))
        })
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn chunk_directory(
        &self,
        input_dir: &str,
        output: Option<&str>,
        entity_ref_prefix: &str,
        max_tokens: Option<usize>,
        overlap_tokens: Option<usize>,
        strip_gutenberg: Option<bool>,
        index: bool,
    ) -> String {
        execute_tool(self, "docproc_chunk", async {
            hkask_mcp_server::validate_path("input_dir", input_dir, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
            let output = output.ok_or_else(|| {
                McpToolError::invalid_argument("'output' is required with 'input_dir'")
            })?;
            hkask_mcp_server::validate_path("output", output, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;

            let mut sources = std::fs::read_dir(input_dir)
                .map_err(|e| McpToolError::internal(format!("Failed to read '{input_dir}': {e}")))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "txt"))
                .collect::<Vec<_>>();
            sources.sort();
            if sources.is_empty() {
                return Err(McpToolError::invalid_argument(format!(
                    "Directory '{input_dir}' contains no .txt files"
                )));
            }

            let output_path = std::path::Path::new(output);
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    McpToolError::internal(format!("Failed to create '{}': {e}", parent.display()))
                })?;
            }
            let temp_path = std::path::PathBuf::from(format!("{}.tmp", output_path.display()));
            let file = std::fs::File::create(&temp_path).map_err(|e| {
                McpToolError::internal(format!("Failed to create '{}': {e}", temp_path.display()))
            })?;
            let mut writer = std::io::BufWriter::new(file);
            let mut total_chunks = 0usize;
            let mut indexed = 0usize;

            let (max_words, min_words) = chunk_word_bounds(max_tokens, overlap_tokens);

            for source in &sources {
                let file_name = source
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .ok_or_else(|| McpToolError::invalid_argument("Invalid source filename"))?;
                let source_prefix = format!(
                    "{}:{}",
                    entity_ref_prefix,
                    file_name.replace(['/', '\\', '.', ' '], "_")
                );

                // Read the .txt file directly — no recursive MCP tool call.
                // chunk_directory operates on already-extracted plain text;
                // format detection and OCR are handled by docproc_convert.
                let source_text = std::fs::read_to_string(source).map_err(|e| {
                    McpToolError::internal(format!("Failed to read '{}': {}", source.display(), e))
                })?;

                // Apply Gutenberg stripping if requested
                let processed = if strip_gutenberg.unwrap_or(false) {
                    SemanticMemory::strip_gutenberg_headers(&source_text)
                } else {
                    source_text
                };
                let processed = sanitize_links(&processed);
                let processed = crate::convert::decode_html_entities(&processed);
                let processed = crate::convert::strip_html_comments(&processed);

                let passages = SemanticMemory::chunk_text(
                    &processed,
                    &source_prefix,
                    min_words,
                    max_words,
                    ".!? ",
                );

                // Index if requested
                if index {
                    let source_label = file_name.to_string();
                    indexed += self.index_passages(&passages, &source_label).await;
                }

                use std::io::Write as _;
                for (entity_ref, passage_text) in &passages {
                    let row = json!({
                        "entity_ref": entity_ref,
                        "source": file_name,
                        "text": passage_text,
                        "word_count": passage_text.split_whitespace().count(),
                    });
                    serde_json::to_writer(&mut writer, &row).map_err(|e| {
                        McpToolError::internal(format!("Failed to serialize chunk: {e}"))
                    })?;
                    writer.write_all(b"\n").map_err(|e| {
                        McpToolError::internal(format!("Failed to write chunks: {e}"))
                    })?;
                    total_chunks += 1;
                }
            }

            use std::io::Write as _;
            writer.flush().map_err(|e| {
                McpToolError::internal(format!("Failed to flush '{}': {e}", temp_path.display()))
            })?;
            std::fs::rename(&temp_path, output_path).map_err(|e| {
                McpToolError::internal(format!(
                    "Failed to publish '{}' as '{}': {e}",
                    temp_path.display(),
                    output_path.display()
                ))
            })?;

            Ok(json!({
                "input_dir": input_dir,
                "output": output,
                "total_documents": sources.len(),
                "total_chunks": total_chunks,
                "indexed": indexed,
            }))
        })
        .await
    }
}

fn is_supported_document(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "pdf" | "html" | "htm" | "md" | "txt" | "docx" | "pptx" | "xlsx" | "xls" | "csv"
            )
        })
}

// ── Request structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertRequest {
    /// Path to a document file or a directory of documents to convert.
    pub path: String,
    /// Output directory for batch conversion. Required when `path` is a directory.
    #[serde(default)]
    pub output: Option<String>,
    /// If true, skip text extraction and go directly to OCR.
    #[serde(default)]
    pub force_ocr: bool,
    /// Target pages to parse (1-based), e.g. `"1-5,10,15-20"`. PDF only.
    /// When set, pages outside the set are skipped in extraction, triage, and
    /// OCR. Mirrors LiteParse's `--target-pages`. `None` = all pages.
    #[serde(default)]
    pub target_pages: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OcrRequest {
    /// Path to the document file to OCR.
    pub path: String,
    /// Vision model to use for OCR (must be available in the inference catalog).
    #[serde(default)]
    pub model: Option<String>,
    /// Maximum tokens for OCR output.
    #[serde(default = "default_ocr_max_tokens")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IsComplexRequest {
    /// Path to the PDF file to triage.
    pub path: String,
    /// Optional target pages (1-based), e.g. "1-5,10,15-20". None = all pages.
    #[serde(default)]
    pub target_pages: Option<String>,
}

fn default_ocr_max_tokens() -> u32 {
    8192
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChunkRequest {
    /// Raw text to chunk. Mutually exclusive with `path` and `input_dir`.
    #[serde(default)]
    pub text: Option<String>,
    /// Path to a document file to extract text from and chunk.
    #[serde(default)]
    pub path: Option<String>,
    /// Directory of extracted text files to chunk as one corpus.
    #[serde(default)]
    pub input_dir: Option<String>,
    /// JSONL output path for directory mode. Required with `input_dir`.
    #[serde(default)]
    pub output: Option<String>,
    /// Prefix for entity references in chunk output.
    pub entity_ref_prefix: String,
    /// Max tokens per chunk (single-tier mode). Default: 256 from HkaskSettings.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// Overlap tokens between chunks (single-tier mode, default 64).
    #[serde(default)]
    pub overlap_tokens: Option<usize>,
    /// Strip Project Gutenberg headers from text before chunking.
    #[serde(default)]
    pub strip_gutenberg: Option<bool>,
    /// If true, produce coarse/medium/fine multi-tier output instead of single-tier.
    #[serde(default)]
    pub multi_tier: Option<bool>,
    /// Max tokens for coarse tier (multi-tier mode, default 2048).
    #[serde(default)]
    pub coarse_max_tokens: Option<usize>,
    /// Max tokens for medium tier (multi-tier mode, default 512).
    #[serde(default)]
    pub medium_max_tokens: Option<usize>,
    /// Max tokens for fine tier (multi-tier mode, default 128).
    #[serde(default)]
    pub fine_max_tokens: Option<usize>,
    /// If true, automatically index passages for later query via docproc_query (default true).
    #[serde(default = "default_true")]
    pub index: bool,
    /// Target pages to parse (1-based) when `path` is a PDF, e.g. "1-5,10,15-20".
    /// Pages outside the set are skipped before chunking. Mirrors `docproc_convert`.
    #[serde(default)]
    pub target_pages: Option<String>,
}

pub(crate) fn default_true() -> bool {
    true
}

/// Strip URLs, file links, and hyperlinks from text before chunking.
///
/// Removes HTML anchor tags (keeps inner text), Markdown URL links (keeps
/// link text), bare URLs, and protocol URIs (http, https, ftp, file, ssh,
/// mailto). Collapses leftover double spaces. Preserves newlines and
/// non-link text.
fn sanitize_links(text: &str) -> String {
    use regex::Regex;

    let re_anchor = Regex::new(r#"(?is)<a\s[^>]*>(.*?)</a>"#).expect("anchor regex");
    let re_md = Regex::new(r"\[([^\]]*)\]\((?:https?://|ftp://|file://|www\.|mailto:)[^)]*\)")
        .expect("md-link regex");
    let re_url = Regex::new(
        r#"(?:https?|ftp|file|ssh)://[^\s<>"'\)\]]+|www\.[^\s<>"'\)\]]+|mailto:[^\s<>"'\)\]]+"#,
    )
    .expect("url regex");
    let re_spaces = Regex::new(r"  +").expect("spaces regex");

    let text = re_anchor.replace_all(text, "$1");
    let text = re_md.replace_all(&text, "$1");
    let text = re_url.replace_all(&text, "");
    let text = re_spaces.replace_all(&text, " ");
    // Trim trailing spaces on each line (left by URL removal at line ends)
    text.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip newsletter/blog subscribe boilerplate from text.
#[cfg(test)]
mod tests {
    use super::sanitize_links;

    #[test]
    fn strips_bare_urls() {
        let input = "Visit https://example.com for details.";
        assert_eq!(sanitize_links(input), "Visit for details.");
    }

    #[test]
    fn strips_www_links() {
        let input = "See www.example.com and http://test.org.";
        assert_eq!(sanitize_links(input), "See and");
    }

    #[test]
    fn strips_markdown_links_keeps_text() {
        let input = "Read [this article](https://example.com) now.";
        assert_eq!(sanitize_links(input), "Read this article now.");
    }

    #[test]
    fn keeps_non_url_markdown_refs() {
        let input = "See [Figure 1](#fig1) on [page 42](page 42).";
        assert_eq!(
            sanitize_links(input),
            "See [Figure 1](#fig1) on [page 42](page 42)."
        );
    }

    #[test]
    fn strips_html_anchors_keeps_text() {
        let input = "Click <a href=\"https://example.com\">here</a> now.";
        assert_eq!(sanitize_links(input), "Click here now.");
    }

    #[test]
    fn strips_file_and_protocol_uris() {
        let input = "Open file:///etc/passwd or ftp://files.example.com/data.";
        assert_eq!(sanitize_links(input), "Open or");
    }

    #[test]
    fn preserves_newlines_and_normal_text() {
        let input = "Normal text here.\n\nAnother paragraph with no links.";
        assert_eq!(sanitize_links(input), input);
    }

    #[test]
    fn strips_mailto() {
        let input = "Contact mailto:user@example.com today.";
        assert_eq!(sanitize_links(input), "Contact today.");
    }

    #[test]
    fn collapses_double_spaces() {
        let input = "Visit  https://example.com  now.";
        assert_eq!(sanitize_links(input), "Visit now.");
    }
}
