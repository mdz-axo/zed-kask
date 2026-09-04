//! OCR Pipeline — Sequential state machine: Decimate → Score → Route → OCR → Assemble.
//!
//! ```text
//! PDF → [Decimate] → PageQueue → [Score → Route → OCR] → ResultBuffer → [Assembly] → VerifiedDocument
//! ```
//!
//! Supports parallel execution via `max_concurrency` for batch/corpus workloads.
//! Interactive MCP tool calls use sequential mode (max_concurrency = None).

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::ocr::{
    ComplexityScore, ComplexityTier, CrossValidation, OcrBackend, OcrResult, PipelineError,
    PipelineOutcome, ThresholdConfig,
};

use image::DynamicImage;

use crate::ocr::complexity::score_page_complexity;
use crate::ocr::routing::{SamplingState, route_page};
use crate::ocr::verification::verify_output;

/// Typed errors for OCR backend execution.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum OcrError {
    #[error("OCR backend {backend} failed: {message}")]
    BackendFailed { backend: String, message: String },
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
}

/// Trait for executing OCR on a single page image via a specific backend.
///
/// Implementors plug in the concrete invocation path for each backend
/// (Tesseract → local binary, LlmOcr → inference router).
///
/// Must be `Send + Sync + 'static` for parallel execution via `tokio::spawn`.
#[async_trait]
pub(crate) trait OcrExecutor: Send + Sync {
    /// Check whether a backend is available for use.
    ///
    /// Returns `true` if the backend is installed and ready.
    /// Implementors should perform a lightweight probe (binary exists,
    /// service reachable) — not a full execution.
    /// Default: all backends are considered available.
    fn is_available(&self, _backend: &OcrBackend) -> bool {
        true
    }

    /// Execute OCR on a single page image.
    ///
    /// Returns `Ok(OcrResult)` on success, or `Err(OcrError)` on failure.
    async fn execute(
        &self,
        page_index: usize,
        backend: &OcrBackend,
        image: &DynamicImage,
        is_fallback: bool,
    ) -> Result<OcrResult, OcrError>;
}

/// Run the OCR pipeline on a set of page images.
///
/// Accepts an iterator for streaming support — pages are processed one at a time
/// without buffering all images in memory.
///
/// # Parallel execution
///
/// When `max_concurrency` is `Some(n)`, pages are processed concurrently using
/// a `tokio::sync::Semaphore` with `n` permits. Results are collected by page
/// index and sorted before verification. This path is intended for batch/corpus
/// workloads — interactive MCP tool calls should use `None` (sequential).
///
/// Regulation observability is handled externally by the GovernedTool membrane
/// (rJoule accounting, variety tracking, RegulationRecord persistence). Internal
/// operational telemetry uses `tracing::info!` under `reg.pipeline` target.
///
/// # Arguments
/// * `pages` — Decimated page images in document order.
/// * `expected_pages` — Total number of pages (for verification).
/// * `executor` — Pluggable OCR executor (`Arc` for parallel task spawning).
/// * `thresholds` — Complexity scoring thresholds.
/// * `llm_model` — Optional model ID for `LlmOcr` backend routing.
/// * `max_concurrency` — `Some(n)` for parallel, `None` for sequential.
///
/// # Returns
/// `PipelineOutcome` — the single sealed output. No partial state escapes.
pub async fn run_pipeline(
    pages: impl IntoIterator<Item = DynamicImage>,
    expected_pages: usize,
    executor: Arc<dyn OcrExecutor>,
    thresholds: &ThresholdConfig,
    llm_model: Option<&str>,
    max_concurrency: Option<usize>,
) -> PipelineOutcome {
    match max_concurrency {
        Some(n) if n > 1 => {
            run_pipeline_parallel(pages, expected_pages, executor, thresholds, llm_model, n).await
        }
        _ => {
            run_pipeline_sequential(pages, expected_pages, &*executor, thresholds, llm_model).await
        }
    }
}

/// Sequential pipeline — original implementation, now extracted as the `None`/`Some(1)` path.
async fn run_pipeline_sequential(
    pages: impl IntoIterator<Item = DynamicImage>,
    expected_pages: usize,
    executor: &(dyn OcrExecutor + '_),
    thresholds: &ThresholdConfig,
    llm_model: Option<&str>,
) -> PipelineOutcome {
    let start = Instant::now();
    let mut last_log = Instant::now();
    let mut state = SamplingState::new(thresholds.moderate_sample_rate);
    let mut results: Vec<OcrResult> = Vec::with_capacity(expected_pages);
    let mut errors: Vec<PipelineError> = Vec::new();
    let mut cross_validations: Vec<CrossValidation> = Vec::new();

    for (page_index, image) in pages.into_iter().enumerate() {
        let (result, cv, err) = process_single_page(
            page_index, &image, executor, thresholds, &mut state, llm_model,
        )
        .await;

        if let Some(e) = err {
            errors.push(e);
        }
        if let Some(r) = result {
            results.push(r);
        }
        if let Some(cv) = cv {
            cross_validations.push(cv);
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

    finalize_outcome_inner(results, cross_validations, errors, expected_pages, start)
}

/// Parallel pipeline — uses `Arc<Semaphore>` + `tokio::spawn` for concurrent page processing.
///
/// Pages are scored and routed synchronously (cheap), then OCR execution is spawned
/// as an async task gated by the semaphore. Results are collected by page index.
async fn run_pipeline_parallel(
    pages: impl IntoIterator<Item = DynamicImage>,
    expected_pages: usize,
    executor: Arc<dyn OcrExecutor>,
    thresholds: &ThresholdConfig,
    llm_model: Option<&str>,
    max_concurrency: usize,
) -> PipelineOutcome {
    let start = Instant::now();
    // Concurrency is two layers with different jobs:
    // - This semaphore is the STATIC total-page bound: it caps in-flight
    //   page execution (local Tesseract subprocesses plus LLM tasks waiting
    //   on the adaptive gate) at `max_concurrency` (`HKASK_MAX_CONCURRENCY`,
    //   the KaskGeneralSettings ceiling). Local resources don't need
    //   adaptation — a fixed bound is correct for them.
    // - The REMOTE bound is adaptive and lives in `LlmOcrExecutor`: an AIMD
    //   limiter (floor 2, +1 per success, halve per failure, same ceiling)
    //   gates each vision call, so LLM concurrency ramps instead of
    //   launching at max. See `batch.rs::AdaptiveLimiter` and the README's
    //   Concurrency section.
    // There is no process-wide global limiter; these two layers are the
    // whole concurrency surface for OCR.
    let semaphore = Arc::new(Semaphore::new(max_concurrency));

    // Pre-score and route all pages (synchronous, cheap)
    struct PageTask {
        page_index: usize,
        image: DynamicImage,
        routing_state: SamplingState,
    }

    let mut state = SamplingState::new(thresholds.moderate_sample_rate);

    // Allocate deterministic routing state in page order before concurrent execution.
    let mut tasks = Vec::new();
    for (page_index, image) in pages.into_iter().enumerate() {
        let score = score_page_complexity(&image, thresholds);
        tasks.push(PageTask {
            page_index,
            routing_state: state.clone(),
            image,
        });
        let _ = route_page(score, &mut state, None, llm_model);
    }

    // Spawn concurrent tasks
    let mut join_set = tokio::task::JoinSet::new();
    let results_slots = Arc::new(tokio::sync::Mutex::new(vec![
        None::<OcrResult>;
        expected_pages
    ]));
    let cvs_slots = Arc::new(tokio::sync::Mutex::new(Vec::<CrossValidation>::new()));
    let errors_slots = Arc::new(tokio::sync::Mutex::new(Vec::<PipelineError>::new()));

    // Shared progress tracking for parallel mode
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let last_progress = Arc::new(tokio::sync::Mutex::new(Instant::now()));

    for task in tasks {
        let sem = Arc::clone(&semaphore);
        let results = Arc::clone(&results_slots);
        let cvs = Arc::clone(&cvs_slots);
        let errs = Arc::clone(&errors_slots);
        let exec = Arc::clone(&executor);
        let thresh = *thresholds;
        let llm = llm_model.map(|s| s.to_string());
        let completed = Arc::clone(&completed);
        let last_progress = Arc::clone(&last_progress);

        join_set.spawn(async move {
            let _permit = sem.acquire().await;
            let mut local_state = task.routing_state;
            let (result, cv, err) = process_single_page(
                task.page_index,
                &task.image,
                &*exec,
                &thresh,
                &mut local_state,
                llm.as_deref(),
            )
            .await;

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
                results_guard[task.page_index] = Some(r);
            }
            if let Some(cv) = cv {
                let mut cvs_guard = cvs.lock().await;
                cvs_guard.push(cv);
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

    let cross_validations = {
        let mut guard = cvs_slots.lock().await;
        std::mem::take(&mut *guard)
    };
    let errors = {
        let mut guard = errors_slots.lock().await;
        std::mem::take(&mut *guard)
    };

    // Semantic enrichment is deferred to caller — the parallel path collects
    // raw CrossValidations without original text access. The caller can
    // enrich via PipelineOutcome if needed.
    finalize_outcome_inner(results, cross_validations, errors, expected_pages, start)
}

/// Process a single page: score, route, execute, cross-validate.
///
/// Returns the primary result, any cross-validation (if dual-routed),
/// any error, and the backend that produced the result.
async fn process_single_page(
    page_index: usize,
    image: &DynamicImage,
    executor: &(dyn OcrExecutor + '_),
    thresholds: &ThresholdConfig,
    state: &mut SamplingState,
    llm_model: Option<&str>,
) -> (
    Option<OcrResult>,
    Option<CrossValidation>,
    Option<PipelineError>,
) {
    let score = score_page_complexity(image, thresholds);
    let backends = route_page(score, state, None, llm_model);

    let available: Vec<OcrBackend> = backends
        .iter()
        .filter(|b| executor.is_available(b))
        .cloned()
        .collect();

    // Degradation: every routed backend is unavailable (e.g. the LLM circuit
    // breaker is open on a Complex page). Re-route through the unified
    // exclusion path so the tier's next-best backend serves the page instead
    // of hard-failing it with an empty backends_tried list.
    let available = if available.is_empty() && !backends.is_empty() {
        let mut degraded: Vec<OcrBackend> = Vec::new();
        for excluded in &backends {
            degraded = route_page(score, state, Some(excluded), llm_model)
                .into_iter()
                .filter(|b| executor.is_available(b) && !backends.contains(b))
                .collect();
            if !degraded.is_empty() {
                break;
            }
        }
        degraded
    } else {
        available
    };

    if available.is_empty() {
        return (
            None,
            None,
            Some(PipelineError::OcrFailed {
                page_index,
                backends_tried: vec![],
            }),
        );
    }

    // Execute OCR
    let (primary, secondary, err) = execute_with_fallback(
        page_index, image, executor, &available, score, state, llm_model,
    )
    .await;

    if let Some(e) = err {
        return (None, None, Some(e));
    }

    let mut primary = match primary {
        Some(r) => r,
        None => return (None, None, None),
    };

    // A breaker-open re-route executes the fallback backend as the primary
    // attempt (`is_fallback=false`), so `was_fallback` alone would hide that
    // degradation. Mark it here: when the final backend was not among the
    // tier's routed backends, the page was served by a degraded path and the
    // verification report must count it in `degraded_pages`.
    if !backends.contains(&primary.backend) {
        primary.was_fallback = true;
    }

    // Cross-validation for dual-routed pages. Both-empty collusion is no
    // longer possible to observe here: an empty LLM result is a typed
    // `EmptyOcrOutput` error (dropped as a failed secondary), never an
    // Ok-empty result, so the former both-empty warn was unreachable and
    // was removed with the empty-output classification.
    let cv = if let Some(ref sec) = secondary {
        compute_cross_validation(&primary, sec)
    } else {
        None
    };

    (Some(primary), cv, None)
}

/// Execute OCR on available backends with fallback on failure.
///
/// Returns (primary_result, secondary_result, backend_used, error).
/// For Moderate dual-routed pages, returns the better of Tesseract/LLM as primary
/// based on confidence comparison (inverts the old blind-trust-primary pattern).
async fn execute_with_fallback(
    page_index: usize,
    image: &DynamicImage,
    executor: &(dyn OcrExecutor + '_),
    available: &[OcrBackend],
    score: ComplexityScore,
    state: &mut SamplingState,
    llm_model: Option<&str>,
) -> (Option<OcrResult>, Option<OcrResult>, Option<PipelineError>) {
    let mut primary_result: Option<OcrResult> = None;
    let mut secondary_result: Option<OcrResult> = None;
    let mut backends_tried: Vec<OcrBackend> = Vec::new();

    for (backend_idx, backend) in available.iter().enumerate() {
        if backends_tried.contains(backend) {
            continue;
        }
        backends_tried.push(backend.clone());

        match executor.execute(page_index, backend, image, false).await {
            Ok(result) => {
                if backend_idx == 0 {
                    primary_result = Some(result);
                } else {
                    secondary_result = Some(result);
                }
            }
            Err(_err_msg) => {
                // Primary failed: re-route with this backend excluded — the
                // unified fallback path. Uses the page's actual score;
                // re-scoring with default thresholds could re-tier the page
                // under tuned thresholds and route the fallback wrongly.
                if backend_idx == 0 {
                    let fallback_backends = route_page(score, state, Some(backend), llm_model);

                    let mut fallback_ok = false;
                    for fb in &fallback_backends {
                        if backends_tried.contains(fb) {
                            continue;
                        }
                        backends_tried.push(fb.clone());
                        if let Ok(mut result) = executor.execute(page_index, fb, image, true).await
                        {
                            result.was_fallback = true;
                            primary_result = Some(result);
                            fallback_ok = true;
                            break;
                        }
                    }

                    if !fallback_ok {
                        return (
                            None,
                            None,
                            Some(PipelineError::OcrFailed {
                                page_index,
                                backends_tried: backends_tried.clone(),
                            }),
                        );
                    }
                }
                // Secondary failed: drop the secondary and keep the primary.
                // A failed cross-validation pass must not fail the page —
                // the primary result stands and the CV is skipped.
            }
        }
    }

    let Some(primary) = primary_result.take() else {
        return (None, None, None);
    };
    let secondary = secondary_result.take();

    // Invert Moderate dual-routing trust:
    // If both Tesseract and LLM ran on a Moderate page, and the LLM has
    // significantly higher confidence while Tesseract's is low, use LLM result.
    let (primary, secondary) = if let Some(ref sec) = secondary {
        let llm_confidence = if primary.backend != OcrBackend::Tesseract {
            primary.confidence
        } else {
            sec.confidence
        };
        let tess_confidence = if primary.backend == OcrBackend::Tesseract {
            primary.confidence
        } else {
            sec.confidence
        };

        if llm_confidence > tess_confidence + 0.3 && tess_confidence < 0.5 {
            // Trust the LLM result over Tesseract
            tracing::info!(
                target: "reg.pipeline.ocr.trust_invert",
                page_index = page_index,
                tess_confidence = tess_confidence,
                llm_confidence = llm_confidence,
                "LLM confidence significantly higher — using LLM result for Moderate page"
            );
            if primary.backend == OcrBackend::Tesseract {
                (sec.clone(), Some(primary))
            } else {
                (primary, Some(sec.clone()))
            }
        } else {
            (primary, secondary)
        }
    } else {
        (primary, secondary)
    };

    (Some(primary), secondary, None)
}

/// Shared outcome finalization: verification + Regulation tracing.
fn finalize_outcome_inner(
    results: Vec<OcrResult>,
    cross_validations: Vec<CrossValidation>,
    errors: Vec<PipelineError>,
    expected_pages: usize,
    start: Instant,
) -> PipelineOutcome {
    let duration_ms = start.elapsed().as_millis() as u64;

    let report = verify_output(expected_pages, &results, &errors);

    let backend_counts: std::collections::HashMap<String, usize> =
        results
            .iter()
            .fold(std::collections::HashMap::new(), |mut acc, r| {
                *acc.entry(r.backend.label().to_string()).or_insert(0) += 1;
                acc
            });

    for cv in &cross_validations {
        tracing::info!(
            target: "reg.pipeline.ocr",
            page_index = cv.page_index,
            similarity = cv.similarity,
            tier = ?cv.tier,
            backend_a = %cv.backend_a,
            backend_b = %cv.backend_b,
            "OCR cross-validation"
        );
    }

    tracing::info!(
        target: "reg.pipeline.ocr",
        total_pages = expected_pages,
        result_count = results.len(),
        error_count = errors.len(),
        duration_ms = duration_ms,
        passed = report.passed,
        backends = ?backend_counts,
        "OCR pipeline verification"
    );

    PipelineOutcome {
        results,
        report,
        backends: backend_counts,
        cross_validations,
        errors,
    }
}

// ── Cross-validation helpers (consolidated from cross_validation.rs + semantic.rs) ─

/// Compute cross-validation between two OCR results for the same page.
///
/// Returns `None` if the results are not comparable (different page index).
/// Otherwise computes normalized Levenshtein similarity and bundles
/// per-backend confidence scores with the complexity tier.
pub(crate) fn compute_cross_validation(
    primary: &OcrResult,
    secondary: &OcrResult,
) -> Option<CrossValidation> {
    if primary.page_index != secondary.page_index {
        return None;
    }

    let similarity = normalized_levenshtein_similarity(&primary.text, &secondary.text);

    Some(CrossValidation {
        page_index: primary.page_index,
        similarity,
        tier: ComplexityTier::Moderate,
        backend_a: primary.backend.clone(),
        backend_b: secondary.backend.clone(),
    })
}

fn normalized_levenshtein_similarity(a: &str, b: &str) -> f32 {
    let dist = levenshtein_distance(a, b);
    let max_len = a.len().max(b.len());
    if max_len == 0 {
        return 1.0;
    }
    1.0 - (dist as f32 / max_len as f32)
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();
    if a_len > b_len {
        return levenshtein_distance(b, a);
    }
    let mut prev_row: Vec<usize> = (0..=a_len).collect();
    let mut curr_row: Vec<usize> = vec![0; a_len + 1];
    for j in 1..=b_len {
        curr_row[0] = j;
        for i in 1..=a_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr_row[i] = (curr_row[i - 1] + 1)
                .min(prev_row[i] + 1)
                .min(prev_row[i - 1] + cost);
        }
        std::mem::swap(&mut prev_row, &mut curr_row);
    }
    prev_row[a_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2px vertical-stripe image: every interior pixel's Sobel window spans
    /// a stripe boundary, so edge density is ~1.0 and the page scores Complex
    /// under default thresholds. (A 1px checkerboard does NOT work — Sobel
    /// gradients cancel to zero on a perfect high-frequency checkerboard and
    /// the page scores Simple.)
    fn complex_scoring_image() -> DynamicImage {
        let size = 64;
        let mut buffer = image::RgbaImage::new(size, size);
        for (x, _y, pixel) in buffer.enumerate_pixels_mut() {
            let value = if (x / 2) % 2 == 0 { 255 } else { 0 };
            *pixel = image::Rgba([value, value, value, 255]);
        }
        DynamicImage::ImageRgba8(buffer)
    }

    fn tesseract_result(
        page_index: usize,
        text: &str,
        is_fallback: bool,
    ) -> Result<OcrResult, OcrError> {
        Ok(OcrResult {
            page_index,
            backend: OcrBackend::Tesseract,
            text: text.to_string(),
            confidence: 0.9,
            duration_ms: 1,
            was_fallback: is_fallback,
        })
    }

    /// Executor simulating a dead-but-responsive LLM endpoint: `LlmOcr`
    /// returns `EmptyOcrOutput` (the post-classification behavior of
    /// `LlmOcrExecutor`), Tesseract returns fixed text.
    struct EmptyLlmExecutor;

    #[async_trait]
    impl OcrExecutor for EmptyLlmExecutor {
        async fn execute(
            &self,
            page_index: usize,
            backend: &OcrBackend,
            _image: &DynamicImage,
            is_fallback: bool,
        ) -> Result<OcrResult, OcrError> {
            match backend {
                OcrBackend::Tesseract => tesseract_result(page_index, "rescue text", is_fallback),
                OcrBackend::LlmOcr(model) => Err(OcrError::EmptyOcrOutput {
                    model: model.clone(),
                    input_bytes: 1024,
                }),
            }
        }
    }

    /// A Complex page whose LLM backend returns empty output must degrade
    /// to Tesseract through the unified fallback path — not hard-fail and not
    /// a silent empty success. This pins the routing degradation ladder end
    /// to end (the former Tesseract anomaly detector is removed; the normal
    /// error path now covers its only reachable case).
    #[tokio::test]
    async fn complex_page_with_empty_llm_output_degrades_to_tesseract() {
        let executor: Arc<dyn OcrExecutor> = Arc::new(EmptyLlmExecutor);
        let outcome = run_pipeline(
            [complex_scoring_image()],
            1,
            executor,
            &ThresholdConfig::default(),
            Some("mock-model"),
            None,
        )
        .await;

        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.backend, OcrBackend::Tesseract);
        assert!(
            result.was_fallback,
            "degraded result must be flagged as fallback"
        );
        assert_eq!(result.text, "rescue text");
    }

    /// Executor whose LLM backend is unavailable (circuit breaker open):
    /// `is_available` reports false for `LlmOcr`, Tesseract returns fixed text.
    struct BreakerOpenExecutor;

    #[async_trait]
    impl OcrExecutor for BreakerOpenExecutor {
        fn is_available(&self, backend: &OcrBackend) -> bool {
            *backend == OcrBackend::Tesseract
        }

        async fn execute(
            &self,
            page_index: usize,
            backend: &OcrBackend,
            _image: &DynamicImage,
            is_fallback: bool,
        ) -> Result<OcrResult, OcrError> {
            match backend {
                OcrBackend::Tesseract => {
                    tesseract_result(page_index, "breaker-open rescue text", is_fallback)
                }
                OcrBackend::LlmOcr(model) => Err(OcrError::BackendFailed {
                    backend: model.clone(),
                    message: "unavailable".to_string(),
                }),
            }
        }
    }

    /// A Complex page with the LLM circuit breaker open must degrade to
    /// Tesseract via the availability re-route — not hard-fail with an empty
    /// `backends_tried` list.
    #[tokio::test]
    async fn complex_page_with_llm_breaker_open_degrades_to_tesseract() {
        let executor: Arc<dyn OcrExecutor> = Arc::new(BreakerOpenExecutor);
        let outcome = run_pipeline(
            [complex_scoring_image()],
            1,
            executor,
            &ThresholdConfig::default(),
            Some("mock-model"),
            None,
        )
        .await;

        assert!(outcome.errors.is_empty(), "errors: {:?}", outcome.errors);
        assert_eq!(outcome.results.len(), 1);
        let result = &outcome.results[0];
        assert_eq!(result.backend, OcrBackend::Tesseract);
        assert_eq!(result.text, "breaker-open rescue text");
        // The re-route serves the page through a degraded path — the routed
        // LLM backend was skipped — so the result must carry the fallback
        // marker and the verification report must count it in degraded_pages.
        assert!(
            result.was_fallback,
            "breaker-open re-route must mark the result as fallback"
        );
        assert_eq!(
            outcome.report.degraded_pages,
            vec![0],
            "breaker-open re-route must surface in degraded_pages"
        );
    }

    /// A Moderate dual-routed page whose LLM secondary returns empty output
    /// (a typed `EmptyOcrOutput` error post-classification) must keep its
    /// Tesseract primary: the failed secondary is dropped and the
    /// cross-validation is skipped — a failed CV pass must not fail the page.
    #[tokio::test]
    async fn moderate_dual_route_with_empty_llm_secondary_keeps_tesseract_primary() {
        let executor = EmptyLlmExecutor;
        let image = complex_scoring_image();
        let available = vec![
            OcrBackend::Tesseract,
            OcrBackend::LlmOcr("mock-model".to_string()),
        ];
        let score = ComplexityScore {
            value: 0.1,
            tier: ComplexityTier::Moderate,
        };
        let mut state = SamplingState::default();

        let (primary, secondary, err) = execute_with_fallback(
            0,
            &image,
            &executor,
            &available,
            score,
            &mut state,
            Some("mock-model"),
        )
        .await;

        assert!(
            err.is_none(),
            "secondary failure must not fail the page: {err:?}"
        );
        let primary = primary.expect("Tesseract primary survives the secondary failure");
        assert_eq!(primary.backend, OcrBackend::Tesseract);
        assert_eq!(primary.text, "rescue text");
        assert!(!primary.was_fallback);
        assert!(
            secondary.is_none(),
            "the failed LLM secondary must be dropped"
        );
    }
}
