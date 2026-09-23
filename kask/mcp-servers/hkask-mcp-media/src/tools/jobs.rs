//! Generation job queue tools — submit, list, status, cancel.
//!
//! Fills the OMC `Task` concept with real-time job tracking. `job_submit`
//! spawns a background tokio task that calls `vision_port.media_generate`,
//! returning a job ID immediately. `job_list` / `job_status` / `job_cancel`
//! read from the in-memory job store.
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

#[tool_router(router = jobs_router, vis = "pub")]
impl MediaServer {
    /// Submit an async media generation job. Returns a job ID immediately;
    /// poll `job_status` for completion. The job runs in the background.
    #[tool(
        description = "Submit an async media generation job. Returns a job ID immediately; poll job_status for completion. The job runs in the background."
    )]
    pub async fn job_submit(
        &self,
        Parameters(JobSubmitRequest { op, params }): Parameters<JobSubmitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_submit", async {
            if op.trim().is_empty() {
                return Err(McpToolError::invalid_argument("op must not be empty"));
            }
            // Only asset-producing generation ops are accepted: the job's
            // result is persisted through the authoritative staged publication aggregate, and a
            // non-generation op has no asset to persist (the raw provider
            // response is never stored — base64 payloads overflow the
            // model's context).
            let Some(kind) = media_op_kind(&op) else {
                return Err(McpToolError::invalid_argument(format!(
                    "unsupported generation op '{op}' — job_submit accepts \
                     generate_image, image_to_image, upscale, remove_background, \
                     generate_video, image_to_video, generate_speech"
                )));
            };
            // Parse the params JSON into MediaGenerateParams.
            let media_params: hkask_types::MediaGenerateParams = serde_json::from_str(&params)
                .map_err(|e| {
                    McpToolError::invalid_argument(format!(
                        "params must be valid JSON MediaGenerateParams: {e}"
                    ))
                })?;

            let effective_params = serde_json::to_value(&media_params).map_err(|error| {
                McpToolError::internal(format!("encode job effective parameters: {error}"))
            })?;
            let job_id = uuid::Uuid::new_v4().to_string();
            let now = hkask_types::time::now_rfc3339();

            // Admission-time gallery capture: a submitted job is in-flight
            // work — it is indexed into the gallery active at submission,
            // never one activated while the job runs.
            let gallery = self.capture_gallery();

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

            // Spawn the background generation task.
            let vision_port = self.vision_port.clone();
            let job_store = self.job_store.clone();
            let gallery_store = self.gallery_store.clone();
            let job_id_for_task = job_id.clone();
            let op_for_task = op.clone();
            let kind_for_task = kind;

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
                    if let Err(error) = lease.finish(crate::jobs::JobOutcome::Cancelled) {
                        tracing::warn!(
                            target: "hkask.mcp.media.jobs",
                            job_id = %job_id_for_task,
                            error = %error,
                            "Failed to record cancelled media job"
                        );
                    }
                    return;
                }

                let generated = tokio::select! {
                    biased;
                    () = token.cancelled() => None,
                    result = vision_port.media_generate(&op_for_task, &media_params) => Some(result),
                };
                let value = match generated {
                    None => {
                        if let Err(error) = lease.finish(crate::jobs::JobOutcome::Cancelled) {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, %error, "Failed to record cancelled media job");
                        }
                        return;
                    }
                    Some(Err(error)) => {
                        if let Err(finish_error) =
                            lease.finish(crate::jobs::JobOutcome::Failed(error.to_string()))
                        {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, error = %finish_error, "Failed to record provider failure");
                        }
                        return;
                    }
                    Some(Ok(value)) => value,
                };

                let staged = tokio::select! {
                    biased;
                    () = token.cancelled() => None,
                    result = crate::assets::stage_job_publication(&value, kind_for_task) => Some(result),
                };
                let mut publication = match staged {
                    None => {
                        if let Err(error) = lease.finish(crate::jobs::JobOutcome::Cancelled) {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, %error, "Failed to record cancelled media job");
                        }
                        return;
                    }
                    Some(Err(error)) => {
                        if let Err(finish_error) = lease.finish(crate::jobs::JobOutcome::Failed(
                            format!("asset not staged: {error}"),
                        )) {
                            tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, error = %finish_error, "Failed to record staging failure");
                        }
                        return;
                    }
                    Some(Ok(publication)) => publication,
                };

                #[cfg(test)]
                {
                    let checkpoint = job_store.publication_checkpoint_for_test();
                    let checkpoint_result = tokio::select! {
                        biased;
                        () = token.cancelled() => None,
                        result = checkpoint => Some(result),
                    };
                    match checkpoint_result {
                        None => {
                            let outcome = match publication.rollback() {
                                Ok(()) => crate::jobs::JobOutcome::Cancelled,
                                Err(error) => crate::jobs::JobOutcome::CancellationFailed(error.to_string()),
                            };
                            if let Err(error) = lease.finish(outcome) {
                                tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, %error, "Failed to record cancellation during staging");
                            }
                            return;
                        }
                        Some(Err(error)) => {
                            let rollback_error = publication.rollback().err();
                            let detail = match rollback_error {
                                Some(rollback_error) => format!(
                                    "publication checkpoint: {error}; {rollback_error}"
                                ),
                                None => format!("publication checkpoint: {error}"),
                            };
                            if let Err(finish_error) =
                                lease.finish(crate::jobs::JobOutcome::Failed(detail))
                            {
                                tracing::warn!(target: "hkask.mcp.media.jobs", job_id = %job_id_for_task, error = %finish_error, "Failed to record publication checkpoint failure");
                            }
                            return;
                        }
                        Some(Ok(())) => {}
                    }
                }

                let slim = match publication.publish_and_slim(
                    gallery.as_ref(),
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

    /// List generation jobs with their status. Optionally filter by status.
    #[tool(
        description = "List process-local generation jobs with their status. History is ephemeral and lost when the media server restarts. Optionally filter by status (queued, running, cancelling, completed, failed, cancelled)."
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

    /// Get the status of a specific generation job by its ID.
    #[tool(
        description = "Get a process-local generation job by ID. History is ephemeral and lost when the media server restarts."
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
                        "Job not found: {job_id}. {JOB_RESTART_BEHAVIOR} Persistent lineage survives in gallery_record_generation; call job_list to see known jobs."
                    ))
                })?;
            let mut value = serde_json::to_value(&job)
                .map_err(|e| McpToolError::internal(format!("encode job status: {e}")))?; // rr0044-ok: serde serialization of own data
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

    /// Cancel a running or queued generation job.
    #[tool(
        description = "Cancel a process-local running or queued generation job and wait for teardown. History is ephemeral and lost when the media server restarts."
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

    struct NoInference;

    impl hkask_types::InferencePort for NoInference {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            panic!("control and overloaded tool tests must not invoke inference")
        }

        fn media_generate<'a>(
            &'a self,
            _: &str,
            _: &hkask_types::MediaGenerateParams,
        ) -> hkask_types::MediaFuture<'a> {
            panic!("an overloaded direct operation must fail before provider work")
        }
    }

    struct BlockingMedia {
        barrier: Arc<tokio::sync::Barrier>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }

    impl hkask_types::InferencePort for BlockingMedia {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            panic!("media job tests must use media_generate")
        }

        fn media_generate<'a>(
            &'a self,
            _: &str,
            _: &hkask_types::MediaGenerateParams,
        ) -> hkask_types::MediaFuture<'a> {
            let barrier = self.barrier.clone();
            let dropped = self.dropped.clone();
            Box::pin(async move {
                struct DropSignal(Arc<std::sync::atomic::AtomicBool>);
                impl Drop for DropSignal {
                    fn drop(&mut self) {
                        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }

                let _drop_signal = DropSignal(dropped);
                barrier.wait().await;
                std::future::pending().await
            })
        }
    }

    struct ImmediateMedia;

    impl hkask_types::InferencePort for ImmediateMedia {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            panic!("media job tests must use media_generate")
        }

        fn media_generate<'a>(
            &'a self,
            _: &str,
            _: &hkask_types::MediaGenerateParams,
        ) -> hkask_types::MediaFuture<'a> {
            Box::pin(async {
                Ok(serde_json::json!({
                    "data": [{"b64_json": "iVBORw0KGgo="}],
                    "model": "blocking-test"
                }))
            })
        }
    }

    struct ErrorMedia {
        message: &'static str,
    }

    impl hkask_types::InferencePort for ErrorMedia {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            panic!("media job tests must use media_generate")
        }

        fn media_generate<'a>(
            &'a self,
            _: &str,
            _: &hkask_types::MediaGenerateParams,
        ) -> hkask_types::MediaFuture<'a> {
            Box::pin(async move {
                Err(hkask_types::InferenceError::Connection(
                    self.message.to_string(),
                ))
            })
        }
    }

    fn server_with_port(
        port: Arc<dyn hkask_types::InferencePort>,
        job_store: crate::jobs::JobStore,
    ) -> Result<MediaServer, Box<dyn std::error::Error>> {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        Ok(MediaServer::new(
            hkask_types::WebID::new(),
            port,
            Arc::new(Mutex::new(None)),
            Arc::new(GalleryStore::from_driver(driver)?),
            crate::templates::create_env()?,
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            job_store,
            None,
            None,
        ))
    }

    async fn submit(server: &MediaServer) -> Result<String, McpToolError> {
        let params = serde_json::to_string(&hkask_types::MediaGenerateParams {
            prompt: Some("blocked test image".to_string()),
            ..Default::default()
        })
        .map_err(|error| McpToolError::internal(format!("encode test params: {error}")))?;
        let response = server
            .job_submit(Parameters(JobSubmitRequest {
                op: "generate_image".to_string(),
                params,
            }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&response)
            .map_err(|error| McpToolError::internal(format!("decode submit response: {error}")))?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        payload["job_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| McpToolError::internal("submit response omitted job_id"))
    }

    async fn wait_until(predicate: impl Fn() -> bool) -> Result<(), Box<dyn std::error::Error>> {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !predicate() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    /// expect: A fifth combined direct operation fails before provider work while control reads stay available.
    #[tokio::test]
    async fn saturated_combined_capacity_rejects_direct_tool_but_keeps_job_list_available()
    -> Result<(), Box<dyn std::error::Error>> {
        let store = crate::jobs::new_job_store();
        let server = server_with_port(Arc::new(NoInference), store.clone())?;
        let _direct = (0..3)
            .map(|_| store.admit_direct())
            .collect::<Result<Vec<_>, _>>()?;
        let _job = store.admit(JobRecord {
            id: "combined-capacity-job".to_string(),
            op: "generate_image".to_string(),
            status: "queued".to_string(),
            created_at: hkask_types::time::now_rfc3339(),
            completed_at: None,
            result: None,
            error: None,
        })?;

        let error = server
            .generate_image(Parameters(GenerateImageRequest {
                prompt: "must not reach provider".to_string(),
                image_size: None,
                num_images: Some(1),
                style: None,
            }))
            .await
            .expect_err("fifth combined operation must fail visibly");
        assert!(error.message.contains("capacity exhausted"));

        let list = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: Some(20),
            }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&list)?;
        let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
        assert_eq!(payload["total"], 1);
        Ok(())
    }

    /// expect: Cancelling one of four running jobs returns only after its provider future is
    /// dropped, its active slot is released, and a fifth job can be admitted immediately.
    #[tokio::test]
    async fn cancellation_acknowledges_only_after_teardown_and_slot_release()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let temp = tempfile::TempDir::new()?;
        let prior = std::env::var_os("HKASK_ARTIFACTS_DIR");
        unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", temp.path()) };

        let barrier = Arc::new(tokio::sync::Barrier::new(
            MAX_CONCURRENT_HEAVY_OPERATIONS + 1,
        ));
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let store = crate::jobs::new_job_store();
        let server = server_with_port(
            Arc::new(BlockingMedia {
                barrier: barrier.clone(),
                dropped: dropped.clone(),
            }),
            store.clone(),
        )?;
        let mut job_ids = Vec::new();
        for _ in 0..MAX_CONCURRENT_HEAVY_OPERATIONS {
            job_ids.push(submit(&server).await?);
        }
        barrier.wait().await;

        let cancel_response = server
            .job_cancel(Parameters(JobCancelRequest {
                job_id: job_ids[0].clone(),
            }))
            .await?;
        let cancel_value: serde_json::Value = serde_json::from_str(&cancel_response)?;
        let cancel_payload = hkask_types::tool_response::unwrap_tool_envelope(cancel_value);
        assert_eq!(cancel_payload["history_scope"], JOB_HISTORY_SCOPE);

        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(store.active_count()?, MAX_CONCURRENT_HEAVY_OPERATIONS - 1);
        let fifth_job_id = submit(&server).await?;
        assert!(!fifth_job_id.is_empty());

        let job = store
            .get(&job_ids[0])?
            .ok_or_else(|| "cancelled job record missing".to_string())?;
        assert_eq!(job.status, "cancelled");
        assert!(job.result.is_none());
        assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);

        match prior {
            Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
            None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
        }
        Ok(())
    }

    /// expect: Cancelling after bytes are staged but before publication removes staged/final
    /// files and leaves no gallery row before acknowledging cancellation.
    #[tokio::test]
    async fn cancellation_after_staging_rolls_back_file_and_gallery_publication()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let temp = tempfile::TempDir::new()?;
        let prior = std::env::var_os("HKASK_ARTIFACTS_DIR");
        unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", temp.path()) };

        let entered = Arc::new(tokio::sync::Semaphore::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let store = crate::jobs::new_job_store();
        store.set_publication_gate_for_test(entered.clone(), release)?;
        let server = server_with_port(Arc::new(ImmediateMedia), store.clone())?;
        let gallery = server.gallery_store.open(
            &temp.path().to_string_lossy(),
            hkask_storage::GalleryMode::ReadOnly,
        )?;
        let mut state = GalleryState::new(
            temp.path().to_path_buf(),
            hkask_storage::GalleryMode::ReadOnly,
        );
        state.gallery_id = Some(gallery.id.clone());
        *server
            .gallery_state
            .lock()
            .map_err(|error| error.to_string())? = Some(state);
        let job_id = submit(&server).await?;
        let entered_permit =
            tokio::time::timeout(std::time::Duration::from_secs(2), entered.acquire_owned())
                .await??;
        entered_permit.forget();
        let staged_paths = std::fs::read_dir(crate::assets::generated_assets_dir())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(staged_paths.len(), 1);
        assert!(
            staged_paths[0]
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".staged"))
        );
        assert_eq!(server.gallery_store.count_assets(&gallery.id)?, 0);

        server
            .job_cancel(Parameters(JobCancelRequest {
                job_id: job_id.clone(),
            }))
            .await?;
        wait_until(|| store.active_count().is_ok_and(|count| count == 0)).await?;

        let job = store
            .get(&job_id)?
            .ok_or_else(|| "cancelled job record missing".to_string())?;
        assert_eq!(job.status, "cancelled");
        assert!(job.result.is_none());
        assert_eq!(
            std::fs::read_dir(crate::assets::generated_assets_dir())?.count(),
            0
        );
        assert_eq!(server.gallery_store.count_assets(&gallery.id)?, 0);

        match prior {
            Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
            None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
        }
        Ok(())
    }

    /// expect: Job completion publishes one durable Asset creation graph before becoming terminal.
    #[tokio::test]
    async fn completed_job_publishes_lineage_and_omc_graph()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let temp = tempfile::TempDir::new()?;
        let prior = std::env::var_os("HKASK_ARTIFACTS_DIR");
        unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", temp.path()) };

        let store = crate::jobs::new_job_store();
        let server = server_with_port(Arc::new(ImmediateMedia), store.clone())?;
        let gallery = server.gallery_store.open(
            &temp.path().to_string_lossy(),
            hkask_storage::GalleryMode::ReadOnly,
        )?;
        let mut state = GalleryState::new(
            temp.path().to_path_buf(),
            hkask_storage::GalleryMode::ReadOnly,
        );
        state.gallery_id = Some(gallery.id.clone());
        *server
            .gallery_state
            .lock()
            .map_err(|error| error.to_string())? = Some(state);

        let job_id = submit(&server).await?;
        wait_until(|| {
            store
                .get(&job_id)
                .is_ok_and(|job| job.is_some_and(|job| job.status == "completed"))
        })
        .await?;
        let job = store
            .get(&job_id)?
            .ok_or_else(|| "completed job record missing".to_string())?;
        let result = job
            .result
            .ok_or_else(|| "completed job omitted publication result".to_string())?;
        let asset_id = result["gallery_asset_id"]
            .as_str()
            .ok_or_else(|| "completed job omitted Asset id".to_string())?;
        let task_id = result["omc_task_id"]
            .as_str()
            .ok_or_else(|| "completed job omitted Task id".to_string())?;
        let generation = server
            .gallery_store
            .get_generation(asset_id)?
            .ok_or_else(|| "completed job omitted generation lineage".to_string())?;
        assert_eq!(generation.id, task_id);
        assert_eq!(generation.prompt.as_deref(), Some("blocked test image"));
        let graph = server
            .gallery_store
            .get_omc_creation_graph(asset_id)?
            .ok_or_else(|| "completed job omitted OMC graph".to_string())?;
        let graph: crate::omc::CreationGraph = serde_json::from_str(&graph.graph_json)?;
        assert_eq!(graph.asset_id, asset_id);
        assert_eq!(graph.task_id, task_id);

        match prior {
            Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
            None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
        }
        Ok(())
    }

    /// expect: Provider failures reach job status without being replaced by persistence text.
    #[tokio::test]
    async fn provider_error_is_preserved_in_failed_job_record()
    -> Result<(), Box<dyn std::error::Error>> {
        const MESSAGE: &str = "provider sentinel: request rejected upstream";
        let store = crate::jobs::new_job_store();
        let server = server_with_port(Arc::new(ErrorMedia { message: MESSAGE }), store.clone())?;
        let job_id = submit(&server).await?;
        wait_until(|| {
            store
                .get(&job_id)
                .ok()
                .flatten()
                .is_some_and(|job| job.status == "failed")
        })
        .await?;

        let job = store
            .get(&job_id)?
            .ok_or_else(|| "failed provider job record missing".to_string())?;
        let expected = hkask_types::InferenceError::Connection(MESSAGE.to_string()).to_string();
        assert_eq!(job.error.as_deref(), Some(expected.as_str()));
        Ok(())
    }

    /// expect: Capacity exhaustion is a typed visible tool error, not an unbounded queue.
    #[tokio::test]
    async fn fifth_submission_returns_typed_overload_error()
    -> Result<(), Box<dyn std::error::Error>> {
        let barrier = Arc::new(tokio::sync::Barrier::new(5));
        let store = crate::jobs::new_job_store();
        let server = server_with_port(
            Arc::new(BlockingMedia {
                barrier: barrier.clone(),
                dropped: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }),
            store,
        )?;
        let mut job_ids = Vec::new();
        for _ in 0..4 {
            job_ids.push(submit(&server).await?);
        }
        barrier.wait().await;

        let error = submit(&server)
            .await
            .expect_err("fifth submission must be rejected");
        assert_eq!(error.kind, hkask_types::McpErrorKind::RateLimited);
        assert!(error.message.contains("4"));
        assert_eq!(job_ids.len(), 4);
        Ok(())
    }

    /// expect: [P7] Actual server responses decode with the same contract the
    /// queue consumes, including valid empty lists and preserved job details.
    #[tokio::test]
    async fn job_list_response_round_trips_through_client_decoder()
    -> Result<(), Box<dyn std::error::Error>> {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoInference),
            Arc::new(Mutex::new(None)),
            Arc::new(GalleryStore::from_driver(driver)?),
            crate::templates::create_env()?,
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            None,
        );
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: None,
            }))
            .await?;
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
        let server = server_with_port(Arc::new(NoInference), crate::jobs::new_job_store())?;
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
        let server = server_with_port(Arc::new(NoInference), crate::jobs::new_job_store())?;
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: None,
            }))
            .await?;
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
