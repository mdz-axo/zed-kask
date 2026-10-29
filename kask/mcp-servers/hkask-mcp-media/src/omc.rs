//! Media-server OMC dispatch — server-specific mapping from media-tool names
//! to OMC concepts.
//!
//! The OMC concept vocabulary and the shared concept→explain-tool dispatch
//! function both live in the shared `hkask-bridge-ontology` crate. This module
//! holds only the media-server-specific mapping: which tool name produces
//! which OMC concept. That is the server's business, not the ontology's.

use hkask_bridge_ontology::omc::OmcConcept;
// Re-exported publicly: the tool-surface tests assert against the same
// fixture-guarded constants the mapping returns (the fixture pins the
// URI's reality; the test pins the tool→concept mapping).
pub use hkask_bridge_ontology::omc::{
    ASSET, CAPTURE, CREATED_ON, CREATIVE_WORK, HAS_PARTICIPANT, HAS_PROVENANCE, HAS_STATE,
    HAS_STATE_DESCRIPTOR, HAS_TASK, IS_CREATED_BY, IS_CREATED_BY_TASK, MEDIA_CREATION_CONTEXT,
    PARTICIPANT, PROVENANCE, ROLE, SCENE, SEQUENCE, SERVICE, SHOT, STATE, STATE_DESCRIPTOR, TASK,
    VERSION_INFO,
};

// Re-export the shared explain-tool dispatch so the media server's tests and
// any in-server consumers reference the single source of truth.
pub use hkask_bridge_ontology::omc::explain_tool_for;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CreationEntity {
    pub id: String,
    pub types: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CreationRelationship {
    pub subject_id: String,
    pub predicate: String,
    pub object_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CreationGraph {
    pub asset_id: String,
    pub task_id: String,
    pub participant_id: String,
    pub state_id: String,
    pub provenance_id: String,
    pub created_at: String,
    pub entities: Vec<CreationEntity>,
    pub relationships: Vec<CreationRelationship>,
}

pub fn creation_graph(asset_id: &str, task_id: &str, created_at: &str) -> CreationGraph {
    let participant_id = "service:hkask-mcp-media".to_string();
    let role_id = format!("{task_id}:role");
    let state_id = format!("{task_id}:state");
    let state_descriptor_id = format!("{task_id}:state-descriptor:completed");
    let provenance_id = format!("{task_id}:provenance");
    let entity = |id: String, types: &[&str]| CreationEntity {
        id,
        types: types.iter().map(|value| (*value).to_string()).collect(),
    };
    let relationship = |subject_id: &str, predicate: &str, object_id: &str| CreationRelationship {
        subject_id: subject_id.to_string(),
        predicate: predicate.to_string(),
        object_id: object_id.to_string(),
    };

    CreationGraph {
        asset_id: asset_id.to_string(),
        task_id: task_id.to_string(),
        participant_id: participant_id.clone(),
        state_id: state_id.clone(),
        provenance_id: provenance_id.clone(),
        created_at: created_at.to_string(),
        entities: vec![
            entity(asset_id.to_string(), &[ASSET]),
            entity(task_id.to_string(), &[TASK]),
            entity(participant_id.clone(), &[PARTICIPANT, SERVICE]),
            entity(role_id.clone(), &[ROLE]),
            entity(state_id.clone(), &[STATE]),
            entity(state_descriptor_id.clone(), &[STATE_DESCRIPTOR]),
            entity(provenance_id.clone(), &[PROVENANCE]),
        ],
        relationships: vec![
            relationship(asset_id, HAS_PROVENANCE, &provenance_id),
            relationship(asset_id, IS_CREATED_BY_TASK, task_id),
            relationship(task_id, HAS_STATE, &state_id),
            relationship(&state_id, HAS_STATE_DESCRIPTOR, &state_descriptor_id),
            relationship(&provenance_id, IS_CREATED_BY, &participant_id),
            relationship(&role_id, HAS_TASK, task_id),
            relationship(&role_id, HAS_PARTICIPANT, &participant_id),
        ],
    }
}

/// Map a media-tool name to its OMC concept URI.
///
/// Every registered tool maps to an OMC concept. The mapping is direct: each
/// tool's output is the seed term for exactly one OMC concept (STAR pattern).
/// Face tools map to `ASSET` (faces are gallery assets — people identified
/// within a set of images, not OMC `PARTICIPANT` which is a production-side
/// concept about who made the media, not who appears in it).
pub fn tool_to_omc(tool: &str) -> Option<OmcConcept> {
    match tool {
        "describe_image" | "gallery_analyze" | "video_caption" => Some(SCENE),
        "gallery_search"
        | "gallery_timeline"
        | "gallery_organize"
        | "gallery_status"
        | "gallery_refresh"
        | "gallery_delete_image"
        | "gallery_add_media"
        | "gallery_asset_detail"
        | "gallery_list_assets"
        | "gallery_create_album"
        | "gallery_list_albums"
        | "gallery_move_to_album"
        | "gallery_remove_from_album"
        | "gallery_delete_album"
        | "gallery_list_album_members"
        | "gallery_annotation_create"
        | "gallery_annotation_list"
        | "gallery_annotation_update"
        | "gallery_annotation_delete"
        | "youtube_search"
        | "video_fetch"
        | "face_register"
        | "face_validate"
        | "face_scan_folder"
        | "face_list"
        | "face_remove"
        | "voice_enroll"
        | "voice_list"
        | "voice_remove"
        | "voice_prune"
        | "gallery_name_face" => Some(ASSET),
        "audio_capture"
        | "record_and_transcribe"
        | "transcribe_bundle"
        | "transcribe_and_store"
        | "audio_trim"
        | "audio_concat"
        | "educt_store_transcript"
        | "educt_list_transcripts"
        | "educt_get_transcript"
        | "educt_delete_transcript"
        | "educt_search_transcripts" => Some(CAPTURE),
        "video_clip" | "video_to_gif" | "video_concat" | "video_add_caption" | "video_info"
        | "educt_render_edl" => Some(SEQUENCE),
        "image_create_collage" => Some(CREATIVE_WORK),
        "video_extract_frames" => Some(SHOT),
        "gallery_lineage" | "job_submit" | "job_list" | "job_status" | "job_cancel" => Some(TASK),
        "model_list" | "model_info" => Some(PARTICIPANT),
        "educt_store_layer"
        | "educt_list_layers"
        | "educt_paragraph_pass"
        | "educt_speaker_pass"
        | "educt_correction_pass"
        | "educt_retranscribe_section"
        | "educt_apply_corrections"
        | "educt_realign_transcript"
        | "educt_highlight_pass"
        | "educt_edl_from_highlights"
        | "educt_export"
        | "educt_locate"
        | "educt_annotation_create"
        | "educt_annotation_list"
        | "educt_annotation_update"
        | "educt_annotation_delete"
        | "layers_store"
        | "layers_list"
        | "layers_get"
        | "layers_shot_pass" => Some(VERSION_INFO),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_graph_uses_official_task_asset_participant_state_and_provenance_edges() {
        let graph = creation_graph("asset-1", "task-1", "2026-09-14T00:00:00Z");
        let types = graph
            .entities
            .iter()
            .flat_map(|entity| entity.types.iter().map(String::as_str))
            .collect::<std::collections::HashSet<_>>();
        for expected in [
            ASSET,
            TASK,
            PARTICIPANT,
            SERVICE,
            ROLE,
            STATE,
            STATE_DESCRIPTOR,
            PROVENANCE,
        ] {
            assert!(
                types.contains(expected),
                "missing OMC entity type {expected}"
            );
        }
        let predicates = graph
            .relationships
            .iter()
            .map(|relationship| relationship.predicate.as_str())
            .collect::<std::collections::HashSet<_>>();
        for expected in [
            HAS_PROVENANCE,
            IS_CREATED_BY_TASK,
            HAS_STATE,
            HAS_STATE_DESCRIPTOR,
            IS_CREATED_BY,
            HAS_TASK,
            HAS_PARTICIPANT,
        ] {
            assert!(
                predicates.contains(expected),
                "missing OMC relationship {expected}"
            );
        }
    }

    #[test]
    fn analysis_tools_map_to_scene() {
        assert_eq!(tool_to_omc("describe_image"), Some(SCENE));
        assert_eq!(tool_to_omc("gallery_analyze"), Some(SCENE));
    }

    #[test]
    fn gallery_retrieval_maps_to_asset() {
        assert_eq!(tool_to_omc("gallery_search"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_add_media"), Some(ASSET));
        assert_eq!(tool_to_omc("gallery_timeline"), Some(ASSET));
    }

    #[test]
    fn audio_tools_map_to_media_source() {
        assert_eq!(tool_to_omc("audio_capture"), Some(CAPTURE));
        assert_eq!(tool_to_omc("record_and_transcribe"), Some(CAPTURE));
    }

    #[test]
    fn video_processing_maps_to_sequence() {
        assert_eq!(tool_to_omc("video_clip"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_to_gif"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_concat"), Some(SEQUENCE));
        assert_eq!(tool_to_omc("video_add_caption"), Some(SEQUENCE));
    }

    #[test]
    fn video_caption_maps_to_scene() {
        assert_eq!(tool_to_omc("video_caption"), Some(SCENE));
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
        assert_eq!(tool_to_omc("transcribe_bundle"), Some(CAPTURE));
        assert_eq!(tool_to_omc("transcribe_and_store"), Some(CAPTURE));
    }

    #[test]
    fn gallery_lineage_tools_map_to_task() {
        assert_eq!(tool_to_omc("gallery_lineage"), Some(TASK));
    }

    #[test]
    fn frame_extraction_maps_to_shot() {
        assert_eq!(tool_to_omc("video_extract_frames"), Some(SHOT));
    }

    #[test]
    fn generation_lineage_maps_to_task() {}

    #[test]
    fn model_browser_maps_to_participant() {
        assert_eq!(tool_to_omc("model_list"), Some(PARTICIPANT));
        assert_eq!(tool_to_omc("model_info"), Some(PARTICIPANT));
    }

    #[test]
    fn job_queue_maps_to_task() {
        assert_eq!(tool_to_omc("job_submit"), Some(TASK));
        assert_eq!(tool_to_omc("job_list"), Some(TASK));
        assert_eq!(tool_to_omc("job_status"), Some(TASK));
        assert_eq!(tool_to_omc("job_cancel"), Some(TASK));
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
        assert_eq!(explain_tool_for(VERSION_INFO), "describe_image");
        assert_eq!(explain_tool_for(CAPTURE), "describe_image");
        assert_eq!(explain_tool_for(SEQUENCE), "describe_image");
    }
}
