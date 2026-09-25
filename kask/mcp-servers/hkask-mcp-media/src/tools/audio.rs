//! Audio tools — voice design, speech generation, transcription, audio capture.
use crate::*;

const CAPTURE_SAMPLE_RATE: u32 = 16_000;
const CAPTURE_CHANNELS: u32 = 1;

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct AudioAdmissionGate {
    pub(crate) entered: std::sync::Arc<tokio::sync::Notify>,
    pub(crate) resume: std::sync::Arc<tokio::sync::Notify>,
}

#[cfg(test)]
static AUDIO_ADMISSION_GATE: std::sync::LazyLock<std::sync::Mutex<Option<AudioAdmissionGate>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

#[cfg(test)]
pub(crate) struct AudioAdmissionGateGuard;

#[cfg(test)]
impl Drop for AudioAdmissionGateGuard {
    fn drop(&mut self) {
        match AUDIO_ADMISSION_GATE.lock() {
            Ok(mut gate) => *gate = None,
            Err(error) => tracing::warn!(
                target: "hkask.mcp.media",
                %error,
                "Failed to clear audio admission test gate"
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn install_audio_admission_gate(
    gate: AudioAdmissionGate,
) -> Result<AudioAdmissionGateGuard, MediaError> {
    let mut installed = AUDIO_ADMISSION_GATE
        .lock()
        .map_err(|error| MediaError::Io(format!("audio admission gate lock: {error}")))?;
    *installed = Some(gate);
    Ok(AudioAdmissionGateGuard)
}

#[cfg(test)]
async fn pause_after_audio_admission() -> Result<(), McpToolError> {
    let gate = AUDIO_ADMISSION_GATE
        .lock()
        .map_err(|error| McpToolError::internal(format!("audio admission gate lock: {error}")))?
        .clone();
    let Some(gate) = gate else {
        return Ok(());
    };
    gate.entered.notify_one();
    gate.resume.notified().await;
    Ok(())
}

#[derive(serde::Serialize)]
struct AudioTrimEffectiveParams<'a> {
    source: &'a str,
    start_sec: f32,
    end_sec: f32,
    duration_sec: f32,
}

#[derive(serde::Serialize)]
struct AudioConcatEffectiveParams<'a> {
    sources: &'a [String],
}

#[derive(serde::Serialize)]
struct AudioCaptureEffectiveParams {
    duration_secs: f32,
    sample_rate: u32,
    channels: u32,
}

/// Parse provider word-timing entries into `TimedWord`s. Whisper-style
/// providers prefix tokens with the separator (" And"); the
/// rendered-form contract (words joined by single spaces, word-boundary
/// aligned) requires clean tokens, so trim each token and drop
/// whitespace-only entries. Storing space-prefixed tokens verbatim made
/// every quote no_match (observed live: `educt_locate` could not find
/// real passages in a verbatim-stored whisper bundle).
fn timed_words_from_provider(entries: &[serde_json::Value]) -> Vec<TimedWord> {
    entries
        .iter()
        .filter_map(|w| {
            let word = w.get("word")?.as_str()?.trim().to_string();
            if word.is_empty() {
                return None;
            }
            Some(TimedWord {
                word,
                start_ms: (w.get("start")?.as_f64()? * 1000.0) as u64,
                end_ms: (w.get("end")?.as_f64()? * 1000.0) as u64,
                confidence: w
                    .get("confidence")
                    .and_then(|c| c.as_f64())
                    .map(hkask_types::Confidence::new),
            })
        })
        .collect()
}

/// Build a `TranscriptBundle` from a raw provider transcription response.
/// Shared by `transcribe_bundle` and `record_and_transcribe` — the capture
/// path previously parsed the response inline WITHOUT the separator-prefix
/// trimming, so its stored words made every `educt_locate` quote no_match.
fn transcript_bundle_from_raw(
    raw: &serde_json::Value,
    audio_path: String,
    language: Option<String>,
    duration_fallback_secs: f64,
) -> TranscriptBundle {
    TranscriptBundle {
        format: "hkask-transcript-v1".to_string(),
        audio_path,
        audio_duration_secs: raw
            .get("duration")
            .and_then(|d| d.as_f64())
            .unwrap_or(duration_fallback_secs) as f32,
        full_text: raw
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        words: raw
            .get("words")
            .and_then(|w| w.as_array())
            .map(|arr| timed_words_from_provider(arr))
            .unwrap_or_default(),
        segments: raw
            .get("segments")
            .and_then(|s| s.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        Some(TranscriptSegment {
                            text: s.get("text")?.as_str()?.to_string(),
                            start_ms: (s.get("start")?.as_f64()? * 1000.0) as u64,
                            end_ms: (s.get("end")?.as_f64()? * 1000.0) as u64,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        language,
        model: raw
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
    }
}

#[tool_router(router = audio_router, vis = "pub")]
impl MediaServer {
    // ── Voice tools ──────────────────────────────────────────────────────────

    #[tool(
        description = "Design a synthetic voice profile from a character description. Returns a VoiceDesign JSON for use with generate_speech."
    )]
    pub async fn voice_design(
        &self,
        Parameters(VoiceDesignRequest {
            character_description,
        }): Parameters<VoiceDesignRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "voice_design", async {
            let _admission = self.admit_heavy_operation()?;
            if character_description.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "character_description must not be empty",
                ));
            }
            let mut vars = HashMap::new();
            vars.insert("character_description", character_description.as_str());
            let prompt = self
                .render_prompt("voice_design", &vars)
                .map_err(|e| McpToolError::internal(format!("Template render failed: {}", e)))?;

            // Fail-visible (the operator's no-hidden-models spec): the
            // transcript-pipeline model is the STT model — no configured
            // STT model is a typed error naming the setting, never a
            // hidden code constant.
            let model = crate::models::stt_model().ok_or_else(|| {
                McpToolError::permission_denied(format!(
                    "no STT model configured — set {} \
                     (injected from kask.media.stt_model); kask never falls back to a hidden \
                     code constant",
                    crate::models::STT_ENV
                ))
            })?;
            let params = hkask_types::template::LLMParameters::default();
            let r = self
                .vision_port
                .generate_with_model(&prompt, &params, Some(model.as_str()), None)
                .await
                .map_err(|e| classify_inference_error("Voice design inference failed", e))?;

            match serde_json::from_str::<serde_json::Value>(&r.text) {
                Ok(v) => Ok(serde_json::json!({
                    "voice_design": v,
                    "model": "llama-3.3-70b",
                })),
                Err(_) => Ok(serde_json::json!({
                    "voice_design": {"description": r.text.trim()},
                    "model": "llama-3.3-70b",
                    "warning": "LLM did not return valid JSON; using raw description."
                })),
            }
        })
        .await
    }

    #[tool(
        description = "Generate speech audio from text using a voice design. Returns the persisted audio file path."
    )]
    pub async fn generate_speech(
        &self,
        Parameters(GenerateSpeechRequest { text, voice_design }): Parameters<GenerateSpeechRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "generate_speech", async {
            let _admission = self.admit_heavy_operation()?;
            if text.trim().is_empty() {
                return Err(McpToolError::invalid_argument("text must not be empty"));
            }
            let voice = if let Some(ref vd_json) = voice_design {
                match serde_json::from_str::<VoiceDesign>(vd_json) {
                    Ok(vd) => vd.to_elevenlabs_voice().to_string(),
                    Err(_) => "Rachel".to_string(),
                }
            } else {
                "Rachel".to_string()
            };

            let media_params = hkask_types::MediaGenerateParams {
                text: Some(text.clone()),
                voice: Some(voice.clone()),
                ..Default::default()
            };
            let args = serde_json::to_value(&media_params).unwrap_or(serde_json::Value::Null);
            let result = self
                .vision_port
                .media_generate("generate_speech", &media_params)
                .await
                .map_err(|e| classify_inference_error("Speech generation failed", e))?;
            // Persist the audio payload and compose the slim result (the
            // base64 data URI never enters the model's context).
            persist_slim_and_enrich(
                &self.gallery_store,
                &result,
                "generate_speech",
                "audio",
                args,
            )
            .await
        })
        .await
    }

    // ── Audio tools ─────────────────────────────────────────────────────────

    #[tool(
        description = "Transcribe audio and return a synchronized TranscriptBundle with word-level timings (full_text carries the plain text). Enables interactive highlighting and click-to-seek in frontends, and is the ingest format for the educt transcript layers."
    )]
    pub async fn transcribe_bundle(
        &self,
        Parameters(TranscribeBundleRequest {
            audio_url,
            language,
        }): Parameters<TranscribeBundleRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "transcribe_bundle", async {
            let _admission = self.admit_heavy_operation()?;
            // Local recordings and fetched media are the primary transcript
            // sources; the SSRF validator is for network URLs (see
            // `is_local_media_path`).
            if !crate::is_local_media_path(&audio_url) {
                validate_tool_url_with_dns(&audio_url).await?;
            }

            let media_params = hkask_types::MediaGenerateParams {
                audio_url: Some(audio_url.clone()),
                language: language.clone(),
                ..Default::default()
            };
            let raw = self
                .vision_port
                .media_generate("transcribe", &media_params)
                .await
                .map_err(|e| classify_inference_error("Transcription failed", e))?;

            let bundle = transcript_bundle_from_raw(&raw, audio_url.clone(), language.clone(), 0.0);

            Ok(serde_json::to_value(&bundle)
                .unwrap_or_else(|_| serde_json::json!({"error": "Failed to serialize bundle"})))
        })
        .await
    }

    #[tool(
        description = "Transcribe audio AND store the TranscriptBundle server-side in one call, returning only the transcript summary (id, words_count, has_word_timings). For long media this is the path: an hour-long talk is ~550KB of bundle JSON, and relaying it through the model context (transcribe_bundle then educt_store_transcript) costs ~140K tokens each way. Accepts the same inputs as transcribe_bundle plus an optional gallery asset id; continue with the educt passes by transcript id."
    )]
    pub async fn transcribe_and_store(
        &self,
        Parameters(TranscribeAndStoreRequest {
            audio_url,
            language,
            gallery_asset_id,
        }): Parameters<TranscribeAndStoreRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "transcribe_and_store", async {
            let _admission = self.admit_heavy_operation()?;
            // Same local-path rule as transcribe_bundle: the SSRF validator
            // is for network URLs (see `is_local_media_path`).
            if !crate::is_local_media_path(&audio_url) {
                validate_tool_url_with_dns(&audio_url).await?;
            }

            let media_params = hkask_types::MediaGenerateParams {
                audio_url: Some(audio_url.clone()),
                language: language.clone(),
                ..Default::default()
            };
            let raw = self
                .vision_port
                .media_generate("transcribe", &media_params)
                .await
                .map_err(|e| classify_inference_error("Transcription failed", e))?;

            let bundle = transcript_bundle_from_raw(&raw, audio_url.clone(), language.clone(), 0.0);

            let driver = &**self.gallery_store.driver();
            let summary = crate::transcript_store::store_transcript(
                driver,
                &bundle,
                gallery_asset_id.as_deref(),
            )
            .map_err(crate::tools::educt::map_store_error)?;

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
        description = "Capture audio from the default system microphone and publish a canonical durable WAV asset optimized for Whisper transcription (16kHz mono)."
    )]
    pub async fn audio_capture(
        &self,
        Parameters(AudioCaptureRequest { duration_secs }): Parameters<AudioCaptureRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "audio_capture", async {
            let _admission = self.admit_heavy_operation()?;
            if duration_secs <= 0.0 || duration_secs > 3600.0 {
                return Err(McpToolError::invalid_argument(
                    "duration_secs must be between 0.1 and 3600 (1 hour).",
                ));
            }

            #[cfg(test)]
            pause_after_audio_admission().await?;
            self.require_ffmpeg()?;
            let effective_params = AudioCaptureEffectiveParams {
                duration_secs,
                sample_rate: CAPTURE_SAMPLE_RATE,
                channels: CAPTURE_CHANNELS,
            };
            let path = self
                .ffmpeg
                .capture_audio(effective_params.duration_secs)
                .await
                .map_err(map_media_error)?;

            crate::assets::publish_local_media(
                &self.gallery_store,
                &path,
                "audio_capture",
                "captured",
                crate::assets::LocalMediaFormat::Wav,
                &effective_params,
            )
        })
        .await
    }

    #[tool(
        description = "Record audio from microphone and transcribe it in one call. Returns linked audio file path and transcript. Use for meetings, notes, or any recording you want to keep."
    )]
    pub async fn record_and_transcribe(
        &self,
        Parameters(RecordAndTranscribeRequest {
            duration_secs,
            language,
        }): Parameters<RecordAndTranscribeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "record_and_transcribe", async {
            let _admission = self.admit_heavy_operation()?;
            if duration_secs <= 0.0 || duration_secs > 3600.0 {
                return Err(McpToolError::invalid_argument(
                    "duration_secs must be between 0.1 and 3600 (1 hour).",
                ));
            }

            self.require_ffmpeg()?;

            let audio_path = self
                .ffmpeg
                .capture_audio(duration_secs)
                .await
                .map_err(map_media_error)?;

            let audio_data = std::fs::read(&audio_path).map_err(|e| {
                hkask_mcp_server::map_io_error(e, "Failed to read captured audio")
            })?;
            let b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &audio_data);
            let audio_uri = format!("data:audio/wav;base64,{}", b64);

            let transcribe_result = {
                let media_params = hkask_types::MediaGenerateParams {
                    audio_url: Some(audio_uri.clone()),
                    language: language.clone(),
                    ..Default::default()
                };
                self.vision_port
                    .media_generate("transcribe", &media_params)
                    .await
                    .map_err(|e| classify_inference_error("Transcription failed", e))
            };

            match transcribe_result {
                Ok(raw) => {
                    let audio_path_str = audio_path.display().to_string();
                    let bundle = transcript_bundle_from_raw(
                        &raw,
                        audio_path_str,
                        language.clone(),
                        duration_secs as f64,
                    );

                    let result = serde_json::to_value(&bundle).unwrap_or_else(|_| {
                        serde_json::json!({"error": "Failed to serialize bundle"})
                    });
                    let args = serde_json::json!({
                        "duration_secs": duration_secs,
                        "language": language,
                    });
                    Ok(crate::media_block::enrich_with_omc_and_provenance(
                        result,
                        "record_and_transcribe",
                        "audio",
                        args,
                        None,
                    ))
                }
                Err(e) => {
                    let result = serde_json::json!({
                        "status": "partial",
                        "duration_secs": duration_secs,
                        "audio_path": audio_path.display().to_string(),
                        "audio_format": "wav",
                        "sample_rate": 16000,
                        "channels": 1,
                        "transcript_error": e.to_json_string(),
                        "message": "Audio captured successfully but transcription failed. The audio file is saved and can be transcribed later."
                    });
                    let args = serde_json::json!({
                        "duration_secs": duration_secs,
                        "language": language,
                    });
                    Ok(crate::media_block::enrich_with_omc_and_provenance(
                        result,
                        "record_and_transcribe",
                        "audio",
                        args,
                        None,
                    ))
                }
            }
        })
        .await
    }

    /// Trim an audio file to specified start/end times. Uses ffmpeg stream
    /// copy for fast, lossless trimming.
    #[tool(
        description = "Trim an audio file to specified start/end times and publish a canonical durable WAV asset using lossless ffmpeg stream copy."
    )]
    pub async fn audio_trim(
        &self,
        Parameters(AudioTrimRequest {
            audio_url,
            start_sec,
            end_sec,
        }): Parameters<AudioTrimRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "audio_trim", async {
            let _admission = self.admit_heavy_operation()?;
            if start_sec < 0.0 || end_sec <= start_sec {
                return Err(McpToolError::invalid_argument(
                    "start_sec must be >= 0 and end_sec must be > start_sec",
                ));
            }
            #[cfg(test)]
            pause_after_audio_admission().await?;
            if !crate::is_local_media_path(&audio_url) {
                validate_tool_url_with_dns(&audio_url).await?;
            }
            self.require_ffmpeg()?;
            let effective_params = AudioTrimEffectiveParams {
                source: &audio_url,
                start_sec,
                end_sec,
                duration_sec: end_sec - start_sec,
            };
            let output = self
                .ffmpeg
                .audio_trim(
                    effective_params.source,
                    effective_params.start_sec,
                    effective_params.end_sec,
                )
                .await
                .map_err(map_media_error)?;
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                "audio_trim",
                "trimmed",
                crate::assets::LocalMediaFormat::Wav,
                &effective_params,
            )
        })
        .await
    }

    /// Concatenate multiple audio files into one. Uses the ffmpeg concat
    /// demuxer for fast, lossless joining.
    #[tool(
        description = "Concatenate audio files and publish one canonical durable WAV asset using the lossless ffmpeg concat demuxer."
    )]
    pub async fn audio_concat(
        &self,
        Parameters(AudioConcatRequest { audio_urls }): Parameters<AudioConcatRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "audio_concat", async {
            let _admission = self.admit_heavy_operation()?;
            validate_item_count(
                "audio_urls",
                audio_urls.len(),
                1,
                hkask_types::media_limits::MAX_CONCAT_ITEMS,
            )?;
            #[cfg(test)]
            pause_after_audio_admission().await?;
            for url in &audio_urls {
                if !crate::is_local_media_path(url) {
                    validate_tool_url_with_dns(url).await?;
                }
            }
            self.require_ffmpeg()?;
            let effective_params = AudioConcatEffectiveParams {
                sources: &audio_urls,
            };
            let output = self
                .ffmpeg
                .audio_concat(effective_params.sources)
                .await
                .map_err(map_media_error)?;
            crate::assets::publish_local_media(
                &self.gallery_store,
                &output,
                "audio_concat",
                "concatenated",
                crate::assets::LocalMediaFormat::Wav,
                &effective_params,
            )
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whisper-style providers prefix tokens with the separator
    /// (" And"); ingestion must emit clean tokens (the rendered-form
    /// contract) and drop whitespace-only entries.
    #[test]
    fn timed_words_from_provider_trims_separator_prefixes() {
        let entries = serde_json::json!([
            {"word": " And", "start": 0.0, "end": 0.379},
            {"word": " ", "start": 0.379, "end": 0.4},
            {"word": "so", "start": 0.379, "end": 0.68}
        ]);
        let words = timed_words_from_provider(entries.as_array().expect("array"));
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "And");
        assert_eq!(words[0].start_ms, 0);
        assert_eq!(words[0].end_ms, 379);
        assert_eq!(words[1].word, "so");
        assert_eq!(words[1].start_ms, 379);
        assert_eq!(words[1].end_ms, 680);
    }
}
