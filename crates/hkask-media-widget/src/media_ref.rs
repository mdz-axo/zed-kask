//! Media reference types — how assets are identified and resolved.

use gpui::SharedString;
use std::path::PathBuf;
use std::sync::Arc;

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

/// Simple `MediaStorage` that resolves filesystem paths, data URIs, and URLs
/// directly — no gallery lookup. This is the default storage for the media
/// widget when no gallery context is available.
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

/// `MediaStorage` backed by the hkask gallery SQLite store.
///
/// Resolves `gallery://<gallery_id>/<index>` URIs to the filesystem
/// `absolute_path` stored in the gallery database. Falls back to direct
/// path/data-URI/URL resolution for non-gallery sources.
pub struct GalleryMediaStorage {
    gallery_store: Arc<hkask_storage::GalleryStore>,
}

impl GalleryMediaStorage {
    pub fn new(gallery_store: Arc<hkask_storage::GalleryStore>) -> Self {
        Self { gallery_store }
    }

    /// Parse a `gallery://<gallery_id>/<index>` URI and look up the
    /// image record to get its filesystem `absolute_path`.
    fn resolve_gallery_uri(&self, gallery_id: &str, index: usize) -> anyhow::Result<ResolvedMedia> {
        let image = self
            .gallery_store
            .get_image(gallery_id, Some(index), None)
            .map_err(|error| anyhow::anyhow!("gallery lookup failed: {error}"))?;
        let path = PathBuf::from(&image.absolute_path);
        if !path.exists() {
            return Err(anyhow::anyhow!(
                "gallery image file not found: {}",
                path.display()
            ));
        }
        let kind = detect_kind(&image.format);
        Ok(ResolvedMedia {
            kind,
            path: Some(path),
            bytes: None,
            url: None,
        })
    }
}

impl MediaStorage for GalleryMediaStorage {
    fn resolve(&self, reference: &MediaRef) -> anyhow::Result<ResolvedMedia> {
        let source = reference.src();

        if let Some(rest) = source.strip_prefix("gallery://") {
            let (gallery_id, index_str) = rest.rsplit_once('/').ok_or_else(|| {
                anyhow::anyhow!("invalid gallery URI: expected gallery://<id>/<index>")
            })?;
            let index = index_str
                .parse::<usize>()
                .map_err(|_| anyhow::anyhow!("invalid gallery index: {index_str}"))?;
            return self.resolve_gallery_uri(gallery_id, index);
        }

        // Non-gallery sources resolve the same as PathMediaStorage.
        PathMediaStorage.resolve(reference)
    }
}

/// Detect `MediaKind` from a file extension or data-URI MIME type.
pub fn detect_kind(src: &str) -> MediaKind {
    if let Some(mime) = src.strip_prefix("data:")
        && let Some((mime_type, _)) = mime.split_once(',')
    {
        return match mime_type.split(';').next().unwrap_or("") {
            "image/svg+xml" => MediaKind::Svg,
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp"
            | "image/tiff" => MediaKind::Image,
            "audio/wav" | "audio/mpeg" | "audio/ogg" | "audio/flac" => MediaKind::Audio,
            "video/mp4" | "video/webm" | "video/x-matroska" => MediaKind::Video,
            _ => MediaKind::Image,
        };
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
