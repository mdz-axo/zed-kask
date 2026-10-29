//! Types for hkask-mcp-media — MCP tool request input structs.
//!
//! All types implement `Deserialize + JsonSchema` for MCP tool input validation.
//! Transcript types (`TimedWord`/`TranscriptSegment`/`TranscriptBundle`) are
//! defined in this server's `transcript` module (`src/transcript.rs`) —
//! `hkask_types` carries no transcript types (verified by grep 2026-08-30).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GallerySearchRequest {
    pub query: String,
    /// Search mode: "tags" (default — fuzzy match against AI-generated tags;
    /// works without embeddings) or "semantic" (caption-embedding cosine
    /// similarity; requires gallery_analyze to have generated captions).
    pub mode: Option<String>,
    /// Semantic mode only: find images similar to this gallery image (by its
    /// caption embedding) instead of the query text.
    pub image_index: Option<usize>,
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
    /// Which pipelines to run: "faces", "objects", "colors", "composition", "scene". Default: "objects", "colors", "composition", "scene"; faces require explicit selection.
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
    /// Optional status filter: "valid" or "rejected".
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
    /// {"kind": "speaker"|"paragraph"|"correction"|"highlight"|"edl"|"realignment", ...}.
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
    /// Default: HKASK_MEDIA_PASS_MODEL, falling back to the media server's
    /// configured analysis model (HKASK_MEDIA_VISION_MODEL) — a chat-capable
    /// default. An explicit per-call override wins.
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
    /// Default: HKASK_MEDIA_PASS_MODEL, falling back to the media server's
    /// configured analysis model (HKASK_MEDIA_VISION_MODEL) — a chat-capable
    /// default. An explicit per-call override wins.
    pub model: Option<String>,
    /// Speaker attribution source: "audio" (default — an audio-capable
    /// model hears the recording; the scaffold's primary source) or
    /// "text" (the text-cue pass — works with every model, approximate).
    pub source: Option<String>,
    /// Opt into the v2 structured-outputs mode for the "text" source
    /// (provider-enforced JSON Schema). Not supported with source
    /// "audio" — the audio path is prompt-schema. Default false.
    pub structured: Option<bool>,
    /// Gated correction mode (plan §7 Phase 1b): instead of attributing
    /// from scratch, refine the latest stored speaker layer. The model
    /// proposes corrections to the current attribution and the
    /// deterministic locality gate accepts only contiguous changes of at
    /// most 5 words, reverting longer rewrites with a surfaced refusal.
    /// Requires source "text". Default false.
    pub correct_existing: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VoiceEnrollRequest {
    pub transcript_id: String,
    /// Inclusive immutable source-word range authorizing one named speaker.
    pub start_word: usize,
    pub end_word: usize,
    pub speaker_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VoiceListRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VoiceRemoveRequest {
    pub profile_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VoicePruneRequest {
    /// Delete profiles older than this many days unless a human-confirmed
    /// match refreshed them within the same retention window. Maximum 365000.
    pub max_age_days: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductRetranscribeSectionRequest {
    pub transcript_id: String,
    /// Inclusive source word range to re-transcribe. Indices anchor the
    /// immutable source words, like every layer — working-view selections
    /// must be translated to source indices first (the reanchored index
    /// map).
    pub start_word: usize,
    pub end_word: usize,
    /// Optional ISO 639-1 language hint for the STT — same semantics as
    /// transcribe_bundle.
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductCorrectionPassRequest {
    pub transcript_id: String,
    /// Optional model override (provider-prefixed, e.g. "OpenRouter/…").
    /// Default: HKASK_MEDIA_PASS_MODEL, falling back to the media server's
    /// configured analysis model (HKASK_MEDIA_VISION_MODEL) — a chat-capable
    /// default. An explicit per-call override wins.
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
pub struct EductRealignTranscriptRequest {
    pub transcript_id: String,
    /// Re-anchor a specific correction layer by ID; defaults to the latest.
    pub layer_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductHighlightPassRequest {
    pub transcript_id: String,
    /// The natural-language selection request (e.g. "where he explains
    /// the Cinderella curve") — resolved to word ranges with labels.
    pub request: String,
    /// Optional model override (provider-prefixed, e.g. "OpenRouter/…").
    /// Default: HKASK_MEDIA_PASS_MODEL, falling back to the media server's
    /// configured analysis model (HKASK_MEDIA_VISION_MODEL) — a chat-capable
    /// default. An explicit per-call override wins.
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductExportRequest {
    pub transcript_id: String,
    /// Export format: "srt" (captions from the word timings, cues split
    /// at sentence punctuation), "highlights_csv" (every stored highlight
    /// with time ranges), or "corpus_text" (the rendered transcript for
    /// corpus ingestion — repository-wide semantic search by composition).
    pub format: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EductLocateRequest {
    pub transcript_id: String,
    /// The passage to locate — quote the rendered form exactly (punctuation
    /// included). All word-aligned matches are returned with their time
    /// ranges; ambiguity is surfaced as all candidates, never a guess.
    pub text: String,
}

/// Lifecycle status of a face registry entry, set at registration by the
/// validation result. Stored as TEXT in SQLite; the storage layer accepts
/// `&str` and this enum implements `AsRef<str>` for a typed call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceStatus {
    Valid,
    Rejected,
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

// ── Image editing request types ──────────────────────────────────────────

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
pub struct VideoAddCaptionRequest {
    pub video_url: String,
    pub text: String,
    pub position: Option<String>,
    pub font_size: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoConcatRequest {
    pub video_urls: Vec<String>,
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

// ── Audio request types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscribeBundleRequest {
    /// URL or base64 data URI of the audio to transcribe.
    pub audio_url: String,
    /// Optional ISO 639-1 language code (e.g., "en", "ja").
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscribeAndStoreRequest {
    /// URL or local path of the audio/video to transcribe (local paths and
    /// file:// URIs naming existing files are read directly).
    pub audio_url: String,
    /// Optional ISO 639-1 language code (e.g., "en", "ja").
    pub language: Option<String>,
    /// Optional gallery asset id to key the transcript record to.
    pub gallery_asset_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AudioCaptureRequest {
    /// Duration to record in seconds (max 3600 = 1 hour).
    pub duration_secs: f32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordAndTranscribeRequest {
    /// Duration to record in seconds (max 3600 = 1 hour).
    pub duration_secs: f32,
    /// Optional ISO 639-1 language code for transcription.
    pub language: Option<String>,
}

// ── Historical lineage request types ─────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryLineageRequest {
    /// Index of the gallery image whose lineage to read.
    pub image_index: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryDeleteImageRequest {
    /// Index of the gallery image to delete from the index (positional —
    /// resolves against the active gallery). Supply exactly one of
    /// image_index or image_id.
    #[serde(default)]
    pub image_index: Option<usize>,
    /// Stable asset id — the identity that survives reordering, rescan, and
    /// gallery switches. Preferred over the positional index for callers
    /// holding a listing record (the panel's delete action).
    #[serde(default)]
    pub image_id: Option<String>,
    /// Whether to also delete the file on disk (default: false — only removes
    /// the gallery index entry, leaving the file untouched). Requires destructive
    /// gallery mode; read-only and copy-on-write modes preserve originals.
    #[serde(default)]
    pub delete_file: bool,
}

// ── Model browser request types ─────────────────────────────────────────

/// Information about an available research media model.
/// Returned by `model_list` and `model_info` tools. Maps to the OMC `Participant`
/// concept — the model/provider is a participant in the creation task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MediaModelInfo {
    /// Unique model identifier (the full prefixed name, e.g. "Provider/model").
    pub id: String,
    /// Human-readable model name without provider prefix.
    pub name: String,
    /// Provider name (e.g. "deepinfra", "openrouter").
    pub provider: String,
    /// Media modality: "image", "video", "audio", or "vision".
    pub modality: String,
    /// Supported research operations, such as transcription or visual analysis.
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

// ── Research job queue request types ──────────────────────────────────

/// A research job record — tracks the lifecycle of an async media operation.
/// Maps to the OMC `Task` concept.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    /// Unique job identifier (UUID).
    pub id: String,
    /// The research operation, such as "transcribe" or "video_clip".
    pub op: String,
    /// Job status: "queued", "running", "cancelling", "completed", "failed", or "cancelled".
    pub status: String,
    /// ISO 8601 timestamp when the job was created.
    pub created_at: String,
    /// ISO 8601 timestamp when the job completed (set when status is completed/failed/cancelled).
    pub completed_at: Option<String>,
    /// The slim durable publication result on success; provider payload bytes are excluded.
    pub result: Option<serde_json::Value>,
    /// Error message on failure.
    pub error: Option<String>,
}

/// Strict `job_list` response consumed by the media panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobListPayload {
    pub jobs: Vec<JobRecord>,
    pub total: usize,
    pub limit: usize,
    pub has_more: bool,
    pub history_scope: String,
    pub restart_behavior: String,
}

/// Request to submit a new async research media job.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobSubmitRequest {
    /// The research operation: "transcribe", "audio_trim", "audio_concat",
    /// "video_clip", or "video_concat".
    pub op: String,
    /// JSON-serialized request for the selected research operation, with its source fields.
    pub params: String,
}

/// Request to list research media jobs.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct JobListRequest {
    /// Optional status filter: "queued", "running", "cancelling", "completed", "failed", or "cancelled".
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

/// Request for gallery_add_media: import a video or audio file into the
/// gallery index (the former gallery_add_video / gallery_add_audio pair,
/// merged — the bodies differed only in media type and dimensions).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryAddMediaRequest {
    /// Absolute path to the media file.
    pub path: String,
    /// Media kind: "video" or "audio".
    pub media_type: String,
    /// Video width in pixels (video only; optional metadata).
    pub width: Option<u32>,
    /// Video height in pixels (video only; optional metadata).
    pub height: Option<u32>,
}

// ── Asset detail request types ──────────────────────────────────────────

/// Request to get complete details for a gallery asset — record, tags,
/// lineage, and face associations in a single call.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GalleryAssetDetailRequest {
    /// Active asset position. Supply exactly one of image_index or image_id.
    pub image_index: Option<usize>,
    /// Stable asset ID, including retained missing records in the active gallery.
    pub image_id: Option<String>,
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

// ── YouTube search and video fetch request types ────────────────────────

/// Request to search YouTube metadata through SerpApi.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct YoutubeSearchRequest {
    /// YouTube search query.
    pub query: String,
    /// Maximum number of video results to return (default 20, maximum 50).
    pub max_results: Option<u32>,
}

/// Request to download a video from a URL (YouTube, Vimeo, direct file, etc.)
/// to local storage and index it in the gallery.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct VideoFetchRequest {
    /// URL of the video to download (YouTube, Vimeo, direct file URL, etc.).
    pub url: String,
}

// ── Media layer requests ──────────────────────────────────────────────

/// Provenance for a layer being stored: which model, which prompt
/// template. `created_at` is stamped server-side at store time.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayersProvenanceRequest {
    /// The model that produced the layer (e.g. an inference-catalog label,
    /// or "user" for researcher-authored annotations).
    pub model: String,
    /// The prompt template that produced it (template name; for
    /// researcher-authored layers, a short description of the authoring path).
    pub prompt_template: String,
}

/// One item of a layer being stored — the wire form. `payload` is arbitrary
/// JSON in the `AnyJsonValue` JSON-text transport.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayersStoreItemRequest {
    /// Range start in source-PTS seconds.
    pub start_sec: f64,
    /// Range end in source-PTS seconds; omit for a point-in-time item.
    pub end_sec: Option<f64>,
    /// The text a lexical search matches — must be nonblank.
    pub searchable_text: String,
    /// Arbitrary structured payload (labels, boxes, confidences, OCR text).
    pub payload: hkask_types::AnyJsonValue,
    /// Gallery asset ids of frames this item references; each must exist.
    pub frame_asset_ids: Option<Vec<String>>,
}

/// Request to store one media layer over a media file.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayersStoreRequest {
    /// Absolute local path (or URL) of the source media the layer annotates.
    pub media_path: String,
    /// Optional gallery asset ID linking the layer to a gallery asset.
    pub gallery_asset_id: Option<String>,
    /// Layer kind: "shot", "semantic", "object", "ocr", "audio_event",
    /// "researcher", or "edl" (closed set — unknown values are rejected
    /// naming the valid kinds).
    pub kind: crate::layer_store::MediaLayerKind,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Which model and prompt template produced the layer.
    pub provenance: LayersProvenanceRequest,
    /// The layer's items, keyed in source-PTS seconds and validated against
    /// the ffprobe-anchored media duration.
    pub items: Vec<LayersStoreItemRequest>,
}

/// Request to list stored media layers.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayersListRequest {
    /// Filter by the annotated media's path.
    pub media_path: Option<String>,
    /// Filter by layer kind (same closed set as layers_store).
    pub kind: Option<crate::layer_store::MediaLayerKind>,
    /// Maximum layers to return (default 50, capped at 500).
    pub limit: Option<u32>,
}

/// Request to load one stored media layer with its full items.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayersGetRequest {
    /// The layer ID returned by layers_store.
    pub layer_id: String,
}

/// Request to detect shot boundaries as a deterministic layer over a video
/// file.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LayersShotPassRequest {
    /// Absolute local path (or URL) of the source video to analyze.
    pub media_path: String,
    /// Optional gallery asset ID linking the layer to a gallery asset.
    pub gallery_asset_id: Option<String>,
    /// Scene score a frame must exceed to count as a cut, in (0.0, 1.0]
    /// (ffmpeg's conventional default 0.3 when omitted).
    pub threshold: Option<f64>,
    /// Minimum shot length in seconds — a boundary closer than this to the
    /// previous one is suppressed (default 0.5; 0.0 disables the guard).
    pub min_shot_length: Option<f64>,
}
