//! OCR server methods — DocProcServer impl block for OCR pipeline operations.
//!
//! Extracted from lib.rs to co-locate OCR-specific code with the ocr/ module.
//! These methods are on `DocProcServer` but are only called by `docproc_convert`
//! and `docproc_ocr` (in tools/document.rs).

use crate::ocr::calibration::{analyze_threshold_drift, emit_drift_alert};
use crate::ocr::llm_ocr::LlmOcrExecutor;
use crate::ocr::pipeline::{self, OcrError, OcrExecutor};
use crate::ocr::tesseract::TesseractExecutor;
use crate::ocr::{OcrBackend, OcrResult};
use crate::*;
use async_trait::async_trait;

/// Shareable OCR executor that bundles Tesseract + LLM backends.
///
/// Created once per server and passed as `Arc<dyn OcrExecutor>` to the pipeline.
/// This avoids the lifetime issues of passing `&DocProcServer` to parallel tasks.
pub struct PipelineExecutor {
    llm_ocr: Arc<LlmOcrExecutor>,
}

impl PipelineExecutor {
    pub fn new(llm_ocr: Arc<LlmOcrExecutor>) -> Self {
        Self { llm_ocr }
    }
}

#[async_trait]
impl OcrExecutor for PipelineExecutor {
    fn is_available(&self, backend: &OcrBackend) -> bool {
        match backend {
            OcrBackend::Tesseract => TesseractExecutor::new().is_available(backend),
            OcrBackend::LlmOcr(_) => self.llm_ocr.is_available(backend),
        }
    }

    async fn execute(
        &self,
        page_index: usize,
        backend: &OcrBackend,
        image: &image::DynamicImage,
        is_fallback: bool,
    ) -> Result<OcrResult, OcrError> {
        static TESSERACT: std::sync::LazyLock<TesseractExecutor> =
            std::sync::LazyLock::new(TesseractExecutor::new);

        match backend {
            OcrBackend::Tesseract => {
                TESSERACT
                    .execute(page_index, backend, image, is_fallback)
                    .await
            }
            _ => {
                self.llm_ocr
                    .execute(page_index, backend, image, is_fallback)
                    .await
            }
        }
    }
}

impl DocProcServer {
    /// Run the OCR pipeline on page images and return joined text + outcome.
    ///
    /// Consolidates 3 duplicated invocation blocks in `docproc_convert`
    /// (Candidate 1 — architectural deepening). Handles embedding router
    /// construction, pipeline execution, persistence, and text joining.
    pub async fn run_ocr_pipeline(
        &self,
        page_images: Vec<image::DynamicImage>,
        model: &str,
    ) -> (String, usize, crate::ocr::PipelineOutcome) {
        let expected = page_images.len();
        let emb_model = default_embedding_model();
        let emb = self.embedding_router.as_ref().map(|r| (r, emb_model));

        let outcome = pipeline::run_pipeline(
            page_images,
            expected,
            Arc::clone(&self.pipeline_executor) as Arc<dyn OcrExecutor>,
            &self.ocr_thresholds,
            Some(model),
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
        let word_count = text.split_whitespace().count();

        (text, word_count, outcome)
    }

    /// Persist pipeline outcome to daemon for Regulation observability.
    pub async fn persist_pipeline_outcome(&self, outcome: &crate::ocr::PipelineOutcome) {
        if let Some(ref daemon) = self.daemon {
            let daemon_clone = daemon.clone();
            let userpod = self.userpod.clone();
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
            tokio::spawn(async move {
                match daemon_clone
                    .store_experience(&userpod, "ocr_pipeline", "verification", &data, Some(0.85))
                    .await
                {
                    Ok(hkask_mcp_server::DaemonResponse::StoreResponse {
                        stored: true, ..
                    }) => {
                        tracing::debug!(target: "hkask.mcp.docproc.reg", "Pipeline outcome persisted to daemon");
                    }
                    Ok(other) => {
                        tracing::warn!(target: "hkask.mcp.docproc.reg", response = ?other, "Unexpected daemon response");
                    }
                    Err(e) => {
                        tracing::warn!(target: "hkask.mcp.docproc.reg", error = %e, "Failed to persist pipeline outcome");
                    }
                }
            });
        }

        self.accumulate_and_check_drift(outcome);
    }

    /// Accumulate cross-validations and check for threshold drift.
    fn accumulate_and_check_drift(&self, outcome: &crate::ocr::PipelineOutcome) {
        let mut acc = self
            .cv_accumulator
            .lock()
            .expect("Failed to lock CV accumulator for drift check");
        acc.extend(outcome.cross_validations.clone());

        let synthetic_outcome = crate::ocr::PipelineOutcome {
            results: vec![],
            report: crate::ocr::VerificationReport::new(true, vec![], 0),
            cross_validations: acc.clone(),
            errors: vec![],
        };

        if let Some(alert) = analyze_threshold_drift(&[synthetic_outcome], &self.ocr_thresholds) {
            emit_drift_alert(&alert);
            acc.clear();
        }
    }

    /// Resolve OCR model: explicit override > HKASK_OCR_MODEL env.
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

        let vision_models = self.inference_router.list_vision_models().await;
        let is_vision = vision_models
            .iter()
            .any(|m| m.model == model || m.prefixed_name == model);

        if !is_vision {
            let all_models = self.inference_router.list_models().await;
            let exists = all_models
                .iter()
                .any(|m| m.model == model || m.prefixed_name == model);
            if exists {
                return Err(OcrError::NotVisionModel {
                    model: model.clone(),
                });
            }
        }

        Ok(model)
    }

    /// Perform OCR by sending base64-encoded bytes to a vision model.
    pub async fn do_ocr(
        &self,
        file_bytes: &[u8],
        model: &str,
        max_tokens: u32,
    ) -> Result<String, OcrError> {
        if file_bytes.is_empty() {
            return Err(OcrError::EmptyFile);
        }

        let b64_data =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, file_bytes);

        let params = LLMParameters {
            temperature: 0.1,
            max_tokens,
            ..Default::default()
        };

        let result = self
            .inference_router
            .generate_vision(
                &crate::ocr::llm_ocr::build_ocr_prompt(None),
                &[b64_data],
                &params,
                Some(model),
            )
            .await
            .map_err(|e| OcrError::InferenceFailed(e.to_string()))?;

        Ok(result.text)
    }
}
