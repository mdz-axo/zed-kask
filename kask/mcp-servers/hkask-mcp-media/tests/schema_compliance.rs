//! Schema-compliance tests for hkask-mcp-media tool request structs.
//!
//! Layer-1 deterministic schema scan enforcing the `.rules` trap "kask MCP
//! tool inputs that accept arbitrary JSON use `AnyJsonValue`": `schemars`
//! renders `serde_json::Value` as the bare boolean `true` in schema-valued
//! positions, which strict-schema-decoding providers (Ollama, Gemini) reject
//! (`400 cannot unmarshal bool into api.ToolProperty`). One bare boolean in
//! any enabled tool's schema fails the whole chat-completion request.
//!
//! Layer 1 only — the `schema_clean_test!` macro asserts no request struct's
//! JSON schema has a bare-boolean schema-valued position. Layer 2 (a
//! `proptest!` deserialization-totality property) is intentionally omitted: it
//! needs `proptest` + `hkask-test-harness` dev-deps to guard a different
//! invariant (P4 deserialization totality) that is out of scope here.

use hkask_mcp_media::types::{
    ApplyStyleRequest, AudioCaptureRequest, AudioConcatRequest, AudioTrimRequest,
    CreateCollageRequest, DescribeImageRequest, ExpandPromptRequest, FaceListRequest,
    FaceRegisterRequest, FaceRemoveRequest, FaceScanFolderRequest, FaceValidateRequest,
    GalleryAddAudioRequest, GalleryAddVideoRequest, GalleryAnalyzeRequest,
    GalleryAssetDetailRequest, GalleryCreateAlbumRequest, GalleryDeleteAlbumRequest,
    GalleryDeleteImageRequest, GalleryFindSimilarRequest, GalleryLineageRequest,
    GalleryListAlbumMembersRequest, GalleryMoveToAlbumRequest, GalleryNameFaceRequest,
    GalleryOrganizeRequest, GalleryRecordGenerationRequest, GalleryRefreshRequest,
    GalleryRemoveFromAlbumRequest, GalleryReproduceRequest, GallerySearchRequest,
    GalleryTimelineRequest, GenerateImageRequest, GenerateSpeechRequest, GenerateVariantsRequest,
    GenerateVideoRequest, ImageEditRegionRequest, ImageToVideoRequest, JobCancelRequest,
    JobListRequest, JobStatusRequest, JobSubmitRequest, ModelInfoRequest, ModelListRequest,
    RecordAndTranscribeRequest, RemoveBackgroundRequest, TranscribeRequest, TransformImageRequest,
    UpscaleImageRequest, VideoAddCaptionRequest, VideoCaptionRequest, VideoClipRequest,
    VideoConcatRequest, VideoExtractFramesRequest, VideoFetchRequest, VideoFromImagesRequest,
    VideoInfoRequest, VideoMemeRequest, VideoRemixRequest, VideoToGifRequest, VoiceDesignRequest,
    WorkflowDeleteRequest, WorkflowLoadRequest, WorkflowSaveRequest,
};
use hkask_mcp_server::find_boolean_schema_positions;
use schemars::schema_for;

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

schema_clean_test!(voice_design_request_schema, VoiceDesignRequest);
schema_clean_test!(generate_speech_request_schema, GenerateSpeechRequest);
schema_clean_test!(transcribe_request_schema, TranscribeRequest);
schema_clean_test!(audio_capture_request_schema, AudioCaptureRequest);
schema_clean_test!(
    record_and_transcribe_request_schema,
    RecordAndTranscribeRequest
);
schema_clean_test!(gallery_organize_request_schema, GalleryOrganizeRequest);
schema_clean_test!(gallery_search_request_schema, GallerySearchRequest);
schema_clean_test!(
    gallery_find_similar_request_schema,
    GalleryFindSimilarRequest
);
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
schema_clean_test!(
    gallery_record_generation_request_schema,
    GalleryRecordGenerationRequest
);
schema_clean_test!(gallery_lineage_request_schema, GalleryLineageRequest);
schema_clean_test!(gallery_reproduce_request_schema, GalleryReproduceRequest);
schema_clean_test!(generate_image_request_schema, GenerateImageRequest);
schema_clean_test!(transform_image_request_schema, TransformImageRequest);
schema_clean_test!(upscale_image_request_schema, UpscaleImageRequest);
schema_clean_test!(generate_video_request_schema, GenerateVideoRequest);
schema_clean_test!(expand_prompt_request_schema, ExpandPromptRequest);
schema_clean_test!(remove_background_request_schema, RemoveBackgroundRequest);
schema_clean_test!(apply_style_request_schema, ApplyStyleRequest);
schema_clean_test!(create_collage_request_schema, CreateCollageRequest);
schema_clean_test!(video_clip_request_schema, VideoClipRequest);
schema_clean_test!(video_to_gif_request_schema, VideoToGifRequest);
schema_clean_test!(image_to_video_request_schema, ImageToVideoRequest);
schema_clean_test!(video_add_caption_request_schema, VideoAddCaptionRequest);
schema_clean_test!(video_remix_request_schema, VideoRemixRequest);
schema_clean_test!(video_from_images_request_schema, VideoFromImagesRequest);
schema_clean_test!(video_concat_request_schema, VideoConcatRequest);
schema_clean_test!(video_caption_request_schema, VideoCaptionRequest);
schema_clean_test!(video_meme_request_schema, VideoMemeRequest);
schema_clean_test!(model_list_request_schema, ModelListRequest);
schema_clean_test!(model_info_request_schema, ModelInfoRequest);
schema_clean_test!(job_submit_request_schema, JobSubmitRequest);
schema_clean_test!(job_list_request_schema, JobListRequest);
schema_clean_test!(job_status_request_schema, JobStatusRequest);
schema_clean_test!(job_cancel_request_schema, JobCancelRequest);
schema_clean_test!(gallery_add_video_request_schema, GalleryAddVideoRequest);
schema_clean_test!(gallery_add_audio_request_schema, GalleryAddAudioRequest);
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
schema_clean_test!(generate_variants_request_schema, GenerateVariantsRequest);
schema_clean_test!(image_edit_region_request_schema, ImageEditRegionRequest);
schema_clean_test!(workflow_save_request_schema, WorkflowSaveRequest);
schema_clean_test!(workflow_load_request_schema, WorkflowLoadRequest);
schema_clean_test!(workflow_delete_request_schema, WorkflowDeleteRequest);
schema_clean_test!(video_info_request_schema, VideoInfoRequest);
schema_clean_test!(video_fetch_request_schema, VideoFetchRequest);
schema_clean_test!(audio_trim_request_schema, AudioTrimRequest);
schema_clean_test!(audio_concat_request_schema, AudioConcatRequest);
schema_clean_test!(
    video_extract_frames_request_schema,
    VideoExtractFramesRequest
);
