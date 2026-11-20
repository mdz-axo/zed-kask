//! Shared resource limits for media MCP operations.
//!
//! These caps are admission limits: callers must reject zero or over-cap
//! requests where the operation requires at least one item. They must never
//! clamp or silently truncate requested work.

/// Maximum number of audio or video inputs accepted by concat operations.
pub const MAX_CONCAT_ITEMS: usize = 64;
/// Maximum number of keyframes accepted by one extraction request.
pub const MAX_EXTRACTED_FRAMES: u32 = 256;
/// Default number of research jobs returned by `job_list`.
pub const DEFAULT_JOB_LIST_LIMIT: usize = 20;
/// Maximum number of research jobs returned by `job_list`.
pub const MAX_JOB_LIST_LIMIT: usize = 256;
/// Maximum duration in seconds accepted by microphone capture
/// (`audio_capture`, `record_and_transcribe`). One hour. `f32` to match
/// the request fields it caps.
pub const MAX_CAPTURE_DURATION_SECS: f32 = 3600.0;
