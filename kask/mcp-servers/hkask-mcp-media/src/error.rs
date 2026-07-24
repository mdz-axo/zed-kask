//! Error types for the media MCP server.
//!
//! Replaces `Result<_, String>` with a structured `MediaError` enum.
//! `map_media_error` classifies errors into MCP wire-level `McpToolError` kinds.

use hkask_mcp_server::server::McpToolError;
use hkask_storage::GalleryStoreError;
use thiserror::Error;

/// Structured error for media server operations.
#[derive(Debug, Error)]
pub enum MediaError {
    /// Gallery not organized or persisted — user must run `gallery_organize` first.
    #[error("No gallery organized. Use gallery_organize first.")]
    GalleryNotInitialized,

    /// Image not found at a given index or ID.
    #[error("{0}")]
    ImageNotFound(String),

    /// Filesystem I/O errors.
    #[error("{0}")]
    Io(String),

    /// Jinja2 template rendering errors.
    #[error("{0}")]
    Template(String),

    /// ffmpeg not installed on the system.
    #[error("ffmpeg not available")]
    FfmpegUnavailable,

    /// ffmpeg command execution failures.
    #[error("{0}")]
    FfmpegFailed(String),

    /// Vision LLM API errors.
    #[error("{0}")]
    VisionApi(String),

    /// Vision response parsing errors.
    #[error("{0}")]
    VisionParse(String),

    /// Face scan: no YAML sidecar found for an image (skippable).
    #[error("{0}: no YAML sidecar found")]
    SidecarNotFound(String),

    /// Face scan: sidecar YAML parse or validation failure.
    #[error("{0}")]
    SidecarInvalid(String),

    /// Face scan: image import or registration failure.
    #[error("{0}")]
    FaceRegistration(String),
}

impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        MediaError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for MediaError {
    fn from(e: serde_json::Error) -> Self {
        MediaError::VisionParse(e.to_string())
    }
}

impl From<GalleryStoreError> for MediaError {
    fn from(e: GalleryStoreError) -> Self {
        match e {
            GalleryStoreError::NotFound(nf) => MediaError::ImageNotFound(nf.to_string()),
            other => MediaError::Io(other.to_string()),
        }
    }
}

/// Map a `MediaError` to the appropriate `McpToolError` kind.
///
/// - `GalleryNotInitialized`, `ImageNotFound` → `invalid_argument` (user error)
/// - `Io`, `FfmpegFailed`, `VisionApi`, `VisionParse`, `Template` → `internal` (system error)
/// - `FfmpegUnavailable` → `unavailable` (system unavailable)
pub fn map_media_error(e: MediaError) -> McpToolError {
    match e {
        MediaError::GalleryNotInitialized | MediaError::ImageNotFound(_) => {
            McpToolError::invalid_argument(e.to_string())
        }
        MediaError::FfmpegUnavailable => McpToolError::unavailable(e.to_string()),
        MediaError::Io(_)
        | MediaError::FfmpegFailed(_)
        | MediaError::VisionApi(_)
        | MediaError::VisionParse(_)
        | MediaError::Template(_)
        | MediaError::SidecarNotFound(_)
        | MediaError::SidecarInvalid(_)
        | MediaError::FaceRegistration(_) => McpToolError::internal(e.to_string()),
    }
}
