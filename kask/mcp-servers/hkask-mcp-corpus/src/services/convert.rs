//! Convert service — document conversion + directory chunking orchestration.
//!
//! Extracted from `CorpusServer::corpus_convert` (single-file OCR orchestration,
//! ~450 lines) and `CorpusServer::chunk_directory` (directory-scanning chunking)
//! in `tools/document.rs`. The shared OCR helpers (`has_ocr`, `resolve_ocr_model`,
//! `do_ocr`, `persist_pipeline_outcome`) and `index_passages` — previously methods
//! on `CorpusServer` — also live here so the service is self-contained and the OCR
//! logic is not duplicated between the service and the server.
//!
//! Follows the `AssertionsService` / `ConsolidationService` pattern: a service struct
//! holding the shared inference router (plus OCR pipeline + index state), async
//! methods returning `Result<Value, McpToolError>`, `#[must_use]` on the public
//! methods, and thin `#[tool]` wrappers in `tools/document.rs` that construct the
//! service and delegate.
//!
//! The directory-conversion dispatcher (`convert_directory`) stays on
//! `CorpusServer` because it recurses through the `corpus_convert` tool wrapper to
//! preserve per-file Regulation spans; it does not call the OCR helpers directly.

use std::sync::{Arc, Mutex};

use hkask_mcp_server::server::McpToolError;
use hkask_types::InferencePort;
use serde_json::{Value, json};

use crate::backend::markdown_pages_to_structure;
use crate::convert::{decode_html_entities, detect_format, strip_html_comments};
use crate::helpers::map_corpus_io_error;
use crate::ocr::calibration::{analyze_threshold_drift, emit_drift_alert};
use crate::ocr::decimation;
use crate::ocr::pipeline::{self, OcrError, OcrExecutor};
use crate::ocr::triage::parse_target_pages;
use crate::ocr::{
    CrossValidation, PipelineExecutor, PipelineOutcome, ThresholdConfig, VerificationReport,
};
use crate::path_safety::{contain_for_read, contain_for_write};
use crate::text::{chunk_text, strip_gutenberg_headers};
use crate::{
    ExtractOutcome, IndexedPassage, OCR_FALLBACK_WORD_THRESHOLD, chunk_word_bounds,
    default_embedding_model, extract_text, filter_outcome_to_pages, ocr_concurrency,
    sanitize_links,
};
use hkask_memory::text_chunking::{filter_boilerplate_pages, has_corrupted_font_encoding};

/// Borrowed OCR + index state drawn from a `CorpusServer`.
///
/// `ConvertService` holds the cheaply-clonable state (inference router, OCR
/// model, thresholds, pipeline executor) by value and the shared mutable
/// accumulators (`cv_accumulator`, `index`) by reference, since `CorpusServer`'s
/// struct definition (the `mcp_server!` macro) cannot change to wrap them in
/// `Arc<Mutex>`. The service is short-lived: it is constructed inside a single
/// `#[tool]` call and dropped when the call returns.
pub(crate) struct ConvertService<'a> {
    inference_router: Arc<dyn InferencePort>,
    ocr_model: Option<String>,
    ocr_thresholds: ThresholdConfig,
    pipeline_executor: Arc<PipelineExecutor>,
    cv_accumulator: &'a Mutex<Vec<CrossValidation>>,
    index: &'a Mutex<Vec<IndexedPassage>>,
}

impl<'a> ConvertService<'a> {
    pub fn new(
        inference_router: Arc<dyn InferencePort>,
        ocr_model: Option<String>,
        ocr_thresholds: ThresholdConfig,
        pipeline_executor: Arc<PipelineExecutor>,
        cv_accumulator: &'a Mutex<Vec<CrossValidation>>,
        index: &'a Mutex<Vec<IndexedPassage>>,
    ) -> Self {
        Self {
            inference_router,
            ocr_model,
            ocr_thresholds,
            pipeline_executor,
            cv_accumulator,
            index,
        }
    }

    /// Construct a service borrowing a `CorpusServer`'s OCR + index state.
    ///
    /// Cheap: two `Arc::clone`s, one `Option<String>` clone, one `Copy` of the
    /// thresholds, and two shared `&Mutex` borrows. Tied to the server's borrow
    /// lifetime so the service cannot outlive the server it was built from.
    pub fn from_corpus(server: &'a crate::CorpusServer) -> Self {
        Self::new(
            Arc::clone(&server.inference_router),
            server.ocr_model.clone(),
            server.ocr_thresholds,
            Arc::clone(&server.pipeline_executor),
            &server.cv_accumulator,
            &server.index,
        )
    }

    /// Check whether OCR capability is available.
    pub fn has_ocr(&self) -> bool {
        self.ocr_model.is_some()
    }

    /// Resolve OCR model: explicit override > configured `ocr_model`.
    ///
    /// Verifies the model is a vision-capable model via the inference port's
    /// model registry. When the inference port returns empty model lists
    /// (the MediaRouter fallback when `HKASK_INFERENCE_SOCKET` is not set),
    /// this returns an error rather than silently passing the model through —
    /// a silent pass-through would let `do_ocr` fail later with a cryptic
    /// "media_generate not supported" error from the MediaRouter.
    pub async fn resolve_ocr_model(
        &self,
        override_model: Option<&str>,
    ) -> Result<String, OcrError> {
        let model = if let Some(m) = override_model
            && !m.is_empty()
        {
            m.to_string()
        } else {
            self.ocr_model.clone().ok_or(OcrError::NoModel)?
        };

        let vision_models = self
            .inference_router
            .list_vision_models()
            .await
            .map_err(|e| {
                OcrError::InferenceFailed(format!(
                    "list_vision_models failed — inference port unavailable: {e}"
                ))
            })?;

        // Empty vision list means the inference port can't enumerate models —
        // the IPC bridge is not configured (MediaRouter fallback). Fail early
        // with a diagnostic error instead of letting do_ocr fail later.
        if vision_models.is_empty() {
            return Err(OcrError::InferenceFailed(
                "No vision models available — the inference IPC bridge is not configured \
                 (HKASK_INFERENCE_SOCKET not set). The corpus MCP server must be launched \
                 by zed so it can route OCR through zed's LanguageModelRegistry. \
                 If the server was started before the inference socket was available, \
                 restart it via sync_kask_mcp_servers."
                    .to_string(),
            ));
        }

        let is_vision = vision_models.iter().any(|m| {
            m.model.eq_ignore_ascii_case(&model) || m.prefixed_name.eq_ignore_ascii_case(&model)
        });

        if !is_vision {
            let all_models = self.inference_router.list_models().await.map_err(|e| {
                OcrError::InferenceFailed(format!(
                    "list_models failed — inference port unavailable: {e}"
                ))
            })?;
            let exists = all_models.iter().any(|m| {
                m.model.eq_ignore_ascii_case(&model) || m.prefixed_name.eq_ignore_ascii_case(&model)
            });
            if exists {
                return Err(OcrError::NotVisionModel {
                    model: model.clone(),
                });
            }

            // Model not found in any list — the inference port may be a
            // MediaRouter with an empty registry, or the model name is wrong.
            // Either way, fail with a diagnostic rather than silently passing.
            return Err(OcrError::InferenceFailed(format!(
                "OCR model '{model}' not found in the inference registry. \
                 Available vision models: {}. \
                 If the list is empty, the inference IPC bridge is not configured \
                 (HKASK_INFERENCE_SOCKET not set).",
                vision_models
                    .iter()
                    .map(|m| m.prefixed_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        Ok(model)
    }

    /// Perform OCR by sending base64-encoded bytes to a vision model.
    pub async fn do_ocr(&self, file_bytes: &[u8], model: &str) -> Result<String, OcrError> {
        if file_bytes.is_empty() {
            return Err(OcrError::EmptyFile);
        }
        crate::ocr::llm_ocr::vision_ocr_bytes(&*self.inference_router, file_bytes, model).await
    }

    /// Persist pipeline outcome for Regulation observability.
    pub async fn persist_pipeline_outcome(&self, outcome: &PipelineOutcome) {
        let data = serde_json::json!({
            "total_pages": outcome.results.len(),
            "error_count": outcome.errors.len(),
            "verification_passed": outcome.report.passed,
            "page_count_match": outcome.report.page_count_match,
            "empty_pages": outcome.report.empty_pages,
            "cross_validations": outcome.cross_validations.len(),
            "backend_distribution": outcome.results.iter()
                .fold(std::collections::HashMap::new(), |mut acc, r| {
                    *acc.entry(r.backend.label().to_string()).or_insert(0) += 1;
                    acc
                }),
        });
        tracing::debug!(
            target: "hkask.mcp.docproc.reg",
            detail = ?data,
            "Pipeline outcome recorded (no daemon — in-process only)",
        );

        self.accumulate_and_check_drift(outcome);
    }

    /// Accumulate cross-validations and check for threshold drift.
    fn accumulate_and_check_drift(&self, outcome: &PipelineOutcome) {
        let mut acc = match self.cv_accumulator.lock() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.corpus.ocr",
                    error = %e,
                    "Failed to lock CV accumulator for drift check — skipping."
                );
                return;
            }
        };
        acc.extend(outcome.cross_validations.clone());

        let synthetic_outcome = PipelineOutcome {
            results: vec![],
            report: VerificationReport::new(true, vec![], 0),
            cross_validations: acc.clone(),
            errors: vec![],
        };

        if let Some(alert) = analyze_threshold_drift(&[synthetic_outcome], &self.ocr_thresholds) {
            emit_drift_alert(&alert);
            acc.clear();
        }
    }

    /// Index passages into the in-memory vector store for later query.
    ///
    /// Embeds each passage text and stores it with metadata.
    /// Returns the number of passages indexed (0 if embedding fails).
    pub async fn index_passages(&self, passages: &[(String, String)], source_label: &str) -> usize {
        let texts: Vec<String> = passages.iter().map(|(_, t)| t.clone()).collect();
        if texts.is_empty() {
            return 0;
        }

        let model_name = std::env::var("HKASK_EMBEDDING_MODEL")
            .unwrap_or_else(|_| default_embedding_model().to_string());

        let vectors = match self.inference_router.embed(&model_name, &texts).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(target: "hkask.mcp.docproc.index", error = %e, "Failed to embed passages for indexing");
                return 0;
            }
        };

        let mut index = match self.index.lock() {
            Ok(guard) => guard,
            Err(e) => {
                tracing::warn!(
                    target: "hkask.mcp.corpus",
                    error = %e,
                    "Failed to lock index for passage indexing — skipping. \
                     The index mutex may be poisoned from a prior panic."
                );
                return 0;
            }
        };
        for (i, ((entity_ref, passage_text), embedding)) in passages.iter().zip(vectors).enumerate()
        {
            index.push(IndexedPassage {
                text: passage_text.clone(),
                metadata: serde_json::json!({
                    "entity_ref": entity_ref,
                    "source": source_label,
                    "position": i,
                }),
                embedding,
            });
        }
        passages.len()
    }

    /// Convert a single document file to text, with OCR fallback.
    ///
    /// Mirrors the former `corpus_convert` file-case body: detect format, parse
    /// `target_pages` (PDF only), and route through text extraction → selective
    /// OCR → typed OCR pipeline → raw-byte OCR, returning a JSON result with
    /// `format`, `path`, `method`, `text`, `word_count`, and pipeline diagnostics.
    ///
    /// `output` is unused here — it only applies to directory mode, which the
    /// `corpus_convert` tool wrapper dispatches to `CorpusServer::convert_directory`
    /// before calling this method.
    #[must_use = "result must be used"]
    pub async fn convert(
        &self,
        path: String,
        force_ocr: bool,
        target_pages: Option<String>,
    ) -> Result<Value, McpToolError> {
        // Contain the caller-supplied path before any read or subprocess spawn:
        // validate_path rejects `..`/control chars but NOT absolute paths, so
        // `/etc/passwd` would pass and be read. contain_for_read canonicalizes
        // and rejects anything escaping the project root (CWE-22/CWE-200).
        let resolved = contain_for_read(&path)?;

        let (format, _, _) = detect_format(&path);

        // Parse target_pages (PDF only) into a 1-based page set. Parsed once
        // here so both the force_ocr and text-extraction paths can use it.
        let target_set: Option<std::collections::HashSet<usize>> = if format == "pdf" {
            target_pages
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(
                    |s| -> Result<std::collections::HashSet<usize>, McpToolError> {
                        Ok(parse_target_pages(s)
                            .map_err(|e| McpToolError::invalid_argument(e.to_string()))?
                            .into_iter()
                            .collect())
                    },
                )
                .transpose()?
        } else {
            None
        };
        // 0-based page indices for decimation, derived from target_set.
        let target_indices = |ts: &std::collections::HashSet<usize>| -> Vec<usize> {
            let mut v: Vec<usize> = ts.iter().map(|p| p - 1).collect();
            v.sort();
            v
        };

        // Read the file from the canonicalized, contained path so a TOCTOU
        // swap between contain_for_read and the read cannot escape the root.
        let file_bytes = match std::fs::read(&resolved) {
            Ok(b) => b,
            Err(e) => {
                return Err(map_corpus_io_error(
                    e,
                    &format!("Failed to read file '{}'", path),
                ));
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

                let page_images = vec![image];
                let expected = page_images.len();
                let outcome = pipeline::run_pipeline(
                    page_images,
                    expected,
                    Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                    &self.ocr_thresholds,
                    Some(&model),
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
                let word_count = text.split_whitespace().count();
                let result = serde_json::json!({
                    "format": format, "path": path, "method": "ocr_pipeline",
                    "model": model, "text": text, "word_count": word_count,
                    "verification_passed": outcome.report.passed,
                    "page_count_match": outcome.report.page_count_match,
                    "empty_pages": outcome.report.empty_pages,
                    "error_count": outcome.errors.len(),
                });
                return Ok(result);
            }

            // Not an image — try decimation + pipeline for PDFs (72 DPI JPEG to stay within 128K token limit)
            if format == "pdf" {
                let imgs_res = if let Some(ref ts) = target_set {
                    decimation::pdf_to_images_for_pages(&resolved, 72, &target_indices(ts)).await
                } else {
                    decimation::pdf_to_images(&resolved, 72).await
                };
                match imgs_res {
                    Ok(page_images) => {
                        let model = match self.resolve_ocr_model(None).await {
                            Ok(m) => m,
                            Err(guidance) => {
                                return Err(McpToolError::failed_precondition(
                                    guidance.to_string(),
                                ));
                            }
                        };
                        let expected = page_images.len();
                        let outcome = pipeline::run_pipeline(
                            page_images,
                            expected,
                            Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                            &self.ocr_thresholds,
                            Some(&model),
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
                        let structure = markdown_pages_to_structure(
                            outcome
                                .results
                                .iter()
                                .map(|r| (r.page_index + 1, r.text.clone())),
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
                        return Ok(result);
                    }
                    Err(e) => {
                        tracing::warn!(target: "hkask.docproc", error = %e, "Decimation failed — falling back to raw bytes OCR");
                    }
                }
            }

            // Final fallback: raw bytes OCR
            match self.resolve_ocr_model(None).await {
                Ok(model) => match self.do_ocr(&file_bytes, &model).await {
                    Ok(text) => {
                        let result = serde_json::json!({
                            "format": format,
                            "path": path,
                            "method": "ocr",
                            "model": model,
                            "text": text,
                            "word_count": text.split_whitespace().count(),
                        });
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
            if let ExtractOutcome::Success {
                ref text,
                word_count,
                ..
            } = quick_result
                && word_count >= OCR_FALLBACK_WORD_THRESHOLD
                && !has_corrupted_font_encoding(text)
            {
                let result = serde_json::json!({
                    "format": format, "path": path,
                    "method": "text_extraction", "text": text, "word_count": word_count,
                });
                return Ok(result);
            }

            // Quality-based OCR fallback: pdftotext succeeded by word count,
            // but the text has control characters from broken font encoding
            // (PDFs with custom ToUnicode CMaps that map character codes to
            // wrong glyphs). Log and fall through to OCR instead of accepting garbage.
            if let ExtractOutcome::Success {
                ref text,
                word_count,
                ..
            } = quick_result
                && word_count >= OCR_FALLBACK_WORD_THRESHOLD
                && has_corrupted_font_encoding(text)
            {
                tracing::warn!(
                    target: "hkask.docproc",
                    path = %path,
                    word_count,
                    "pdftotext output has corrupted font encoding — falling through to OCR",
                );
                // Fall through to the OCR paths below by NOT returning early.
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
                        let outcome = pipeline::run_pipeline(
                            page_images,
                            expected,
                            Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                            &self.ocr_thresholds,
                            Some(&model),
                            Some(ocr_concurrency()),
                        )
                        .await;
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
                        let structure = markdown_pages_to_structure(
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
                    decimation::pdf_to_images_for_pages(&resolved, 72, &target_indices(ts)).await
                } else {
                    decimation::pdf_to_images(&resolved, 72).await
                };
                match imgs_res {
                    Ok(page_images) => {
                        let expected = page_images.len();
                        let outcome = pipeline::run_pipeline(
                            page_images,
                            expected,
                            Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
                            &self.ocr_thresholds,
                            Some(&model),
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
                        let word_count = text.split_whitespace().count();
                        let structure = markdown_pages_to_structure(
                            outcome
                                .results
                                .iter()
                                .map(|r| (r.page_index + 1, r.text.clone())),
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
            ExtractOutcome::Success {
                text,
                word_count,
                structure,
            } => {
                let mut result = serde_json::json!({
                    "format": format,
                    "path": path,
                    "method": "text_extraction",
                    "text": text,
                    "word_count": word_count,
                });
                if let Some(doc_structure) = structure {
                    result["structure"] =
                        serde_json::to_value(&doc_structure).unwrap_or(serde_json::Value::Null);
                    result["block_count"] = serde_json::json!(
                        doc_structure
                            .pages
                            .iter()
                            .map(|p| p.blocks.len())
                            .sum::<usize>()
                    );
                }
                Ok(result)
            }
            ExtractOutcome::NeedsOcr {
                partial_text,
                word_count,
            } => {
                // Fall back to OCR — re-read file bytes for do_ocr
                let file_bytes = std::fs::read(&path).map_err(|e| {
                    map_corpus_io_error(e, &format!("Failed to read file '{}' for OCR", path))
                })?;
                match self.resolve_ocr_model(None).await {
                    Ok(model) => match self.do_ocr(&file_bytes, &model).await {
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
                    },
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
    }

    /// Chunk every `.txt` file in a directory into a single JSONL output.
    ///
    /// Mirrors the former `chunk_directory` helper: validate + contain paths,
    /// scan for `.txt` sources, chunk each with the configured token bounds,
    /// optionally index passages, and atomically publish the JSONL via a
    /// `.tmp` rename. Returns a summary JSON (`input_dir`, `output`,
    /// `total_documents`, `total_chunks`, `indexed`).
    #[must_use = "result must be used"]
    pub async fn chunk_directory(
        &self,
        input_dir: &str,
        output: Option<&str>,
        entity_ref_prefix: &str,
        max_tokens: Option<usize>,
        overlap_tokens: Option<usize>,
        strip_gutenberg: Option<bool>,
        index: bool,
    ) -> Result<Value, McpToolError> {
        hkask_mcp_server::validate_path("input_dir", input_dir, 4096)
            .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
        let input_dir = contain_for_read(input_dir)?;
        let output = output.ok_or_else(|| {
            McpToolError::invalid_argument("'output' is required with 'input_dir'")
        })?;
        hkask_mcp_server::validate_path("output", output, 4096)
            .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
        let output_path = contain_for_write(output)?;

        let mut sources = std::fs::read_dir(&input_dir)
            .map_err(|e| {
                map_corpus_io_error(e, &format!("Failed to read '{}'", input_dir.display()))
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "txt"))
            .collect::<Vec<_>>();
        sources.sort();
        if sources.is_empty() {
            return Err(McpToolError::invalid_argument(format!(
                "Directory '{}' contains no .txt files",
                input_dir.display()
            )));
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                map_corpus_io_error(e, &format!("Failed to create '{}'", parent.display()))
            })?;
        }
        let temp_path = std::path::PathBuf::from(format!("{}.tmp", output_path.display()));
        let file = std::fs::File::create(&temp_path).map_err(|e| {
            map_corpus_io_error(e, &format!("Failed to create '{}'", temp_path.display()))
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
            // format detection and OCR are handled by corpus_convert.
            let source_text = std::fs::read_to_string(source).map_err(|e| {
                map_corpus_io_error(e, &format!("Failed to read '{}'", source.display()))
            })?;

            // Apply Gutenberg stripping if requested
            let processed = if strip_gutenberg.unwrap_or(false) {
                strip_gutenberg_headers(&source_text)
            } else {
                source_text
            };
            let processed = sanitize_links(&processed);
            let processed = decode_html_entities(&processed);
            let processed = strip_html_comments(&processed);
            let processed = filter_boilerplate_pages(&processed);

            let passages = chunk_text(&processed, &source_prefix, min_words, max_words, ".!? ");

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
                    McpToolError::internal(format!("Failed to serialize chunk: {e}")) // rr0044-ok: serde serialization of own struct
                })?;
                writer
                    .write_all(b"\n")
                    .map_err(|e| map_corpus_io_error(e, "Failed to write chunks"))?;
                total_chunks += 1;
            }
        }

        use std::io::Write as _;
        writer.flush().map_err(|e| {
            map_corpus_io_error(e, &format!("Failed to flush '{}'", temp_path.display()))
        })?;
        std::fs::rename(&temp_path, &output_path).map_err(|e| {
            map_corpus_io_error(
                e,
                &format!(
                    "Failed to publish '{}' as '{}'",
                    temp_path.display(),
                    output_path.display()
                ),
            )
        })?;

        Ok(json!({
            "input_dir": input_dir.display().to_string(),
            "output": output_path.display().to_string(),
            "total_documents": sources.len(),
            "total_chunks": total_chunks,
            "indexed": indexed,
        }))
    }
}
