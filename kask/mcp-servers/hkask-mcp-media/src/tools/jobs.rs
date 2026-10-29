//! Research jobs — source-linked transcription and lossless trim/concat.
//!
//! File-producing jobs share staged publication and cancellation rollback;
//! transcription returns the source-linked TranscriptBundle without creating media.
use crate::types::{
    JobCancelRequest, JobListPayload, JobListRequest, JobRecord, JobStatusRequest, JobSubmitRequest,
};
use crate::*;

pub const JOB_HISTORY_SCOPE: &str = "ephemeral_process_local";
pub const JOB_RESTART_BEHAVIOR: &str = "Job history is ephemeral and process-local; records are lost when the media server restarts and older terminal records may be removed by bounded retention.";

const JOB_STATUSES: &[&str] = &[
    "queued",
    "running",
    "cancelling",
    "completed",
    "failed",
    "cancelled",
];

/// Decode and validate the one current `job_list` wire contract.
///
/// expect: A broken queue response must not look like an empty queue.
/// [P7] Motivating: server and panel share one strict response contract.
/// pre: output is the tool's serialized response.
/// post: every count, history field, and status is present and coherent or decoding fails.
pub fn parse_job_list_response(output: &str) -> Result<JobListPayload, JobListParseError> {
    let value: serde_json::Value =
        serde_json::from_str(output).map_err(JobListParseError::InvalidJson)?;
    let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
    if let Some(error) = hkask_types::tool_response::parse_tool_error_value(&payload) {
        return Err(JobListParseError::ToolError(error.message));
    }
    let payload: JobListPayload =
        serde_json::from_value(payload).map_err(JobListParseError::ShapeMismatch)?;
    if payload.limit == 0
        || payload.limit > hkask_types::media_limits::MAX_JOB_LIST_LIMIT
        || payload.jobs.len() > payload.limit
        || payload.total < payload.jobs.len()
        || payload.has_more != (payload.total > payload.jobs.len())
        || payload.history_scope != JOB_HISTORY_SCOPE
        || payload.restart_behavior != JOB_RESTART_BEHAVIOR
        || payload
            .jobs
            .iter()
            .any(|job| !JOB_STATUSES.contains(&job.status.as_str()))
    {
        return Err(JobListParseError::InvalidContract);
    }
    Ok(payload)
}

/// `job_list` wire-contract decode failures. Shared by the server tests and
/// the media panel — both consume the same response contract [P7].
#[derive(Debug, thiserror::Error)]
pub enum JobListParseError {
    #[error("invalid job_list response: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("{0}")]
    ToolError(String),
    #[error("invalid job_list response (expected the strict current object contract): {0}")]
    ShapeMismatch(#[source] serde_json::Error),
    #[error("invalid job_list response (incoherent count, scope, or status metadata)")]
    InvalidContract,
}

fn map_job_store_error(error: crate::jobs::JobStoreError) -> McpToolError {
    match error {
        crate::jobs::JobStoreError::Overloaded { .. } => {
            McpToolError::rate_limited(error.to_string())
        }
        crate::jobs::JobStoreError::NotFound(_) => {
            McpToolError::not_found(format!("{error}. {JOB_RESTART_BEHAVIOR}"))
        }
        crate::jobs::JobStoreError::AlreadyTerminal { .. } => {
            McpToolError::invalid_argument(error.to_string())
        }
        crate::jobs::JobStoreError::LockPoisoned { .. }
        | crate::jobs::JobStoreError::Duplicate(_)
        | crate::jobs::JobStoreError::AdmissionClosed
        | crate::jobs::JobStoreError::MissingCancellationControl(_)
        | crate::jobs::JobStoreError::CompletionSignalClosed(_)
        | crate::jobs::JobStoreError::CancellationCleanupFailed { .. } => {
            McpToolError::internal(error.to_string())
        }
    }
}

/// Finish a job lease as cancelled, warning (named by job id) when the
/// record could not be updated — the shared exit of the spawned task's
/// cancellation paths.
fn finish_cancelled(lease: crate::jobs::JobLease, job_id: &str) {
    if let Err(error) = lease.finish(crate::jobs::JobOutcome::Cancelled) {
        tracing::warn!(
            target: "hkask.mcp.media.jobs",
            job_id = %job_id,
            %error,
            "Failed to record cancelled media job"
        );
    }
}

/// A closed dispatch set: creative operations cannot enter the background queue.
enum ResearchJob {
    Transcribe(TranscribeBundleRequest),
    AudioTrim(AudioTrimRequest),
    AudioConcat(AudioConcatRequest),
    VideoClip(VideoClipRequest),
    VideoConcat(VideoConcatRequest),
}

const RESEARCH_OPS: &str = "transcribe, audio_trim, audio_concat, video_clip, video_concat";

impl ResearchJob {
    fn parse(op: &str, params: &str) -> Result<Self, McpToolError> {
        fn decode<T: serde::de::DeserializeOwned>(params: &str) -> Result<T, McpToolError> {
            serde_json::from_str(params).map_err(|error| {
                McpToolError::invalid_argument(format!(
                    "params must match the research operation's request: {error}"
                ))
            })
        }
        match op {
            "transcribe" => Ok(Self::Transcribe(decode(params)?)),
            "audio_trim" => Ok(Self::AudioTrim(decode(params)?)),
            "audio_concat" => Ok(Self::AudioConcat(decode(params)?)),
            "video_clip" => Ok(Self::VideoClip(decode(params)?)),
            "video_concat" => Ok(Self::VideoConcat(decode(params)?)),
            _ => Err(McpToolError::invalid_argument(format!(
                "unsupported research op '{op}' — job_submit accepts {RESEARCH_OPS}"
            ))),
        }
    }

    fn sources(&self) -> Vec<&str> {
        match self {
            Self::Transcribe(request) => vec![&request.audio_url],
            Self::AudioTrim(request) => vec![&request.audio_url],
            Self::VideoClip(request) => vec![&request.video_url],
            Self::AudioConcat(request) => request.audio_urls.iter().map(String::as_str).collect(),
            Self::VideoConcat(request) => request.video_urls.iter().map(String::as_str).collect(),
        }
    }

    async fn validate(&self) -> Result<(), McpToolError> {
        match self {
            Self::AudioTrim(request) => validate_trim(request.start_sec, request.end_sec)?,
            Self::VideoClip(request) => validate_trim(request.start_sec, request.end_sec)?,
            Self::AudioConcat(request) => validate_item_count(
                "audio_urls",
                request.audio_urls.len(),
                1,
                hkask_types::media_limits::MAX_CONCAT_ITEMS,
            )?,
            Self::VideoConcat(request) => validate_item_count(
                "video_urls",
                request.video_urls.len(),
                2,
                hkask_types::media_limits::MAX_CONCAT_ITEMS,
            )?,
            Self::Transcribe(_) => {}
        }
        for source in self.sources() {
            ensure_local_input_exists(source)?;
            if !is_local_media_path(source) {
                validate_tool_url_with_dns(source).await?;
            }
        }
        Ok(())
    }

    async fn run(
        self,
        port: &Arc<dyn InferencePort>,
        ffmpeg: Option<&FfmpegRunner>,
    ) -> Result<ResearchOutput, McpToolError> {
        use crate::assets::LocalMediaFormat;
        // The transcribe arm needs the raw runner (the STT precondition);
        // the closure below shadows the parameter for the editing arms.
        let ffmpeg_runner = ffmpeg;
        let ffmpeg = || {
            ffmpeg_runner
                .filter(|runner| runner.available)
                .ok_or_else(|| {
                    McpToolError::failed_precondition(
                        "ffmpeg is required for source-linked editing jobs",
                    )
                })
        };
        let (output, format, mut args) = match self {
            Self::Transcribe(request) => {
                // The STT upload precondition (2026-10-25 repair) covers the
                // async job path too — wired 2026-10-26 review; the bundle's
                // identity stays the ORIGINAL input.
                let transcribe_from =
                    super::audio::precondition_local_stt_input(ffmpeg_runner, &request.audio_url)
                        .await?;
                let params = hkask_types::MediaGenerateParams {
                    audio_url: Some(
                        transcribe_from
                            .clone()
                            .unwrap_or_else(|| request.audio_url.clone()),
                    ),
                    language: request.language.clone(),
                    ..Default::default()
                };
                let result = port.media_generate("transcribe", &params).await;
                if let Some(extracted) = &transcribe_from {
                    if let Err(error) = std::fs::remove_file(extracted) {
                        tracing::warn!(
                            target: "hkask.mcp.media.ffmpeg",
                            "failed to remove preconditioned STT artifact {extracted}: {error}"
                        );
                    }
                }
                let raw = result
                    .map_err(|error| classify_inference_error("Transcription failed", error))?;
                let bundle = super::audio::transcript_bundle_from_raw(
                    &raw,
                    request.audio_url,
                    request.language,
                    0.0,
                );
                return serde_json::to_value(bundle)
                    .map(ResearchOutput::Transcript)
                    .map_err(|error| {
                        McpToolError::internal(format!("encode job transcript: {error}"))
                    });
            }
            Self::AudioTrim(request) => (
                ffmpeg()?
                    .audio_trim(&request.audio_url, request.start_sec, request.end_sec)
                    .await,
                LocalMediaFormat::Wav,
                serde_json::json!({"source": request.audio_url, "start_sec": request.start_sec, "end_sec": request.end_sec, "duration_sec": request.end_sec - request.start_sec}),
            ),
            Self::AudioConcat(request) => (
                ffmpeg()?.audio_concat(&request.audio_urls).await,
                LocalMediaFormat::Wav,
                serde_json::json!({"sources": request.audio_urls}),
            ),
            Self::VideoClip(request) => (
                ffmpeg()?
                    .clip(&request.video_url, request.start_sec, request.end_sec)
                    .await,
                LocalMediaFormat::Mp4,
                serde_json::json!({"source": request.video_url, "start_sec": request.start_sec, "end_sec": request.end_sec, "duration_sec": request.end_sec - request.start_sec}),
            ),
            Self::VideoConcat(request) => (
                ffmpeg()?.concat(&request.video_urls).await,
                LocalMediaFormat::Mp4,
                serde_json::json!({"sources": request.video_urls}),
            ),
        };
        let output = output.map_err(map_media_error)?;
        // A clip's output kind follows its input (audio decodes to wav) —
        // the publication format must match the real output, not the op's
        // default container (2026-10-27).
        let format = if format == LocalMediaFormat::Mp4
            && output.extension().and_then(|ext| ext.to_str()) == Some("wav")
        {
            LocalMediaFormat::Wav
        } else {
            format
        };
        args["format"] = serde_json::Value::String(format.extension().to_string());
        let publication = crate::assets::stage_local_media_publication(&output, format)
            .map_err(map_media_error)?;
        Ok(ResearchOutput::Media {
            publication,
            args,
            kind: format.block_kind(),
        })
    }
}

fn validate_trim(start_sec: f32, end_sec: f32) -> Result<(), McpToolError> {
    if !start_sec.is_finite() || !end_sec.is_finite() || start_sec < 0.0 || end_sec <= start_sec {
        return Err(McpToolError::invalid_argument(
            "start_sec must be finite and >= 0; end_sec must be finite and > start_sec",
        ));
    }
    Ok(())
}

enum ResearchOutput {
    Transcript(serde_json::Value),
    Media {
        publication: crate::assets::StagedJobPublication,
        args: serde_json::Value,
        kind: &'static str,
    },
}

#[tool_router(router = jobs_router, vis = "pub")]
impl MediaServer {
    /// Submit an async research media job. Returns a job ID immediately;
    /// poll `job_status` for completion. The job runs in the background.
    #[tool(
        description = "Submit a research job: transcribe, audio_trim, audio_concat, video_clip, or video_concat. params is a JSON-serialized request for that operation: transcribe takes audio_url and optional language; trim/clip takes audio_url/video_url, start_sec, end_sec; concat takes audio_urls/video_urls. Transcription returns a source-linked TranscriptBundle; edits publish durable source-linked media. No creative operations. Returns job_id; poll job_status for completion."
    )]
    pub async fn job_submit(
        &self,
        Parameters(JobSubmitRequest { op, params }): Parameters<JobSubmitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_submit", async {
            let operation = ResearchJob::parse(&op, &params)?;
            operation.validate().await?;
            if !matches!(operation, ResearchJob::Transcribe(_)) {
                self.require_ffmpeg()?;
            }
            let job_id = uuid::Uuid::new_v4().to_string();
            let now = hkask_types::time::now_rfc3339();

            // Admission owns a slot before the queued record becomes visible,
            // so overload cannot grow an unbounded waiting queue.
            let lease = self
                .job_store
                .admit(JobRecord {
                    id: job_id.clone(),
                    op: op.clone(),
                    status: "queued".to_string(),
                    created_at: now,
                    completed_at: None,
                    result: None,
                    error: None,
                })
                .map_err(map_job_store_error)?;

            // Spawn the admitted research operation.
            let vision_port = self.vision_port.clone();
            let job_store = self.job_store.clone();
            let gallery_store = self.gallery_store.clone();
            let job_id_for_task = job_id.clone();
            let op_for_task = op.clone();
            // Each worker owns its scratch directory. Cloning the server runner
            // would share a directory whose Drop cleanup can delete sibling work.
            let ffmpeg = if matches!(operation, ResearchJob::Transcribe(_)) {
                None
            } else {
                Some(FfmpegRunner::detect())
            };

            tokio::spawn(async move {
                let mut lease = lease;
                let token = lease.cancellation_token();
                let running = match job_store.mark_running(&job_id_for_task) {
                    Ok(running) => running,
                    Err(error) => {
                        tracing::warn!(
                            target: "hkask.mcp.media.jobs",
                            job_id = %job_id_for_task,
                            error = %error,
                            "Failed to mark admitted media job running"
                        );
                        return;
                    }
                };
                if !running {
                    finish_cancelled(lease, &job_id_for_task);
                    return;
                }

                let result = tokio::select! {
                    biased;
                    () = token.cancelled() => None,
                    result = operation.run(&vision_port, ffmpeg.as_ref()) => Some(result),
                };
                let value = match result {
                    None => {
                        finish_cancelled(lease, &job_id_for_task);
                        return;
                    }
                    Some(Err(error)) => {
                        if let Err(finish_error) =
                            lease.finish(crate::jobs::JobOutcome::Failed(error.to_string()))
                        {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, error = %finish_error, "Failed to record research operation failure");
                        }
                        return;
                    }
                    Some(Ok(value)) => value,
                };

                let (mut publication, effective_params, kind_for_task) = match value {
                    ResearchOutput::Transcript(bundle) => {
                        if let Err(error) = lease.finish(crate::jobs::JobOutcome::Completed(bundle)) {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, %error, "Failed to record transcript completion");
                        }
                        return;
                    }
                    ResearchOutput::Media { publication, args, kind } => (publication, args, kind),
                };

                let slim = match publication.publish_and_slim(
                    &gallery_store,
                    &op_for_task,
                    &effective_params,
                ) {
                    Ok(slim) => slim,
                    Err(error) => {
                        let outcome = if token.is_cancelled() {
                            crate::jobs::JobOutcome::CancellationFailed(error.to_string())
                        } else {
                            crate::jobs::JobOutcome::Failed(format!(
                                "asset not persisted: {error}"
                            ))
                        };
                        if let Err(finish_error) = lease.finish(outcome) {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, error = %finish_error, "Failed to record publication failure");
                        }
                        return;
                    }
                };

                let completed = crate::media_block::enrich_with_omc_and_provenance(
                    slim,
                    &op_for_task,
                    kind_for_task,
                    effective_params,
                    None,
                );
                match lease.finish_published(completed, &mut publication) {
                    Ok(true) => {}
                    Ok(false) => {
                        let outcome = match publication.rollback() {
                            Ok(()) => crate::jobs::JobOutcome::Cancelled,
                            Err(error) => {
                                crate::jobs::JobOutcome::CancellationFailed(error.to_string())
                            }
                        };
                        if let Err(error) = lease.finish(outcome) {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, %error, "Failed to record cancellation after publication");
                        }
                    }
                    Err(error) => {
                        if let Err(rollback_error) = publication.rollback() {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, error = %rollback_error, "Failed to roll back publication after terminalization error");
                        }
                        tracing::warn!(
                            target: "hkask.mcp.media.jobs",
                            job_id = %job_id_for_task,
                            %error,
                            "Failed to record media job completion"
                        );
                    }
                }
            });

            Ok(serde_json::json!({
                "job_id": job_id,
                "status": "queued",
                "op": op,
            }))
        })
        .await
    }

    /// List research media jobs with their status. Optionally filter by status.
    #[tool(
        description = "List process-local research media jobs with their status. History is ephemeral and lost when the media server restarts. Optionally filter by status (queued, running, cancelling, completed, failed, cancelled)."
    )]
    pub async fn job_list(
        &self,
        Parameters(JobListRequest { status, limit }): Parameters<JobListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_list", async {
            let limit = limit.unwrap_or(hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT);
            validate_item_count(
                "limit",
                limit,
                1,
                hkask_types::media_limits::MAX_JOB_LIST_LIMIT,
            )?;
            let mut matching_jobs = self
                .job_store
                .list()
                .map_err(map_job_store_error)?
                .into_iter()
                .filter(|job| status.as_ref().is_none_or(|s| job.status == *s))
                .collect::<Vec<_>>();

            // Sort by created_at descending (newest first).
            matching_jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let total = matching_jobs.len();
            let jobs = matching_jobs.into_iter().take(limit).collect::<Vec<_>>();

            serde_json::to_value(JobListPayload {
                jobs,
                total,
                limit,
                has_more: total > limit,
                history_scope: JOB_HISTORY_SCOPE.to_string(),
                restart_behavior: JOB_RESTART_BEHAVIOR.to_string(),
            })
            .map_err(|error| McpToolError::internal(format!("encode job_list: {error}")))
        })
        .await
    }

    /// Get the status of a specific research media job by its ID.
    #[tool(
        description = "Get a process-local research media job by ID. History is ephemeral and lost when the media server restarts."
    )]
    pub async fn job_status(
        &self,
        Parameters(JobStatusRequest { job_id }): Parameters<JobStatusRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_status", async {
            let job = self
                .job_store
                .get(&job_id)
                .map_err(map_job_store_error)?
                .ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "Job not found: {job_id}. {JOB_RESTART_BEHAVIOR} Persistent lineage remains available through gallery_lineage; call job_list to see known jobs."
                    ))
                })?;
            let mut value = serde_json::to_value(&job)
                .map_err(|e| McpToolError::internal(format!("encode job status: {e}")))?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| McpToolError::internal("encoded job status was not an object"))?;
            object.insert(
                "history_scope".to_string(),
                serde_json::Value::String(JOB_HISTORY_SCOPE.to_string()),
            );
            object.insert(
                "restart_behavior".to_string(),
                serde_json::Value::String(JOB_RESTART_BEHAVIOR.to_string()),
            );
            Ok(value)
        })
        .await
    }

    /// Cancel a running or queued research media job.
    #[tool(
        description = "Cancel a process-local running or queued research media job and wait for teardown. History is ephemeral and lost when the media server restarts."
    )]
    pub async fn job_cancel(
        &self,
        Parameters(JobCancelRequest { job_id }): Parameters<JobCancelRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_cancel", async {
            self.job_store
                .cancel(&job_id)
                .await
                .map_err(map_job_store_error)?;
            Ok(serde_json::json!({
                "job_id": job_id,
                "status": "cancelled",
                "history_scope": JOB_HISTORY_SCOPE,
                "restart_behavior": JOB_RESTART_BEHAVIOR,
            }))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::MAX_CONCURRENT_HEAVY_OPERATIONS;

    struct TestStt {
        barrier: Option<Arc<tokio::sync::Barrier>>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
        fail: bool,
    }
    crate::test_support::media_inference_stub! {
        TestStt,
        generate(self, _prompt): { panic!("transcription must use media_generate") },
        media_generate(self, op, params): {
            assert_eq!(op, "transcribe", "creative dispatch must never reach inference");
            assert_eq!(params.language.as_deref(), Some("en"));
            let barrier = self.barrier.clone();
            let dropped = self.dropped.clone();
            let fail = self.fail;
            Box::pin(async move {
                struct DropSignal(Arc<std::sync::atomic::AtomicBool>);
                impl Drop for DropSignal {
                    fn drop(&mut self) { self.0.store(true, std::sync::atomic::Ordering::SeqCst); }
                }
                let _drop_signal = DropSignal(dropped);
                if let Some(barrier) = barrier {
                    barrier.wait().await;
                    return std::future::pending().await;
                }
                if fail {
                    return Err(hkask_types::InferenceError::Connection("provider sentinel".into()));
                }
                Ok(serde_json::json!({
                    "text": "source evidence", "duration": 1.0, "model": "test-stt",
                    "words": [{"word": " source", "start": 0.0, "end": 0.5}, {"word": " evidence", "start": 0.5, "end": 1.0}]
                }))
            })
        }
    }

    fn server(port: TestStt) -> Result<MediaServer, Box<dyn std::error::Error>> {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        Ok(MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(port),
            Arc::new(Mutex::new(None)),
            Arc::new(GalleryStore::from_driver(driver)?),
            crate::templates::create_env()?,
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            Default::default(),
        ))
    }

    fn stt() -> TestStt {
        TestStt {
            barrier: None,
            dropped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fail: false,
        }
    }

    fn payload(response: &str) -> serde_json::Value {
        hkask_types::tool_response::unwrap_tool_envelope(
            serde_json::from_str(response).expect("tool response JSON"),
        )
    }

    async fn submit(
        server: &MediaServer,
        source: &std::path::Path,
    ) -> Result<String, McpToolError> {
        let response = server
            .job_submit(Parameters(JobSubmitRequest {
                op: "transcribe".into(),
                params: serde_json::json!({"audio_url": source, "language": "en"}).to_string(),
            }))
            .await?;
        Ok(payload(&response)["job_id"]
            .as_str()
            .expect("submitted job ID")
            .into())
    }

    async fn wait_until(predicate: impl Fn() -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !predicate() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("job reaches expected state");
    }

    /// expect: Creative operations cannot reach inference through jobs, while
    /// legitimate research operations keep their typed request contracts.
    #[test]
    fn dispatcher_is_closed_to_creative_operations() {
        for op in [
            "generate_image",
            "image_to_image",
            "upscale",
            "remove_background",
            "generate_video",
            "image_to_video",
            "generate_speech",
            "voice_design",
            "workflow_execute",
            "expand_prompt",
        ] {
            let error = ResearchJob::parse(op, "{}")
                .err()
                .expect("creative job rejected");
            assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
            assert!(error.message.contains(RESEARCH_OPS));
        }
        for (op, params) in [
            ("transcribe", r#"{"audio_url":"recording.wav"}"#),
            (
                "audio_trim",
                r#"{"audio_url":"recording.wav","start_sec":0,"end_sec":1}"#,
            ),
            ("audio_concat", r#"{"audio_urls":["recording.wav"]}"#),
            (
                "video_clip",
                r#"{"video_url":"recording.mp4","start_sec":0,"end_sec":1}"#,
            ),
            ("video_concat", r#"{"video_urls":["recording.mp4"]}"#),
        ] {
            assert!(ResearchJob::parse(op, params).is_ok(), "{op}");
            assert!(ResearchJob::parse(op, "{}").is_err(), "{op} needs sources");
        }
        assert!(validate_trim(f32::NAN, 1.0).is_err());
        assert!(validate_trim(0.0, f32::INFINITY).is_err());
        assert!(validate_trim(1.0, 1.0).is_err());
        validate_trim(0.0, 1.0).expect("valid trim accepted");
    }

    /// expect: Running the admitted dispatcher yields source-linked timed words,
    /// never a generated media asset, and keeps the panel response parser usable.
    #[tokio::test]
    async fn transcription_job_completes_with_source_and_timings()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = server(stt())?;
        let input = tempfile::NamedTempFile::new()?;
        let id = submit(&server, input.path()).await?;
        wait_until(|| server.job_store.active_count().expect("active count") == 0).await;
        let job = server.job_store.get(&id)?.expect("completed job");
        assert_eq!(job.status, "completed");
        let bundle = job.result.expect("transcript result");
        assert_eq!(
            bundle["audio_path"],
            input.path().to_string_lossy().as_ref()
        );
        assert_eq!(bundle["words"][0]["word"], "source");
        assert_eq!(bundle["words"][1]["start_ms"], 500);
        assert_eq!(bundle["model"], "test-stt");
        let page = parse_job_list_response(
            &server
                .job_list(Parameters(JobListRequest {
                    status: None,
                    limit: Some(1),
                }))
                .await?,
        )?;
        assert_eq!(page.jobs.len(), 1);
        assert_eq!(page.total, 1);
        assert!(!page.has_more);
        assert_eq!(page.history_scope, JOB_HISTORY_SCOPE);
        assert_eq!(page.jobs[0].id, id);
        Ok(())
    }

    /// expect: Cancellation waits for transcription teardown and releases the
    /// shared heavy-operation slot; control reads still work under saturation.
    #[tokio::test]
    async fn cancellation_and_shared_admission_remain_operational()
    -> Result<(), Box<dyn std::error::Error>> {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server = server(TestStt {
            barrier: Some(barrier.clone()),
            dropped: dropped.clone(),
            fail: false,
        })?;
        let input = tempfile::NamedTempFile::new()?;
        let id = submit(&server, input.path()).await?;
        barrier.wait().await;
        let permits = (1..MAX_CONCURRENT_HEAVY_OPERATIONS)
            .map(|_| server.job_store.admit_direct())
            .collect::<Result<Vec<_>, _>>()?;
        let error = submit(&server, input.path())
            .await
            .expect_err("shared capacity exhausted");
        assert_eq!(error.kind, hkask_types::McpErrorKind::RateLimited);
        assert_eq!(
            parse_job_list_response(
                &server
                    .job_list(Parameters(JobListRequest {
                        status: None,
                        limit: None
                    }))
                    .await?
            )?
            .jobs
            .len(),
            1
        );
        server
            .job_cancel(Parameters(JobCancelRequest { job_id: id.clone() }))
            .await?;
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(server.job_store.active_count()?, 0);
        assert_eq!(
            server.job_store.get(&id)?.expect("cancelled job").status,
            "cancelled"
        );
        drop(permits);
        assert!(server.job_store.admit_direct().is_ok());
        Ok(())
    }

    /// expect: Source edits retain their effective sources and format through
    /// durable publication, while cancellation rolls back files and gallery rows.
    #[cfg(unix)]
    #[tokio::test]
    async fn research_edits_publish_source_lineage_and_rollback_on_cancellation()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let root = tempfile::tempdir()?;
        struct EnvGuard(Option<std::ffi::OsString>);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
                    None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
                }
            }
        }
        let _env = EnvGuard(std::env::var_os("HKASK_ARTIFACTS_DIR"));
        unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", root.path()) };
        let binary = root.path().join("fake-ffmpeg");
        std::fs::write(
            &binary,
            "#!/bin/sh\nfor arg do output=$arg; done\nprintf processed-source > \"$output\"\n",
        )?;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))?;
        let runner = FfmpegRunner::with_binary(
            binary.to_string_lossy().into_owned(),
            root.path().join("scratch"),
        );
        let server = server(stt())?;
        let input = root.path().join("source.wav");
        std::fs::write(&input, b"immutable source")?;
        for (op, params) in [
            (
                "audio_trim",
                serde_json::json!({"audio_url": input, "start_sec": 0, "end_sec": 1}),
            ),
            ("audio_concat", serde_json::json!({"audio_urls": [input]})),
            (
                "video_clip",
                serde_json::json!({"video_url": input, "start_sec": 0, "end_sec": 1}),
            ),
            (
                "video_concat",
                serde_json::json!({"video_urls": [input, input]}),
            ),
        ] {
            let operation = ResearchJob::parse(op, &params.to_string())?;
            operation.validate().await?;
            let ResearchOutput::Media {
                mut publication,
                args,
                kind,
            } = operation.run(&server.vision_port, Some(&runner)).await?
            else {
                panic!("source editing must stage a media publication");
            };
            // A clip's published kind follows its INPUT (2026-10-27): the
            // shared wav fixture exercises exactly that — audio_trim and
            // audio_concat stay audio/wav, video_concat hardcodes its mp4
            // container, and video_clip on the wav input now correctly
            // publishes as audio/wav (the pre-fix mp4/video label was the
            // mislabel the kind-aware clip corrects).
            let expect_kind = match op {
                "audio_trim" | "audio_concat" | "video_clip" => "audio",
                _ => "video",
            };
            let expect_format = match op {
                "audio_trim" | "audio_concat" | "video_clip" => "wav",
                _ => "mp4",
            };
            assert_eq!(kind, expect_kind);
            assert_eq!(args["format"], expect_format);
            if op.ends_with("concat") {
                assert_eq!(args["sources"][0], input.to_string_lossy().as_ref());
            } else {
                assert_eq!(args["source"], input.to_string_lossy().as_ref());
            }
            let slim = publication.publish_and_slim(&server.gallery_store, op, &args)?;
            let asset_id = slim["gallery_asset_id"]
                .as_str()
                .expect("published asset ID")
                .to_string();
            let lineage = server
                .gallery_store
                .get_generation(&asset_id)?
                .expect("source lineage");
            assert_eq!(lineage.op, op);
            let effective: serde_json::Value =
                serde_json::from_str(lineage.params.as_deref().expect("effective source params"))?;
            assert_eq!(effective, args);
            let output = std::path::PathBuf::from(slim["output"].as_str().expect("durable output"));
            assert!(output.is_file());
            // A cancelling job cannot commit an asset already published at
            // the staging boundary. Explicit rollback removes row and file.
            let job_id = uuid::Uuid::new_v4().to_string();
            let mut lease = server.job_store.admit(JobRecord {
                id: job_id.clone(),
                op: op.into(),
                status: "cancelling".into(),
                created_at: hkask_types::time::now_rfc3339(),
                completed_at: None,
                result: None,
                error: None,
            })?;
            assert!(!lease.finish_published(slim, &mut publication)?);
            publication.rollback()?;
            lease.finish(crate::jobs::JobOutcome::Cancelled)?;
            assert!(!output.exists());
            assert!(server.gallery_store.get_generation(&asset_id)?.is_none());
            assert_eq!(std::fs::read(&input)?, b"immutable source");
        }
        Ok(())
    }

    /// expect: Caller errors are classified before enqueue; provider failures
    /// remain visible in the failed job instead of becoming successful emptiness.
    #[tokio::test]
    async fn research_job_errors_are_visible() -> Result<(), Box<dyn std::error::Error>> {
        let mut port = stt();
        port.fail = true;
        let server = server(port)?;
        let input = tempfile::NamedTempFile::new()?;
        let singleton = ResearchJob::VideoConcat(VideoConcatRequest {
            video_urls: vec![input.path().to_string_lossy().into_owned()],
        });
        let error = singleton
            .validate()
            .await
            .expect_err("concat requires two sources before enqueue");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        let pair = ResearchJob::VideoConcat(VideoConcatRequest {
            video_urls: vec![input.path().to_string_lossy().into_owned(); 2],
        });
        pair.validate()
            .await
            .expect("two existing sources are valid");
        let missing = input.path().with_extension("missing");
        let error = submit(&server, &missing).await.expect_err("missing source");
        assert_eq!(error.kind, hkask_types::McpErrorKind::NotFound);
        assert!(server.job_store.list()?.is_empty());
        let id = submit(&server, input.path()).await?;
        wait_until(|| server.job_store.active_count().expect("active count") == 0).await;
        let job = server.job_store.get(&id)?.expect("failed job");
        assert_eq!(job.status, "failed");
        assert!(
            job.error
                .expect("provider error")
                .contains("provider sentinel")
        );
        for limit in [0, hkask_types::media_limits::MAX_JOB_LIST_LIMIT + 1] {
            assert_eq!(
                server
                    .job_list(Parameters(JobListRequest {
                        status: None,
                        limit: Some(limit)
                    }))
                    .await
                    .expect_err("invalid page limit")
                    .kind,
                hkask_types::McpErrorKind::InvalidArgument
            );
        }
        for response in [
            r#"{"content":[]}"#,
            r#"{"content":{"jobs":[],"total":0,"has_more":false}}"#,
        ] {
            assert!(parse_job_list_response(response).is_err());
        }
        let error = server
            .job_status(Parameters(JobStatusRequest {
                job_id: "lost-after-restart".into(),
            }))
            .await
            .expect_err("unknown job");
        assert_eq!(error.kind, hkask_types::McpErrorKind::NotFound);
        assert!(error.message.contains(JOB_RESTART_BEHAVIOR));
        Ok(())
    }

    async fn no_inference_job_list() -> Result<(MediaServer, String), Box<dyn std::error::Error>> {
        let server = server(stt())?;
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: None,
            }))
            .await?;
        Ok((server, response))
    }

    /// expect: The existing panel queue contract still reconciles ordering,
    /// pagination, result details, and process-local history metadata.
    #[tokio::test]
    async fn job_list_response_round_trips_through_client_decoder()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, response) = no_inference_job_list().await?;
        let empty = parse_job_list_response(&response)?;
        assert!(empty.jobs.is_empty());
        assert_eq!(empty.total, 0);
        assert_eq!(
            empty.limit,
            hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT
        );
        assert!(!empty.has_more);
        assert_eq!(empty.history_scope, JOB_HISTORY_SCOPE);
        for (id, status, created_at) in [
            ("older", "completed", "2026-09-04T00:00:00Z"),
            ("newer", "running", "2026-09-05T00:00:00Z"),
        ] {
            server.job_store.insert_record_for_test(JobRecord {
                id: id.into(),
                op: "generate_image".into(),
                status: status.into(),
                created_at: created_at.into(),
                completed_at: None,
                result: Some(serde_json::json!({"output": "/tmp/雪.png"})),
                error: None,
            })?;
        }
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: None,
            }))
            .await?;
        let page = parse_job_list_response(&response)?;
        assert_eq!(page.jobs.len(), 2);
        assert_eq!(page.jobs[0].id, "newer");
        assert_eq!(page.jobs[1].id, "older");
        assert_eq!(page.total, 2);
        assert_eq!(
            page.limit,
            hkask_types::media_limits::DEFAULT_JOB_LIST_LIMIT
        );
        assert!(!page.has_more);
        assert_eq!(page.history_scope, JOB_HISTORY_SCOPE);
        assert_eq!(page.restart_behavior, JOB_RESTART_BEHAVIOR);
        assert_eq!(
            page.jobs[0].result,
            Some(serde_json::json!({"output": "/tmp/雪.png"}))
        );
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: Some(1),
            }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        assert_eq!(payload["total"], 2);
        assert_eq!(payload["has_more"], true);
        let page = parse_job_list_response(&response)?;
        assert_eq!(page.jobs.len(), 1);
        assert_eq!(page.jobs[0].id, "newer");
        assert_eq!(page.total, 2);
        assert_eq!(page.limit, 1);
        assert!(page.has_more);

        let response = server
            .job_status(Parameters(JobStatusRequest {
                job_id: "newer".to_string(),
            }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        assert_eq!(payload["history_scope"], JOB_HISTORY_SCOPE);
        Ok(())
    }

    /// expect: Legacy or incomplete queue responses fail instead of looking empty.
    /// [P7] Motivating: producer and panel consume one strict current contract.
    /// pre: the response is an array or omits required scope/count fields.
    /// post: decoding reports a shape mismatch.
    #[test]
    fn job_list_decoder_rejects_legacy_and_incomplete_shapes() {
        for response in [
            r#"{"content":[]}"#,
            r#"{"content":{"jobs":[],"total":0,"has_more":false}}"#,
            r#"{"content":{"jobs":[],"total":0,"limit":20,"has_more":false,"history_scope":"wrong","restart_behavior":"unknown"}}"#,
        ] {
            assert!(
                parse_job_list_response(response).is_err(),
                "malformed current contract decoded: {response}"
            );
        }
    }

    /// expect: Invalid job page sizes are rejected rather than clamped or treated as an empty
    /// queue.
    #[tokio::test]
    async fn job_list_rejects_zero_and_over_cap_limits() -> Result<(), Box<dyn std::error::Error>> {
        let server = server(stt())?;
        for limit in [0, hkask_types::media_limits::MAX_JOB_LIST_LIMIT + 1] {
            let error = server
                .job_list(Parameters(JobListRequest {
                    status: None,
                    limit: Some(limit),
                }))
                .await
                .expect_err("invalid job limit must fail");
            assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        }
        Ok(())
    }

    /// expect: List, status, and cancel disclose process-local restart loss and bounded retention.
    #[tokio::test]
    async fn all_job_history_surfaces_disclose_ephemeral_restart_scope()
    -> Result<(), Box<dyn std::error::Error>> {
        let (server, response) = no_inference_job_list().await?;
        let value: serde_json::Value = serde_json::from_str(&response)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        assert_eq!(payload["history_scope"], "ephemeral_process_local");
        assert!(
            payload["restart_behavior"]
                .as_str()
                .is_some_and(|message| message.contains("lost"))
        );
        assert_eq!(payload["jobs"], serde_json::json!([]));

        for error in [
            server
                .job_status(Parameters(JobStatusRequest {
                    job_id: "lost-after-restart".to_string(),
                }))
                .await
                .expect_err("unknown in-memory job status must be not found"),
            server
                .job_cancel(Parameters(JobCancelRequest {
                    job_id: "lost-after-restart".to_string(),
                }))
                .await
                .expect_err("unknown in-memory job cancellation must be not found"),
        ] {
            assert_eq!(error.kind, hkask_types::McpErrorKind::NotFound);
            assert!(error.message.contains("ephemeral"));
            assert!(error.message.contains("restart"));
            assert!(error.message.contains("retention"));
        }
        Ok(())
    }

    /// expect: A fresh in-memory controller starts with no records.
    #[test]
    fn job_store_starts_empty() -> Result<(), Box<dyn std::error::Error>> {
        let store = crate::jobs::new_job_store();
        assert!(store.list()?.is_empty());
        Ok(())
    }

    /// expect: Tests can seed records without exposing the controller's mutable map.
    #[test]
    fn job_store_controlled_test_insert_and_cloned_get() -> Result<(), Box<dyn std::error::Error>> {
        let store = crate::jobs::new_job_store();
        let record = JobRecord {
            id: "test-job-1".to_string(),
            op: "generate_image".to_string(),
            status: "queued".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            completed_at: None,
            result: None,
            error: None,
        };
        store.insert_record_for_test(record)?;
        assert_eq!(store.list()?.len(), 1);
        let job = store
            .get("test-job-1")?
            .ok_or_else(|| "inserted record missing".to_string())?;
        assert_eq!(job.op, "generate_image");
        Ok(())
    }
}
