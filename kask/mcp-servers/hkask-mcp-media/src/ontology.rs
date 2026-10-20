//! Dual-axis ontology dispatch for hkask-mcp-media (P5.4, operator ruling
//! B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the media
//! server's mapping onto those two axes, wired as the central
//! `ToolContext::annotate_response` hook via the `mcp_server!` macro's
//! annotate override — one point stamps every tool response at the
//! `execute_tool` boundary.
//!
//! Domain bridge: per the PRINCIPLES.md dual-axis table, media's domain
//! bridge is OMC (MovieLabs Ontology for Media Creation) — layered ON
//! TOP of the dual-axis core where DC+BIBO is not specific enough,
//! never replacing it. The OMC supplement already lives in the
//! `display_hint` path (`omc.rs`'s `tool_to_omc` → the fenced ```media
//! block's nested `ontology` field, rendered by the media widget's
//! Explain affordance) and in the persisted creation graph — those are
//! untouched; this module supplies the core PKO + DC+BIBO axes the
//! ruling puts on every tool response.
//!
//! - **Process axis (PKO)** — media work is a creation procedure:
//!   generation/transformation/processing tools execute the creation
//!   procedure's steps; gallery/face/model/status tools read and manage
//!   stored records; the job queue and workflow tools track task
//!   execution; transcript tools capture and version spoken text.
//! - **State axis (DC+BIBO)** — write responses are datasets (a stored
//!   or modified measurement — a generated file, a persisted asset, a
//!   stored transcript); read responses are compiled readings over
//!   stored records — reports.
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology`
//! crate; this module holds only the media-specific tool→concept table.
//! That is the server's business, not the ontology's — the architectural
//! invariant (user directive 2026-08-05) that no ontology vocabulary is
//! defined inside an MCP server.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a media tool's response occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Generation: produces a new creative work — one step of the
        // creation procedure executing.
        "generate_image"
        | "generate_video"
        | "generate_speech"
        | "video_meme"
        | "expand_prompt"
        | "voice_design"
        | "image_create_collage" => Some(STEP_EXECUTION),
        // Transform/upscale/edit: produces a version of an existing
        // work — a derivation step executing.
        "transform_image"
        | "upscale_image"
        | "image_remove_background"
        | "image_apply_style"
        | "image_edit_region" => Some(STEP_EXECUTION),
        // Video/audio processing: clip, concat, trim, caption, GIF,
        // frames — processing steps executing.
        "video_clip"
        | "video_to_gif"
        | "image_to_video"
        | "video_concat"
        | "video_add_caption"
        | "video_remix"
        | "video_from_images"
        | "video_extract_frames"
        | "audio_capture"
        | "audio_trim"
        | "audio_concat" => Some(STEP_EXECUTION),
        // Transcription: capturing and storing spoken text — capture
        // steps executing.
        "record_and_transcribe" | "transcribe_bundle" | "transcribe_and_store" => {
            Some(STEP_EXECUTION)
        }
        // Transcript layering: correction/speaker/paragraph/highlight
        // passes and EDL composition version the stored transcript —
        // derivation steps executing over the capture.
        "educt_store_transcript"
        | "educt_store_layer"
        | "educt_paragraph_pass"
        | "educt_speaker_pass"
        | "educt_correction_pass"
        | "educt_apply_corrections"
        | "educt_realign_transcript"
        | "educt_highlight_pass"
        | "educt_edl_from_highlights"
        | "educt_render_edl" => Some(STEP_EXECUTION),
        // Gallery writes: import, organize, albums, refresh, delete —
        // gallery-management steps executing.
        "gallery_add_media"
        | "gallery_organize"
        | "gallery_refresh"
        | "gallery_create_album"
        | "gallery_delete_album"
        | "gallery_move_to_album"
        | "gallery_remove_from_album"
        | "gallery_delete_image"
        | "gallery_analyze"
        | "gallery_name_face"
        | "gallery_record_generation"
        | "gallery_reproduce" => Some(STEP_EXECUTION),
        // Face registry writes.
        "face_register" | "face_remove" | "face_scan_folder" => Some(STEP_EXECUTION),
        // Gallery reads: list, search, detail, timeline, status —
        // compiled readings over stored records, observing the
        // executions that produced the index.
        "gallery_list_assets"
        | "gallery_list_albums"
        | "gallery_list_album_members"
        | "gallery_search"
        | "gallery_asset_detail"
        | "gallery_timeline"
        | "gallery_status"
        | "gallery_lineage" => Some(PROCEDURE_EXECUTION),
        // Reads: image/video description, info, caption — analysis
        // readings.
        "describe_image" | "video_caption" | "video_info" | "face_list" | "face_validate" => {
            Some(PROCEDURE_EXECUTION)
        }
        // Model browser: listing models/participants.
        "model_list" | "model_info" => Some(PROCEDURE_EXECUTION),
        // Job queue: submit executes a task; list/status observe it;
        // cancel changes it.
        "job_submit" => Some(STEP_EXECUTION),
        "job_list" | "job_status" => Some(PROCEDURE_EXECUTION),
        "job_cancel" => Some(STEP_EXECUTION),
        // Workflows: save/load/delete mutate the stored workflow
        // records — steps; list reads them — a compiled reading.
        "workflow_save" | "workflow_load" | "workflow_delete" => Some(STEP_EXECUTION),
        "workflow_list" => Some(PROCEDURE_EXECUTION),
        // Reduct cloud reads: snapshots, status, transcripts,
        // highlights, reel detail — readings over the provider's
        // records.
        "reduct_connection_status"
        | "reduct_connection_probe"
        | "reduct_projects_snapshot"
        | "reduct_recordings_snapshot"
        | "reduct_recording_status"
        | "reduct_recording_transcript"
        | "reduct_recording_highlights"
        | "reduct_reels_snapshot"
        | "reduct_reel_detail" => Some(PROCEDURE_EXECUTION),
        // Reduct cloud writes: create/import/upload/publish — steps
        // executing against the provider.
        "reduct_create_recording"
        | "reduct_import_media"
        | "reduct_upload_gallery_media"
        | "reduct_upload_local_media"
        | "reduct_create_reel"
        | "reduct_add_reel_clip"
        | "reduct_add_reel_title"
        | "reduct_edit_reel_clip_range"
        | "reduct_publish_reel"
        | "reduct_add_recording_highlight"
        | "reduct_edit_recording_highlight" => Some(STEP_EXECUTION),
        // Transcript reads: list/get/locate/export — readings over
        // stored transcripts.
        "educt_list_transcripts"
        | "educt_get_transcript"
        | "educt_list_layers"
        | "educt_locate"
        | "educt_export"
        | "educt_delete_transcript" => Some(PROCEDURE_EXECUTION),
        // Discovery: youtube search and video fetch acquire sources.
        "youtube_search" | "video_fetch" => Some(STEP_EXECUTION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a media tool's response occupies.
///
/// Write responses are datasets (a stored or modified measurement — a
/// generated file, a persisted asset, a stored record); read responses
/// are compiled readings over stored records — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Reads: listings, searches, status, descriptions, snapshots,
        // transcripts read out — reports.
        "gallery_list_assets"
        | "gallery_list_albums"
        | "gallery_list_album_members"
        | "gallery_search"
        | "gallery_asset_detail"
        | "gallery_timeline"
        | "gallery_status"
        | "gallery_lineage"
        | "describe_image"
        | "video_caption"
        | "video_info"
        | "face_list"
        | "face_validate"
        | "model_list"
        | "model_info"
        | "job_list"
        | "job_status"
        | "workflow_list"
        | "reduct_connection_status"
        | "reduct_connection_probe"
        | "reduct_projects_snapshot"
        | "reduct_recordings_snapshot"
        | "reduct_recording_status"
        | "reduct_recording_transcript"
        | "reduct_recording_highlights"
        | "reduct_reels_snapshot"
        | "reduct_reel_detail"
        | "educt_list_transcripts"
        | "educt_get_transcript"
        | "educt_list_layers"
        | "educt_locate"
        | "educt_export" => Some(REPORT),
        // Writes: generation, transformation, processing, capture,
        // layering, gallery and face mutations, job and workflow
        // mutations, Reduct writes — datasets.
        "generate_image"
        | "generate_video"
        | "generate_speech"
        | "video_meme"
        | "expand_prompt"
        | "voice_design"
        | "image_create_collage"
        | "transform_image"
        | "upscale_image"
        | "image_remove_background"
        | "image_apply_style"
        | "image_edit_region"
        | "video_clip"
        | "video_to_gif"
        | "image_to_video"
        | "video_concat"
        | "video_add_caption"
        | "video_remix"
        | "video_from_images"
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
        | "educt_apply_corrections"
        | "educt_realign_transcript"
        | "educt_highlight_pass"
        | "educt_edl_from_highlights"
        | "educt_render_edl"
        | "educt_delete_transcript"
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
        | "gallery_record_generation"
        | "gallery_reproduce"
        | "face_register"
        | "face_remove"
        | "face_scan_folder"
        | "job_submit"
        | "job_cancel"
        | "workflow_save"
        | "workflow_load"
        | "workflow_delete" => Some(DATASET),
        "reduct_create_recording"
        | "reduct_import_media"
        | "reduct_upload_gallery_media"
        | "reduct_upload_local_media"
        | "reduct_create_reel"
        | "reduct_add_reel_clip"
        | "reduct_add_reel_title"
        | "reduct_edit_reel_clip_range"
        | "reduct_publish_reel"
        | "reduct_add_recording_highlight"
        | "reduct_edit_recording_highlight"
        | "youtube_search"
        | "video_fetch" => Some(DATASET),
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
    &["gallery_", "face_", "educt_", "audio_", "video_", "image_", "model_", "job_", "workflow_", "reduct_", "youtube_", "generate_", "transform_", "upscale_", "describe_", "expand_prompt", "record_", "transcribe_", "voice_design"],
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

    /// The axes must discriminate: a generation step and a gallery
    /// reading are different process concepts; a write and a read are
    /// different state concepts.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_pko("generate_image"), Some(STEP_EXECUTION));
        assert_eq!(
            tool_to_pko("gallery_list_assets"),
            Some(PROCEDURE_EXECUTION)
        );

        assert_eq!(tool_to_dc("generate_image"), Some(DATASET));
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
        // gallery state — workflow_list needs no inference and no
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
            None,
        );
        let response = server
            .workflow_list(Parameters(crate::types::WorkflowListRequest {
                limit: None,
            }))
            .await
            .expect("workflow_list succeeds on a fresh server");
        let value: serde_json::Value = serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {response}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {response}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION),
            "the live workflow_list response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live workflow_list response carries the state axis: {content}"
        );
    }
}
