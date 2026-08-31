//! Educt transcript-store tools — persist and recall transcripts + layers.
//!
//! Gap 1 of the local-mode scaffold
//! (`tasks/reduct-dual-mode-video-analysis.md`): transcripts stop being
//! per-call artifacts dropped after the conversation and become queryable
//! objects keyed to their media path and optional gallery asset. All six
//! tools are local-only — no inference, no network, no Reduct; the store
//! is the media server's own SQLite (design doc §1.4).

use crate::transcript::TranscriptBundle;
use crate::transcript_layers::TranscriptLayer;
use crate::transcript_pass::{self, PassError};
use crate::transcript_store::{self, TranscriptFilter, TranscriptStoreError};
use crate::types::{
    EductApplyCorrectionsRequest, EductCorrectionPassRequest, EductDeleteTranscriptRequest,
    EductGetTranscriptRequest, EductListLayersRequest, EductListTranscriptsRequest,
    EductParagraphPassRequest, EductSpeakerPassRequest, EductStoreLayerRequest,
    EductStoreTranscriptRequest,
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
        TranscriptStoreError::Serialization(message) => McpToolError::internal(message),
        TranscriptStoreError::Db(error) => {
            McpToolError::internal(format!("transcript store: {error}"))
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
            McpToolError::internal(format!("prompt construction: {message}"))
        }
        PassError::Inference(error) => classify_inference_error("paragraph pass failed", error),
    }
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
            let bundle: TranscriptBundle = serde_json::from_value(transcript.0).map_err(|e| {
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
                .map_err(|e| McpToolError::internal(format!("serialize summary: {e}")))?;
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
            let parsed: TranscriptLayer = serde_json::from_value(layer.0).map_err(|e| {
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
            let layer = transcript_pass::run_paragraph_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
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
        description = "Run the speaker pass over a stored transcript: an LLM infers speaker turns from textual cues (discourse markers, register shifts, role language) as word-index spans with confidence. Text-cue attribution is approximate — the confidence field carries that honestly. The output is validated (disjoint spans, in-bounds indices, non-empty labels) and stored as a provenance-carrying SpeakerLayer. The response carries pass stats."
    )]
    pub async fn educt_speaker_pass(
        &self,
        Parameters(EductSpeakerPassRequest {
            transcript_id,
            model,
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
            let layer = transcript_pass::run_speaker_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
                model.as_deref(),
            )
            .await
            .map_err(map_pass_error)?;
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
            let layer = transcript_pass::run_correction_pass(
                &self.vision_port,
                &self.template_env,
                &bundle,
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
                return Err(McpToolError::internal(
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
}
