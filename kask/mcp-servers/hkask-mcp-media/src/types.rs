//! Types for hkask-mcp-media — MCP tool request input structs.
//!
//! All types implement `Deserialize + JsonSchema` for MCP tool input validation.
//! Transcript types (`TimedWord`/`TranscriptSegment`/`TranscriptBundle`) are
//! defined in this server's `transcript` module (`src/transcript.rs`) —
//! `hkask_types` carries no transcript types (verified by grep 2026-08-30).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Generation request types ─────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageRequest {
    pub prompt: String,
    pub image_size: Option<String>,
    pub num_images: Option<u32>,
    pub style: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransformImageRequest {
    pub prompt: String,
    pub image_url: String,
    pub strength: Option<f32>,
    pub style: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpscaleImageRequest {
    pub image_url: String,
    pub scale: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateVideoRequest {
    pub prompt: String,
    pub duration: Option<f32>,
    pub style: Option<String>,
}

// ── Image description ────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DescribeImageRequest {
    /// Image URL or gallery search result reference.
    pub image_url: String,
    /// Caption style: "descriptive", "artistic", "technical", "alt_text".
    pub style: Option<String>,
}

// ── Gallery request types ────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryOrganizeRequest {
    /// Absolute path to the gallery folder.
    pub path: String,
    /// Policy mode: "read-only", "copy-on-write", or "destructive".
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Whether to scan subdirectories recursively (default: true).
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Whether to automatically run AI analysis on newly added images (default: false).
    /// Vision LLM calls incur cost and latency. Only use when you want immediate searchability.
    #[serde(default)]
    pub auto_analyze: bool,
}

fn default_mode() -> String {
    "read-only".to_string()
}

/// Request to expand a short media prompt into a rich, detailed prompt
/// using a vision LLM (Fooocus "V2" pattern). Optionally applies a
/// style preset (default, anime, realistic, cinematic, minimal).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExpandPromptRequest {
    /// The short media prompt to expand (e.g., "a cat in space").
    pub prompt: String,
    /// Optional style preset to apply to the expanded prompt.
    /// Available: default, anime, realistic, cinematic, minimal.
    pub style: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GallerySearchRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub tag_types: Option<Vec<String>>,
    pub min_similarity: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryAnalyzeRequest {
    /// Which images to analyze: "new" (untagged only), "all" (everything), or "selection" (specific indices).
    #[serde(default = "default_analyze_mode")]
    pub mode: String,
    /// Specific image indices (only when mode="selection").
    pub image_indices: Option<Vec<usize>>,
    /// Which pipelines to run: "faces", "objects", "colors", "composition", "scene". Default: all.
    pub pipelines: Option<Vec<String>>,
    /// Maximum images to process (safety limit, default: 50).
    #[serde(default = "default_analyze_limit")]
    pub max_images: usize,
}

fn default_analyze_mode() -> String {
    "new".to_string()
}
fn default_analyze_limit() -> usize {
    50
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryRefreshRequest {
    /// Whether to scan subdirectories recursively (default: true).
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Whether to include face detection in the pipeline (default: false).
    /// Face tagging is a separate workflow — enable this only when you want to re-tag faces.
    #[serde(default)]
    pub include_faces: bool,
    /// Maximum images to process (safety limit, default: 50).
    #[serde(default = "default_analyze_limit")]
    pub max_images: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryNameFaceRequest {
    /// The face group number (from analyze results).
    pub face_group: usize,
    /// Human-readable name for this person.
    /// If face_id is provided, this is ignored — the name is pulled from the registry.
    pub name: Option<String>,
    /// Optional: face registry ID. When provided, the name is resolved from the registry
    /// instead of using the free-text name field.
    pub face_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FaceValidateRequest {
    /// Gallery image index to validate as a face reference.
    pub image_index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FaceRegisterRequest {
    /// Gallery image index of the validated reference portrait.
    pub image_index: usize,
    /// Person's first name.
    pub first_name: String,
    /// Person's last name.
    pub last_name: String,
    /// Skip validation and register directly as valid (default: false).
    /// Use when you know the image is a good reference but validation is overly strict.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FaceListRequest {
    /// Optional status filter: "valid", "rejected", or "pending".
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FaceRemoveRequest {
    /// Face registry ID to remove.
    pub face_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FaceScanFolderRequest {
    /// Absolute path to the folder of reference face images. Each image must
    /// have a YAML sidecar (e.g. `alice.jpg.yaml`) with `first_name`,
    /// `last_name`, and optional `notes`. Defaults to `mcp/media/faces/`.
    pub folder_path: Option<String>,
    /// Skip validation and register each image directly as valid (default: false).
    /// Use when you know the image is a good reference but validation is overly strict.
    #[serde(default)]
    pub force: bool,
}

// ── Educt transcript-store requests ─────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductStoreTranscriptRequest {
    /// The full TranscriptBundle JSON exactly as returned by transcribe_bundle.
    pub transcript: hkask_types::AnyJsonValue,
    /// Optional gallery asset ID linking this transcript to a gallery asset
    /// (the asset JOIN for recall by asset).
    pub gallery_asset_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductListTranscriptsRequest {
    /// Filter by the transcribed media's path (the bundle's audio_path).
    pub media_path: Option<String>,
    /// Filter by gallery asset ID.
    pub gallery_asset_id: Option<String>,
    /// Maximum transcripts to return (default 50, capped at 500).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductGetTranscriptRequest {
    pub transcript_id: String,
    /// Include stored layers in the response (default true).
    pub include_layers: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductDeleteTranscriptRequest {
    pub transcript_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductStoreLayerRequest {
    pub transcript_id: String,
    /// The layer as a tagged JSON object:
    /// {"kind": "speaker"|"paragraph"|"correction"|"highlight"|"edl", ...}.
    /// Validated against the transcript's word count before storage; a
    /// layer that fails validation is rejected with the named invariant.
    pub layer: hkask_types::AnyJsonValue,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductListLayersRequest {
    pub transcript_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductParagraphPassRequest {
    pub transcript_id: String,
    /// Optional model override (provider-prefixed, e.g. "OpenRouter/…").
    /// Default: HKASK_MEDIA_PASS_MODEL, then the classifier-tier default.
    pub model: Option<String>,
    /// Opt into the v2 structured-outputs mode (provider-enforced JSON
    /// Schema via chat_json). Default false — v1 (schema-in-prompt), the
    /// mode every catalog model serves.
    pub structured: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductSpeakerPassRequest {
    pub transcript_id: String,
    /// Optional model override (provider-prefixed, e.g. "OpenRouter/…").
    /// Default: HKASK_MEDIA_PASS_MODEL, then the classifier-tier default.
    pub model: Option<String>,
    /// Speaker attribution source: "audio" (default — an audio-capable
    /// model hears the recording; the scaffold's primary source) or
    /// "text" (the text-cue pass — works with every model, approximate).
    pub source: Option<String>,
    /// Opt into the v2 structured-outputs mode for the "text" source
    /// (provider-enforced JSON Schema). Not supported with source
    /// "audio" — the audio path is prompt-schema. Default false.
    pub structured: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductCorrectionPassRequest {
    pub transcript_id: String,
    /// Optional model override (provider-prefixed, e.g. "OpenRouter/…").
    /// Default: HKASK_MEDIA_PASS_MODEL, then the classifier-tier default.
    pub model: Option<String>,
    /// Opt into the v2 structured-outputs mode (provider-enforced JSON
    /// Schema via chat_json). Default false.
    pub structured: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductApplyCorrectionsRequest {
    pub transcript_id: String,
    /// Apply a specific correction layer by ID; defaults to the latest.
    pub layer_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductHighlightPassRequest {
    pub transcript_id: String,
    /// The natural-language selection request (e.g. "where he explains
    /// the Cinderella curve") — resolved to word ranges with labels.
    pub request: String,
    /// Optional model override (provider-prefixed, e.g. "OpenRouter/…").
    /// Default: HKASK_MEDIA_PASS_MODEL, then the classifier-tier default.
    pub model: Option<String>,
    /// Opt into the v2 structured-outputs mode (provider-enforced JSON
    /// Schema via chat_json). Default false.
    pub structured: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductEdlFromHighlightsRequest {
    pub transcript_id: String,
    /// Compose only highlights with this exact label; all highlights when
    /// omitted.
    pub label: Option<String>,
    /// Compose from a specific highlight layer by ID; defaults to the
    /// latest.
    pub layer_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductRenderEdlRequest {
    pub transcript_id: String,
    /// Render a specific EDL layer by ID; defaults to the latest.
    pub layer_id: Option<String>,
}

/// Lifecycle status of a face registry entry.
/// Stored as TEXT in SQLite; the storage layer accepts `&str` and this enum
/// implements `AsRef<str>` for a typed call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceStatus {
    Valid,
    Rejected,
    Pending,
}

impl FaceStatus {
    /// Returns true if this status is `Valid`.
    pub fn is_valid(self) -> bool {
        matches!(self, Self::Valid)
    }
}

impl std::fmt::Display for FaceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl AsRef<str> for FaceStatus {
    fn as_ref(&self) -> &str {
        match self {
            Self::Valid => "valid",
            Self::Rejected => "rejected",
            Self::Pending => "pending",
        }
    }
}

impl std::str::FromStr for FaceStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "valid" => Ok(Self::Valid),
            "rejected" => Ok(Self::Rejected),
            "pending" => Ok(Self::Pending),
            other => Err(format!("unknown face status: {}", other)),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryTimelineRequest {
    /// Time period: "year", "month", or "decade".
    #[serde(default = "default_period")]
    pub period: String,
    /// How many periods to include (default: 5).
    #[serde(default = "default_count")]
    pub count: usize,
    /// Max images per period (default: 3).
    #[serde(default = "default_per_period")]
    pub per_period: usize,
    /// Optional search terms to filter by.
    pub search_terms: Option<Vec<String>>,
}

fn default_period() -> String {
    "year".to_string()
}
fn default_count() -> usize {
    5
}
fn default_per_period() -> usize {
    3
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryFindSimilarRequest {
    /// Find images similar to this text description.
    pub text: Option<String>,
    /// Find images visually similar to this gallery image (uses its AI caption).
    pub image_index: Option<usize>,
    /// Maximum results to return (default: 5).
    #[serde(default = "default_similar_limit")]
    pub limit: usize,
    /// Minimum similarity threshold 0.0–1.0 (default: 0.3).
    #[serde(default = "default_similar_threshold")]
    pub min_similarity: f32,
}

fn default_similar_limit() -> usize {
    5
}
fn default_similar_threshold() -> f32 {
    0.3
}

// ── Image editing request types ──────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveBackgroundRequest {
    pub image_index: usize,
    pub new_bg_color: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApplyStyleRequest {
    pub image_index: usize,
    pub style_prompt: String,
    pub strength: Option<f32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCollageRequest {
    pub search_terms: Option<Vec<String>>,
    pub similar_to_index: Option<usize>,
    pub image_indices: Option<Vec<usize>>,
    #[serde(default = "default_max_items")]
    pub max_items: usize,
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default = "default_spacing")]
    pub spacing: u32,
    #[serde(default = "default_canvas")]
    pub canvas_size: String,
}

fn default_max_items() -> usize {
    6
}
fn default_layout() -> String {
    "grid".to_string()
}
fn default_spacing() -> u32 {
    8
}
fn default_canvas() -> String {
    "1200x900".to_string()
}

// ── Video request types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoClipRequest {
    pub video_url: String,
    pub start_sec: f32,
    pub end_sec: f32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoToGifRequest {
    pub video_url: String,
    pub start_sec: Option<f32>,
    pub duration_sec: Option<f32>,
    pub width: Option<u32>,
    pub fps: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImageToVideoRequest {
    pub image_index: usize,
    pub prompt: Option<String>,
    pub duration: Option<f32>,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoAddCaptionRequest {
    pub video_url: String,
    pub text: String,
    pub position: Option<String>,
    pub font_size: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoRemixRequest {
    pub video_url: String,
    pub start_sec: f32,
    pub end_sec: f32,
    pub caption_text: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoFromImagesRequest {
    pub image_indices: Vec<usize>,
    pub fps: Option<u32>,
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoConcatRequest {
    pub video_urls: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoCaptionRequest {
    pub video_url: String,
    pub style: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoExtractFramesRequest {
    /// URL of the video to extract frames from.
    pub video_url: String,
    /// Interval between frames in seconds (default 2.0).
    #[serde(default = "default_frame_interval")]
    pub interval_sec: f32,
    /// Maximum number of frames to extract (default 10).
    #[serde(default = "default_max_frames")]
    pub max_frames: u32,
}

fn default_frame_interval() -> f32 {
    2.0
}

fn default_max_frames() -> u32 {
    10
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoMemeRequest {
    /// Gallery image index to use as the meme base.
    pub image_index: usize,
    /// Text at the top of the image (Impact-style meme text).
    pub top_text: Option<String>,
    /// Text at the bottom of the image.
    pub bottom_text: Option<String>,
    /// Camera motion for the video (e.g., "slow zoom in", "dramatic pan right").
    #[serde(default = "default_motion")]
    pub motion: String,
    /// Video duration in seconds.
    pub duration: Option<f32>,
    /// Optional font path (TTF/OTF). Falls back to system DejaVu Sans Bold on Linux.
    pub font_path: Option<String>,
}

fn default_motion() -> String {
    "slow zoom in".to_string()
}

// ── Voice request types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VoiceDesignRequest {
    pub character_description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateSpeechRequest {
    pub text: String,
    pub voice_design: Option<String>,
}

// ── Audio request types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscribeRequest {
    /// URL or base64 data URI of the audio to transcribe.
    pub audio_url: String,
    /// Optional ISO 639-1 language code (e.g., "en", "ja").
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AudioCaptureRequest {
    /// Duration to record in seconds (max 3600 = 1 hour).
    pub duration_secs: f32,
    /// Optional output path. Defaults to temp directory with UUID filename.
    pub output_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordAndTranscribeRequest {
    /// Duration to record in seconds (max 3600 = 1 hour).
    pub duration_secs: f32,
    /// Optional ISO 639-1 language code for transcription.
    pub language: Option<String>,
}

// ── Generation lineage request types (WS-3) ─────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryRecordGenerationRequest {
    /// Index of the gallery image to attach lineage to (the generated asset
    /// must already be in the gallery — call gallery_organize / gallery_refresh
    /// after saving the generated file).
    pub image_index: usize,
    /// The media op that produced the image ("generate_image",
    /// "image_to_image", "upscale", "image_to_video", ...).
    pub op: String,
    /// The prompt used.
    pub prompt: Option<String>,
    /// The provider-specific model id used.
    pub model: Option<String>,
    /// The provider that produced the image ("deepinfra", "openrouter", ...).
    pub provider: Option<String>,
    /// The generation seed, if known.
    pub seed: Option<i64>,
    /// JSON string of the generation params (serialize the `MediaGenerateParams`
    /// used, so `gallery_reproduce` can replay them).
    pub params: Option<String>,
    /// Workflow id if the image came from a multi-step workflow.
    pub workflow_id: Option<String>,
    /// Index of the parent gallery image this was derived from (img2img /
    /// upscale / image_to_video), if any. Resolved to an image id internally.
    pub parent_image_index: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryLineageRequest {
    /// Index of the gallery image whose lineage to read.
    pub image_index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryReproduceRequest {
    /// Index of the gallery image to reproduce (re-runs its stored op + params).
    pub image_index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryDeleteImageRequest {
    /// Index of the gallery image to delete from the index.
    pub image_index: usize,
    /// Whether to also delete the file on disk (default: false — only removes
    /// the gallery index entry, leaving the file untouched).
    #[serde(default)]
    pub delete_file: bool,
}

// ── Model browser request types ─────────────────────────────────────────

/// Information about an available media generation model.
/// Returned by `model_list` and `model_info` tools. Maps to the OMC `Participant`
/// concept — the model/provider is a participant in the creation task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaModelInfo {
    /// Unique model identifier (the full prefixed name, e.g. "DeepInfra/black-forest-labs/FLUX-2-klein-4b").
    pub id: String,
    /// Human-readable model name without provider prefix.
    pub name: String,
    /// Provider name (e.g. "deepinfra", "openrouter").
    pub provider: String,
    /// Media modality: "image", "video", "audio", or "vision".
    pub modality: String,
    /// List of supported media operations (e.g. ["generate_image", "image_to_image"]).
    pub capabilities: Vec<String>,
    /// Whether this is the configured default model for its modality.
    pub is_default: bool,
    /// Optional human-readable description.
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ModelListRequest {
    /// Optional provider filter (e.g. "deepinfra", "openrouter").
    /// If omitted, lists models from all providers.
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ModelInfoRequest {
    /// The model id to look up (the full prefixed name returned by `model_list`).
    pub model_id: String,
}

// ── Generation job queue request types ──────────────────────────────────

/// A generation job record — tracks the lifecycle of an async media generation.
/// Maps to the OMC `Task` concept.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    /// Unique job identifier (UUID).
    pub id: String,
    /// The media operation (e.g. "generate_image", "generate_video").
    pub op: String,
    /// Job status: "queued", "running", "completed", "failed", "cancelled".
    pub status: String,
    /// ISO 8601 timestamp when the job was created.
    pub created_at: String,
    /// ISO 8601 timestamp when the job completed (set when status is completed/failed/cancelled).
    pub completed_at: Option<String>,
    /// The generation result (provider response JSON) on success.
    pub result: Option<serde_json::Value>,
    /// Error message on failure.
    pub error: Option<String>,
}

/// Request to submit a new async generation job.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobSubmitRequest {
    /// The media operation to execute (e.g. "generate_image", "generate_video",
    /// "image_to_image", "upscale", "generate_speech", "transcribe").
    pub op: String,
    /// JSON-serialized `MediaGenerateParams` (prompt, image_url, size, etc.).
    pub params: String,
}

/// Request to list generation jobs.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobListRequest {
    /// Optional status filter: "queued", "running", "completed", "failed", "cancelled".
    pub status: Option<String>,
    /// Maximum number of jobs to return (default: 20).
    pub limit: Option<usize>,
}

/// Request to get the status of a specific job.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobStatusRequest {
    /// The job id (returned by `job_submit`).
    pub job_id: String,
}

/// Request to cancel a running job.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobCancelRequest {
    /// The job id to cancel.
    pub job_id: String,
}

// ── Media import request types ──────────────────────────────────────────

/// Request to import a video file into the gallery index.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryAddVideoRequest {
    /// Absolute path to the video file.
    pub path: String,
    /// Optional: video width in pixels (0 if unknown).
    #[serde(default)]
    pub width: u32,
    /// Optional: video height in pixels (0 if unknown).
    #[serde(default)]
    pub height: u32,
}

/// Request to import an audio file into the gallery index.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryAddAudioRequest {
    /// Absolute path to the audio file.
    pub path: String,
}

// ── Asset detail request types ──────────────────────────────────────────

/// Request to get complete details for a gallery asset — record, tags,
/// lineage, and face associations in a single call.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryAssetDetailRequest {
    /// Index of the gallery asset to inspect.
    pub image_index: usize,
}

/// Request for `gallery_list_assets` — the panel/library data source.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct GalleryListAssetsRequest {
    /// 0-based offset (matches gallery index semantics — index `offset + i`
    /// in the result is the `image_index` other gallery tools accept).
    #[serde(default)]
    pub offset: usize,
    /// Page size (1–500, default 100).
    #[serde(default = "default_asset_list_limit")]
    pub limit: usize,
}

fn default_asset_list_limit() -> usize {
    100
}

// ── Album request types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryCreateAlbumRequest {
    /// Album name.
    pub name: String,
    /// Optional parent album ID for nested grouping.
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryMoveToAlbumRequest {
    /// Gallery image index to add to the album.
    pub image_index: usize,
    /// Album ID to add the image to.
    pub album_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryRemoveFromAlbumRequest {
    /// Gallery image index to remove from the album.
    pub image_index: usize,
    /// Album ID to remove the image from.
    pub album_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryDeleteAlbumRequest {
    /// Album ID to delete.
    pub album_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryListAlbumMembersRequest {
    /// Album ID to list members for.
    pub album_id: String,
}

// ── Variant generation request types ────────────────────────────────────

/// Request to generate N image variants from a single prompt. Each variant
/// is persisted individually with its own gallery entry and lineage record.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateVariantsRequest {
    /// The prompt for image generation.
    pub prompt: String,
    /// Number of variants to generate (1–10).
    #[serde(default = "default_variant_count")]
    pub count: u32,
    /// Optional image size (e.g. "1024x1024").
    pub image_size: Option<String>,
    /// Optional style preset.
    pub style: Option<String>,
}

fn default_variant_count() -> u32 {
    4
}

// ── Region-selective editing request types ──────────────────────────────

/// Request to apply a transform to a region of an image (inpaint/outpaint).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImageEditRegionRequest {
    /// URL or data URI of the source image to edit.
    pub image_url: String,
    /// Mask image (base64 data URI). White regions are edited; black regions
    /// are preserved. Must be the same dimensions as the source image.
    pub mask: String,
    /// The prompt describing the edit to apply in the masked region.
    pub prompt: String,
    /// Strength of the edit (0.0–1.0). Default: 0.85.
    pub strength: Option<f32>,
}

// ── Workflow composer request types ─────────────────────────────────────

/// Request to save a workflow definition.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowSaveRequest {
    /// Serialized workflow JSON (the step sequence and parameters).
    pub graph_json: String,
}

/// Request to load a saved workflow by ID.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowLoadRequest {
    /// The workflow ID (returned by `workflow_save`).
    pub workflow_id: String,
}

/// Request to delete a saved workflow.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WorkflowDeleteRequest {
    /// The workflow ID to delete.
    pub workflow_id: String,
}

// ── Video info request types ────────────────────────────────────────────

/// Request to probe a video file for metadata (duration, dimensions, codec, fps).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoInfoRequest {
    /// URL or path of the video to probe.
    pub video_url: String,
}

// ── Audio editing request types ─────────────────────────────────────────

/// Request to trim an audio file to specified start/end times.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AudioTrimRequest {
    /// URL or path of the audio file to trim.
    pub audio_url: String,
    /// Start time in seconds.
    pub start_sec: f32,
    /// End time in seconds.
    pub end_sec: f32,
}

/// Request to concatenate multiple audio files into one.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AudioConcatRequest {
    /// List of audio file URLs or paths to concatenate, in order.
    pub audio_urls: Vec<String>,
}

// ── Video fetch request types ───────────────────────────────────────────

/// Request to download a video from a URL (YouTube, Vimeo, direct file, etc.)
/// to local storage and index it in the gallery.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoFetchRequest {
    /// URL of the video to download (YouTube, Vimeo, direct file URL, etc.).
    pub url: String,
}
