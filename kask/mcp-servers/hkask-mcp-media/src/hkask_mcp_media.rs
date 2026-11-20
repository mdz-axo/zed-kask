// `forbid(unsafe_code)` in production builds; test builds allow it for
// `std::env::set_var` (artifacts-dir isolation in the persistence tests) —
// the same `cfg_attr(not(test), …)` pattern as hkask-inference.
#![cfg_attr(not(test), forbid(unsafe_code))]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Media — source-media research and curation.
//!
//! Tool families retain source discovery, gallery analysis and annotations,
//! source-linked audio/video editing, transcription, and transcript-backed reels.
//! Historical lineage remains readable; creative generation is not a research job.

mod assets;
mod error;
mod faces;
mod gallery;
mod images;
pub mod jobs;
pub use hkask_media::{
    media_block, omc, transcript, transcript_layers, transcript_retranscribe, transcript_select,
};
pub(crate) mod ontology;
mod templates;
#[cfg(test)]
mod test_support;
pub mod video;

pub use error::{
    MediaError, classify_embedding_error, classify_inference_error, map_gallery_store_error,
    map_image_open_error, map_media_error,
};

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use crate::gallery::{GalleryMode, GalleryStore};
use gallery::GalleryState;
use gallery::vision::{self};
use hkask_mcp_server::server::{McpToolError, execute_tool, validate_tool_url_with_dns};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::InferencePort;

use crate::transcript::{TimedWord, TranscriptBundle, TranscriptSegment};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
pub mod tools;

// Extracted implementation modules (C2 split). Re-exported so the
// `use crate::*` imports in tools/ keep resolving without churn.
pub(crate) use faces::default_face_folder;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Maximum image file size for base64 encoding (32 MiB). Gallery images larger
/// than this are rejected to prevent OOM — the image is read into memory and
/// base64-encoded, which triples the size. A multi-GB image would exhaust the
/// process's address space.
const MAX_IMAGE_READ_BYTES: u64 = 32 * 1024 * 1024;

/// Validate a bounded collection request before any allocation or external work.
pub(crate) fn validate_item_count(
    field: &str,
    count: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), McpToolError> {
    if count < minimum || count > maximum {
        return Err(McpToolError::invalid_argument(format!(
            "{field} must contain between {minimum} and {maximum} items; received {count}"
        )));
    }
    Ok(())
}

use video::FfmpegRunner;
use video::YtDlpRunner;

/// Serializes tests that mutate `HKASK_ARTIFACTS_DIR` — the env var is
/// process-global and parallel test threads race on it (each test's
/// tempdir must stay alive until its own assertions finish).
#[cfg(test)]
pub(crate) static ARTIFACTS_ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

// ── Model configuration ───────────────────────────────────────────────

/// Configured media models — resolved from env vars (injected from the
/// visible kask settings). NO hidden constants: every accessor returns
/// `None` when unset and callers fail visibly naming the env var (the
/// operator's no-hidden-models spec).
pub mod models {
    pub const STT_ENV: &str = "HKASK_MEDIA_STT_MODEL";
    /// The cross-check STT fleet (transcript-reel Phase 1b) — the
    /// independent ASR voices the transcribe tools' `model` override
    /// addresses per call, listed by `model_list`. Comma-joined.
    pub const CROSS_CHECK_STT_ENV: &str = "HKASK_MEDIA_CROSS_CHECK_STT_MODELS";
    pub const VISION_ENV: &str = "HKASK_MEDIA_VISION_MODEL";
    pub const PASS_ENV: &str = "HKASK_MEDIA_PASS_MODEL";
    pub const SOUND_ANNOTATION_ENV: &str = "HKASK_MEDIA_SOUND_ANNOTATION_MODEL";
    pub const TAGGING_ENV: &str = "HKASK_MEDIA_TAGGING_MODEL";

    /// Resolve a configured media model from its env var — `None` when
    /// unset (the operator's no-hidden-models spec: no constant fallback;
    /// callers fail visibly naming the env var).
    fn configured(env_key: &str) -> Option<String> {
        std::env::var(env_key).ok().filter(|m| !m.trim().is_empty())
    }

    pub fn stt_model() -> Option<String> {
        configured(STT_ENV)
    }
    /// The configured cross-check STT fleet — empty when unset (the
    /// cross-check's second-model leg then reports the fleet unconfigured
    /// rather than guessing a model).
    pub fn cross_check_stt_models() -> Vec<String> {
        configured(CROSS_CHECK_STT_ENV)
            .map(|fleet| {
                fleet
                    .split(',')
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn vision_model() -> Option<String> {
        configured(VISION_ENV)
    }
    pub fn pass_model() -> Option<String> {
        configured(PASS_ENV)
    }
    /// The audio passes' role model (settings > kask > models) — audio-only;
    /// `layers_audio_pass` resolves per-call → this setting → NotConfigured,
    /// never via the shared pass-model env.
    pub fn sound_annotation_model() -> Option<String> {
        configured(SOUND_ANNOTATION_ENV)
    }
    /// The grounded-tagging role model (settings > kask > models) — the
    /// grounded vision pass resolves per-call → this setting → the vision
    /// default.
    pub fn tagging_model() -> Option<String> {
        configured(TAGGING_ENV)
    }
}

/// Whether a tool input names a local media file rather than a network URL.
///
/// The SSRF URL validator rejects anything without an `http(s)` scheme —
/// including the local files this server's own tools produce (`video_fetch`
/// downloads, `audio_capture` recordings, gallery asset paths). Those are
/// the primary inputs of the local analysis pipelines, and the downstream
/// layers (ffmpeg, `download_audio_bytes`, `load_image_as_png_base64`)
/// read local paths directly, so an existing local file is a valid input
/// that simply is not a URL. Anything that is not an existing local file
/// falls through to URL validation, which fails closed on malformed input.
pub(crate) fn is_local_media_path(input: &str) -> bool {
    if input.starts_with("file://") {
        return true;
    }
    if input.contains("://") || input.starts_with("data:") {
        return false;
    }
    true
}

/// A local input path that does not exist is the caller's error, not a
/// server fault — reject it as `not_found` before ffmpeg/ffprobe/provider
/// calls surface it as `internal`/`unavailable` (mcp-tool-review M-02: a
/// nonexistent local file previously classified as a server error kind —
/// live probes 2026-10-04 returned `[internal] ffprobe … No such file or
/// directory` and `[unavailable] Connection error: audio file read
/// failed`). Remote inputs (URLs, data URIs) pass through untouched —
/// their existence is the remote side's to discover.
pub(crate) fn ensure_local_input_exists(input: &str) -> Result<(), McpToolError> {
    if !is_local_media_path(input) {
        return Ok(());
    }
    let path = input.strip_prefix("file://").unwrap_or(input);
    hkask_mcp_server::ensure_existing_file(path, "input file")
}

/// Lock-free snapshot of gallery state — safe to hold across .await points.
struct GalleryAccess {
    gallery_id: String,
    image_count: u64,
    root_path: PathBuf,
    mode: String,
    total_size_bytes: u64,
}

hkask_mcp_server::mcp_server!(
    pub struct MediaServer {
        /// Inference port for vision/chat/embed calls routed through zed's
        /// LanguageModelRegistry via the IPC bridge. Transcription is
        /// handled child-locally: `media_generate("transcribe", ...)`
        /// dispatches to the local `MediaRouter` with this process's
        /// env-injected keys — no IPC
        /// round-trip. `generate_vision` handles
        /// face/object/color/composition/caption analysis; `embed` handles
        /// the gallery embedding path.
        pub vision_port: Arc<dyn InferencePort>,
        pub gallery_state: Arc<Mutex<Option<GalleryState>>>,
        pub gallery_store: Arc<GalleryStore>,
        pub template_env: minijinja::Environment<'static>,
        pub ffmpeg: FfmpegRunner,
        /// yt-dlp runner for video downloading (video_fetch tool).
        pub ytdlp: YtDlpRunner,
        /// In-memory research job store for async job tracking (OMC `Task`).
        pub job_store: jobs::JobStore,
        /// SerpApi credential for structured YouTube discovery and metadata.
        /// Binary media retrieval remains the separate `YtDlpRunner` concern.
        pub serpapi_key: Option<String>,
        /// Protected voice-profile storage and lazy local encoder state.
        pub voice: voice_profiles::VoiceProfileRuntime,
    } = crate::ontology::annotate;
);

pub mod layer_pass;
pub mod layer_store;
pub mod redaction;
pub mod track_stitcher;
pub mod transcript_export;
pub mod transcript_pass;
pub mod transcript_review;
pub mod transcript_store;
pub mod types;
pub mod voice_encoder;
pub mod voice_profiles;
use types::*;

/// Read an image file with a size cap to prevent OOM. Gallery images are
/// read into memory and base64-encoded (which triples the size), so a
/// multi-GB image would exhaust the process's address space. Rejects files
/// larger than `MAX_IMAGE_READ_BYTES` before reading.
fn read_image_capped(path: &str) -> Result<Vec<u8>, MediaError> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| MediaError::Io(format!("Failed to stat image: {e}")))?;
    let size = metadata.len();
    if size > MAX_IMAGE_READ_BYTES {
        return Err(MediaError::Io(format!(
            "Image file is {size} bytes ({:.1} MiB) — exceeds the {} byte limit; \
             use a smaller image or increase MAX_IMAGE_READ_BYTES",
            size as f64 / (1024.0 * 1024.0),
            MAX_IMAGE_READ_BYTES
        )));
    }
    std::fs::read(path).map_err(|e| MediaError::Io(format!("Failed to read image: {e}")))
}

/// Compute normalized Levenshtein similarity between two strings.
/// Returns 1.0 for identical strings, 0.0 for completely different.
fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 && b_len == 0 {
        return 1.0;
    }
    if a_len == 0 || b_len == 0 {
        return 0.0;
    }

    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_chars: Vec<char> = a_lower.chars().collect();
    let b_chars: Vec<char> = b_lower.chars().collect();

    // Space-optimized DP: only keep two rows
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    let distance = prev[b_len];
    let max_len = a_len.max(b_len) as f64;
    1.0 - (distance as f64 / max_len)
}

#[cfg(test)]
mod levenshtein_tests {
    use super::*;

    #[test]
    fn identical_strings() {
        assert!((levenshtein_similarity("sunset", "sunset") - 1.0).abs() < 0.001);
    }

    #[test]
    fn completely_different() {
        let sim = levenshtein_similarity("sunset", "xyzzy");
        assert!(sim < 0.3, "expected low similarity, got {}", sim);
    }

    #[test]
    fn case_insensitive() {
        assert!((levenshtein_similarity("Sunset", "sunset") - 1.0).abs() < 0.001);
    }

    #[test]
    fn typo_tolerant() {
        let sim = levenshtein_similarity("sunset", "sunest");
        assert!(sim > 0.6, "expected high similarity for typo, got {}", sim);
    }

    #[test]
    fn empty_strings() {
        assert!((levenshtein_similarity("", "") - 1.0).abs() < 0.001);
        assert!((levenshtein_similarity("sunset", "") - 0.0).abs() < 0.001);
        assert!((levenshtein_similarity("", "sunset") - 0.0).abs() < 0.001);
    }
}

impl MediaServer {
    /// Fail fast before a direct heavy RPC reaches provider, FFmpeg, or analysis work.
    pub(crate) fn admit_heavy_operation(
        &self,
    ) -> Result<crate::jobs::HeavyOperationPermit, McpToolError> {
        self.job_store.admit_direct().map_err(|error| match error {
            crate::jobs::JobStoreError::Overloaded { .. } => {
                McpToolError::rate_limited(error.to_string())
            }
            crate::jobs::JobStoreError::AdmissionClosed => {
                McpToolError::internal(error.to_string())
            }
            _ => McpToolError::internal(format!(
                "unexpected direct media admission failure: {error}"
            )),
        })
    }

    /// Lock the gallery and extract essential state. Drops the lock before
    /// returning, so the result is safe to hold across .await points.
    fn access_gallery(&self) -> Result<GalleryAccess, MediaError> {
        let guard = self
            .gallery_state
            .lock()
            .map_err(|e| MediaError::Io(format!("Gallery state lock error: {}", e)))?;
        let state = guard.as_ref().ok_or(MediaError::GalleryNotInitialized)?;
        let gallery_id = state
            .gallery_id
            .clone()
            .ok_or(MediaError::GalleryNotInitialized)?;
        let record = self.gallery_store.get(&gallery_id)?;
        let access = GalleryAccess {
            gallery_id,
            image_count: record.image_count as u64,
            root_path: PathBuf::from(record.root_path),
            mode: record.mode,
            total_size_bytes: record.total_size_bytes,
        };
        Ok(access)
    }

    /// Return the ffmpeg runner, or the missing-binary refusal naming what
    /// to install (`unavailable`: see `map_media_error`).
    fn require_ffmpeg(&self) -> Result<&FfmpegRunner, McpToolError> {
        if self.ffmpeg.available {
            Ok(&self.ffmpeg)
        } else {
            Err(map_media_error(MediaError::FfmpegUnavailable))
        }
    }

    /// Return the yt-dlp runner, or the missing-binary refusal.
    fn require_yt_dlp(&self) -> Result<&YtDlpRunner, McpToolError> {
        if self.ytdlp.available {
            Ok(&self.ytdlp)
        } else {
            Err(map_media_error(MediaError::YtDlpUnavailable))
        }
    }

    /// Return the configured vision model (`HKASK_MEDIA_VISION_MODEL`) or a
    /// fail-visible `permission_denied` error naming the env var.
    fn require_vision(&self) -> Result<(String, String), McpToolError> {
        self.resolve_vision_model().ok_or_else(|| {
            McpToolError::permission_denied(format!(
                "no vision model configured — set {} (injected from kask.models.media_vision_model); \
                 kask never falls back to a hidden code constant or catalog guess",
                models::VISION_ENV
            ))
        })
    }

    /// Embed a single text via the inference port's `embed` method.
    ///
    /// Resolves the embedding model from `HKASK_EMBEDDING_MODEL` (injected
    /// from the kask settings chain; code default
    /// `OpenRouter/qwen/qwen3-embedding-8b`) and returns the first (only)
    /// embedding vector. Used by gallery similarity search, which compares
    /// the query and candidate-caption embeddings within one request —
    /// width-agnostic, so the model's native width is used (no
    /// `dimensions` request).
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, McpToolError> {
        // Fail-visible (the operator's no-hidden-models spec): no configured
        // embedding model is a typed error naming the setting — never a
        // hidden constant.
        let model = hkask_inference::model_constants::embedding_model().ok_or_else(|| {
            McpToolError::permission_denied(
                "no embedding model configured — set \
                     kask.models.embedding_model (injected as \
                     HKASK_EMBEDDING_MODEL); kask never falls back to a \
                     hidden code constant",
            )
        })?;
        let batch = self
            .vision_port
            .embed(&model, std::slice::from_ref(&text.to_string()))
            .await
            .map_err(|e| classify_embedding_error("Embedding failed", e))?;
        batch
            .vectors
            .into_iter()
            .next()
            .ok_or_else(|| McpToolError::unavailable("Embedding model returned an empty response"))
    }

    /// Render a Jinja2 prompt template with the given variables.
    fn render_prompt(&self, name: &str, vars: &HashMap<&str, &str>) -> Result<String, MediaError> {
        templates::render(&self.template_env, name, vars)
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// YAML sidecar format for `face_scan_folder`.
/// Maps a reference image file to a person name.
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif"];

// ── Canonical tool-name list ─────────────────────────────────────────────
//
// Generated by `build.rs` from `pub async fn *` signatures in `src/**/*.rs`
// (excluding `run`). The single source of truth for the media server's tool
// surface — consumers (e.g. `media_panel`) re-export this const to verify
// Steer prompt tool advertisements.
include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

impl MediaServer {
    #[doc(hidden)]
    pub fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::gallery_router()
            + Self::processing_router()
            + Self::audio_router()
            + Self::models_router()
            + Self::jobs_router()
            + Self::educt_router()
            + Self::layers_passes_router()
            + Self::layers_search_router()
            + Self::layers_export_router()
            + Self::layers_store_router()
            + Self::layers_edl_router()
            + Self::voice_router()
            + Self::youtube_router()
            + Self::fetch_image_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for MediaServer {}

#[cfg(test)]
mod tool_surface_tests;

/// Run the media MCP server (used by binary target).
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    // Resolve the inference port — chat/vision/embed/list_models route
    // through zed's LanguageModelRegistry via the IPC bridge when
    // `HKASK_INFERENCE_SOCKET` is set; media generation
    // (`media_generate`) is child-local via the `MediaRouter` with
    // env-injected keys, bridge or not.
    //
    // Fallback behavior when the IPC bridge is unavailable:
    // - `generate_with_model` / `embed` → `DirectEmbeddingPort` (Ollama)
    // - `generate_vision` / `list_models` / `generate_batch` → socket-named
    //   error (no direct fallback — these require the bridge)
    let vision_port = hkask_inference::resolve_inference_port().await;

    // Build the GalleryStore. Durable (file-backed SQLite) at
    // `{kask_data_dir}/mcp/media/gallery.db` (D28 — Standardized Artifact
    // Storage), or override via `HKASK_MEDIA_DB`. Unlike kata-kanban there is
    // no in-memory fallback: a gallery DB open failure aborts startup. The
    // fallback silently degraded every subsequent tool call to "gallery
    // empty" — a broken feedback loop (the operator cannot distinguish a DB
    // outage from a genuinely empty gallery, and re-organizing against the
    // throwaway in-memory DB loses tag/face/lineage metadata). The file DB is
    // unencrypted (gallery metadata is not a secret), so it does NOT use
    // `HKASK_DB_PASSPHRASE` — avoiding leaking the global SQLCipher key to
    // this child process. Schema is initialized by `from_driver()`.
    let gallery_store = {
        let db_path_str = hkask_mcp_server::resolve_server_db_path(
            "HKASK_MEDIA_DB",
            &hkask_types::agent_paths::mcp_server_db("media", "gallery"),
        );
        let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = {
            let pool = SqliteDriver::file_pool(&db_path_str).map_err(|e| {
                tracing::error!(
                    target: "hkask.mcp.media",
                    path = %db_path_str,
                    error = %e,
                    "Gallery DB open failed — refusing to start with an ephemeral \
                     in-memory gallery (metadata would be lost on restart)"
                );
                hkask_mcp_server::McpError::UnexpectedResponse {
                    context: "gallery DB open".to_string(),
                    detail: format!("{db_path_str}: {e}"),
                }
            })?;
            tracing::info!(
                target: "hkask.mcp.media",
                path = %db_path_str,
                "Gallery store using durable file DB"
            );
            Arc::new(SqliteDriver::new_labeled(pool, db_path_str.as_str()))
        };
        match GalleryStore::from_driver(driver) {
            Ok(store) => {
                tracing::info!(target: "hkask.mcp.media", "Gallery store initialized");
                Arc::new(store)
            }
            Err(e) => {
                tracing::error!(target: "hkask.mcp.media", error = %e, "Failed to create GalleryStore");
                return Err(e.into());
            }
        }
    };

    hkask_mcp_server::run_server(
        "hkask-mcp-media",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            let template_env = templates::create_env().map_err(|e| {
                hkask_mcp_server::McpError::UnexpectedResponse {
                    context: "media template registration".to_string(),
                    detail: e.to_string(),
                }
            })?;
            let voice_path = hkask_types::agent_paths::resolve_under_data_dir(
                &hkask_types::agent_paths::mcp_server_db("media", "voice_profiles"),
            );
            let voice = match hkask_mcp_server::resolve_db_passphrase(&ctx.credentials) {
                Ok(passphrase) => voice_profiles::VoiceProfileRuntime::open(
                    &voice_path.to_string_lossy(),
                    &passphrase,
                ),
                Err(error) => {
                                    voice_profiles::VoiceProfileRuntime::missing_credential(error.to_string())
                                }
            };
            // Re-activate the operator's most-recently-organized gallery
            // so a server restart does not leave the gallery dormant until
            // a manual gallery_organize call (the gallery papercut: two
            // restarts in two days each required one re-organize call).
            // The durable record is already in the gallery DB; only the
            // in-memory activation is per-process.
            let gallery_state = match gallery_store.most_recently_organized() {
                Ok(Some(record)) => {
                    tracing::info!(
                        target: "hkask.mcp.media",
                        gallery_id = %record.id,
                        root_path = %record.root_path,
                        mode = %record.mode,
                        "Re-activating most-recently-organized gallery at startup"
                    );
                    let mode: crate::gallery::GalleryMode = record
                        .mode
                        .parse()
                        .unwrap_or(crate::gallery::GalleryMode::ReadOnly);
                    Arc::new(Mutex::new(Some(GalleryState {
                        path: std::path::PathBuf::from(&record.root_path),
                        mode,
                        gallery_id: Some(record.id),
                    })))
                }
                Ok(None) => {
                    tracing::info!(
                        target: "hkask.mcp.media",
                        "No previously-organized gallery found — the gallery starts dormant until gallery_organize"
                    );
                    Arc::new(Mutex::new(None))
                }
                Err(error) => {
                    tracing::warn!(
                        target: "hkask.mcp.media",
                        %error,
                        "Failed to look up the most-recently-organized gallery at startup — the gallery starts dormant; gallery_organize activates it"
                    );
                    Arc::new(Mutex::new(None))
                }
            };
            Ok(MediaServer::new(
                ctx.webid,
                vision_port.clone(),
                gallery_state,
                gallery_store.clone(),
                template_env,
                FfmpegRunner::detect(),
                YtDlpRunner::detect(),
                jobs::new_job_store(),
                ctx.credentials.get("HKASK_SERPAPI_API_KEY").cloned(),
                voice,
            ))
        },
        vec![
            hkask_mcp_server::CredentialRequirement::optional(
                "OPENROUTER_API_KEY",
                "OpenRouter API key for vision LLMs",
            ),
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_SERPAPI_API_KEY",
                "SerpApi key for YouTube search metadata",
            ),
            hkask_mcp_server::db_passphrase_requirement_optional_with(
                "SQLCipher passphrase for the protected voice-profile database; other media tools remain available without it",
            ),
        ],
    )
    .await
}

#[cfg(test)]
mod dead_surface_pins;

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod tool_behavior_tests;

#[cfg(test)]
mod gallery_lifecycle_tests;
