//! Types for hkask-mcp-media — request types and transcript data models.
//!
//! - Request types: MCP tool input structs (Deserialize + JsonSchema)
//! - Transcript types: synchronized audio + word-level timed transcript

pub mod transcript;

use schemars::JsonSchema;
use serde::Deserialize;

// ── Generation request types ─────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateImageRequest {
    pub prompt: String,
    pub image_size: Option<String>,
    pub num_images: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransformImageRequest {
    pub prompt: String,
    pub image_url: String,
    pub strength: Option<f32>,
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
}

/// Workflow execution request — accepts a Fal-compatible workflow JSON string.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecuteWorkflowRequest {
    /// A Fal-compatible workflow JSON string with input, run, and display nodes.
    /// Run nodes support "mode": "sync" (default) or "mode": "queue" for long-running models.
    pub workflow: String,
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
    /// `last_name`, and optional `notes`. Defaults to `~/.hkask/faces/`.
    pub folder_path: Option<String>,
    /// Skip validation and register each face directly as valid (default: false).
    /// Use when you know the images are good references but validation is overly strict.
    #[serde(default)]
    pub force: bool,
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
pub struct ExtractObjectRequest {
    /// Gallery image index containing the object.
    pub image_index: usize,
    /// Description of the object to extract (e.g., "the golden retriever on the left").
    pub object_description: String,
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
