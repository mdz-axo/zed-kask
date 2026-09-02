//! Educt transcript-store tools — persist and recall transcripts + layers.
//!
//! Gap 1 of the local-mode scaffold
//! (`tasks/reduct-dual-mode-video-analysis.md`): transcripts stop being
//! per-call artifacts dropped after the conversation and become queryable
//! objects keyed to their media path and optional gallery asset. All six
//! tools are local-only — no inference, no network, no Reduct; the store
//! is the media server's own SQLite (design doc §1.4).

use crate::transcript::TranscriptBundle;
use crate::transcript_layers::{EdlLayer, HighlightEntry, LayerProvenance, TranscriptLayer};
use crate::transcript_pass::{self, PassError, PassMode};
use crate::transcript_select::{
    Edl, EdlEntry, EdlOp, WordRange, edl_to_clip_plan, text_to_word_ranges, union_ranges,
    word_range_to_time_range,
};
use crate::transcript_store::{self, TranscriptFilter, TranscriptStoreError};
use crate::types::{
    EductApplyCorrectionsRequest, EductCorrectionPassRequest, EductDeleteTranscriptRequest,
    EductEdlFromHighlightsRequest, EductExportRequest, EductGetTranscriptRequest,
    EductHighlightPassRequest, EductListLayersRequest, EductLocateRequest,
    EductParagraphPassRequest, EductRenderEdlRequest, EductSpeakerPassRequest,
    EductStoreLayerRequest, EductStoreTranscriptRequest,
};
use crate::*;

/// Map store errors to MCP wire-level kinds per-variant (never a blanket
/// internal): a missing transcript is NotFound; a validation failure is
/// InvalidArgument carrying the named invariant; a DB failure is Internal.
fn map_store_error(error: TranscriptStoreError) -> McpToolError {
    match error {
        TranscriptStoreError::TranscriptNotFound { transcript_id } => {
            McpToolError::not_found(format!("transcript {transcript_id} not found"))
        }
        TranscriptStoreError::Validation(validation) => {
            McpToolError::invalid_argument(format!("layer rejected: {validation}"))
        }
        TranscriptStoreError::Serialization(message) => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
        TranscriptStoreError::Db(error) => {
            McpToolError::internal(format!("transcript store: {error}")) // rr0044-ok: infra-db-failure
        }
    }
}

/// Map pass errors per-variant: precondition failures are InvalidArgument
/// (named), unparseable model output is Unavailable (retryable with a
/// different model), inference failures classify through the canonical
/// helper, prompt-construction failures are Internal.
fn map_pass_error(error: PassError) -> McpToolError {
    match error {
        PassError::NoWordTimings => McpToolError::invalid_argument(
            "transcript has no word-level timings; the paragraph pass cannot anchor \
             (NoWordTimings)",
        ),
        PassError::UnparseableOutput { expected, raw } => {
            McpToolError::unavailable(format!("model output failed to parse as {expected}: {raw}"))
        }
        PassError::Validation(validation) => {
            McpToolError::invalid_argument(format!("layer rejected: {validation}"))
        }
        PassError::Prompt(message) => {
            McpToolError::internal(format!("prompt construction: {message}")) // rr0044-ok: own prompt construction
        }
        PassError::Inference(error) => classify_inference_error("transcript pass failed", error),
    }
}

/// Normalize a loosely-typed (AnyJsonValue) tool input that may arrive
/// stringified: some MCP transports serialize object params into their
/// JSON-string form. Accept both — the parsed object or the string form —
/// so every client can store transcripts and layers.
fn parse_json_value(
    value: serde_json::Value,
    what: &str,
) -> Result<serde_json::Value, McpToolError> {
    match value {
        serde_json::Value::String(text) => serde_json::from_str(&text).map_err(|e| {
            McpToolError::invalid_argument(format!(
                "{what} must be valid JSON (object or JSON-string form): {e}"
            ))
        }),
        other => Ok(other),
    }
}

/// Whether a media path is an audio file (extension check) — selects the
/// audio trim/concat render path over the video path.
fn is_audio_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    [".wav", ".mp3", ".flac", ".m4a", ".ogg", ".aac", ".wma"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

#[tool_router(router = educt_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Persist a TranscriptBundle (exactly as returned by transcribe_bundle) as a queryable transcript record, keyed to its media path and an optional gallery asset. Returns the transcript summary with its ID. Transcripts without word-level timings are stored with a surfaced degradation note — text and segments remain usable, but layers cannot anchor to them."
    )]
    pub async fn educt_store_transcript(
        &self,
        Parameters(EductStoreTranscriptRequest {
            transcript,
            gallery_asset_id,
        }): Parameters<EductStoreTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_store_transcript", async {
            let value = parse_json_value(transcript.0, "transcript")?;
            let bundle: TranscriptBundle = serde_json::from_value(value).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "transcript must be valid TranscriptBundle JSON (as returned \
                         by transcribe_bundle): {e}"
                ))
            })?;
            let driver = &**self.gallery_store.driver();
            let summary =
                transcript_store::store_transcript(driver, &bundle, gallery_asset_id.as_deref())
                    .map_err(map_store_error)?;
            let mut result = serde_json::to_value(&summary)
                .map_err(|e| McpToolError::internal(format!("serialize summary: {e}")))?; // rr0044-ok: serde serialization of own data
            if !summary.has_word_timings {
                result["degradation"] = serde_json::json!(
                    "no word-level timings — stored for text/segments only; layers \
                     cannot anchor (NoWordTimings)"
                );
            }
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "List stored transcripts, optionally filtered by media path or gallery asset ID, newest first. Each summary carries words_count and has_word_timings so degradations are visible per record."
    )]
    pub async fn educt_list_transcripts(
        &self,
        Parameters(EductListTranscriptsRequest {
            media_path,
            gallery_asset_id,
            limit,
        }): Parameters<EductListTranscriptsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_list_transcripts", async {
            let driver = &**self.gallery_store.driver();
            let filter = TranscriptFilter {
                media_path,
                gallery_asset_id,
                limit: limit.unwrap_or(50).min(500) as usize,
            };
            let summaries =
                transcript_store::list_transcripts(driver, &filter).map_err(map_store_error)?;
            Ok(serde_json::json!({
                "transcripts": summaries,
                "count": summaries.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Get a stored transcript by ID: its summary, the full TranscriptBundle, and (by default) its layers."
    )]
    pub async fn educt_get_transcript(
        &self,
        Parameters(EductGetTranscriptRequest {
            transcript_id,
            include_layers,
        }): Parameters<EductGetTranscriptRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_get_transcript", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            let mut result = serde_json::json!({
                "summary": summary,
                "transcript": bundle,
            });
            if include_layers.unwrap_or(true) {
                let layers = transcript_store::list_layers(driver, &transcript_id)
                    .map_err(map_store_error)?;
                result["layers"] = serde_json::json!(layers);
            }
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Delete a stored transcript and all of its layers (layers are removed first — a failure can leave a transcript without layers, never an orphaned layer). Returns removal counts; not-found when neither existed."
    )]
    pub async fn educt_delete_transcript(
        &self,
        Parameters(EductDeleteTranscriptRequest { transcript_id }): Parameters<
            EductDeleteTranscriptRequest,
        >,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_delete_transcript", async {
            let driver = &**self.gallery_store.driver();
            let counts = transcript_store::delete_transcript(driver, &transcript_id)
                .map_err(map_store_error)?;
            if counts.transcripts_removed == 0 && counts.layers_removed == 0 {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            }
            Ok(serde_json::json!({
                "transcripts_removed": counts.transcripts_removed,
                "layers_removed": counts.layers_removed,
            }))
        })
        .await
    }

    #[tool(
        description = "Store a layer over a transcript's words: {\"kind\": \"speaker\"|\"paragraph\"|\"correction\"|\"highlight\"|\"edl\", ...}. The layer is validated against the transcript's word count before storage — a layer that fails validation is rejected with the named failing invariant and nothing is persisted. Layers anchor to word indices, never timestamps."
    )]
    pub async fn educt_store_layer(
        &self,
        Parameters(EductStoreLayerRequest {
            transcript_id,
            layer,
        }): Parameters<EductStoreLayerRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_store_layer", async {
            let value = parse_json_value(layer.0, "layer")?;
            let parsed: TranscriptLayer = serde_json::from_value(value).map_err(|e| {
                McpToolError::invalid_argument(format!(
                    "layer must be a tagged layer JSON object {{\"kind\": \
                     \"speaker\"|\"paragraph\"|\"correction\"|\"highlight\"|\"edl\", \
                     ...}}: {e}"
                ))
            })?;
            let driver = &**self.gallery_store.driver();
            let record = transcript_store::store_layer(driver, &transcript_id, &parsed)
                .map_err(map_store_error)?;
            Ok(serde_json::json!({ "stored": record }))
        })
        .await
    }

    #[tool(
        description = "List the layers stored over a transcript, oldest first. Each record carries the layer kind, its provenance (model, prompt template, created_at), and the payload."
    )]
    pub async fn educt_list_layers(
        &self,
        Parameters(EductListLayersRequest { transcript_id }): Parameters<EductListLayersRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_list_layers", async {
            let driver = &**self.gallery_store.driver();
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            Ok(serde_json::json!({
                "layers": layers,
                "count": layers.len(),
            }))
        })
        .await
    }

    #[tool(
        description = "Run the paragraph pass over a stored transcript: an LLM identifies paragraph boundaries as word indices (never timestamps — the deterministic layer maps indices to time). The output is validated against the transcript's word count and stored as a provenance-carrying ParagraphLayer. The response carries pass stats (attempts/rejections/rejection_rate) — the measured v1 failure rate."
    )]
    pub async fn educt_paragraph_pass(
        &self,
        Parameters(EductParagraphPassRequest {
            transcript_id,
            model,
            structured,
        }): Parameters<EductParagraphPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_paragraph_pass", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; the paragraph pass cannot \
                     anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                     first",
                ));
            }
            let mode = if structured.unwrap_or(false) {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            let layer = transcript_pass::run_paragraph_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
                mode,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Paragraph(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::paragraph_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Run the speaker pass over a stored transcript. Source \"audio\" (default): an audio-capable model hears the recording and attributes speaker turns as word-index spans — the primary source. Source \"text\": the text-cue pass (works with every model, approximate). Both produce the same validated SpeakerLayer; provenance records which source produced it. The response carries pass stats."
    )]
    pub async fn educt_speaker_pass(
        &self,
        Parameters(EductSpeakerPassRequest {
            transcript_id,
            model,
            source,
            structured,
        }): Parameters<EductSpeakerPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_speaker_pass", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; the speaker pass cannot \
                     anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                     first",
                ));
            }
            // Structured outputs apply to the text passes only — the
            // audio path is prompt-schema (rejected here, never a silent
            // no-op).
            let structured_requested = structured.unwrap_or(false);
            if structured_requested && !matches!(source.as_deref(), Some("text")) {
                return Err(McpToolError::invalid_argument(
                    "structured outputs apply to the text passes (source \"text\"); the \
                     audio path is prompt-schema",
                ));
            }
            let mode = if structured_requested {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            // Source dispatch: "audio" (default — the scaffold's primary,
            // decision 7) or "text" (the text-cue pass). An audio failure
            // surfaces as its own error — retry with source "text" is the
            // operator's choice, never a silent fallback.
            let layer = match source.as_deref() {
                None | Some("audio") => transcript_pass::run_speaker_pass_audio(
                    &self.vision_port,
                    &self.template_env,
                    &bundle,
                    model.as_deref(),
                )
                .await
                .map_err(map_pass_error)?,
                Some("text") => transcript_pass::run_speaker_pass(
                    &self.vision_port,
                    &self.template_env,
                    &bundle,
                    mode,
                    model.as_deref(),
                )
                .await
                .map_err(map_pass_error)?,
                Some(other) => {
                    return Err(McpToolError::invalid_argument(format!(
                        "source must be \"audio\" or \"text\", got \"{other}\""
                    )));
                }
            };
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Speaker(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::speaker_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Run the correction pass over a stored transcript: an LLM proposes text replacements over word ranges for likely speech-to-text errors (mishearings, homophones, garbled fragments). Edits are proposals — timings are never touched, and applying them (educt_apply_corrections) produces a derived text view while the original words stay immutable. The output is validated (disjoint, in-bounds) and stored as a CorrectionLayer. The response carries pass stats."
    )]
    pub async fn educt_correction_pass(
        &self,
        Parameters(EductCorrectionPassRequest {
            transcript_id,
            model,
            structured,
        }): Parameters<EductCorrectionPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_correction_pass", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; the correction pass cannot \
                     anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                     first",
                ));
            }
            let mode = if structured.unwrap_or(false) {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            let layer = transcript_pass::run_correction_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
                mode,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Correction(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::correction_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Apply a stored correction layer to its transcript: returns the corrected text view — a pure projection recomputable from the layer at any time (the immutable words and timings are never modified). Defaults to the latest correction layer; pass layer_id to apply a specific one."
    )]
    pub async fn educt_apply_corrections(
        &self,
        Parameters(EductApplyCorrectionsRequest {
            transcript_id,
            layer_id,
        }): Parameters<EductApplyCorrectionsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_apply_corrections", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; corrections cannot anchor \
                     (NoWordTimings)",
                ));
            }
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let mut correction_layers: Vec<_> = layers
                .into_iter()
                .filter(|record| record.layer.kind() == "correction")
                .collect();
            // Newest first (RFC 3339 timestamps sort lexicographically).
            correction_layers.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let record = match layer_id {
                Some(id) => correction_layers.into_iter().find(|record| record.id == id),
                None => correction_layers.into_iter().next(),
            };
            let Some(record) = record else {
                return Err(McpToolError::not_found(format!(
                    "no correction layer found for transcript {transcript_id}"
                )));
            };
            let TranscriptLayer::Correction(correction) = &record.layer else {
                return Err(McpToolError::internal( // rr0044-ok: store-layer-kind-mismatch
                    "layer kind mismatch after correction filter",
                ));
            };
            let corrected =
                crate::transcript_layers::corrected_text_view(&bundle.words, &correction.edits);
            Ok(serde_json::json!({
                "corrected_text": corrected,
                "applied_layer": {
                    "id": record.id,
                    "provenance": record.layer.provenance(),
                    "edits": correction.edits.len(),
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Run the highlight pass — the semantic selection: a natural-language request (e.g. \"where he explains the Cinderella curve\") resolved to word ranges with theme labels. The output is validated (in-bounds) and stored as a HighlightLayer; overlapping selections are allowed. The response carries pass stats."
    )]
    pub async fn educt_highlight_pass(
        &self,
        Parameters(EductHighlightPassRequest {
            transcript_id,
            request,
            model,
            structured,
        }): Parameters<EductHighlightPassRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_highlight_pass", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; the highlight pass cannot \
                     anchor (NoWordTimings) — store a transcript from transcribe_bundle \
                     first",
                ));
            }
            let mode = if structured.unwrap_or(false) {
                PassMode::Structured
            } else {
                PassMode::PromptSchema
            };
            let layer = transcript_pass::run_highlight_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
                &request,
                mode,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
            let record = transcript_store::store_layer(
                driver,
                &transcript_id,
                &TranscriptLayer::Highlight(layer),
            )
            .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": record,
                "pass_stats": transcript_pass::highlight_pass_stats(),
            }))
        })
        .await
    }

    #[tool(
        description = "Compose an EDL (the Reel) from a stored highlight layer, deterministically: each selected highlight becomes a Keep op over its word range, and overlapping selections are union-merged so the Keep ops are disjoint. Optionally filter highlights by exact label; defaults to the latest highlight layer. The composed EDL is validated and stored as an EdlLayer — render it with educt_render_edl."
    )]
    pub async fn educt_edl_from_highlights(
        &self,
        Parameters(EductEdlFromHighlightsRequest {
            transcript_id,
            label,
            layer_id,
        }): Parameters<EductEdlFromHighlightsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_edl_from_highlights", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, _bundle)) =
                transcript_store::load_transcript(driver, &transcript_id)
                    .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; an EDL cannot anchor \
                     (NoWordTimings)",
                ));
            }
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let mut highlight_layers: Vec<_> = layers
                .into_iter()
                .filter(|record| record.layer.kind() == "highlight")
                .collect();
            // Newest first (RFC 3339 timestamps sort lexicographically).
            highlight_layers.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let record = match layer_id {
                Some(id) => highlight_layers.into_iter().find(|record| record.id == id),
                None => highlight_layers.into_iter().next(),
            };
            let Some(record) = record else {
                return Err(McpToolError::not_found(format!(
                    "no highlight layer found for transcript {transcript_id}"
                )));
            };
            let TranscriptLayer::Highlight(highlight) = &record.layer else {
                return Err(McpToolError::internal( // rr0044-ok: store-layer-kind-mismatch
                    "layer kind mismatch after highlight filter",
                ));
            };
            let selected: Vec<&HighlightEntry> = match label.as_deref() {
                Some(label) => highlight
                    .highlights
                    .iter()
                    .filter(|entry| entry.label == label)
                    .collect(),
                None => highlight.highlights.iter().collect(),
            };
            if selected.is_empty() {
                return Err(McpToolError::not_found(format!(
                    "no highlights{} in layer {}",
                    label
                        .as_deref()
                        .map(|label| format!(" labeled {label:?}"))
                        .unwrap_or_default(),
                    record.id
                )));
            }
            // Union-merge the selected ranges: overlapping highlights are
            // fine as annotations, but EDL Keep ops must be disjoint — the
            // union covers exactly the highlighted words.
            let ranges: Vec<WordRange> = selected
                .iter()
                .map(|entry| WordRange::new(entry.start_word, entry.end_word))
                .collect();
            let merged = union_ranges(&ranges);
            let ranges_merged = merged.len();
            let edl = EdlLayer {
                provenance: LayerProvenance {
                    model: "deterministic".to_string(),
                    prompt_template: "educt_edl_from_highlights".to_string(),
                    created_at: hkask_types::time::now_rfc3339(),
                },
                ops: merged
                    .into_iter()
                    .map(|range| EdlEntry {
                        range,
                        op: EdlOp::Keep,
                    })
                    .collect(),
            };
            let stored =
                transcript_store::store_layer(driver, &transcript_id, &TranscriptLayer::Edl(edl))
                    .map_err(map_store_error)?;
            Ok(serde_json::json!({
                "stored": stored,
                "composed_from": {
                    "layer_id": record.id,
                    "highlights_selected": selected.len(),
                    "ranges_merged": ranges_merged,
                    "label_filter": label,
                },
            }))
        })
        .await
    }

    #[tool(
        description = "Render a stored EDL layer to a media file: the selection algebra maps the EDL's word ranges to time ranges, then the existing ffmpeg stream-copy path clips each range and concatenates them — lossless, no re-encode. Audio media (wav/mp3/…) uses the audio trim/concat path; everything else uses the video path. Defaults to the latest EDL layer."
    )]
    pub async fn educt_render_edl(
        &self,
        Parameters(EductRenderEdlRequest {
            transcript_id,
            layer_id,
        }): Parameters<EductRenderEdlRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_render_edl", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; an EDL cannot render \
                     (NoWordTimings)",
                ));
            }
            let layers =
                transcript_store::list_layers(driver, &transcript_id).map_err(map_store_error)?;
            let mut edl_layers: Vec<_> = layers
                .into_iter()
                .filter(|record| record.layer.kind() == "edl")
                .collect();
            edl_layers.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let record = match layer_id {
                Some(id) => edl_layers.into_iter().find(|record| record.id == id),
                None => edl_layers.into_iter().next(),
            };
            let Some(record) = record else {
                return Err(McpToolError::not_found(format!(
                    "no EDL layer found for transcript {transcript_id}"
                )));
            };
            let TranscriptLayer::Edl(edl) = &record.layer else {
                return Err(McpToolError::internal( // rr0044-ok: store-layer-kind-mismatch
                    "layer kind mismatch after EDL filter",
                ));
            };
            // The slice-1 algebra: EDL → keep ranges → clip plan (the only
            // index→time mapping; the render consumes its output).
            let selection_edl = Edl {
                ops: edl.ops.clone(),
            };
            let plan_ms = edl_to_clip_plan(&bundle.words, &selection_edl).map_err(|error| {
                McpToolError::invalid_argument(format!("EDL rejected: {error}"))
            })?;
            if plan_ms.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "EDL cuts the entire transcript — nothing to render",
                ));
            }
            let plan_secs: Vec<(f64, f64)> = plan_ms
                .iter()
                .map(|(start_ms, end_ms)| (*start_ms as f64 / 1000.0, *end_ms as f64 / 1000.0))
                .collect();

            self.require_ffmpeg()?;
            let media_path = bundle.audio_path.clone();
            let audio = is_audio_path(&media_path);
            let mut clip_paths: Vec<String> = Vec::with_capacity(plan_secs.len());
            for (start_sec, end_sec) in &plan_secs {
                let clipped = if audio {
                    self.ffmpeg
                        .audio_trim(&media_path, *start_sec as f32, *end_sec as f32)
                        .await
                } else {
                    self.ffmpeg
                        .clip(&media_path, *start_sec as f32, *end_sec as f32)
                        .await
                }
                .map_err(map_media_error)?;
                clip_paths.push(clipped.display().to_string());
            }
            let output = if clip_paths.len() == 1 {
                std::path::PathBuf::from(clip_paths[0].clone())
            } else if audio {
                self.ffmpeg
                    .audio_concat(&clip_paths)
                    .await
                    .map_err(map_media_error)?
            } else {
                self.ffmpeg
                    .concat(&clip_paths)
                    .await
                    .map_err(map_media_error)?
            };

            let result = serde_json::json!({
                "status": "rendered",
                "source": media_path,
                "clip_plan": plan_secs,
                "clips": clip_paths.len(),
                "output": output.display().to_string(),
                "edl_layer": {
                    "id": record.id,
                    "provenance": record.layer.provenance(),
                },
            });
            let args = serde_json::json!({
                "transcript_id": transcript_id,
                "layer_id": record.id,
            });
            Ok(crate::media_block::enrich_with_omc_and_provenance(
                result,
                "educt_render_edl",
                if audio { "audio" } else { "video" },
                args,
                None,
            ))
        })
        .await
    }

    #[tool(
        description = "Export a stored transcript in a shareable or ingestable format. \"srt\": caption file from the word timings (cues split at sentence punctuation). \"highlights_csv\": every stored highlight as CSV rows with time ranges. \"corpus_text\": the rendered transcript text for corpus ingestion — run corpus_convert → corpus_chunk → corpus_embed on the exported file; corpus_query hits map back to word ranges over the stored transcript (repository-wide semantic search, by composition)."
    )]
    pub async fn educt_export(
        &self,
        Parameters(EductExportRequest {
            transcript_id,
            format,
        }): Parameters<EductExportRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_export", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            let dir = crate::assets::generated_assets_dir();
            match format.as_str() {
                "srt" => {
                    if !summary.has_word_timings {
                        return Err(McpToolError::invalid_argument(
                            "transcript has no word-level timings; SRT cues cannot anchor \
                             (NoWordTimings)",
                        ));
                    }
                    let srt = crate::transcript_export::srt_from_words(&bundle.words).map_err(
                        |error| McpToolError::invalid_argument(format!("SRT export: {error}")),
                    )?;
                    let cues = srt.matches("\n\n").count();
                    let path = dir.join(format!("educt-{transcript_id}.srt"));
                    std::fs::write(&path, &srt).map_err(|e| {
                        McpToolError::internal(format!("write {}: {e}", path.display())) // rr0044-ok: write to server-managed assets dir
                    })?;
                    Ok(serde_json::json!({
                        "status": "exported",
                        "format": "srt",
                        "output": path.display().to_string(),
                        "cues": cues,
                    }))
                }
                "highlights_csv" => {
                    let layers = transcript_store::list_layers(driver, &transcript_id)
                        .map_err(map_store_error)?;
                    let highlight_records: Vec<_> = layers
                        .into_iter()
                        .filter(|record| record.layer.kind() == "highlight")
                        .collect();
                    if highlight_records.is_empty() {
                        return Err(McpToolError::not_found(format!(
                            "no highlight layers found for transcript {transcript_id}"
                        )));
                    }
                    let rows = highlight_records
                        .iter()
                        .map(|record| match &record.layer {
                            TranscriptLayer::Highlight(highlight) => highlight.highlights.len(),
                            _ => 0,
                        })
                        .sum::<usize>();
                    let csv =
                        crate::transcript_export::highlights_csv(&bundle.words, &highlight_records)
                            .map_err(|error| {
                                McpToolError::invalid_argument(format!("CSV export: {error}"))
                            })?;
                    let path = dir.join(format!("educt-{transcript_id}-highlights.csv"));
                    std::fs::write(&path, &csv).map_err(|e| {
                        McpToolError::internal(format!("write {}: {e}", path.display())) // rr0044-ok: write to server-managed assets dir
                    })?;
                    Ok(serde_json::json!({
                        "status": "exported",
                        "format": "highlights_csv",
                        "output": path.display().to_string(),
                        "rows": rows,
                    }))
                }
                "corpus_text" => {
                    // The rendered form (words joined by single spaces) is
                    // what text_to_word_ranges matches — a corpus hit on
                    // this text maps back to word ranges exactly. Without
                    // word timings, fall back to the provider's full_text
                    // and surface the degradation (search works; clip
                    // mapping does not).
                    let (text, word_timings) = if summary.has_word_timings {
                        (
                            crate::transcript_select::rendered_transcript(&bundle.words),
                            true,
                        )
                    } else {
                        (bundle.full_text.clone(), false)
                    };
                    let path = dir.join(format!("educt-transcript-{transcript_id}.txt"));
                    std::fs::write(&path, &text).map_err(|e| {
                        McpToolError::internal(format!("write {}: {e}", path.display())) // rr0044-ok: write to server-managed assets dir
                    })?;
                    let mut result = serde_json::json!({
                        "status": "exported",
                        "format": "corpus_text",
                        "output": path.display().to_string(),
                        "words": bundle.words.len(),
                        "composition": "run corpus_convert → corpus_chunk → corpus_embed on this \
                         file; corpus_query hits map back to word ranges via \
                         text_to_word_ranges over the stored transcript",
                    });
                    if !word_timings {
                        result["degradation"] = serde_json::json!(
                            "no word-level timings — exported the provider's full_text; \
                             corpus hits cannot map back to word ranges (NoWordTimings)"
                        );
                    }
                    Ok(result)
                }
                other => Err(McpToolError::invalid_argument(format!(
                    "format must be \"srt\", \"highlights_csv\", or \"corpus_text\", got \
                     \"{other}\""
                ))),
            }
        })
        .await
    }

    #[tool(
        description = "Locate a quoted passage in a stored transcript, deterministically: returns every word-aligned match as a word range with its time range (start_ms/end_ms). Quote the rendered form exactly (punctuation included). Ambiguity is surfaced as all candidate ranges — never a guess; a surfaced no_match means the quote does not appear word-aligned. This is the mechanical mapping step for verified citations (e.g. the listening skill's evidence quotes): a verbatim substring of the transcript resolves to a media range with no model in the loop."
    )]
    pub async fn educt_locate(
        &self,
        Parameters(EductLocateRequest {
            transcript_id,
            text,
        }): Parameters<EductLocateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "educt_locate", async {
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) = transcript_store::load_transcript(driver, &transcript_id)
                .map_err(map_store_error)?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if !summary.has_word_timings {
                return Err(McpToolError::invalid_argument(
                    "transcript has no word-level timings; a quote cannot map to a media \
                     range (NoWordTimings)",
                ));
            }
            let ranges = text_to_word_ranges(&bundle.words, &text);
            if ranges.is_empty() {
                return Ok(serde_json::json!({
                    "status": "no_match",
                    "ranges": [],
                    "count": 0,
                    "note": "no word-aligned match — quote the rendered form exactly \
                             (punctuation included)",
                }));
            }
            let mut located = Vec::with_capacity(ranges.len());
            for range in ranges {
                let (start_ms, end_ms) =
                    word_range_to_time_range(&bundle.words, range).map_err(|error| {
                        McpToolError::internal(format!( // rr0044-ok: documented-impossible-invariant
                            "impossible: text_to_word_ranges produced an out-of-bounds \
                             range: {error}"
                        ))
                    })?;
                located.push(serde_json::json!({
                    "start_word": range.start_word,
                    "end_word": range.end_word,
                    "start_ms": start_ms,
                    "end_ms": end_ms,
                }));
            }
            Ok(serde_json::json!({
                "status": "located",
                "ranges": located,
                "count": located.len(),
            }))
        })
        .await
    }
}
