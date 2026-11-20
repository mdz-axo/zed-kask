//! Strict `job_list` wire contract shared by `hkask-mcp-media` and `media_panel`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const JOB_HISTORY_SCOPE: &str = "ephemeral_process_local";
pub const JOB_RESTART_BEHAVIOR: &str = "Job history is ephemeral and process-local; records are lost when the media server restarts and older terminal records may be removed by bounded retention.";

/// Every status a job record can carry — the `job_list` filter's closed set.
pub const JOB_STATUSES: [&str; 6] = [
    "queued",
    "running",
    "cancelling",
    "completed",
    "failed",
    "cancelled",
];

/// A research job record — tracks the lifecycle of an async media operation.
/// Maps to the OMC `Task` concept.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    /// Unique job identifier (UUID).
    pub id: String,
    /// The research operation, such as "transcribe" or "video_clip".
    pub op: String,
    /// Job status: "queued", "running", "cancelling", "completed", "failed", or "cancelled".
    pub status: String,
    /// ISO 8601 timestamp when the job was created.
    pub created_at: String,
    /// ISO 8601 timestamp when the job completed (set when status is completed/failed/cancelled).
    pub completed_at: Option<String>,
    /// The slim durable publication result on success; provider payload bytes are excluded.
    pub result: Option<serde_json::Value>,
    /// Error message on failure.
    pub error: Option<String>,
    /// The operation's primary source path/URL — the transcribe input, the
    /// trim/clip source. The media panel's recordings rows match running
    /// jobs to their recording by it (R2/R5, 2026-10-28).
    #[serde(default)]
    pub source: Option<String>,
}

/// Strict `job_list` response consumed by the media panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobListPayload {
    pub jobs: Vec<JobRecord>,
    pub total: usize,
    pub limit: usize,
    pub has_more: bool,
    pub history_scope: String,
    pub restart_behavior: String,
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
        || payload.limit > crate::media_limits::MAX_JOB_LIST_LIMIT
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
