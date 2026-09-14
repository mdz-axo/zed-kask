//! Shared resource limits for media MCP operations.
//!
//! These caps are admission limits: callers must reject zero or over-cap
//! requests where the operation requires at least one item. They must never
//! clamp or silently truncate requested work.

/// Maximum number of audio or video inputs accepted by concat operations.
pub const MAX_CONCAT_ITEMS: usize = 64;
/// Maximum number of gallery images accepted by one image-sequence render.
pub const MAX_IMAGE_SEQUENCE_ITEMS: usize = 256;
/// Maximum number of keyframes accepted by one extraction request.
pub const MAX_EXTRACTED_FRAMES: u32 = 256;
/// Maximum number of image variants accepted by one generation request.
pub const MAX_GENERATION_VARIANTS: u32 = 10;
/// Default number of generation jobs returned by `job_list`.
pub const DEFAULT_JOB_LIST_LIMIT: usize = 20;
/// Maximum number of generation jobs returned by `job_list`.
pub const MAX_JOB_LIST_LIMIT: usize = 256;
/// Maximum UTF-8 byte length of a serialized workflow graph.
pub const MAX_WORKFLOW_GRAPH_BYTES: usize = 1_048_576;
/// Default number of workflow summaries returned by `workflow_list`.
pub const DEFAULT_WORKFLOW_LIST_LIMIT: usize = 100;
/// Maximum number of workflow summaries returned by `workflow_list`.
pub const MAX_WORKFLOW_LIST_LIMIT: usize = 256;
