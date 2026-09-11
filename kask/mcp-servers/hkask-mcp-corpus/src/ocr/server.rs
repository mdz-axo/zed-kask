//! OCR pipeline executor — the LLM backend.
//!
//! `corpus_convert` and `corpus_ocr` in `tools/document.rs` construct a
//! `ConvertService` and delegate to it. The former Tesseract executor was
//! removed with the backend (2026-09-10): there is one OCR path — the
//! configured vision model — and its failures are surfaced, never
//! substituted.

use crate::Arc;
use crate::ocr::OcrResult;
use crate::ocr::llm_ocr::LlmOcrExecutor;
use crate::ocr::pipeline::{OcrError, OcrExecutor};
use async_trait::async_trait;
use image::DynamicImage;

/// Shareable OCR executor wrapping the LLM backend.
///
/// Created once per server and passed as `Arc<dyn OcrExecutor>` to the pipeline.
/// This avoids the lifetime issues of passing `&CorpusServer` to parallel tasks.
pub(crate) struct PipelineExecutor {
    llm_ocr: Arc<LlmOcrExecutor>,
}

impl PipelineExecutor {
    pub fn new(llm_ocr: Arc<LlmOcrExecutor>) -> Self {
        Self { llm_ocr }
    }

    /// LLM OCR circuit breaker state at call time — stamped into pipeline
    /// outcomes for tool-result visibility.
    pub fn llm_breaker_open(&self) -> bool {
        self.llm_ocr.breaker_open()
    }

    /// Adaptive LLM concurrency allowance at call time — the ramp level the
    /// run reached, stamped into tool results so the operator can watch the
    /// AIMD ramp (floor → ceiling on success, halved on failure) across runs.
    pub fn llm_adaptive_concurrency(&self) -> usize {
        self.llm_ocr.adaptive_concurrency()
    }
}

#[async_trait]
impl OcrExecutor for PipelineExecutor {
    async fn execute(
        &self,
        page_index: usize,
        model: &str,
        image: &DynamicImage,
    ) -> Result<OcrResult, OcrError> {
        self.llm_ocr.execute(page_index, model, image).await
    }
}
