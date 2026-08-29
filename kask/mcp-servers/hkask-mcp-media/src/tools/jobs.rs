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
        if let Ok(mut store) = self.job_store.lock()
            && let Some(job) = store.get_mut(&self.job_id)
            && job.status != "cancelled"
        {
            job.status = "failed".to_string();
            job.error =
                Some("job task terminated unexpectedly (panic or abort)".to_string());
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
    ) -> String {
        execute_tool_semantic(
            self,
            "job_submit",
            Self::ontology_anchor("job_submit"),
            async {
                if op.trim().is_empty() {
                    return Err(McpToolError::invalid_argument("op must not be empty"));
                }
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
                        .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?;
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
                let job_id_for_task = job_id.clone();
                let op_for_task = op.clone();

                tokio::spawn(async move {
                    // Update status to "running".
                    {
                        if let Ok(mut store) = job_store.lock() {
                            if let Some(job) = store.get_mut(&job_id_for_task) {
                                job.status = "running".to_string();
                            }
                        }
                    }

                    // If the generation future panics or the task is aborted,
                    // this guard marks the job failed on drop instead of
                    // leaving it stuck in "running".
                    let guard =
                        JobPanicGuard::new(job_store.clone(), job_id_for_task.clone());

                    let result = vision_port
                        .media_generate(&op_for_task, &media_params)
                        .await;

                    // Update the job record with the result.
                    {
                        if let Ok(mut store) = job_store.lock() {
                            if let Some(job) = store.get_mut(&job_id_for_task) {
                                let now = hkask_types::time::now_rfc3339();
                                job.completed_at = Some(now);
                                match result {
                                    Ok(value) => {
                                        // Check if the job was cancelled while running.
                                        if job.status == "cancelled" {
                                            // Keep cancelled status.
                                        } else {
                                            job.status = "completed".to_string();
                                            job.result = Some(value);
                                        }
                                    }
                                    Err(e) => {
                                        if job.status == "cancelled" {
                                            // Keep cancelled status.
                                        } else {
                                            job.status = "failed".to_string();
                                            job.error = Some(e.to_string());
                                        }
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
            },
        )
        .await
    }

    /// List generation jobs with their status. Optionally filter by status.
    #[tool(
        description = "List generation jobs with their status. Optionally filter by status (queued, running, completed, failed, cancelled)."
    )]
    pub async fn job_list(
        &self,
        Parameters(JobListRequest { status, limit }): Parameters<JobListRequest>,
    ) -> String {
        execute_tool_semantic(self, "job_list", Self::ontology_anchor("job_list"), async {
            let store = self
                .job_store
                .lock()
                .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?;

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
                .map_err(|e| McpToolError::internal(format!("encode job list: {e}")))
        })
        .await
    }

    /// Get the status of a specific generation job by its ID.
    #[tool(description = "Get the status of a specific generation job by its ID.")]
    pub async fn job_status(
        &self,
        Parameters(JobStatusRequest { job_id }): Parameters<JobStatusRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "job_status",
            Self::ontology_anchor("job_status"),
            async {
                let store = self
                    .job_store
                    .lock()
                    .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?;
                let job = store.get(&job_id).ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "Job not found: {job_id}. The job store is in-memory — if the \
                         media server restarted, all job records were lost (persistent \
                         lineage survives in gallery_record_generation). Call job_list \
                         to see known jobs."
                    ))
                })?;
                serde_json::to_value(job)
                    .map_err(|e| McpToolError::internal(format!("encode job status: {e}")))
            },
        )
        .await
    }

    /// Cancel a running or queued generation job.
    #[tool(description = "Cancel a running or queued generation job by its ID.")]
    pub async fn job_cancel(
        &self,
        Parameters(JobCancelRequest { job_id }): Parameters<JobCancelRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "job_cancel",
            Self::ontology_anchor("job_cancel"),
            async {
                let mut store = self
                    .job_store
                    .lock()
                    .map_err(|e| McpToolError::internal(format!("job store lock: {e}")))?;
                let job = store.get_mut(&job_id).ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "Job not found: {job_id}. Call job_list to see known jobs."
                    ))
                })?;

                if job.status == "completed" || job.status == "failed" || job.status == "cancelled"
                {
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
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
