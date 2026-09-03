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
    ComplexityTier, CrossValidation, OcrBackend, OcrResult, PipelineError, PipelineOutcome,
    ThresholdConfig,
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
    #[error("File is empty")]
    EmptyFile,
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
    let mut backend_counts: std::collections::HashMap<OcrBackend, usize> =
        std::collections::HashMap::new();

    for (page_index, image) in pages.into_iter().enumerate() {
        let (result, cv, err, used_backend) = process_single_page(
            page_index, &image, executor, thresholds, &mut state, llm_model,
        )
        .await;

        if let Some(e) = err {
            errors.push(e);
        }
        if let Some(r) = result {
            *backend_counts
                .entry(used_backend.unwrap_or(r.backend.clone()))
                .or_insert(0) += 1;
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

    finalize_outcome(results, cross_validations, errors, expected_pages, start).await
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
    // OCR runs in a subprocess (`hkask-mcp-corpus` binary) whose `OnceLock` is
    // distinct from the zed main process. The global concurrency limiter is
    // never wired here — `global_concurrency_limiter()` always returns `None`
    // in this process. Use a local per-pipeline semaphore bounded by
    // `max_concurrency` (from `HKASK_MAX_CONCURRENCY`, the system-wide
    // concurrency ceiling from KaskGeneralSettings). The process-wide
    // limiter lives in the zed process and gates skill execution + MCP tool
    // calls there; OCR's concurrency is bounded locally.
    //
    // If a future change embeds the corpus server in-process (no subprocess),
    // re-evaluate: the global limiter could then be shared, but `on_throttle`
    // would need wiring (currently OCR has no throttle backoff because
    // `PipelineError` doesn't carry the inner `InferenceError`).
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
    let backend_counts = Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::<
        OcrBackend,
        usize,
    >::new()));

    // Shared progress tracking for parallel mode
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let last_progress = Arc::new(tokio::sync::Mutex::new(Instant::now()));

    for task in tasks {
        let sem = Arc::clone(&semaphore);
        let results = Arc::clone(&results_slots);
        let cvs = Arc::clone(&cvs_slots);
        let errs = Arc::clone(&errors_slots);
        let counts = Arc::clone(&backend_counts);
        let exec = Arc::clone(&executor);
        let thresh = *thresholds;
        let llm = llm_model.map(|s| s.to_string());
        let completed = Arc::clone(&completed);
        let last_progress = Arc::clone(&last_progress);

        join_set.spawn(async move {
            let _permit = sem.acquire().await;
            let mut local_state = task.routing_state;
            let (result, cv, err, used_backend) = process_single_page(
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
                let mut counts_guard = counts.lock().await;
                *counts_guard
                    .entry(used_backend.unwrap_or(r.backend.clone()))
                    .or_insert(0) += 1;
                drop(counts_guard);

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
    Option<OcrBackend>,
) {
    let score = score_page_complexity(image, thresholds);
    let backends = route_page(score, state, None, llm_model);

    let available: Vec<OcrBackend> = backends
        .into_iter()
        .filter(|b| executor.is_available(b))
        .collect();

    if available.is_empty() {
        return (
            None,
            None,
            Some(PipelineError::OcrFailed {
                page_index,
                backends_tried: vec![],
            }),
            None,
        );
    }

    let is_complex = score.tier == ComplexityTier::Complex;

    // Execute OCR
    let (primary, secondary, backend_used, err) =
        execute_with_fallback(page_index, image, executor, &available, state, llm_model).await;

    if let Some(e) = err {
        return (None, None, Some(e), None);
    }

    let primary = match primary {
        Some(r) => r,
        None => return (None, None, None, None),
    };

    // Tesseract anomaly detection for Complex pages:
    // If the LLM produced empty/near-empty output on a Complex page,
    // run Tesseract as a silent-failure detector. If Tesseract found
    // substantially more text, use Tesseract result instead.
    let primary = if is_complex
        && primary.text.trim().is_empty()
        && executor.is_available(&OcrBackend::Tesseract)
    {
        match executor
            .execute(page_index, &OcrBackend::Tesseract, image, true)
            .await
        {
            Ok(tess_result) if !tess_result.text.trim().is_empty() => {
                tracing::warn!(
                    target: "reg.pipeline.ocr.silent_failure",
                    page_index = page_index,
                    llm_model = %primary.backend,
                    tesseract_words = tess_result.text.split_whitespace().count(),
                    "LLM returned empty output on Complex page but Tesseract found text — using Tesseract result"
                );
                tess_result
            }
            _ => primary,
        }
    } else {
        primary
    };

    // Cross-validation for dual-routed pages
    let cv = if let Some(ref sec) = secondary {
        if primary.text.trim().is_empty() && sec.text.trim().is_empty() {
            tracing::warn!(
                target: "reg.pipeline.ocr.collusion",
                page_index = page_index,
                backend_a = %primary.backend,
                backend_b = %sec.backend,
                "Both OCR backends produced empty output — possible blank page or collusion"
            );
        }
        compute_cross_validation(&primary, sec)
    } else {
        None
    };

    (Some(primary), cv, None, backend_used)
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
    state: &mut SamplingState,
    llm_model: Option<&str>,
) -> (
    Option<OcrResult>,
    Option<OcrResult>,
    Option<OcrBackend>,
    Option<PipelineError>,
) {
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
                // Fallback: re-route with this backend excluded
                let fallback_backends = route_page(
                    score_page_complexity(image, &ThresholdConfig::default()),
                    state,
                    Some(backend),
                    llm_model,
                );

                let mut fallback_ok = false;
                for fb in &fallback_backends {
                    backends_tried.push(fb.clone());
                    if let Ok(mut result) = executor.execute(page_index, fb, image, true).await {
                        result.was_fallback = true;
                        if backend_idx == 0 {
                            primary_result = Some(result);
                        } else {
                            secondary_result = Some(result);
                        }
                        fallback_ok = true;
                        break;
                    }
                }

                if !fallback_ok {
                    return (
                        None,
                        None,
                        None,
                        Some(PipelineError::OcrFailed {
                            page_index,
                            backends_tried: backends_tried.clone(),
                        }),
                    );
                }
            }
        }
    }

    let Some(primary) = primary_result.take() else {
        return (None, None, None, None);
    };
    let secondary = secondary_result.take();
    let backend_used = Some(primary.backend.clone());

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

    (Some(primary), secondary, backend_used, None)
}

/// Finalize a sequential pipeline outcome with Regulation tracing.
async fn finalize_outcome(
    results: Vec<OcrResult>,
    cross_validations: Vec<CrossValidation>,
    errors: Vec<PipelineError>,
    expected_pages: usize,
    start: Instant,
) -> PipelineOutcome {
    // Semantic enrichment requires the original text strings, which CrossValidation
    // doesn't store (it stores backend identifiers and confidences). Enrichment is
    // done inline in the sequential loop where text is available, and skipped here.
    finalize_outcome_inner(results, cross_validations, errors, expected_pages, start)
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
            semantic_similarity = cv.semantic_similarity,
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
        semantic_similarity: None,
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
