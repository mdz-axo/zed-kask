//! Authorized voice-profile enrollment and lifecycle tools.

use crate::transcript_layers::TranscriptLayer;
use crate::voice_profiles::{
    EncoderFingerprint, EnrollmentAudioEmbedding, NewVoiceEnrollment, OperatorAuthorization,
    VoiceProfile, VoiceProfileError, VoiceProfileRemoval, VoiceProfileStore,
};
use crate::*;

const MAX_ENROLLMENT_SPAN_MS: u64 = 60_000;

#[derive(Debug, serde::Serialize)]
struct VoiceSegmentView {
    id: String,
    transcript_id: String,
    start_word: usize,
    end_word: usize,
    media_path: String,
    gallery_asset_id: Option<String>,
    start_ms: u64,
    end_ms: u64,
}

#[derive(Debug, serde::Serialize)]
struct VoiceProfileView {
    id: String,
    speaker_name: String,
    authorization: OperatorAuthorization,
    fingerprint: EncoderFingerprint,
    segments: Vec<VoiceSegmentView>,
    created_at: String,
    last_confirmed_match_at: Option<String>,
}

impl From<&VoiceProfile> for VoiceProfileView {
    fn from(profile: &VoiceProfile) -> Self {
        Self {
            id: profile.id.clone(),
            speaker_name: profile.speaker_name.clone(),
            authorization: profile.authorization.clone(),
            fingerprint: profile.fingerprint.clone(),
            segments: profile
                .segments
                .iter()
                .map(|segment| VoiceSegmentView {
                    id: segment.id.clone(),
                    transcript_id: segment.transcript_id.clone(),
                    start_word: segment.start_word,
                    end_word: segment.end_word,
                    media_path: segment.media_path.clone(),
                    gallery_asset_id: segment.gallery_asset_id.clone(),
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                })
                .collect(),
            created_at: profile.created_at.clone(),
            last_confirmed_match_at: profile.last_confirmed_match_at.clone(),
        }
    }
}

fn map_voice_error(error: VoiceProfileError) -> McpToolError {
    match error {
        VoiceProfileError::Unavailable(message) => McpToolError::failed_precondition(message),
        VoiceProfileError::MissingCredential(message) => McpToolError::permission_denied(message),
        VoiceProfileError::ProfileNotFound(message) => McpToolError::not_found(message),
        VoiceProfileError::ModelMismatch { profile_id, .. } => McpToolError::invalid_argument(
            format!("voice profile {profile_id} has an incompatible encoder fingerprint"),
        ),
        other => McpToolError::internal(other.to_string()),
    }
}

fn remove_profile_and_derived(
    driver: &dyn hkask_storage::database::driver::DatabaseDriver,
    store: &VoiceProfileStore,
    profile_id: &str,
) -> Result<(VoiceProfileRemoval, usize), McpToolError> {
    crate::transcript_store::ensure_schema(driver)
        .map_err(|error| McpToolError::internal(error.to_string()))?;
    // Derived layers are deleted first: if either database write fails, the
    // profile remains as a durable retry handle instead of stranding layers.
    let derived_layers_removed = driver
        .execute(
            "DELETE FROM transcript_layers
             WHERE layer_kind = 'identification'
               AND json_extract(payload_json, '$.profile_id') = ?1",
            &[hkask_storage::database::value::DbValue::Text(
                profile_id.to_string(),
            )],
        )
        .map_err(|error| McpToolError::internal(error.to_string()))?;
    let removal = store.remove_profile(profile_id).map_err(map_voice_error)?;
    Ok((removal, derived_layers_removed))
}

fn single_speaker_evidence(
    driver: &dyn hkask_storage::database::driver::DatabaseDriver,
    transcript_id: &str,
    start_word: usize,
    end_word: usize,
) -> Result<String, McpToolError> {
    let layers = crate::transcript_store::list_layers(driver, transcript_id)
        .map_err(|error| McpToolError::internal(error.to_string()))?;
    let Some(speaker_layer) = layers.iter().rev().find_map(|record| match &record.layer {
        TranscriptLayer::Speaker(layer) => Some(layer),
        _ => None,
    }) else {
        return Ok("operator_confirmed_exact_range".to_string());
    };
    let mut speakers = Vec::new();
    for index in start_word..=end_word {
        let span = speaker_layer
            .spans
            .iter()
            .find(|span| span.start_word <= index && index <= span.end_word)
            .ok_or_else(|| {
                McpToolError::failed_precondition(format!(
                    "word {index} has no speaker attribution in the latest speaker layer"
                ))
            })?;
        if !speakers.contains(&span.speaker) {
            speakers.push(span.speaker.clone());
        }
    }
    if speakers.len() != 1 {
        return Err(McpToolError::failed_precondition(format!(
            "enrollment requires exactly one attributed speaker; found {}",
            speakers.len()
        )));
    }
    Ok(format!("latest_speaker_layer:{}", speakers[0]))
}

#[tool_router(router = voice_router, vis = "pub")]
impl MediaServer {
    #[tool(
        description = "Enroll one explicitly authorized speaker name over an inclusive immutable source-word range spanning 1–60 seconds. The exact audio span is decoded to 16 kHz mono before fbank and embedded locally with the pinned CAM++ model. Full operator-authorization and model provenance is stored in a separate SQLCipher DB; no speaker annotation is authored and no identity match is made. Returns metadata only, never voiceprint bytes."
    )]
    pub async fn voice_enroll(
        &self,
        Parameters(VoiceEnrollRequest {
            transcript_id,
            start_word,
            end_word,
            speaker_name,
        }): Parameters<VoiceEnrollRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "voice_enroll", async {
            let _admission = self.admit_heavy_operation()?;
            let store = self.voice.profile_store().map_err(map_voice_error)?;
            if speaker_name.trim().is_empty() {
                return Err(McpToolError::invalid_argument(
                    "speaker_name must not be blank",
                ));
            }
            let driver = &**self.gallery_store.driver();
            let Some((summary, bundle)) =
                crate::transcript_store::load_transcript(driver, &transcript_id)
                    .map_err(|error| McpToolError::internal(error.to_string()))?
            else {
                return Err(McpToolError::not_found(format!(
                    "transcript {transcript_id} not found"
                )));
            };
            if bundle.words.is_empty() {
                return Err(McpToolError::failed_precondition(
                    "transcript has no word timings; enrollment cannot anchor an audio span",
                ));
            }
            if start_word > end_word || end_word >= bundle.words.len() {
                return Err(McpToolError::invalid_argument(format!(
                    "word range {start_word}..={end_word} is outside 0..={}",
                    bundle.words.len() - 1
                )));
            }
            let start_ms = bundle.words[start_word].start_ms;
            let end_ms = bundle.words[end_word].end_ms;
            if end_ms <= start_ms {
                return Err(McpToolError::failed_precondition(
                    "enrollment audio span must have positive duration",
                ));
            }
            if end_ms - start_ms < 1_000 {
                return Err(McpToolError::failed_precondition(
                    "enrollment audio span is under 1000 ms; select a longer single-speaker range",
                ));
            }
            if end_ms - start_ms > MAX_ENROLLMENT_SPAN_MS {
                return Err(McpToolError::failed_precondition(format!(
                    "enrollment audio span exceeds the {MAX_ENROLLMENT_SPAN_MS} ms resource bound; select a shorter single-speaker range"
                )));
            }
            let evidence = single_speaker_evidence(driver, &transcript_id, start_word, end_word)?;
            if !crate::is_local_media_path(&bundle.audio_path) {
                validate_tool_url_with_dns(&bundle.audio_path).await?;
            }
            let encoder = self
                .voice
                .encoder()
                .await
                .map_err(|error| McpToolError::unavailable(error.to_string()))?;
            let embedded = encoder
                .embed_span(
                    &self.ffmpeg,
                    std::path::Path::new(&bundle.audio_path),
                    start_ms,
                    end_ms,
                )
                .await
                .map_err(|error| McpToolError::unavailable(error.to_string()))?;
            let profile = store
                .enroll(NewVoiceEnrollment {
                    speaker_name: speaker_name.trim().to_string(),
                    authorization: OperatorAuthorization {
                        transcript_id: transcript_id.clone(),
                        start_word,
                        end_word,
                        recorded_at: hkask_types::time::now_rfc3339(),
                    },
                    fingerprint: embedded.fingerprint,
                    segments: vec![EnrollmentAudioEmbedding {
                        transcript_id,
                        start_word,
                        end_word,
                        media_path: bundle.audio_path,
                        gallery_asset_id: summary.gallery_asset_id,
                        start_ms,
                        end_ms,
                        embedding: embedded.embedding,
                    }],
                })
                .map_err(map_voice_error)?;
            Ok(serde_json::json!({
                "profile": VoiceProfileView::from(&profile),
                "speaker_evidence": evidence,
                "authorization_kind": "operator_authorization",
                "authenticated_data_subject_consent": false,
            }))
        })
        .await
    }

    #[tool(
        description = "List authorized speaker voice profiles with exact-range, media, and model provenance. Responses contain segment metadata only — never centroid or per-segment voiceprint bytes."
    )]
    pub async fn voice_list(
        &self,
        Parameters(VoiceListRequest {}): Parameters<VoiceListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "voice_list", async {
            let store = self.voice.profile_store().map_err(map_voice_error)?;
            let profiles = store.list_profiles().map_err(map_voice_error)?;
            let views: Vec<VoiceProfileView> =
                profiles.iter().map(VoiceProfileView::from).collect();
            Ok(serde_json::json!({
                "count": views.len(),
                "profiles": views,
                "voiceprint_bytes": "not included",
            }))
        })
        .await
    }

    #[tool(
        description = "Delete one authorized voice profile and all its enrolled embeddings, plus any derived identification layers carrying that profile id. Transcript source words, human annotations, exports, and rendered media are preserved."
    )]
    pub async fn voice_remove(
        &self,
        Parameters(VoiceRemoveRequest { profile_id }): Parameters<VoiceRemoveRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "voice_remove", async {
            let store = self.voice.profile_store().map_err(map_voice_error)?;
            let (removal, derived_layers_removed) =
                remove_profile_and_derived(&**self.gallery_store.driver(), &store, &profile_id)?;
            Ok(serde_json::json!({
                "profile_id": profile_id,
                "profiles_removed": removal.profiles_removed,
                "embeddings_removed": removal.embeddings_removed,
                "derived_layers_removed": derived_layers_removed,
            }))
        })
        .await
    }

    #[tool(
        description = "Prune voice profiles older than max_age_days, sparing profiles with a human-confirmed match inside the same retention window. Each candidate goes through the voice_remove cleanup path for embeddings and derived identification layers. No pruning runs implicitly."
    )]
    pub async fn voice_prune(
        &self,
        Parameters(VoicePruneRequest { max_age_days }): Parameters<VoicePruneRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "voice_prune", async {
            let _admission = self.admit_heavy_operation()?;
            let cutoff =
                (chrono::Utc::now() - chrono::Duration::days(i64::from(max_age_days))).to_rfc3339();
            let store = self.voice.profile_store().map_err(map_voice_error)?;
            let candidates = store.stale_profile_ids(&cutoff).map_err(map_voice_error)?;
            let mut profiles_removed = 0;
            let mut embeddings_removed = 0;
            let mut derived_layers_removed = 0;
            for profile_id in &candidates {
                let (removal, derived) =
                    remove_profile_and_derived(&**self.gallery_store.driver(), &store, profile_id)?;
                profiles_removed += removal.profiles_removed;
                embeddings_removed += removal.embeddings_removed;
                derived_layers_removed += derived;
            }
            Ok(serde_json::json!({
                "max_age_days": max_age_days,
                "candidates": candidates.len(),
                "profiles_removed": profiles_removed,
                "embeddings_removed": embeddings_removed,
                "derived_layers_removed": derived_layers_removed,
            }))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_profiles::{
        EncoderFingerprint, EnrollmentAudioEmbedding, NewVoiceEnrollment, OperatorAuthorization,
        VoiceProfileRuntime,
    };

    struct NoopPort;
    crate::test_support::media_inference_stub! {
        NoopPort,
        generate(self, _prompt): {
            panic!("voice-profile tools do not call the LLM inference port")
        }
    }

    struct ArtifactsEnv(Option<String>);

    impl ArtifactsEnv {
        fn new(path: &std::path::Path) -> Self {
            let prior = std::env::var("HKASK_ARTIFACTS_DIR").ok();
            unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", path) };
            Self(prior)
        }
    }

    impl Drop for ArtifactsEnv {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
                None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
            }
        }
    }

    fn content_of(result: &str) -> serde_json::Value {
        let envelope: serde_json::Value = serde_json::from_str(result).expect("tool JSON");
        envelope["content"].clone()
    }

    fn enrollment() -> NewVoiceEnrollment {
        NewVoiceEnrollment {
            speaker_name: "Test Speaker".to_string(),
            authorization: OperatorAuthorization {
                transcript_id: "transcript-1".to_string(),
                start_word: 0,
                end_word: 2,
                recorded_at: "2026-10-28T23:00:00Z".to_string(),
            },
            fingerprint: EncoderFingerprint {
                model_family: "test-encoder".to_string(),
                weights_sha256: "test-weights".to_string(),
                preprocessing: "test-preprocessing".to_string(),
                dimension: 2,
            },
            segments: vec![EnrollmentAudioEmbedding {
                transcript_id: "transcript-1".to_string(),
                start_word: 0,
                end_word: 2,
                media_path: "/tmp/test.wav".to_string(),
                gallery_asset_id: None,
                start_ms: 0,
                end_ms: 1_500,
                embedding: vec![0.25, 0.75],
            }],
        }
    }

    fn server(store: VoiceProfileStore) -> MediaServer {
        server_with_runtime(VoiceProfileRuntime::with_store(store))
    }

    fn server_with_runtime(voice: VoiceProfileRuntime) -> MediaServer {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let gallery_store = Arc::new(GalleryStore::from_driver(driver).expect("gallery store"));
        MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopPort),
            Arc::new(Mutex::new(None)),
            gallery_store,
            templates::create_env().expect("templates"),
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            voice,
        )
    }

    async fn store_test_transcript(
        server: &MediaServer,
        words: Vec<crate::transcript::TimedWord>,
    ) -> String {
        let full_text = words
            .iter()
            .map(|word| word.word.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let duration_secs = words.last().map_or(0, |word| word.end_ms) as f32 / 1_000.0;
        let bundle = crate::transcript::TranscriptBundle {
            words,
            ..crate::transcript::TranscriptBundle::new(
                "/tmp/test.wav".to_string(),
                duration_secs,
                full_text,
            )
        };
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store transcript");
        content_of(&stored)["id"]
            .as_str()
            .expect("transcript id")
            .to_string()
    }

    fn seed_identification_layer(
        server: &MediaServer,
        transcript_id: &str,
        layer_id: &str,
        profile_id: &str,
    ) {
        server
            .gallery_store
            .driver()
            .execute(
                "INSERT INTO transcript_layers
                 (id, transcript_id, layer_kind, payload_json, created_at)
                 VALUES (?1, ?2, 'identification', ?3, '2026-10-28T23:00:00Z')",
                &[
                    hkask_storage::database::value::DbValue::Text(layer_id.to_string()),
                    hkask_storage::database::value::DbValue::Text(transcript_id.to_string()),
                    hkask_storage::database::value::DbValue::Text(
                        serde_json::json!({
                            "kind": "identification",
                            "profile_id": profile_id,
                            "speaker": "Test Speaker"
                        })
                        .to_string(),
                    ),
                ],
            )
            .expect("seed derived identification layer");
    }

    fn identification_layer_count(server: &MediaServer) -> usize {
        let rows = server
            .gallery_store
            .driver()
            .query(
                "SELECT COUNT(*) FROM transcript_layers WHERE layer_kind = 'identification'",
                &[],
            )
            .expect("count derived identification layers");
        rows[0].get_int(0).expect("layer count") as usize
    }

    #[tokio::test]
    #[ignore = "requires the pinned 29 MiB CAM++ model in the media data directory"]
    async fn voice_enroll_persists_real_embedding_with_authorization_and_no_side_effects() {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let fixture = tempfile::tempdir().expect("voice enrollment fixture");
        let _artifacts = ArtifactsEnv::new(&fixture.path().join("artifacts"));
        let source = fixture.path().join("8khz-stereo.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2:sample_rate=8000",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&source)
            .status()
            .await
            .expect("generate enrollment media");
        assert!(status.success());
        let store = VoiceProfileStore::open(
            &fixture.path().join("voice.db").to_string_lossy(),
            "test-passphrase",
        )
        .expect("open voice store");
        let server = server(store);
        let bundle = crate::transcript::TranscriptBundle {
            words: ["alpha", "beta", "gamma"]
                .iter()
                .enumerate()
                .map(|(index, word)| crate::transcript::TimedWord {
                    word: (*word).to_string(),
                    start_ms: index as u64 * 500,
                    end_ms: index as u64 * 500 + 500,
                    confidence: None,
                })
                .collect(),
            ..crate::transcript::TranscriptBundle::new(
                source.to_string_lossy().into_owned(),
                2.0,
                "alpha beta gamma".to_string(),
            )
        };
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store transcript");
        let transcript_id = content_of(&stored)["id"]
            .as_str()
            .expect("transcript id")
            .to_string();

        let enrolled = server
            .voice_enroll(Parameters(VoiceEnrollRequest {
                transcript_id: transcript_id.clone(),
                start_word: 0,
                end_word: 2,
                speaker_name: "Test Speaker".to_string(),
            }))
            .await
            .expect("enroll exact range");
        assert!(!enrolled.contains("centroid"));
        assert!(!enrolled.contains("embedding"));
        let content = content_of(&enrolled);
        assert_eq!(
            content["profile"]["fingerprint"]["dimension"],
            serde_json::json!(512)
        );
        assert_eq!(
            content["profile"]["authorization"]["end_word"],
            serde_json::json!(2)
        );
        let profile_id = content["profile"]["id"]
            .as_str()
            .expect("profile id")
            .to_string();
        let reopened = VoiceProfileStore::open(
            &fixture.path().join("voice.db").to_string_lossy(),
            "test-passphrase",
        )
        .expect("reopen voice store");
        let profiles = reopened.list_profiles().expect("load enrolled profile");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, profile_id);
        assert_eq!(profiles[0].segments[0].embedding.len(), 512);
        assert_eq!(profiles[0].segments[0].start_ms, 0);
        assert_eq!(profiles[0].segments[0].end_ms, 1_500);
        assert!(
            crate::transcript_store::list_layers(&**server.gallery_store.driver(), &transcript_id)
                .expect("list transcript layers")
                .is_empty(),
            "enrollment must not author a speaker annotation as a side effect"
        );
        for format in ["srt", "highlights_csv", "corpus_text"] {
            let exported = server
                .educt_export(Parameters(EductExportRequest {
                    transcript_id: transcript_id.clone(),
                    format: format.to_string(),
                }))
                .await
                .expect("export transcript");
            let document_path = content_of(&exported)["output"]
                .as_str()
                .expect("export document path")
                .to_string();
            let document = std::fs::read_to_string(document_path).expect("read export");
            assert!(
                !document.contains(&profile_id),
                "{format} leaked profile id"
            );
            assert!(
                !document
                    .contains("c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef"),
                "{format} leaked model fingerprint"
            );
            assert!(!document.contains("centroid"), "{format} leaked centroid");
            assert!(!document.contains("embedding"), "{format} leaked embedding");
        }
    }

    #[tokio::test]
    async fn voice_remove_retries_after_derived_layer_delete_failure() {
        let fixture = tempfile::tempdir().expect("voice DB fixture");
        let path = fixture.path().join("voice.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open voice store");
        let profile = store.enroll(enrollment()).expect("seed profile");
        let profile_id = profile.id.clone();
        let server = server(store);
        let transcript_id = store_test_transcript(
            &server,
            vec![crate::transcript::TimedWord {
                word: "alpha".to_string(),
                start_ms: 0,
                end_ms: 500,
                confidence: None,
            }],
        )
        .await;
        seed_identification_layer(&server, &transcript_id, "derived-1", &profile_id);
        server
            .gallery_store
            .driver()
            .execute_batch(
                "CREATE TRIGGER reject_identification_delete
                 BEFORE DELETE ON transcript_layers
                 WHEN OLD.layer_kind = 'identification'
                 BEGIN
                     SELECT RAISE(FAIL, 'injected gallery delete failure');
                 END;",
            )
            .expect("install gallery failure injection");

        server
            .voice_remove(Parameters(VoiceRemoveRequest {
                profile_id: profile_id.clone(),
            }))
            .await
            .expect_err("injected gallery failure must abort removal");
        assert_eq!(
            server
                .voice
                .profile_store()
                .expect("voice store")
                .list_profiles()
                .expect("profiles after failed removal")
                .len(),
            1,
            "the profile must remain as the retry handle"
        );
        assert_eq!(identification_layer_count(&server), 1);

        server
            .gallery_store
            .driver()
            .execute_batch("DROP TRIGGER reject_identification_delete")
            .expect("clear gallery failure injection");
        let removed = server
            .voice_remove(Parameters(VoiceRemoveRequest { profile_id }))
            .await
            .expect("retry removal");
        assert_eq!(
            content_of(&removed)["derived_layers_removed"],
            serde_json::json!(1)
        );
        assert_eq!(identification_layer_count(&server), 0);
    }

    #[tokio::test]
    async fn voice_prune_uses_shared_cleanup_and_spares_recent_confirmation() {
        let fixture = tempfile::tempdir().expect("voice DB fixture");
        let path = fixture.path().join("voice.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open voice store");
        let stale = store.enroll(enrollment()).expect("seed stale profile");
        let recent = store.enroll(enrollment()).expect("seed recent profile");
        store
            .record_confirmed_match(&recent.id, "2999-01-01T00:00:00Z")
            .expect("record recent confirmation");
        let server = server(store);
        let transcript_id = store_test_transcript(
            &server,
            vec![crate::transcript::TimedWord {
                word: "alpha".to_string(),
                start_ms: 0,
                end_ms: 500,
                confidence: None,
            }],
        )
        .await;
        seed_identification_layer(&server, &transcript_id, "derived-stale", &stale.id);
        seed_identification_layer(&server, &transcript_id, "derived-recent", &recent.id);

        let pruned = server
            .voice_prune(Parameters(VoicePruneRequest { max_age_days: 0 }))
            .await
            .expect("prune stale profiles");
        let content = content_of(&pruned);
        assert_eq!(content["candidates"], serde_json::json!(1));
        assert_eq!(content["profiles_removed"], serde_json::json!(1));
        assert_eq!(content["embeddings_removed"], serde_json::json!(1));
        assert_eq!(content["derived_layers_removed"], serde_json::json!(1));
        let profiles = server
            .voice
            .profile_store()
            .expect("voice store")
            .list_profiles()
            .expect("profiles after pruning");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, recent.id);
        assert_eq!(
            identification_layer_count(&server),
            1,
            "the recently confirmed profile's derived layer must remain"
        );
    }

    #[tokio::test]
    async fn voice_enroll_rejects_an_excessive_span_before_decoding() {
        let fixture = tempfile::tempdir().expect("voice DB fixture");
        let path = fixture.path().join("voice.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open voice store");
        let server = server(store);
        let transcript_id = store_test_transcript(
            &server,
            vec![
                crate::transcript::TimedWord {
                    word: "alpha".to_string(),
                    start_ms: 0,
                    end_ms: 1_000,
                    confidence: None,
                },
                crate::transcript::TimedWord {
                    word: "beta".to_string(),
                    start_ms: 1_000,
                    end_ms: 61_000,
                    confidence: None,
                },
            ],
        )
        .await;
        let error = server
            .voice_enroll(Parameters(VoiceEnrollRequest {
                transcript_id,
                start_word: 0,
                end_word: 1,
                speaker_name: "Test Speaker".to_string(),
            }))
            .await
            .expect_err("an excessive decode span must be rejected");
        assert_eq!(error.kind, hkask_types::McpErrorKind::FailedPrecondition);
        assert!(error.message.contains("60000 ms resource bound"));
    }

    #[tokio::test]
    async fn voice_list_classifies_a_missing_passphrase_as_permission_denied() {
        let server = server_with_runtime(VoiceProfileRuntime::missing_credential(
            "HKASK_DB_PASSPHRASE not configured".to_string(),
        ));
        let error = server
            .voice_list(Parameters(VoiceListRequest {}))
            .await
            .expect_err("missing protected-storage credential must be visible");
        assert_eq!(error.kind, hkask_types::McpErrorKind::PermissionDenied);
        assert!(error.message.contains("HKASK_DB_PASSPHRASE"));
    }

    #[tokio::test]
    async fn voice_list_and_remove_expose_provenance_without_voiceprint_bytes() {
        let fixture = tempfile::tempdir().expect("voice DB fixture");
        let path = fixture.path().join("voice.db");
        let store = VoiceProfileStore::open(&path.to_string_lossy(), "test-passphrase")
            .expect("open voice store");
        let profile = store.enroll(enrollment()).expect("seed profile");
        let server = server(store);

        let listed = server
            .voice_list(Parameters(VoiceListRequest {}))
            .await
            .expect("list profiles");
        let listed_json = content_of(&listed);
        assert_eq!(listed_json["count"], serde_json::json!(1));
        assert!(!listed.contains("centroid"));
        assert!(!listed.contains("embedding"));
        assert_eq!(
            listed_json["profiles"][0]["authorization"]["start_word"],
            serde_json::json!(0)
        );

        let removed = server
            .voice_remove(Parameters(VoiceRemoveRequest {
                profile_id: profile.id,
            }))
            .await
            .expect("remove profile");
        assert_eq!(
            content_of(&removed)["embeddings_removed"],
            serde_json::json!(1)
        );
        let listed = server
            .voice_list(Parameters(VoiceListRequest {}))
            .await
            .expect("list after removal");
        assert_eq!(content_of(&listed)["count"], serde_json::json!(0));
    }
}
