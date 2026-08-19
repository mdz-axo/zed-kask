//! Document processing tools — convert, OCR, chunk.
//!
//! The heavy orchestration that previously lived here — `corpus_convert`'s
//! ~450-line OCR branching and the `chunk_directory` directory scanner — has
//! moved to `services::convert::ConvertService`. The `#[tool]` methods below
//! are now thin I/O framing: deserialize params, construct a `ConvertService`
//! borrowing `self`, delegate, and return. The shared OCR helpers
//! (`resolve_ocr_model`, `do_ocr`, `has_ocr`, `persist_pipeline_outcome`) and
//! `index_passages` also live on `ConvertService`, so `corpus_ocr` and the
//! file-case `corpus_chunk` path delegate to the service for those calls too.
//!
//! `convert_directory` stays here (on `CorpusServer`) because it recurses
//! through the `corpus_convert` tool wrapper to preserve per-file Regulation
//! spans; it does not call the OCR helpers directly.
use crate::helpers::map_corpus_io_error;
use crate::services::convert::ConvertService;
use crate::{
    CorpusServer, ExtractOutcome, McpToolError, Parameters, chunk_structure, chunk_word_bounds,
    convert, execute_tool_semantic, extract_text, filter_outcome_to_pages,
    json, sanitize_links, serialize_passages, tokens_to_words, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[tool_router(router = document_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Extract text from a document or directory. Detects format and automatically falls back to OCR for scanned PDFs. Directory conversion requires an output directory, persists one .txt file per supported source, and resumes non-empty outputs."
    )]
    pub async fn corpus_convert(
        &self,
        Parameters(ConvertRequest {
            path,
            output,
            force_ocr,
            target_pages,
        }): Parameters<ConvertRequest>,
    ) -> String {
        if std::path::Path::new(&path).is_dir() {
            return self
                .convert_directory(&path, output.as_deref(), force_ocr)
                .await;
        }

        execute_tool_semantic(
            self,
            "corpus_convert",
            Self::ontology_anchor("corpus_convert"),
            async {
                ConvertService::from_corpus(self)
                    .convert(path, force_ocr, target_pages)
                    .await
            },
        )
        .await
    }

    #[tool(
        description = "OCR a document using a local vision model. Requires HKASK_OCR_MODEL env var or explicit model parameter. The model must be a vision-capable model available in the inference catalog."
    )]
    pub async fn corpus_ocr(
        &self,
        Parameters(OcrRequest {
            path,
            model,
            
        }): Parameters<OcrRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_ocr",
            Self::ontology_anchor("corpus_ocr"),
            async {
                let resolved = crate::path_safety::contain_for_read(&path)?;

                let service = ConvertService::from_corpus(self);
                let model = match service.resolve_ocr_model(model.as_deref()).await {
                    Ok(m) => m,
                    Err(guidance) => {
                        return Err(McpToolError::failed_precondition(guidance.to_string()));
                    }
                };

                let file_bytes = match std::fs::read(&resolved) {
                    Ok(b) => b,
                    Err(e) => {
                        return Err(map_corpus_io_error(
                            e,
                            &format!("Failed to read file '{}'", path),
                        ));
                    }
                };

                match service.do_ocr(&file_bytes, &model).await {
                    Ok(text) => {
                        let result = serde_json::json!({
                            "path": path,
                            "model": model,
                            "text": text,
                            "word_count": text.split_whitespace().count(),
                        });
                        Ok(result)
                    }
                    Err(e) => Err(McpToolError::unavailable(e.to_string())),
                }
            },
        )
        .await
    }

    /// Cheaply check whether a PDF needs OCR or heavier parsing, *before*
    /// committing to a full parse. Runs a text-layer pass (`pdftotext`) plus an
    /// image inventory (`pdfimages`) -- no page rendering -- and classifies each
    /// page as text-native or needing OCR, with typed reasons.
    ///
    /// The docproc-native analogue of LiteParse's `lit is-complex`. Use it to
    /// route, reject, or estimate cost before calling `corpus_convert`.
    /// Emits `reg.pipeline.triage` spans. PDF only.
    #[tool(
        description = "Check whether a PDF needs OCR before a full parse. Returns per-page triage verdicts with typed reasons (scanned, no-text, sparse-text, embedded-images). PDF only. No page rendering -- cheap text-layer + image-inventory pass."
    )]
    pub async fn corpus_is_complex(
        &self,
        Parameters(IsComplexRequest { path, target_pages }): Parameters<IsComplexRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_is_complex",
            Self::ontology_anchor("corpus_is_complex"),
            async {
                let resolved = crate::path_safety::contain_for_read(&path)?;
                let (format, _, _) = convert::detect_format(&path);
                if format != "pdf" {
                    return Err(McpToolError::invalid_argument(
                        "corpus_is_complex supports PDF only",
                    ));
                }
                let cfg = crate::ocr::TriageConfig::from_env();
                let mut verdicts = crate::ocr::triage::triage_pdf(&resolved, &cfg)
                    .await
                    .map_err(crate::helpers::map_triage_error)?;

                if let Some(spec) = target_pages.as_deref().filter(|s| !s.trim().is_empty()) {
                    let target: std::collections::HashSet<usize> =
                        crate::ocr::triage::parse_target_pages(spec)
                            .map_err(|e| McpToolError::invalid_argument(e.to_string()))?
                            .into_iter()
                            .collect();
                    verdicts.retain(|v| target.contains(&v.page_number));
                }

                let needs_ocr = verdicts.iter().any(|v| v.needs_ocr);
                let ocr_page_count = verdicts.iter().filter(|v| v.needs_ocr).count();
                let pages: Vec<serde_json::Value> = verdicts
                    .iter()
                    .map(|v| {
                        serde_json::json!({
                            "page": v.page_number,
                            "word_count": v.word_count,
                            "needs_ocr": v.needs_ocr,
                            "reasons": v.reasons.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                tracing::info!(
                    target: "reg.pipeline.triage",
                    path = path,
                    pages = verdicts.len(),
                    ocr_pages = ocr_page_count,
                    needs_ocr,
                    "is-complex triage complete"
                );
                let result = serde_json::json!({
                    "path": path,
                    "pages": pages,
                    "page_count": verdicts.len(),
                    "ocr_pages": ocr_page_count,
                    "needs_ocr": needs_ocr,
                });
                Ok(result)
            },
        )
        .await
    }

    #[tool(
        description = "Chunk text into passages at configurable token granularity. Accepts raw text or a file path (extracts text from PDF/MD/HTML/TXT with OCR fallback for scanned PDFs). Supports single-tier or multi-tier (coarse/medium/fine) output."
    )]
    pub async fn corpus_chunk(
        &self,
        Parameters(ChunkRequest {
            text,
            path,
            input_dir,
            output,
            entity_ref_prefix,
            
            overlap_tokens,
            strip_gutenberg,
            multi_tier,
            coarse_max_tokens,
            medium_max_tokens,
            fine_max_tokens,
            index,
            target_pages,
        }): Parameters<ChunkRequest>,
    ) -> String {
        if let Some(input_dir) = input_dir {
            return execute_tool_semantic(
                self,
                "corpus_chunk",
                Self::ontology_anchor("corpus_chunk"),
                async {
                    ConvertService::from_corpus(self)
                        .chunk_directory(
                            &input_dir,
                            output.as_deref(),
                            &entity_ref_prefix,
                            
                            overlap_tokens,
                            strip_gutenberg,
                            index,
                        )
                        .await
                },
            )
            .await;
        }

        execute_tool_semantic(
            self,
            "corpus_chunk",
            Self::ontology_anchor("corpus_chunk"),
            async {
                // Exactly one of text or path must be provided
                let has_text = text.as_ref().is_some_and(|t| !t.is_empty());
                let has_path = path.as_ref().is_some_and(|p| !p.is_empty());
                if has_text == has_path {
                    return Err(McpToolError::invalid_argument(
                        "Exactly one of 'text' or 'path' must be provided",
                    ));
                }

                if entity_ref_prefix.is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "entity_ref_prefix must not be empty",
                    ));
                }
                hkask_mcp_server::validate_identifier("entity_ref_prefix", &entity_ref_prefix, 256)
                    .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;

                let service = ConvertService::from_corpus(self);

                // Resolve the source text
                let source_text: String;
                let source_label: String;
                // Structure from office-format backends — enables section-aware chunking.
                let mut source_structure: Option<hkask_types::document::DocStructure> = None;

                if let Some(ref raw_text) = text
                    && !raw_text.is_empty()
                {
                    source_text = raw_text.clone();
                    source_label = entity_ref_prefix.clone();
                } else if let Some(ref file_path) = path
                    && !file_path.is_empty()
                {
                    // Use shared extract_text for format detection + text extraction
                    let mut extract_outcome = extract_text(file_path).await?;
                    if let Some(spec) = target_pages.as_deref().filter(|s| !s.trim().is_empty()) {
                        let target: std::collections::HashSet<usize> =
                            crate::ocr::triage::parse_target_pages(spec)
                                .map_err(|e| McpToolError::invalid_argument(e.to_string()))?
                                .into_iter()
                                .collect();
                        extract_outcome = filter_outcome_to_pages(extract_outcome, &target);
                    }
                    match extract_outcome {
                        ExtractOutcome::Success {
                            text: extracted,
                            structure: Some(doc_structure),
                            ..
                        } => {
                            // Preserve structure for section-aware chunking later.
                            let structure_text = doc_structure.text();
                            source_text = if structure_text.split_whitespace().count()
                                >= extracted.split_whitespace().count()
                            {
                                structure_text
                            } else {
                                extracted
                            };
                            source_structure = Some(doc_structure);
                        }
                        ExtractOutcome::Success {
                            text: extracted, ..
                        } => {
                            source_text = extracted;
                        }
                        ExtractOutcome::NeedsOcr {
                            partial_text,
                            word_count: _,
                        } => {
                            // Try OCR fallback; use partial_text if OCR unavailable/fails
                            if let Ok(model) = service.resolve_ocr_model(None).await {
                                let file_bytes = std::fs::read(file_path).map_err(|e| {
                                    map_corpus_io_error(
                                        e,
                                        &format!("Failed to read '{}'", file_path),
                                    )
                                })?;
                                match service
                                    .do_ocr(&file_bytes, &model)
                                    .await
                                {
                                    Ok(ocr_text) if !ocr_text.is_empty() => {
                                        source_structure = Some(
                                            crate::backend::markdown_to_structure(&ocr_text, "pdf"),
                                        );
                                        source_text = ocr_text;
                                    }
                                    _ => {
                                        source_text = partial_text;
                                    }
                                }
                            } else {
                                source_text = partial_text;
                            }
                        }
                        ExtractOutcome::PartialOcr {
                            page_texts,
                            ocr_pages,
                            ..
                        } => {
                            // Mixed PDF: some pages text-native, some need OCR.
                            // Chunk's selective-OCR optimization is deferred; for
                            // now, fall back to whole-doc OCR (like NeedsOcr) so no
                            // page is silently lost. The joined native text is the
                            // fallback if OCR is unavailable.
                            let partial = page_texts.join("\n\x0c");
                            if !ocr_pages.is_empty()
                                && let Ok(model) = service.resolve_ocr_model(None).await
                            {
                                let file_bytes = std::fs::read(file_path).map_err(|e| {
                                    map_corpus_io_error(
                                        e,
                                        &format!("Failed to read '{}'", file_path),
                                    )
                                })?;
                                match service
                                    .do_ocr(&file_bytes, &model)
                                    .await
                                {
                                    Ok(ocr_text) if !ocr_text.is_empty() => {
                                        source_structure = Some(
                                            crate::backend::markdown_to_structure(&ocr_text, "pdf"),
                                        );
                                        source_text = ocr_text;
                                    }
                                    _ => source_text = partial,
                                }
                            } else {
                                source_text = partial;
                            }
                        }
                    }
                    source_label = file_path.replace(['/', '\\', '.', ' '], "_");
                } else {
                    return Err(McpToolError::invalid_argument("No text or path provided"));
                }

                // Apply Gutenberg stripping if requested
                let processed = if strip_gutenberg.unwrap_or(false) {
                    crate::text::strip_gutenberg_headers(&source_text)
                } else {
                    source_text
                };
                let processed = sanitize_links(&processed);
                let processed = crate::convert::decode_html_entities(&processed);
                let processed = crate::convert::strip_html_comments(&processed);

                let boundary = ".!? ";

                if multi_tier.unwrap_or(false) {
                    // Multi-tier: coarse / medium / fine
                    let chunk_tier =
                        |tier: &str, max_tok: Option<usize>, default: usize| -> Vec<_> {
                            let w = tokens_to_words(max_tok.unwrap_or(default));
                            crate::text::chunk_text(
                                &processed,
                                &format!("{source_label}:{tier}"),
                                w / 4,
                                w,
                                boundary,
                            )
                        };

                    let coarse = chunk_tier("coarse", coarse_max_tokens, 2048);
                    let medium = chunk_tier("medium", medium_max_tokens, 512);
                    let fine = chunk_tier("fine", fine_max_tokens, 128);

                    let result = json!({
                        "source": source_label,
                        "multi_tier": true,
                        "coarse_max_tokens": coarse_max_tokens.unwrap_or(2048),
                        "medium_max_tokens": medium_max_tokens.unwrap_or(512),
                        "fine_max_tokens": fine_max_tokens.unwrap_or(128),
                        "coarse": serialize_passages(&coarse),
                        "medium": serialize_passages(&medium),
                        "fine": serialize_passages(&fine),
                    });

                    // Auto-index if requested
                    let indexed = if index {
                        let all: Vec<_> = coarse.into_iter().chain(medium).chain(fine).collect();
                        service.index_passages(&all, &source_label).await
                    } else {
                        0
                    };

                    let mut result = result;
                    result["indexed"] = json!(indexed);
                    Ok(result)
                } else {
                    // Single-tier
                    let (max_words, min_words) = chunk_word_bounds(max_tokens, overlap_tokens);

                    // Use structure-aware chunking when a DocStructure is available
                    // (office formats). Falls back to flat chunk_text otherwise.
                    let passages = if let Some(ref structure) = source_structure {
                        chunk_structure(
                            structure,
                            &entity_ref_prefix,
                            min_words,
                            max_words,
                            boundary,
                        )
                    } else {
                        crate::text::chunk_text(
                            &processed,
                            &entity_ref_prefix,
                            min_words,
                            max_words,
                            boundary,
                        )
                    };

                    let total_passages = passages.len();
                    let serialized = serialize_passages(&passages);

                    // Auto-index if requested
                    let indexed = if index {
                        service.index_passages(&passages, &source_label).await
                    } else {
                        0
                    };

                    let result = json!({
                        "source": source_label,
                        "multi_tier": false,
                        "total_passages": total_passages,
                        "passages": serialized,
                        "max_tokens": max_tokens.unwrap_or(512),
                        "overlap_tokens": overlap_tokens.unwrap_or(64),
                        "max_words": max_words,
                        "min_words": min_words,
                        "sentence_boundary": boundary,
                        "stripped_gutenberg": strip_gutenberg.unwrap_or(false),
                        "indexed": indexed,
                    });
                    Ok(result)
                }
            },
        )
        .await
    }
}

impl CorpusServer {
    /// expect: "The corpus pipeline uses hKask MCP servers, not external scripts."
    /// [P7] Motivating: Composable Systems — one MCP call executes the manifest's directory conversion step.
    /// pre: `path` names a readable directory and `output` names its destination directory
    /// post: each supported source has a non-empty `.txt` output or an entry in `failures`
    /// inv: existing outputs larger than 50 bytes are preserved unchanged
    /// [P3] Constraining: Generative Space — batch progress and failures remain visible in the tool result.
    async fn convert_directory(&self, path: &str, output: Option<&str>, force_ocr: bool) -> String {
        execute_tool_semantic(self, "corpus_convert", Self::ontology_anchor("corpus_convert"), async {
            hkask_mcp_server::validate_path("path", path, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
            let path = crate::path_safety::contain_for_read(path)?;
            let output = output.ok_or_else(|| {
                McpToolError::invalid_argument(
                    "'output' directory is required when 'path' is a directory",
                )
            })?;
            hkask_mcp_server::validate_path("output", output, 4096)
                .map_err(|e| McpToolError::new(e.kind, e.to_json_string()))?;
            let output = crate::path_safety::contain_for_write(output)?;

            let output_dir = output.as_path();
            std::fs::create_dir_all(output_dir).map_err(|e| {
                map_corpus_io_error(
                    e,
                    &format!("Failed to create output directory '{}'", output_dir.display()),
                )
            })?;

            let mut sources = std::fs::read_dir(&path)
                .map_err(|e| {
                    map_corpus_io_error(e, &format!("Failed to read directory '{}'", path.display()))
                })?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|entry| entry.is_file() && is_supported_document(entry))
                .collect::<Vec<_>>();
            sources.sort();

            if sources.is_empty() {
                return Err(McpToolError::invalid_argument(format!(
                    "Directory '{}' contains no supported documents",
                    path.display()
                )));
            }

            let mut extracted = 0usize;
            let mut skipped = 0usize;
            let mut failures = Vec::new();

            for source in &sources {
                let Some(file_name) = source.file_name() else {
                    continue;
                };
                let output_path = output_dir.join(format!("{}.txt", file_name.to_string_lossy()));
                if output_path
                    .metadata()
                    .is_ok_and(|metadata| metadata.len() > 50)
                {
                    skipped += 1;
                    continue;
                }

                let response = Box::pin(self.corpus_convert(Parameters(ConvertRequest {
                    path: source.to_string_lossy().into_owned(),
                    output: None,
                    force_ocr,
                    target_pages: None,
                })))
                .await;

                let content = hkask_types::tool_response::parse_tool_response(&response);
                let text = content
                    .as_ref()
                    .and_then(|value| value.get("text"))
                    .and_then(serde_json::Value::as_str);

                match text {
                    Some(text) if !text.trim().is_empty() => {
                        if let Err(e) = std::fs::write(&output_path, text) {
                            failures.push(json!({
                                "path": source,
                                "error": format!("Failed to write '{}': {}", output_path.display(), e),
                            }));
                        } else {
                            extracted += 1;
                        }
                    }
                    _ => failures.push(json!({
                        "path": source,
                        "error": content
                            .unwrap_or_else(|| json!({"response": response})),
                    })),
                }
            }

            Ok(json!({
                "path": path,
                "output": output,
                "source_documents": sources.len(),
                "total_documents": extracted + skipped,
                "extracted": extracted,
                "skipped": skipped,
                "failed": failures.len(),
                "failures": failures,
            }))
        })
        .await
    }
}

fn is_supported_document(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "pdf" | "html" | "htm" | "md" | "txt" | "docx" | "pptx" | "xlsx" | "xls" | "csv"
            )
        })
}

// ── Request structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ConvertRequest {
    /// Path to a document file or a directory of documents to convert.
    pub path: String,
    /// Output directory for batch conversion. Required when `path` is a directory.
    #[serde(default)]
    pub output: Option<String>,
    /// If true, skip text extraction and go directly to OCR.
    #[serde(default)]
    pub force_ocr: bool,
    /// Target pages to parse (1-based), e.g. `"1-5,10,15-20"`. PDF only.
    /// When set, pages outside the set are skipped in extraction, triage, and
    /// OCR. Mirrors LiteParse's `--target-pages`. `None` = all pages.
    #[serde(default)]
    pub target_pages: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OcrRequest {
    /// Path to the document file to OCR.
    pub path: String,
    /// Vision model to use for OCR (must be available in the inference catalog).
    #[serde(default)]
    pub model: Option<String>,
    /// Maximum tokens for OCR output.
    #[serde(default = "default_ocr_max_tokens")]
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IsComplexRequest {
    /// Path to the PDF file to triage.
    pub path: String,
    /// Optional target pages (1-based), e.g. "1-5,10,15-20". None = all pages.
    #[serde(default)]
    pub target_pages: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ChunkRequest {
    /// Raw text to chunk. Mutually exclusive with `path` and `input_dir`.
    #[serde(default)]
    pub text: Option<String>,
    /// Path to a document file to extract text from and chunk.
    #[serde(default)]
    pub path: Option<String>,
    /// Directory of extracted text files to chunk as one corpus.
    #[serde(default)]
    pub input_dir: Option<String>,
    /// JSONL output path for directory mode. Required with `input_dir`.
    #[serde(default)]
    pub output: Option<String>,
    /// Prefix for entity references in chunk output.
    pub entity_ref_prefix: String,
    /// Max tokens per chunk (single-tier mode). Default: 256 from HkaskSettings.
    #[serde(default)]
    pub max_tokens: Option<usize>,
    /// Overlap tokens between chunks (single-tier mode, default 64).
    #[serde(default)]
    pub overlap_tokens: Option<usize>,
    /// Strip Project Gutenberg headers from text before chunking.
    #[serde(default)]
    pub strip_gutenberg: Option<bool>,
    /// If true, produce coarse/medium/fine multi-tier output instead of single-tier.
    #[serde(default)]
    pub multi_tier: Option<bool>,
    /// Max tokens for coarse tier (multi-tier mode, default 2048).
    #[serde(default)]
    pub coarse_max_tokens: Option<usize>,
    /// Max tokens for medium tier (multi-tier mode, default 512).
    #[serde(default)]
    pub medium_max_tokens: Option<usize>,
    /// Max tokens for fine tier (multi-tier mode, default 128).
    #[serde(default)]
    pub fine_max_tokens: Option<usize>,
    /// If true, automatically index passages for later query via corpus_query (default true).
    #[serde(default = "default_true")]
    pub index: bool,
    /// Target pages to parse (1-based) when `path` is a PDF, e.g. "1-5,10,15-20".
    /// Pages outside the set are skipped before chunking. Mirrors `corpus_convert`.
    #[serde(default)]
    pub target_pages: Option<String>,
}

pub(crate) fn default_true() -> bool {
    true
}
