//! Error types for the media MCP server.
//!
//! Replaces `Result<_, String>` with a structured `MediaError` enum.
//! `map_media_error` classifies errors into MCP wire-level `McpToolError` kinds.

use hkask_mcp_server::server::McpToolError;
use hkask_storage::GalleryStoreError;
use hkask_types::{EmbeddingGenerationError, InferenceError};
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
        | MediaError::FaceRegistration(_) => McpToolError::internal(e.to_string()), // rr0044-ok: mapper-internal-arm
    }
}

/// Classify a `GalleryStoreError` from a gallery-store query into the MCP
/// wire-level `McpToolError` kind: `NotFound` → `not_found`, infrastructure
/// → per-variant via the shared `map_infra_error`, `InvalidMode` /
/// `AlreadyExists` are caller-fixable (`invalid_argument`).
pub fn map_gallery_store_error(e: GalleryStoreError) -> McpToolError {
    let message = e.to_string();
    match e {
        GalleryStoreError::NotFound(_) => McpToolError::not_found(message),
        GalleryStoreError::Infra(ref infra) => {
            hkask_mcp_server::server::map_infra_error(infra, "gallery store")
        }
        GalleryStoreError::InvalidMode(_) | GalleryStoreError::AlreadyExists(_) => {
            McpToolError::invalid_argument(message)
        }
    }
}

/// Classify an `image::open` failure on a caller-referenced path.
///
/// A missing file is `not_found` and a permission failure is
/// `permission_denied` (caller/environment errors); other I/O kinds and
/// opaque decode failures stay `internal`.
pub fn map_image_open_error(path: &std::path::Path, e: image::ImageError) -> McpToolError {
    let message = format!("Failed to open {}: {}", path.display(), e);
    match e {
        image::ImageError::IoError(io) => match io.kind() {
            std::io::ErrorKind::NotFound => McpToolError::not_found(message),
            std::io::ErrorKind::PermissionDenied => McpToolError::permission_denied(message),
            _ => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
        },
        _ => McpToolError::internal(message), // rr0044-ok: mapper-internal-arm
    }
}

/// Substrings that mark an embedding error as a missing-credential /
/// missing-provider configuration failure rather than a transient outage.
///
/// NOTE: string-matching on "api key not configured" / "no provider
/// configured" — `InferenceError` now carries a typed `NotConfigured` variant
/// (see `classify_inference_error`), but `EmbeddingGenerationError` does not
/// yet have one. No embedding backend currently emits a not-configured
/// message, so this is purely defensive. If you add a not-configured
/// construction site to an embedding backend, add a `NotConfigured(String)`
/// variant to `EmbeddingGenerationError` in `hkask-types/src/ports/embedding.rs`
/// and update `classify_embedding_error` to match on the variant instead.
fn is_credential_missing_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("api key not configured") || lower.contains("no provider configured")
}

/// Classify an `InferenceError` from a media/vision inference call into the
/// MCP wire-level `McpToolError` kind.
///
/// A missing-credential / missing-provider configuration failure (the typed
/// `InferenceError::NotConfigured` variant, emitted by `hkask-inference` when
/// an API key env var is unset or no provider is registered for the op) maps
/// to `permission_denied` (matching the canonical `hkask-mcp-swarm` pattern
/// for `"no API key configured"`); every other failure (transient outage,
/// model error, JSON parse, circuit open) stays `unavailable`. The full
/// error message is preserved so the operator can diagnose.
pub fn classify_inference_error(prefix: &str, error: InferenceError) -> McpToolError {
    let message = format!("{}: {}", prefix, error);
    match error {
        InferenceError::NotConfigured(_) => McpToolError::permission_denied(message),
        _ => McpToolError::unavailable(message),
    }
}

/// Classify an `EmbeddingGenerationError` from an embedding call into the MCP
/// wire-level `McpToolError` kind. Same credential-vs-transient split as
/// [`classify_inference_error`], but `EmbeddingGenerationError` has no typed
/// `NotConfigured` variant yet, so this falls back to string-matching via
/// [`is_credential_missing_error`].
pub fn classify_embedding_error(prefix: &str, error: EmbeddingGenerationError) -> McpToolError {
    let message = format!("{}: {}", prefix, error);
    if is_credential_missing_error(&message) {
        McpToolError::permission_denied(message)
    } else {
        McpToolError::unavailable(message)
    }
}
