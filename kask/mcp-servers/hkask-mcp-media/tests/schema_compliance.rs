//! Schema-compliance tests for hkask-mcp-media tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. Layer 2 — the
//! `deser_totality_test!` proptest deserialization-totality property — was
//! intentionally omitted at first write; it is included since Batch 4 of
//! the testing-protocol propagation plan
//! (`kask/docs/reference/testing-protocol.md`).

use hkask_mcp_media::tools::educt::{
    EductAnnotationCreateRequest, EductAnnotationDeleteRequest, EductAnnotationListRequest,
    EductAnnotationUpdateRequest, EductSearchTranscriptsRequest,
};
use hkask_mcp_media::tools::gallery::{
    GalleryAnnotationCreateRequest, GalleryAnnotationDeleteRequest, GalleryAnnotationListRequest,
    GalleryAnnotationUpdateRequest,
};
use hkask_mcp_media::tools::processing::SampledVideoCaptionRequest;
use hkask_mcp_media::types::{
    AudioCaptureRequest, AudioConcatRequest, AudioTrimRequest, CreateCollageRequest,
    DescribeImageRequest, EductApplyCorrectionsRequest, EductCorrectionPassRequest,
    EductDeleteTranscriptRequest, EductEdlFromHighlightsRequest, EductExportRequest,
    EductGetTranscriptRequest, EductHighlightPassRequest, EductListLayersRequest,
    EductListTranscriptsRequest, EductLocateRequest, EductParagraphPassRequest,
    EductRealignTranscriptRequest, EductRenderEdlRequest, EductRetranscribeSectionRequest,
    EductSpeakerPassRequest, EductStoreLayerRequest, EductStoreTranscriptRequest, FaceListRequest,
    FaceRegisterRequest, FaceRemoveRequest, FaceScanFolderRequest, FaceValidateRequest,
    GalleryAddMediaRequest, GalleryAnalyzeRequest, GalleryAssetDetailRequest,
    GalleryCreateAlbumRequest, GalleryDeleteAlbumRequest, GalleryDeleteImageRequest,
    GalleryLineageRequest, GalleryListAlbumMembersRequest, GalleryListAssetsRequest,
    GalleryMoveToAlbumRequest, GalleryNameFaceRequest, GalleryOrganizeRequest,
    GalleryRefreshRequest, GalleryRemoveFromAlbumRequest, GallerySearchRequest,
    GalleryTimelineRequest, JobCancelRequest, JobListRequest, JobStatusRequest, JobSubmitRequest,
    LayersGetRequest, LayersListRequest, LayersSemanticPassRequest, LayersShotPassRequest,
    LayersStitchTracksRequest, LayersStoreRequest, ModelInfoRequest, ModelListRequest,
    RecordAndTranscribeRequest, TranscribeAndStoreRequest, TranscribeBundleRequest,
    VideoAddCaptionRequest, VideoClipRequest, VideoConcatRequest, VideoExtractFramesRequest,
    VideoFetchRequest, VideoInfoRequest, VideoToGifRequest, VoiceEnrollRequest, VoiceListRequest,
    VoicePruneRequest, VoiceRemoveRequest, YoutubeSearchRequest,
};
use hkask_types::find_boolean_schema_positions;
use schemars::schema_for;

/// expect: Every advertised request, including tool-local annotation/search
/// requests, is checked through the actual combined router, not a stale types list.
#[test]
fn live_router_schema_inventory_is_complete_and_clean() {
    let tools = hkask_mcp_media::MediaServer::combined_router().list_all();
    assert_eq!(
        tools.len(),
        90,
        "approved surface: 92 - 20 Reduct cloud tools + 5 media-layer tools + 1 section re-transcribe + 4 voice-profile tools + 1 voice identification + 1 audio pass + 1 layers search + 1 layers transcript + 1 layers edl compose + 1 layers export + 1 layers render edl + 1 track stitcher + 1 layers render redactions"
    );
    let actual = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    let declared = hkask_mcp_media::TOOL_NAMES
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        actual, declared,
        "schema inventory must match the registered tool surface"
    );
    for retired in [
        "generate_image",
        "transform_image",
        "upscale_image",
        "generate_video",
        "expand_prompt",
        "image_edit_region",
        "image_remove_background",
        "image_apply_style",
        "image_to_video",
        "video_meme",
        "video_remix",
        "video_from_images",
        "voice_design",
        "generate_speech",
        "gallery_record_generation",
        "gallery_reproduce",
        "workflow_save",
        "workflow_list",
        "workflow_load",
        "workflow_delete",
        "reduct_connection_status",
        "reduct_connection_probe",
        "reduct_projects_snapshot",
        "reduct_recordings_snapshot",
        "reduct_reels_snapshot",
        "reduct_reel_detail",
        "reduct_create_reel",
        "reduct_add_reel_clip",
        "reduct_add_reel_title",
        "reduct_edit_reel_clip_range",
        "reduct_create_recording",
        "reduct_import_media",
        "reduct_upload_gallery_media",
        "reduct_upload_local_media",
        "reduct_recording_highlights",
        "reduct_add_recording_highlight",
        "reduct_edit_recording_highlight",
        "reduct_publish_reel",
        "reduct_recording_status",
        "reduct_recording_transcript",
    ] {
        assert!(
            !actual.contains(retired),
            "retired tool remains registered: {retired}"
        );
    }
    for tool in tools {
        let schema = serde_json::to_value(&tool.input_schema).expect("router schema serializes");
        let violations = find_boolean_schema_positions(&schema);
        assert!(
            violations.is_empty(),
            "{} live schema contains bare-boolean positions: {violations:?}",
            tool.name
        );
    }
}

macro_rules! schema_clean_test {
    ($test_name:ident, $ty:ty) => {
        #[test]
        fn $test_name() {
            let schema = serde_json::to_value(&schema_for!($ty)).expect("schema serializes");
            let violations = find_boolean_schema_positions(&schema);
            assert!(
                violations.is_empty(),
                "{} schema has bare-boolean schema positions (Ollama/Gemini would reject): {violations:?}",
                stringify!($ty),
            );
        }
    };
}

schema_clean_test!(
    gallery_annotation_create_request_schema,
    GalleryAnnotationCreateRequest
);
schema_clean_test!(
    gallery_annotation_list_request_schema,
    GalleryAnnotationListRequest
);
schema_clean_test!(
    gallery_annotation_update_request_schema,
    GalleryAnnotationUpdateRequest
);
schema_clean_test!(
    gallery_annotation_delete_request_schema,
    GalleryAnnotationDeleteRequest
);
schema_clean_test!(
    educt_annotation_create_request_schema,
    EductAnnotationCreateRequest
);
schema_clean_test!(
    educt_annotation_list_request_schema,
    EductAnnotationListRequest
);
schema_clean_test!(
    educt_annotation_update_request_schema,
    EductAnnotationUpdateRequest
);
schema_clean_test!(
    educt_annotation_delete_request_schema,
    EductAnnotationDeleteRequest
);
schema_clean_test!(
    educt_search_transcripts_request_schema,
    EductSearchTranscriptsRequest
);
schema_clean_test!(transcribe_request_schema, TranscribeBundleRequest);
schema_clean_test!(audio_capture_request_schema, AudioCaptureRequest);

#[test]
fn audio_capture_schema_has_no_caller_selected_output_path() {
    let schema =
        serde_json::to_value(&schema_for!(AudioCaptureRequest)).expect("schema serializes");
    assert!(schema.pointer("/properties/duration_secs").is_some());
    assert!(
        schema.pointer("/properties/output_path").is_none(),
        "audio_capture must publish only to canonical generated storage"
    );
}

schema_clean_test!(
    record_and_transcribe_request_schema,
    RecordAndTranscribeRequest
);
schema_clean_test!(gallery_organize_request_schema, GalleryOrganizeRequest);
schema_clean_test!(gallery_search_request_schema, GallerySearchRequest);
schema_clean_test!(gallery_add_media_request_schema, GalleryAddMediaRequest);
schema_clean_test!(gallery_refresh_request_schema, GalleryRefreshRequest);
schema_clean_test!(describe_image_request_schema, DescribeImageRequest);
schema_clean_test!(gallery_analyze_request_schema, GalleryAnalyzeRequest);
schema_clean_test!(gallery_name_face_request_schema, GalleryNameFaceRequest);
schema_clean_test!(face_validate_request_schema, FaceValidateRequest);
schema_clean_test!(face_register_request_schema, FaceRegisterRequest);
schema_clean_test!(face_scan_folder_request_schema, FaceScanFolderRequest);
schema_clean_test!(face_list_request_schema, FaceListRequest);
schema_clean_test!(face_remove_request_schema, FaceRemoveRequest);
schema_clean_test!(gallery_timeline_request_schema, GalleryTimelineRequest);
schema_clean_test!(gallery_lineage_request_schema, GalleryLineageRequest);
schema_clean_test!(create_collage_request_schema, CreateCollageRequest);
schema_clean_test!(video_clip_request_schema, VideoClipRequest);
schema_clean_test!(video_to_gif_request_schema, VideoToGifRequest);
schema_clean_test!(video_add_caption_request_schema, VideoAddCaptionRequest);
schema_clean_test!(video_concat_request_schema, VideoConcatRequest);
schema_clean_test!(video_caption_request_schema, SampledVideoCaptionRequest);
schema_clean_test!(model_list_request_schema, ModelListRequest);
schema_clean_test!(model_info_request_schema, ModelInfoRequest);
schema_clean_test!(job_submit_request_schema, JobSubmitRequest);
schema_clean_test!(job_list_request_schema, JobListRequest);
schema_clean_test!(job_status_request_schema, JobStatusRequest);
schema_clean_test!(job_cancel_request_schema, JobCancelRequest);
schema_clean_test!(gallery_list_assets_request_schema, GalleryListAssetsRequest);
schema_clean_test!(
    gallery_asset_detail_request_schema,
    GalleryAssetDetailRequest
);
schema_clean_test!(
    gallery_create_album_request_schema,
    GalleryCreateAlbumRequest
);
schema_clean_test!(
    gallery_move_to_album_request_schema,
    GalleryMoveToAlbumRequest
);
schema_clean_test!(
    gallery_remove_from_album_request_schema,
    GalleryRemoveFromAlbumRequest
);
schema_clean_test!(
    gallery_delete_album_request_schema,
    GalleryDeleteAlbumRequest
);
schema_clean_test!(
    gallery_list_album_members_request_schema,
    GalleryListAlbumMembersRequest
);
schema_clean_test!(
    gallery_delete_image_request_schema,
    GalleryDeleteImageRequest
);
schema_clean_test!(video_info_request_schema, VideoInfoRequest);
schema_clean_test!(video_fetch_request_schema, VideoFetchRequest);
schema_clean_test!(youtube_search_request_schema, YoutubeSearchRequest);
schema_clean_test!(audio_trim_request_schema, AudioTrimRequest);
schema_clean_test!(audio_concat_request_schema, AudioConcatRequest);
schema_clean_test!(
    video_extract_frames_request_schema,
    VideoExtractFramesRequest
);

// Retained transcript request contracts. Tool-local gallery and
// sampled-caption requests are covered below and by the live-router scan.

schema_clean_test!(
    transcribe_and_store_request_schema,
    TranscribeAndStoreRequest
);
schema_clean_test!(
    educt_store_transcript_request_schema,
    EductStoreTranscriptRequest
);
schema_clean_test!(
    educt_list_transcripts_request_schema,
    EductListTranscriptsRequest
);
schema_clean_test!(
    educt_get_transcript_request_schema,
    EductGetTranscriptRequest
);
schema_clean_test!(
    educt_delete_transcript_request_schema,
    EductDeleteTranscriptRequest
);
schema_clean_test!(educt_store_layer_request_schema, EductStoreLayerRequest);
schema_clean_test!(educt_list_layers_request_schema, EductListLayersRequest);
schema_clean_test!(
    educt_paragraph_pass_request_schema,
    EductParagraphPassRequest
);
schema_clean_test!(educt_speaker_pass_request_schema, EductSpeakerPassRequest);
schema_clean_test!(
    educt_correction_pass_request_schema,
    EductCorrectionPassRequest
);
schema_clean_test!(
    educt_apply_corrections_request_schema,
    EductApplyCorrectionsRequest
);
schema_clean_test!(
    educt_realign_transcript_request_schema,
    EductRealignTranscriptRequest
);
schema_clean_test!(
    educt_highlight_pass_request_schema,
    EductHighlightPassRequest
);
schema_clean_test!(
    educt_edl_from_highlights_request_schema,
    EductEdlFromHighlightsRequest
);
schema_clean_test!(educt_render_edl_request_schema, EductRenderEdlRequest);
schema_clean_test!(
    educt_retranscribe_section_request_schema,
    EductRetranscribeSectionRequest
);
schema_clean_test!(educt_export_request_schema, EductExportRequest);
schema_clean_test!(educt_locate_request_schema, EductLocateRequest);
schema_clean_test!(layers_store_request_schema, LayersStoreRequest);
schema_clean_test!(layers_list_request_schema, LayersListRequest);
schema_clean_test!(layers_get_request_schema, LayersGetRequest);
schema_clean_test!(layers_shot_pass_request_schema, LayersShotPassRequest);
schema_clean_test!(
    layers_semantic_pass_request_schema,
    LayersSemanticPassRequest
);
schema_clean_test!(
    layers_stitch_tracks_request_schema,
    LayersStitchTracksRequest
);
schema_clean_test!(voice_enroll_request_schema, VoiceEnrollRequest);
schema_clean_test!(voice_list_request_schema, VoiceListRequest);
schema_clean_test!(voice_remove_request_schema, VoiceRemoveRequest);
schema_clean_test!(voice_prune_request_schema, VoicePruneRequest);

/// The closed layer-kind vocabulary rejects unknown values at the wire
/// boundary naming every valid kind — the rmcp deserializer surfaces this
/// as the invalid-params (InvalidArgument) class, so the closed set is
/// self-describing at the tool input seam.
#[test]
fn layers_store_kind_vocabulary_rejects_unknown_values_naming_valid_kinds() {
    let bad = serde_json::json!({
        "media_path": "/tmp/a.mp4",
        "kind": "cloud",
        "provenance": {"model": "user", "prompt_template": "t"},
        "items": []
    });
    let error = serde_json::from_value::<LayersStoreRequest>(bad)
        .expect_err("unknown kind must be rejected");
    for valid in [
        "shot",
        "semantic",
        "object",
        "ocr",
        "audio_event",
        "researcher",
        "edl",
    ] {
        assert!(
            error.to_string().contains(valid),
            "the rejection names valid kind {valid:?}: {error}"
        );
    }
}

// ── Layer 2: deserialization totality (proptest) ──────────────────────────
// Closes the documented omission: arbitrary tool input must never panic a
// request deserializer — it deserializes or returns a typed error. A panic
// inside a `#[tool]` deserializer would crash the server on malformed input,
// and strict-schema providers send exactly that.

use proptest::prelude::*;

fn arb_json(depth: u8) -> BoxedStrategy<serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::json!(n)),
        (0.0f64..=1.0e6).prop_map(|n| serde_json::json!(n)),
        ".{0,32}".prop_map(serde_json::Value::String),
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    prop_oneof![
        leaf,
        proptest::collection::vec(arb_json(depth - 1), 0..4).prop_map(serde_json::Value::Array),
        proptest::collection::hash_map(".{0,12}", arb_json(depth - 1), 0..4)
            .prop_map(|map| serde_json::Value::Object(map.into_iter().collect()),),
    ]
    .boxed()
}

macro_rules! deser_totality_test {
    ($test_name:ident, $ty:ty) => {
        proptest! {
            #[test]
            fn $test_name(input in arb_json(3)) {
                let _ = serde_json::from_value::<$ty>(input);
            }
        }
    };
}

deser_totality_test!(
    gallery_annotation_create_request_deser_totality,
    GalleryAnnotationCreateRequest
);
deser_totality_test!(
    gallery_annotation_list_request_deser_totality,
    GalleryAnnotationListRequest
);
deser_totality_test!(
    gallery_annotation_update_request_deser_totality,
    GalleryAnnotationUpdateRequest
);
deser_totality_test!(
    gallery_annotation_delete_request_deser_totality,
    GalleryAnnotationDeleteRequest
);
deser_totality_test!(
    educt_annotation_create_request_deser_totality,
    EductAnnotationCreateRequest
);
deser_totality_test!(
    educt_annotation_list_request_deser_totality,
    EductAnnotationListRequest
);
deser_totality_test!(
    educt_annotation_update_request_deser_totality,
    EductAnnotationUpdateRequest
);
deser_totality_test!(
    educt_annotation_delete_request_deser_totality,
    EductAnnotationDeleteRequest
);
deser_totality_test!(
    educt_search_transcripts_request_deser_totality,
    EductSearchTranscriptsRequest
);
deser_totality_test!(transcribe_request_deser_totality, TranscribeBundleRequest);
deser_totality_test!(audio_capture_request_deser_totality, AudioCaptureRequest);
deser_totality_test!(
    record_and_transcribe_request_deser_totality,
    RecordAndTranscribeRequest
);
deser_totality_test!(
    gallery_organize_request_deser_totality,
    GalleryOrganizeRequest
);
deser_totality_test!(gallery_search_request_deser_totality, GallerySearchRequest);
deser_totality_test!(
    gallery_add_media_request_deser_totality,
    GalleryAddMediaRequest
);
deser_totality_test!(
    gallery_refresh_request_deser_totality,
    GalleryRefreshRequest
);
deser_totality_test!(describe_image_request_deser_totality, DescribeImageRequest);
deser_totality_test!(
    gallery_analyze_request_deser_totality,
    GalleryAnalyzeRequest
);
deser_totality_test!(
    gallery_name_face_request_deser_totality,
    GalleryNameFaceRequest
);
deser_totality_test!(face_validate_request_deser_totality, FaceValidateRequest);
deser_totality_test!(face_register_request_deser_totality, FaceRegisterRequest);
deser_totality_test!(
    face_scan_folder_request_deser_totality,
    FaceScanFolderRequest
);
deser_totality_test!(face_list_request_deser_totality, FaceListRequest);
deser_totality_test!(face_remove_request_deser_totality, FaceRemoveRequest);
deser_totality_test!(
    gallery_timeline_request_deser_totality,
    GalleryTimelineRequest
);
deser_totality_test!(
    gallery_lineage_request_deser_totality,
    GalleryLineageRequest
);
deser_totality_test!(create_collage_request_deser_totality, CreateCollageRequest);
deser_totality_test!(video_clip_request_deser_totality, VideoClipRequest);
deser_totality_test!(video_to_gif_request_deser_totality, VideoToGifRequest);
deser_totality_test!(
    video_add_caption_request_deser_totality,
    VideoAddCaptionRequest
);
deser_totality_test!(video_concat_request_deser_totality, VideoConcatRequest);
deser_totality_test!(
    video_caption_request_deser_totality,
    SampledVideoCaptionRequest
);
deser_totality_test!(model_list_request_deser_totality, ModelListRequest);
deser_totality_test!(model_info_request_deser_totality, ModelInfoRequest);
deser_totality_test!(job_submit_request_deser_totality, JobSubmitRequest);
deser_totality_test!(job_list_request_deser_totality, JobListRequest);
deser_totality_test!(job_status_request_deser_totality, JobStatusRequest);
deser_totality_test!(job_cancel_request_deser_totality, JobCancelRequest);
deser_totality_test!(
    gallery_list_assets_request_deser_totality,
    GalleryListAssetsRequest
);
deser_totality_test!(
    gallery_asset_detail_request_deser_totality,
    GalleryAssetDetailRequest
);
deser_totality_test!(
    gallery_create_album_request_deser_totality,
    GalleryCreateAlbumRequest
);
deser_totality_test!(
    gallery_move_to_album_request_deser_totality,
    GalleryMoveToAlbumRequest
);
deser_totality_test!(
    gallery_remove_from_album_request_deser_totality,
    GalleryRemoveFromAlbumRequest
);
deser_totality_test!(
    gallery_delete_album_request_deser_totality,
    GalleryDeleteAlbumRequest
);
deser_totality_test!(
    gallery_list_album_members_request_deser_totality,
    GalleryListAlbumMembersRequest
);
deser_totality_test!(
    gallery_delete_image_request_deser_totality,
    GalleryDeleteImageRequest
);
deser_totality_test!(video_info_request_deser_totality, VideoInfoRequest);
deser_totality_test!(video_fetch_request_deser_totality, VideoFetchRequest);
deser_totality_test!(youtube_search_request_deser_totality, YoutubeSearchRequest);
deser_totality_test!(audio_trim_request_deser_totality, AudioTrimRequest);
deser_totality_test!(audio_concat_request_deser_totality, AudioConcatRequest);
deser_totality_test!(
    video_extract_frames_request_deser_totality,
    VideoExtractFramesRequest
);

// ── Coverage completion (2026-10-06): the educt request structs and
// the two pre-existing gaps join the deser battery — arbitrary tool input
// must never panic a request deserializer on any of the tools' inputs.

deser_totality_test!(
    transcribe_and_store_request_deser_totality,
    TranscribeAndStoreRequest
);
deser_totality_test!(
    educt_store_transcript_request_deser_totality,
    EductStoreTranscriptRequest
);
deser_totality_test!(
    educt_list_transcripts_request_deser_totality,
    EductListTranscriptsRequest
);
deser_totality_test!(
    educt_get_transcript_request_deser_totality,
    EductGetTranscriptRequest
);
deser_totality_test!(
    educt_delete_transcript_request_deser_totality,
    EductDeleteTranscriptRequest
);
deser_totality_test!(
    educt_store_layer_request_deser_totality,
    EductStoreLayerRequest
);
deser_totality_test!(
    educt_list_layers_request_deser_totality,
    EductListLayersRequest
);
deser_totality_test!(
    educt_paragraph_pass_request_deser_totality,
    EductParagraphPassRequest
);
deser_totality_test!(
    educt_speaker_pass_request_deser_totality,
    EductSpeakerPassRequest
);
deser_totality_test!(
    educt_correction_pass_request_deser_totality,
    EductCorrectionPassRequest
);
deser_totality_test!(
    educt_apply_corrections_request_deser_totality,
    EductApplyCorrectionsRequest
);
deser_totality_test!(
    educt_realign_transcript_request_deser_totality,
    EductRealignTranscriptRequest
);
deser_totality_test!(
    educt_highlight_pass_request_deser_totality,
    EductHighlightPassRequest
);
deser_totality_test!(
    educt_edl_from_highlights_request_deser_totality,
    EductEdlFromHighlightsRequest
);
deser_totality_test!(
    educt_render_edl_request_deser_totality,
    EductRenderEdlRequest
);
deser_totality_test!(educt_export_request_deser_totality, EductExportRequest);
deser_totality_test!(
    educt_retranscribe_section_request_deser_totality,
    EductRetranscribeSectionRequest
);
deser_totality_test!(educt_locate_request_deser_totality, EductLocateRequest);
deser_totality_test!(layers_store_request_deser_totality, LayersStoreRequest);
deser_totality_test!(layers_list_request_deser_totality, LayersListRequest);
deser_totality_test!(layers_get_request_deser_totality, LayersGetRequest);
deser_totality_test!(
    layers_shot_pass_request_deser_totality,
    LayersShotPassRequest
);
deser_totality_test!(
    layers_semantic_pass_request_deser_totality,
    LayersSemanticPassRequest
);
deser_totality_test!(voice_enroll_request_deser_totality, VoiceEnrollRequest);
deser_totality_test!(voice_list_request_deser_totality, VoiceListRequest);
deser_totality_test!(voice_remove_request_deser_totality, VoiceRemoveRequest);
deser_totality_test!(voice_prune_request_deser_totality, VoicePruneRequest);
