//! Media reference types — how assets are identified and resolved.

use base64::Engine as _;
use gpui::SharedString;
use hkask_types::BlockProvenance;
use serde::Deserialize;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
    sync::OnceLock,
};
use url::{Host, Url};

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
pub struct MediaRef {
    src: SharedString,
    kind: MediaKind,
}

impl MediaRef {
    /// Create a new media reference.
    pub fn new(src: SharedString, kind: MediaKind) -> Self {
        Self { src, kind }
    }

    /// The source URL/path/data-URI.
    pub fn src(&self) -> &str {
        self.src.as_ref()
    }

    /// The media kind.
    pub fn kind(&self) -> MediaKind {
        self.kind
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

/// Maximum decoded inline payload admitted from an assistant-authored media block.
pub const MAX_INLINE_MEDIA_BYTES: usize = 32 * 1024 * 1024;

/// The widget's locator-validation boundary. Local reads are confined to
/// canonical approved roots; inline payloads and remote URL syntax are checked
/// before a decoder or subprocess receives them.
pub struct PathMediaStorage {
    allowed_roots: Vec<PathBuf>,
}

static APPROVED_GALLERY_FILES: OnceLock<parking_lot::RwLock<HashSet<PathBuf>>> = OnceLock::new();

/// Register one exact path observed through a successful gallery tool result.
/// Assistant-authored media-block fields never call this authority boundary.
pub fn approve_gallery_media_path(path: &Path) -> anyhow::Result<()> {
    let path = path.canonicalize().map_err(|error| {
        anyhow::anyhow!(
            "canonicalize approved gallery media {}: {error}",
            path.display()
        )
    })?;
    if !path.is_file() {
        return Err(anyhow::anyhow!(
            "approved gallery media is not a file: {}",
            path.display()
        ));
    }
    APPROVED_GALLERY_FILES
        .get_or_init(Default::default)
        .write()
        .insert(path);
    Ok(())
}

impl Default for PathMediaStorage {
    fn default() -> Self {
        let artifacts = hkask_types::agent_paths::resolve_artifacts_dir();
        let allowed_roots = artifacts.canonicalize().into_iter().collect::<Vec<_>>();
        #[cfg(test)]
        let mut allowed_roots = allowed_roots;
        #[cfg(test)]
        if let Ok(test_data) = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .canonicalize()
        {
            allowed_roots.push(test_data);
        }
        Self { allowed_roots }
    }
}

impl PathMediaStorage {
    #[cfg(test)]
    fn with_allowed_roots<'a>(roots: impl IntoIterator<Item = &'a Path>) -> anyhow::Result<Self> {
        let allowed_roots = roots
            .into_iter()
            .map(|root| {
                root.canonicalize().map_err(|error| {
                    anyhow::anyhow!(
                        "canonicalize approved media root {}: {error}",
                        root.display()
                    )
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self { allowed_roots })
    }

    fn resolve_local(&self, source: &str, kind: MediaKind) -> anyhow::Result<ResolvedMedia> {
        let path = if source.starts_with("file:") {
            let url = Url::parse(source)
                .map_err(|error| anyhow::anyhow!("invalid media file URL: {error}"))?;
            url.to_file_path()
                .map_err(|()| anyhow::anyhow!("media file URL must identify a local path"))?
        } else {
            PathBuf::from(source)
        };
        let path = path
            .canonicalize()
            .map_err(|error| anyhow::anyhow!("media file not found: {source}: {error}"))?;
        if !path.is_file() {
            return Err(anyhow::anyhow!(
                "media path is not a regular file: {}",
                path.display()
            ));
        }
        let gallery_approved = APPROVED_GALLERY_FILES
            .get_or_init(Default::default)
            .read()
            .contains(&path);
        if !gallery_approved && !self.allowed_roots.iter().any(|root| path.starts_with(root)) {
            return Err(anyhow::anyhow!(
                "media path is outside approved media roots: {}",
                path.display()
            ));
        }
        if matches!(kind, MediaKind::Image | MediaKind::Svg) {
            let metadata = std::fs::metadata(&path)?;
            if metadata.len() > MAX_INLINE_MEDIA_BYTES as u64 {
                return Err(anyhow::anyhow!(
                    "media image exceeds {MAX_INLINE_MEDIA_BYTES} bytes"
                ));
            }
            let bytes = std::fs::read(&path)?;
            if bytes.len() > MAX_INLINE_MEDIA_BYTES {
                return Err(anyhow::anyhow!(
                    "media image exceeds {MAX_INLINE_MEDIA_BYTES} bytes"
                ));
            }
            return Ok(ResolvedMedia {
                kind,
                path: None,
                bytes: Some(bytes),
                url: None,
            });
        }
        Ok(ResolvedMedia {
            kind,
            path: Some(path),
            bytes: None,
            url: None,
        })
    }
}

impl MediaStorage for PathMediaStorage {
    fn resolve(&self, reference: &MediaRef) -> anyhow::Result<ResolvedMedia> {
        let src = reference.src();
        let kind = reference.kind();

        if src.starts_with("data:") {
            let bytes = decode_data_uri(src, kind)?;
            Ok(ResolvedMedia {
                kind,
                path: None,
                bytes: Some(bytes),
                url: None,
            })
        } else if src.starts_with("http:") || src.starts_with("https:") {
            validate_remote_url_with_addresses(src, &[])?;
            Ok(ResolvedMedia {
                kind,
                path: None,
                bytes: None,
                url: Some(SharedString::from(src)),
            })
        } else {
            self.resolve_local(src, kind)
        }
    }
}

fn decode_data_uri(source: &str, kind: MediaKind) -> anyhow::Result<Vec<u8>> {
    let (metadata, encoded) = source
        .strip_prefix("data:")
        .and_then(|body| body.split_once(','))
        .ok_or_else(|| anyhow::anyhow!("invalid media data URI"))?;
    let (mime, encoding) = metadata
        .split_once(';')
        .ok_or_else(|| anyhow::anyhow!("media data URI must declare base64 encoding"))?;
    if encoding != "base64" {
        return Err(anyhow::anyhow!("media data URI must use base64 encoding"));
    }
    let mime_matches = match kind {
        MediaKind::Image => mime.starts_with("image/") && mime != "image/svg+xml",
        MediaKind::Svg => mime == "image/svg+xml",
        MediaKind::Audio => mime.starts_with("audio/"),
        MediaKind::Video => mime.starts_with("video/"),
    };
    if !mime_matches {
        return Err(anyhow::anyhow!(
            "media data URI MIME type does not match its kind"
        ));
    }
    let decoded_upper_bound = encoded.len().saturating_add(3) / 4 * 3;
    if decoded_upper_bound > MAX_INLINE_MEDIA_BYTES {
        return Err(anyhow::anyhow!(
            "media data URI decoded size exceeds {MAX_INLINE_MEDIA_BYTES} bytes"
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| anyhow::anyhow!("invalid media data URI base64: {error}"))?;
    if bytes.len() > MAX_INLINE_MEDIA_BYTES {
        return Err(anyhow::anyhow!(
            "media data URI decoded size exceeds {MAX_INLINE_MEDIA_BYTES} bytes"
        ));
    }
    Ok(bytes)
}

pub(crate) fn validate_remote_url_with_addresses(
    source: &str,
    resolved_addresses: &[IpAddr],
) -> anyhow::Result<Url> {
    let url = Url::parse(source).map_err(|error| anyhow::anyhow!("invalid media URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(anyhow::anyhow!("media URL scheme must be http or https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(anyhow::anyhow!(
            "media URL must not contain embedded credentials"
        ));
    }
    let host = url
        .host()
        .ok_or_else(|| anyhow::anyhow!("media URL must include a host"))?;
    match host {
        Host::Ipv4(address) if !ipv4_is_public(address) => {
            return Err(anyhow::anyhow!(
                "media URL targets a non-public IPv4 address"
            ));
        }
        Host::Ipv6(address) if !ipv6_is_public(address) => {
            return Err(anyhow::anyhow!(
                "media URL targets a non-public IPv6 address"
            ));
        }
        _ => {}
    }
    if resolved_addresses.iter().any(|address| match address {
        IpAddr::V4(address) => !ipv4_is_public(*address),
        IpAddr::V6(address) => !ipv6_is_public(*address),
    }) {
        return Err(anyhow::anyhow!(
            "media URL DNS resolved to a non-public address"
        ));
    }
    Ok(url)
}

fn ipv4_is_public(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    !(address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_multicast()
        || first == 0
        || first >= 240
        || (first == 100 && (64..=127).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 192 && second == 0 && third == 2)
        || (first == 198 && (second == 18 || second == 19))
        || (first == 198 && second == 51 && third == 100)
        || (first == 203 && second == 0 && third == 113))
}

fn ipv6_is_public(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return ipv4_is_public(mapped);
    }
    let segments = address.segments();
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] & 0xffc0) == 0xfec0
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
}

/// The parsed body of a ```` ```media ```` block. Carries the untrusted media
/// locator plus optional ontology and provenance metadata. Neither metadata
/// field grants filesystem or network authority.
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
    /// Stable gallery Asset identity when the publisher indexed this media.
    #[serde(default)]
    pub gallery_asset_id: Option<String>,
    /// Ontology concept URI (e.g. `omc:CreativeWork`, `fibo:Corporation`,
    /// `pko:Step`). Drives the "Explain" affordance's tool selection (the
    /// "I" pattern — ontology-bounded affordances). `None` on older blocks
    /// → the widget falls back to the default explain tool.
    #[serde(default)]
    pub ontology: Option<String>,
    /// Model-visible provenance metadata for re-issuing the originating tool
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

#[cfg(test)]
mod locator_policy_tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn image_ref(src: &str) -> MediaRef {
        MediaRef::new(SharedString::from(src), MediaKind::Image)
    }

    /// expect: Local media is readable only from an explicitly approved root.
    /// [P1] Motivating: assistant-authored media blocks cannot read arbitrary local files.
    /// pre: one file is inside an approved root and one is outside it.
    /// post: the contained file resolves and the outside file is rejected.
    #[test]
    fn local_paths_are_contained_by_approved_roots() -> anyhow::Result<()> {
        let approved = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let inside_path = approved.path().join("inside.png");
        let outside_path = outside.path().join("outside.png");
        std::fs::write(&inside_path, b"inside")?;
        std::fs::write(&outside_path, b"outside")?;
        let storage = PathMediaStorage::with_allowed_roots([approved.path()])?;

        assert!(
            storage
                .resolve(&image_ref(&inside_path.to_string_lossy()))
                .is_ok()
        );
        let error = storage
            .resolve(&image_ref(&outside_path.to_string_lossy()))
            .expect_err("outside path must be rejected");
        assert!(error.to_string().contains("approved media roots"));
        Ok(())
    }

    #[test]
    fn gallery_result_can_approve_one_exact_external_media_path() -> anyhow::Result<()> {
        let gallery = tempfile::tempdir()?;
        let path = gallery.path().join("gallery.png");
        std::fs::write(&path, b"gallery")?;
        let reference = image_ref(&path.to_string_lossy());
        let storage = PathMediaStorage::default();
        assert!(storage.resolve(&reference).is_err());
        approve_gallery_media_path(&path)?;
        assert!(storage.resolve(&reference).is_ok());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn local_symlink_escape_is_rejected() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let approved = tempfile::tempdir()?;
        let outside = tempfile::tempdir()?;
        let outside_path = outside.path().join("outside.png");
        std::fs::write(&outside_path, b"outside")?;
        let link = approved.path().join("escape.png");
        symlink(&outside_path, &link)?;
        let storage = PathMediaStorage::with_allowed_roots([approved.path()])?;

        assert!(
            storage
                .resolve(&image_ref(&link.to_string_lossy()))
                .is_err()
        );
        Ok(())
    }

    /// expect: Inline media has a decoded-size ceiling before allocation reaches a loader.
    /// [P1] Motivating: assistant-authored blocks cannot force unbounded inline decoding.
    /// pre: the data URI decodes beyond MAX_INLINE_MEDIA_BYTES.
    /// post: resolution rejects it with a size error.
    #[test]
    fn oversized_data_uri_is_rejected() -> anyhow::Result<()> {
        use base64::Engine as _;

        let bytes = vec![0_u8; MAX_INLINE_MEDIA_BYTES + 1];
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let storage = PathMediaStorage::with_allowed_roots(std::iter::empty::<&std::path::Path>())?;
        let error = storage
            .resolve(&image_ref(&format!("data:image/png;base64,{encoded}")))
            .expect_err("oversized data URI must be rejected");
        assert!(error.to_string().contains("decoded size"));
        Ok(())
    }

    #[test]
    fn public_remote_urls_are_admitted_and_private_literals_are_rejected() {
        let public = "https://example.com/media/video.mp4";
        assert!(
            validate_remote_url_with_addresses(public, &[IpAddr::from([93, 184, 216, 34])]).is_ok()
        );

        for url in [
            "http://127.0.0.1/media.mp4",
            "http://10.0.0.1/media.mp4",
            "http://169.254.1.1/media.mp4",
            "http://100.64.0.1/media.mp4",
            "http://198.18.0.1/media.mp4",
            "http://192.0.2.1/media.mp4",
            "http://[::1]/media.mp4",
            "http://[2001:db8::1]/media.mp4",
            "http://[::ffff:192.168.1.1]/media.mp4",
            "https://user:secret@example.com/media.mp4",
        ] {
            assert!(
                validate_remote_url_with_addresses(url, &[]).is_err(),
                "unsafe URL admitted: {url}"
            );
        }
    }

    /// expect: A public hostname that DNS maps to a private address is rejected.
    /// [P1] Motivating: hostname spelling cannot bypass private-network isolation.
    /// pre: the parsed public URL has a private resolved address.
    /// post: validation rejects the resolved destination.
    #[test]
    fn dns_resolving_to_private_addresses_is_rejected() {
        let private = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));
        let link_local = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
        assert!(
            validate_remote_url_with_addresses("https://media.example/video.mp4", &[private])
                .is_err()
        );
        assert!(
            validate_remote_url_with_addresses("https://media.example/video.mp4", &[link_local])
                .is_err()
        );
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
        assert_eq!(reference.kind(), MediaKind::Video);
    }

    #[test]
    fn to_media_ref_rejects_unknown_kind() {
        let body = MediaBlockBody::parse(r##"{"kind":"hologram","src":"/x"}"##).unwrap();
        assert!(body.to_media_ref().is_err());
    }
}
