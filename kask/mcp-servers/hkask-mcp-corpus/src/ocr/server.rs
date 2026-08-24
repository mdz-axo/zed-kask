//! OCR pipeline executor — `PipelineExecutor` (Tesseract + LLM backends).
//!
//! `corpus_convert` and `corpus_ocr` in `tools/document.rs` construct a
//! `ConvertService` and delegate to it.

use crate::Arc;
use crate::ocr::llm_ocr::LlmOcrExecutor;
use crate::ocr::pipeline::{OcrError, OcrExecutor};
use crate::ocr::tesseract::TesseractExecutor;
use crate::ocr::{OcrBackend, OcrResult};
use async_trait::async_trait;

/// Shareable OCR executor that bundles Tesseract + LLM backends.
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
