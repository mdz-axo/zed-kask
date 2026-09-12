//! OCR Pipeline — Sequential state machine: Decimate → OCR → Quality → Verify.
//!
//! ```text
//! PDF → [Decimate] → PageQueue → [OCR (single LLM backend)] → ResultBuffer → [Assembly] → VerifiedDocument
//! ```
//!
//! There is exactly one OCR backend: the configured vision model, invoked
//! per page. A page either gets text from that model or a typed, surfaced
//! error. Output quality is gated deterministically (`ocr::quality`)
//! instead of trusted from the backend.
//!
//! Supports parallel execution via `max_concurrency` for batch/corpus workloads.
//! Interactive MCP tool calls use sequential mode (max_concurrency = None).

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::ocr::{OcrResult, PipelineError, PipelineOutcome};

use image::DynamicImage;

use crate::ocr::verification::verify_output;

/// Typed errors for OCR backend execution.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum OcrError {
    #[error(
        "Required OCR template ocr-extract is missing or invalid; check HKASK_TEMPLATE_ROOT and host template deployment, then restart the corpus server"
    )]
    TemplateUnavailable,
    #[error("OCR model '{model}' failed: {message}")]
    BackendFailed { model: String, message: String },
    #[error("No OCR model configured. Set HKASK_OCR_MODEL env var or pass the 'model' parameter.")]
    NoModel,
    #[error("Model '{model}' exists but may not support vision input")]
    NotVisionModel { model: String },
    #[error("OCR inference failed: {0}")]
    InferenceFailed(String),
    #[error(
        "OCR model '{model}' returned no text for {input_bytes} bytes of input — empty output is a failure, not a success"
    )]
    EmptyOcrOutput { model: String, input_bytes: usize },
    #[error(
        "OCR circuit breaker open for model '{model}' — the endpoint is quarantined after repeated failures; wait for the cooldown or fix the endpoint"
    )]
    BreakerOpen { model: String },
}

/// Trait for executing OCR on a single page image via the configured model.
///
/// Must be `Send + Sync + 'static` for parallel execution via `tokio::spawn`.
#[async_trait]
pub(crate) trait OcrExecutor: Send + Sync {
    /// Execute OCR on a single page image with the given model.
    ///
    /// Returns `Ok(OcrResult)` on success, or `Err(OcrError)` on failure.
    /// There is no fallback: the error is the page's verdict.
    async fn execute(
        &self,
        page_index: usize,
        model: &str,
        image: &DynamicImage,
    ) -> Result<OcrResult, OcrError>;
}

/// Run the OCR pipeline on a set of page images.
///
/// Accepts an iterator for streaming support — pages are processed one at a
/// time without buffering all images in memory.
///
/// # Parallel execution
///
/// When `max_concurrency` is `Some(n)`, pages are processed concurrently using
/// a `tokio::sync::Semaphore` with `n` permits. Results are collected by page
/// index and sorted before verification. This path is intended for batch/corpus
/// workloads — interactive MCP tool calls should use `None` (sequential).
///
/// # Arguments
/// * `pages` — Decimated page images in document order.
/// * `expected_pages` — Total number of pages (for verification).
/// * `executor` — Pluggable OCR executor (`Arc` for parallel task spawning).
/// * `model` — The OCR model ID (resolved by the caller; never defaulted).
/// * `max_concurrency` — `Some(n)` for parallel, `None` for sequential.
///
/// # Returns
/// `PipelineOutcome` — the single sealed output. No partial state escapes.
pub async fn run_pipeline(
    pages: impl IntoIterator<Item = DynamicImage>,
    expected_pages: usize,
    executor: Arc<dyn OcrExecutor>,
    model: &str,
    max_concurrency: Option<usize>,
) -> PipelineOutcome {
    match max_concurrency {
        Some(n) if n > 1 => run_pipeline_parallel(pages, expected_pages, executor, model, n).await,
        _ => run_pipeline_sequential(pages, expected_pages, &*executor, model).await,
    }
}

/// Sequential pipeline — the `None`/`Some(1)` path.
async fn run_pipeline_sequential(
    pages: impl IntoIterator<Item = DynamicImage>,
    expected_pages: usize,
    executor: &(dyn OcrExecutor + '_),
    model: &str,
) -> PipelineOutcome {
    let start = Instant::now();
    let mut last_log = Instant::now();
    let mut results: Vec<OcrResult> = Vec::with_capacity(expected_pages);
    let mut errors: Vec<PipelineError> = Vec::new();

    for (page_index, image) in pages.into_iter().enumerate() {
        let (result, err) = process_single_page(page_index, &image, executor, model).await;

        if let Some(e) = err {
            errors.push(e);
        }
        if let Some(r) = result {
            results.push(r);
        }

        // Progress report every 50 pages or 30 seconds
        let elapsed = last_log.elapsed();
        if (page_index + 1) % 50 == 0 || elapsed.as_secs() >= 30 {
            let pct = ((page_index + 1) as f64 / expected_pages as f64 * 100.0) as u32;
            tracing::info!(
                target: "reg.pipeline",
                page = page_index + 1,
                total = expected_pages,
                percent = pct,
                elapsed_s = start.elapsed().as_secs(),
                results = results.len(),
                errors = errors.len(),
                "OCR progress"
            );
            last_log = Instant::now();
        }
    }

    finalize_outcome_inner(results, errors, expected_pages, start)
}

/// Parallel pipeline — uses `Arc<Semaphore>` + `tokio::spawn` for concurrent page processing.
async fn run_pipeline_parallel(
    pages: impl IntoIterator<Item = DynamicImage>,
    expected_pages: usize,
    executor: Arc<dyn OcrExecutor>,
    model: &str,
    max_concurrency: usize,
) -> PipelineOutcome {
    let start = Instant::now();
    // Concurrency is two layers with different jobs:
    // - This semaphore is the STATIC total-page bound: it caps in-flight
    //   page execution at `max_concurrency` (`HKASK_MAX_CONCURRENCY`, the
    //   KaskGeneralSettings ceiling).
    // - The REMOTE bound is adaptive and lives in `LlmOcrExecutor`: an AIMD
    //   limiter (floor 2, +1 per success, halve per failure, same ceiling)
    //   gates each vision call, so LLM concurrency ramps instead of
    //   launching at max. See `batch.rs::AdaptiveLimiter`.
    let semaphore = Arc::new(Semaphore::new(max_concurrency));

    let mut join_set = tokio::task::JoinSet::new();
    let results_slots = Arc::new(tokio::sync::Mutex::new(vec![
        None::<OcrResult>;
        expected_pages
    ]));
    let errors_slots = Arc::new(tokio::sync::Mutex::new(Vec::<PipelineError>::new()));

    // Shared progress tracking for parallel mode
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let last_progress = Arc::new(tokio::sync::Mutex::new(Instant::now()));

    for (page_index, image) in pages.into_iter().enumerate() {
        let sem = Arc::clone(&semaphore);
        let results = Arc::clone(&results_slots);
        let errs = Arc::clone(&errors_slots);
        let exec = Arc::clone(&executor);
        let model = model.to_string();
        let completed = Arc::clone(&completed);
        let last_progress = Arc::clone(&last_progress);

        join_set.spawn(async move {
            let _permit = sem.acquire().await;
            let (result, err) = process_single_page(page_index, &image, &*exec, &model).await;

            // Progress: check after each page completes
            let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let mut last = last_progress.lock().await;
            let elapsed = last.elapsed();
            if done.is_multiple_of(50) || elapsed.as_secs() >= 10 {
                let pct = (done as f64 / expected_pages as f64 * 100.0) as u32;
                tracing::info!(
                    target: "reg.pipeline",
                    page = done,
                    total = expected_pages,
                    percent = pct,
                    elapsed_s = start.elapsed().as_secs(),
                    "OCR progress (parallel)"
                );
                *last = Instant::now();
            }
            drop(last);

            if let Some(r) = result {
                let mut results_guard = results.lock().await;
                results_guard[page_index] = Some(r);
            }
            if let Some(e) = err {
                let mut errs_guard = errs.lock().await;
                errs_guard.push(e);
            }
        });
    }

    // Wait for all tasks
    while join_set.join_next().await.is_some() {}

    // Collect results in page order
    let results: Vec<OcrResult> = {
        let guard = results_slots.lock().await;
        guard.iter().flatten().cloned().collect()
    };

    let errors = {
        let mut guard = errors_slots.lock().await;
        std::mem::take(&mut *guard)
    };

    finalize_outcome_inner(results, errors, expected_pages, start)
}

/// Process a single page: one OCR invocation, no fallback.
///
/// Returns the result (with quality assessed at construction) or a typed
/// error carrying the model and the failure reason.
async fn process_single_page(
    page_index: usize,
    image: &DynamicImage,
    executor: &(dyn OcrExecutor + '_),
    model: &str,
) -> (Option<OcrResult>, Option<PipelineError>) {
    match executor.execute(page_index, model, image).await {
        Ok(result) => (Some(result), None),
        Err(e) => (
            None,
            Some(PipelineError::OcrFailed {
                page_index,
                model: model.to_string(),
                reason: e.to_string(),
            }),
        ),
    }
}

/// Shared outcome finalization: verification + Regulation tracing.
fn finalize_outcome_inner(
    results: Vec<OcrResult>,
    errors: Vec<PipelineError>,
    expected_pages: usize,
    start: Instant,
) -> PipelineOutcome {
    let duration_ms = start.elapsed().as_millis() as u64;

    let report = verify_output(expected_pages, &results, &errors);

    tracing::info!(
        target: "reg.pipeline.ocr",
        total_pages = expected_pages,
        result_count = results.len(),
        error_count = errors.len(),
        quality_failed = report.quality_failed_pages.len(),
        duration_ms = duration_ms,
        passed = report.passed,
        "OCR pipeline verification"
    );

    PipelineOutcome {
        results,
        report,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock executor that returns canned results per page index.
    struct MockExecutor {
        results: Vec<Result<String, OcrError>>,
    }

    #[async_trait]
    impl OcrExecutor for MockExecutor {
        async fn execute(
            &self,
            page_index: usize,
            _model: &str,
            _image: &DynamicImage,
        ) -> Result<OcrResult, OcrError> {
            match self.results.get(page_index) {
                Some(Ok(text)) => Ok(OcrResult::new(page_index, "mock-model", text.clone())),
                Some(Err(e)) => Err(e.clone()),
                None => Err(OcrError::BackendFailed {
                    model: "mock-model".into(),
                    message: "no canned result".into(),
                }),
            }
        }
    }

    fn blank_page() -> DynamicImage {
        DynamicImage::new_rgb8(100, 100)
    }

    fn clean_text() -> String {
        "Sleepless and persevering, the government over the next few months \
         hauled in suspects named Caruso, Abato, Ferro, and De Filipos. No firm \
         evidence could be found against any of them, and a few clues gradually \
         turned up that led the investigators nowhere at all."
            .to_string()
    }

    #[tokio::test]
    async fn clean_pages_pass_verification() {
        let executor = Arc::new(MockExecutor {
            results: vec![Ok(clean_text()), Ok(clean_text())],
        });
        let outcome = run_pipeline(
            [blank_page(), blank_page()],
            2,
            executor,
            "mock-model",
            None,
        )
        .await;
        assert!(outcome.report.passed);
        assert!(outcome.report.quality_failed_pages.is_empty());
        assert!(outcome.errors.is_empty());
    }

    #[tokio::test]
    async fn backend_failure_is_surfaced_not_substituted() {
        let executor = Arc::new(MockExecutor {
            results: vec![
                Ok(clean_text()),
                Err(OcrError::EmptyOcrOutput {
                    model: "mock-model".into(),
                    input_bytes: 20480,
                }),
            ],
        });
        let outcome = run_pipeline(
            [blank_page(), blank_page()],
            2,
            executor,
            "mock-model",
            None,
        )
        .await;
        // The failed page is an error, not a degraded substitution.
        assert!(!outcome.report.passed);
        assert_eq!(outcome.report.error_count, 1);
        assert_eq!(outcome.results.len(), 1);
        match &outcome.errors[0] {
            PipelineError::OcrFailed {
                page_index,
                model,
                reason,
            } => {
                assert_eq!(*page_index, 1);
                assert_eq!(model, "mock-model");
                assert!(reason.contains("empty output"), "reason: {reason}");
            }
            other => panic!("expected OcrFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn breaker_open_fails_visibly() {
        let executor = Arc::new(MockExecutor {
            results: vec![Err(OcrError::BreakerOpen {
                model: "mock-model".into(),
            })],
        });
        let outcome = run_pipeline([blank_page()], 1, executor, "mock-model", None).await;
        assert!(!outcome.report.passed);
        assert_eq!(outcome.results.len(), 0);
        match &outcome.errors[0] {
            PipelineError::OcrFailed { reason, .. } => {
                assert!(reason.contains("circuit breaker open"), "reason: {reason}");
            }
            other => panic!("expected OcrFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn degenerate_output_fails_quality_gate() {
        // The observed glm-ocr failure: a phrase cycled for the whole page.
        let degenerate =
            "another carrier in Elizabeth Street insisted that he had made the shoes ".repeat(30);
        let executor = Arc::new(MockExecutor {
            results: vec![Ok(degenerate)],
        });
        let outcome = run_pipeline([blank_page()], 1, executor, "mock-model", None).await;
        // The text exists, but the verdict is failed and the page is named.
        assert!(!outcome.report.passed);
        assert_eq!(outcome.report.quality_failed_pages, vec![0]);
        assert_eq!(outcome.results.len(), 1);
        assert_eq!(
            outcome.results[0].quality.failed_gates,
            vec!["repetition-loop".to_string()]
        );
    }

    #[tokio::test]
    async fn parallel_path_matches_sequential() {
        let results: Vec<_> = (0..10).map(|_| Ok(clean_text())).collect();
        let executor = Arc::new(MockExecutor { results });
        let pages: Vec<_> = (0..10).map(|_| blank_page()).collect();
        let outcome = run_pipeline(pages, 10, executor, "mock-model", Some(4)).await;
        assert!(outcome.report.passed);
        assert_eq!(outcome.results.len(), 10);
    }
}
