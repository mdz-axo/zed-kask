//! Shared MCP media wire contracts — tool-name inventory and request DTOs
//! shared between `hkask-mcp-media` and `media_panel` so the panel does not
//! link the server crate (`PRINCIPLES.md:P12.1`).
//!
//! `hkask-mcp-media`'s `tool_surface_tests` pins `MEDIA_TOOL_NAMES` against
//! its build-generated `TOOL_NAMES` and live rmcp router so a rename, addition,
//! or removal cannot silently drift.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical sorted list of every tool exposed by `hkask-mcp-media`.
/// Pinned against `hkask_mcp_media::TOOL_NAMES` in `hkask-mcp-media`'s tests.
pub const MEDIA_TOOL_NAMES: &[&str] = &[
    "audio_capture",
    "audio_concat",
    "audio_trim",
    "describe_image",
    "educt_annotation_create",
    "educt_annotation_delete",
    "educt_annotation_list",
    "educt_annotation_update",
    "educt_apply_corrections",
    "educt_correction_pass",
    "educt_delete_transcript",
    "educt_edl_from_highlights",
    "educt_export",
    "educt_get_transcript",
    "educt_highlight_pass",
    "educt_list_layers",
    "educt_list_transcripts",
    "educt_locate",
    "educt_paragraph_pass",
    "educt_realign_transcript",
    "educt_render_edl",
    "educt_retranscribe_section",
    "educt_search_transcripts",
    "educt_speaker_pass",
    "educt_store_layer",
    "educt_store_transcript",
    "face_list",
    "face_register",
    "face_remove",
    "face_scan_folder",
    "face_validate",
    "gallery_add_media",
    "gallery_analyze",
    "gallery_annotation_create",
    "gallery_annotation_delete",
    "gallery_annotation_list",
    "gallery_annotation_update",
    "gallery_asset_detail",
    "gallery_create_album",
    "gallery_delete_album",
    "gallery_delete_image",
    "gallery_lineage",
    "gallery_list_album_members",
    "gallery_list_albums",
    "gallery_list_assets",
    "gallery_move_to_album",
    "gallery_name_face",
    "gallery_organize",
    "gallery_refresh",
    "gallery_remove_from_album",
    "gallery_search",
    "gallery_status",
    "gallery_timeline",
    "image_create_collage",
    "job_cancel",
    "job_list",
    "job_status",
    "job_submit",
    "layers_audio_pass",
    "layers_edl_compose",
    "layers_export",
    "layers_get",
    "layers_list",
    "layers_render_edl",
    "layers_render_redactions",
    "layers_search",
    "layers_semantic_pass",
    "layers_shot_pass",
    "layers_stitch_tracks",
    "layers_store",
    "layers_transcript",
    "media_fetch_image",
    "model_info",
    "model_list",
    "record_and_transcribe",
    "transcribe_and_store",
    "transcribe_bundle",
    "video_add_caption",
    "video_caption",
    "video_clip",
    "video_concat",
    "video_extract_frames",
    "video_fetch",
    "video_info",
    "video_to_gif",
    "voice_enroll",
    "voice_identify",
    "voice_list",
    "voice_prune",
    "voice_remove",
    "youtube_search",
];

/// Closed vocabularies for transcript annotation kind and locate basis.
pub const TRANSCRIPT_ANNOTATION_KINDS: [&str; 2] = ["highlight", "strike"];
pub const LOCATE_BASES: [&str; 2] = ["working", "source"];

fn optional_string_enum(values: &[&str]) -> schemars::Schema {
    schemars::json_schema!({"anyOf": [{"type": "string", "enum": values}, {"type": "null"}]})
}

fn optional_transcript_annotation_kind_schema(
    _: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    optional_string_enum(&TRANSCRIPT_ANNOTATION_KINDS)
}

fn optional_locate_basis_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    optional_string_enum(&LOCATE_BASES)
}

/// Deserialize an `AnyJsonValue` from either a JSON-encoded string or an inline JSON value.
pub fn json_text_or_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<hkask_types::AnyJsonValue, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) => serde_json::from_str(&text)
            .map(hkask_types::AnyJsonValue)
            .map_err(|error| {
                serde::de::Error::custom(format!("expected complete JSON text: {error}"))
            }),
        value => Ok(hkask_types::AnyJsonValue(value)),
    }
}

/// The closed layer-kind vocabulary — what a layer asserts over its media.
/// Shot, semantic, object, ocr and audio_event layers are model-pass
/// products; researcher layers are human-authored annotations; edl layers
/// are composed edit decisions. The wire form is snake_case, matching the
/// stored `kind` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaLayerKind {
    Shot,
    Semantic,
    Object,
    Ocr,
    AudioEvent,
    Researcher,
    Edl,
}

impl MediaLayerKind {
    /// Every valid wire value, in declaration order — the closed set named
    /// in rejection messages.
    pub const VALID_VALUES: &'static [&'static str] = &[
        "shot",
        "semantic",
        "object",
        "ocr",
        "audio_event",
        "researcher",
        "edl",
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            MediaLayerKind::Shot => "shot",
            MediaLayerKind::Semantic => "semantic",
            MediaLayerKind::Object => "object",
            MediaLayerKind::Ocr => "ocr",
            MediaLayerKind::AudioEvent => "audio_event",
            MediaLayerKind::Researcher => "researcher",
            MediaLayerKind::Edl => "edl",
        }
    }

    /// Parse a stored `kind` column (or any wire value) into the enum.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "shot" => Some(MediaLayerKind::Shot),
            "semantic" => Some(MediaLayerKind::Semantic),
            "object" => Some(MediaLayerKind::Object),
            "ocr" => Some(MediaLayerKind::Ocr),
            "audio_event" => Some(MediaLayerKind::AudioEvent),
            "researcher" => Some(MediaLayerKind::Researcher),
            "edl" => Some(MediaLayerKind::Edl),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EductAnnotationCreateRequest {
    pub transcript_id: String,
    /// Inclusive immutable source-word indices, not corrected-text indices.
    pub start_word: usize,
    pub end_word: usize,
    pub label: String,
    pub note: String,
    /// Annotation kind: highlight (default — a labeled selection) or strike
    /// (an unannotated cut over the range; label and note must be blank).
    #[serde(default)]
    #[schemars(schema_with = "optional_transcript_annotation_kind_schema")]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EductAnnotationUpdateRequest {
    pub transcript_id: String,
    pub annotation_id: String,
    pub start_word: usize,
    pub end_word: usize,
    pub label: String,
    pub note: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EductStoreLayerRequest {
    pub transcript_id: String,
    /// The layer as a tagged JSON object:
    /// {"kind": "speaker"|"paragraph"|"correction"|"highlight"|"edl"|"realignment", ...}.
    /// Validated against the transcript's word count before storage; a
    /// layer that fails validation is rejected with the named invariant.
    #[serde(deserialize_with = "json_text_or_value")]
    pub layer: hkask_types::AnyJsonValue,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EductRealignTranscriptRequest {
    pub transcript_id: String,
    /// Re-anchor a specific correction layer by ID; defaults to the latest.
    pub layer_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EductLocateRequest {
    pub transcript_id: String,
    /// The passage to locate — quote the rendered form exactly (punctuation
    /// included). All word-aligned matches are returned with their time
    /// ranges; ambiguity is surfaced as all candidates, never a guess.
    pub text: String,
    /// The index basis of the returned ranges: `"working"` (the default —
    /// the timing-aligned working view the quote matched against) or
    /// `"source"` (immutable source-word indices, what correction-layer
    /// edits and other source-anchored layers require; converted through
    /// the re-anchored index map when a realignment is applied).
    #[serde(default)]
    #[schemars(schema_with = "optional_locate_basis_schema")]
    pub basis: Option<String>,
}

/// Provenance wire object for `layers_store`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayersProvenanceRequest {
    /// The model that produced the layer (e.g. an inference-catalog label,
    /// or "user" for researcher-authored annotations).
    pub model: String,
    /// The prompt template that produced it (template name; for
    /// researcher-authored layers, a short description of the authoring path).
    pub prompt_template: String,
}

/// One item of a layer being stored — the wire form. `payload` is arbitrary
/// JSON in the `AnyJsonValue` JSON-text transport.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayersStoreItemRequest {
    /// Range start in source-PTS seconds.
    pub start_sec: f64,
    /// Range end in source-PTS seconds; omit for a point-in-time item.
    pub end_sec: Option<f64>,
    /// The text a lexical search matches — must be nonblank.
    pub searchable_text: String,
    /// Arbitrary structured payload (labels, boxes, confidences, OCR text).
    #[serde(deserialize_with = "json_text_or_value")]
    pub payload: hkask_types::AnyJsonValue,
    /// Gallery asset ids of frames this item references; each must exist.
    pub frame_asset_ids: Option<Vec<String>>,
}

/// Request to store one media layer over a media file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayersStoreRequest {
    /// Absolute local path (or URL) of the source media the layer annotates.
    pub media_path: String,
    /// Optional gallery asset ID linking the layer to a gallery asset.
    pub gallery_asset_id: Option<String>,
    /// Layer kind: "shot", "semantic", "object", "ocr", "audio_event",
    /// "researcher", or "edl" (closed set — unknown values are rejected
    /// naming the valid kinds).
    pub kind: MediaLayerKind,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Which model and prompt template produced the layer.
    pub provenance: LayersProvenanceRequest,
    /// The layer's items, keyed in source-PTS seconds and validated against
    /// the ffprobe-anchored media duration.
    pub items: Vec<LayersStoreItemRequest>,
}
