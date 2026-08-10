//! OCR utilities for scanned PDF fallback.
//
// S3 unification: this module previously carried a divergent OCR path that
// called RunPod directly via `reqwest` (`runpod_credentials`, `OCR_SYSTEM_PROMPT`,
// `ocr_via_decimation`, base64-PDF fallback). The embed pipeline
// (`EmbedService::embed_corpus` → `fetch_text` → `ocr_pdf_bytes`) got inferior
// OCR — no shared prompt template, no shared model resolution, no circuit
// breaker, separate error handling.
//
// `ocr_pdf_bytes` now routes through `ocr::llm_ocr::vision_ocr_bytes`, the
// same typed primitive that `ConvertService::do_ocr` (the `corpus_ocr`/
// `corpus_convert` path) calls. This unifies the LLM-OCR call path: same
// `build_ocr_prompt` template (`docproc/ocr-extract.j2` with
// `OCR_FALLBACK_PROMPT` fallback), same `generate_vision` dispatch, same
// `OcrError` surface, same `LLMParameters` (temperature 0.1, max_tokens from
// `default_ocr_max_tokens`). The RunPod-specific code is deleted.
//
// The full typed pipeline (`ocr::pipeline::run_pipeline` — decimation,
// complexity routing, cross-validation, calibration, verification) is NOT
// wired here: it requires `DynamicImage` page images, a `PipelineExecutor`
// (Tesseract + LLM backends), and `ThresholdConfig`, none of which `fetch_text`
// has. `vision_ocr_bytes` is the right unification point — it is exactly what
// `ConvertService::do_ocr` calls under the hood, so the embed path now matches
// the `corpus_ocr` tool's behavior for raw-file OCR.

use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use hkask_types::InferencePort;

use crate::ocr::default_ocr_max_tokens;
use crate::ocr::llm_ocr::vision_ocr_bytes;
use crate::ocr::pipeline::OcrError;

/// Resolve the OCR model: `HKASK_OCR_MODEL` env var > `HkaskSettings::ocr_model`
/// (which itself falls back to `DEFAULT_OCR_MODEL`).
///
/// Mirrors the resolution order in `ocr_pdf_bytes`'s former body and in
/// `ConvertService::resolve_ocr_model` (env override > configured default).
fn resolve_ocr_model() -> String {
    if let Ok(model) = std::env::var("HKASK_OCR_MODEL")
        && !model.is_empty()
    {
        return model;
    }
    hkask_services_core::HkaskSettings::load().ocr_model()
}

/// Map `OcrError` to `ServiceError` per variant, rather than a blanket
/// `internal` classification. Follows the `.rules` "MCP tool error
/// classification" trap: `NoModel`/`NotVisionModel`/`EmptyFile` are
/// caller-errors (`BadRequest`), `BackendFailed`/`InferenceFailed` are
/// downstream failures (`ServiceUnavailable`).
fn map_ocr_error(error: OcrError) -> ServiceError {
    let (kind, message) = match error {
        OcrError::NoModel => (
            ErrorKind::BadRequest,
            "No OCR model configured. Set HKASK_OCR_MODEL env var or the ocr_model setting.".into(),
        ),
        OcrError::NotVisionModel { model } => (
            ErrorKind::BadRequest,
            format!("Model '{model}' exists but may not support vision input"),
        ),
        OcrError::EmptyFile => (
            ErrorKind::BadRequest,
            "OCR input file is empty".into(),
        ),
        OcrError::BackendFailed { backend, message } => (
            ErrorKind::ServiceUnavailable,
            format!("OCR backend {backend} failed: {message}"),
        ),
        OcrError::InferenceFailed(detail) => (
            ErrorKind::ServiceUnavailable,
            format!("OCR inference failed: {detail}"),
        ),
    };
    ServiceError::Domain {
        domain: DomainKind::Wallet,
        kind,
        source: None,
        message,
    }
}

/// Attempt OCR on PDF bytes via the typed LLM-OCR primitive.
///
/// Routes through `ocr::llm_ocr::vision_ocr_bytes` — the same primitive used
/// by `ConvertService::do_ocr` (the `corpus_ocr`/`corpus_convert` path). The
/// embed pipeline (`fetch_text` → `ocr_pdf_bytes`) therefore gets the same
/// OCR behavior as the convert pipeline: shared `docproc/ocr-extract.j2`
/// prompt template, shared `generate_vision` dispatch, shared
/// `default_ocr_max_tokens` budget, shared `OcrError` surface.
///
/// `inference_port` is threaded down from `EmbedService::embed_corpus`
/// (which holds `Arc<dyn InferencePort>`) through `resolve_work_text` →
/// `resolve_from_cache_or_download` → `fetch_text` → here.
#[must_use = "result must be used"]
pub async fn ocr_pdf_bytes(
    bytes: &[u8],
    url: &str,
    inference_port: &dyn InferencePort,
) -> Result<String, ServiceError> {
    // P9: Regulation span
    tracing::info!(target: "hkask.embed", operation = "ocr_pdf_bytes", url = %url, byte_len = bytes.len(), "REG");

    let model = resolve_ocr_model();
    let max_tokens = default_ocr_max_tokens();
    vision_ocr_bytes(inference_port, bytes, &model, max_tokens)
        .await
        .map_err(map_ocr_error)
}
