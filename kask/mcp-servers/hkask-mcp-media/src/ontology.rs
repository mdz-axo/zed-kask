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

/// Both axes for a registered tool, or `None` when the name is not one.
///
/// P5.4's dual-axis core: a caller never gets one axis without the other,
/// so a partial mapping cannot silently satisfy the contract.
pub(crate) fn tool_to_dual_axis(tool_name: &str) -> Option<(PkoConcept, DcConcept)> {
    match (tool_to_pko(tool_name), tool_to_dc(tool_name)) {
        (Some(process), Some(state)) => Some((process, state)),
        _ => None,
    }
}

/// Stamp the dual-axis identity onto a tool response.
///
/// This is the production consumer of the mapping: wired as the central
/// `ToolContext::annotate_response` hook via the `mcp_server!` macro's
/// annotate override, so every media tool response carries the process
/// (PKO) and state (DC+BIBO) concept its artifact occupies without a
/// per-site wrap at each of the 102 tool returns. The annotation is
/// additive — the existing body is untouched (the OMC supplement in the
/// `display_hint`'s nested `ontology` field is a different key path and
/// is untouched).
///
/// A name with no mapping returns the body unchanged rather than
/// inventing an axis: `dual_axis_covers_all_registered_tools` is what
/// guarantees the registered surface is total, and a silent default
/// would defeat it.
pub(crate) fn annotate(tool_name: &str, mut body: serde_json::Value) -> serde_json::Value {
    let Some((process, state)) = tool_to_dual_axis(tool_name) else {
        return body;
    };
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "ontology".to_string(),
            serde_json::json!({
                "pko_procedure": process,
                "dc_type": state,
            }),
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    // The generated canonical tool-name list — included from OUT_DIR, so
    // the totality test derives its expectations from the live tool
    // surface (build.rs scan), never a hardcoded list.
    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

    /// P5.4: "Every MCP server uses BOTH axes." Every registered tool
    /// must carry a process AND a state identity — a tool added without
    /// an explicit arm fails here rather than silently losing its axes.
    #[test]
    fn dual_axis_covers_all_registered_tools() {
        for name in TOOL_NAMES {
            let mapped = tool_to_dual_axis(name);
            assert!(
                mapped.is_some(),
                "no dual-axis mapping for registered tool '{name}'; add an explicit arm \
                 to tool_to_pko and tool_to_dc in src/ontology.rs"
            );
        }
        // The reverse direction: every name the mapping claims must be a
        // registered tool. Coverage alone let a phantom name sit in the
        // swarm mapping undetected (live 2026-10-18: `swarm_delete_local`,
        // a typo, matched no call site and still passed coverage).
        let ontology_src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ontology.rs"),
        )
        .expect("src/ontology.rs must be readable");
        let mut mapped: Vec<&str> = Vec::new();
        // The stamp's own JSON keys are not tool names — excluded by name,
        // not by pattern (a bare contains-'_' filter would catch them).
        const STAMP_KEYS: [&str; 3] = ["ontology", "pko_procedure", "dc_type"];
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            if !chunk.is_empty()
                && chunk.contains('_')
                && !STAMP_KEYS.contains(&chunk)
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            {
                mapped.push(chunk);
            }
        }
        mapped.sort_unstable();
        mapped.dedup();
        assert!(
            !mapped.is_empty(),
            "the reverse scan found no mapping names — the instrument is broken"
        );
        for name in &mapped {
            assert!(
                TOOL_NAMES.contains(name),
                "mapping names '{name}', which is not a registered tool — a stale or \
                 misspelled arm in src/ontology.rs"
            );
        }
    }

    /// The mapping is defined only over the registered surface: an
    /// unknown name returns None on both axes rather than a default
    /// concept.
    #[test]
    fn unknown_tool_has_no_axes() {
        assert!(tool_to_pko("not_a_tool").is_none());
        assert!(tool_to_dc("not_a_tool").is_none());
        assert!(tool_to_dual_axis("not_a_tool").is_none());
    }

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

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`. The OMC supplement's
    /// nested `ontology` field inside `display_hint` (a fenced-block
    /// STRING, not an object at this level) is untouched by the hook.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"output": "/tmp/img.png"});
        let stamped = annotate("generate_image", body.clone());
        assert_eq!(
            stamped["output"],
            serde_json::json!("/tmp/img.png"),
            "{stamped:?}"
        );
        assert_eq!(
            stamped["ontology"]["pko_procedure"],
            serde_json::json!(STEP_EXECUTION)
        );
        assert_eq!(stamped["ontology"]["dc_type"], serde_json::json!(DATASET));
        // An unmapped name returns the body unchanged — never an
        // invented axis.
        let untouched = annotate("not_a_registered_tool", body.clone());
        assert_eq!(untouched, body);
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use rmcp::handler::server::wrapper::Parameters;
        use std::sync::Arc;

        // The tool_behavior_tests construction (make_server_with_port),
        // built here: a minimal inference port over an empty gallery state
        // — workflow_list needs no inference and no credential. Only the
        // non-defaulted trait methods are implemented; vision/embed default
        // to typed refusals, which this tool never reaches.
        struct NoopPort;
        impl hkask_types::ports::InferencePort for NoopPort {
            fn generate(
                &self,
                _: &str,
                _: &hkask_types::template::LLMParameters,
                _: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<
                                hkask_types::InferenceResult,
                                hkask_types::InferenceError,
                            >,
                        > + Send
                        + '_,
                >,
            > {
                Box::pin(async {
                    Err(hkask_types::InferenceError::Connection(
                        "noop inference port — not configured for this test".into(),
                    ))
                })
            }
        }

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
