//! Media-server OMC dispatch — server-specific mapping from media-tool names
//! to OMC concepts.
//!
//! The OMC concept vocabulary and the shared concept→explain-tool dispatch
//! function both live in the shared `hkask-bridge-ontology` crate. This module
//! holds only the media-server-specific mapping: which tool name produces
//! which OMC concept. That is the server's business, not the ontology's.

use hkask_bridge_ontology::omc::OmcConcept;
use hkask_bridge_ontology::omc::{
    ASSET, CREATIVE_WORK, MEDIA_SOURCE, PARTICIPANT, SCENE, SEQUENCE, SHOT, TASK, VERSION,
};

// Re-export the shared explain-tool dispatch so the media server's tests and
// any in-server consumers reference the single source of truth.
pub use hkask_bridge_ontology::omc::explain_tool_for;

/// Map a media-tool name to its OMC concept URI.
///
/// Every registered tool maps to an OMC concept. The mapping is direct: each
/// tool's output is the seed term for exactly one OMC concept (STAR pattern).
/// Face tools map to `ASSET` (faces are gallery assets — people identified
/// within a set of images, not OMC `PARTICIPANT` which is a production-side
/// concept about who made the media, not who appears in it).
pub fn tool_to_omc(tool: &str) -> Option<OmcConcept> {
    match tool {
        // Generation — produces a new creative work.
        "generate_image" | "generate_video" | "video_meme" | "expand_prompt" => Some(CREATIVE_WORK),
        // Transform / upscale — produces a version of an existing work.
        "transform_image" | "upscale_image" | "image_remove_background" | "image_apply_style" => {
            Some(VERSION)
        }
        // Analysis — produces a scene description (the scene is the subject).
        "describe_image" | "gallery_analyze" | "video_caption" => Some(SCENE),
        // Gallery management — produces/manages asset references.
        "gallery_search"
        | "gallery_find_similar"
        | "gallery_timeline"
        | "gallery_organize"
        | "gallery_status"
        | "gallery_refresh"
        | "gallery_delete_image" => Some(ASSET),
        // Face management — faces are gallery assets (people identified within
        // images, NOT production participants).
        "face_register" | "face_validate" | "face_scan_folder" | "face_list" | "face_remove"
        | "gallery_name_face" => Some(ASSET),
        // Audio — produces a media source (audio/text is a source asset).
        "generate_speech"
        | "audio_capture"
        | "record_and_transcribe"
        | "voice_design"
        | "transcribe"
        | "transcribe_bundle" => Some(MEDIA_SOURCE),
        // Video processing — produces a sequence (a clip is a sequence of shots).
        "video_clip" | "video_to_gif" | "image_to_video" | "video_concat" | "video_add_caption"
        | "video_remix" | "video_from_images" => Some(SEQUENCE),
        // Collage — produces a new creative work from sources.
        "image_create_collage" => Some(CREATIVE_WORK),
        // Frame extraction — produces shots (individual frames as gallery assets).
        "video_extract_frames" => Some(SHOT),
        // Generation lineage — produces/reads task records (production work).
        "gallery_record_generation" | "gallery_lineage" | "gallery_reproduce" => Some(TASK),
        // Model browser — the model/provider is a participant in the creation task.
        "model_list" | "model_info" => Some(PARTICIPANT),
        // Unknown tool — no OMC concept.
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
        assert_eq!(tool_to_omc("video_add_caption"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_remix"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_from_images"), Some(SEQUENCE));
    }

    #[test]
    fn video_caption_maps_to_scene() {
        assert_eq!(tool_to_omc("video_caption"), Some(SCENE));
    }

    #[test]
    fn video_meme_maps_to_creative_work() {
        assert_eq!(tool_to_omc("video_meme"), Some(CREATIVE_WORK));
    }

    #[test]
    fn expand_prompt_maps_to_creative_work() {
        assert_eq!(tool_to_omc("expand_prompt"), Some(CREATIVE_WORK));
    }

    #[test]
    fn gallery_management_maps_to_asset() {
        assert_eq!(tool_to_omc("gallery_organize"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_status"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_refresh"), Some(ASSET));
    }

    #[test]
    fn face_tools_map_to_asset() {
        // Faces are gallery assets (people identified within images),
        // NOT OMC PARTICIPANT (which is a production-side concept about
        // who made the media, not who appears in it).
        assert_eq!(tool_to_omc("face_register"), Some(ASSET));
        assert_eq!(tool_to_omc("face_validate"), Some(ASSET));
        assert_eq!(tool_to_omc("face_scan_folder"), Some(ASSET));
        assert_eq!(tool_to_omc("face_list"), Some(ASSET));
        assert_eq!(tool_to_omc("face_remove"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_name_face"), Some(ASSET));
    }

    #[test]
    fn audio_transcription_maps_to_media_source() {
        assert_eq!(tool_to_omc("voice_design"), Some(MEDIA_SOURCE));
        assert_eq!(tool_to_omc("transcribe"), Some(MEDIA_SOURCE));
        assert_eq!(tool_to_omc("transcribe_bundle"), Some(MEDIA_SOURCE));
    }

    #[test]
    fn gallery_lineage_tools_map_to_task() {
        assert_eq!(tool_to_omc("gallery_lineage"), Some(TASK));
        assert_eq!(tool_to_omc("gallery_reproduce"), Some(TASK));
    }

    #[test]
    fn frame_extraction_maps_to_shot() {
        assert_eq!(tool_to_omc("video_extract_frames"), Some(SHOT));
    }

    #[test]
    fn generation_lineage_maps_to_task() {
        assert_eq!(tool_to_omc("gallery_record_generation"), Some(TASK));
    }

    #[test]
    fn model_browser_maps_to_participant() {
        assert_eq!(tool_to_omc("model_list"), Some(PARTICIPANT));
        assert_eq!(tool_to_omc("model_info"), Some(PARTICIPANT));
    }

    #[test]
    fn unknown_tool_returns_none() {
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
