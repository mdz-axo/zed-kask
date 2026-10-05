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

use crate::batch::BreakerFeedback;

/// Typed errors for OCR backend execution.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum OcrError {
    #[error(
        "Required OCR template ocr-extract is missing or invalid; check HKASK_TEMPLATE_ROOT and host template deployment, then restart the corpus server"
    )]
    TemplateUnavailable,
    #[error("OCR response violates the required page protocol: {0}")]
    InvalidResponse(String),
    /// The model detected a rotated page and proposed a correction in
    /// degrees. Actionable, not a failure of the endpoint or the protocol:
    /// the executor re-renders the page rotated and re-OCRs it once. Never
    /// counts against the breaker (the endpoint answered) and never carries
    /// a pause verdict.
    #[error("page requests rotation correction {correction} degrees")]
    RotationRequested { correction: i16 },
    #[error("OCR model '{model}' failed: {message}")]
    BackendFailed { model: String, message: String },
    #[error("No OCR model configured. Set HKASK_OCR_MODEL env var or pass the 'model' parameter.")]
    NoModel,
    #[error("Model '{model}' exists but may not support vision input")]
    NotVisionModel { model: String },
    /// Pre-call diagnostic (registry/configuration failures that never
    /// reached an inference call). Never carries a breaker verdict.
    #[error("OCR inference failed: {0}")]
    InferenceFailed(String),
    /// A typed inference-call outcome, carrying the source `InferenceError`
    /// unstringified so the outcome classifier can recognize the shared
    /// bridge's `CircuitOpen` verdict — the 2026-10-02 spiral began with
    /// `e.to_string()` destroying that signal at this boundary.
    #[error("OCR inference failed: {source}")]
    Inference { source: hkask_types::InferenceError },
    #[error(
        "OCR model '{model}' returned no text for {input_bytes} bytes of input — empty output is a failure, not a success"
    )]
    EmptyOcrOutput { model: String, input_bytes: usize },
    /// The inference endpoint was unreachable at the transport layer
    /// (`InferenceError::Connection`: connection refused, closed channel,
    /// or persistent HTTP transport failure) for
    /// [`ENDPOINT_UNREACHABLE_CONSECUTIVE`] consecutive pages. The run
    /// aborts cleanly: remaining pages fail fast without a vision call,
    /// the file stays pending (no receipt), and the driver relaunches
    /// when the host returns. Not a capacity signal — never counts
    /// against the breaker.
    #[error(
        "inference endpoint unreachable after {consecutive} consecutive connection failures — run aborted; relaunch when the host returns"
    )]
    EndpointUnreachable { consecutive: u32 },
}

impl BreakerFeedback for OcrError {
    fn breaker_retry_after(&self) -> Option<std::time::Duration> {
        match self {
            // Only a typed inference-call outcome can carry the shared
            // bridge's breaker verdict; pre-call diagnostics and page-level
            // failures are capacity signals, never pause signals.
            Self::Inference { source } => source.breaker_retry_after(),
            _ => None,
        }
    }
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

/// Run-scoped consecutive-connection trip for [`OcrError::EndpointUnreachable`].
///
/// A dead socket or persistently failing transport cannot recover inside
/// a cooldown cycle — waiting one out is the wrong action against it (the
/// 2026-10-03 dead-socket grind: five hours of 300s pause cycles against
/// a severed inference socket). Three bounded attempts absorb a
/// briefly-restarting socket; a still-dead endpoint loses the run to a
/// cheap relaunch (the receipt short-circuit makes pending files nearly
/// free to retry). Any successful page resets the count; other failure
/// classes neither count nor reset — only recovery clears distress.
const ENDPOINT_UNREACHABLE_CONSECUTIVE: u32 = 3;

#[derive(Default)]
struct EndpointWatch {
    consecutive: std::sync::atomic::AtomicU32,
    tripped: std::sync::atomic::AtomicBool,
}

impl EndpointWatch {
    fn record_success(&self) {
        self.consecutive
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Record a page failure. Returns `true` when this failure is the one
    /// that trips the abort (first crossing only) — the caller logs it.
    fn record_failure(&self, error: &OcrError) -> bool {
        if !matches!(
            error,
            OcrError::Inference {
                source: hkask_types::InferenceError::Connection(_),
            }
        ) {
            return false;
        }
        let consecutive = self
            .consecutive
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if consecutive >= ENDPOINT_UNREACHABLE_CONSECUTIVE {
            self.tripped
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        consecutive == ENDPOINT_UNREACHABLE_CONSECUTIVE
    }

    fn tripped(&self) -> bool {
        self.tripped.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The typed abort for a page short-circuited after the trip.
    fn unreachable_error(&self, page_index: usize, model: &str) -> PipelineError {
        PipelineError::OcrFailed {
            page_index,
            model: model.to_string(),
            reason: OcrError::EndpointUnreachable {
                consecutive: ENDPOINT_UNREACHABLE_CONSECUTIVE,
            }
            .to_string(),
        }
    }
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
    let watch = EndpointWatch::default();

    for (page_index, image) in pages.into_iter().enumerate() {
        // A tripped endpoint watch fails the remaining pages fast — no
        // vision call, no slot, no cooldown cycle against a dead endpoint.
        if watch.tripped() {
            errors.push(watch.unreachable_error(page_index, model));
            continue;
        }
        let (result, err) = process_single_page(page_index, &image, executor, model, &watch).await;

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
    //   limiter (floor 2, +1 per success, halve per failure, paused while
    //   any breaker is open, same ceiling) gates each vision call, so LLM
    //   concurrency ramps instead of launching at max — and waits out
    //   breaker cooldowns instead of failing pages into them. See
    //   `batch.rs::AdaptiveLimiter`.
    let semaphore = Arc::new(Semaphore::new(max_concurrency));
    let watch = Arc::new(EndpointWatch::default());

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
        let watch = Arc::clone(&watch);

        join_set.spawn(async move {
            let _permit = sem.acquire().await;
            // A tripped endpoint watch fails this page fast — no vision
            // call, no slot, no cooldown cycle against a dead endpoint.
            if watch.tripped() {
                let mut errs_guard = errs.lock().await;
                errs_guard.push(watch.unreachable_error(page_index, &model));
                return;
            }
            let (result, err) =
                process_single_page(page_index, &image, &*exec, &model, &watch).await;

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
    watch: &EndpointWatch,
) -> (Option<OcrResult>, Option<PipelineError>) {
    match executor.execute(page_index, model, image).await {
        Ok(result) => {
            watch.record_success();
            (Some(result), None)
        }
        Err(e) => {
            if watch.record_failure(&e) {
                tracing::error!(
                    target: "reg.pipeline.ocr.unreachable",
                    page_index,
                    consecutive = ENDPOINT_UNREACHABLE_CONSECUTIVE,
                    "inference endpoint unreachable — aborting the run; the file stays pending, relaunch when the host returns"
                );
            }
            (
                None,
                Some(PipelineError::OcrFailed {
                    page_index,
                    model: model.to_string(),
                    reason: e.to_string(),
                }),
            )
        }
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
        blank = report.blank_pages.len(),
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

    /// Scripted executor: consumes canned outcomes by CALL order (not
    /// page index) and counts calls — the endpoint-watch tests need to
    /// know exactly how many pages reached the executor.
    struct ScriptedExecutor {
        script: Vec<Result<String, OcrError>>,
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl OcrExecutor for ScriptedExecutor {
        async fn execute(
            &self,
            page_index: usize,
            _model: &str,
            _image: &DynamicImage,
        ) -> Result<OcrResult, OcrError> {
            let call = self
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match self.script.get(call) {
                Some(Ok(text)) => Ok(OcrResult::new(page_index, "scripted-model", text.clone())),
                Some(Err(e)) => Err(e.clone()),
                None => Ok(OcrResult::new(
                    page_index,
                    "scripted-model",
                    "script exhausted".to_string(),
                )),
            }
        }
    }

    fn connection_refusal() -> OcrError {
        OcrError::Inference {
            source: hkask_types::InferenceError::Connection(
                "connection refused (test fixture)".into(),
            ),
        }
    }

    /// Three consecutive connection refusals abort the run: the trip
    /// bounds executor calls, and every page still carries a typed
    /// error. The dead-socket grind class (2026-10-03) dies here — the
    /// run ends instead of cycling cooldowns against a dead endpoint.
    #[tokio::test]
    async fn persistent_connection_refusal_aborts_the_run() {
        let executor = Arc::new(ScriptedExecutor {
            script: vec![
                Err(connection_refusal()),
                Err(connection_refusal()),
                Err(connection_refusal()),
            ],
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let handle = Arc::clone(&executor);
        let pages: Vec<DynamicImage> = (0..8).map(|_| blank_page()).collect();
        let outcome = run_pipeline(pages, 8, executor, "scripted-model", None).await;

        assert_eq!(
            handle.calls.load(std::sync::atomic::Ordering::Relaxed),
            3,
            "exactly the bounded attempts reach the executor"
        );
        assert_eq!(outcome.errors.len(), 8, "every page carries an error");
        assert!(
            outcome.results.is_empty(),
            "no page produces a result against a dead endpoint"
        );
        let unreachable = outcome
            .errors
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    PipelineError::OcrFailed { reason, .. }
                        if reason.contains("unreachable")
                )
            })
            .count();
        assert_eq!(
            unreachable, 5,
            "the five post-trip pages fail fast with the typed abort"
        );
        assert!(!outcome.report.passed);
    }

    /// A connection blip between successes does not trip the abort: only
    /// consecutive connection failures count, and any successful page
    /// resets the count — every page is still attempted.
    #[tokio::test]
    async fn connection_blips_between_successes_do_not_abort() {
        let executor = Arc::new(ScriptedExecutor {
            script: vec![
                Err(connection_refusal()),
                Ok("page one".to_string()),
                Err(connection_refusal()),
                Ok("page two".to_string()),
                Err(connection_refusal()),
            ],
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let handle = Arc::clone(&executor);
        let pages: Vec<DynamicImage> = (0..5).map(|_| blank_page()).collect();
        let outcome = run_pipeline(pages, 5, executor, "scripted-model", None).await;

        assert_eq!(
            handle.calls.load(std::sync::atomic::Ordering::Relaxed),
            5,
            "no page is short-circuited — blips never trip"
        );
        assert_eq!(outcome.errors.len(), 3, "the three blip pages fail");
        assert_eq!(outcome.results.len(), 2);
        assert!(
            outcome.errors.iter().all(|e| {
                !matches!(
                    e,
                    PipelineError::OcrFailed { reason, .. }
                        if reason.contains("unreachable")
                )
            }),
            "no typed abort is emitted for blips"
        );
    }

    /// The parallel path trips too: a dead endpoint burns a bounded number
    /// of executor calls (the trip count plus at most one in-flight wave),
    /// never one per page.
    #[tokio::test]
    async fn parallel_connection_refusal_is_bounded() {
        let executor = Arc::new(ScriptedExecutor {
            script: std::iter::repeat_with(connection_refusal)
                .map(Err)
                .take(20)
                .collect(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let handle = Arc::clone(&executor);
        let pages: Vec<DynamicImage> = (0..20).map(|_| blank_page()).collect();
        let outcome = run_pipeline(pages, 20, executor, "scripted-model", Some(2)).await;

        let calls = handle.calls.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            (3..=4).contains(&calls),
            "the abort bounds executor calls to the trip count plus one in-flight wave, got {calls}"
        );
        assert_eq!(outcome.errors.len(), 20, "every page carries an error");
        assert!(!outcome.report.passed);
    }

    /// A pre-detected blank page is a named, non-failing report category:
    /// verification counts it in `blank_pages`, never `empty_pages`, and
    /// `passed` does not depend on it.
    #[test]
    fn blank_pages_are_named_not_failing() {
        let results = vec![
            OcrResult::new(0, "mock-model", "real text".to_string()),
            OcrResult::blank_page(1),
        ];
        let report = verify_output(2, &results, &[]);
        assert!(report.passed, "a blank page is not a conversion failure");
        assert_eq!(report.blank_pages, vec![1]);
        assert!(report.empty_pages.is_empty());
        assert!(report.page_count_match);
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

    /// Only a typed inference-call outcome can carry the shared bridge's
    /// breaker verdict; pre-call diagnostics and page-level failures are
    /// capacity signals, never pause signals.
    #[test]
    fn ocr_error_delegates_breaker_feedback_through_the_typed_variant() {
        let verdict = OcrError::Inference {
            source: hkask_types::InferenceError::CircuitOpen(
                "transient inference failure threshold reached; retry after 30s".into(),
            ),
        };
        assert_eq!(
            verdict.breaker_retry_after(),
            Some(std::time::Duration::from_secs(32))
        );
        assert_eq!(
            OcrError::EmptyOcrOutput {
                model: "m".into(),
                input_bytes: 1
            }
            .breaker_retry_after(),
            None,
            "page-level failures are capacity signals, never pause signals"
        );
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
