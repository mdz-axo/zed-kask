//! Research audio tools — transcription, source-linked editing, and capture.
use crate::*;

const CAPTURE_SAMPLE_RATE: u32 = 16_000;
const CAPTURE_CHANNELS: u32 = 1;
/// ffmpeg receives the duration with one decimal place; anything shorter
/// would be sent as `-t 0.0`.
const MIN_CAPTURE_DURATION_SECS: f32 = 0.1;

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
///
/// Two key shapes parse here: Whisper's verbose_json keys the token as
/// `word` — and the live DeepInfra Nemotron-Transcribe response (the
/// default STT since 2026-10-29, `chunk_level: "word"`, verified against
/// the real endpoint) is the same `word`-keyed shape. DeepInfra's API
/// docs' example keys the token as `text`, so that shape is accepted
/// too.
fn timed_words_from_provider(entries: &[serde_json::Value]) -> Vec<TimedWord> {
    entries
        .iter()
        .filter_map(|w| {
            let word = w
                .get("word")
                .or_else(|| w.get("text"))?
                .as_str()?
                .trim()
                .to_string();
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
pub(super) fn transcript_bundle_from_raw(
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

/// Precondition a local transcription input for provider upload, shared
/// by the direct transcribe tools and the async job path (2026-10-25
/// repair; the 2026-10-26 review wired the missing job consumer): local
/// inputs above the threshold get their speech track extracted (16 kHz
/// mono — the profile the provider consumes internally), `file://` URIs
/// resolve to their path, and the ORIGINAL input remains the caller's
/// bundle identity (the extraction is a transport detail, never the
/// transcript's identity). `None` when the input transcribes as-is (small
/// local files and remote URLs). Fails closed with the named remediation
/// when even the extracted speech audio exceeds the provider cap; an
/// unavailable ffmpeg on the large-input path fails closed naming
/// transcription, not the video tools.
pub(crate) async fn precondition_local_stt_input(
    ffmpeg: Option<&FfmpegRunner>,
    audio_url: &str,
) -> Result<Option<String>, McpToolError> {
    if !crate::is_local_media_path(audio_url) {
        return Ok(None);
    }
    // `file://` URIs are local (is_local_media_path) but metadata needs
    // the stripped path — ensure_local_input_exists strips it, and the
    // 2026-10-26 review caught the unstripped metadata read silently
    // skipping preconditioning for exactly that input class.
    let path = std::path::Path::new(audio_url.strip_prefix("file://").unwrap_or(audio_url));
    let Ok(size) = std::fs::metadata(path) else {
        // The admission guard already surfaced a missing input.
        return Ok(None);
    };
    if size.len() <= MediaServer::STT_PRECONDITION_THRESHOLD_BYTES {
        return Ok(None);
    }
    let runner = ffmpeg.ok_or_else(|| {
        McpToolError::failed_precondition(
            "ffmpeg is required to extract the speech track before transcribing large local inputs — install ffmpeg or transcribe in shorter ranges",
        )
    })?;
    let extracted = runner
        .extract_speech_audio(path)
        .await
        .map_err(map_media_error)?;
    let extracted_size = std::fs::metadata(&extracted)
        .map_err(|e| {
            McpToolError::internal(format!(
                "read extracted artifact {}: {e}",
                extracted.display()
            ))
        })?
        .len();
    ensure_within_stt_upload_cap(size.len(), extracted_size)?;
    tracing::info!(
        target: "hkask.mcp.media.ffmpeg",
        source_bytes = size.len(),
        extracted_bytes = extracted_size,
        "STT input preconditioned"
    );
    Ok(Some(extracted.to_string_lossy().into_owned()))
}

/// The fail-closed upload cap (the provider's observed body cap with
/// headroom): extracted speech above it names the remediation (transcribe
/// in shorter ranges). Pure so the boundary is unit-pinned — the branch
/// itself was previously unasserted (2026-10-26 review).
fn ensure_within_stt_upload_cap(
    source_bytes: u64,
    extracted_bytes: u64,
) -> Result<(), McpToolError> {
    if extracted_bytes > MediaServer::STT_UPLOAD_LIMIT_BYTES {
        return Err(McpToolError::invalid_argument(format!(
            "speech audio is still {extracted_bytes} bytes after 16 kHz mono downmix \
             (source {source_bytes} bytes) — above the transcription provider's upload cap \
             ({} bytes). Transcribe the recording in shorter ranges.",
            MediaServer::STT_UPLOAD_LIMIT_BYTES
        )));
    }
    Ok(())
}

#[tool_router(router = audio_router, vis = "pub")]
impl MediaServer {
    // ── Audio tools ─────────────────────────────────────────────────

    /// Local inputs above this size get their speech track extracted
    /// before STT upload (16 kHz mono 32 kbps ≈ 20 MB per 80 minutes).
    /// Provider upload cap observed 2026-10-25: OpenRouter rejects request
    /// bodies over 50 MB (413) — a 274 MB video container was uploaded
    /// whole and failed after the full transfer.
    const STT_PRECONDITION_THRESHOLD_BYTES: u64 = 40 * 1024 * 1024;
    /// The upload cap with headroom: even extracted speech audio above this
    /// fails closed with the named remediation (transcribe in ranges).
    const STT_UPLOAD_LIMIT_BYTES: u64 = 48 * 1024 * 1024;

    /// Transcribe `audio_url` into a `TranscriptBundle`: admit the local
    /// input, validate network URLs for SSRF (local paths skip the network
    /// validator — see `is_local_media_path`), then run the provider
    /// transcription. The admission guard stays at each tool body so its
    /// scope covers the whole tool, not just the generate.
    pub(crate) async fn transcribe_to_bundle(
        &self,
        audio_url: &str,
        language: &Option<String>,
        model: Option<&str>,
    ) -> Result<TranscriptBundle, McpToolError> {
        ensure_local_input_exists(audio_url)?;
        if !crate::is_local_media_path(audio_url) {
            validate_tool_url_with_dns(audio_url).await?;
        }
        // STT upload precondition (2026-10-25 repair): local inputs above
        // the threshold get their speech track extracted first — the
        // extraction preserves the source timeline, so the bundle keeps
        // the ORIGINAL media as its path (the extracted file is a transport
        // detail, never the transcript's identity).
        let transcribe_from = self.stt_precondition_input(audio_url).await?;
        let upload_audio_url = transcribe_from
            .clone()
            .unwrap_or_else(|| audio_url.to_string());
        let media_params = hkask_types::MediaGenerateParams {
            audio_url: Some(upload_audio_url),
            language: language.clone(),
            // Per-call override wins; None keeps the router's configured
            // STT model (HKASK_MEDIA_STT_MODEL) — the override is what makes
            // the cross-check restart-free (2026-11-02).
            model: model.map(str::to_string),
            ..Default::default()
        };
        let generate = self
            .vision_port
            .media_generate("transcribe", &media_params)
            .await;
        if let Some(extracted) = &transcribe_from {
            // The extracted speech audio is a transport artifact — remove it
            // once the upload completes or fails (2026-10-26 review: it
            // previously accumulated in the ffmpeg temp dir for the process
            // lifetime, one multi-MB artifact per large transcription).
            if let Err(error) = std::fs::remove_file(extracted) {
                tracing::warn!(
                    target: "hkask.mcp.media.ffmpeg",
                    "failed to remove preconditioned STT artifact {extracted}: {error}"
                );
            }
        }
        let raw = generate.map_err(|e| classify_inference_error("Transcription failed", e))?;
        Ok(transcript_bundle_from_raw(
            &raw,
            audio_url.to_string(),
            language.clone(),
            0.0,
        ))
    }

    /// Precondition a local transcription input for provider upload:
    /// `Some(extracted_path)` when the input exceeded the upload threshold
    /// and its speech track was extracted, `None` when the input transcribes
    /// as-is (small local files and remote URLs). Fails closed with the
    /// named remediation when even the extracted speech audio exceeds the
    /// provider cap. Delegates to the shared [`precondition_local_stt_input`]
    /// — the async job path consumes the same helper.
    pub(crate) async fn stt_precondition_input(
        &self,
        audio_url: &str,
    ) -> Result<Option<String>, McpToolError> {
        let runner = self.ffmpeg.available.then_some(&self.ffmpeg);
        precondition_local_stt_input(runner, audio_url).await
    }

    #[tool(
        description = "Transcribe audio or video and return a synchronized TranscriptBundle with word-level timings (full_text carries the plain text). Enables interactive highlighting and click-to-seek in frontends, and is the ingest format for the educt transcript layers."
    )]
    pub async fn transcribe_bundle(
        &self,
        Parameters(TranscribeBundleRequest {
            audio_url,
            language,
            model,
            gallery_asset_id,
        }): Parameters<TranscribeBundleRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "transcribe_bundle", async {
            // R5 (operator ruling 2026-10-28): storing belongs to the
            // dedicated paths — this tool returns a bundle only, so the
            // storing parameter is refused with a pointer rather than
            // silently accepted and dropped.
            if let Some(asset_id) = gallery_asset_id {
                return Err(McpToolError::invalid_argument(format!(
                    "transcribe_bundle returns a TranscriptBundle only — pass \
                     gallery_asset_id to transcribe_and_store, or submit a transcribe \
                     job with it, to store the transcript against recording {asset_id}"
                )));
            }
            let _admission = self.admit_heavy_operation()?;
            let bundle = self
                .transcribe_to_bundle(&audio_url, &language, model.as_deref())
                .await?;

            serde_json::to_value(&bundle).map_err(|error| {
                McpToolError::internal(format!("encode transcript bundle: {error}"))
            })
        })
        .await
    }

    #[tool(
        description = "Transcribe audio or video AND store the TranscriptBundle server-side in one call, keyed to its recording when a gallery asset id is supplied, returning only the transcript summary (id, words_count, has_word_timings, gallery_asset_id when linked). For long media this is the path: an hour-long talk is ~550KB of bundle JSON, and relaying it through the model context (transcribe_bundle then educt_store_transcript) costs ~140K tokens each way. Accepts the same inputs as transcribe_bundle plus an optional gallery asset id; continue with the educt passes by transcript id."
    )]
    pub async fn transcribe_and_store(
        &self,
        Parameters(TranscribeAndStoreRequest {
            audio_url,
            language,
            model,
            gallery_asset_id,
        }): Parameters<TranscribeAndStoreRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "transcribe_and_store", async {
            let _admission = self.admit_heavy_operation()?;
            let bundle = self
                .transcribe_to_bundle(&audio_url, &language, model.as_deref())
                .await?;

            let driver = &**self.gallery_store.driver();
            crate::tools::educt::store_transcript_summary_value(
                driver,
                &bundle,
                gallery_asset_id.as_deref(),
            )
        })
        .await
    }

    /// Record from the default input and publish the recording as a durable
    /// WAV gallery asset. Shared by `audio_capture` and
    /// `record_and_transcribe`, so both leave the recording in the generated
    /// gallery rather than in the runner's scratch directory, which is
    /// removed when the server exits.
    async fn capture_and_publish(
        &self,
        op: &str,
        duration_secs: f32,
    ) -> Result<serde_json::Value, McpToolError> {
        if !(MIN_CAPTURE_DURATION_SECS..=hkask_types::media_limits::MAX_CAPTURE_DURATION_SECS)
            .contains(&duration_secs)
        {
            return Err(McpToolError::invalid_argument(format!(
                "duration_secs must be between {MIN_CAPTURE_DURATION_SECS} and {} (1 hour).",
                hkask_types::media_limits::MAX_CAPTURE_DURATION_SECS
            )));
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
            op,
            "captured",
            crate::assets::LocalMediaFormat::Wav,
            &effective_params,
        )
    }

    #[tool(
        description = "Capture audio from the default system microphone and publish a canonical durable WAV asset optimized for STT transcription (16 kHz mono)."
    )]
    pub async fn audio_capture(
        &self,
        Parameters(AudioCaptureRequest { duration_secs }): Parameters<AudioCaptureRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "audio_capture", async {
            let _admission = self.admit_heavy_operation()?;
            self.capture_and_publish("audio_capture", duration_secs)
                .await
        })
        .await
    }

    #[tool(
        description = "Record audio from the microphone, publish it as a durable WAV gallery asset, and transcribe it in one call. Returns the published recording (output, gallery_asset_id) with the synchronized TranscriptBundle under transcript. If transcription fails the recording is kept and the response carries status partial with transcript_error. The transcript is not stored: pass transcript to educt_store_transcript."
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
            let mut result = self
                .capture_and_publish("record_and_transcribe", duration_secs)
                .await?;
            let recording = result
                .get("output")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    McpToolError::internal("published recording carries no output path")
                })?
                .to_string();
            // The published file goes through the same path as every other
            // transcription, so a long recording gets the upload
            // preconditioning instead of one oversized inline payload.
            match self.transcribe_to_bundle(&recording, &language, None).await {
                Ok(bundle) => {
                    result["transcript"] = serde_json::to_value(&bundle).map_err(|error| {
                        McpToolError::internal(format!("encode transcript bundle: {error}"))
                    })?;
                }
                Err(error) => {
                    result["status"] = serde_json::json!("partial");
                    result["transcript_error"] =
                        serde_json::json!({"error": error.message, "kind": error.kind.to_string()});
                }
            }
            Ok(result)
        })
        .await
    }

    #[tool(
        description = "Trim an audio file, or the audio track of a video, to start/end seconds and publish a durable PCM WAV asset. A PCM source is copied bit-exact; any other codec is decoded to 16-bit PCM. A start at or past the end of the media is refused."
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
            crate::tools::jobs::validate_trim(start_sec, end_sec)?;
            ensure_local_input_exists(&audio_url)?;
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

    #[tool(
        description = "Concatenate audio files in order and publish one durable 16-bit PCM WAV asset. Every input is decoded and resampled to the first input's sample rate and channel layout, so mixed codecs and rates join correctly."
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
            for url in &audio_urls {
                ensure_local_input_exists(url)?;
            }
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

    /// dcterms:identifier: `ensure_within_stt_upload_cap`
    /// expect: Extracted speech at exactly the cap passes; above it the
    /// precondition fails closed naming the remediation (transcribe in
    /// shorter ranges) — the previously unasserted branch.
    #[test]
    fn stt_upload_cap_fails_closed_above_the_limit() {
        let boundary = MediaServer::STT_UPLOAD_LIMIT_BYTES;
        assert!(
            ensure_within_stt_upload_cap(40 * 1024 * 1024 * 1024, boundary).is_ok(),
            "exactly at the cap passes"
        );
        let refused =
            ensure_within_stt_upload_cap(1, boundary + 1).expect_err("above the cap fails closed");
        assert!(
            refused.message.contains("shorter ranges"),
            "the refusal names the remediation: {refused:?}"
        );
    }

    /// expect: A misspelled argument to an audio, transcription or search
    /// tool is refused, not dropped: `langauge` must not transcribe in the
    /// provider's default language as though I had asked for nothing.
    #[test]
    fn audio_and_search_tools_refuse_an_argument_they_do_not_declare() {
        use crate::test_support::{live_input_schema, wire};
        let misspelled = wire::<TranscribeAndStoreRequest>(
            serde_json::json!({"audio_url": "a.wav", "langauge": "ja"}),
        )
        .err()
        .expect("an undeclared key is refused");
        assert!(misspelled.to_string().contains("langauge"), "{misspelled}");
        assert!(
            wire::<TranscribeBundleRequest>(serde_json::json!({"audio_url": "a.wav", "zz": 1}))
                .is_err()
        );
        assert!(
            wire::<AudioTrimRequest>(
                serde_json::json!({"audio_url": "a.wav", "start_sec": 0, "end_sec": 1, "end": 2})
            )
            .is_err()
        );
        assert!(
            wire::<AudioConcatRequest>(serde_json::json!({"audio_urls": [], "urls": []})).is_err()
        );
        assert!(
            wire::<RecordAndTranscribeRequest>(
                serde_json::json!({"duration_secs": 1.0, "duration": 5})
            )
            .is_err()
        );
        assert!(
            wire::<AudioCaptureRequest>(serde_json::json!({"duration_secs": 1.0, "zz": 1}))
                .is_err()
        );
        assert!(
            wire::<crate::types::YoutubeSearchRequest>(
                serde_json::json!({"query": "zz", "limit": 5})
            )
            .is_err(),
            "a guessed `limit` must not silently fall back to the default page size"
        );
        for tool in [
            "transcribe_bundle",
            "transcribe_and_store",
            "audio_capture",
            "record_and_transcribe",
            "audio_trim",
            "audio_concat",
            "video_clip",
            "video_concat",
            "youtube_search",
        ] {
            assert_eq!(
                live_input_schema(tool)["additionalProperties"],
                serde_json::json!(false),
                "{tool} schema must be closed"
            );
        }
    }

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
