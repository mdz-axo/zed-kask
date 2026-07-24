//! Audio tools — voice design, speech generation, transcription, audio capture.
use crate::*;

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
    ) -> String {
        execute_tool(self, "voice_design", async {
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

            let params = hkask_types::template::LLMParameters::default();
            let r = self
                .inference
                .generate_with_model(
                    &prompt,
                    &params,
                    Some("DI/meta-llama/Llama-3.3-70B-Instruct"),
                    None,
                )
                .await
                .map_err(|e| {
                    McpToolError::unavailable(format!("Voice design inference failed: {}", e))
                })?;

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
        description = "Generate speech audio from text using a voice design. Returns audio as base64 data URI."
    )]
    pub async fn generate_speech(
        &self,
        Parameters(GenerateSpeechRequest { text, voice_design }): Parameters<GenerateSpeechRequest>,
    ) -> String {
        execute_tool(self, "generate_speech", async {
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

            self.inference
                .generate_speech(&text, &voice)
                .await
                .map_err(|e| McpToolError::unavailable(format!("Speech generation failed: {}", e)))
        })
        .await
    }

    // ── Audio tools ─────────────────────────────────────────────────────────

    #[tool(
        description = "Transcribe speech audio to text. Returns transcribed text for REPL injection."
    )]
    pub async fn transcribe(
        &self,
        Parameters(TranscribeRequest {
            audio_url,
            language,
        }): Parameters<TranscribeRequest>,
    ) -> String {
        execute_tool(self, "transcribe", async {
            validate_tool_url(&audio_url)?;

            self.inference
                .transcribe(&audio_url, language.as_deref())
                .await
                .map_err(|e| McpToolError::unavailable(format!("Transcription failed: {}", e)))
        })
        .await
    }

    #[tool(
        description = "Transcribe audio and return a synchronized TranscriptBundle with word-level timings. Enables interactive highlighting and click-to-seek in frontends."
    )]
    pub async fn transcribe_bundle(
        &self,
        Parameters(TranscribeRequest {
            audio_url,
            language,
        }): Parameters<TranscribeRequest>,
    ) -> String {
        execute_tool(self, "transcribe_bundle", async {
            validate_tool_url(&audio_url)?;

            let raw = self
                .inference
                .transcribe(&audio_url, language.as_deref())
                .await
                .map_err(|e| McpToolError::unavailable(format!("Transcription failed: {}", e)))?;

            let full_text = raw
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let duration = raw.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0) as f32;
            let model = raw
                .get("model")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string());
            let words: Vec<TimedWord> = raw
                .get("words")
                .and_then(|w| w.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|w| {
                            Some(TimedWord {
                                word: w.get("word")?.as_str()?.to_string(),
                                start_ms: (w.get("start")?.as_f64()? * 1000.0) as u64,
                                end_ms: (w.get("end")?.as_f64()? * 1000.0) as u64,
                                confidence: w
                                    .get("confidence")
                                    .and_then(|c| c.as_f64())
                                    .map(hkask_types::Confidence::new),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let segments: Vec<TranscriptSegment> = raw
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
                .unwrap_or_default();

            let bundle = TranscriptBundle {
                format: "hkask-transcript-v1".to_string(),
                audio_path: audio_url.clone(),
                repl_chat_ref: None,
                audio_duration_secs: duration,
                full_text,
                words,
                segments,
                language: language.clone(),
                model,
            };

            Ok(serde_json::to_value(&bundle)
                .unwrap_or_else(|_| serde_json::json!({"error": "Failed to serialize bundle"})))
        })
        .await
    }

    #[tool(
        description = "Capture audio from the default system microphone. Records to a WAV file optimized for Whisper transcription (16kHz mono)."
    )]
    pub async fn audio_capture(
        &self,
        Parameters(AudioCaptureRequest {
            duration_secs,
            output_path,
        }): Parameters<AudioCaptureRequest>,
    ) -> String {
        execute_tool(self, "audio_capture", async {
            if duration_secs <= 0.0 || duration_secs > 3600.0 {
                return Err(McpToolError::invalid_argument(
                    "duration_secs must be between 0.1 and 3600 (1 hour).",
                ));
            }

            self.require_ffmpeg()?;

            let path = self
                .ffmpeg
                .capture_audio(duration_secs, output_path.as_deref())
                .await
                .map_err(map_media_error)?;

            Ok(serde_json::json!({
                "status": "captured",
                "duration_secs": duration_secs,
                "output": path.display().to_string(),
                "format": "wav",
                "sample_rate": 16000,
                "channels": 1,
            }))
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
    ) -> String {
        execute_tool(self, "record_and_transcribe", async {
            if duration_secs <= 0.0 || duration_secs > 3600.0 {
                return Err(McpToolError::invalid_argument(
                    "duration_secs must be between 0.1 and 3600 (1 hour).",
                ));
            }

            self.require_ffmpeg()?;

            let audio_path = self
                .ffmpeg
                .capture_audio(duration_secs, None)
                .await
                .map_err(|e| McpToolError::internal(format!("Audio capture failed: {}", e)))?;

            let audio_data = std::fs::read(&audio_path).map_err(|e| {
                McpToolError::internal(format!("Failed to read captured audio: {}", e))
            })?;
            let b64 =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &audio_data);
            let audio_uri = format!("data:audio/wav;base64,{}", b64);

            let transcribe_result = self
                .inference
                .transcribe(&audio_uri, language.as_deref())
                .await
                .map_err(|e| McpToolError::unavailable(format!("Transcription failed: {}", e)));

            match transcribe_result {
                Ok(raw) => {
                    let full_text = raw
                        .get("text")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    let duration = raw
                        .get("duration")
                        .and_then(|d| d.as_f64())
                        .unwrap_or(duration_secs as f64) as f32;
                    let model = raw
                        .get("model")
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string());
                    let words: Vec<TimedWord> = raw
                        .get("words")
                        .and_then(|w| w.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|w| {
                                    Some(TimedWord {
                                        word: w.get("word")?.as_str()?.to_string(),
                                        start_ms: (w.get("start")?.as_f64()? * 1000.0) as u64,
                                        end_ms: (w.get("end")?.as_f64()? * 1000.0) as u64,
                                        confidence: w.get("confidence").and_then(|c| c.as_f64()).map(hkask_types::Confidence::new),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let segments: Vec<TranscriptSegment> = raw
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
                        .unwrap_or_default();

                    let audio_path_str = audio_path.display().to_string();
                    let bundle = TranscriptBundle {
                        format: "hkask-transcript-v1".to_string(),
                        audio_path: audio_path_str.clone(),
                        repl_chat_ref: Some("repl_chat_hook".to_string()),
                        audio_duration_secs: duration,
                        full_text,
                        words,
                        segments,
                        language: language.clone(),
                        model,
                    };

                    Ok(serde_json::to_value(&bundle).unwrap_or_else(|_| {
                        serde_json::json!({"error": "Failed to serialize bundle"})
                    }))
                }
                Err(e) => Ok(serde_json::json!({
                    "status": "partial",
                    "duration_secs": duration_secs,
                    "audio_path": audio_path.display().to_string(),
                    "audio_format": "wav",
                    "sample_rate": 16000,
                    "channels": 1,
                    "transcript_error": e.to_json_string(),
                    "message": "Audio captured successfully but transcription failed. The audio file is saved and can be transcribed later."
                })),
            }
        })
        .await
    }
}
