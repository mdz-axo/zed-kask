//! MovieLabs Ontology for Media Creation (OMC) bridge for hkask-mcp-media.
//!
//! Maps media-tool names to OMC concept URIs. OMC is the MovieLabs standard
//! ontology for media production workflows (capture → post → distribution).
//! We anchor to OMC rather than inventing our own taxonomy.
//!
//! Reference: <https://movielabs.com/ontology-for-media-creation/>
//! Source: <https://github.com/MovieLabs/OMC>
//!
//! This module follows the STAR extraction pattern (PRINCIPLES.md §P8): seed
//! terms + direct logical entailments, no intermediate hierarchy. It is the
//! `media` server's domain bridge layered on top of the DC+BIBO dual-axis
//! core (PRINCIPLES.md §P5).
//!
//! ## Why OMC
//!
//! The condenser (`hkask-condenser/src/algorithms.rs::derive_ontology_anchor`)
//! already classifies media-tool names (`generate_*`, `video_*`, `image_*`,
//! `gallery_*`, `face_*`) into `OntologyNamespace::Omc`. Without this bridge,
//! that classification points at a generic `dcterms:Collection` concept —
//! not a media-creation concept. This module supplies the concrete OMC
//! concepts so the `display_hint` block carries an ontologically meaningful
//! tag the media widget can dispatch on (the "I" pattern — ontology-bounded
//! affordances).

/// An OMC concept URI — the canonical identifier for a media-creation concept.
pub type OmcConcept = &'static str;

// ── OMC concept constants (STAR seed terms) ──────────────────────────────
//
// These are the top-level OMC concepts most directly entailed by the media
// server's tool outputs. OMC is large; we extract only the seed terms the
// tools actually produce, plus their direct logical entailments (a
// `Version` is a `CreativeWork`, a `Shot` is part of a `Scene`, etc.).

/// A distinct intellectual or artistic creation — the root creative artifact.
/// OMC: `omc:CreativeWork` (analogous to `dcterms:Work`).
pub const CREATIVE_WORK: OmcConcept = "omc:CreativeWork";
/// A continuous sequence of media — a single rendered image or video clip.
/// OMC: `omc:Scene` (a contiguous segment of a creative work).
pub const SCENE: OmcConcept = "omc:Scene";
/// A single camera capture — a frame or take within a scene.
/// OMC: `omc:Shot`.
pub const SHOT: OmcConcept = "omc:Shot";
/// An ordered series of scenes — a multi-step media workflow output.
/// OMC: `omc:Sequence`.
pub const SEQUENCE: OmcConcept = "omc:Sequence";
/// A person or system participating in media creation (model, artist, tool).
/// OMC: `omc:Participant`.
pub const PARTICIPANT: OmcConcept = "omc:Participant";
/// A source media asset — the raw input to a transform or generation.
/// OMC: `omc:MediaSource`.
pub const MEDIA_SOURCE: OmcConcept = "omc:MediaSource";
/// A managed media asset in the gallery — a stored, tagged, retrievable item.
/// OMC: `omc:Asset`.
pub const ASSET: OmcConcept = "omc:Asset";
/// A unit of production work — a workflow execution, a generation job.
/// OMC: `omc:Task`.
pub const TASK: OmcConcept = "omc:Task";
/// A derived or modified form of a creative work — an upscale, transform,
/// or remix output. OMC: `omc:Version` (a version is a creative work).
pub const VERSION: OmcConcept = "omc:Version";

// ── tool name → OMC concept mapping ──────────────────────────────────────

/// Map a media-tool name to its OMC concept URI.
///
/// Returns `None` for tools not covered by OMC (provider-specific metadata,
/// pure query tools without a media artifact). The mapping is direct: each
/// tool's output is the seed term for exactly one OMC concept (STAR pattern).
pub fn tool_to_omc(tool: &str) -> Option<OmcConcept> {
    match tool {
        // Generation — produces a new creative work.
        "generate_image" | "generate_video" => Some(CREATIVE_WORK),
        // Transform / upscale — produces a version of an existing work.
        "transform_image" | "upscale_image" | "image_remove_background" | "image_apply_style" => {
            Some(VERSION)
        }
        // Analysis — produces a scene description (the scene is the subject).
        "describe_image" | "gallery_analyze" => Some(SCENE),
        // Gallery retrieval — produces asset references.
        "gallery_search" | "gallery_find_similar" | "gallery_timeline" => Some(ASSET),
        // Audio — produces a media source (audio is a source asset).
        "generate_speech" | "audio_capture" | "record_and_transcribe" => Some(MEDIA_SOURCE),
        // Workflow — produces a task (a multi-step production job).
        "execute_workflow" => Some(TASK),
        // Video processing — produces a sequence (a clip is a sequence of shots).
        "video_clip" | "video_to_gif" | "image_to_video" | "video_concat" => Some(SEQUENCE),
        // Collage / extract — produces a new creative work from sources.
        "image_create_collage" | "extract_object" => Some(CREATIVE_WORK),
        // Not covered by OMC (pure metadata / registry tools).
        _ => None,
    }
}

/// The OMC concept that the "Explain" affordance should dispatch on, given the
/// block's OMC tag. This is the "I" pattern (ontology-bounded affordances):
/// the OMC concept determines which explain tool the widget dispatches.
///
/// - `omc:CreativeWork` / `omc:Version` → `describe_image` (vision caption).
/// - `omc:Scene` → `gallery_analyze` (scene analysis pipeline).
/// - `omc:Asset` → `gallery_analyze` (asset inspection).
/// - Others → `describe_image` (the general vision fallback).
pub fn explain_tool_for(omc: &str) -> &'static str {
    match omc {
        SCENE | ASSET => "gallery_analyze",
        // CreativeWork, Version, MediaSource, Sequence, Shot, Participant, Task
        // — all describe-able via the vision caption tool.
        _ => "describe_image",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_tools_map_to_creative_work() {
        assert_eq!(tool_to_omc("generate_image"), Some(CREATIVE_WORK));
        assert_eq!(tool_to_omc("generate_video"), Some(CREATIVE_WORK));
    }

    #[test]
    fn transform_tools_map_to_version() {
        assert_eq!(tool_to_omc("transform_image"), Some(VERSION));
        assert_eq!(tool_to_omc("upscale_image"), Some(VERSION));
        assert_eq!(tool_to_omc("image_remove_background"), Some(VERSION));
        assert_eq!(tool_to_omc("image_apply_style"), Some(VERSION));
    }

    #[test]
    fn analysis_tools_map_to_scene() {
        assert_eq!(tool_to_omc("describe_image"), Some(SCENE));
        assert_eq!(tool_to_omc("gallery_analyze"), Some(SCENE));
    }

    #[test]
    fn gallery_retrieval_maps_to_asset() {
        assert_eq!(tool_to_omc("gallery_search"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_find_similar"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_timeline"), Some(ASSET));
    }

    #[test]
    fn audio_tools_map_to_media_source() {
        assert_eq!(tool_to_omc("generate_speech"), Some(MEDIA_SOURCE));
        assert_eq!(tool_to_omc("audio_capture"), Some(MEDIA_SOURCE));
        assert_eq!(tool_to_omc("record_and_transcribe"), Some(MEDIA_SOURCE));
    }

    #[test]
    fn workflow_maps_to_task() {
        assert_eq!(tool_to_omc("execute_workflow"), Some(TASK));
    }

    #[test]
    fn video_processing_maps_to_sequence() {
        assert_eq!(tool_to_omc("video_clip"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_to_gif"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("image_to_video"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_concat"), Some(SEQUENCE));
    }

    #[test]
    fn unknown_tool_returns_none() {
        assert!(tool_to_omc("gallery_organize").is_none());
        assert!(tool_to_omc("face_register").is_none());
        assert!(tool_to_omc("").is_none());
        assert!(tool_to_omc("some_unknown_tool").is_none());
    }

    #[test]
    fn explain_tool_dispatches_on_omc_concept() {
        // The "I" pattern: OMC concept drives the explain tool.
        assert_eq!(explain_tool_for(SCENE), "gallery_analyze");
        assert_eq!(explain_tool_for(ASSET), "gallery_analyze");
        assert_eq!(explain_tool_for(CREATIVE_WORK), "describe_image");
        assert_eq!(explain_tool_for(VERSION), "describe_image");
        assert_eq!(explain_tool_for(MEDIA_SOURCE), "describe_image");
        assert_eq!(explain_tool_for(SEQUENCE), "describe_image");
        assert_eq!(explain_tool_for(SHOT), "describe_image");
        assert_eq!(explain_tool_for(TASK), "describe_image");
        assert_eq!(explain_tool_for(PARTICIPANT), "describe_image");
    }
}
