//! Media reference types — how assets are identified and resolved.

use gpui::SharedString;
use hkask_tool_invoker::BlockProvenance;
use serde::Deserialize;
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
        } else if let Some(path_str) = src.strip_prefix("file://") {
            // file:// URL — resolve to the underlying filesystem path so the
            // widget's local-file branches (image read, VideoPlayer::open)
            // handle it. Without this, `file://` falls into the plain-path
            // branch where `PathBuf::from("file://...").exists()` fails.
            let path = PathBuf::from(path_str);
            if !path.exists() {
                return Err(anyhow::anyhow!("media file not found: {src}"));
            }
            Ok(ResolvedMedia {
                kind,
                path: Some(path),
                bytes: None,
                url: None,
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

/// The parsed body of a ```` ```media ```` block. Carries the media reference
/// plus optional ontology concept tag and server-authoritative provenance.
///
/// `ontology` and `provenance` are `#[serde(default)]` so existing blocks without
/// them still parse and render — just without the ontology-driven "Explain" and
/// "I disagree" affordances. This is the additive contract: the media widget
/// gains affordances when the block carries ontology + provenance, and falls back
/// to transport-only display when it doesn't.
#[derive(Debug, Clone, Deserialize)]
pub struct MediaBlockBody {
    /// Media kind discriminator ("image", "svg", "audio", "video").
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Source URL/path/data-URI.
    pub src: String,
    /// Ontology concept URI (e.g. `omc:CreativeWork`, `fibo:Corporation`,
    /// `pko:Step`). Drives the "Explain" affordance's tool selection (the
    /// "I" pattern — ontology-bounded affordances). `None` on older blocks
    /// → the widget falls back to the default explain tool.
    #[serde(default)]
    pub ontology: Option<String>,
    /// Server-authoritative provenance for re-issuing the originating tool
    /// (Explain) or composing a revision request (I disagree). `None` on
    /// older blocks → the widget renders without dispatch/compose-back
    /// affordances.
    #[serde(default)]
    pub provenance: BlockProvenance,
}

fn default_kind() -> String {
    "image".to_string()
}

/// Whether a parse failure is merely truncated JSON — the block body is still
/// streaming in. Streaming re-renders re-parse the partial body on every
/// delta, so an EOF must not be logged as a malformed block; only a complete
/// body with a real syntax error is warn-worthy.
pub fn is_truncated_json(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<serde_json::Error>())
        .any(|json_error| json_error.classify() == serde_json::error::Category::Eof)
}

#[cfg(test)]
mod truncate_tests {
    use super::*;

    // Pins the streaming gate: a body still streaming in (truncated JSON)
    // must classify as truncated so the render path stays silent, while a
    // complete body with a real syntax error must not — that one warns.
    #[test]
    fn truncated_body_classifies_as_streaming() {
        let error = MediaBlockBody::parse(r#"{"kind":"image","src":"/tmp/a.jpg"#).unwrap_err();
        assert!(is_truncated_json(&error));
    }

    #[test]
    fn syntax_error_does_not_classify_as_streaming() {
        let error = MediaBlockBody::parse(r#"{"kind": }"#).unwrap_err();
        assert!(!is_truncated_json(&error));
    }

    #[test]
    fn complete_body_parses() {
        let block = MediaBlockBody::parse(r#"{"kind":"image","src":"/tmp/a.jpg"}"#)
            .expect("complete body parses");
        assert_eq!(block.src, "/tmp/a.jpg");
    }
}

impl MediaBlockBody {
    /// Parse a ```` ```media ```` block body. Tolerant: missing `kind` defaults
    /// to `"image"`; missing `ontology`/`provenance` default to `None`/empty so
    /// older blocks still parse and render without the new affordances.
    pub fn parse(body: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(body.trim())?)
    }

    /// Resolve to a `MediaRef` for the widget's media loader.
    pub fn to_media_ref(&self) -> anyhow::Result<MediaRef> {
        let kind = match self.kind.as_str() {
            "image" | "img" => MediaKind::Image,
            "svg" => MediaKind::Svg,
            "audio" => MediaKind::Audio,
            "video" => MediaKind::Video,
            other => {
                return Err(anyhow::anyhow!(
                    "unknown media kind '{other}' — expected image, svg, audio, or video"
                ));
            }
        };
        Ok(MediaRef::new(SharedString::from(self.src.as_str()), kind))
    }
}

#[cfg(test)]
mod block_body_tests {
    use super::*;

    #[test]
    fn parses_minimal_body_with_only_kind_and_src() {
        let body = MediaBlockBody::parse(r##"{"kind":"image","src":"/a.png"}"##)
            .expect("minimal body parses");
        assert_eq!(body.kind, "image");
        assert_eq!(body.src, "/a.png");
        assert!(body.ontology.is_none());
        assert!(!body.provenance.is_dispatchable());
    }

    #[test]
    fn parses_body_with_ontology_and_provenance() {
        let json = r##"{"kind":"image","src":"/a.png","ontology":"omc:CreativeWork","provenance":{"tool":"generate_image","server":"hkask-mcp-media","args":{"prompt":"a cat"}}}"##;
        let body = MediaBlockBody::parse(json).expect("full body parses");
        assert_eq!(body.ontology.as_deref(), Some("omc:CreativeWork"));
        assert!(body.provenance.is_dispatchable());
        assert_eq!(body.provenance.tool.as_deref(), Some("generate_image"));
    }

    #[test]
    fn parses_body_with_default_kind_when_absent() {
        let body = MediaBlockBody::parse(r##"{"src":"/a.png"}"##).expect("parses");
        assert_eq!(body.kind, "image");
    }

    #[test]
    fn to_media_ref_resolves_kind_and_src() {
        let body = MediaBlockBody::parse(r##"{"kind":"video","src":"/c.mp4"}"##).unwrap();
        let reference = body.to_media_ref().expect("resolves");
        assert_eq!(reference.src(), "/c.mp4");
        assert_eq!(reference.kind(), Some(MediaKind::Video));
    }

    #[test]
    fn to_media_ref_rejects_unknown_kind() {
        let body = MediaBlockBody::parse(r##"{"kind":"hologram","src":"/x"}"##).unwrap();
        assert!(body.to_media_ref().is_err());
    }
}
