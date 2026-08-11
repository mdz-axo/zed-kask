//! Media-server OMC dispatch — server-specific mapping from media-tool names
//! to OMC concepts.
//!
//! The OMC concept vocabulary and the shared concept→explain-tool dispatch
//! function both live in the shared `hkask-bridge-ontology` crate. This module
//! holds only the media-server-specific mapping: which tool name produces
//! which OMC concept. That is the server's business, not the ontology's.

use hkask_bridge_ontology::omc::OmcConcept;
use hkask_bridge_ontology::omc::{ASSET, CREATIVE_WORK, MEDIA_SOURCE, SCENE, SEQUENCE, VERSION};

// Re-export the shared explain-tool dispatch so the media server's tests and
// any in-server consumers reference the single source of truth.
pub use hkask_bridge_ontology::omc::explain_tool_for;

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
        // Video processing — produces a sequence (a clip is a sequence of shots).
        "video_clip" | "video_to_gif" | "image_to_video" | "video_concat" => Some(SEQUENCE),
        // Collage — produces a new creative work from sources.
        "image_create_collage" => Some(CREATIVE_WORK),
        // Not covered by OMC (pure metadata / registry tools).
        _ => None,
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
    }
}
