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

    /// yt-dlp not installed on the system (video_fetch unavailable).
    #[error("yt-dlp not available")]
    YtDlpUnavailable,

    /// yt-dlp ran but exited non-zero. Carries the exit status and the
    /// tail of yt-dlp's stderr so the operator sees the actual failure
    /// (unsupported URL, HTTP 403, stale extractor) instead of a generic hint.
    #[error("yt-dlp fetch failed: {0}")]
    YtDlpFailed(String),

    /// ffmpeg command execution failures.
    #[error("{0}")]
    FfmpegFailed(String),

    /// Vision LLM API errors.
    #[error("{0}")]
    VisionApi(String),

    /// Vision response parsing errors.
    #[error("{0}")]
    VisionParse(String),

    /// Generated-asset persistence failure. The raw provider payload is
    /// never the tool-result fallback (base64 payloads overflow the model
    /// context — see `persist_and_slim_result`), so a persist failure fails
    /// the tool with this error and the operator can retry.
    #[error("Generated asset not persisted: {0}")]
    AssetPersistence(String),

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
/// - `FfmpegUnavailable`, `YtDlpUnavailable` → `unavailable` (system unavailable)
pub fn map_media_error(e: MediaError) -> McpToolError {
    match e {
        MediaError::GalleryNotInitialized | MediaError::ImageNotFound(_) => {
            McpToolError::invalid_argument(e.to_string())
        }
        MediaError::FfmpegUnavailable | MediaError::YtDlpUnavailable => {
            McpToolError::unavailable(e.to_string())
        }
        MediaError::YtDlpFailed(_) => McpToolError::internal(e.to_string()), // rr0044-ok: mapper-internal-arm
        MediaError::Io(_)
        | MediaError::FfmpegFailed(_)
        | MediaError::VisionApi(_)
        | MediaError::VisionParse(_)
        | MediaError::Template(_)
        | MediaError::AssetPersistence(_)
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
/// for `"no API key configured"`); so does a rejected credential (the typed
/// `InferenceError::Auth` variant — HTTP 401/403 from a provider: the key is
/// present but invalid, expired, or unauthorized for the resource, which is
/// an authorization failure to fix, not a transient outage to retry).
/// Invalid model selection maps to `invalid_argument`, overload to
/// `rate_limited`, and timeout to `timeout`. Other failures (connection,
/// JSON parse, circuit open) stay `unavailable`. Messages are preserved.
pub fn classify_inference_error(prefix: &str, error: InferenceError) -> McpToolError {
    let message = format!("{}: {}", prefix, error);
    match error {
        InferenceError::NotConfigured(_) | InferenceError::Auth(_) => {
            McpToolError::permission_denied(message)
        }
        InferenceError::Model(_) => McpToolError::invalid_argument(message),
        InferenceError::Overloaded(_) => McpToolError::rate_limited(message),
        InferenceError::Timeout(_) => {
            McpToolError::new(hkask_types::McpErrorKind::Timeout, message)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::McpErrorKind;

    /// expect: "An invalid media model is a fixable argument, not an outage."
    /// [P1] Motivating; dcterms:identifier: classify_inference_error
    #[tokio::test]
    async fn real_router_model_error_is_invalid_argument() {
        let router = hkask_inference::media_router::MediaRouter::new(
            hkask_inference::InferenceConfig::default(),
        );
        let error = router
            .media_generate(
                "generate_image",
                &hkask_types::MediaGenerateParams {
                    model: Some("unqualified-model".into()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("invalid routing");
        assert_eq!(
            classify_inference_error("Image generation failed", error).kind,
            McpErrorKind::InvalidArgument
        );
    }

    /// expect: "A rejected provider key reaches the media tool as permission_denied."
    /// [P4] Motivating; dcterms:identifier: classify_inference_error
    #[tokio::test]
    async fn real_router_http_auth_reaches_media_tool_mapper() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        for provider in ["DeepInfra", "OpenRouter"] {
            for status in [401, 403] {
                let selected = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("selected HTTP");
                let unselected = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("unselected HTTP");
                let selected_url = format!("http://{}", selected.local_addr().expect("address"));
                let unselected_url =
                    format!("http://{}", unselected.local_addr().expect("address"));
                let response = tokio::spawn(async move {
                    let (stream, _) = selected.accept().await.expect("selected request");
                    let mut stream = BufReader::new(stream);
                    let mut line = String::new();
                    let mut length = 0;
                    loop {
                        line.clear();
                        stream.read_line(&mut line).await.expect("request headers");
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = value.trim().parse().expect("content length");
                        }
                    }
                    stream
                        .read_exact(&mut vec![0; length])
                        .await
                        .expect("request body");
                    stream.get_mut().write_all(format!("HTTP/1.1 {status} Unauthorized\r\nContent-Length: 8\r\nConnection: close\r\n\r\nrejected").as_bytes()).await.expect("auth response");
                });
                let router = hkask_inference::media_router::MediaRouter::new(
                    hkask_inference::InferenceConfig {
                        deepinfra_api_key: "sentinel-deepinfra".into(),
                        openrouter_api_key: "sentinel-openrouter".into(),
                        deepinfra_base_url: if provider == "DeepInfra" {
                            selected_url.clone()
                        } else {
                            unselected_url.clone()
                        },
                        openrouter_base_url: if provider == "OpenRouter" {
                            selected_url
                        } else {
                            unselected_url
                        },
                        ..Default::default()
                    },
                );
                let error = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    router.media_generate(
                        "generate_image",
                        &hkask_types::MediaGenerateParams {
                            model: Some(format!("{provider}/vendor/model")),
                            prompt: Some("test".into()),
                            ..Default::default()
                        },
                    ),
                )
                .await
                .expect("bounded routing")
                .expect_err("provider rejects credential");
                assert!(matches!(error, InferenceError::Auth(_)));
                let mapped = classify_inference_error("Image generation failed", error);
                assert_eq!(mapped.kind, McpErrorKind::PermissionDenied);
                assert!(mapped.message.contains("rejected"));
                assert!(!mapped.message.contains("sentinel-"));
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(10), unselected.accept())
                        .await
                        .is_err()
                );
                response.await.expect("HTTP task");
            }
        }
    }

    #[test]
    fn overload_and_timeout_keep_actionable_kinds() {
        assert_eq!(
            classify_inference_error("media", InferenceError::Overloaded("busy".into())).kind,
            McpErrorKind::RateLimited
        );
        assert_eq!(
            classify_inference_error("media", InferenceError::Timeout("deadline".into())).kind,
            McpErrorKind::Timeout
        );
    }

    /// Pins the authorization-failure classification: a rejected credential
    /// (typed `InferenceError::Auth` — HTTP 401/403 from a provider) surfaces
    /// as `permission_denied`, not `unavailable`. The operator seeing
    /// "unavailable" diagnoses a transient outage and retries; the correct
    /// reading is "fix your API key" — the 2026-08-31 DeepInfra 401
    /// (split-brain stale key) presented as `unavailable` and hid the fix.
    #[test]
    fn classify_inference_error_maps_auth_to_permission_denied() {
        let auth = classify_inference_error(
            "Image generation failed",
            InferenceError::Auth(
                "DeepInfra 401 Unauthorized: User is not authorized to access this resource"
                    .to_string(),
            ),
        );
        assert_eq!(auth.kind, McpErrorKind::PermissionDenied);

        let not_configured = classify_inference_error(
            "Image generation failed",
            InferenceError::NotConfigured("OpenRouter API key not configured".to_string()),
        );
        assert_eq!(not_configured.kind, McpErrorKind::PermissionDenied);

        // Transient failures stay `unavailable`.
        let connection = classify_inference_error(
            "Image generation failed",
            InferenceError::Connection("all providers failed".to_string()),
        );
        assert_eq!(connection.kind, McpErrorKind::Unavailable);
    }
}
