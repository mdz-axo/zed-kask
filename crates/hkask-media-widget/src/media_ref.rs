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

/// Resolves a `MediaRef` to loadable `ResolvedMedia`. The media widget always
/// resolves server-emitted filesystem paths/URLs directly here - the gallery
/// MCP server resolves `gallery://`-style refs to absolute paths before
/// emitting a ```media display_hint, so the widget never needs the gallery
/// SQLite store (the earlier `GalleryMediaStorage` widget-side resolver was
/// removed as the abandoned alternative architecture).
pub trait MediaStorage: Send + Sync {
    fn resolve(&self, reference: &MediaRef) -> anyhow::Result<ResolvedMedia>;
}

/// The widget's `MediaStorage`: resolves filesystem paths, data URIs, and
/// URLs directly. Gallery images reach the widget as filesystem paths
/// (`absolute_path`) in server-emitted `display_hint` media blocks.
pub struct PathMediaStorage;

impl MediaStorage for PathMediaStorage {
    fn resolve(&self, reference: &MediaRef) -> anyhow::Result<ResolvedMedia> {
        let src = reference.src();
        let kind = reference.kind().unwrap_or(MediaKind::Image);

        if src.starts_with("data:") {
            // Data URI — the bytes are inline. The widget handles decoding.
            Ok(ResolvedMedia {
                kind,
                path: None,
                bytes: None,
                url: Some(SharedString::from(src)),
            })
        } else if src.starts_with("http://") || src.starts_with("https://") {
            // Remote URL — the widget loads via the image resolver.
            Ok(ResolvedMedia {
                kind,
                path: None,
                bytes: None,
                url: Some(SharedString::from(src)),
            })
        } else {
            // Filesystem path — check it exists.
            let path = PathBuf::from(src);
            if !path.exists() {
                return Err(anyhow::anyhow!("media file not found: {src}"));
            }
            Ok(ResolvedMedia {
                kind,
                path: Some(path),
                bytes: None,
                url: None,
            })
        }
    }
}
