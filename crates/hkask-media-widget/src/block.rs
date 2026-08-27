//! The ```` ```media ```` block body model + parser.
//!
//! The block body is a JSON object emitted by the `hkask-mcp-media` server's
//! `media_block` helper. The `viz` discriminator is `"media"`; the `kind`
//! field selects the rendering mode (image, video, audio, svg).

use serde::Deserialize;

/// The discriminator-tagged body of a ```` ```media ```` block.
///
/// `viz` selects the renderer (`"media"`). `kind` selects the media type
/// (image, video, audio, svg). `src` is the asset URL or file path.
/// `ontology` and `provenance` are optional fields baked in by the media
/// server for OMC-driven explain affordances.
#[derive(Debug, Clone, Deserialize)]
pub struct MediaBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub ontology: Option<String>,
    #[serde(default)]
    pub provenance: Option<ProvenanceBody>,
}

/// Provenance metadata baked into the block body by the media server.
#[derive(Debug, Clone, Deserialize)]
pub struct ProvenanceBody {
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub span_id: Option<String>,
}

/// Parse a ```` ```media ```` block body.
///
/// Tolerant: foreign-shaped JSON parses without error (defaulting to an
/// empty `viz`) so it is rejected by the `VIZ_TAG` check rather than logged
/// as malformed.
pub fn parse_media_body(body: &str) -> anyhow::Result<MediaBlockBody> {
    Ok(serde_json::from_str(body.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_image_block() {
        let body = r#"{"viz":"media","kind":"image","src":"/tmp/img.png"}"#;
        let parsed = parse_media_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("media"));
        assert_eq!(parsed.kind.as_deref(), Some("image"));
        assert_eq!(parsed.src.as_deref(), Some("/tmp/img.png"));
    }

    #[test]
    fn parses_video_block_with_provenance() {
        let body = r#"{"viz":"media","kind":"video","src":"/tmp/clip.mp4","ontology":"omc:Sequence","provenance":{"tool":"generate_video","server":"hkask-mcp-media"}}"#;
        let parsed = parse_media_body(body).expect("valid body parses");
        assert_eq!(parsed.kind.as_deref(), Some("video"));
        assert_eq!(parsed.ontology.as_deref(), Some("omc:Sequence"));
        assert_eq!(
            parsed.provenance.as_ref().and_then(|p| p.tool.as_deref()),
            Some("generate_video")
        );
    }

    #[test]
    fn parses_audio_block() {
        let body = r#"{"viz":"media","kind":"audio","src":"data:audio/mp3;base64,SUQzBAAAAAA"}"#;
        let parsed = parse_media_body(body).expect("valid body parses");
        assert_eq!(parsed.kind.as_deref(), Some("audio"));
        assert!(parsed.src.as_deref().unwrap().starts_with("data:audio"));
    }

    #[test]
    fn falls_through_non_media_bodies() {
        let graph = r#"{"viz":"event_tree","nodes":[]}"#;
        let parsed = parse_media_body(graph).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("media"));

        assert!(parse_media_body("not json").is_err());
    }

    #[test]
    fn tolerates_missing_fields() {
        let body = r#"{"kind":"image"}"#;
        let parsed = parse_media_body(body).expect("partial body parses");
        assert_eq!(parsed.kind.as_deref(), Some("image"));
        assert!(parsed.viz.is_none());
        assert!(parsed.src.is_none());
    }
}
