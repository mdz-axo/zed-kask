//! Research-media response identities use the shared PKO and DC/BIBO axes.
//! Asset derivation and historical provenance additionally carry MovieLabs OMC.
//! No ontology vocabulary is defined in this server.
use hkask_bridge_ontology::dc_bibo::DcConcept;
pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
use hkask_bridge_ontology::pko::PkoConcept;
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

/// Explicit response state; the router coverage and phantom-name tests pin both directions.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        "gallery_list_assets"
        | "gallery_list_albums"
        | "gallery_list_album_members"
        | "gallery_search"
        | "gallery_asset_detail"
        | "gallery_timeline"
        | "gallery_status"
        | "gallery_lineage"
        | "gallery_annotation_list"
        | "describe_image"
        | "video_caption"
        | "video_info"
        | "face_list"
        | "face_validate"
        | "model_list"
        | "model_info"
        | "job_list"
        | "job_status"
        | "educt_list_transcripts"
        | "educt_get_transcript"
        | "educt_list_layers"
        | "educt_locate"
        | "educt_export"
        | "educt_annotation_list"
        | "educt_search_transcripts"
        | "layers_list"
        | "layers_get"
        | "voice_list" => Some(REPORT),
        "image_create_collage"
        | "video_clip"
        | "video_to_gif"
        | "video_concat"
        | "video_add_caption"
        | "video_extract_frames"
        | "audio_capture"
        | "audio_trim"
        | "audio_concat"
        | "record_and_transcribe"
        | "transcribe_bundle"
        | "transcribe_and_store"
        | "educt_store_transcript"
        | "educt_store_layer"
        | "educt_paragraph_pass"
        | "educt_speaker_pass"
        | "educt_correction_pass"
        | "educt_retranscribe_section"
        | "educt_apply_corrections"
        | "educt_realign_transcript"
        | "educt_highlight_pass"
        | "educt_edl_from_highlights"
        | "educt_render_edl"
        | "educt_delete_transcript"
        | "educt_annotation_create"
        | "educt_annotation_update"
        | "educt_annotation_delete"
        | "gallery_annotation_create"
        | "gallery_annotation_update"
        | "gallery_annotation_delete"
        | "gallery_add_media"
        | "gallery_organize"
        | "gallery_refresh"
        | "gallery_create_album"
        | "gallery_delete_album"
        | "gallery_move_to_album"
        | "gallery_remove_from_album"
        | "gallery_delete_image"
        | "gallery_analyze"
        | "gallery_name_face"
        | "face_register"
        | "face_remove"
        | "voice_enroll"
        | "voice_remove"
        | "voice_prune"
        | "voice_identify"
        | "face_scan_folder"
        | "job_submit"
        | "job_cancel"
        | "youtube_search"
        | "video_fetch"
        | "layers_store"
        | "layers_shot_pass"
        | "layers_semantic_pass"
        | "layers_audio_pass"
        | "layers_search"
        | "layers_edl_compose"
        | "layers_render_edl"
        | "layers_export" => Some(DATASET),
        _ => None,
    }
}

/// Observations report a procedure; mutations execute a step of the curation procedure.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_to_dc(tool_name)? {
        REPORT => Some(PROCEDURE_EXECUTION),
        DATASET => Some(STEP_EXECUTION),
        _ => None,
    }
}

// The shared dual-axis machinery (the both-axes combinator, the central
// annotate stamp, and the both-directions totality tests) comes from the
// framework crate's `dual_axis_annotate!` macro — one definition across
// the nine stamped servers instead of a copy per server.

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &[
        "gallery_",
        "face_",
        "educt_",
        "layers_",
        "audio_",
        "video_",
        "image_",
        "model_",
        "job_",
        "youtube_",
        "describe_",
        "record_",
        "transcribe_",
    ],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;

    // The both-directions totality tests (coverage + the phantom-name
    // reverse scan) are emitted by the framework crate's
    // `dual_axis_annotate!` macro above — one definition across the nine
    // stamped servers, including its own generated TOOL_NAMES include.
    // This module keeps the media-specific tests.

    /// The axes must discriminate: an annotation step and a gallery
    /// reading are different process concepts; a write and a read are
    /// different state concepts.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(
            tool_to_pko("gallery_annotation_create"),
            Some(STEP_EXECUTION)
        );
        assert_eq!(
            tool_to_pko("gallery_list_assets"),
            Some(PROCEDURE_EXECUTION)
        );

        assert_eq!(tool_to_dc("gallery_annotation_create"), Some(DATASET));
        assert_eq!(tool_to_dc("gallery_list_assets"), Some(REPORT));
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use rmcp::handler::server::wrapper::Parameters;
        use std::sync::Arc;

        // The tool_behavior_tests construction (make_server_with_port),
        // built here: the shared noop inference double over an empty
        // gallery state — job_list needs no inference and no
        // credential.
        use hkask_mcp_server::server::test_support::NoopInferencePort as NoopPort;

        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let gallery_store = Arc::new(
            crate::gallery::store::GalleryStore::from_driver(driver).expect("gallery store init"),
        );
        let server = crate::MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopPort),
            Arc::new(std::sync::Mutex::new(None)),
            gallery_store,
            crate::templates::create_env().expect("media templates must compile"),
            crate::video::ffmpeg::FfmpegRunner::detect(),
            crate::video::ytdlp::YtDlpRunner::detect(),
            crate::jobs::new_job_store(),
            None,
            Default::default(),
        );
        let response = server
            .job_list(Parameters(crate::types::JobListRequest {
                status: None,
                limit: None,
            }))
            .await
            .expect("job_list succeeds on a fresh server");
        let value: serde_json::Value = serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {response}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {response}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION),
            "the live job_list response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live job_list response carries the state axis: {content}"
        );
    }
}
