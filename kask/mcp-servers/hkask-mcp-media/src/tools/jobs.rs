//! Generation job queue tools — submit, list, status, cancel.
//!
//! Fills the OMC `Task` concept with real-time job tracking. `job_submit`
//! spawns a background tokio task that calls `vision_port.media_generate`,
//! returning a job ID immediately. `job_list` / `job_status` / `job_cancel`
//! read from the in-memory job store.
use crate::types::{
    JobCancelRequest, JobListRequest, JobRecord, JobStatusRequest, JobSubmitRequest,
};
use crate::*;

/// Decode the `job_list` wire contract at a client boundary. The payload is
/// an array of complete job records, not an object containing `jobs`.
///
/// expect: A broken queue response must not look like an empty queue.
/// [P7] Motivating: server and panel share one response contract.
/// pre: output is the tool's serialized response.
/// post: valid arrays (including empty) decode; malformed data and tool errors fail.
pub fn parse_job_list_response(output: &str) -> Result<Vec<JobRecord>, String> {
    let value: serde_json::Value = serde_json::from_str(output)
        .map_err(|error| format!("invalid job_list response: {error}"))?;
    let payload = hkask_types::tool_response::unwrap_tool_envelope(value);
    if let Some(error) = hkask_types::tool_response::parse_tool_error_value(&payload) {
        return Err(error.message);
    }
    serde_json::from_value(payload).map_err(|error| {
        format!("invalid job_list response (expected an array of job records): {error}")
    })
}

/// Marks a job `failed` when dropped without being defused — a panic or
/// abort inside the spawned generation task must not leave the record in
/// "running" forever (the operator could not distinguish a live job from a
/// dead one). Defused on the normal completion path.
struct JobPanicGuard {
    job_store: crate::jobs::JobStore,
    job_id: String,
    defused: bool,
}

impl JobPanicGuard {
    fn new(job_store: crate::jobs::JobStore, job_id: String) -> Self {
        Self {
            job_store,
            job_id,
            defused: false,
        }
    }

    /// Disarm the guard — the task completed and updated the record itself.
    fn defuse(mut self) {
        self.defused = true;
    }
}

impl Drop for JobPanicGuard {
    fn drop(&mut self) {
        if self.defused {
            return;
        }
        let Ok(mut store) = self.job_store.lock() else {
            tracing::warn!(
                target: "hkask.mcp.media.jobs",
                job_id = %self.job_id,
                "Job store lock poisoned — panicked job left in its prior status"
            );
            return;
        };
        if let Some(job) = store.get_mut(&self.job_id)
            && job.status != "cancelled"
        {
            job.status = "failed".to_string();
            job.error = Some("job task terminated unexpectedly (panic or abort)".to_string());
            job.completed_at = Some(hkask_types::time::now_rfc3339());
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
            // result is persisted through `persist_and_slim_result`, and a
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

            let job_id = uuid::Uuid::new_v4().to_string();
            let now = hkask_types::time::now_rfc3339();

            // Insert the job record with "queued" status.
            {
                let mut store = self
                    .job_store
                    .lock()
                    .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?; // rr0044-ok: lock-poisoning-after-panic
                store.insert(
                    job_id.clone(),
                    JobRecord {
                        id: job_id.clone(),
                        op: op.clone(),
                        status: "queued".to_string(),
                        created_at: now,
                        completed_at: None,
                        result: None,
                        error: None,
                    },
                );
            }

            // Spawn the background generation task.
            let vision_port = self.vision_port.clone();
            let job_store = self.job_store.clone();
            let gallery_state = self.gallery_state.clone();
            let gallery_store = self.gallery_store.clone();
            let job_id_for_task = job_id.clone();
            let op_for_task = op.clone();
            let kind_for_task = kind;

            tokio::spawn(async move {
                // Update status to "running".
                match job_store.lock() {
                    Ok(mut store) => {
                        if let Some(job) = store.get_mut(&job_id_for_task) {
                            job.status = "running".to_string();
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "hkask.mcp.media.jobs",
                            job_id = %job_id_for_task,
                            error = %e,
                            "Job store lock poisoned — 'running' status update skipped"
                        );
                        // Continue anyway: the generation can still run, and
                        // the final update below retries the lock.
                    }
                }

                // If the generation future panics or the task is aborted,
                // this guard marks the job failed on drop instead of
                // leaving it stuck in "running".
                let guard = JobPanicGuard::new(job_store.clone(), job_id_for_task.clone());

                let result = vision_port
                    .media_generate(&op_for_task, &media_params)
                    .await;

                // Persist the payload and compose the slim result BEFORE
                // taking the job-store lock — the std Mutex guard is !Send
                // and cannot be held across the await. The slim result means
                // the raw provider response (base64 payloads) never enters
                // the job record the model reads back.
                let persisted = match result {
                    Ok(value) => persist_and_slim_result(
                        &gallery_state,
                        &gallery_store,
                        &value,
                        kind_for_task,
                    )
                    .await
                    .map_err(|error| format!("asset not persisted: {error}")),
                    Err(e) => Err(e.to_string()),
                };

                // Update the job record with the outcome.
                {
                    let Ok(mut store) = job_store.lock() else {
                        tracing::warn!(
                            target: "hkask.mcp.media.jobs",
                            job_id = %job_id_for_task,
                            "Job store lock poisoned — completed job outcome could not be recorded"
                        );
                        return;
                    };
                    if let Some(job) = store.get_mut(&job_id_for_task) {
                        let now = hkask_types::time::now_rfc3339();
                        job.completed_at = Some(now);
                        // A job cancelled while running keeps its cancelled
                        // status — the outcome is discarded, not recorded.
                        if job.status != "cancelled" {
                            match persisted {
                                Ok(slim) => {
                                    job.status = "completed".to_string();
                                    job.result = Some(slim);
                                }
                                Err(error) => {
                                    job.status = "failed".to_string();
                                    job.error = Some(error);
                                }
                            }
                        }
                    }
                }

                // Normal completion — disarm the panic guard.
                guard.defuse();
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
        description = "List generation jobs with their status. Optionally filter by status (queued, running, completed, failed, cancelled)."
    )]
    pub async fn job_list(
        &self,
        Parameters(JobListRequest { status, limit }): Parameters<JobListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_list", async {
            let store = self
                .job_store
                .lock()
                .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?; // rr0044-ok: lock-poisoning-after-panic

            let max = limit.unwrap_or(20);
            let mut jobs: Vec<JobRecord> = store
                .values()
                .filter(|job| status.as_ref().map_or(true, |s| job.status == *s))
                .cloned()
                .collect();

            // Sort by created_at descending (newest first).
            jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            jobs.truncate(max);

            serde_json::to_value(&jobs)
                .map_err(|e| McpToolError::internal(format!("encode job list: {e}"))) // rr0044-ok: serde serialization of own data
        })
        .await
    }

    /// Get the status of a specific generation job by its ID.
    #[tool(description = "Get the status of a specific generation job by its ID.")]
    pub async fn job_status(
        &self,
        Parameters(JobStatusRequest { job_id }): Parameters<JobStatusRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_status", async {
            let store = self
                .job_store
                .lock()
                .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?; // rr0044-ok: lock-poisoning-after-panic
            let job = store.get(&job_id).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "Job not found: {job_id}. The job store is in-memory — if the \
                         media server restarted, all job records were lost (persistent \
                         lineage survives in gallery_record_generation). Call job_list \
                         to see known jobs."
                ))
            })?;
            serde_json::to_value(job)
                .map_err(|e| McpToolError::internal(format!("encode job status: {e}"))) // rr0044-ok: serde serialization of own data
        })
        .await
    }

    /// Cancel a running or queued generation job.
    #[tool(description = "Cancel a running or queued generation job by its ID.")]
    pub async fn job_cancel(
        &self,
        Parameters(JobCancelRequest { job_id }): Parameters<JobCancelRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "job_cancel", async {
            let mut store = self
                .job_store
                .lock()
                .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?; // rr0044-ok: lock-poisoning-after-panic
            let job = store.get_mut(&job_id).ok_or_else(|| {
                McpToolError::not_found(format!(
                    "Job not found: {job_id}. Call job_list to see known jobs."
                ))
            })?;

            if job.status == "completed" || job.status == "failed" || job.status == "cancelled" {
                return Err(McpToolError::invalid_argument(format!(
                    "Job {job_id} is already {} — cannot cancel",
                    job.status
                )));
            }

            job.status = "cancelled".to_string();
            job.completed_at = Some(hkask_types::time::now_rfc3339());

            Ok(serde_json::json!({
                "job_id": job_id,
                "status": "cancelled",
            }))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            panic!("job_list must not invoke inference")
        }
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
        );
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: None,
            }))
            .await?;
        assert!(parse_job_list_response(&response)?.is_empty());
        {
            let mut store = server.job_store.lock().map_err(|error| error.to_string())?;
            for (id, status, created_at) in [
                ("older", "completed", "2026-09-04T00:00:00Z"),
                ("newer", "running", "2026-09-05T00:00:00Z"),
            ] {
                store.insert(
                    id.into(),
                    JobRecord {
                        id: id.into(),
                        op: "generate_image".into(),
                        status: status.into(),
                        created_at: created_at.into(),
                        completed_at: None,
                        result: Some(serde_json::json!({"output": "/tmp/雪.png"})),
                        error: None,
                    },
                );
            }
        }
        let response = server
            .job_list(Parameters(JobListRequest {
                status: None,
                limit: None,
            }))
            .await?;
        let jobs = parse_job_list_response(&response)?;
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].id, "newer");
        assert_eq!(jobs[1].id, "older");
        assert_eq!(
            jobs[0].result,
            Some(serde_json::json!({"output": "/tmp/雪.png"}))
        );
        let response = server
            .job_list(Parameters(JobListRequest {
                status: Some("completed".into()),
                limit: Some(1),
            }))
            .await?;
        let jobs = parse_job_list_response(&response)?;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "older");
        Ok(())
    }

    #[test]
    fn job_store_starts_empty() {
        let store = crate::jobs::new_job_store();
        let guard = store.lock().unwrap();
        assert!(guard.is_empty());
    }

    #[test]
    fn job_store_insert_and_get() {
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
        {
            let mut guard = store.lock().unwrap();
            guard.insert("test-job-1".to_string(), record);
        }
        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 1);
        assert_eq!(guard.get("test-job-1").unwrap().op, "generate_image");
    }
}
