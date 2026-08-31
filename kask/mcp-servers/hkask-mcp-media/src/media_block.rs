//! Helper for formatting ```media fenced blocks in tool responses.
//!
//! When a media tool (generate_image, generate_video, etc.) returns a result,
//! it includes a `display_hint` field containing a pre-formatted ```media
//! markdown block. The model can copy this into its reply, and the D18 seam
//! in `crates/markdown` will render it as a `MediaWidget`.
//!
//! The block body carries OMC concept tags + server-authoritative provenance
//! (tool, server, args, span_id) so the media widget can dispatch the
//! OMC-driven "Explain" affordance and compose-back the "I disagree" gesture.
//! Both fields are additive (`#[serde(default)]` on the widget side) —
//! existing blocks without them still parse and render, just without the
//! new affordances.

use crate::omc;

/// Format a ```media fenced block for inclusion in tool responses.
///
/// The block body is a JSON object: `{"kind","src","ontology","provenance"}`.
/// `ontology` and `provenance` are optional — older blocks without them still
/// parse (the widget uses `#[serde(default)]`).
///
/// ```text
/// ```media
/// {"kind":"image","src":"/path/to/image.png","ontology":"omc:CreativeWork","provenance":{"tool":"generate_image","server":"hkask-mcp-media","args":{}}}
/// ```
/// ```
pub fn media_block(kind: &str, src: &str) -> String {
    format!("```media\n{{\"kind\":\"{kind}\",\"src\":\"{src}\"}}\n```")
}

/// Format a ```media block with an OMC concept tag and provenance.
///
/// `omc` is the OMC concept URI (e.g. `omc:CreativeWork`) — `None` omits the
/// `ontology` field (the widget falls back to its default explain tool). `provenance` is
/// the server-authoritative record of which tool produced this artifact, with
/// which args, under which regulation span — `None` omits the field (the widget
/// renders without dispatch/compose-back affordances).
pub fn media_block_with_omc(
    kind: &str,
    src: &str,
    omc: Option<&str>,
    provenance: Option<&Provenance>,
) -> String {
    let mut body = format!("{{\"kind\":\"{kind}\",\"src\":\"{src}\"");
    if let Some(omc) = omc {
        body.push_str(&format!(",\"ontology\":\"{omc}\""));
    }
    if let Some(prov) = provenance {
        body.push_str(",\"provenance\":");
        body.push_str(&serde_json::to_string(prov).unwrap_or_else(|_| "null".into()));
    }
    body.push('}');
    format!("```media\n{body}\n```")
}

/// Server-authoritative provenance baked into a ```media block body so the
/// media widget can re-issue the originating tool (Explain) or compose a
/// revision request (I disagree). Mirrors `hkask_tool_invoker::BlockProvenance`
/// but lives in the MCP server crate (no GPUI dependency) so the server can
/// serialize it into the block body.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Provenance {
    /// The MCP tool name that produced this block (e.g. `"generate_image"`).
    pub tool: String,
    /// The MCP server name (always `"hkask-mcp-media"` for this server).
    pub server: String,
    /// The args the tool was invoked with, as a JSON object.
    pub args: serde_json::Value,
    /// The `reg.*` span id under which the producing tool call was traced.
    pub span_id: Option<String>,
}

impl Provenance {
    /// Build provenance for a tool invocation. The server name is fixed to
    /// `hkask-mcp-media`; the caller supplies the tool name, args, and span.
    pub fn for_tool(tool: &str, args: serde_json::Value, span_id: Option<String>) -> Self {
        Self {
            tool: tool.to_string(),
            server: "hkask-mcp-media".to_string(),
            args,
            span_id,
        }
    }
}

/// Format a ```media block for an image asset.
pub fn image_block(src: &str) -> String {
    media_block("image", src)
}

/// Build an OMC-tagged, provenance-carrying display hint for a tool output,
/// then attach it to the result as `display_hint`.
///
/// This is the OMC-aware enrichment path. It:
/// 1. Resolves the OMC concept for `tool` via `omc::tool_to_omc`.
/// 2. Extracts the asset src from the result (via `extract_src`).
/// 3. Formats a ```media block carrying `kind`, `src`, `omc`, `provenance`.
/// 4. Attaches the block to the result as `display_hint`.
///
/// `tool` is the MCP tool name (drives the OMC tag). `kind` is the media
/// kind ("image"/"video"/"audio"). `args` is the JSON args the tool was
/// invoked with (baked into provenance). `span_id` is the regulation span.
/// Returns the enriched result (unchanged if no src could be extracted).
pub fn enrich_with_omc_and_provenance(
    mut result: serde_json::Value,
    tool: &str,
    kind: &str,
    args: serde_json::Value,
    span_id: Option<String>,
) -> serde_json::Value {
    if let Some(src) = extract_src(&result, kind) {
        let omc = omc::tool_to_omc(tool);
        let provenance = Provenance::for_tool(tool, args, span_id);
        let hint = media_block_with_omc(kind, &src, omc, Some(&provenance));
        result["display_hint"] = serde_json::Value::String(hint);
    }
    result
}

/// Extract the asset src (a persisted file path) from a tool result,
/// dispatching on the media kind. Media tools compose their results via
/// `persist_and_slim_result` (assets.rs), so the `output` field carries the
/// persisted path for every kind; the audio arm additionally accepts the
/// `audio_path` field produced by the record-and-transcribe tools.
pub fn extract_src(result: &serde_json::Value, kind: &str) -> Option<String> {
    match kind {
        "audio" => result
            .get("audio_path")
            .and_then(|path| path.as_str())
            .map(str::to_string)
            .or_else(|| {
                result
                    .get("output")
                    .and_then(|output| output.as_str())
                    .map(str::to_string)
            }),
        _ => result
            .get("output")
            .and_then(|output| output.as_str())
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_block_format() {
        let block = media_block("image", "/path/to/img.png");
        assert_eq!(
            block,
            "```media\n{\"kind\":\"image\",\"src\":\"/path/to/img.png\"}\n```"
        );
    }

    #[test]
    fn test_media_block_with_omc_includes_tag_and_provenance() {
        let prov = Provenance::for_tool(
            "generate_image",
            serde_json::json!({"prompt": "a cat"}),
            Some("span-1".into()),
        );
        let block = media_block_with_omc(
            "image",
            "/tmp/img.png",
            Some("omc:CreativeWork"),
            Some(&prov),
        );
        assert!(block.contains("\"kind\":\"image\""));
        assert!(block.contains("/tmp/img.png"));
        assert!(block.contains("\"ontology\":\"omc:CreativeWork\""));
        assert!(block.contains("\"tool\":\"generate_image\""));
        assert!(block.contains("\"server\":\"hkask-mcp-media\""));
        assert!(block.contains("\"span_id\":\"span-1\""));
    }

    #[test]
    fn test_media_block_with_omc_omits_optional_fields_when_none() {
        let block = media_block_with_omc("image", "/tmp/img.png", None, None);
        assert!(block.contains("\"kind\":\"image\""));
        assert!(block.contains("/tmp/img.png"));
        assert!(!block.contains("\"ontology\""));
        assert!(!block.contains("\"provenance\""));
    }

    #[test]
    fn test_enrich_with_omc_and_provenance_generate_image() {
        // The slim result shape every media tool returns after
        // persist_and_slim_result: `output` carries the persisted path.
        let result = serde_json::json!({"output": "/tmp/img.png"});
        let enriched = enrich_with_omc_and_provenance(
            result,
            "generate_image",
            "image",
            serde_json::json!({"prompt": "a cat"}),
            None,
        );
        let hint = enriched["display_hint"].as_str().expect("hint attached");
        assert!(hint.contains("\"ontology\":\"omc:CreativeWork\""));
        assert!(hint.contains("\"tool\":\"generate_image\""));
        assert!(hint.contains("\"kind\":\"image\""));
    }

    #[test]
    fn test_enrich_with_omc_and_provenance_transform_maps_to_version() {
        let result = serde_json::json!({"output": "/tmp/out.png"});
        let enriched = enrich_with_omc_and_provenance(
            result,
            "transform_image",
            "image",
            serde_json::json!({}),
            None,
        );
        let hint = enriched["display_hint"].as_str().expect("hint attached");
        assert!(hint.contains("\"ontology\":\"omc:VersionInfo\""));
    }

    #[test]
    fn test_enrich_with_omc_and_provenance_audio_uses_audio_path() {
        let result = serde_json::json!({"audio_path": "/tmp/rec.wav"});
        let enriched = enrich_with_omc_and_provenance(
            result,
            "record_and_transcribe",
            "audio",
            serde_json::json!({}),
            None,
        );
        let hint = enriched["display_hint"].as_str().expect("hint attached");
        assert!(hint.contains("\"kind\":\"audio\""));
        assert!(hint.contains("/tmp/rec.wav"));
        assert!(hint.contains("\"ontology\":\"omc:Capture\""));
    }

    #[test]
    fn test_enrich_with_omc_and_provenance_no_src_leaves_result_unchanged() {
        let result = serde_json::json!({"status": "ok"});
        let enriched = enrich_with_omc_and_provenance(
            result,
            "generate_image",
            "image",
            serde_json::json!({}),
            None,
        );
        assert!(enriched.get("display_hint").is_none());
    }

    #[test]
    fn test_enrich_with_omc_unknown_tool_omits_omc_field() {
        // A tool not in the OMC map still gets provenance, just no `omc` tag.
        let result = serde_json::json!({"output": "/tmp/x.png"});
        let enriched = enrich_with_omc_and_provenance(
            result,
            "some_unknown_tool",
            "image",
            serde_json::json!({}),
            None,
        );
        let hint = enriched["display_hint"].as_str().expect("hint attached");
        assert!(hint.contains("\"tool\":\"some_unknown_tool\""));
        assert!(!hint.contains("\"ontology\""));
    }

    #[test]
    fn test_extract_src_image_from_output_field() {
        let result = serde_json::json!({"output": "/tmp/b.png"});
        assert_eq!(extract_src(&result, "image"), Some("/tmp/b.png".into()));
    }

    #[test]
    fn test_extract_src_audio_from_audio_path() {
        let result = serde_json::json!({"audio_path": "/tmp/c.wav"});
        assert_eq!(extract_src(&result, "audio"), Some("/tmp/c.wav".into()));
    }

    // The audio arm must also read `output` — generate_speech composes its
    // slim result through persist_and_slim_result, whose path field is
    // `output` (same as every other kind). Without this fallback the speech
    // display hint never attached.
    #[test]
    fn test_extract_src_audio_falls_back_to_output_field() {
        let result = serde_json::json!({"output": "/tmp/speech.mp3"});
        assert_eq!(
            extract_src(&result, "audio"),
            Some("/tmp/speech.mp3".into())
        );
    }
}
