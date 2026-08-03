//! Media reference types — how assets are identified and resolved.

use gpui::SharedString;
use std::path::PathBuf;

/// The type of media asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// Raster image (JPEG, PNG, WebP, BMP, TIFF, etc.) — rendered via GPUI `img()`.
    Image,
    /// SVG — rendered via GPUI `svg()`.
    Svg,
    /// Audio file (WAV, MP3, Ogg, FLAC) — played via `rodio`.
    Audio,
    /// Video file (MP4, WebM, MKV, etc.) — decoded via FFmpeg to RGBA, rendered via `img()`.
    Video,
}

/// How a media asset is referenced — mirrors what the hkask media MCP server
/// actually emits in tool responses (filesystem paths, data URIs, remote URLs).
#[derive(Debug, Clone)]
pub enum MediaRef {
    /// A reference to a media asset.
    Asset { src: SharedString, kind: MediaKind },
    /// An error placeholder — displayed when parsing fails.
    Error(SharedString),
}

impl MediaRef {
    /// Create a new media reference.
    pub fn new(src: SharedString, kind: MediaKind) -> Self {
        Self::Asset { src, kind }
    }

    /// The source URL/path/data-URI.
    pub fn src(&self) -> &str {
        match self {
            Self::Asset { src, .. } => src.as_ref(),
            Self::Error(_) => "",
        }
    }

    /// The media kind.
    pub fn kind(&self) -> Option<MediaKind> {
        match self {
            Self::Asset { kind, .. } => Some(*kind),
            Self::Error(_) => None,
        }
    }

    /// Whether this is an error placeholder.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }
}

/// Resolved media — the concrete, loadable form after a `MediaRef` is resolved.
#[derive(Debug, Clone)]
pub struct ResolvedMedia {
    pub kind: MediaKind,
    /// Filesystem path, if the source is a local file.
    pub path: Option<PathBuf>,
    /// Raw bytes, if the source is inline (data URI or pre-loaded).
    pub bytes: Option<Vec<u8>>,
    /// Remote URL, if the source is a network resource.
    pub url: Option<SharedString>,
}

/// Trait for resolving `MediaRef` values to `ResolvedMedia`.
///
/// The gallery-backed implementation looks up `absolute_path` from the
/// SQLite store; the path/data-URI/URL implementations resolve directly.
pub trait MediaStorage: Send + Sync {
    fn resolve(&self, reference: &MediaRef) -> anyhow::Result<ResolvedMedia>;
}

/// Detect `MediaKind` from a file extension or data-URI MIME type.
pub fn detect_kind(src: &str) -> MediaKind {
    if let Some(mime) = src.strip_prefix("data:") {
        if let Some((mime_type, _)) = mime.split_once(',') {
            return match mime_type.split(';').next().unwrap_or("") {
                "image/svg+xml" => MediaKind::Svg,
                "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp"
                | "image/tiff" => MediaKind::Image,
                "audio/wav" | "audio/mpeg" | "audio/ogg" | "audio/flac" => MediaKind::Audio,
                "video/mp4" | "video/webm" | "video/x-matroska" => MediaKind::Video,
                _ => MediaKind::Image,
            };
        }
    }

    let extension = src.rsplit('.').next().unwrap_or("").to_lowercase();
    match extension.as_str() {
        "svg" => MediaKind::Svg,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tiff" | "tif" | "avif" | "ico"
        | "tga" | "dds" | "hdr" | "exr" | "pbm" | "ppm" | "pgm" | "qoi" => MediaKind::Image,
        "wav" | "mp3" | "ogg" | "flac" | "aac" | "m4a" | "opus" => MediaKind::Audio,
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "wmv" => MediaKind::Video,
        _ => MediaKind::Image,
    }
}
