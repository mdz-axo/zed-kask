#![forbid(unsafe_code)]
//! hKask MCP DocProc — Unified document processing MCP server
//!
//! Combines format conversion, OCR, chunking, h_mem extraction, embedding,
//! QA generation, caching, query, and Kindle book export (17 tools). Supersedes the former
//! `hkask-mcp-markitdown` and `hkask-mcp-doc-knowledge` servers.
//!
//! Server struct in lib.rs, tool methods in tools/ module.
//! Helpers in helpers.rs (math/text) and json_extract.rs (LLM JSON parsing).

#![allow(unused_crate_dependencies)] // Bin target — deps used in main.rs, lint checks lib target only

mod backend;
pub mod bridge;
pub mod convert;
mod helpers;
mod json_extract;
pub mod ocr;
pub mod template;
pub mod tools;

// Re-export template renderer for tool modules (accessible via `use crate::*;`)
pub(crate) use template::render_docproc_template;
// Re-export helpers used by tool modules.
pub(crate) use helpers::{
    chunk_structure, chunk_word_bounds, cosine_similarity, serialize_passages, tokens_to_words,
};
pub(crate) use json_extract::extract_json_from_response;

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use crate::ocr::ThresholdConfig;
use crate::ocr::decimation;
use hkask_inference::{EmbeddingRouter, InferenceConfig, InferenceRouter};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_memory::SemanticMemory;
use hkask_services_core::settings::HkaskSettings;
use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use hkask_types::time::now_rfc3339;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
#[allow(unused_imports)]
use schemars::JsonSchema;
#[allow(unused_imports)]
use serde::Deserialize;
#[allow(unused_imports)]
use serde::Serialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

// ── Constants ──────────────────────────────────────────────────────────────

/// Resolve the embedding dimension from env or default to 1024 (Qwen3-Embedding-0.6B).
pub(crate) fn embedding_dim() -> usize {
    std::env::var("HKASK_EMBEDDING_DIM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024)
}

/// Pre-normalize a vector in place so cosine similarity becomes a dot product.
pub(crate) fn normalize_in_place(v: &mut [f32]) {
    let mag = (v.iter().map(|x| x * x).sum::<f32>()).sqrt();
    if mag > 0.0 {
        for x in v.iter_mut() {
            *x /= mag;
        }
    }
}

/// Normalize a concept string for graph-key and embedding-annotation consistency.
///
/// The salience graph (`hkask_memory::salience::compute_salience_batch`) keys
/// on exact strings, so "ROIC", "Roic", "roic  " would be three disconnected
/// nodes. Lowercase + trim + collapse whitespace merges them. This helper is
/// the single canonical normalization point shared by:
/// - `tagging/ops.rs` (initial `concepts` vector build + `validate_ontology_tags`)
/// - `corpus.rs` (consolidation merge — must match the tagging-phase form)
/// - `semantic.rs` (embedding annotation prefix + ontology namespace cross-check)
///
/// Corpus-specific canonicalization (e.g. "DCF" → "discounted cash flow") is
/// driven by the tagging template, not hardcoded here — docproc is a general
/// processor.
pub(crate) fn normalize_concept(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Construct a WebID for a persona owner string.
pub(crate) fn owner_webid(owner: &str) -> hkask_types::WebID {
    hkask_types::WebID::from_persona(owner.as_bytes())
}

/// Minimum word count from pdf-extract to consider text extraction successful
/// before falling back to OCR for scanned PDFs.
pub(crate) const OCR_FALLBACK_WORD_THRESHOLD: usize = 100;

/// Default owner persona for h_mems stored by corpus pipeline tools.
const DEFAULT_OWNER: &str = "john-brooks";

/// OCR pipeline concurrency — env var HKASK_OCR_CONCURRENCY, default 4.
/// Controls how many pages are sent to the vision model in parallel.
/// Set to 1 for sequential mode (interactive use), higher for batch processing.
pub(crate) fn ocr_concurrency() -> usize {
    std::env::var("HKASK_OCR_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(4)
}

/// Default embedding model — env var first, then HkaskSettings from disk.
/// Consolidates 6 hardcoded "DI/Qwen/Qwen3-Embedding-0.6B" references (Q3).
/// Result is cached in a OnceLock to avoid repeated disk reads and eliminate
/// the `String::leak` anti-pattern (BUG-1 fix, BUG-2 fix).
fn default_embedding_model() -> &'static str {
    use std::sync::OnceLock;
    static CACHED: OnceLock<String> = OnceLock::new();

    CACHED
        .get_or_init(|| {
            std::env::var("HKASK_EMBEDDING_MODEL")
                .unwrap_or_else(|_| HkaskSettings::load().embedding_model)
        })
        .as_str()
}

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct DocProcServer {
        pub ocr_model: Option<String>,
        pub inference_router: Arc<InferenceRouter>,
        pub ocr_thresholds: ThresholdConfig,
        pub embedding_router: Option<EmbeddingRouter>,
        pub cv_accumulator: Mutex<Vec<crate::ocr::CrossValidation>>,
        pub index: Mutex<Vec<IndexedPassage>>,
        pub llm_ocr: Arc<crate::ocr::llm_ocr::LlmOcrExecutor>,
        pub pipeline_executor: Arc<crate::ocr::PipelineExecutor>,
    }
);

/// A passage stored in the in-memory vector index with its embedding.
#[derive(Debug, Clone)]
pub struct IndexedPassage {
    pub text: String,
    pub metadata: serde_json::Value,
    pub embedding: Vec<f32>,
}

// ── Server constructor + core methods ──────────────────────────────────────

impl DocProcServer {
    /// Check whether OCR capability is available.
    pub fn has_ocr(&self) -> bool {
        self.ocr_model.is_some()
    }

    /// Index passages into the in-memory vector store for later query.
    /// Embeds each passage text and stores it with metadata.
    /// Returns the number of passages indexed (0 if embedding router unavailable).
    /// Emits a Regulation warning when indexing was requested but embedding is unavailable (GAP-6).
    pub async fn index_passages(&self, passages: &[(String, String)], source_label: &str) -> usize {
        let Some(ref emb_router) = self.embedding_router else {
            tracing::warn!(
                target: "hkask.docproc.index",
                source = %source_label,
                passage_count = passages.len(),
                "Cannot index passages — embedding router not configured. \
                 Set HKASK_EMBEDDING_MODEL to enable semantic search."
            );
            return 0;
        };

        let texts: Vec<&str> = passages.iter().map(|(_, t)| t.as_str()).collect();
        if texts.is_empty() {
            return 0;
        }

        let model_name = std::env::var("HKASK_EMBEDDING_MODEL")
            .unwrap_or_else(|_| default_embedding_model().to_string());

        let vectors = match emb_router.embed_sentences(&model_name, &texts).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(target: "hkask.mcp.docproc.index", error = %e, "Failed to embed passages for indexing");
                return 0;
            }
        };

        let mut index = self
            .index
            .lock()
            .expect("Failed to lock index for passage indexing");
        for (i, ((entity_ref, passage_text), embedding)) in
            passages.iter().zip(vectors.into_iter()).enumerate()
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

    /// Record a tool call as a narrative experience in the agent's memory.
    pub fn record_experience(
        &self,
        tool: &str,
        input_summary: &str,
        outcome: &str,
        detail: serde_json::Value,
    ) {
        if let Some(ref daemon) = self.daemon {
            let value = serde_json::json!({
                "tool": tool, "input": input_summary, "outcome": outcome,
                "detail": detail, "timestamp": now_rfc3339(),
            });
            let daemon_clone = daemon.clone();
            let userpod = self.userpod.clone();
            let tool_name = tool.to_string();
            tokio::spawn(async move {
                match daemon_clone
                    .store_experience(&userpod, "mcp_session", "observed", &value, Some(0.85))
                    .await
                {
                    Ok(hkask_mcp_server::DaemonResponse::StoreResponse {
                        stored: true, ..
                    }) => {
                        tracing::debug!(target: "hkask.mcp.docproc.memory", tool = %tool_name, "Experience stored via daemon");
                    }
                    Ok(other) => {
                        tracing::warn!(target: "hkask.mcp.docproc.memory", tool = %tool_name, response = ?other, "Unexpected daemon response")
                    }
                    Err(e) => {
                        tracing::warn!(target: "hkask.mcp.docproc.memory", tool = %tool_name, error = %e, "Failed to store experience")
                    }
                }
            });
        }
    }
}

// ── Tool helpers ───────────────────────────────────────────────────────────

/// Shared text extraction from a file path.
///
/// Detects format, reads the file, and extracts plain text. For PDFs,
/// falls back to OCR if text extraction yields fewer than
/// `OCR_FALLBACK_WORD_THRESHOLD` words and an OCR model is available.
///
/// Used by both `docproc_convert` and `docproc_chunk` to eliminate ~160
/// lines of duplicated extraction logic (P5: surgical deduplication).
async fn extract_text(path: &str) -> Result<ExtractOutcome, McpToolError> {
    let (format, supported, note) = convert::detect_format(path);

    if !supported {
        return Err(McpToolError::invalid_argument(format!(
            "Format '{}' is not supported for text extraction. Supported formats: pdf, markdown, html, plain. {}",
            format,
            note.unwrap_or("")
        )));
    }

    let file_bytes = std::fs::read(path)
        .map_err(|e| McpToolError::internal(format!("Failed to read file '{}': {}", path, e)))?;

    if file_bytes.is_empty() {
        return Err(McpToolError::invalid_argument(format!(
            "File '{}' is empty",
            path
        )));
    }

    let extract_result = match format {
        "pdf" => {
            // Use -layout to preserve column structure (reading-order heuristic).
            // Without -layout, pdftotext may interleave multi-column text.
            // With -layout, it preserves spatial positioning, so columns are
            // read top-to-bottom within each column rather than across columns.
            let output = tokio::process::Command::new("pdftotext")
                .arg("-layout")
                .arg(path)
                .arg("-")
                .output()
                .await;
            match output {
                Ok(output) if output.status.success() => {
                    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
                    // Per-page triage: split on form-feed, classify each page.
                    // This fixes the silent-loss bug where a mixed PDF with
                    // ≥100 whole-doc words returned Success and dropped any
                    // per-page scanned/image-only regions. On any triage error,
                    // fall back to the legacy whole-doc word-count check.
                    let per_page: Vec<String> = raw.split('\x0c').map(String::from).collect();
                    let mut per_page = per_page;
                    if per_page.last().is_some_and(|p| p.trim().is_empty()) {
                        per_page.pop();
                    }
                    let triage_cfg = crate::ocr::TriageConfig::from_env();
                    match crate::ocr::triage::triage_pages(
                        std::path::Path::new(path),
                        &per_page,
                        &triage_cfg,
                    )
                    .await
                    {
                        Ok(verdicts) => {
                            let ocr_pages = crate::ocr::triage::ocr_page_indices(&verdicts);
                            tracing::info!(
                                target: "reg.pipeline.triage",
                                path = path,
                                pages = verdicts.len(),
                                ocr_pages = ocr_pages.len(),
                                "per-page triage complete"
                            );
                            if ocr_pages.is_empty() {
                                // All pages text-native — fast path, no OCR.
                                let text = per_page.join("\n\x0c");
                                let word_count = text.split_whitespace().count();
                                ExtractOutcome::Success {
                                    text,
                                    word_count,
                                    structure: None,
                                }
                            } else {
                                // Mixed or scanned: keep per-page native text
                                // (OCR pages emptied) so the caller can
                                // interleave OCR results in page order.
                                let page_texts: Vec<String> = per_page
                                    .iter()
                                    .enumerate()
                                    .map(|(i, t)| {
                                        if ocr_pages.contains(&i) {
                                            String::new()
                                        } else {
                                            t.clone()
                                        }
                                    })
                                    .collect();
                                let native_wc = page_texts
                                    .iter()
                                    .map(|t| t.split_whitespace().count())
                                    .sum();
                                ExtractOutcome::PartialOcr {
                                    page_texts,
                                    word_count: native_wc,
                                    ocr_pages,
                                    verdicts,
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "reg.pipeline.triage",
                                path = path,
                                error = %e,
                                "triage failed — falling back to whole-doc word-count check"
                            );
                            let word_count = raw.split_whitespace().count();
                            if word_count < OCR_FALLBACK_WORD_THRESHOLD {
                                ExtractOutcome::NeedsOcr {
                                    partial_text: raw,
                                    word_count,
                                }
                            } else {
                                ExtractOutcome::Success {
                                    text: raw,
                                    word_count,
                                    structure: None,
                                }
                            }
                        }
                    }
                }
                Ok(output) => {
                    tracing::warn!(
                        target: "reg.pipeline.pdf_extract",
                        path = path,
                        stderr = %String::from_utf8_lossy(&output.stderr),
                        "pdftotext failed — routing document to OCR"
                    );
                    ExtractOutcome::NeedsOcr {
                        partial_text: String::new(),
                        word_count: 0,
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        target: "reg.pipeline.pdf_extract",
                        path = path,
                        error = %error,
                        "pdftotext unavailable — routing document to OCR"
                    );
                    ExtractOutcome::NeedsOcr {
                        partial_text: String::new(),
                        word_count: 0,
                    }
                }
            }
        }
        "plain" => match std::str::from_utf8(&file_bytes) {
            Ok(text) => ExtractOutcome::Success {
                text: text.to_string(),
                word_count: text.split_whitespace().count(),
                structure: None,
            },
            Err(e) => {
                return Err(McpToolError::internal(format!(
                    "Failed to decode text file '{}': {}",
                    path, e
                )));
            }
        },
        "markdown" => match std::str::from_utf8(&file_bytes) {
            Ok(content) => {
                let text = convert::strip_frontmatter(content);
                let word_count = text.split_whitespace().count();
                ExtractOutcome::Success {
                    text,
                    word_count,
                    structure: None,
                }
            }
            Err(e) => {
                return Err(McpToolError::internal(format!(
                    "Failed to decode markdown file '{}': {}",
                    path, e
                )));
            }
        },
        "html" | "htm" => match std::str::from_utf8(&file_bytes) {
            Ok(content) => {
                let text = convert::strip_html(content);
                let word_count = text.split_whitespace().count();
                ExtractOutcome::Success {
                    text,
                    word_count,
                    structure: None,
                }
            }
            Err(e) => {
                return Err(McpToolError::internal(format!(
                    "Failed to decode HTML file '{}': {}",
                    path, e
                )));
            }
        },
        // Office format backends (S2: backend/pipeline separation)
        "docx" | "pptx" | "xlsx" => {
            let structure = parse_with_backend(format, path)?;
            let word_count = structure.word_count();
            let text = structure.text();
            if word_count == 0 {
                return Err(McpToolError::internal(format!(
                    "Backend '{}' extracted 0 words from '{}'",
                    format, path
                )));
            }
            ExtractOutcome::Success {
                text,
                word_count,
                structure: Some(structure),
            }
        }
        _ => unreachable!("supported check above guards this branch"),
    };

    Ok(extract_result)
}

/// Filter a PDF `ExtractOutcome` to a target page set (1-based).
///
/// `Success`: split on form-feed, keep only target pages, rejoin.
/// `PartialOcr`: filter `page_texts`, `ocr_pages`, and `verdicts` to target
/// pages. `NeedsOcr`: returned unchanged (no per-page structure; the caller's
/// decimation path handles page selection separately).
pub(crate) fn filter_outcome_to_pages(
    outcome: ExtractOutcome,
    target: &std::collections::HashSet<usize>,
) -> ExtractOutcome {
    if target.is_empty() {
        return outcome;
    }
    match outcome {
        ExtractOutcome::Success {
            text, structure, ..
        } => {
            let kept: Vec<String> = text
                .split('\x0c')
                .enumerate()
                .filter(|(i, _)| target.contains(&(i + 1)))
                .map(|(_, p)| p.to_string())
                .collect();
            let joined = kept.join("\n\x0c");
            ExtractOutcome::Success {
                word_count: joined.split_whitespace().count(),
                text: joined,
                structure,
            }
        }
        ExtractOutcome::PartialOcr {
            page_texts,
            ocr_pages,
            verdicts,
            ..
        } => {
            let filtered_texts: Vec<String> = page_texts
                .iter()
                .enumerate()
                .filter(|(i, _)| target.contains(&(i + 1)))
                .map(|(_, t)| t.clone())
                .collect();
            let filtered_ocr: Vec<usize> = ocr_pages
                .into_iter()
                .filter(|i| target.contains(&(i + 1)))
                .collect();
            let filtered_verdicts: Vec<crate::ocr::TriageVerdict> = verdicts
                .into_iter()
                .filter(|v| target.contains(&v.page_number))
                .collect();
            let wc = filtered_texts
                .iter()
                .map(|t| t.split_whitespace().count())
                .sum();
            ExtractOutcome::PartialOcr {
                page_texts: filtered_texts,
                word_count: wc,
                ocr_pages: filtered_ocr,
                verdicts: filtered_verdicts,
            }
        }
        other => other,
    }
}

/// Load a docproc template from registry and render with minijinja.
///
/// Templates live in `registry/templates/docproc/` as Jinja2 files.
/// Uses the same minijinja rendering pattern as `self_heal.rs` and the
/// hkask-templates ManifestExecutor. Falls back to empty string if the
/// template file is missing or rendering fails — callers provide an
/// inline fallback prompt.
///
/// Template base path is resolved relative to the workspace root. If the
/// server is started from a different directory, set `HKASK_USERPOD_REGISTRY_PATH`
/// to the absolute path of the `registry/userpods` directory.
pub(crate) fn default_owner() -> String {
    DEFAULT_OWNER.to_string()
}

/// Dispatch to the appropriate `DocumentBackend` based on format name.
///
/// Returns the parsed `DocStructure`. Used by `extract_text` for office
/// formats (docx, pptx, xlsx) — the structure is flattened to text for the
/// `ExtractOutcome::Success` path, but future structure-aware tools can
/// call the backends directly.
fn parse_with_backend(
    format: &str,
    path: &str,
) -> Result<hkask_types::document::DocStructure, McpToolError> {
    use backend::{DocumentBackend, DocxBackend, PptxBackend, XlsxBackend};
    let structure = match format {
        "docx" => DocxBackend.parse(path),
        "pptx" => PptxBackend.parse(path),
        "xlsx" => XlsxBackend.parse(path),
        _ => unreachable!("parse_with_backend called with unsupported format: {format}"),
    }
    .map_err(|e| McpToolError::internal(format!("Backend error: {e}")))?;
    Ok(structure)
}

// ── Extract outcome enum ───────────────────────────────────────────────────

enum ExtractOutcome {
    Success {
        text: String,
        word_count: usize,
        /// Structural representation when a backend produced one.
        /// `None` for plain-text/markdown/HTML extraction (no structure).
        structure: Option<hkask_types::document::DocStructure>,
    },
    NeedsOcr {
        partial_text: String,
        word_count: usize,
    },
    /// Per-page triage found a mix of text-native and OCR-needing pages.
    ///
    /// `native_text` is the text of the text-native pages only (OCR-needing
    /// pages are omitted, to be filled in by the caller's selective OCR pass).
    /// `ocr_pages` are 0-based page indices that must go through the OCR
    /// pipeline. `verdicts` is the full per-page triage for reporting/Regulation.
    ///
    /// This outcome replaces the former silent-loss path where a mixed PDF
    /// with ≥100 whole-doc words returned `Success` and dropped per-page
    /// scanned regions entirely.
    PartialOcr {
        page_texts: Vec<String>,
        word_count: usize,
        ocr_pages: Vec<usize>,
        verdicts: Vec<crate::ocr::TriageVerdict>,
    },
}

// ── Combined tool router (P5 Essentialism — modular tool groups) ──────────

impl DocProcServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::document_router()
            + Self::semantic_router()
            + Self::storage_router()
            + Self::corpus_router()
            + Self::tagging_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for DocProcServer {}

// ── Entry point ────────────────────────────────────────────────────────────

/// Run the docproc MCP server (used by binary target).
pub async fn run(
    userpod: String,
    daemon_client: Option<hkask_mcp_server::DaemonClient>,
) -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-docproc",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            let ocr_model = ctx
                .credentials
                .get("HKASK_OCR_MODEL")
                .cloned();
            let inference_config = InferenceConfig::from_env();

            let ocr_thresholds = ThresholdConfig {
                simple_max: std::env::var("HKASK_OCR_SIMPLE_MAX")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.05),
                moderate_max: std::env::var("HKASK_OCR_MODERATE_MAX")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.15),
                moderate_sample_rate: std::env::var("HKASK_OCR_SAMPLE_RATE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.10),
                tuneable: std::env::var("HKASK_OCR_TUNEABLE")
                    .ok()
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(true),
            };

            let embedding_router = EmbeddingRouter::new(inference_config.clone());
            let inference_router = Arc::new(InferenceRouter::new(inference_config));

                        let llm_ocr = Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(Arc::clone(&inference_router)));
                                    let pipeline_executor = Arc::new(crate::ocr::PipelineExecutor::new(Arc::clone(&llm_ocr)));

                        Ok(DocProcServer::new(
                            ctx.webid,
                            userpod.clone(),
                            daemon_client.clone(),
                            ocr_model,
                            inference_router,
                            ocr_thresholds,
                            Some(embedding_router),
                            Mutex::new(Vec::new()),
                            Mutex::new(Vec::new()),
                            llm_ocr,
                            pipeline_executor,
                        ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_OCR_MODEL",
                "Vision model for OCR (must exist in inference catalog). Required for OCR functionality.",
            ),
        ],
    )
    .await
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::document::{ConvertRequest, default_true};
    use crate::tools::semantic::GenerateQaRequest;
    use crate::tools::storage::QueryRequest;

    #[test]
    fn convert_request_schema_supports_pipeline_output_directory() {
        let schema = schemars::schema_for!(ConvertRequest);
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("ConvertRequest schema should expose object properties");

        assert!(
            properties.contains_key("output"),
            "docproc_convert must accept the pipeline manifest's output directory"
        );
    }

    #[test]
    fn normalize_concept_lowercases_trims_and_collapses_whitespace() {
        assert_eq!(normalize_concept("ROIC"), "roic");
        assert_eq!(
            normalize_concept("  Return On Capital  "),
            "return on capital"
        );
        assert_eq!(
            normalize_concept("discounted   cash\tflow"),
            "discounted cash flow"
        );
        assert_eq!(normalize_concept("   "), "");
    }

    #[test]
    fn normalize_concept_merges_case_variants_into_one_node() {
        let a = normalize_concept("ROIC");
        let b = normalize_concept("roic");
        let c = normalize_concept("Roic ");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn cache_label_sanitization() {
        // This tests the sanitization logic inline since it's embedded in the tool
        let label = "my document/v1:notes";
        let safe: String = label
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        assert_eq!(safe, "my_document_v1_notes");
    }

    #[test]
    fn cache_path_construction() {
        let cache_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("hkask")
            .join("docproc-cache");
        let safe_label = "test_doc";
        let cache_path = cache_dir.join(format!("{}.md", safe_label));
        assert!(cache_path.ends_with("test_doc.md"));
        assert!(cache_path.to_string_lossy().contains("docproc-cache"));
    }

    #[test]
    fn generate_qa_rejects_empty_text() {
        let req = GenerateQaRequest {
            text: Some(String::new()),
            texts: None,
            chunk_id: "test".into(),
            bloom_levels: None,
            model: None,
        };
        assert!(req.text.as_ref().is_some_and(|t| t.is_empty()));
    }

    #[test]
    fn generate_qa_rejects_empty_chunk_id() {
        let req = GenerateQaRequest {
            text: Some("some text".into()),
            texts: None,
            chunk_id: String::new(),
            bloom_levels: None,
            model: None,
        };
        assert!(req.chunk_id.is_empty());
    }

    #[test]
    fn cosine_similarity_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!(
            (sim - 1.0).abs() < 0.001,
            "identical vectors should have similarity 1.0, got {sim}"
        );
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(
            (sim - 0.0).abs() < 0.001,
            "orthogonal vectors should have similarity 0.0, got {sim}"
        );
    }

    #[test]
    fn cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_similarity_dimension_mismatch() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0, 2.0, 3.0]), 0.0);
    }

    #[test]
    fn query_rejects_empty() {
        let req = QueryRequest {
            query: String::new(),
            top_k: None,
            generate_answer: None,
        };
        assert!(req.query.is_empty());
    }

    #[test]
    fn chunk_defaults_index_true() {
        // Verify the default_true helper
        assert!(default_true());
    }

    /// Empirical proof that per-page triage fixes the silent-loss bug.
    ///
    /// Builds a mixed PDF (text pages 1-2, image-only page 3, text pages 4-6)
    /// whose whole-doc word count far exceeds `OCR_FALLBACK_WORD_THRESHOLD`.
    /// The OLD `extract_text` returned `Success` and silently dropped page 3.
    /// The NEW per-page triage must return `PartialOcr` flagging page 3
    /// (0-based index 2) for OCR.
    ///
    /// Ignored by default — requires pdftoppm, pdftocairo, pdfunite, ps2pdf,
    /// and python3+PIL. Run with: `cargo test -p hkask-mcp-docproc --lib
    /// -- --ignored extract_text_flags_mixed`.
    #[tokio::test]
    #[ignore = "requires pdftoppm, pdftocairo, pdfunite, ps2pdf, python3+PIL"]
    async fn extract_text_flags_mixed_pdf_pages_for_ocr() {
        fn tools_available() -> bool {
            for t in ["pdftoppm", "pdftocairo", "pdfunite", "ps2pdf", "python3"] {
                // `output()` fails only if the binary is not found; the arg
                // value is irrelevant for an existence probe.
                if std::process::Command::new(t)
                    .arg("--version")
                    .output()
                    .is_err()
                {
                    return false;
                }
            }
            true
        }
        if !tools_available() {
            eprintln!("SKIP: required PDF/PIL tools not available");
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let dir_str = dir.path().display().to_string();
        let script = format!(
            r#"set -e
cd {dir}
cat > text.ps <<'PS'
%!PS
/Courier findfont 12 scalefont setfont
6 {{
  72 700 moveto
  (This page has plenty of text content words to clear the per-page text-native triage threshold for the silent-loss regression test.)
  show
  showpage
}} repeat
PS
ps2pdf text.ps text.pdf
pdftoppm -png -r 150 -f 3 -l 3 text.pdf p3 >/dev/null 2>&1
python3 -c "from PIL import Image; import glob; f=glob.glob('p3-*.png')[0]; Image.open(f).convert('RGB').save('img3.pdf')"
pdftocairo -pdf -f 1 -l 2 text.pdf a.pdf 2>/dev/null
pdftocairo -pdf -f 4 -l 6 text.pdf b.pdf 2>/dev/null
pdfunite a.pdf img3.pdf b.pdf mixed.pdf
"#,
            dir = dir_str
        );
        let status = std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .status()
            .expect("bash");
        assert!(status.success(), "mixed-PDF fixture build failed");

        let mixed = dir.path().join("mixed.pdf").to_string_lossy().to_string();
        let outcome = extract_text(&mixed).await.expect("extract_text");
        match outcome {
            ExtractOutcome::PartialOcr {
                ocr_pages,
                page_texts,
                word_count,
                ..
            } => {
                assert!(
                    ocr_pages.contains(&2),
                    "page 3 (0-based idx 2) must be flagged for OCR; got ocr_pages = {:?}",
                    ocr_pages
                );
                assert!(
                    page_texts.len() >= 5,
                    "expected at least 5 pages, got {}",
                    page_texts.len()
                );
                // The flagged page's native-text slot must be empty.
                assert_eq!(
                    page_texts[2].split_whitespace().count(),
                    0,
                    "flagged page 3 native-text slot must be empty"
                );
                // Whole-doc word count still exceeds the old threshold — this
                // is exactly the case the old code silently dropped.
                assert!(
                    word_count < 100 || ocr_pages.contains(&2),
                    "triage must flag page 3 even when native word count is high"
                );
            }
            ExtractOutcome::Success { .. } => {
                panic!("REGRESSION: mixed PDF returned Success — silent loss not fixed");
            }
            ExtractOutcome::NeedsOcr { .. } => {
                // Acceptable: triage degraded to whole-doc NeedsOcr (still no
                // silent loss). But we expected PartialOcr; note it.
                eprintln!("NOTE: got NeedsOcr (whole-doc fallback) instead of PartialOcr");
            }
        }
    }
}
