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
    ApplyStyleRequest, AudioCaptureRequest, CreateCollageRequest, DescribeImageRequest,
    ExpandPromptRequest, ExtractObjectRequest, FaceListRequest, FaceRegisterRequest,
    FaceRemoveRequest, FaceScanFolderRequest, FaceValidateRequest, GalleryAnalyzeRequest,
    GalleryFindSimilarRequest, GalleryLineageRequest, GalleryNameFaceRequest,
    GalleryOrganizeRequest, GalleryRecordGenerationRequest, GalleryRefreshRequest,
    GalleryReproduceRequest, GallerySearchRequest, GalleryTimelineRequest, GenerateImageRequest,
    GenerateSpeechRequest, GenerateVideoRequest, ImageToVideoRequest, RecordAndTranscribeRequest,
    RemoveBackgroundRequest, TranscribeRequest, TransformImageRequest, UpscaleImageRequest,
    VideoAddCaptionRequest, VideoCaptionRequest, VideoClipRequest, VideoConcatRequest,
    VideoFromImagesRequest, VideoMemeRequest, VideoRemixRequest, VideoToGifRequest,
    VoiceDesignRequest,
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
schema_clean_test!(extract_object_request_schema, ExtractObjectRequest);
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
