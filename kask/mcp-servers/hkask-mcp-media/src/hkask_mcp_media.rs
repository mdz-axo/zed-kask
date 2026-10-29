// `forbid(unsafe_code)` in production builds; test builds allow it for
// `std::env::set_var` (artifacts-dir isolation in the persistence tests) —
// the same `cfg_attr(not(test), …)` pattern as hkask-email/hkask-inference.
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
pub mod media_block;
pub mod omc;
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
    pub const VISION_ENV: &str = "HKASK_MEDIA_VISION_MODEL";
    pub const PASS_ENV: &str = "HKASK_MEDIA_PASS_MODEL";

    /// Resolve a configured media model from its env var — `None` when
    /// unset (the operator's no-hidden-models spec: no constant fallback;
    /// callers fail visibly naming the env var).
    fn configured(env_key: &str) -> Option<String> {
        std::env::var(env_key).ok().filter(|m| !m.trim().is_empty())
    }

    pub fn stt_model() -> Option<String> {
        configured(STT_ENV)
    }
    pub fn vision_model() -> Option<String> {
        configured(VISION_ENV)
    }
    pub fn pass_model() -> Option<String> {
        configured(PASS_ENV)
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
pub mod transcript;
pub mod transcript_export;
pub mod transcript_layers;
pub mod transcript_pass;
pub mod transcript_retranscribe;
pub mod transcript_review;
pub mod transcript_select;
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

    /// Return the ffmpeg runner or an error if ffmpeg is not installed.
    fn require_ffmpeg(&self) -> Result<&FfmpegRunner, McpToolError> {
        if self.ffmpeg.available {
            Ok(&self.ffmpeg)
        } else {
            Err(McpToolError::unavailable(
                "ffmpeg not found on system PATH — video tools unavailable.",
            ))
        }
    }

    /// Return the yt-dlp runner or an error if yt-dlp is not installed.
    fn require_yt_dlp(&self) -> Result<&YtDlpRunner, McpToolError> {
        if self.ytdlp.available {
            Ok(&self.ytdlp)
        } else {
            Err(McpToolError::unavailable(
                "yt-dlp not found on system PATH — video_fetch unavailable. \
                 Install via: pip install yt-dlp  (or apt install yt-dlp on Ubuntu 24.04+)",
            ))
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
            + Self::layers_router()
            + Self::voice_router()
            + Self::youtube_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for MediaServer {}

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // One macro call pins the surface two ways: the exact count (81) and
    // the generated TOOL_NAMES set against the live rmcp surface. Catches
    // silent registration drops — a `#[tool]` impl block without
    // `#[tool_router]`, or a sub-router missing from `combined_router()`,
    // silently registers nothing (`cargo check` passes on an unwired
    // orphan) — and same-count renames, which the count alone cannot. The
    // Steer prompt's tool list is RENDERED from TOOL_NAMES
    // (hkask_steer::render_grouped_tool_advertisement in media_panel), so
    // the name pin is that chain's integrity check: without it, an unrouted
    // `#[tool]` fn would flow into the prompt as a dead name.
    hkask_mcp_server::tool_surface_pin!(
        MediaServer::combined_router(),
        "combined_router",
        84,
        tool_surface_is_exactly_84_registered_tools,
    );

    // Coverage: every registered tool must map to an OMC concept. Catches
    // the silent-drop failure mode where a new tool is added to the router
    // without a corresponding arm in omc::tool_to_omc.
    #[test]
    fn omc_mapping_covers_all_registered_tools() {
        let router = MediaServer::combined_router();
        for tool in router.list_all() {
            assert!(
                omc::tool_to_omc(&tool.name).is_some(),
                "omc::tool_to_omc returned None for registered tool '{}'; \
                 add an explicit arm",
                tool.name
            );
        }
    }

    // Regression: distinct tool families must map to distinct concepts.
    #[test]
    fn omc_mapping_distinguishes_tool_families() {
        let version = omc::tool_to_omc("educt_locate");
        let scene = omc::tool_to_omc("describe_image");
        let asset = omc::tool_to_omc("gallery_organize");
        let source = omc::tool_to_omc("audio_capture");
        let sequence = omc::tool_to_omc("video_clip");
        let shot = omc::tool_to_omc("video_extract_frames");
        let task = omc::tool_to_omc("job_submit");
        let participant = omc::tool_to_omc("model_list");
        // Eight distinct concepts across retained research tool families.
        let concepts = [
            version,
            scene,
            asset,
            source,
            sequence,
            shot,
            task,
            participant,
        ];
        for (i, a) in concepts.iter().enumerate() {
            for (j, b) in concepts.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        a, b,
                        "tool families {i} and {j} must anchor on distinct concepts"
                    );
                }
            }
        }
        assert_eq!(version, Some(omc::VERSION_INFO));
        assert_eq!(scene, Some(omc::SCENE));
        assert_eq!(asset, Some(omc::ASSET));
        assert_eq!(source, Some(omc::CAPTURE));
        assert_eq!(sequence, Some(omc::SEQUENCE));
        assert_eq!(shot, Some(omc::SHOT));
        assert_eq!(task, Some(omc::TASK));
        assert_eq!(participant, Some(omc::PARTICIPANT));
    }
}

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
        let default_media_db = hkask_types::agent_paths::resolve_under_data_dir(
            &hkask_types::agent_paths::mcp_server_db("media", "gallery"),
        );
        let db_path = std::env::var("HKASK_MEDIA_DB")
            .ok()
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or(default_media_db);
        if let Some(parent) = db_path.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                tracing::warn!(
                    target: "hkask.mcp.media",
                    path = %parent.display(),
                    %error,
                    "Failed to create gallery DB directory \
                     — the subsequent DB open will surface the failure"
                );
            }
        }
        let db_path_str = db_path.to_string_lossy().to_string();
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
            Ok(MediaServer::new(
                ctx.webid,
                vision_port.clone(),
                Arc::new(Mutex::new(None)),
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
            hkask_mcp_server::CredentialRequirement::optional(
                "HKASK_DB_PASSPHRASE",
                "SQLCipher passphrase for the protected voice-profile database; other media tools remain available without it",
            ),
        ],
    )
    .await
}

// ── OMC consumer pin ────────────────────────────────────────────────────
//
// Pins that the MovieLabs OMC bridge module has a consumer
// (`media_block::enrich_with_omc_and_provenance` references `omc::tool_to_omc`).
// Per `.rules` "Advertised invariants need enforcement points", a module
// with no consumer is dead surface regardless of its doc comments.
#[cfg(test)]
mod dead_surface_pins {
    #[test]
    fn omc_module_present_with_consumer() {
        // The source file must exist.
        let omc_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/omc.rs");
        assert!(
            omc_path.exists(),
            "src/omc.rs must exist — it is the MovieLabs OMC bridge"
        );
        // The lib root must declare the module.
        let lib_root = include_str!("hkask_mcp_media.rs");
        let declared = lib_root.lines().any(|line| {
            let t = line.trim();
            t == "pub mod omc;" || t == "mod omc;"
        });
        assert!(declared, "the omc module must be declared in the lib root");
        // The media_block module must reference the omc module — the consumer.
        // A line-level scan avoids matching this test's own text.
        let media_block = include_str!("media_block.rs");
        let has_consumer = media_block
            .lines()
            .any(|line| line.contains("crate::omc") || line.contains("use crate::omc"));
        assert!(
            has_consumer,
            "media_block.rs must reference the omc module — \
             a module without a consumer is dead surface"
        );
    }
}

// ── Integration tests ────────────────────────────────────────────────────
//
// These tests exercise the GalleryStore + GalleryState pipeline and collage
// composition logic. Inference-dependent tools require a running LLM backend
// and are tested via the MCP protocol in live sessions.

#[cfg(test)]
mod integration_tests {
    use crate::gallery::GalleryState;
    use crate::gallery::{GalleryMode, GalleryStore};
    use image::{Rgb, RgbImage};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_store() -> (Arc<GalleryStore>, TempDir) {
        let temp = TempDir::new().expect("tempdir");
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(GalleryStore::from_driver(driver).expect("gallery store init"));
        (store, temp)
    }

    fn create_test_image(dir: &std::path::Path, name: &str, r: u8, g: u8, b: u8) {
        let img: RgbImage = RgbImage::from_pixel(64, 64, Rgb([r, g, b]));
        img.save(dir.join(name)).expect("save test image");
    }

    #[test]
    fn gallery_lifecycle_init_to_search() {
        let (store, temp) = setup_store();

        create_test_image(temp.path(), "sunset.jpg", 255, 100, 50);
        create_test_image(temp.path(), "ocean.jpg", 50, 100, 255);
        create_test_image(temp.path(), "forest.png", 34, 139, 34);

        let gallery = store
            .open(
                &temp.path().to_string_lossy(),
                crate::gallery::GalleryMode::ReadOnly,
            )
            .expect("create gallery");
        assert_eq!(gallery.image_count, 0);

        let state = GalleryState::new(temp.path().to_path_buf(), GalleryMode::ReadOnly);
        let scan = state.scan(false, None);
        assert_eq!(scan.entries.len(), 3);

        for entry in &scan.entries {
            store
                .add_image(
                    &gallery.id,
                    &temp.path().join(&entry.absolute_path).to_string_lossy(),
                    &entry.hash,
                    entry.width,
                    entry.height,
                    &entry.format,
                    entry.size_bytes,
                )
                .expect("add image");
        }

        let img = store
            .get_image(&gallery.id, Some(0), None)
            .expect("get image");
        assert_eq!(img.width, 64);

        store
            .tag_image(&img.id, "object", "sunset", 0.95, "test")
            .expect("tag");

        let tags = store.get_tags(&img.id).expect("get tags");
        assert_eq!(tags.len(), 1);

        let all_tags = store.get_all_tags(&gallery.id).expect("get all tags");
        assert!(!all_tags.is_empty());
        assert!(all_tags.iter().any(|(t, _)| t.value == "sunset"));
    }

    #[test]
    fn collage_compose_grid_layout() {
        let temp = TempDir::new().expect("tempdir");

        let images: Vec<image::DynamicImage> = vec![
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([255u8, 0, 0]))),
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([0, 255u8, 0]))),
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([0, 0, 255u8]))),
            image::DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([255u8, 255u8, 0]))),
        ];

        let spacing: u32 = 8;
        let canvas_w: u32 = 800;
        let canvas_h: u32 = 600;
        let cols = (images.len() as f64).sqrt().ceil() as u32;
        let rows = (images.len() as u32).div_ceil(cols);
        assert_eq!(cols, 2);
        assert_eq!(rows, 2);

        let canvas = crate::tools::processing::compose_collage_grid(
            &images, cols, rows, canvas_w, canvas_h, spacing,
        );

        let output_path = temp.path().join("collage_test.png");
        canvas.save(&output_path).expect("save collage");
        let collage = image::open(&output_path).expect("reopen");
        assert_eq!(collage.width(), 800);
        assert_eq!(collage.height(), 600);

        let non_bg = collage
            .to_rgba8()
            .pixels()
            .filter(|p| p.0 != [30, 30, 30, 255])
            .count();
        assert!(
            non_bg > 100,
            "collage should have non-bg pixels (got {})",
            non_bg
        );
    }

    #[test]
    fn gallery_store_image_not_found() {
        let (store, temp) = setup_store();
        let gallery = store
            .open(
                &temp.path().to_string_lossy(),
                crate::gallery::GalleryMode::ReadOnly,
            )
            .expect("create gallery");

        assert!(store.get_image(&gallery.id, Some(999), None).is_err());
        assert!(
            store
                .get_image(&gallery.id, None, Some("nonexistent"))
                .is_err()
        );
    }

    #[test]
    fn gallery_three_state_policy() {
        use crate::gallery::GalleryMode;
        assert_eq!(GalleryMode::ReadOnly.as_str(), "read-only");
        assert_eq!(GalleryMode::CopyOnWrite.as_str(), "copy-on-write");
        assert_eq!(GalleryMode::Destructive.as_str(), "destructive");
        assert_ne!(
            GalleryMode::ReadOnly.as_str(),
            GalleryMode::Destructive.as_str()
        );
    }

    // ── Face recognition tests ─────────────────────────────────────────────

    #[test]
    fn face_validation_deserialize_pass() {
        let json = r#"{
            "valid": true,
            "face_count": 1,
            "face_coverage_pct": 45,
            "pose": "frontal",
            "lighting": "good",
            "occlusion": "none",
            "clarity": "sharp",
            "issues": []
        }"#;
        let result: crate::gallery::vision::FaceValidationResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(result.valid);
        assert_eq!(result.face_count, 1);
        assert_eq!(result.face_coverage_pct, 45);
        assert_eq!(result.pose, "frontal");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn face_validation_deserialize_reject() {
        let json = r#"{
            "valid": false,
            "face_count": 2,
            "face_coverage_pct": 10,
            "pose": "profile",
            "lighting": "poor",
            "occlusion": "significant",
            "clarity": "blurry",
            "issues": [
                "Multiple faces detected (2) — reference must contain exactly 1 face",
                "Face coverage too low (10%) — minimum 15% required",
                "Profile pose — frontal or near-frontal required"
            ]
        }"#;
        let result: crate::gallery::vision::FaceValidationResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(!result.valid);
        assert_eq!(result.face_count, 2);
        assert_eq!(result.issues.len(), 3);
        assert!(result.issues[0].contains("Multiple faces"));
    }

    #[test]
    fn face_match_deserialize_match() {
        let json = r#"{
            "match": true,
            "confidence": 0.94,
            "reasoning": "Same bone structure, identical eye spacing, matching nose shape."
        }"#;
        let result: crate::gallery::vision::FaceMatchResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(result.is_match);
        assert!((result.confidence - 0.94).abs() < 0.001);
        assert!(result.reasoning.contains("bone structure"));
    }

    #[test]
    fn face_match_deserialize_no_match() {
        let json = r#"{
            "match": false,
            "confidence": 0.85,
            "reasoning": "Different jawline structure and eye shape — likely different people."
        }"#;
        let result: crate::gallery::vision::FaceMatchResult =
            serde_json::from_str(json).expect("deserialize");
        assert!(!result.is_match);
        assert!((result.confidence - 0.85).abs() < 0.001);
        assert!(result.reasoning.contains("Different"));
    }

    #[test]
    fn face_registry_lifecycle() {
        let (store, _temp) = setup_store();

        // Create a gallery and image for the face reference
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .expect("create gallery");
        let img = store
            .add_image(
                &gallery.id,
                "/tmp/test-gallery/alice.jpg",
                "hash1",
                400,
                600,
                "jpg",
                50000,
            )
            .expect("add image");

        // Register a face
        let face = store
            .register_face("Alice", "Chen", &img.id, "valid", "Frontal portrait")
            .expect("register face");
        assert_eq!(face.first_name, "Alice");
        assert_eq!(face.status, "valid");

        // List faces
        let faces = store.list_faces(None).expect("list faces");
        assert_eq!(faces.len(), 1);

        // Get by ID
        let retrieved = store.get_face(&face.id).expect("get face");
        assert_eq!(retrieved.last_name, "Chen");

        // Remove
        store.remove_face(&face.id).expect("remove face");
        let faces = store.list_faces(None).expect("list after remove");
        assert_eq!(faces.len(), 0);
    }

    #[test]
    fn face_registry_status_filter() {
        let (store, _temp) = setup_store();
        let gallery = store
            .open(
                tempfile::tempdir()
                    .expect("root")
                    .path()
                    .to_str()
                    .expect("UTF-8 root"),
                GalleryMode::ReadOnly,
            )
            .expect("create gallery");
        let img1 = store
            .add_image(&gallery.id, "/tmp/a.jpg", "h1", 100, 100, "jpg", 1000)
            .expect("add img1");
        let img2 = store
            .add_image(&gallery.id, "/tmp/b.jpg", "h2", 100, 100, "jpg", 1000)
            .expect("add img2");

        store
            .register_face("Alice", "A", &img1.id, "valid", "")
            .unwrap();
        store
            .register_face("Bob", "B", &img2.id, "rejected", "Too dark")
            .unwrap();

        let valid = store.list_faces(Some("valid")).unwrap();
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].first_name, "Alice");

        let rejected = store.list_faces(Some("rejected")).unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].first_name, "Bob");
    }

    #[test]
    fn historical_gallery_lineage_round_trip() {
        let (store, temp) = setup_store();
        let gallery = store
            .open(&temp.path().to_string_lossy(), GalleryMode::ReadOnly)
            .unwrap();
        create_test_image(temp.path(), "gen.png", 10, 20, 30);
        let img = store
            .add_image(
                &gallery.id,
                temp.path().join("gen.png").to_str().unwrap(),
                "hash-gen",
                64,
                64,
                "png",
                1024,
            )
            .unwrap();

        // Historical generation metadata stays readable even though the
        // creative handlers that originally produced it have been retired.
        let wf = store
            .record_workflow("{\"nodes\":[],\"parallel\":false}")
            .unwrap();
        let params_json = serde_json::json!({ "size": "1024x1024" }).to_string();
        let record = store
            .record_generation(
                &img.id,
                "generate_image",
                Some("a serene mountain landscape"),
                Some("DeepInfra/black-forest-labs/FLUX-2-klein-4b"),
                Some("deepinfra"),
                Some(12345),
                Some(&params_json),
                Some(&wf.id),
                None,
            )
            .unwrap();
        assert_eq!(record.op, "generate_image");

        // Re-read the lineage (what gallery_lineage returns).
        let lineage = store
            .get_generation(&img.id)
            .unwrap()
            .expect("lineage should be recorded");
        assert_eq!(lineage.op, "generate_image");
        assert_eq!(
            lineage.prompt.as_deref(),
            Some("a serene mountain landscape")
        );
        assert_eq!(
            lineage.model.as_deref(),
            Some("DeepInfra/black-forest-labs/FLUX-2-klein-4b")
        );
        assert_eq!(lineage.provider.as_deref(), Some("deepinfra"));
        assert_eq!(lineage.seed, Some(12345));
        assert_eq!(lineage.workflow_id.as_deref(), Some(wf.id.as_str()));
        // Historical parameters round-trip without re-executing their retired operation.
        let replay: serde_json::Value =
            serde_json::from_str(lineage.params.as_deref().unwrap()).unwrap();
        assert_eq!(replay["size"], "1024x1024");

        // No lineage for an unrelated image.
        create_test_image(temp.path(), "other.png", 1, 2, 3);
        let other = store
            .add_image(
                &gallery.id,
                temp.path().join("other.png").to_str().unwrap(),
                "hash-other",
                64,
                64,
                "png",
                1024,
            )
            .unwrap();
        assert!(store.get_generation(&other.id).unwrap().is_none());
    }
}

// ── Tool-behavior contract tests (check-mcp-tool-tests.sh) ─────────────────
//
// Drives the real `Parameters<T>` tool seam for `gallery_refresh`. The
// contract pinned here: calling a gallery tool before any gallery is
// initialized returns the structured `{"error", "kind"}` envelope (not a
// panic, not raw text) — the degraded-state path operators hit first.
#[cfg(test)]
mod tool_behavior_tests {
    use super::*;
    use crate::test_support::read_only_organize_request;
    use crate::transcript_pass::PassMode;
    use crate::types::{GalleryRefreshRequest, YoutubeSearchRequest};
    use rmcp::handler::server::wrapper::Parameters;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn make_server() -> MediaServer {
        make_server_with_port(Arc::new(NoopInferencePort))
    }

    #[tokio::test]
    async fn youtube_search_surfaces_missing_serpapi_credential()
    -> Result<(), Box<dyn std::error::Error>> {
        let error = make_server()
            .youtube_search(Parameters(YoutubeSearchRequest {
                query: "Kurt Vonnegut story shapes".to_string(),
                max_results: Some(10),
            }))
            .await
            .expect_err("missing SerpApi credential must fail before network access");
        assert!(error.to_string().contains("permission_denied"));
        assert!(error.to_string().contains("HKASK_SERPAPI_API_KEY"));
        Ok(())
    }

    /// M-02 pin (mcp-tool-review): a nonexistent local input path is the
    /// caller's error — `not_found` naming the path, never `internal`
    /// (ffprobe) or `unavailable` (STT transport) as before the fix.
    #[tokio::test]
    async fn video_info_missing_local_file_is_not_found() {
        let error = make_server()
            .video_info(Parameters(crate::types::VideoInfoRequest {
                video_url: "/nonexistent/mcp-review-probe.mp4".to_string(),
            }))
            .await
            .expect_err("missing file must fail");
        assert!(
            error.to_string().contains("not_found"),
            "classified not_found: {error}"
        );
        assert!(
            error
                .to_string()
                .contains("/nonexistent/mcp-review-probe.mp4"),
            "names the missing path: {error}"
        );
    }

    /// M-02 pin: the STT path classifies the same way — a missing local
    /// audio file is not_found before any provider connection is attempted.
    #[tokio::test]
    async fn transcribe_bundle_missing_local_file_is_not_found() {
        let error = make_server()
            .transcribe_bundle(Parameters(crate::types::TranscribeBundleRequest {
                audio_url: "/nonexistent/mcp-review-probe.wav".to_string(),
                language: None,
                gallery_asset_id: None,
            }))
            .await
            .expect_err("missing file must fail");
        assert!(
            error.to_string().contains("not_found"),
            "classified not_found: {error}"
        );
    }

    /// Helper unit pins: remote URLs and data URIs pass untouched (their
    /// existence is the remote side's to discover); an existing local file
    /// and a file:// URI to it pass; a missing local path and a missing
    /// file:// URI are not_found.
    #[test]
    fn ensure_local_input_exists_classifies_by_input_kind() {
        ensure_local_input_exists("https://example.com/a.mp4").expect("remote URL passes");
        ensure_local_input_exists("data:audio/wav;base64,AAAA").expect("data URI passes");
        let existing = std::env::temp_dir().join("mcp-review-exists.txt");
        std::fs::write(&existing, "x").expect("write temp");
        ensure_local_input_exists(existing.to_str().expect("utf8 path"))
            .expect("existing local file passes");
        ensure_local_input_exists(&format!("file://{}", existing.to_str().expect("utf8 path")))
            .expect("file:// URI to an existing file passes");
        let missing = ensure_local_input_exists("/nonexistent/mcp-review-probe.wav")
            .expect_err("missing file must fail");
        assert!(missing.to_string().contains("not_found"));
        let missing_uri = ensure_local_input_exists("file:///nonexistent/mcp-review-probe.wav")
            .expect_err("missing file:// URI must fail");
        assert!(missing_uri.to_string().contains("not_found"));
    }

    /// dcterms:identifier: `MediaServer` bounded batch tool admissions
    /// expect: Oversized or zero-sized media batches fail visibly before gallery, network,
    /// inference, or FFmpeg work begins.
    /// [P9] Motivating: one request cannot silently expand beyond the server's resource budget.
    /// pre: each request violates its documented shared cardinality limit.
    /// post: every tool returns `invalid_argument` without requiring an active gallery.
    /// [P1] Constraining: the server never clamps or partially executes rejected work.
    #[tokio::test]
    async fn media_batch_tools_reject_invalid_cardinality_at_admission()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = make_server();
        let concat_over_cap =
            vec!["local.wav".to_string(); hkask_types::media_limits::MAX_CONCAT_ITEMS + 1];
        let video_over_cap =
            vec!["local.mp4".to_string(); hkask_types::media_limits::MAX_CONCAT_ITEMS + 1];

        let errors = [
            server
                .audio_concat(Parameters(AudioConcatRequest {
                    audio_urls: concat_over_cap,
                }))
                .await
                .expect_err("oversized audio concat must fail"),
            server
                .video_concat(Parameters(VideoConcatRequest {
                    video_urls: video_over_cap,
                }))
                .await
                .expect_err("oversized video concat must fail"),
            server
                .video_extract_frames(Parameters(VideoExtractFramesRequest {
                    video_url: "local.mp4".to_string(),
                    interval_sec: 2.0,
                    max_frames: 0,
                }))
                .await
                .expect_err("zero extracted frames must fail"),
            server
                .video_extract_frames(Parameters(VideoExtractFramesRequest {
                    video_url: "local.mp4".to_string(),
                    interval_sec: 2.0,
                    max_frames: hkask_types::media_limits::MAX_EXTRACTED_FRAMES + 1,
                }))
                .await
                .expect_err("oversized frame extraction must fail"),
        ];

        for error in errors {
            assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        }
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_extract_frames`
    /// expect: Valid extracted frames remain available when a sibling frame import fails, and
    /// the response preserves that frame's original cause without leaving failed copies.
    /// [P1] Motivating: one bad frame cannot erase useful extracted media.
    /// pre: FFmpeg emits one valid JPEG and one invalid JPEG.
    /// post: status is partial, one asset remains, one causal failure is reported, and scratch
    /// plus the failed durable copy are removed.
    #[cfg(unix)]
    #[tokio::test]
    async fn video_extract_frames_reports_partial_and_cleans_failed_copies()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir()?;
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let gallery_root = tempfile::tempdir()?;
        let template = artifacts.path().join("valid.jpg");
        image::RgbImage::new(2, 2).save(&template)?;
        let fake_ffmpeg = artifacts.path().join("extract-mixed.sh");
        let template_path = template.to_string_lossy().replace('\'', "'\\''");
        std::fs::write(
            &fake_ffmpeg,
            format!(
                "#!/bin/sh\nfor arg do pattern=$arg; done\nfirst=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/001/')\nsecond=$(printf '%s\\n' \"$pattern\" | sed 's/%03d/002/')\ncp '{template_path}' \"$first\"\nprintf invalid > \"$second\"\nprintf '[Parsed_showinfo_1] n: 0 pts: 37 pts_time:3.7\\n[Parsed_showinfo_1] n: 1 pts: 725 pts_time:7.25\\n' >&2\n"
            ),
        )?;
        let mut permissions = std::fs::metadata(&fake_ffmpeg)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_ffmpeg, permissions)?;
        let scratch = artifacts.path().join("scratch");
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let store = Arc::new(GalleryStore::from_driver(driver)?);
        let gallery = store.open(
            gallery_root.path().to_str().ok_or("gallery root UTF-8")?,
            GalleryMode::ReadOnly,
        )?;
        let source = gallery_root.path().join("source.mp4");
        std::fs::write(&source, b"fake video")?;
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopInferencePort),
            Arc::new(Mutex::new(Some(GalleryState {
                path: gallery_root.path().to_path_buf(),
                mode: GalleryMode::ReadOnly,
                gallery_id: Some(gallery.id.clone()),
            }))),
            store.clone(),
            templates::create_env()?,
            FfmpegRunner::with_binary(fake_ffmpeg.to_string_lossy().into_owned(), scratch.clone()),
            YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        );

        let response = server
            .video_extract_frames(Parameters(VideoExtractFramesRequest {
                video_url: source.display().to_string(),
                interval_sec: 2.0,
                max_frames: 2,
            }))
            .await?;
        let payload = content_of(&response);
        assert_eq!(payload["status"], "partial");
        assert_eq!(payload["frames_imported"], 1);
        assert_eq!(
            payload["frames"][0]["effective_params"]["source_pts_sec"],
            3.7
        );
        assert_eq!(payload["failures"].as_array().map(Vec::len), Some(1));
        assert_eq!(payload["failures"][0]["source_pts_sec"], 7.25);
        assert!(
            payload["failures"][0]["cause"]
                .as_str()
                .is_some_and(|cause| !cause.is_empty())
        );
        assert_eq!(store.count_assets(&gallery.id)?, 0);
        assert_eq!(store.count_assets(&generated_gallery(&store))?, 1);
        assert_eq!(
            std::fs::read_dir(crate::assets::generated_assets_dir())?.count(),
            1,
            "failed durable frame copy survived"
        );
        assert_eq!(std::fs::read_dir(&scratch)?.count(), 0);
        Ok(())
    }

    /// Organize a read-only gallery over one fresh 2x2 png and return
    /// (server, directory) — the shared prologue of the reopen/rescan
    /// gallery-identity tests, which vary only in what they assert after
    /// the first organize.
    async fn organized_single_image_gallery()
    -> Result<(MediaServer, tempfile::TempDir), Box<dyn std::error::Error>> {
        let server = make_server();
        let directory = tempfile::tempdir()?;
        image::RgbImage::new(2, 2).save(directory.path().join("one.png"))?;
        server
            .gallery_organize(read_only_organize_request(directory.path()))
            .await?;
        Ok((server, directory))
    }

    /// expect: Reopening my gallery after restart restores its original identity. [P1]
    #[tokio::test]
    async fn gallery_reopen_restores_requested_root() -> Result<(), Box<dyn std::error::Error>> {
        let (server, directory) = organized_single_image_gallery().await?;
        let original = server.access_gallery()?.gallery_id;
        *server
            .gallery_state
            .lock()
            .map_err(|error| error.to_string())? = None;
        server
            .gallery_organize(read_only_organize_request(directory.path()))
            .await?;
        assert_eq!(server.access_gallery()?.gallery_id, original);
        assert_eq!(server.gallery_store.count_assets(&original)?, 1);
        Ok(())
    }

    /// expect: Rescanning preserves one identity and my annotations, not duplicate rows. [P1]
    #[tokio::test]
    async fn gallery_rescan_preserves_identity() -> Result<(), Box<dyn std::error::Error>> {
        let (server, directory) = organized_single_image_gallery().await?;
        let gallery = server.access_gallery()?;
        let original = server
            .gallery_store
            .get_image(&gallery.gallery_id, Some(0), None)?;
        server
            .gallery_store
            .tag_image(&original.id, "caption", "My photo", 1.0, "user")?;
        server
            .gallery_organize(read_only_organize_request(directory.path()))
            .await?;
        assert_eq!(server.gallery_store.count_assets(&gallery.gallery_id)?, 1);
        let current = server
            .gallery_store
            .get_image(&gallery.gallery_id, Some(0), None)?;
        assert_eq!(current.id, original.id);
        assert_eq!(current.added_at, original.added_at);
        assert_eq!(server.gallery_store.get_tags(&current.id)?.len(), 1);
        Ok(())
    }

    /// PR-M2 pin (MF-13): the collage routes through the canonical storage
    /// contract — the output is gallery-indexed with lineage
    /// (op `image_create_collage`) at a durable path, never a bare temp
    /// path; the scratch write is consumed by the publication.
    #[tokio::test]
    async fn collage_publishes_through_canonical_storage() -> Result<(), Box<dyn std::error::Error>>
    {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir()?;
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let server = make_server();
        let directory = tempfile::tempdir()?;
        image::RgbImage::new(2, 2).save(directory.path().join("one.png"))?;
        image::RgbImage::new(2, 2).save(directory.path().join("two.png"))?;
        server
            .gallery_organize(read_only_organize_request(directory.path()))
            .await?;
        let result = server
            .image_create_collage(Parameters(crate::types::CreateCollageRequest {
                search_terms: None,
                similar_to_index: None,
                image_indices: Some(vec![0, 1]),
                max_items: 6,
                layout: "horizontal".into(),
                spacing: 8,
                canvas_size: "1200x900".into(),
            }))
            .await?;
        let content = content_of(&result);
        assert_eq!(content["status"], serde_json::json!("created"));
        assert_eq!(content["image_count"], serde_json::json!(2));
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .expect("the collage result carries gallery identity")
            .to_string();
        // The gallery row exists and is an image.
        let row = server.gallery_store.get_by_id(&asset_id)?;
        assert_eq!(row.media_type, "image");
        // The lineage is recorded under the collage op.
        let lineage = server
            .gallery_store
            .get_generation(&asset_id)?
            .expect("lineage recorded for the collage publication");
        assert_eq!(lineage.op, "image_create_collage");
        // The output is the durable published path — under the artifacts
        // root, never the OS temp dir — and the file exists there.
        let output = content["output"].as_str().expect("output path");
        let artifacts_root = crate::assets::generated_assets_dir();
        assert!(
            std::path::Path::new(output).starts_with(&artifacts_root),
            "the collage publishes durably: {output} (artifacts root {})",
            artifacts_root.display()
        );
        assert!(std::path::Path::new(output).is_file());
        // The display hint connects the published asset to the widget.
        assert!(
            content["display_hint"]
                .as_str()
                .expect("display hint")
                .contains(&asset_id),
            "the hint carries the gallery asset id"
        );
        Ok(())
    }

    /// A no-op inference port — gallery_refresh without face analysis never
    /// reaches inference, so this only satisfies the struct field.
    struct NoopInferencePort;

    crate::test_support::media_inference_stub! {
        NoopInferencePort,
        generate(self, _prompt): {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "noop inference port — not configured for contract tests".into(),
                ))
            })
        }
    }

    /// Parse a tool result's `{"content": …}` envelope into its JSON value.
    /// The gallery every produced asset is filed under (its root holds `media-mcp/generated/`).
    fn generated_gallery(store: &GalleryStore) -> String {
        crate::assets::owning_gallery_id(
            store,
            &crate::assets::generated_assets_dir(),
            &crate::assets::generated_assets_dir(),
        )
        .expect("generated gallery resolves")
    }

    fn content_of(result: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(result)
            .expect("tool result is JSON")
            .get("content")
            .cloned()
            .expect("tool result has the content envelope")
    }

    struct ArtifactsEnvGuard {
        prior: Option<String>,
    }

    impl ArtifactsEnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let prior = std::env::var("HKASK_ARTIFACTS_DIR").ok();
            unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", path) };
            Self { prior }
        }
    }

    impl Drop for ArtifactsEnvGuard {
        fn drop(&mut self) {
            match self.prior.take() {
                Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
                None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
            }
        }
    }

    fn file_backed_gallery_store(
        db_path: &std::path::Path,
    ) -> Result<
        (
            Arc<GalleryStore>,
            Arc<hkask_storage::database::sqlite::SqliteDriver>,
        ),
        Box<dyn std::error::Error>,
    > {
        let db_path = db_path.to_string_lossy().into_owned();
        let driver = Arc::new(hkask_storage::database::sqlite::SqliteDriver::new_labeled(
            hkask_storage::database::sqlite::SqliteDriver::file_pool(&db_path)?,
            db_path.as_str(),
        ));
        let store = Arc::new(GalleryStore::from_driver(driver.clone())?);
        Ok((store, driver))
    }

    /// The single-gallery contract-test environment: the artifacts env
    /// lock, the artifacts dir (under the env guard), the gallery root,
    /// the file-backed store with its driver (the fault-injection sites
    /// install triggers through it), and the opened gallery. Every
    /// gallery contract test repeated this prologue; the source-material
    /// creation and the server construction stay at each call site —
    /// they are what the test varies.
    async fn media_test_env() -> Result<
        (
            tokio::sync::MutexGuard<'static, ()>,
            tempfile::TempDir,
            ArtifactsEnvGuard,
            tempfile::TempDir,
            Arc<GalleryStore>,
            Arc<hkask_storage::database::sqlite::SqliteDriver>,
            crate::gallery::store::GalleryRecord,
        ),
        Box<dyn std::error::Error>,
    > {
        let env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir()?;
        let env = ArtifactsEnvGuard::set(artifacts.path());
        let gallery_root = tempfile::tempdir()?;
        let (store, driver) = file_backed_gallery_store(&artifacts.path().join("gallery.db"))?;
        let gallery = store.open(
            gallery_root.path().to_str().ok_or("gallery root UTF-8")?,
            GalleryMode::ReadOnly,
        )?;
        Ok((
            env_lock,
            artifacts,
            env,
            gallery_root,
            store,
            driver,
            gallery,
        ))
    }

    fn server_with_gallery_state(
        store: Arc<GalleryStore>,
        gallery_id: String,
        gallery_root: &std::path::Path,
    ) -> (MediaServer, Arc<std::sync::Mutex<Option<GalleryState>>>) {
        let gallery_state = Arc::new(std::sync::Mutex::new(Some(GalleryState {
            path: gallery_root.to_path_buf(),
            mode: GalleryMode::ReadOnly,
            gallery_id: Some(gallery_id),
        })));
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopInferencePort),
            gallery_state.clone(),
            store,
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        );
        (server, gallery_state)
    }

    fn server_with_gallery(
        store: Arc<GalleryStore>,
        gallery_id: String,
        gallery_root: &std::path::Path,
    ) -> MediaServer {
        server_with_gallery_state(store, gallery_id, gallery_root).0
    }

    /// A server over the gallery state with the deterministic fake-ffmpeg
    /// runner — the construction the capture/rollback tests need (they must
    /// not depend on a real ffmpeg). The two tests each hand-built this
    /// block before the helper (2026-10-09 ratchet pass); the existing
    /// `server_with_gallery_state` uses the real `FfmpegRunner::detect()`
    /// and does not fit.
    #[cfg(unix)]
    fn server_with_fake_ffmpeg(
        store: Arc<GalleryStore>,
        gallery_id: String,
        gallery_root: &std::path::Path,
        artifacts_root: &std::path::Path,
    ) -> Result<MediaServer, Box<dyn std::error::Error>> {
        let gallery_state = Arc::new(std::sync::Mutex::new(Some(GalleryState {
            path: gallery_root.to_path_buf(),
            mode: GalleryMode::ReadOnly,
            gallery_id: Some(gallery_id),
        })));
        Ok(MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopInferencePort),
            gallery_state,
            store,
            templates::create_env()?,
            fake_successful_ffmpeg(artifacts_root)?,
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        ))
    }

    #[cfg(unix)]
    fn fake_successful_ffmpeg(
        root: &std::path::Path,
    ) -> Result<video::ffmpeg::FfmpegRunner, Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let executable = root.join("fake-ffmpeg.sh");
        std::fs::write(
            &executable,
            "#!/bin/sh\nfor arg do output=$arg; done\nprintf fake-media > \"$output\"\n",
        )?;
        let mut permissions = std::fs::metadata(&executable)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions)?;
        Ok(video::ffmpeg::FfmpegRunner::with_binary(
            executable.to_string_lossy().into_owned(),
            root.join("ffmpeg-output"),
        ))
    }

    /// The two-gallery fixture every generated-gallery test uses: artifacts
    /// dir under the env guard, galleries A and B, A active at admission,
    /// a server over the shared store, and the `entered`/`resume` notify
    /// pair every admission gate parks its operation on. The gate and the
    /// spawned operation stay at each call site — they are what the test
    /// varies. The roots and artifacts dir are held so the fixture's
    /// registrations outlive the test body.
    #[cfg(unix)]
    struct TwoGalleryArms {
        _env: ArtifactsEnvGuard,
        _artifacts: tempfile::TempDir,
        _gallery_a_root: tempfile::TempDir,
        gallery_b_root: tempfile::TempDir,
        store: Arc<GalleryStore>,
        gallery_a: crate::gallery::store::GalleryRecord,
        gallery_b: crate::gallery::store::GalleryRecord,
        gallery_state: Arc<std::sync::Mutex<Option<GalleryState>>>,
        server: Arc<MediaServer>,
        entered: Arc<tokio::sync::Notify>,
        resume: Arc<tokio::sync::Notify>,
    }

    #[cfg(unix)]
    async fn two_gallery_arms() -> Result<TwoGalleryArms, Box<dyn std::error::Error>> {
        let artifacts = tempfile::tempdir()?;
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let gallery_a_root = tempfile::tempdir()?;
        let gallery_b_root = tempfile::tempdir()?;
        let (store, _) = file_backed_gallery_store(&artifacts.path().join("gallery.db"))?;
        let gallery_a = store.open(
            gallery_a_root
                .path()
                .to_str()
                .ok_or("gallery A root UTF-8")?,
            GalleryMode::ReadOnly,
        )?;
        let gallery_b = store.open(
            gallery_b_root
                .path()
                .to_str()
                .ok_or("gallery B root UTF-8")?,
            GalleryMode::ReadOnly,
        )?;
        let gallery_state = Arc::new(std::sync::Mutex::new(Some(GalleryState {
            path: gallery_a_root.path().to_path_buf(),
            mode: GalleryMode::ReadOnly,
            gallery_id: Some(gallery_a.id.clone()),
        })));
        let server = Arc::new(MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopInferencePort),
            gallery_state.clone(),
            store.clone(),
            templates::create_env()?,
            fake_successful_ffmpeg(artifacts.path())?,
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        ));
        Ok(TwoGalleryArms {
            _env,
            _artifacts: artifacts,
            _gallery_a_root: gallery_a_root,
            gallery_b_root,
            store,
            gallery_a,
            gallery_b,
            gallery_state,
            server,
            entered: Arc::new(tokio::sync::Notify::new()),
            resume: Arc::new(tokio::sync::Notify::new()),
        })
    }

    #[cfg(unix)]
    fn fake_ytdlp(
        root: &std::path::Path,
        body: &str,
    ) -> Result<video::ytdlp::YtDlpRunner, Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let executable = root.join("fake-yt-dlp.sh");
        std::fs::write(&executable, format!("#!/bin/sh\n{body}\n"))?;
        let mut permissions = std::fs::metadata(&executable)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions)?;
        Ok(video::ytdlp::YtDlpRunner::with_binary(
            executable.to_string_lossy().into_owned(),
        ))
    }

    #[cfg(unix)]
    fn server_with_ytdlp(
        store: Arc<GalleryStore>,
        gallery_id: Option<String>,
        gallery_root: &std::path::Path,
        ytdlp: video::ytdlp::YtDlpRunner,
    ) -> MediaServer {
        MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopInferencePort),
            Arc::new(std::sync::Mutex::new(gallery_id.map(|gallery_id| {
                GalleryState {
                    path: gallery_root.to_path_buf(),
                    mode: GalleryMode::ReadOnly,
                    gallery_id: Some(gallery_id),
                }
            }))),
            store,
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            ytdlp,
            jobs::new_job_store(),
            None,
            Default::default(),
        )
    }

    /// dcterms:identifier: `MediaServer::video_fetch`
    /// expect: A fetched platform video publishes once under one durable gallery identity.
    /// [P1] Motivating: download success and indexed durable publication are the same outcome.
    /// pre: an active gallery and successful downloader exist.
    /// post: file, row, lineage, graph, result id, and display-hint id agree.
    /// [P1] Constraining: no unindexed file-only success is permitted.
    #[tokio::test]
    async fn video_fetch_publishes_one_canonical_asset() -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let gallery_id = gallery.id.clone();
        let runner = fake_ytdlp(
            artifacts.path(),
            "while [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then shift; output=$1; fi; shift; done; printf fetched-video > \"$output\"; echo 'WARNING: No supported JavaScript runtime could be found; some formats may be missing' >&2",
        )?;
        let server =
            server_with_ytdlp(store.clone(), Some(gallery.id), gallery_root.path(), runner);

        let result = server
            .video_fetch(Parameters(VideoFetchRequest {
                url: "https://93.184.216.34/video".into(),
            }))
            .await?;
        let content = content_of(&result);
        let expected = serde_json::json!({
            "source_url": "https://93.184.216.34/video",
            "format": "mp4",
        });
        let output =
            std::path::PathBuf::from(content["output"].as_str().ok_or("missing durable output")?);
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing stable gallery asset id")?;
        let hint = media_hint_body(
            content["display_hint"]
                .as_str()
                .ok_or("missing display hint")?,
        )?;
        assert!(output.is_file());
        assert_eq!(content["effective_params"], expected);
        assert_eq!(hint["src"], content["output"]);
        assert_eq!(hint["gallery_asset_id"], asset_id);
        assert_eq!(hint["provenance"]["tool"], "video_fetch");
        assert_eq!(hint["provenance"]["args"], expected);
        assert!(
            content["warning"]
                .as_str()
                .is_some_and(|warning| warning.contains("JavaScript runtime"))
        );
        let asset = store.get_by_id(asset_id)?;
        assert_eq!(asset.gallery_id, generated_gallery(&store));
        assert_eq!(asset.absolute_path, output.to_string_lossy());
        assert_eq!(asset.format, "mp4");
        assert_eq!(asset.media_type, "video");
        let lineage = store
            .get_generation(asset_id)?
            .ok_or("video_fetch lineage missing")?;
        assert_eq!(lineage.op, "video_fetch");
        assert!(store.get_omc_creation_graph(asset_id)?.is_some());
        assert_eq!(store.count_assets(&gallery_id)?, 0);
        assert_eq!(store.count_assets(&generated_gallery(&store))?, 1);
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_fetch`
    /// expect: Unsafe source destinations fail before yt-dlp starts.
    /// [P1] Motivating: a fetch request cannot reach private-network services or embedded credentials.
    #[tokio::test]
    async fn video_fetch_rejects_unsafe_sources_before_downloader()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let marker = artifacts.path().join("downloader-started");
        let script = format!("touch '{}'; exit 0", marker.display());
        let runner = fake_ytdlp(artifacts.path(), &script)?;
        let server = server_with_ytdlp(store, Some(gallery.id), gallery_root.path(), runner);

        for url in [
            "http://127.0.0.1/video",
            "http://10.0.0.1/video",
            "http://169.254.1.1/video",
            "http://[::ffff:192.168.1.1]/video",
            "https://user:secret@example.com/video",
            "http://localhost/video",
        ] {
            let error = server
                .video_fetch(Parameters(VideoFetchRequest { url: url.into() }))
                .await
                .expect_err("unsafe source must fail");
            assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
            assert!(!marker.exists(), "downloader started for {url}");
        }
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_fetch`
    /// expect: Downloader failure or missing output leaves no file or database residue.
    /// [P1] Motivating: a failed fetch cannot masquerade as published user work.
    /// pre: yt-dlp either writes a partial file then fails or exits successfully without output.
    /// post: both cases fail visibly with zero generated files and zero gallery rows.
    #[tokio::test]
    async fn video_fetch_cleans_download_failure_and_missing_output()
    -> Result<(), Box<dyn std::error::Error>> {
        for body in [
            "while [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then shift; output=$1; fi; shift; done; printf partial > \"$output\"; printf fragment > \"$output.part\"; echo 'ERROR: video unavailable' >&2; exit 1",
            "exit 0",
        ] {
            let (_env_lock, artifacts, _env, gallery_root, store, _driver, gallery) =
                media_test_env().await?;
            let gallery_id = gallery.id.clone();
            let runner = fake_ytdlp(artifacts.path(), body)?;
            let server =
                server_with_ytdlp(store.clone(), Some(gallery.id), gallery_root.path(), runner);

            server
                .video_fetch(Parameters(VideoFetchRequest {
                    url: "https://93.184.216.34/video".into(),
                }))
                .await
                .expect_err("incomplete download must fail");
            assert_eq!(store.count_assets(&gallery_id)?, 0);
            let generated = crate::assets::generated_assets_dir();
            if generated.exists() {
                assert_eq!(std::fs::read_dir(generated)?.count(), 0);
            }
        }
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::stt_precondition_input`
    /// expect: Local inputs above the upload threshold get their speech
    /// track extracted (mp3, smaller than the source); small inputs and
    /// remote URLs pass through untouched — the 413 seam cannot recur.
    /// [P1] Motivating: a 274 MB video container was uploaded whole and
    /// failed at the provider boundary after the full transfer.
    #[tokio::test]
    async fn stt_precondition_extracts_large_local_inputs() {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await.expect("test env");
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());

        // A real WAV above the 40 MB precondition threshold (16 kHz mono
        // ≈ 32 KB/s → 1320 s ≈ 42 MB) — the 413 seam's input class.
        let large = gallery_root.path().join("stt-large-input.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1320",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                large.to_str().expect("utf-8 path"),
            ])
            .output()
            .await
            .expect("ffmpeg runs");
        assert!(status.status.success(), "large fixture generated");

        let outcome = server
            .stt_precondition_input(large.to_str().expect("utf-8 path"))
            .await
            .expect("large local input preconditions");
        let extracted = outcome.expect("extraction ran");
        let extracted_path = std::path::Path::new(&extracted);
        assert!(extracted_path.exists(), "extracted artifact exists");
        assert_eq!(
            extracted_path.extension().and_then(|e| e.to_str()),
            Some("mp3")
        );
        let extracted_size = std::fs::metadata(extracted_path).unwrap().len();
        let source_size = std::fs::metadata(&large).unwrap().len();
        assert!(
            extracted_size < source_size,
            "extraction shrinks the upload ({extracted_size} < {source_size})"
        );

        // file:// URIs are local inputs too — the 2026-10-26 review caught
        // the unstripped URI silently skipping preconditioning for exactly
        // that input class; the URI form must extract like the plain path.
        let file_uri = format!("file://{}", large.display());
        let uri_outcome = server
            .stt_precondition_input(&file_uri)
            .await
            .expect("file:// URI preconditions");
        assert!(
            uri_outcome.is_some(),
            "the URI form extracts like the plain path"
        );

        // Small local inputs and remote URLs pass through untouched.
        let small = gallery_root.path().join("stt-small-input.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-ar",
                "16000",
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                small.to_str().expect("utf-8 path"),
            ])
            .output()
            .await
            .expect("ffmpeg runs");
        assert!(status.status.success(), "small fixture generated");
        assert_eq!(
            server
                .stt_precondition_input(small.to_str().expect("utf-8 path"))
                .await
                .expect("small local input passes"),
            None,
            "small local inputs transcribe as-is"
        );
        assert_eq!(
            server
                .stt_precondition_input("https://example.com/recording.mp3")
                .await
                .expect("remote URL passes"),
            None,
            "remote URLs pass through to the network path"
        );
    }

    async fn create_real_audio(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-ar",
                "16000",
                "-ac",
                "1",
                path.to_str().ok_or("audio fixture path is not UTF-8")?,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(format!(
                "real ffmpeg audio fixture creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    async fn create_real_video(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=64x64:d=2",
                "-c:v",
                "mpeg4",
                path.to_str().ok_or("fixture path is not UTF-8")?,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(format!(
                "real ffmpeg fixture creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    /// A real two-scene fixture: one second of solid blue concatenated with
    /// one second of solid red — a deterministic hard cut at ~1.0s whose
    /// scene score is near maximal.
    async fn create_real_hard_cut_video(
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=64x64:d=1",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x64:d=1",
                "-filter_complex",
                "[0:v:0][1:v:0]concat=n=2:v=1:a=0[out]",
                "-map",
                "[out]",
                "-c:v",
                "mpeg4",
                path.to_str().ok_or("hard-cut fixture path is not UTF-8")?,
            ])
            .output()
            .await?;
        if !output.status.success() {
            return Err(format!(
                "real ffmpeg hard-cut fixture creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    /// The real-video server environment shared by the video publication and
    /// rollback tests: the media test env (env lock, artifacts dir, env
    /// guard, gallery root), a real source.mp4, and a server over the
    /// gallery. The driver is exposed for the rollback tests' trigger
    /// injection — the trigger SQL stays at each call site, it is what the
    /// test varies. The holders stay bound in the struct so the env outlives
    /// the test body.
    ///
    /// Field order is load-bearing: struct fields drop in declaration order,
    /// so the env guard must be declared BEFORE the lock — otherwise the
    /// lock releases while `HKASK_ARTIFACTS_DIR` still points into this
    /// test's tempdir, and a test waiting on the lock can set its own env
    /// inside that window only for this guard's Drop to restore a stale
    /// value over it (live 2026-10-28: `video_clip_file_publication_failure`
    /// saw staging succeed against another test's artifacts dir). The env
    /// restores first, the lock releases last.
    struct VideoServerEnv {
        _env: ArtifactsEnvGuard,
        _env_lock: tokio::sync::MutexGuard<'static, ()>,
        _artifacts: tempfile::TempDir,
        _gallery_root: tempfile::TempDir,
        source: std::path::PathBuf,
        store: Arc<GalleryStore>,
        driver: Arc<hkask_storage::database::sqlite::SqliteDriver>,
        gallery_id: String,
        server: MediaServer,
    }

    async fn video_server_env() -> Result<VideoServerEnv, Box<dyn std::error::Error>> {
        let (env_lock, artifacts, env, gallery_root, store, driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("source.mp4");
        create_real_video(&source).await?;
        let server = server_with_gallery(store.clone(), gallery.id.clone(), gallery_root.path());
        Ok(VideoServerEnv {
            _env_lock: env_lock,
            _artifacts: artifacts,
            _env: env,
            _gallery_root: gallery_root,
            source,
            store,
            driver,
            gallery_id: gallery.id,
            server,
        })
    }

    fn media_hint_body(hint: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let body = hint
            .strip_prefix("```media\n")
            .ok_or("missing media fence")?
            .strip_suffix("\n```")
            .ok_or("missing closing media fence")?;
        Ok(serde_json::from_str(body)?)
    }

    /// The shared durable-audio-asset contract (trim/concat/capture): a
    /// canonical output under the generated-assets dir, a stable gallery
    /// identity, an audio display hint with OMC CAPTURE provenance,
    /// effective params equal to `expected`, a generated-gallery row of wav
    /// audio, and lineage naming `op` with params equal to `expected`. The
    /// three audio tests each carried this block before the helper
    /// (2026-10-09 duplication-ratchet pass); trim's extras (hint identity,
    /// absolute path, the OMC detail graph) stay in its test.
    fn assert_durable_audio_asset(
        content: &serde_json::Value,
        store: &GalleryStore,
        expected: &serde_json::Value,
        op: &str,
    ) -> Result<(std::path::PathBuf, String, serde_json::Value), Box<dyn std::error::Error>> {
        let output =
            std::path::PathBuf::from(content["output"].as_str().ok_or("missing durable output")?);
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing stable gallery_asset_id")?
            .to_string();
        let hint = media_hint_body(
            content["display_hint"]
                .as_str()
                .ok_or("missing audio display hint")?,
        )?;
        assert!(output.starts_with(crate::assets::generated_assets_dir()));
        assert!(output.is_file());
        assert_eq!(hint["kind"], "audio");
        assert_eq!(hint["ontology"], hkask_bridge_ontology::omc::CAPTURE);
        assert_eq!(hint["provenance"]["args"], *expected);
        assert_eq!(content["effective_params"], *expected);
        let asset = store.get_by_id(&asset_id)?;
        assert_eq!(asset.gallery_id, generated_gallery(store));
        assert_eq!(asset.format, "wav");
        assert_eq!(asset.media_type, "audio");
        let lineage = store
            .get_generation(&asset_id)?
            .ok_or(format!("{op} lineage missing"))?;
        assert_eq!(lineage.op, op);
        let params: serde_json::Value =
            serde_json::from_str(lineage.params.as_deref().ok_or("lineage params missing")?)?;
        assert_eq!(params, *expected);
        Ok((output, asset_id, hint))
    }

    /// The gallery-switch choreography shared by the two-gallery tests:
    /// wait for the spawned operation to reach its admission/validation
    /// gate, swap the active gallery to B, resume the operation, await it,
    /// and return (content, stable gallery asset id). Each switch test
    /// carried this block before the helper (2026-10-09 ratchet pass).
    async fn switch_to_gallery_b_and_finish(
        entered: &Arc<tokio::sync::Notify>,
        resume: &Arc<tokio::sync::Notify>,
        gallery_state: &Arc<std::sync::Mutex<Option<GalleryState>>>,
        gallery_b_root: &std::path::Path,
        gallery_b_id: &str,
        operation: tokio::task::JoinHandle<Result<String, hkask_mcp_server::server::McpToolError>>,
    ) -> Result<(serde_json::Value, String), Box<dyn std::error::Error>> {
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified()).await?;
        *gallery_state.lock().map_err(|error| error.to_string())? = Some(GalleryState {
            path: gallery_b_root.to_path_buf(),
            mode: GalleryMode::ReadOnly,
            gallery_id: Some(gallery_b_id.to_string()),
        });
        resume.notify_one();
        let content = content_of(&operation.await??);
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing stable gallery asset id")?
            .to_string();
        Ok((content, asset_id))
    }

    /// dcterms:identifier: `MediaServer::audio_trim`
    /// expect: My trimmed recording remains addressable by one stable gallery identity after teardown.
    /// [P1] Motivating: completed audio work survives processor and server teardown.
    /// pre: a real source WAV and active file-backed gallery exist.
    /// post: the durable WAV, gallery row, lineage, result id, and media-block id agree.
    /// [P1] Constraining: the source path never becomes a fabricated parent identity.
    #[tokio::test]
    async fn audio_trim_publishes_durable_asset_and_lineage_after_server_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("source.wav");
        create_real_audio(&source).await?;
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let content = content_of(
            &server
                .audio_trim(Parameters(AudioTrimRequest {
                    audio_url: source.to_string_lossy().into_owned(),
                    start_sec: 0.25,
                    end_sec: 1.25,
                }))
                .await?,
        );

        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing stable gallery_asset_id")?
            .to_string();
        let expected = serde_json::json!({
            "source": source.to_string_lossy(),
            "start_sec": 0.25,
            "end_sec": 1.25,
            "duration_sec": 1.0,
            "format": "wav",
        });
        let detail = content_of(
            &server
                .gallery_asset_detail(Parameters(GalleryAssetDetailRequest {
                    image_index: None,
                    image_id: Some(asset_id.clone()),
                }))
                .await?,
        );
        assert_eq!(detail["omc_creation_graph"]["asset_id"], asset_id);
        assert_eq!(
            detail["omc_creation_graph"]["task_id"],
            content["omc_task_id"]
        );
        drop(server);
        let (output, asset_id, hint) =
            assert_durable_audio_asset(&content, &store, &expected, "audio_trim")?;
        assert_eq!(hint["gallery_asset_id"], asset_id);
        let asset = store.get_by_id(&asset_id)?;
        assert_eq!(std::path::Path::new(&asset.absolute_path), output);
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::audio_concat`
    /// expect: My concatenated recordings remain one durable, traceable gallery asset.
    /// [P1] Motivating: completed audio work survives processor and server teardown.
    /// pre: two real WAV sources and an active file-backed gallery exist.
    /// post: the durable WAV, gallery row, provenance, lineage, and stable identity agree.
    /// [P1] Constraining: ordered source paths never become one fabricated parent identity.
    #[tokio::test]
    async fn audio_concat_publishes_durable_asset_and_lineage_after_server_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let first = gallery_root.path().join("first.wav");
        let second = gallery_root.path().join("second.wav");
        create_real_audio(&first).await?;
        create_real_audio(&second).await?;
        let sources = vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let content = content_of(
            &server
                .audio_concat(Parameters(AudioConcatRequest {
                    audio_urls: sources.clone(),
                }))
                .await?,
        );
        drop(server);

        let expected = serde_json::json!({"sources": sources, "format": "wav"});
        assert_durable_audio_asset(&content, &store, &expected, "audio_concat")?;
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::audio_capture`
    /// expect: My microphone recording is published canonically without a caller-selected output path.
    /// [P1] Motivating: captured audio has the same durable identity contract as processed media.
    /// pre: a capture-capable FFmpeg runner and active file-backed gallery exist.
    /// post: one canonical WAV, gallery row, provenance, lineage, and stable identity agree.
    /// [P1] Constraining: the temporary device-capture file is consumed by publication.
    #[cfg(unix)]
    #[tokio::test]
    async fn audio_capture_publishes_canonical_asset_without_output_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let server = server_with_fake_ffmpeg(
            store.clone(),
            gallery.id,
            gallery_root.path(),
            artifacts.path(),
        )?;
        let content = content_of(
            &server
                .audio_capture(Parameters(AudioCaptureRequest { duration_secs: 1.0 }))
                .await?,
        );
        drop(server);

        let expected = serde_json::json!({
            "duration_secs": 1.0,
            "sample_rate": 16000,
            "channels": 1,
            "format": "wav",
        });
        assert_durable_audio_asset(&content, &store, &expected, "audio_capture")?;
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::audio_capture`
    /// expect: Produced media is filed under the canonical generated gallery, whatever gallery is active.
    /// [P1] Motivating: one canonical home for every output the media server makes.
    /// pre: an unrelated gallery A is active; capture pauses after admission and B is activated.
    /// post: the recording is indexed in the gallery whose root contains `media-mcp/generated/`, not A or B.
    #[cfg(unix)]
    #[tokio::test]
    async fn audio_capture_files_under_the_generated_gallery()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        // Field access (not a destructure): the keep-alive fixture fields
        // (_env, _artifacts, gallery_a_root) must stay bound for the test
        // body — a `..` rest or early drop deletes the artifacts dir (and
        // the fake ffmpeg) before the operation spawns.
        let arms = two_gallery_arms().await?;
        let _gate = crate::tools::audio::install_audio_admission_gate(
            crate::tools::audio::AudioAdmissionGate {
                entered: arms.entered.clone(),
                resume: arms.resume.clone(),
            },
        )?;
        let operation = {
            let server = arms.server.clone();
            tokio::spawn(async move {
                server
                    .audio_capture(Parameters(AudioCaptureRequest { duration_secs: 1.0 }))
                    .await
            })
        };
        let (_content, asset_id) = switch_to_gallery_b_and_finish(
            &arms.entered,
            &arms.resume,
            &arms.gallery_state,
            arms.gallery_b_root.path(),
            &arms.gallery_b.id,
            operation,
        )
        .await?;

        let generated = arms
            .store
            .containing(&crate::assets::generated_assets_dir())?
            .ok_or("generated gallery registered")?;
        assert_eq!(arms.store.get_by_id(&asset_id)?.gallery_id, generated.id);
        assert_eq!(arms.store.count_assets(&arms.gallery_a.id)?, 0);
        assert_eq!(arms.store.count_assets(&arms.gallery_b.id)?, 0);
        Ok(())
    }

    /// dcterms:identifier: `assets::publish_local_media`
    /// expect: An audio lineage failure removes the WAV and gallery identity while preserving the cause.
    /// [P1] Motivating: provenance failure cannot leave partially published user work.
    /// pre: real FFmpeg and gallery insertion succeed before audio lineage is rejected.
    /// post: the database cause is returned and file, row, and lineage are absent.
    /// [P1] Constraining: publication commits only after lineage is durable.
    #[tokio::test]
    async fn audio_lineage_failure_rolls_back_file_gallery_and_lineage()
    -> Result<(), Box<dyn std::error::Error>> {
        use hkask_storage::database::driver::DatabaseDriver;

        let (_env_lock, _artifacts, _env, gallery_root, store, driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("source.wav");
        create_real_audio(&source).await?;
        driver.execute(
            "CREATE TRIGGER fail_audio_lineage BEFORE INSERT ON gallery_generation \
             WHEN NEW.op = 'audio_trim' BEGIN SELECT RAISE(ABORT, 'injected audio lineage failure'); END",
            &[],
        )?;
        let gallery_id = gallery.id.clone();
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let error = server
            .audio_trim(Parameters(AudioTrimRequest {
                audio_url: source.to_string_lossy().into_owned(),
                start_sec: 0.0,
                end_sec: 1.0,
            }))
            .await
            .expect_err("audio lineage publication must fail");

        assert!(
            error.to_string().contains("injected audio lineage failure"),
            "original lineage cause was not preserved: {error}"
        );
        assert_eq!(store.count_assets(&gallery_id)?, 0);
        let mut generated_files = Vec::new();
        for entry in std::fs::read_dir(crate::assets::generated_assets_dir())? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                generated_files.push(entry.path());
            }
        }
        assert!(
            generated_files.is_empty(),
            "audio lineage rollback left generated files: {generated_files:?}"
        );
        Ok(())
    }

    /// dcterms:identifier: `assets::publish_local_media`
    /// expect: An OMC graph failure leaves no published media or database residue.
    /// [P1] Motivating: semantic persistence is part of the asset's atomic commit.
    /// pre: capture, gallery insertion, and lineage succeed before graph insertion is rejected.
    /// post: the graph cause is surfaced and file, asset row, lineage, and graph are absent.
    /// [P1] Constraining: publication remains rollback-armed until the graph is durable.
    #[cfg(unix)]
    #[tokio::test]
    async fn omc_graph_failure_rolls_back_file_asset_lineage_and_graph()
    -> Result<(), Box<dyn std::error::Error>> {
        use hkask_storage::database::driver::DatabaseDriver;

        let (_env_lock, artifacts, _env, gallery_root, store, driver, gallery) =
            media_test_env().await?;
        driver.execute(
            "CREATE TRIGGER fail_omc_graph BEFORE INSERT ON gallery_omc_creation_graph \
             BEGIN SELECT RAISE(ABORT, 'injected OMC graph failure'); END",
            &[],
        )?;
        let gallery_id = gallery.id.clone();
        let server = server_with_fake_ffmpeg(
            store.clone(),
            gallery.id,
            gallery_root.path(),
            artifacts.path(),
        )?;
        let error = server
            .audio_capture(Parameters(AudioCaptureRequest { duration_secs: 1.0 }))
            .await
            .expect_err("OMC graph publication must fail");

        assert!(
            error.to_string().contains("injected OMC graph failure"),
            "original graph cause was not preserved: {error}"
        );
        assert_eq!(store.count_assets(&gallery_id)?, 0);
        assert_eq!(
            std::fs::read_dir(crate::assets::generated_assets_dir())?.count(),
            0
        );
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_clip`
    /// expect: My clipped video remains addressable by one stable gallery identity after restart.
    /// [P1] Motivating: user work survives processor and server teardown.
    /// pre: a real source video and active file-backed gallery exist.
    /// post: the durable file, gallery row, lineage, result id, and media-block id survive reopen and agree.
    /// [P1] Constraining: no parent identity is inferred from the source path.
    #[tokio::test]
    async fn video_clip_publishes_durable_asset_and_lineage_across_store_reopen()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir()?;
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let gallery_root = tempfile::tempdir()?;
        let source = gallery_root.path().join("source.mp4");
        create_real_video(&source).await?;
        let db_path = artifacts.path().join("gallery.db");

        let (output_path, asset_id) = {
            let (store, _) = file_backed_gallery_store(&db_path)?;
            let gallery = store.open(
                gallery_root
                    .path()
                    .to_str()
                    .ok_or("gallery root is not UTF-8")?,
                GalleryMode::ReadOnly,
            )?;
            let server = server_with_gallery(store, gallery.id, gallery_root.path());
            let result = server
                .video_clip(Parameters(VideoClipRequest {
                    video_url: source.to_string_lossy().into_owned(),
                    start_sec: 0.25,
                    end_sec: 1.25,
                }))
                .await?;
            let content = content_of(&result);
            let output_path = std::path::PathBuf::from(
                content["output"].as_str().ok_or("missing durable output")?,
            );
            let asset_id = content["gallery_asset_id"]
                .as_str()
                .ok_or("missing stable gallery_asset_id")?
                .to_string();
            let hint = media_hint_body(
                content["display_hint"]
                    .as_str()
                    .ok_or("missing media display hint")?,
            )?;

            assert!(output_path.starts_with(crate::assets::generated_assets_dir()));
            assert!(
                output_path.is_file(),
                "published clip must exist before restart"
            );
            assert_eq!(hint["src"], content["output"]);
            assert_eq!(hint["gallery_asset_id"], asset_id);
            assert_eq!(hint["ontology"], hkask_bridge_ontology::omc::SEQUENCE);
            assert_eq!(hint["kind"], "video");
            assert_eq!(hint["provenance"]["tool"], "video_clip");
            let effective_params = serde_json::json!({
                "source": source.to_string_lossy(),
                "start_sec": 0.25,
                "end_sec": 1.25,
                "duration_sec": 1.0,
                "format": "mp4",
            });
            assert_eq!(content["effective_params"], effective_params);
            assert_eq!(hint["provenance"]["args"], effective_params);
            (output_path, asset_id)
        };

        let (reopened, _) = file_backed_gallery_store(&db_path)?;
        let asset = reopened.get_by_id(&asset_id)?;
        assert_eq!(asset.gallery_id, generated_gallery(&reopened));
        assert_eq!(asset.id, asset_id);
        assert_eq!(std::path::Path::new(&asset.absolute_path), output_path);
        assert_eq!(asset.format, "mp4");
        assert_eq!(asset.media_type, "video");
        assert!(
            output_path.is_file(),
            "runner/server drop must not delete publication"
        );
        let lineage = reopened
            .get_generation(&asset_id)?
            .ok_or("video_clip lineage missing after store reopen")?;
        assert_eq!(lineage.op, "video_clip");
        assert!(
            lineage.parent_image_id.is_none(),
            "source path is not a parent identity"
        );
        let params: serde_json::Value =
            serde_json::from_str(lineage.params.as_deref().ok_or("lineage params missing")?)?;
        assert_eq!(params["source"], source.to_string_lossy().as_ref());
        assert_eq!(params["start_sec"], 0.25);
        assert_eq!(params["end_sec"], 1.25);
        assert_eq!(params["duration_sec"], 1.0);
        assert_eq!(params["format"], "mp4");
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_clip` (audio inputs)
    /// expect: Trimming an audio input decodes to wav and publishes an
    /// AUDIO-kind asset with an audio display hint — the one Media-tab
    /// trim path serves both media kinds (2026-10-27 unification), and
    /// the output kind never mislabels the input.
    /// [P1] Constraining: stream copy cannot land compressed audio in a
    /// wav container; the decode path is the only honest trim.
    #[tokio::test]
    async fn video_clip_trims_audio_inputs_to_audio_kind_assets()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir()?;
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        let gallery_root = tempfile::tempdir()?;
        let source = gallery_root.path().join("source.mp3");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:a",
                "libmp3lame",
                "-q:a",
                "4",
                source.to_str().ok_or("mp3 fixture path is not UTF-8")?,
            ])
            .output()
            .await?;
        assert!(status.status.success(), "mp3 fixture generated");
        let db_path = artifacts.path().join("gallery.db");

        let (store, _) = file_backed_gallery_store(&db_path)?;
        let gallery = store.open(
            gallery_root
                .path()
                .to_str()
                .ok_or("gallery root is not UTF-8")?,
            GalleryMode::ReadOnly,
        )?;
        let server = server_with_gallery(store, gallery.id, gallery_root.path());
        let result = server
            .video_clip(Parameters(VideoClipRequest {
                video_url: source.to_string_lossy().into_owned(),
                start_sec: 0.25,
                end_sec: 1.25,
            }))
            .await?;
        let content = content_of(&result);
        let output_path =
            std::path::PathBuf::from(content["output"].as_str().ok_or("missing durable output")?);
        assert_eq!(
            output_path.extension().and_then(|ext| ext.to_str()),
            Some("wav"),
            "audio inputs decode to wav, never a video container"
        );
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing stable gallery_asset_id")?
            .to_string();
        let hint = media_hint_body(
            content["display_hint"]
                .as_str()
                .ok_or("missing media display hint")?,
        )?;
        assert_eq!(hint["kind"], "audio", "the hint kind follows the output");
        assert_eq!(hint["src"], content["output"]);
        assert_eq!(hint["provenance"]["tool"], "video_clip");
        let effective_params = serde_json::json!({
            "source": source.to_string_lossy(),
            "start_sec": 0.25,
            "end_sec": 1.25,
            "duration_sec": 1.0,
            "format": "wav",
        });
        assert_eq!(content["effective_params"], effective_params);

        let (reopened, _) = file_backed_gallery_store(&db_path)?;
        let asset = reopened.get_by_id(&asset_id)?;
        assert_eq!(asset.media_type, "audio");
        assert_eq!(asset.format, "wav");
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_clip`
    /// expect: A gallery-admission failure leaves none of my clipped output or database residue behind.
    /// [P1] Motivating: failed operations do not corrupt or clutter user work.
    /// pre: FFmpeg succeeds but the gallery row insert is rejected.
    /// post: the cause is surfaced and no durable file, gallery row, or lineage remains.
    /// [P1] Constraining: rollback remains armed until the gallery row is durable.
    #[tokio::test]
    async fn video_clip_gallery_failure_rolls_back_real_ffmpeg_output()
    -> Result<(), Box<dyn std::error::Error>> {
        use hkask_storage::database::driver::DatabaseDriver;
        let env = video_server_env().await?;
        env.driver.execute(
            "CREATE TRIGGER fail_video_clip_asset BEFORE INSERT ON gallery_images \
             BEGIN SELECT RAISE(ABORT, 'injected gallery failure'); END",
            &[],
        )?;

        let error = env
            .server
            .video_clip(Parameters(VideoClipRequest {
                video_url: env.source.to_string_lossy().into_owned(),
                start_sec: 0.0,
                end_sec: 1.0,
            }))
            .await
            .expect_err("gallery publication must fail");

        assert!(
            error.to_string().contains("injected gallery failure"),
            "original gallery failure cause was not preserved: {error}"
        );
        assert_eq!(env.store.count_assets(&env.gallery_id)?, 0);
        assert_eq!(env.store.count_assets(&generated_gallery(&env.store))?, 0);
        let generated = crate::assets::generated_assets_dir();
        assert_eq!(std::fs::read_dir(generated)?.count(), 0);
        Ok(())
    }

    /// dcterms:identifier: `assets::stage_local_media_publication`
    /// expect: A durable-file publication failure removes the temporary clip produced on my behalf.
    /// [P1] Motivating: failed operations leave no hidden user-work residue.
    /// pre: real FFmpeg succeeds and the generated-assets destination rejects staging.
    /// post: the staging cause is surfaced and the FFmpeg temp output is absent.
    /// [P1] Constraining: temporary and staged paths stay rollback-owned until publication.
    // TEST-DEBT: tests the crate-private staging seam because the tool result cannot expose
    // its rollback-owned FFmpeg path after the injected publication failure.
    #[tokio::test]
    async fn video_clip_file_publication_failure_rolls_back_real_ffmpeg_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        let artifacts = tempfile::tempdir()?;
        let _env = ArtifactsEnvGuard::set(artifacts.path());
        std::fs::write(
            artifacts.path().join("media-mcp"),
            b"blocks generated directory",
        )?;
        let source_dir = tempfile::tempdir()?;
        let source = source_dir.path().join("source.mp4");
        create_real_video(&source).await?;
        let runner = video::ffmpeg::FfmpegRunner::detect();
        let processor_output = runner
            .clip(source.to_str().ok_or("source path is not UTF-8")?, 0.0, 1.0)
            .await?;
        assert!(
            processor_output.is_file(),
            "real FFmpeg output must exist before staging"
        );

        let error = match crate::assets::stage_local_media_publication(
            &processor_output,
            crate::assets::LocalMediaFormat::Mp4,
        ) {
            Ok(_) => return Err("blocked generated directory unexpectedly published".into()),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("stage"),
            "original staging failure cause was not preserved: {error}"
        );
        assert!(
            !processor_output.exists(),
            "failed publication left the FFmpeg temp output behind"
        );
        assert!(artifacts.path().join("media-mcp").is_file());
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_clip`
    /// expect: A lineage-write failure removes my clipped file and its gallery identity.
    /// [P1] Motivating: provenance failure cannot leave a partially published asset.
    /// pre: real FFmpeg, durable-file publication, and gallery insertion succeed before lineage is rejected.
    /// post: the lineage cause is surfaced and no durable file, gallery row, or lineage remains.
    /// [P1] Constraining: commit occurs only after lineage is durable.
    #[tokio::test]
    async fn video_clip_lineage_failure_rolls_back_real_ffmpeg_output()
    -> Result<(), Box<dyn std::error::Error>> {
        use hkask_storage::database::driver::DatabaseDriver;

        let env = video_server_env().await?;
        env.driver.execute(
            "CREATE TRIGGER fail_video_clip_lineage BEFORE INSERT ON gallery_generation \
             WHEN NEW.op = 'video_clip' BEGIN SELECT RAISE(ABORT, 'injected lineage failure'); END",
            &[],
        )?;

        let error = env
            .server
            .video_clip(Parameters(VideoClipRequest {
                video_url: env.source.to_string_lossy().into_owned(),
                start_sec: 0.0,
                end_sec: 1.0,
            }))
            .await
            .expect_err("lineage publication must fail");

        assert!(
            error.to_string().contains("injected lineage failure"),
            "original lineage failure cause was not preserved: {error}"
        );
        assert_eq!(env.store.count_assets(&env.gallery_id)?, 0);
        let generated = crate::assets::generated_assets_dir();
        assert_eq!(std::fs::read_dir(generated)?.count(), 0);
        Ok(())
    }

    fn assert_local_publication(
        content: &serde_json::Value,
        store: &GalleryStore,
        expected_op: &str,
        expected_extension: &str,
        expected_media_type: &str,
        expected_effective_params: &serde_json::Value,
    ) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let output_path =
            std::path::PathBuf::from(content["output"].as_str().ok_or("missing durable output")?);
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing stable gallery_asset_id")?;
        let hint = media_hint_body(
            content["display_hint"]
                .as_str()
                .ok_or("missing media display hint")?,
        )?;

        assert!(output_path.starts_with(crate::assets::generated_assets_dir()));
        assert_eq!(
            output_path.extension().and_then(std::ffi::OsStr::to_str),
            Some(expected_extension)
        );
        assert!(output_path.is_file(), "durable publication is missing");
        assert_eq!(hint["src"], content["output"]);
        assert_eq!(hint["gallery_asset_id"], asset_id);
        assert_eq!(hint["ontology"], hkask_bridge_ontology::omc::SEQUENCE);
        assert_eq!(hint["kind"], expected_media_type);
        assert_eq!(hint["provenance"]["tool"], expected_op);
        assert_eq!(
            hint["provenance"]["args"], *expected_effective_params,
            "media-block provenance must use the authoritative effective parameters"
        );
        assert_eq!(
            content["effective_params"], *expected_effective_params,
            "result metadata must use the authoritative effective parameters"
        );

        let asset = store.get_by_id(asset_id)?;
        assert_eq!(asset.gallery_id, generated_gallery(store));
        assert_eq!(std::path::Path::new(&asset.absolute_path), output_path);
        assert_eq!(asset.format, expected_extension);
        assert_eq!(asset.media_type, expected_media_type);
        let lineage = store
            .get_generation(asset_id)?
            .ok_or("local video lineage missing")?;
        assert_eq!(lineage.op, expected_op);
        assert!(
            lineage.parent_image_id.is_none(),
            "source paths or indices must not be promoted to a parent identity"
        );
        let params: serde_json::Value =
            serde_json::from_str(lineage.params.as_deref().ok_or("lineage params missing")?)?;
        assert_eq!(&params, expected_effective_params);
        let graph_record = store
            .get_omc_creation_graph(asset_id)?
            .ok_or("OMC creation graph missing")?;
        let graph: crate::omc::CreationGraph = serde_json::from_str(&graph_record.graph_json)?;
        assert_eq!(graph.asset_id, asset_id);
        assert_eq!(graph.task_id, lineage.id);
        assert_eq!(graph.participant_id, "service:hkask-mcp-media");
        assert_eq!(graph.relationships.len(), 7);
        Ok(output_path)
    }

    fn collect_temp_media_files(
        root: &std::path::Path,
        files: &mut std::collections::HashSet<std::path::PathBuf>,
    ) -> Result<(), std::io::Error> {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                collect_temp_media_files(&path, files)?;
            } else {
                files.insert(path);
            }
        }
        Ok(())
    }

    /// The hermetic intermediate-observation scope: one ffmpeg runner's
    /// per-instance temp directory, never the shared `hkask-media` root.
    /// The root is written by every concurrent runner on the machine —
    /// sibling tests in the suite and any live media server — so
    /// snapshotting it turned these cleanup assertions into races against
    /// unrelated processes (the recorded intermittent EDL-render flake:
    /// green in isolation, red in full runs when a sibling runner's file
    /// landed between the before/after snapshots).
    fn temp_media_files(
        runner_temp: &std::path::Path,
    ) -> Result<std::collections::HashSet<std::path::PathBuf>, std::io::Error> {
        let mut files = std::collections::HashSet::new();
        collect_temp_media_files(runner_temp, &mut files)?;
        Ok(files)
    }

    /// dcterms:identifier: `MediaServer::video_to_gif`
    /// expect: My converted GIF remains addressable by one stable gallery identity after restart.
    /// [P1] Motivating: user work survives processor and server teardown.
    /// pre: a real source video and active file-backed gallery exist.
    /// post: the durable GIF, gallery row, lineage, result id, and media-block id agree after server drop.
    /// [P1] Constraining: source paths never become parent identities.
    #[tokio::test]
    async fn video_to_gif_publishes_durable_asset_and_lineage_after_server_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let env = video_server_env().await?;
        let content = content_of(
            &env.server
                .video_to_gif(Parameters(VideoToGifRequest {
                    video_url: env.source.to_string_lossy().into_owned(),
                    start_sec: Some(0.25),
                    duration_sec: Some(1.0),
                    width: Some(48),
                    fps: Some(8),
                }))
                .await?,
        );
        drop(env.server);

        assert_local_publication(
            &content,
            &env.store,
            "video_to_gif",
            "gif",
            "image",
            &serde_json::json!({
                "source": env.source.to_string_lossy(),
                "start_sec": 0.25,
                "duration_sec": 1.0,
                "width": 48,
                "fps": 8,
                "format": "gif",
            }),
        )?;
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_add_caption`
    /// expect: My captioned video remains addressable by one stable gallery identity after restart.
    /// [P1] Motivating: user work survives processor and server teardown.
    /// pre: a real source video and active file-backed gallery exist.
    /// post: the durable MP4, gallery row, complete lineage, result id, and media-block id agree.
    /// [P1] Constraining: source paths never become parent identities.
    #[tokio::test]
    async fn video_add_caption_publishes_durable_asset_and_lineage_after_server_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let env = video_server_env().await?;
        let content = content_of(
            &env.server
                .video_add_caption(Parameters(VideoAddCaptionRequest {
                    video_url: env.source.to_string_lossy().into_owned(),
                    text: "durable caption".to_string(),
                    position: Some("top".to_string()),
                    font_size: Some(18),
                }))
                .await?,
        );
        drop(env.server);

        assert_local_publication(
            &content,
            &env.store,
            "video_add_caption",
            "mp4",
            "video",
            &serde_json::json!({
                "source": env.source.to_string_lossy(),
                "text": "durable caption",
                "position": "top",
                "font_size": 18,
                "format": "mp4",
            }),
        )?;
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_concat`
    /// expect: My concatenated clips become one durable gallery video with traceable sources.
    /// [P1] Motivating: user work survives processor and server teardown.
    /// pre: two real source videos and an active file-backed gallery exist.
    /// post: the durable MP4 and complete ordered source lineage survive server drop.
    /// [P1] Constraining: source paths never become parent identities.
    #[tokio::test]
    async fn video_concat_publishes_durable_asset_and_lineage_after_server_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let first = gallery_root.path().join("first.mp4");
        let second = gallery_root.path().join("second.mp4");
        create_real_video(&first).await?;
        create_real_video(&second).await?;
        let sources = vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ];
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let content = content_of(
            &server
                .video_concat(Parameters(VideoConcatRequest {
                    video_urls: sources.clone(),
                }))
                .await?,
        );
        drop(server);

        assert_local_publication(
            &content,
            &store,
            "video_concat",
            "mp4",
            "video",
            &serde_json::json!({"sources": sources, "format": "mp4"}),
        )?;
        Ok(())
    }

    /// dcterms:identifier: `assets::stage_local_media_publication`
    /// expect: Any local-video gallery failure rolls back the final file while preserving its original cause.
    /// [P1] Motivating: failed publication leaves no partial user work.
    /// pre: real FFmpeg produces a GIF but the gallery row insert is rejected.
    /// post: the original gallery cause is returned and no generated file, gallery row, or lineage remains.
    /// [P1] Constraining: rollback stays armed through lineage publication.
    #[tokio::test]
    async fn local_video_publication_failure_rolls_back_gif_and_intermediates()
    -> Result<(), Box<dyn std::error::Error>> {
        use hkask_storage::database::driver::DatabaseDriver;
        let env = video_server_env().await?;
        let before = temp_media_files(env.server.ffmpeg.temp_dir())?;
        env.driver.execute(
            "CREATE TRIGGER fail_gif_asset BEFORE INSERT ON gallery_images \
             BEGIN SELECT RAISE(ABORT, 'injected gallery failure'); END",
            &[],
        )?;

        let error = env
            .server
            .video_to_gif(Parameters(VideoToGifRequest {
                video_url: env.source.to_string_lossy().into_owned(),
                start_sec: Some(0.0),
                duration_sec: Some(1.0),
                width: Some(48),
                fps: Some(8),
            }))
            .await
            .expect_err("gallery publication must fail");

        assert!(
            error.to_string().contains("injected gallery failure"),
            "original gallery cause was not preserved: {error}"
        );
        assert_eq!(env.store.count_assets(&env.gallery_id)?, 0);
        assert_eq!(env.store.count_assets(&generated_gallery(&env.store))?, 0);
        assert_eq!(
            std::fs::read_dir(crate::assets::generated_assets_dir())?.count(),
            0
        );
        let after = temp_media_files(env.server.ffmpeg.temp_dir())?;
        assert_eq!(
            after.difference(&before).count(),
            0,
            "failed GIF publication left FFmpeg files: {:?}",
            after.difference(&before).collect::<Vec<_>>()
        );
        Ok(())
    }

    /// dcterms:identifier: `assets::publish_local_media`
    /// expect: A lineage failure removes the migrated operation's final file and gallery identity.
    /// [P1] Motivating: provenance failure cannot leave partially published user work.
    /// pre: real FFmpeg, durable GIF staging, and gallery insertion succeed before lineage is rejected.
    /// post: the original lineage cause is surfaced and file, gallery row, and lineage are absent.
    /// [P1] Constraining: commit occurs only after lineage is durable.
    #[tokio::test]
    async fn local_video_lineage_failure_rolls_back_file_gallery_and_lineage()
    -> Result<(), Box<dyn std::error::Error>> {
        use hkask_storage::database::driver::DatabaseDriver;

        let env = video_server_env().await?;
        env.driver.execute(
            "CREATE TRIGGER fail_local_video_lineage BEFORE INSERT ON gallery_generation \
             WHEN NEW.op = 'video_to_gif' BEGIN SELECT RAISE(ABORT, 'injected migrated lineage failure'); END",
            &[],
        )?;

        let error = env
            .server
            .video_to_gif(Parameters(VideoToGifRequest {
                video_url: env.source.to_string_lossy().into_owned(),
                start_sec: Some(0.0),
                duration_sec: Some(1.0),
                width: Some(48),
                fps: Some(8),
            }))
            .await
            .expect_err("lineage publication must fail");

        assert!(
            error
                .to_string()
                .contains("injected migrated lineage failure"),
            "original lineage failure cause was not preserved: {error}"
        );
        assert_eq!(env.store.count_assets(&env.gallery_id)?, 0);
        assert_eq!(
            std::fs::read_dir(crate::assets::generated_assets_dir())?.count(),
            0
        );
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::video_clip`
    /// expect: Produced video is filed under the generated gallery, whatever gallery is active.
    /// [P1] Motivating: one canonical home for every output the media server makes.
    /// pre: Gallery A is active, remote validation is delayed, and Gallery B becomes active before processing resumes.
    /// post: the output is indexed only in the gallery whose root holds `media-mcp/generated/`.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_video_output_files_under_the_generated_gallery()
    -> Result<(), Box<dyn std::error::Error>> {
        let _env_lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
        // Field access — the keep-alive fixture fields stay bound in `arms`
        // (a `..` rest drops the artifacts dir before the operation spawns).
        let arms = two_gallery_arms().await?;
        let _gate = crate::tools::processing::install_local_video_validation_gate(
            crate::tools::processing::LocalVideoValidationGate {
                entered: arms.entered.clone(),
                resume: arms.resume.clone(),
                bypass_dns: true,
            },
        )?;
        let operation = {
            let server = arms.server.clone();
            tokio::spawn(async move {
                server
                    .video_clip(Parameters(VideoClipRequest {
                        video_url: "https://example.com/source.mp4".to_string(),
                        start_sec: 0.0,
                        end_sec: 1.0,
                    }))
                    .await
            })
        };
        let (_content, asset_id) = switch_to_gallery_b_and_finish(
            &arms.entered,
            &arms.resume,
            &arms.gallery_state,
            arms.gallery_b_root.path(),
            &arms.gallery_b.id,
            operation,
        )
        .await?;

        assert_eq!(
            arms.store.get_by_id(&asset_id)?.gallery_id,
            generated_gallery(&arms.store)
        );
        assert_eq!(arms.store.count_assets(&arms.gallery_a.id)?, 0);
        assert_eq!(arms.store.count_assets(&arms.gallery_b.id)?, 0);
        Ok(())
    }

    /// A mock inference port returning canned responses on both paths —
    /// `generate` (the text passes) and `media_generate` (the audio-chat
    /// path) — so the full pass pipelines run end-to-end without a live
    /// model. When `media_response_is_json` is set, `media_generate` returns
    /// the canned string as a full provider-response JSON object instead
    /// (the STT shape: text/words/segments/duration) — the
    /// transcribe_and_store path.
    struct MockInferencePort {
        response: String,
        media_response: String,
        media_response_is_json: bool,
        audio_urls: Arc<std::sync::Mutex<Vec<Option<String>>>>,
    }

    crate::test_support::media_inference_stub! {
        MockInferencePort,
        generate(self, _prompt): {
            let result = hkask_types::InferenceResult {
                text: self.response.clone(),
                model: "mock-model".to_string(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                    reported: true,
                },
                finish_reason: "stop".to_string(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            };
            Box::pin(async move { Ok(result) })
        },
        media_generate(self, _op, params): {
            self.audio_urls
                .lock()
                .expect("audio URL recorder")
                .push(params.audio_url.clone());
            if self.media_response_is_json {
                let value = serde_json::from_str::<serde_json::Value>(&self.media_response)
                    .expect("media_response_is_json requires valid JSON");
                Box::pin(async move { Ok(value) })
            } else {
                let text = self.media_response.clone();
                Box::pin(
                    async move { Ok(serde_json::json!({ "text": text, "model": "mock-model" })) },
                )
            }
        }
    }

    /// Points `HKASK_ARTIFACTS_DIR` at a fresh temp dir for one test and
    /// restores the prior value on drop, so tests that publish exports never
    /// write into the real ~/Documents/zk-data tree. Holds the env lock for
    /// the test's whole lifetime.
    struct IsolatedArtifacts {
        _lock: tokio::sync::MutexGuard<'static, ()>,
        _temp: tempfile::TempDir,
        prior: Option<String>,
    }

    impl IsolatedArtifacts {
        async fn new() -> Self {
            let lock = crate::ARTIFACTS_ENV_LOCK.lock().await;
            let temp = tempfile::TempDir::new().expect("tempdir for artifacts isolation");
            let prior = std::env::var("HKASK_ARTIFACTS_DIR").ok();
            unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", temp.path()) };
            Self {
                _lock: lock,
                _temp: temp,
                prior,
            }
        }
    }

    impl Drop for IsolatedArtifacts {
        fn drop(&mut self) {
            match &self.prior {
                Some(value) => unsafe { std::env::set_var("HKASK_ARTIFACTS_DIR", value) },
                None => unsafe { std::env::remove_var("HKASK_ARTIFACTS_DIR") },
            }
        }
    }

    /// A server wired with the mock inference port — the capability under
    /// test (the pass pipeline needs a real generate path, not the noop).
    fn make_pass_server(response: String) -> MediaServer {
        make_inference_server(response.clone(), response)
    }

    /// A server with distinct canned responses for the text path
    /// (`generate`) and the media path (`media_generate` — the audio-chat
    /// source).
    fn make_inference_server(response: String, media_response: String) -> MediaServer {
        make_server_with_port(Arc::new(MockInferencePort {
            response,
            media_response,
            media_response_is_json: false,
            audio_urls: Arc::new(std::sync::Mutex::new(Vec::new())),
        }))
    }

    /// A server whose `media_generate` returns the canned string as a full
    /// provider-response JSON object as-is — the STT shape the
    /// transcribe_and_store path consumes.
    fn make_stt_server(media_json: String) -> MediaServer {
        make_server_with_port(Arc::new(MockInferencePort {
            response: String::new(),
            media_response: media_json,
            media_response_is_json: true,
            audio_urls: Arc::new(std::sync::Mutex::new(Vec::new())),
        }))
    }

    fn make_server_with_port(port: Arc<dyn hkask_types::ports::InferencePort>) -> MediaServer {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let gallery_store =
            Arc::new(GalleryStore::from_driver(driver).expect("gallery store init"));
        MediaServer::new(
            hkask_types::WebID::new(),
            port,
            Arc::new(std::sync::Mutex::new(None)),
            gallery_store,
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        )
    }

    /// transcribe_and_store composes transcription + storage server-side and
    /// returns ONLY the summary — the long-media path (an hour-long bundle
    /// is ~550KB of JSON; the two-step flow relays it through the model
    /// context at ~140K tokens each way). The stored record must carry the
    /// full word-timed bundle with separator-prefixed tokens trimmed (the
    /// rendered-form contract), verified by loading it back through
    /// educt_get_transcript. The fixture is a real local file, so the test
    /// also pins the local-path acceptance at the tool boundary.
    #[tokio::test]
    async fn transcribe_and_store_returns_summary_and_stores_word_timed_bundle() {
        let media_json = serde_json::json!({
            "text": " alpha  beta ",
            "duration": 1.0,
            "words": [
                {"word": " alpha", "start": 0.0, "end": 0.5},
                {"word": " beta", "start": 0.5, "end": 1.0}
            ],
            "segments": [
                {"text": " alpha  beta ", "start": 0.0, "end": 1.0}
            ]
        })
        .to_string();
        let audio_urls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let server = make_server_with_port(Arc::new(MockInferencePort {
            response: String::new(),
            media_response: media_json,
            media_response_is_json: true,
            audio_urls: audio_urls.clone(),
        }));

        let fixture = std::env::temp_dir().join(format!(
            "transcribe-and-store-{}-{}.m4a",
            std::process::id(),
            line!()
        ));
        std::fs::write(&fixture, b"fixture").expect("fixture write");
        let audio_url = fixture.to_str().expect("utf-8 path").to_string();

        let result = server
            .transcribe_and_store(Parameters(crate::types::TranscribeAndStoreRequest {
                audio_url: audio_url.clone(),
                language: Some("en".to_string()),
                gallery_asset_id: None,
            }))
            .await
            .expect("transcribe_and_store succeeds");
        let content = content_of(&result);
        assert_eq!(
            *audio_urls.lock().expect("audio URL recorder"),
            vec![Some(audio_url)]
        );
        assert_eq!(content["words_count"], serde_json::json!(2));
        assert_eq!(content["has_word_timings"], serde_json::json!(true));
        let transcript_id = content["id"].as_str().expect("id present").to_string();

        let stored = server
            .educt_get_transcript(Parameters(crate::types::EductGetTranscriptRequest {
                transcript_id,
                include_layers: Some(false),
            }))
            .await
            .expect("get transcript succeeds");
        let stored = content_of(&stored);
        let words = stored["transcript"]["words"]
            .as_array()
            .expect("words array");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0]["word"], serde_json::json!("alpha"));
        assert_eq!(words[0]["start_ms"], serde_json::json!(0));
        assert_eq!(words[1]["end_ms"], serde_json::json!(1000));
    }

    /// The two-word timed bundle ("alpha" 0-500ms, "beta" 500-900ms) every
    /// educt store test stages — the fixture behind `store_two_word_transcript`
    /// and the stringified-transport pin, which vary only the transport form.
    fn two_word_bundle() -> crate::transcript::TranscriptBundle {
        use crate::transcript::{TimedWord, TranscriptBundle};
        TranscriptBundle {
            words: vec![
                TimedWord {
                    word: "alpha".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    confidence: None,
                },
                TimedWord {
                    word: "beta".to_string(),
                    start_ms: 500,
                    end_ms: 900,
                    confidence: None,
                },
            ],
            ..TranscriptBundle::new("/tmp/a.wav".to_string(), 1.0, "alpha beta".to_string())
        }
    }

    /// Store a two-word transcript and return its ID.
    async fn store_two_word_transcript(server: &MediaServer) -> String {
        use crate::types::EductStoreTranscriptRequest;
        let bundle = two_word_bundle();
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store succeeds");
        content_of(&stored)["id"]
            .as_str()
            .expect("id present")
            .to_string()
    }

    /// Store an N-word timed transcript — the windowed-pass fixture.
    async fn store_n_word_transcript(server: &MediaServer, count: usize) -> String {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::types::EductStoreTranscriptRequest;
        let words: Vec<TimedWord> = (0..count)
            .map(|index| TimedWord {
                word: format!("w{index}"),
                start_ms: index as u64 * 100,
                end_ms: index as u64 * 100 + 90,
                confidence: None,
            })
            .collect();
        let bundle = TranscriptBundle {
            words,
            ..TranscriptBundle::new(
                "/tmp/long.wav".to_string(),
                count as f32 * 0.1,
                String::new(),
            )
        };
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store succeeds");
        content_of(&stored)["id"]
            .as_str()
            .expect("id present")
            .to_string()
    }

    /// dcterms:identifier: `transcript_pass::run_paragraph_pass` (windowed)
    /// expect: Long transcripts pass in windows with breaks offset back to
    /// transcript space — the 10,434-word single-call cliff class cannot
    /// recur.
    /// [P1] Motivating: the Dunning long-form sample's paragraph layer was
    /// structurally valid but semantically broken in one call.
    #[tokio::test]
    async fn educt_paragraph_pass_windows_long_transcripts() {
        // 4,500 words over the 2,000-word window → three pass calls, each
        // returning the same canned local break [10]; merged and offset:
        // [10, 2010, 4010].
        let server = make_pass_server("{\"breaks_after\": [10]}".to_string());
        let transcript_id = store_n_word_transcript(&server, 4_500).await;
        let result = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("pass succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("paragraph")
        );
        assert_eq!(
            content["stored"]["layer"]["breaks_after"],
            serde_json::json!([10, 2010, 4010]),
            "windowed breaks are offset back to transcript space"
        );
    }

    #[tokio::test]
    async fn educt_paragraph_pass_stores_a_valid_layer() {
        let server = make_pass_server("{\"breaks_after\": [0]}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("pass succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("paragraph")
        );
        assert_eq!(
            content["stored"]["layer"]["breaks_after"],
            serde_json::json!([0])
        );
        assert!(
            content["pass_stats"]["attempts"].as_u64().unwrap_or(0) >= 1,
            "pass stats must count the attempt: {}",
            content["pass_stats"]
        );
        // The layer is recallable through the store.
        let layers = server
            .educt_list_layers(Parameters(crate::types::EductListLayersRequest {
                transcript_id,
            }))
            .await
            .expect("list layers succeeds");
        assert_eq!(content_of(&layers)["count"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn educt_paragraph_pass_rejects_out_of_bounds_indices() {
        let server = make_pass_server("{\"breaks_after\": [99]}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect_err("out-of-bounds index must be rejected");
        assert!(
            error.message.contains("out of bounds"),
            "the named invariant must surface: {}",
            error.message
        );
        // Nothing persisted.
        let layers = server
            .educt_list_layers(Parameters(crate::types::EductListLayersRequest {
                transcript_id,
            }))
            .await
            .expect("list layers succeeds");
        assert_eq!(content_of(&layers)["count"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn educt_paragraph_pass_surfaces_unparseable_output() {
        let server = make_pass_server("I cannot produce that.".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect_err("unparseable output must surface");
        assert!(
            error.message.contains("failed to parse"),
            "the parse failure must be named: {}",
            error.message
        );
        let layers = server
            .educt_list_layers(Parameters(crate::types::EductListLayersRequest {
                transcript_id,
            }))
            .await
            .expect("list layers succeeds");
        assert_eq!(content_of(&layers)["count"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn educt_paragraph_pass_requires_word_timings() {
        // The mock WOULD return valid output — the precondition must reject
        // before any model call.
        let server = make_pass_server("{\"breaks_after\": [0]}".to_string());
        use crate::transcript::TranscriptBundle;
        use crate::types::EductStoreTranscriptRequest;
        let degraded = TranscriptBundle::new("/tmp/silent.wav".to_string(), 0.0, String::new());
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&degraded).expect("serialize"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("degraded transcript is stored with a surfaced note");
        let transcript_id = content_of(&stored)["id"].as_str().expect("id").to_string();
        let error = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect_err("no word timings must reject");
        assert!(
            error.message.contains("no word-level timings"),
            "the degradation must be named: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_paragraph_pass_on_missing_transcript_is_not_found() {
        let server = make_pass_server("{\"breaks_after\": [0]}".to_string());
        let error = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id: "no-such-transcript".to_string(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect_err("missing transcript must be not-found");
        assert!(
            error.message.contains("not found"),
            "not-found must be named: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_speaker_pass_stores_a_valid_layer() {
        let server = make_pass_server(
            r#"{"spans": [{"start_word": 0, "end_word": 0, "speaker": "speaker-1", "confidence": 0.9}, {"start_word": 1, "end_word": 1, "speaker": "speaker-2", "confidence": 0.8}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                source: Some("text".to_string()),
                structured: None,
                correct_existing: None,
            }))
            .await
            .expect("pass succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("speaker")
        );
        assert_eq!(
            content["stored"]["layer"]["spans"].as_array().map(Vec::len),
            Some(2)
        );
        // The text source names the text-cue template in provenance.
        assert_eq!(
            content["stored"]["layer"]["provenance"]["prompt_template"],
            serde_json::json!("educt_speaker_pass")
        );
        // The layer is recallable through the store.
        let layers = server
            .educt_list_layers(Parameters(crate::types::EductListLayersRequest {
                transcript_id,
            }))
            .await
            .expect("list layers succeeds");
        assert_eq!(content_of(&layers)["count"].as_u64(), Some(1));
    }

    /// Assert a rejected pass surfaces its named invariant and stores no
    /// layer — the shared tail of the speaker/correction rejection tests,
    /// varying exactly the invariant substring.
    async fn assert_rejection_stores_no_layer(
        server: &MediaServer,
        transcript_id: &str,
        message: &str,
        invariant: &str,
    ) {
        assert!(
            message.contains(invariant),
            "the named invariant must surface: {message}"
        );
        let layers = server
            .educt_list_layers(Parameters(crate::types::EductListLayersRequest {
                transcript_id: transcript_id.to_string(),
            }))
            .await
            .expect("list layers succeeds");
        assert_eq!(content_of(&layers)["count"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn educt_speaker_pass_rejects_overlapping_spans() {
        let server = make_pass_server(
            r#"{"spans": [{"start_word": 0, "end_word": 1, "speaker": "speaker-1", "confidence": 0.9}, {"start_word": 1, "end_word": 1, "speaker": "speaker-2", "confidence": 0.8}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                source: Some("text".to_string()),
                structured: None,
                correct_existing: None,
            }))
            .await
            .expect_err("overlapping spans must be rejected");
        assert_rejection_stores_no_layer(&server, &transcript_id, &error.message, "overlapping")
            .await;
    }

    #[tokio::test]
    async fn educt_speaker_pass_defaults_to_the_audio_source() {
        let server = make_inference_server(
            "unused".to_string(),
            r#"{"spans": [{"start_word": 0, "end_word": 0, "speaker": "speaker-1", "confidence": 0.95}, {"start_word": 1, "end_word": 1, "speaker": "speaker-2", "confidence": 0.9}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: None,
                structured: None,
                correct_existing: None,
            }))
            .await
            .expect("audio pass succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("speaker")
        );
        // Provenance names the audio template — the source is
        // distinguishable, exactly as decision 7 specified.
        assert_eq!(
            content["stored"]["layer"]["provenance"]["prompt_template"],
            serde_json::json!("educt_speaker_audio_pass")
        );
    }

    #[tokio::test]
    async fn educt_speaker_pass_rejects_unknown_source() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: Some("bogus".to_string()),
                structured: None,
                correct_existing: None,
            }))
            .await
            .expect_err("unknown source must be rejected");
        assert!(
            error.message.contains("source must be"),
            "the named error must surface: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_speaker_pass_audio_unparseable_output_surfaces() {
        let server = make_inference_server(
            "unused".to_string(),
            "I cannot listen to this audio.".to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: Some("audio".to_string()),
                structured: None,
                correct_existing: None,
            }))
            .await
            .expect_err("unparseable audio output must surface");
        assert!(
            error.message.contains("failed to parse"),
            "the parse failure must be named: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_correction_pass_stores_and_applies() {
        let server = make_pass_server(
            r#"{"edits": [{"start_word": 0, "end_word": 0, "replacement": "Alpha", "reason": "capitalization"}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let stored = server
            .educt_correction_pass(Parameters(crate::types::EductCorrectionPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("pass succeeds");
        let content = content_of(&stored);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("correction")
        );

        // Apply: the derived corrected view, words untouched.
        let applied = server
            .educt_apply_corrections(Parameters(crate::types::EductApplyCorrectionsRequest {
                transcript_id,
                layer_id: None,
            }))
            .await
            .expect("apply succeeds");
        let applied = content_of(&applied);
        assert_eq!(applied["corrected_text"], serde_json::json!("Alpha beta"));
        assert_eq!(applied["applied_layer"]["edits"].as_u64(), Some(1));
    }

    /// dcterms:identifier: `MediaServer::educt_locate`
    /// expect: After an aligned correction, I navigate and export the corrected working transcript rather than stale source text.
    /// [P1] Motivating: Transcript correction changes the text users work with without changing its timing evidence.
    #[tokio::test]
    async fn aligned_correction_drives_inspection_locate_and_srt_export() {
        let _artifacts = IsolatedArtifacts::new().await;
        let server = make_pass_server(
            r#"{"edits": [{"start_word": 0, "end_word": 0, "replacement": "  Alpha  ", "reason": "capitalization"}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        server
            .educt_correction_pass(Parameters(crate::types::EductCorrectionPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("correction stored");

        let inspected = server
            .educt_get_transcript(Parameters(crate::types::EductGetTranscriptRequest {
                transcript_id: transcript_id.clone(),
                include_layers: Some(false),
            }))
            .await
            .expect("transcript inspection");
        let inspected = content_of(&inspected);
        assert_eq!(inspected["working_transcript"]["text"], "Alpha beta");
        assert_eq!(inspected["working_transcript"]["alignment"], "aligned");
        assert_eq!(inspected["transcript"]["words"][0]["word"], "alpha");
        assert_eq!(inspected["working_transcript"]["words"][0]["word"], "Alpha");
        assert_eq!(
            inspected["working_transcript"]["words"][0]["start_ms"],
            inspected["transcript"]["words"][0]["start_ms"]
        );

        let located = server
            .educt_locate(Parameters(crate::types::EductLocateRequest {
                transcript_id: transcript_id.clone(),
                text: "Alpha beta".to_string(),
            }))
            .await
            .expect("corrected quote locates");
        assert_eq!(content_of(&located)["count"].as_u64(), Some(1));
        let stale = server
            .educt_locate(Parameters(crate::types::EductLocateRequest {
                transcript_id: transcript_id.clone(),
                text: "alpha beta".to_string(),
            }))
            .await
            .expect("stale quote returns no-match");
        assert_eq!(content_of(&stale)["status"], "no_match");

        let exported = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id: transcript_id.clone(),
                format: "srt".to_string(),
            }))
            .await
            .expect("corrected SRT export");
        let exported = content_of(&exported);
        let srt = std::fs::read_to_string(exported["output"].as_str().expect("SRT path"))
            .expect("read SRT");
        assert!(srt.contains("Alpha beta"), "{srt}");
        assert!(exported["correction_layer_id"].is_string());

        let corpus = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id,
                format: "corpus_text".to_string(),
            }))
            .await
            .expect("aligned corpus export");
        let corpus = content_of(&corpus);
        assert_eq!(
            std::fs::read_to_string(corpus["output"].as_str().expect("corpus path"))
                .expect("read corpus text"),
            "Alpha beta"
        );
        assert_eq!(corpus["correction_alignment"], "aligned");
        assert_eq!(corpus["word_timings"], true);
        assert!(corpus.get("degradation").is_none());
    }

    /// dcterms:identifier: `MediaServer::educt_highlight_pass`
    /// expect: Semantic highlighting reads the corrected working words, not stale transcription text.
    /// [P1] Motivating: Agent-selected highlights must describe the transcript the user sees.
    #[tokio::test]
    async fn aligned_correction_drives_semantic_highlight_prompt() {
        struct CapturingInference {
            prompt: Arc<std::sync::Mutex<Option<String>>>,
        }
        crate::test_support::media_inference_stub! {
            CapturingInference,
            generate(self, prompt): {
                *self.prompt.lock().expect("capture prompt") = Some(prompt.to_string());
                Box::pin(async {
                    Ok(hkask_types::InferenceResult {
                        text: r#"{"highlights":[{"start_word":0,"end_word":1,"label":"corrected","note":""}]}}"#.to_string(),
                        model: "mock-model".to_string(),
                        usage: Default::default(),
                        finish_reason: "stop".to_string(),
                        tool_calls: Vec::new(),
                        reasoning: None,
                        cost_usd: None,
                    })
                })
            }
        }

        let captured = Arc::new(std::sync::Mutex::new(None));
        let server = make_server_with_port(Arc::new(CapturingInference {
            prompt: captured.clone(),
        }));
        let transcript_id = store_two_word_transcript(&server).await;
        let correction = crate::transcript_layers::TranscriptLayer::Correction(
            crate::transcript_layers::CorrectionLayer {
                provenance: crate::transcript_layers::LayerProvenance {
                    model: "operator".to_string(),
                    prompt_template: "manual".to_string(),
                    created_at: "2026-09-14T00:00:00Z".to_string(),
                },
                edits: vec![crate::transcript_layers::CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "Alpha".to_string(),
                    reason: "capitalization".to_string(),
                }],
            },
        );
        server
            .educt_store_layer(Parameters(crate::types::EductStoreLayerRequest {
                transcript_id: transcript_id.clone(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(correction).expect("serialize correction"),
                ),
            }))
            .await
            .expect("store correction");

        server
            .educt_highlight_pass(Parameters(crate::types::EductHighlightPassRequest {
                transcript_id,
                request: "find corrected words".to_string(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("highlight corrected transcript");

        let prompt = captured
            .lock()
            .expect("read prompt")
            .clone()
            .expect("prompt captured");
        assert!(prompt.contains("0:Alpha 1:beta"), "{prompt}");
        assert!(!prompt.contains("0:alpha 1:beta"), "{prompt}");
    }

    /// dcterms:identifier: `MediaServer::educt_locate`
    /// expect: A correction without a safe word-to-timing map stays readable but cannot masquerade as timed navigation or captions.
    /// [P1] Motivating: The system surfaces alignment loss instead of inventing media precision.
    #[tokio::test]
    async fn unaligned_correction_blocks_timed_consumers_and_exports_text_with_degradation() {
        let _artifacts = IsolatedArtifacts::new().await;
        let server = make_pass_server(
            r#"{"edits": [{"start_word": 0, "end_word": 1, "replacement": "AlphaBeta", "reason": "merged speech"}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        server
            .educt_correction_pass(Parameters(crate::types::EductCorrectionPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("correction stored");

        let applied = server
            .educt_apply_corrections(Parameters(crate::types::EductApplyCorrectionsRequest {
                transcript_id: transcript_id.clone(),
                layer_id: None,
            }))
            .await
            .expect("unaligned view remains readable");
        let applied = content_of(&applied);
        assert_eq!(applied["corrected_text"], "AlphaBeta");
        assert_eq!(applied["alignment"], "unaligned");
        assert!(applied["alignment_error"].as_str().is_some());
        assert!(applied["working_words"].is_null());

        let locate_error = server
            .educt_locate(Parameters(crate::types::EductLocateRequest {
                transcript_id: transcript_id.clone(),
                text: "AlphaBeta".to_string(),
            }))
            .await
            .expect_err("unaligned locate must fail");
        assert_eq!(
            locate_error.kind,
            hkask_types::McpErrorKind::FailedPrecondition
        );
        assert!(locate_error.message.contains("2 timed words with 1 tokens"));

        let highlight_error = server
            .educt_highlight_pass(Parameters(crate::types::EductHighlightPassRequest {
                transcript_id: transcript_id.clone(),
                request: "find the merged phrase".to_string(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect_err("unaligned highlight must fail before inference");
        assert_eq!(
            highlight_error.kind,
            hkask_types::McpErrorKind::FailedPrecondition
        );

        let srt_error = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id: transcript_id.clone(),
                format: "srt".to_string(),
            }))
            .await
            .expect_err("unaligned SRT must fail");
        assert_eq!(
            srt_error.kind,
            hkask_types::McpErrorKind::FailedPrecondition
        );

        let corpus = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id,
                format: "corpus_text".to_string(),
            }))
            .await
            .expect("corrected text-only corpus export");
        let corpus = content_of(&corpus);
        assert_eq!(
            std::fs::read_to_string(corpus["output"].as_str().expect("corpus path"))
                .expect("read corpus text"),
            "AlphaBeta"
        );
        assert_eq!(corpus["correction_alignment"], "unaligned");
        assert_eq!(corpus["word_timings"], false);
        assert!(corpus["degradation"].as_str().is_some());
    }

    #[tokio::test]
    async fn educt_correction_pass_rejects_out_of_bounds_edits() {
        let server = make_pass_server(
            r#"{"edits": [{"start_word": 0, "end_word": 99, "replacement": "x", "reason": "r"}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_correction_pass(Parameters(crate::types::EductCorrectionPassRequest {
                transcript_id: transcript_id.clone(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect_err("out-of-bounds edit must be rejected");
        assert_rejection_stores_no_layer(&server, &transcript_id, &error.message, "out of bounds")
            .await;
    }

    #[tokio::test]
    async fn educt_apply_corrections_without_a_layer_is_not_found() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_apply_corrections(Parameters(crate::types::EductApplyCorrectionsRequest {
                transcript_id,
                layer_id: None,
            }))
            .await
            .expect_err("no correction layer must be not-found");
        assert!(
            error.message.contains("no correction layer"),
            "the missing layer must be named: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_highlight_pass_stores_a_selection() {
        let server = make_pass_server(
            r#"{"highlights": [{"start_word": 0, "end_word": 1, "label": "key-argument", "note": "explains the curve"}]}"#
                .to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_highlight_pass(Parameters(crate::types::EductHighlightPassRequest {
                transcript_id: transcript_id.clone(),
                request: "where he explains the curve".to_string(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("pass succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("highlight")
        );
        assert_eq!(
            content["stored"]["layer"]["highlights"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        // The layer is recallable through the store.
        let layers = server
            .educt_list_layers(Parameters(crate::types::EductListLayersRequest {
                transcript_id,
            }))
            .await
            .expect("list layers succeeds");
        assert_eq!(content_of(&layers)["count"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn educt_paragraph_pass_structured_mode_stores_via_chat_json() {
        let server =
            make_inference_server("unused".to_string(), "{\"breaks_after\": [0]}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                structured: Some(true),
            }))
            .await
            .expect("structured pass succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["stored"]["layer"]["kind"],
            serde_json::json!("paragraph")
        );
        // The structured A/B measurement counts the attempt.
        assert!(
            content["pass_stats"]["structured"]["attempts"]
                .as_u64()
                .unwrap_or(0)
                >= 1,
            "structured stats must count the attempt: {}",
            content["pass_stats"]
        );
    }

    #[tokio::test]
    async fn educt_paragraph_pass_structured_unparseable_surfaces() {
        let server = make_inference_server(
            "unused".to_string(),
            "I refuse to emit schema-conforming JSON.".to_string(),
        );
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                structured: Some(true),
            }))
            .await
            .expect_err("unparseable structured output must surface");
        assert!(
            error.message.contains("failed to parse"),
            "the parse failure must be named: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_speaker_pass_rejects_structured_with_audio_source() {
        let server = make_inference_server("unused".to_string(), "{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: None,
                structured: Some(true),
                correct_existing: None,
            }))
            .await
            .expect_err("structured+audio must be rejected, never a silent no-op");
        assert!(
            error.message.contains("structured outputs apply"),
            "the named rejection must surface: {}",
            error.message
        );
    }

    /// Store a highlight layer with the standard test provenance over
    /// `entries` — the layer construction + store call the three
    /// highlight-layer tests share, varying exactly the highlight entries.
    /// The provenance timestamp is unified (no test asserts it).
    async fn store_highlight_layer(
        server: &MediaServer,
        transcript_id: &str,
        entries: Vec<crate::transcript_layers::HighlightEntry>,
    ) {
        use crate::transcript_layers::{HighlightLayer, LayerProvenance, TranscriptLayer};
        use crate::types::EductStoreLayerRequest;
        let highlight = TranscriptLayer::Highlight(HighlightLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-08-30T00:00:00Z".to_string(),
            },
            highlights: entries,
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.to_string(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&highlight).expect("serialize highlight"),
                ),
            }))
            .await
            .expect("highlight stored");
    }

    #[tokio::test]
    async fn educt_edl_from_highlights_merges_overlapping_selections() {
        use crate::transcript_layers::HighlightEntry;

        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        // Two overlapping highlights over a 2-word transcript: [0,1] and
        // [1,1] → union [0,1] → one Keep op.
        store_highlight_layer(
            &server,
            &transcript_id,
            vec![
                HighlightEntry {
                    start_word: 0,
                    end_word: 1,
                    label: "a".to_string(),
                    note: String::new(),
                },
                HighlightEntry {
                    start_word: 1,
                    end_word: 1,
                    label: "b".to_string(),
                    note: String::new(),
                },
            ],
        )
        .await;

        let composed = server
            .educt_edl_from_highlights(Parameters(crate::types::EductEdlFromHighlightsRequest {
                transcript_id,
                label: None,
                layer_id: None,
            }))
            .await
            .expect("compose succeeds");
        let content = content_of(&composed);
        assert_eq!(
            content["composed_from"]["highlights_selected"].as_u64(),
            Some(2)
        );
        assert_eq!(content["composed_from"]["ranges_merged"].as_u64(), Some(1));
        assert_eq!(
            content["stored"]["layer"]["ops"].as_array().map(Vec::len),
            Some(1)
        );
        assert_eq!(
            content["stored"]["layer"]["ops"][0]["op"],
            serde_json::json!("keep")
        );
    }

    #[tokio::test]
    async fn educt_edl_from_highlights_label_miss_is_not_found() {
        use crate::transcript_layers::HighlightEntry;

        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        store_highlight_layer(
            &server,
            &transcript_id,
            vec![HighlightEntry {
                start_word: 0,
                end_word: 1,
                label: "a".to_string(),
                note: String::new(),
            }],
        )
        .await;
        let error = server
            .educt_edl_from_highlights(Parameters(crate::types::EductEdlFromHighlightsRequest {
                transcript_id,
                label: Some("missing-label".to_string()),
                layer_id: None,
            }))
            .await
            .expect_err("label miss must be not-found");
        assert!(
            error.message.contains("no highlights"),
            "the filter miss must be named: {}",
            error.message
        );
    }

    /// dcterms:identifier: `MediaServer::educt_render_edl`
    /// expect: My rendered reel is a durable gallery asset, not temporary FFmpeg output.
    /// [P1] Motivating: completed edits survive teardown with one stable identity.
    /// pre: a timed transcript, stored EDL, real media source, and active gallery exist.
    /// post: canonical file, gallery row, media block, provenance, and lineage agree.
    /// [P1] Constraining: clip intermediates are removed and no parent asset is invented.
    #[tokio::test]
    async fn educt_render_edl_produces_a_clip_from_a_real_media_file()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::transcript_layers::{EdlLayer, LayerProvenance, TranscriptLayer};
        use crate::transcript_select::{EdlEntry, EdlOp, WordRange};
        use crate::types::{
            EductRenderEdlRequest, EductStoreLayerRequest, EductStoreTranscriptRequest,
        };

        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        // Generate a real 2-second WAV via ffmpeg — the same binary the
        // render path uses; the test proves the full loop (EDL → clip plan
        // → stream-copy render) against real media.
        let wav = gallery_root.path().join("educt-render-source.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-ac",
                "1",
                "-ar",
                "16000",
            ])
            .arg(&wav)
            .output()
            .await
            .expect("ffmpeg runs");
        assert!(
            status.status.success(),
            "ffmpeg must generate the test WAV: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let wav_path = wav.display().to_string();

        // A transcript over it: two words, 0-1s and 1-2s.
        let bundle = TranscriptBundle {
            words: vec![
                TimedWord {
                    word: "alpha".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    confidence: None,
                },
                TimedWord {
                    word: "beta".to_string(),
                    start_ms: 1000,
                    end_ms: 1500,
                    confidence: None,
                },
                TimedWord {
                    word: "gamma".to_string(),
                    start_ms: 2500,
                    end_ms: 3000,
                    confidence: None,
                },
            ],
            ..TranscriptBundle::new(wav_path.clone(), 3.0, "alpha beta gamma".to_string())
        };
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("transcript stored");
        let transcript_id = content_of(&stored)["id"].as_str().expect("id").to_string();

        // EDL: keep word 0 only (the first second).
        let edl = TranscriptLayer::Edl(EdlLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-08-30T00:00:00Z".to_string(),
            },
            ops: vec![
                EdlEntry {
                    range: WordRange::new(0, 0),
                    op: EdlOp::Keep,
                },
                EdlEntry {
                    range: WordRange::new(2, 2),
                    op: EdlOp::Keep,
                },
            ],
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.clone(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&edl).expect("serialize edl"),
                ),
            }))
            .await
            .expect("edl stored");

        let temp_files_before = temp_media_files(server.ffmpeg.temp_dir())
            .expect("read temp media files before render");
        let rendered = server
            .educt_render_edl(Parameters(EductRenderEdlRequest {
                transcript_id: transcript_id.clone(),
                layer_id: None,
            }))
            .await
            .expect("render succeeds");
        let content = content_of(&rendered);
        assert_eq!(content["clips"].as_u64(), Some(2));
        assert_eq!(
            content["clip_plan"],
            serde_json::json!([[0.0, 0.5], [2.5, 3.0]]),
            "the clip plan is the word range mapped to seconds"
        );
        let output = std::path::PathBuf::from(content["output"].as_str().ok_or("output path")?);
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing gallery_asset_id")?;
        let hint = media_hint_body(
            content["display_hint"]
                .as_str()
                .ok_or("missing display_hint")?,
        )?;
        assert!(output.starts_with(crate::assets::generated_assets_dir()));
        assert!(
            output.is_file(),
            "rendered clip exists at {}",
            output.display()
        );
        assert_eq!(hint["kind"], "audio");
        assert_eq!(hint["gallery_asset_id"], asset_id);
        assert_eq!(hint["ontology"], hkask_bridge_ontology::omc::SEQUENCE);
        assert_eq!(store.get_by_id(asset_id)?.media_type, "audio");
        let lineage = store
            .get_generation(asset_id)?
            .ok_or("EDL render lineage missing")?;
        assert_eq!(lineage.op, "educt_render_edl");
        let render_relation = crate::transcript_store::get_render(&**store.driver(), asset_id)?
            .ok_or("EDL render relationship missing")?;
        assert_eq!(
            render_relation.transcript_id.as_deref(),
            Some(transcript_id.as_str())
        );
        assert_eq!(render_relation.source_transcript_id, transcript_id);
        assert_eq!(
            render_relation.edl_layer_id,
            Some(render_relation.source_edl_layer_id.clone())
        );
        let temp_files_after = temp_media_files(server.ffmpeg.temp_dir())?;
        assert_eq!(
            temp_files_after.difference(&temp_files_before).count(),
            0,
            "EDL render left FFmpeg intermediates: {:?}",
            temp_files_after
                .difference(&temp_files_before)
                .collect::<Vec<_>>()
        );
        Ok(())
    }

    /// expect: A researcher strike (a single-Cut EDL annotation) renders via
    /// `educt_render_edl` with the strike layer id as whole-transcript-minus-
    /// strike — the subtractive semantics end to end over real media. [P1]
    /// Constraining: clip intermediates are removed and the render lineage
    /// names the struck EDL.
    #[tokio::test]
    async fn strike_annotation_renders_full_transcript_minus_the_strike()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::tools::educt::EductAnnotationCreateRequest;
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::types::{EductRenderEdlRequest, EductStoreTranscriptRequest};

        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let wav = gallery_root.path().join("educt-strike-source.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-ac",
                "1",
                "-ar",
                "16000",
            ])
            .arg(&wav)
            .output()
            .await
            .expect("ffmpeg runs");
        assert!(
            status.status.success(),
            "ffmpeg must generate the test WAV: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let bundle = TranscriptBundle {
            words: vec![
                TimedWord {
                    word: "alpha".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    confidence: None,
                },
                TimedWord {
                    word: "beta".to_string(),
                    start_ms: 1000,
                    end_ms: 1500,
                    confidence: None,
                },
                TimedWord {
                    word: "gamma".to_string(),
                    start_ms: 2500,
                    end_ms: 3000,
                    confidence: None,
                },
            ],
            ..TranscriptBundle::new(
                wav.display().to_string(),
                3.0,
                "alpha beta gamma".to_string(),
            )
        };
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("transcript stored");
        let transcript_id = content_of(&stored)["id"].as_str().expect("id").to_string();

        // Strike the middle word (1-1.5s) through the annotation surface.
        let strike = content_of(
            &server
                .educt_annotation_create(Parameters(EductAnnotationCreateRequest {
                    transcript_id: transcript_id.clone(),
                    start_word: 1,
                    end_word: 1,
                    label: String::new(),
                    note: String::new(),
                    kind: Some("strike".to_string()),
                }))
                .await
                .expect("strike stored"),
        );
        assert_eq!(strike["layer"]["kind"], "edl");
        let strike_id = strike["annotation_id"]
            .as_str()
            .expect("strike annotation ID")
            .to_string();

        let temp_files_before = temp_media_files(server.ffmpeg.temp_dir())
            .expect("read temp media files before render");
        let rendered = server
            .educt_render_edl(Parameters(EductRenderEdlRequest {
                transcript_id: transcript_id.clone(),
                layer_id: Some(strike_id.clone()),
            }))
            .await
            .expect("the strike renders subtractively");
        let content = content_of(&rendered);
        // Whole transcript minus the struck middle word: two clips, the
        // 1-1.5s word gone.
        assert_eq!(content["clips"].as_u64(), Some(2));
        assert_eq!(
            content["clip_plan"],
            serde_json::json!([[0.0, 0.5], [2.5, 3.0]]),
            "the strike renders as full-transcript-minus-strike"
        );
        let output = std::path::PathBuf::from(content["output"].as_str().ok_or("output path")?);
        let asset_id = content["gallery_asset_id"]
            .as_str()
            .ok_or("missing gallery_asset_id")?;
        assert!(
            output.starts_with(crate::assets::generated_assets_dir()),
            "the render publishes under the generated assets dir"
        );
        assert!(
            output.is_file(),
            "rendered clip exists at {}",
            output.display()
        );
        assert_eq!(store.get_by_id(asset_id)?.media_type, "audio");
        let render_relation = crate::transcript_store::get_render(&**store.driver(), asset_id)?
            .ok_or("EDL render relationship missing")?;
        assert_eq!(render_relation.source_transcript_id, transcript_id);
        assert_eq!(
            render_relation.edl_layer_id.as_deref(),
            Some(strike_id.as_str()),
            "the render lineage names the struck EDL"
        );
        let temp_files_after = temp_media_files(server.ffmpeg.temp_dir())?;
        assert_eq!(
            temp_files_after.difference(&temp_files_before).count(),
            0,
            "strike render left FFmpeg intermediates: {:?}",
            temp_files_after
                .difference(&temp_files_before)
                .collect::<Vec<_>>()
        );
        Ok(())
    }

    #[tokio::test]
    async fn educt_render_edl_without_an_edl_is_not_found() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_render_edl(Parameters(crate::types::EductRenderEdlRequest {
                transcript_id,
                layer_id: None,
            }))
            .await
            .expect_err("no EDL layer must be not-found");
        assert!(
            error.message.contains("no EDL layer"),
            "the missing layer must be named: {}",
            error.message
        );
    }

    /// Run one educt_export in `format` and return (content, output path,
    /// parsed export metadata) — the call + path extraction + metadata
    /// read/parse choreography the durable-export tests share, varying
    /// exactly the format. The srt test keeps its inline choreography:
    /// its per-test asserts (existence, cue count, export id, folder name)
    /// interleave the shared steps.
    async fn export_content_and_metadata(
        server: &MediaServer,
        transcript_id: &str,
        format: &str,
    ) -> (serde_json::Value, String, serde_json::Value) {
        let result = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id: transcript_id.to_string(),
                format: format.to_string(),
            }))
            .await
            .expect("export succeeds");
        let content = content_of(&result);
        let output = content["output"].as_str().expect("output path").to_string();
        let metadata_path = content["metadata_path"].as_str().expect("metadata path");
        let metadata: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(metadata_path).expect("read export metadata"),
        )
        .expect("parse export metadata");
        (content, output, metadata)
    }

    /// Store a paragraph layer with the standard test provenance over
    /// `breaks_after` — the review-flag fixtures' shared store call (through
    /// the real store-layer tool, so validation runs).
    async fn store_paragraph_layer(
        server: &MediaServer,
        transcript_id: &str,
        breaks_after: Vec<usize>,
    ) {
        use crate::transcript_layers::{LayerProvenance, ParagraphLayer, TranscriptLayer};
        use crate::types::EductStoreLayerRequest;
        let paragraph = TranscriptLayer::Paragraph(ParagraphLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-10-27T00:00:01Z".to_string(),
            },
            breaks_after,
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.to_string(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&paragraph).expect("serialize paragraph"),
                ),
            }))
            .await
            .expect("store paragraph layer");
    }

    /// Store a speaker layer with the standard test provenance over
    /// `spans` — the review-flag fixtures' shared store call.
    async fn store_speaker_layer(
        server: &MediaServer,
        transcript_id: &str,
        spans: Vec<crate::transcript_layers::SpeakerSpan>,
    ) {
        use crate::transcript_layers::{LayerProvenance, SpeakerLayer, TranscriptLayer};
        use crate::types::EductStoreLayerRequest;
        let speaker = TranscriptLayer::Speaker(SpeakerLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-10-27T00:00:02Z".to_string(),
            },
            spans,
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.to_string(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&speaker).expect("serialize speaker"),
                ),
            }))
            .await
            .expect("store speaker layer");
    }

    /// A speaker-span fixture with a confidence — the review-flag tests
    /// vary exactly the range and score.
    fn speaker_span(
        start_word: usize,
        end_word: usize,
        confidence: f64,
    ) -> crate::transcript_layers::SpeakerSpan {
        crate::transcript_layers::SpeakerSpan {
            start_word,
            end_word,
            speaker: "speaker-1".to_string(),
            confidence,
        }
    }

    /// dcterms:identifier: `transcript_review::derive_review_flags`
    /// expect: educt_get_transcript carries the derived review view —
    /// paragraph blocks scored by the latest speaker layer's stored
    /// confidences, with layer provenance and the threshold; a
    /// below-threshold block is flagged, an at-threshold one is ok.
    #[tokio::test]
    async fn educt_get_transcript_derives_review_flags() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        store_paragraph_layer(&server, &transcript_id, vec![0]).await;
        store_speaker_layer(
            &server,
            &transcript_id,
            vec![speaker_span(0, 0, 0.9), speaker_span(1, 1, 0.5)],
        )
        .await;

        let result = server
            .educt_get_transcript(Parameters(crate::types::EductGetTranscriptRequest {
                transcript_id,
                include_layers: None,
            }))
            .await
            .expect("get transcript succeeds");
        let review = content_of(&result)["review"].clone();
        assert_eq!(review["threshold"].as_f64(), Some(0.75));
        assert_eq!(review["note"], serde_json::json!(null));
        assert!(review["paragraph_layer_id"].as_str().is_some());
        assert!(review["speaker_layer_id"].as_str().is_some());
        assert_eq!(
            review["blocks"][0],
            serde_json::json!({
                "start_word": 0,
                "end_word": 0,
                "min_confidence": 0.9,
                "status": "ok"
            })
        );
        assert_eq!(
            review["blocks"][1],
            serde_json::json!({
                "start_word": 1,
                "end_word": 1,
                "min_confidence": 0.5,
                "status": "flagged"
            })
        );
    }

    /// dcterms:identifier: `transcript_review::derive_review_flags`
    /// expect: A transcript with no layers names the missing paragraph
    /// layer in the review view — a surfaced degradation, never a
    /// fabricated green or an empty success.
    #[tokio::test]
    async fn educt_get_transcript_review_names_missing_layers() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_get_transcript(Parameters(crate::types::EductGetTranscriptRequest {
                transcript_id,
                include_layers: None,
            }))
            .await
            .expect("get transcript succeeds");
        let review = content_of(&result)["review"].clone();
        assert_eq!(review["blocks"].as_array().map(Vec::len), Some(0));
        let note = review["note"].as_str().expect("note names the absence");
        assert!(note.contains("educt_paragraph_pass"), "note: {note}");
    }

    #[tokio::test]
    async fn educt_export_review_csv_writes_flags() {
        let _artifacts = IsolatedArtifacts::new().await;
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        store_paragraph_layer(&server, &transcript_id, vec![0]).await;
        store_speaker_layer(
            &server,
            &transcript_id,
            vec![speaker_span(0, 0, 0.9), speaker_span(1, 1, 0.5)],
        )
        .await;

        let (content, output, metadata) =
            export_content_and_metadata(&server, &transcript_id, "review_csv").await;
        assert_eq!(
            metadata["effective_params"], content["effective_params"],
            "the export row and the response agree on provenance"
        );
        assert_eq!(metadata["transcript_id"], serde_json::json!(transcript_id));
        assert_eq!(metadata["format"], serde_json::json!("review_csv"));
        // The provenance lands in the export's effective params.
        assert_eq!(
            metadata["effective_params"]["flagged"],
            serde_json::json!(1)
        );
        assert_eq!(metadata["effective_params"]["blocks"], serde_json::json!(2));
        assert_eq!(
            metadata["effective_params"]["threshold"],
            serde_json::json!(0.75)
        );
        let csv = std::fs::read_to_string(&output).expect("read review CSV");
        assert!(
            csv.starts_with(
                "block_index,start_word,end_word,start_ms,end_ms,min_confidence,status"
            )
        );
        // Block 0 "alpha" (0–500 ms) scores ok; block 1 "beta" (500–900 ms)
        // is flagged at the stored 0.5.
        assert!(csv.contains("\n0,0,0,0,500,0.9,ok\n"), "rows: {csv}");
        assert!(csv.contains("\n1,1,1,500,900,0.5,flagged\n"), "rows: {csv}");
    }

    #[tokio::test]
    async fn educt_export_review_csv_without_paragraph_layer_is_not_found() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id,
                format: "review_csv".to_string(),
            }))
            .await
            .expect_err("review csv without a paragraph layer is not-found");
        assert!(
            error.message.contains("educt_paragraph_pass"),
            "the missing layer must be named: {}",
            error.message
        );
    }

    /// dcterms:identifier: `transcript_pass::gate_speaker_corrections`
    /// expect: The correction mode stores the GATED result — the short
    /// proposed fix lands, the long rewrite reverts to the existing
    /// labels, and the refusal is surfaced in the response with the
    /// gate's counts.
    #[tokio::test]
    async fn educt_speaker_pass_correction_mode_gates_locality() {
        let server = make_inference_server(
            r#"{"spans": [{"start_word": 0, "end_word": 0, "speaker": "speaker-2", "confidence": 0.8}, {"start_word": 1, "end_word": 1, "speaker": "speaker-1", "confidence": 0.95}, {"start_word": 2, "end_word": 11, "speaker": "speaker-2", "confidence": 0.7}]}"#
                .to_string(),
            "{}".to_string(),
        );
        let transcript_id = store_n_word_transcript(&server, 12).await;
        store_speaker_layer(&server, &transcript_id, vec![speaker_span(0, 11, 0.9)]).await;

        let result = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: Some("text".to_string()),
                structured: None,
                correct_existing: Some(true),
            }))
            .await
            .expect("correction pass succeeds");
        let content = content_of(&result);
        // The gate: word 0's one-word change accepted; the ten-word
        // rewrite reverted — and both counted, never silent.
        assert_eq!(content["gate"]["runs_accepted"], serde_json::json!(1));
        assert_eq!(content["gate"]["words_accepted"], serde_json::json!(1));
        assert_eq!(content["gate"]["runs_reverted"], serde_json::json!(1));
        assert_eq!(content["gate"]["words_reverted"], serde_json::json!(10));
        assert_eq!(content["gate"]["max_locality_words"], serde_json::json!(5));
        let note = content["gate_note"].as_str().expect("refusal surfaced");
        assert!(note.contains("reverted"), "note: {note}");
        assert!(content["pass_stats"]["attempts"].as_u64().is_some());
        // The stored layer is the gated merge: the short fix on word 0,
        // the existing label over the reverted monologue.
        let spans = content["stored"]["layer"]["spans"].clone();
        assert_eq!(spans.as_array().map(Vec::len), Some(2));
        assert_eq!(spans[0]["speaker"], serde_json::json!("speaker-2"));
        assert_eq!(spans[0]["start_word"], serde_json::json!(0));
        assert_eq!(spans[0]["end_word"], serde_json::json!(0));
        assert_eq!(spans[1]["speaker"], serde_json::json!("speaker-1"));
        assert_eq!(spans[1]["start_word"], serde_json::json!(1));
        assert_eq!(spans[1]["end_word"], serde_json::json!(11));
    }

    /// dcterms:identifier: `educt_speaker_pass` correction mode
    /// expect: Correction without an existing speaker layer is typed
    /// not_found naming the prerequisite — never a silent no-op.
    #[tokio::test]
    async fn educt_speaker_pass_correction_without_layer_is_not_found() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: Some("text".to_string()),
                structured: None,
                correct_existing: Some(true),
            }))
            .await
            .expect_err("correction without a speaker layer is not-found");
        assert!(
            error.message.contains("educt_speaker_pass"),
            "the prerequisite must be named: {}",
            error.message
        );
    }

    /// dcterms:identifier: `educt_speaker_pass` correction mode
    /// expect: Correction requires source "text" — the audio default is
    /// rejected with the reason, never silently coerced.
    #[tokio::test]
    async fn educt_speaker_pass_correction_requires_the_text_source() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_speaker_pass(Parameters(crate::types::EductSpeakerPassRequest {
                transcript_id,
                model: Some("test-stt-model".to_string()),
                source: None,
                structured: None,
                correct_existing: Some(true),
            }))
            .await
            .expect_err("correction with the audio default is rejected");
        assert!(
            error.message.contains("source \"text\""),
            "the source requirement must be named: {}",
            error.message
        );
    }

    /// A real 16 kHz mono WAV fixture (ffmpeg-generated — the render test's
    /// pattern): section re-transcribe clips it, so the file must be
    /// genuinely clip-able, not a dummy byte blob.
    async fn real_wav_fixture(name: &str) -> std::path::PathBuf {
        let wav =
            std::env::temp_dir().join(format!("retranscribe-{name}-{}.wav", std::process::id()));
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=3",
                "-ac",
                "1",
                "-ar",
                "16000",
            ])
            .arg(&wav)
            .output()
            .await
            .expect("ffmpeg runs");
        assert!(
            status.status.success(),
            "ffmpeg must generate the fixture: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        wav
    }

    /// Store a four-word timed transcript over `wav_path` — the section
    /// re-transcribe fixtures' shared setup (alpha/beta/gamma/delta,
    /// 500ms slots).
    async fn store_four_word_transcript(
        server: &MediaServer,
        wav_path: &std::path::Path,
    ) -> String {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::types::EductStoreTranscriptRequest;
        let words: Vec<TimedWord> = ["alpha", "beta", "gamma", "delta"]
            .iter()
            .enumerate()
            .map(|(index, word)| TimedWord {
                word: (*word).to_string(),
                start_ms: index as u64 * 500,
                end_ms: index as u64 * 500 + 450,
                confidence: None,
            })
            .collect();
        let bundle = TranscriptBundle {
            words,
            ..TranscriptBundle::new(
                wav_path.display().to_string(),
                3.0,
                "alpha beta gamma delta".to_string(),
            )
        };
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store succeeds");
        content_of(&stored)["id"]
            .as_str()
            .expect("id present")
            .to_string()
    }

    /// Store a correction layer with the standard test provenance over
    /// `edits` — the re-transcribe fixtures' shared store call (through
    /// the real store-layer tool, so validation runs).
    async fn store_correction_layer(
        server: &MediaServer,
        transcript_id: &str,
        edits: Vec<crate::transcript_layers::CorrectionEdit>,
    ) {
        use crate::transcript_layers::{CorrectionLayer, LayerProvenance, TranscriptLayer};
        use crate::types::EductStoreLayerRequest;
        let correction = TranscriptLayer::Correction(CorrectionLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-10-28T00:00:01Z".to_string(),
            },
            edits,
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.to_string(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&correction).expect("serialize correction"),
                ),
            }))
            .await
            .expect("store correction layer");
    }

    /// The canned STT response every re-transcribe fixture shares: two
    /// words over the span.
    fn retranscribe_stt_json() -> String {
        serde_json::json!({
            "text": " gamma  delta ",
            "duration": 1.0,
            "words": [
                {"word": " gamma", "start": 0.0, "end": 0.5},
                {"word": " delta", "start": 0.5, "end": 1.0}
            ],
            "segments": [
                {"text": " gamma  delta ", "start": 0.0, "end": 1.0}
            ]
        })
        .to_string()
    }

    /// dcterms:identifier: `educt_retranscribe_section`
    /// expect: The section's STT text lands as a candidate correction
    /// edit over the source range; the source words stay immutable; the
    /// span audio is a transport detail, consumed by the configured STT
    /// path (the same one transcribe_bundle uses).
    #[tokio::test]
    async fn educt_retranscribe_section_lands_a_candidate_correction() {
        let server = make_stt_server(retranscribe_stt_json());
        let wav = real_wav_fixture("happy").await;
        let transcript_id = store_four_word_transcript(&server, &wav).await;

        let result = server
            .educt_retranscribe_section(Parameters(crate::types::EductRetranscribeSectionRequest {
                transcript_id: transcript_id.clone(),
                start_word: 1,
                end_word: 2,
                language: Some("en".to_string()),
            }))
            .await
            .expect("section re-transcribe succeeds");
        let content = content_of(&result);
        assert_eq!(content["section"]["text"], serde_json::json!("gamma delta"));
        assert_eq!(content["section"]["words_count"], serde_json::json!(2));
        assert_eq!(content["section"]["start_ms"], serde_json::json!(500));
        assert_eq!(content["section"]["end_ms"], serde_json::json!(1450));
        assert_eq!(
            content["superseded_edits"].as_array().map(Vec::len),
            Some(0)
        );
        let edits = content["stored"]["layer"]["edits"].clone();
        assert_eq!(edits.as_array().map(Vec::len), Some(1));
        assert_eq!(edits[0]["start_word"], serde_json::json!(1));
        assert_eq!(edits[0]["end_word"], serde_json::json!(2));
        assert_eq!(edits[0]["replacement"], serde_json::json!("gamma delta"));
        // The source words are never mutated — criterion 2.
        let stored = server
            .educt_get_transcript(Parameters(crate::types::EductGetTranscriptRequest {
                transcript_id,
                include_layers: Some(false),
            }))
            .await
            .expect("get transcript succeeds");
        let words = content_of(&stored)["transcript"]["words"]
            .as_array()
            .expect("words array")
            .clone();
        assert_eq!(words.len(), 4);
        assert_eq!(words[1]["word"], serde_json::json!("beta"));
    }

    /// dcterms:identifier: `educt_retranscribe_section`
    /// expect: Prior correction edits overlapping the range are
    /// superseded and LISTED with the note; disjoint ones survive
    /// verbatim in the new layer — never a silent merge or drop.
    #[tokio::test]
    async fn educt_retranscribe_section_surfaces_superseded_edits() {
        use crate::transcript_layers::CorrectionEdit;
        let server = make_stt_server(retranscribe_stt_json());
        let wav = real_wav_fixture("supersede").await;
        let transcript_id = store_four_word_transcript(&server, &wav).await;
        store_correction_layer(
            &server,
            &transcript_id,
            vec![
                CorrectionEdit {
                    start_word: 0,
                    end_word: 0,
                    replacement: "alpa-fixed".to_string(),
                    reason: "prior".to_string(),
                },
                CorrectionEdit {
                    start_word: 1,
                    end_word: 3,
                    replacement: "old-mid".to_string(),
                    reason: "prior".to_string(),
                },
            ],
        )
        .await;

        let result = server
            .educt_retranscribe_section(Parameters(crate::types::EductRetranscribeSectionRequest {
                transcript_id,
                start_word: 1,
                end_word: 2,
                language: None,
            }))
            .await
            .expect("section re-transcribe succeeds");
        let content = content_of(&result);
        let superseded = content["superseded_edits"]
            .as_array()
            .expect("superseded list");
        assert_eq!(superseded.len(), 1);
        assert_eq!(superseded[0]["start_word"], serde_json::json!(1));
        assert_eq!(superseded[0]["end_word"], serde_json::json!(3));
        assert_eq!(superseded[0]["replacement"], serde_json::json!("old-mid"));
        let note = content["note"].as_str().expect("supersession surfaced");
        assert!(note.contains("superseded"), "note: {note}");
        // The new layer carries the disjoint prior edit verbatim plus the
        // new STT edit — the working view keeps every surviving
        // correction.
        let edits = content["stored"]["layer"]["edits"].clone();
        assert_eq!(edits.as_array().map(Vec::len), Some(2));
        assert_eq!(edits[0]["replacement"], serde_json::json!("alpa-fixed"));
        assert_eq!(edits[1]["replacement"], serde_json::json!("gamma delta"));
        assert_eq!(edits[1]["start_word"], serde_json::json!(1));
        assert_eq!(edits[1]["end_word"], serde_json::json!(2));
    }

    /// dcterms:identifier: `educt_retranscribe_section`
    /// expect: Out-of-bounds and reversed ranges are typed rejections
    /// naming the bound; an untimed transcript is the named NoWordTimings
    /// degradation — never a silent no-op.
    #[tokio::test]
    async fn educt_retranscribe_section_rejects_bad_ranges_and_untimed() {
        use crate::transcript::TranscriptBundle;
        use crate::types::{EductRetranscribeSectionRequest, EductStoreTranscriptRequest};
        let server = make_stt_server("{}".to_string());
        // The error paths precede the clip, so the media path needs no
        // real file.
        let transcript_id =
            store_four_word_transcript(&server, std::path::Path::new("/tmp/absent.wav")).await;
        for (start_word, end_word, needle) in
            [(4usize, 4usize, "out of bounds"), (2, 1, "reversed")]
        {
            let error = server
                .educt_retranscribe_section(Parameters(EductRetranscribeSectionRequest {
                    transcript_id: transcript_id.clone(),
                    start_word,
                    end_word,
                    language: None,
                }))
                .await
                .expect_err("bad range must be rejected");
            assert!(
                error.message.contains(needle),
                "the named rejection must surface: {}",
                error.message
            );
        }
        // An untimed transcript cannot anchor a range — the named
        // degradation, before any clip or STT spend.
        let untimed = TranscriptBundle::new("/tmp/absent.wav".to_string(), 3.0, "text".to_string());
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&untimed).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store succeeds");
        let untimed_id = content_of(&stored)["id"].as_str().expect("id").to_string();
        let error = server
            .educt_retranscribe_section(Parameters(EductRetranscribeSectionRequest {
                transcript_id: untimed_id,
                start_word: 0,
                end_word: 0,
                language: None,
            }))
            .await
            .expect_err("untimed transcript must be rejected");
        assert!(
            error.message.contains("NoWordTimings"),
            "the named degradation must surface: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_export_srt_writes_captions() {
        let _artifacts = IsolatedArtifacts::new().await;
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id: transcript_id.clone(),
                format: "srt".to_string(),
            }))
            .await
            .expect("srt export succeeds");
        let content = content_of(&result);
        let output = content["output"].as_str().expect("output path").to_string();
        assert!(
            std::path::Path::new(&output).exists(),
            "exported SRT exists at {output}"
        );
        assert_eq!(content["cues"].as_u64(), Some(1));
        let export_id = content["export_id"].as_str().expect("export id");
        let metadata_path = content["metadata_path"].as_str().expect("metadata path");
        assert!(output.contains("transcript-exports"));
        // The folder is named for the recording and format, not the export id.
        let folder = std::path::Path::new(&output)
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .expect("export folder name");
        assert!(
            folder.ends_with("-a-srt") && !folder.contains(export_id),
            "export folder must be readable: {folder}"
        );
        let metadata: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(metadata_path).expect("read export metadata"),
        )
        .expect("parse export metadata");
        assert_eq!(metadata["export_id"], export_id);
        assert_eq!(metadata["transcript_id"], transcript_id);
        assert_eq!(metadata["format"], "srt");
        assert_eq!(metadata["provenance"], content["provenance"]);
        assert_eq!(metadata["effective_params"], content["effective_params"]);
        let exports =
            crate::transcript_store::list_exports(&**server.gallery_store.driver(), &transcript_id)
                .expect("list durable exports");
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_id, export_id);
        assert_eq!(exports[0].document_path, output);
        assert_eq!(exports[0].metadata_path, metadata_path);
        assert_eq!(
            exports[0].transcript_id.as_deref(),
            Some(transcript_id.as_str())
        );
        let srt = std::fs::read_to_string(&output).expect("read SRT");
        assert!(
            srt.contains("1\n00:00:00,000 --> 00:00:00,900\nalpha beta\n"),
            "the cue is the words' time range: {srt}"
        );
    }

    #[tokio::test]
    async fn educt_export_highlights_csv_writes_rows() {
        let _artifacts = IsolatedArtifacts::new().await;
        use crate::transcript_layers::HighlightEntry;

        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        store_highlight_layer(
            &server,
            &transcript_id,
            vec![HighlightEntry {
                start_word: 0,
                end_word: 1,
                label: "key-argument".to_string(),
                note: "the curve".to_string(),
            }],
        )
        .await;

        let (content, output, metadata) =
            export_content_and_metadata(&server, &transcript_id, "highlights_csv").await;
        assert_eq!(content["rows"].as_u64(), Some(1));
        assert_eq!(metadata["transcript_id"], transcript_id);
        assert_eq!(metadata["format"], "highlights_csv");
        assert_eq!(metadata["effective_params"], content["effective_params"]);
        let csv = std::fs::read_to_string(&output).expect("read CSV");
        assert!(csv.starts_with("layer_id,model,start_word,end_word,start_ms,end_ms,label,note"));
        assert!(csv.contains("key-argument"));
        // The time range is the algebra's mapping of words [0,1].
        assert!(csv.contains(",0,1,0,900,"));
    }

    #[tokio::test]
    async fn educt_export_corpus_text_writes_the_rendered_form() {
        let _artifacts = IsolatedArtifacts::new().await;
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let (content, output, metadata) =
            export_content_and_metadata(&server, &transcript_id, "corpus_text").await;
        assert_eq!(metadata["transcript_id"], transcript_id);
        assert_eq!(metadata["format"], "corpus_text");
        assert_eq!(metadata["effective_params"], content["effective_params"]);
        let text = std::fs::read_to_string(&output).expect("read corpus text");
        // The rendered form — exactly what text_to_word_ranges matches,
        // so corpus hits map back to word ranges.
        assert_eq!(text, "alpha beta");
        assert!(content["composition"].as_str().is_some());
    }

    #[tokio::test]
    async fn educt_export_corpus_text_surfaces_the_no_timings_degradation() {
        let _artifacts = IsolatedArtifacts::new().await;
        use crate::transcript::TranscriptBundle;
        use crate::types::EductStoreTranscriptRequest;

        let server = make_pass_server("{}".to_string());
        let degraded =
            TranscriptBundle::new("/tmp/silent.wav".to_string(), 0.0, "just text".to_string());
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&degraded).expect("serialize"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("degraded transcript stored");
        let transcript_id = content_of(&stored)["id"].as_str().expect("id").to_string();
        let result = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id,
                format: "corpus_text".to_string(),
            }))
            .await
            .expect("corpus export still succeeds for text-only search");
        let content = content_of(&result);
        assert!(
            content["degradation"].as_str().is_some(),
            "the degradation must be surfaced, not silent: {content}"
        );
    }

    #[tokio::test]
    async fn educt_export_rejects_unknown_format() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id,
                format: "bogus".to_string(),
            }))
            .await
            .expect_err("unknown format must be rejected");
        assert!(
            error.message.contains("format must be"),
            "the named error must surface: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_export_highlights_csv_without_layers_is_not_found() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let error = server
            .educt_export(Parameters(crate::types::EductExportRequest {
                transcript_id,
                format: "highlights_csv".to_string(),
            }))
            .await
            .expect_err("no highlight layers must be not-found");
        assert!(
            error.message.contains("no highlight layers"),
            "the missing layers must be named: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_store_transcript_accepts_the_stringified_transport_form() {
        use crate::types::EductStoreTranscriptRequest;

        // Some MCP transports serialize loosely-typed object params into
        // their JSON-string form (verified live 2026-08-31 — the agent's
        // object-valued transcript arrived as a string and was rejected).
        // The tool must accept both forms.
        let server = make_server();
        let bundle = two_word_bundle();
        let json_string =
            serde_json::to_string(&bundle).expect("serialize bundle to a JSON string");
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(serde_json::Value::String(json_string)),
                gallery_asset_id: None,
            }))
            .await
            .expect("the stringified transport form must store");
        let content = content_of(&stored);
        assert_eq!(content["words_count"].as_u64(), Some(2));
        assert_eq!(content["has_word_timings"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn educt_locate_maps_a_quote_to_word_and_time_ranges() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_locate(Parameters(crate::types::EductLocateRequest {
                transcript_id,
                text: "alpha beta".to_string(),
            }))
            .await
            .expect("locate succeeds");
        let content = content_of(&result);
        assert_eq!(content["status"], serde_json::json!("located"));
        assert_eq!(content["count"].as_u64(), Some(1));
        assert_eq!(
            content["ranges"][0],
            serde_json::json!({
                "start_word": 0,
                "end_word": 1,
                "start_ms": 0,
                "end_ms": 900,
            })
        );
    }

    /// MF-2 reached-when pin (target condition 2): a correction that
    /// inserts words leaves timed consumers failing with the unaligned
    /// precondition (retained by design); storing a realignment layer
    /// via educt_realign_transcript restores them — the same corrected
    /// Store a transcript bundle and return its transcript id — the store
    /// call + id extraction the realignment and repeat-locate tests share,
    /// varying exactly the bundle.
    async fn store_bundle_transcript(
        server: &MediaServer,
        bundle: &crate::transcript::TranscriptBundle,
    ) -> String {
        use crate::types::EductStoreTranscriptRequest;
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: None,
            }))
            .await
            .expect("store succeeds");
        content_of(&stored)["id"]
            .as_str()
            .expect("id present")
            .to_string()
    }

    /// text subsequently drives educt_locate, the highlight pass, and SRT
    /// export over re-anchored timings.
    #[tokio::test]
    async fn realignment_restores_timed_consumers_after_cardinality_change() {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::types::{
            EductExportRequest, EductHighlightPassRequest, EductLocateRequest,
            EductRealignTranscriptRequest, EductStoreLayerRequest,
        };

        let _artifacts = IsolatedArtifacts::new().await;
        let server = make_pass_server(
            r#"{"highlights": [{"start_word": 2, "end_word": 3, "label": "key", "note": ""}]}"#
                .to_string(),
        );
        // Four timed words at 1000ms intervals: "the cinder ela curve".
        let bundle = TranscriptBundle {
            words: ["the", "cinder", "ela", "curve"]
                .iter()
                .enumerate()
                .map(|(index, text)| TimedWord {
                    word: text.to_string(),
                    start_ms: index as u64 * 1000,
                    end_ms: index as u64 * 1000 + 500,
                    confidence: None,
                })
                .collect(),
            ..TranscriptBundle::new(
                "/tmp/realign.wav".to_string(),
                4.0,
                "the cinder ela curve".to_string(),
            )
        };
        let transcript_id = store_bundle_transcript(&server, &bundle).await;

        // A cardinality-changing correction: word 2 ("ela") becomes two
        // tokens — an insertion.
        let correction = server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.clone(),
                layer: hkask_types::AnyJsonValue(serde_json::json!({
                    "kind": "correction",
                    "provenance": {
                        "model": "test-model",
                        "prompt_template": "test",
                        "created_at": "2026-10-06T00:00:00Z"
                    },
                    "edits": [{
                        "start_word": 2,
                        "end_word": 2,
                        "replacement": "Cinder ella",
                        "reason": "misheard"
                    }]
                })),
            }))
            .await
            .expect("correction stores");
        let correction_layer_id = content_of(&correction)["stored"]["id"]
            .as_str()
            .expect("correction layer id")
            .to_string();

        // The unaligned precondition is RETAINED: locate on the corrected
        // text fails visibly until a realignment layer exists.
        let unaligned = server
            .educt_locate(Parameters(EductLocateRequest {
                transcript_id: transcript_id.clone(),
                text: "the cinder Cinder ella curve".to_string(),
            }))
            .await
            .expect_err("unaligned locate must fail");
        assert_eq!(
            unaligned.kind,
            hkask_types::McpErrorKind::FailedPrecondition,
            "{unaligned:?}"
        );
        assert!(
            unaligned.message.contains("educt_realign_transcript"),
            "the precondition names the re-alignment path: {unaligned:?}"
        );

        // The re-alignment path: store the realignment layer.
        let realigned = server
            .educt_realign_transcript(Parameters(EductRealignTranscriptRequest {
                transcript_id: transcript_id.clone(),
                layer_id: None,
            }))
            .await
            .expect("realign succeeds");
        let realigned = content_of(&realigned);
        assert_eq!(realigned["alignment"], serde_json::json!("re_anchored"));
        assert_eq!(
            realigned["reanchored"]["correction_layer_id"],
            serde_json::json!(correction_layer_id)
        );
        assert_eq!(
            realigned["stored"]["layer"]["kind"],
            serde_json::json!("realignment")
        );

        // Reached-when (a): locate drives the corrected text to word and
        // time ranges over the re-anchored timings.
        let located = server
            .educt_locate(Parameters(EductLocateRequest {
                transcript_id: transcript_id.clone(),
                text: "the cinder Cinder ella curve".to_string(),
            }))
            .await
            .expect("locate succeeds after realignment");
        let located = content_of(&located);
        assert_eq!(located["status"], serde_json::json!("located"));
        assert_eq!(located["count"].as_u64(), Some(1));
        assert_eq!(located["ranges"][0]["start_word"], serde_json::json!(0));
        assert_eq!(located["ranges"][0]["end_word"], serde_json::json!(4));
        assert_eq!(located["ranges"][0]["start_ms"], serde_json::json!(0));
        assert_eq!(located["ranges"][0]["end_ms"], serde_json::json!(3500));

        // Reached-when (b): the highlight pass runs over the re-anchored
        // working transcript.
        let highlighted = server
            .educt_highlight_pass(Parameters(EductHighlightPassRequest {
                transcript_id: transcript_id.clone(),
                request: "the key moment".to_string(),
                model: Some("test-stt-model".to_string()),
                structured: None,
            }))
            .await
            .expect("highlight pass succeeds after realignment");
        assert_eq!(
            content_of(&highlighted)["stored"]["layer"]["kind"],
            serde_json::json!("highlight")
        );
        // The discriminating pin (2026-10-26 review): the canned selection
        // [2, 3] is over the re-anchored WORKING tokens ("Cinder ella", the
        // insertion), which the working→source translation maps to SOURCE
        // [2, 2]. Without the translation the stored entry would read
        // [2, 3] — in-bounds, silently spanning "ela curve" — so this
        // assertion fails if the translation block is deleted.
        let stored_layer = content_of(&highlighted);
        let stored_highlights = stored_layer["stored"]["layer"]["highlights"]
            .as_array()
            .expect("stored highlight entries");
        assert_eq!(stored_highlights[0]["start_word"], serde_json::json!(2));
        assert_eq!(stored_highlights[0]["end_word"], serde_json::json!(2));

        // Reached-when (c): SRT export produces cues carrying the
        // corrected text over the re-anchored timings.
        let srt = server
            .educt_export(Parameters(EductExportRequest {
                transcript_id: transcript_id.clone(),
                format: "srt".to_string(),
            }))
            .await
            .expect("SRT export succeeds after realignment");
        let srt_content = content_of(&srt);
        let output = srt_content["output"].as_str().expect("output path");
        let srt_body = std::fs::read_to_string(output).expect("read SRT");
        assert!(
            srt_body.contains("Cinder ella"),
            "SRT carries the corrected text: {srt_body}"
        );
    }

    #[tokio::test]
    async fn educt_locate_surfaces_all_candidates_on_ambiguity() {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::types::EductLocateRequest;

        let server = make_pass_server("{}".to_string());
        // "the plan" appears twice — both candidates must surface.
        let bundle = TranscriptBundle {
            words: ["the", "plan", "the", "plan", "again"]
                .iter()
                .enumerate()
                .map(|(index, text)| TimedWord {
                    word: text.to_string(),
                    start_ms: index as u64 * 1000,
                    end_ms: index as u64 * 1000 + 500,
                    confidence: None,
                })
                .collect(),
            ..TranscriptBundle::new(
                "/tmp/repeat.wav".to_string(),
                5.0,
                "the plan the plan again".to_string(),
            )
        };
        let transcript_id = store_bundle_transcript(&server, &bundle).await;
        let result = server
            .educt_locate(Parameters(EductLocateRequest {
                transcript_id,
                text: "the plan".to_string(),
            }))
            .await
            .expect("locate succeeds");
        let content = content_of(&result);
        assert_eq!(
            content["count"].as_u64(),
            Some(2),
            "both candidates: {content}"
        );
    }

    #[tokio::test]
    async fn educt_locate_no_match_is_surfaced_not_silent() {
        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_locate(Parameters(crate::types::EductLocateRequest {
                transcript_id,
                text: "a passage that never appears".to_string(),
            }))
            .await
            .expect("no_match is a surfaced status, not an error");
        let content = content_of(&result);
        assert_eq!(content["status"], serde_json::json!("no_match"));
        assert_eq!(content["count"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn educt_transcript_store_lifecycle_round_trip() {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::transcript_layers::{
            HighlightEntry, HighlightLayer, LayerProvenance, TranscriptLayer,
        };
        use crate::types::{
            EductDeleteTranscriptRequest, EductGetTranscriptRequest, EductListTranscriptsRequest,
            EductStoreLayerRequest, EductStoreTranscriptRequest,
        };

        let server = make_server();
        let gallery_root = tempfile::tempdir().expect("gallery root");
        let source = gallery_root.path().join("a.wav");
        std::fs::write(&source, b"source audio").expect("source media");
        let gallery = server
            .gallery_store
            .open(
                gallery_root.path().to_str().expect("UTF-8 gallery root"),
                GalleryMode::CopyOnWrite,
            )
            .expect("open gallery");
        let asset = server
            .gallery_store
            .add_media(
                &gallery.id,
                source.to_str().expect("UTF-8 source path"),
                "source-hash",
                0,
                0,
                "wav",
                12,
                "audio",
            )
            .expect("add source asset");
        let bundle = TranscriptBundle {
            words: vec![
                TimedWord {
                    word: "alpha".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    confidence: None,
                },
                TimedWord {
                    word: "beta".to_string(),
                    start_ms: 500,
                    end_ms: 900,
                    confidence: None,
                },
            ],
            ..TranscriptBundle::new(
                source.to_string_lossy().into_owned(),
                1.0,
                "alpha beta".to_string(),
            )
        };

        // Store.
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: Some(asset.id.clone()),
            }))
            .await
            .expect("educt_store_transcript succeeds");
        let stored = content_of(&stored);
        let transcript_id = stored["id"].as_str().expect("id present").to_string();
        assert_eq!(stored["words_count"].as_u64(), Some(2));
        assert_eq!(stored["has_word_timings"], serde_json::json!(true));

        // List by asset (the asset JOIN).
        let listed = server
            .educt_list_transcripts(Parameters(EductListTranscriptsRequest {
                media_path: None,
                gallery_asset_id: Some(asset.id.clone()),
                limit: Some(10),
            }))
            .await
            .expect("educt_list_transcripts succeeds");
        let listed = content_of(&listed);
        assert_eq!(listed["count"].as_u64(), Some(1));

        // Store a highlight layer (validated against words_count).
        let layer = TranscriptLayer::Highlight(HighlightLayer {
            provenance: LayerProvenance {
                model: "agent".to_string(),
                prompt_template: "none".to_string(),
                created_at: "2026-08-30T00:00:00Z".to_string(),
            },
            highlights: vec![HighlightEntry {
                start_word: 0,
                end_word: 1,
                label: "key moment".to_string(),
                note: String::new(),
            }],
        });
        let layer_stored = server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.clone(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&layer).expect("serialize layer"),
                ),
            }))
            .await
            .expect("educt_store_layer succeeds");
        let layer_stored = content_of(&layer_stored);
        assert_eq!(
            layer_stored["stored"]["layer"]["kind"],
            serde_json::json!("highlight")
        );

        // Get with layers.
        let got = server
            .educt_get_transcript(Parameters(EductGetTranscriptRequest {
                transcript_id: transcript_id.clone(),
                include_layers: Some(true),
            }))
            .await
            .expect("educt_get_transcript succeeds");
        let got = content_of(&got);
        assert_eq!(got["layers"].as_array().map(Vec::len), Some(1));
        assert_eq!(got["summary"]["id"], serde_json::json!(transcript_id));

        // Delete cascades.
        let deleted = server
            .educt_delete_transcript(Parameters(EductDeleteTranscriptRequest { transcript_id }))
            .await
            .expect("educt_delete_transcript succeeds");
        let deleted = content_of(&deleted);
        assert_eq!(deleted["transcripts_removed"].as_u64(), Some(1));
        assert_eq!(deleted["layers_removed"].as_u64(), Some(1));
    }

    // ── Media layer store contract tests ────────────────────────────────

    /// One wire-form layer item at `start_sec..end_sec` with searchable
    /// text and an object payload.
    fn wire_item(start_sec: f64, end_sec: Option<f64>) -> crate::types::LayersStoreItemRequest {
        crate::types::LayersStoreItemRequest {
            start_sec,
            end_sec,
            searchable_text: "a labeled moment".to_string(),
            payload: hkask_types::AnyJsonValue(serde_json::json!({"label": "moment"})),
            frame_asset_ids: None,
        }
    }

    /// A researcher-authored layer request over the given media path with
    /// the given items — the store/list/get fixture shape.
    fn researcher_layer_request(
        media_path: String,
        items: Vec<crate::types::LayersStoreItemRequest>,
    ) -> crate::types::LayersStoreRequest {
        crate::types::LayersStoreRequest {
            media_path,
            gallery_asset_id: None,
            kind: crate::layer_store::MediaLayerKind::Researcher,
            label: Some("review selections".to_string()),
            provenance: crate::types::LayersProvenanceRequest {
                model: "user".to_string(),
                prompt_template: "researcher_annotation".to_string(),
            },
            items,
        }
    }

    /// t1 AC1: a researcher layer with items round-trips store -> list ->
    /// get on a real fixture video, with every range inside the ffprobe
    /// duration. One payload rides the `AnyJsonValue` JSON-string
    /// transport and must decode to an object (the transcript-store
    /// wire-form precedent).
    #[tokio::test]
    async fn researcher_layer_round_trips_store_list_get_on_a_real_fixture_video()
    -> Result<(), Box<dyn std::error::Error>> {
        let env = video_server_env().await?;
        let media_path = env.source.to_string_lossy().into_owned();
        let string_transport_item = crate::types::LayersStoreItemRequest {
            searchable_text: "second moment".to_string(),
            payload: hkask_types::AnyJsonValue(serde_json::Value::String(
                r#"{"label": "string-transport"}"#.to_string(),
            )),
            ..wire_item(1.0, Some(1.5))
        };
        let stored = content_of(
            &env.server
                .layers_store(Parameters(researcher_layer_request(
                    media_path.clone(),
                    vec![wire_item(0.0, Some(0.5)), string_transport_item],
                )))
                .await?,
        );
        assert_eq!(stored["kind"], serde_json::json!("researcher"));
        assert_eq!(stored["items_count"].as_u64(), Some(2));
        let layer_id = stored["id"].as_str().ok_or("missing layer id")?.to_string();
        let probed_duration = stored["probed_duration_secs"]
            .as_f64()
            .ok_or("missing probed duration")?;
        assert!(
            probed_duration > 0.0,
            "the real fixture carries a positive ffprobe duration"
        );

        let listed = content_of(
            &env.server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path.clone()),
                    kind: Some(crate::layer_store::MediaLayerKind::Researcher),
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(listed["count"].as_u64(), Some(1));
        let summary = &listed["layers"][0];
        assert_eq!(summary["id"].as_str(), Some(layer_id.as_str()));
        assert_eq!(summary["items_count"].as_u64(), Some(2));
        assert_eq!(summary["provenance"]["model"], serde_json::json!("user"));

        let fetched = content_of(
            &env.server
                .layers_get(Parameters(crate::types::LayersGetRequest {
                    layer_id: layer_id.clone(),
                }))
                .await?,
        );
        let items = fetched["layer"]["items"].as_array().ok_or("items array")?;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["start_sec"].as_f64(), Some(0.0));
        assert_eq!(items[0]["end_sec"].as_f64(), Some(0.5));
        assert_eq!(
            items[0]["searchable_text"],
            serde_json::json!("a labeled moment")
        );
        assert_eq!(
            items[1]["payload"]["label"],
            serde_json::json!("string-transport"),
            "the JSON-string payload transport decodes to an object"
        );
        for item in items {
            let start = item["start_sec"].as_f64().ok_or("start_sec")?;
            let end = item["end_sec"].as_f64().ok_or("end_sec")?;
            assert!(
                start >= 0.0 && end <= probed_duration,
                "item range [{start}, {end}] inside the probed duration {probed_duration}"
            );
        }
        Ok(())
    }

    /// t1 AC2: reversed and out-of-bounds ranges are rejected with typed
    /// errors naming the bounds and the probed duration — never silently
    /// stored.
    #[tokio::test]
    async fn layers_store_rejects_reversed_and_out_of_bounds_ranges_naming_the_bounds()
    -> Result<(), Box<dyn std::error::Error>> {
        let env = video_server_env().await?;
        let media_path = env.source.to_string_lossy().into_owned();
        let info = content_of(
            &env.server
                .video_info(Parameters(crate::types::VideoInfoRequest {
                    video_url: media_path.clone(),
                }))
                .await?,
        );
        let duration = info["duration_secs"].as_f64().ok_or("duration")?;
        assert!(duration >= 1.5, "fixture long enough for the anchor range");

        let reversed = env
            .server
            .layers_store(Parameters(researcher_layer_request(
                media_path.clone(),
                vec![wire_item(1.5, Some(0.5))],
            )))
            .await
            .expect_err("reversed range rejected");
        let message = reversed.to_string();
        assert!(
            message.contains("invalid_argument"),
            "classified invalid_argument: {message}"
        );
        assert!(
            message.contains("reversed") && message.contains('1') && message.contains('0'),
            "names the reversed invariant and both bounds: {message}"
        );

        let beyond_end = duration + 10.0;
        let out_of_bounds = env
            .server
            .layers_store(Parameters(researcher_layer_request(
                media_path.clone(),
                vec![wire_item(1.0, Some(beyond_end))],
            )))
            .await
            .expect_err("end beyond duration rejected");
        let message = out_of_bounds.to_string();
        assert!(
            message.contains("end_sec") && message.contains(&beyond_end.to_string()),
            "names the offending end_sec value: {message}"
        );
        assert!(
            message.contains(&duration.to_string()),
            "names the probed duration: {message}"
        );

        let start_beyond = duration + 5.0;
        let out_of_bounds_start = env
            .server
            .layers_store(Parameters(researcher_layer_request(
                media_path.clone(),
                vec![wire_item(start_beyond, None)],
            )))
            .await
            .expect_err("start beyond duration rejected");
        let message = out_of_bounds_start.to_string();
        assert!(
            message.contains("start_sec"),
            "names the offending bound: {message}"
        );

        let listed = content_of(
            &env.server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path.clone()),
                    kind: None,
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(
            listed["count"].as_u64(),
            Some(0),
            "no rejected layer is stored"
        );
        Ok(())
    }

    /// t1 AC3: deleting a layer cascades its items and never touches the
    /// source media file. Deletion is a store-level operation (the t1 tool
    /// surface is store/list/get), called through the same gallery driver
    /// the tools use.
    #[tokio::test]
    async fn deleting_a_layer_cascades_items_and_never_touches_source_media()
    -> Result<(), Box<dyn std::error::Error>> {
        let env = video_server_env().await?;
        let media_path = env.source.to_string_lossy().into_owned();
        let source_before = std::fs::read(&env.source)?;
        let stored = content_of(
            &env.server
                .layers_store(Parameters(researcher_layer_request(
                    media_path.clone(),
                    vec![wire_item(0.0, Some(0.5)), wire_item(1.0, None)],
                )))
                .await?,
        );
        let layer_id = stored["id"].as_str().ok_or("layer id")?.to_string();

        let driver = &**env.server.gallery_store.driver();
        let counts = crate::layer_store::delete_layer(driver, &layer_id)
            .map_err(|error| format!("delete layer: {error}"))?;
        assert_eq!(counts.layers_removed, 1);
        assert_eq!(
            counts.items_removed, 2,
            "the cascade receipt names both items"
        );

        let gone = env
            .server
            .layers_get(Parameters(crate::types::LayersGetRequest {
                layer_id: layer_id.clone(),
            }))
            .await
            .expect_err("deleted layer is gone");
        let message = gone.to_string();
        assert!(
            message.contains("not_found") && message.contains(&layer_id),
            "names the deleted layer id: {message}"
        );

        let row = driver
            .query_optional(
                "SELECT COUNT(*) FROM media_layer_items WHERE layer_id = ?1",
                &[hkask_storage::database::value::DbValue::Text(
                    layer_id.clone(),
                )],
            )?
            .ok_or("count row")?;
        assert_eq!(row.get_int(0)?, 0, "the cascade removed every item row");

        let source_after = std::fs::read(&env.source)?;
        assert_eq!(
            source_before, source_after,
            "layers never write source media bytes"
        );
        Ok(())
    }

    /// t1 AC4: unknown layer ids and nonexistent media paths classify
    /// not_found naming the id/path (the typed-error contract — a
    /// caller-named resource that is absent is never invalid_argument or
    /// internal).
    #[tokio::test]
    async fn layers_get_unknown_id_and_missing_media_path_are_not_found() {
        let server = make_server();
        let error = server
            .layers_get(Parameters(crate::types::LayersGetRequest {
                layer_id: "no-such-layer".to_string(),
            }))
            .await
            .expect_err("unknown layer id fails");
        let message = error.to_string();
        assert!(
            message.contains("not_found"),
            "classified not_found: {message}"
        );
        assert!(
            message.contains("no-such-layer"),
            "names the unknown id: {message}"
        );

        let error = server
            .layers_store(Parameters(researcher_layer_request(
                "/nonexistent/media-layers-probe.mp4".to_string(),
                vec![wire_item(0.0, Some(0.5))],
            )))
            .await
            .expect_err("missing media path fails");
        let message = error.to_string();
        assert!(
            message.contains("not_found"),
            "classified not_found: {message}"
        );
        assert!(
            message.contains("/nonexistent/media-layers-probe.mp4"),
            "names the missing path: {message}"
        );
    }

    /// t2 AC1: a hard-cut fixture yields two or more ordered, disjoint
    /// shot items inside the probed duration, with provenance recording
    /// the ffmpeg:scdet template and the exact filter + threshold
    /// parameters.
    #[tokio::test]
    async fn layers_shot_pass_detects_a_hard_cut_as_ordered_disjoint_shots()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("hard-cut.mp4");
        create_real_hard_cut_video(&source).await?;
        let media_path = source.to_string_lossy().into_owned();
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let result = content_of(
            &server
                .layers_shot_pass(Parameters(crate::types::LayersShotPassRequest {
                    media_path: media_path.clone(),
                    gallery_asset_id: None,
                    threshold: None,
                    min_shot_length: None,
                }))
                .await?,
        );
        assert_eq!(result["status"], serde_json::json!("completed"));
        let layer = &result["layer"];
        assert_eq!(layer["kind"], serde_json::json!("shot"));
        assert_eq!(
            layer["items_count"].as_u64(),
            Some(2),
            "a single hard cut yields exactly two shots: {layer}"
        );
        let duration = result["probed_duration_secs"]
            .as_f64()
            .ok_or("probed duration")?;
        assert!(duration > 1.5, "the two-second fixture probes its duration");

        // Provenance records the template and the exact parameters.
        assert_eq!(layer["provenance"]["model"], serde_json::json!("ffmpeg"));
        assert_eq!(
            layer["provenance"]["prompt_template"],
            serde_json::json!("ffmpeg:scdet")
        );
        let parameters = &layer["provenance"]["parameters"];
        assert!(
            parameters["filter"]
                .as_str()
                .is_some_and(|filter| filter.contains("gt(scene,0.300)")),
            "provenance records the exact filter: {parameters}"
        );
        assert_eq!(parameters["threshold"], serde_json::json!(0.3));
        assert_eq!(parameters["min_shot_length"], serde_json::json!(0.5));

        // The items are recallable through the store: ordered, disjoint,
        // and inside the probed duration.
        let layer_id = layer["id"].as_str().ok_or("layer id")?.to_string();
        let fetched = content_of(
            &server
                .layers_get(Parameters(crate::types::LayersGetRequest {
                    layer_id: layer_id.clone(),
                }))
                .await?,
        );
        let items = fetched["layer"]["items"].as_array().ok_or("items array")?;
        assert_eq!(items.len(), 2);
        for pair in items.windows(2) {
            let previous_end = pair[0]["end_sec"].as_f64().ok_or("end_sec")?;
            let next_start = pair[1]["start_sec"].as_f64().ok_or("start_sec")?;
            assert!(
                previous_end <= next_start,
                "shots are ordered and disjoint: {items:?}"
            );
        }
        for item in items {
            let start = item["start_sec"].as_f64().ok_or("start_sec")?;
            let end = item["end_sec"].as_f64().ok_or("end_sec")?;
            assert!(
                start >= 0.0 && end <= duration,
                "shot range [{start}, {end}] inside the probed duration {duration}"
            );
            assert!(
                !item["searchable_text"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            );
        }
        Ok(())
    }

    /// t2 AC2a: a no-cut fixture returns the explicit no-shots note carrying
    /// the exact filter parameters — a named degradation, never an error
    /// and never a silent empty success — and stores nothing.
    #[tokio::test]
    async fn layers_shot_pass_no_cut_fixture_returns_the_named_no_shots_note()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("single-scene.mp4");
        create_real_video(&source).await?;
        let media_path = source.to_string_lossy().into_owned();
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let result = content_of(
            &server
                .layers_shot_pass(Parameters(crate::types::LayersShotPassRequest {
                    media_path: media_path.clone(),
                    gallery_asset_id: None,
                    threshold: None,
                    min_shot_length: None,
                }))
                .await?,
        );
        assert_eq!(result["status"], serde_json::json!("no_shots"));
        let note = result["note"].as_str().ok_or("note present")?;
        assert!(
            note.contains("no scene changes"),
            "the note names the degradation: {note}"
        );
        assert!(
            result["filter"]
                .as_str()
                .is_some_and(|filter| filter.contains("gt(scene,0.300)")),
            "the note carries the exact filter parameters: {result}"
        );
        assert_eq!(result["threshold"], serde_json::json!(0.3));
        assert_eq!(result["min_shot_length"], serde_json::json!(0.5));
        assert!(
            result["probed_duration_secs"]
                .as_f64()
                .is_some_and(|d| d > 0.0)
        );
        let listed = content_of(
            &server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path.clone()),
                    kind: None,
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(
            listed["count"].as_u64(),
            Some(0),
            "the no-shots degradation stores no layer"
        );
        Ok(())
    }

    /// t2 AC2b: media without a video stream (an audio-only file) returns a
    /// named degradation naming the requirement — never a raw ffmpeg
    /// error — and stores nothing.
    #[tokio::test]
    async fn layers_shot_pass_audio_only_is_a_named_degradation()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("audio-only.wav");
        create_real_audio(&source).await?;
        let media_path = source.to_string_lossy().into_owned();
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let error = server
            .layers_shot_pass(Parameters(crate::types::LayersShotPassRequest {
                media_path: media_path.clone(),
                gallery_asset_id: None,
                threshold: None,
                min_shot_length: None,
            }))
            .await
            .expect_err("audio-only media must fail the stream requirement");
        let message = error.to_string();
        assert!(
            message.contains("failed_precondition"),
            "classified failed_precondition (the named degradation kind): {message}"
        );
        assert!(
            message.contains("video stream"),
            "names the requirement: {message}"
        );
        assert!(
            message.contains(&media_path),
            "names the media path: {message}"
        );
        let listed = content_of(
            &server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path.clone()),
                    kind: None,
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(listed["count"].as_u64(), Some(0), "nothing is stored");
        Ok(())
    }

    /// t2 AC2c: a missing media file fails as not_found naming the path.
    #[tokio::test]
    async fn layers_shot_pass_missing_media_is_not_found() {
        let server = make_server();
        let error = server
            .layers_shot_pass(Parameters(crate::types::LayersShotPassRequest {
                media_path: "/nonexistent/shot-pass-probe.mp4".to_string(),
                gallery_asset_id: None,
                threshold: None,
                min_shot_length: None,
            }))
            .await
            .expect_err("missing media path fails");
        let message = error.to_string();
        assert!(
            message.contains("not_found"),
            "classified not_found: {message}"
        );
        assert!(
            message.contains("/nonexistent/shot-pass-probe.mp4"),
            "names the missing path: {message}"
        );
    }

    // ── Layer pass core contract tests (t3) ────────────────────────────

    /// Serializes tests that set the model env vars (`ARTIFACTS_ENV_LOCK`'s
    /// discipline): every existing pass test passes an explicit model
    /// override and reads no env, so only these chain tests touch the
    /// vars today; any future test that relies on configured model envs
    /// takes this lock.
    static MODEL_ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    /// Sets the pass/vision model env vars for one test and restores the
    /// prior values on drop (missing restored by removal).
    struct ModelEnvGuard {
        prior_pass: Option<String>,
        prior_vision: Option<String>,
    }

    impl ModelEnvGuard {
        fn set(pass: Option<&str>, vision: Option<&str>) -> Self {
            let guard = Self {
                prior_pass: std::env::var(crate::models::PASS_ENV).ok(),
                prior_vision: std::env::var(crate::models::VISION_ENV).ok(),
            };
            match pass {
                Some(value) => unsafe { std::env::set_var(crate::models::PASS_ENV, value) },
                None => unsafe { std::env::remove_var(crate::models::PASS_ENV) },
            }
            match vision {
                Some(value) => unsafe { std::env::set_var(crate::models::VISION_ENV, value) },
                None => unsafe { std::env::remove_var(crate::models::VISION_ENV) },
            }
            guard
        }
    }

    impl Drop for ModelEnvGuard {
        fn drop(&mut self) {
            match self.prior_pass.take() {
                Some(value) => unsafe { std::env::set_var(crate::models::PASS_ENV, value) },
                None => unsafe { std::env::remove_var(crate::models::PASS_ENV) },
            }
            match self.prior_vision.take() {
                Some(value) => unsafe { std::env::set_var(crate::models::VISION_ENV, value) },
                None => unsafe { std::env::remove_var(crate::models::VISION_ENV) },
            }
        }
    }

    /// A mock layer-pass output — the core is generic over the typed
    /// output; t4/t5 pass outputs are pass-specific shapes of this same
    /// contract.
    #[derive(
        Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
    )]
    struct MockLayerPassOutput {
        items: Vec<MockLayerPassItem>,
    }

    #[derive(
        Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
    )]
    struct MockLayerPassItem {
        start_sec: f64,
        end_sec: Option<f64>,
        searchable_text: String,
    }

    /// The canned two-item layer output every mock layer pass returns.
    fn mock_layer_response() -> String {
        serde_json::json!({
            "items": [
                {"start_sec": 0.0, "end_sec": 1.0, "searchable_text": "first moment"},
                {"start_sec": 1.0, "end_sec": 2.0, "searchable_text": "second moment"},
            ]
        })
        .to_string()
    }

    fn mock_layer_schema() -> String {
        serde_json::to_string(&schemars::schema_for!(MockLayerPassOutput))
            .expect("mock output schema serializes")
    }

    /// The mock inference port over the canned layer response — the
    /// capability-carrying constructor the pass tests use (`make_pass_server`
    /// hides its port; the core takes the port directly).
    fn mock_layer_inference(
        response: String,
        media_response: String,
        media_response_is_json: bool,
    ) -> Arc<dyn hkask_types::InferencePort> {
        Arc::new(MockInferencePort {
            response,
            media_response,
            media_response_is_json,
            audio_urls: Arc::new(std::sync::Mutex::new(Vec::new())),
        })
    }

    /// Drive the t1 store from the core's outputs — the composition AC1
    /// pins: the typed outputs flow through the layer store and are
    /// recallable.
    fn store_mock_pass_layer(
        server: &MediaServer,
        output: &MockLayerPassOutput,
        model: &str,
    ) -> crate::layer_store::MediaLayerSummary {
        use crate::layer_store::{LayerProvenance, MediaLayerKind, NewMediaLayer};
        let items = output
            .items
            .iter()
            .map(|item| crate::layer_store::MediaLayerItem {
                start_sec: item.start_sec,
                end_sec: item.end_sec,
                searchable_text: item.searchable_text.clone(),
                payload: serde_json::json!({"source": "mock layer pass"}),
                frame_asset_ids: None,
            })
            .collect();
        let driver = &**server.gallery_store.driver();
        crate::layer_store::store_layer(
            driver,
            &NewMediaLayer {
                media_path: "/tmp/mock-layer-pass.mp4".to_string(),
                gallery_asset_id: None,
                kind: MediaLayerKind::Researcher,
                label: Some("mock pass layer".to_string()),
                provenance: LayerProvenance {
                    model: model.to_string(),
                    prompt_template: "mock_layer_pass".to_string(),
                    created_at: hkask_types::time::now_rfc3339(),
                    parameters: None,
                },
                duration_secs: 2.0,
                items,
            },
        )
        .expect("the core's outputs store as a layer")
    }

    /// t3 AC1a: the v1 prompt-schema mode drives a stored layer from a mock
    /// inference response — one inference call per chunk, typed outputs,
    /// the layer recallable through the store with the resolved model in
    /// its provenance.
    #[tokio::test]
    async fn layer_pass_prompt_mode_drives_a_stored_layer_from_a_mock_response() {
        let inference = mock_layer_inference(mock_layer_response(), String::new(), false);
        let schema = mock_layer_schema();
        let prompts = vec!["analyze chunk 0".to_string()];
        let (attempts, rejections) = (AtomicU64::new(0), AtomicU64::new(0));
        let result = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &inference,
            &schema,
            PassMode::PromptSchema,
            Some("test-pass-model"),
            &prompts,
            0,
            10,
            &attempts,
            &rejections,
            "MockLayerPassOutput {\"items\": [...]}",
        )
        .await
        .expect("the prompt-schema pass core succeeds");
        assert_eq!(result.model, "test-pass-model");
        assert_eq!(result.resume_from, None);
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(rejections.load(Ordering::Relaxed), 0);
        assert_eq!(result.outputs.len(), 1);
        assert_eq!(result.outputs[0].items.len(), 2);

        let server = make_server();
        let summary = store_mock_pass_layer(&server, &result.outputs[0], &result.model);
        assert_eq!(summary.items_count, 2);
        let driver = &**server.gallery_store.driver();
        let record = crate::layer_store::get_layer(driver, &summary.id)
            .expect("get layer")
            .expect("stored layer present");
        assert_eq!(record.items.len(), 2);
        assert_eq!(record.items[1].item.searchable_text, "second moment");
        assert_eq!(record.provenance.model, "test-pass-model");
    }

    /// t3 AC1a (structured half): the v2 mode drives a stored layer through
    /// `chat_json` — the mock's media arm serves the response inside
    /// `{"text": …}` and the core reads exactly that member.
    #[tokio::test]
    async fn layer_pass_structured_mode_drives_a_stored_layer_via_chat_json() {
        let inference = mock_layer_inference(String::new(), mock_layer_response(), false);
        let schema = mock_layer_schema();
        let prompts = vec!["analyze chunk 0".to_string()];
        let (attempts, rejections) = (AtomicU64::new(0), AtomicU64::new(0));
        let result = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &inference,
            &schema,
            PassMode::Structured,
            Some("test-pass-model"),
            &prompts,
            0,
            10,
            &attempts,
            &rejections,
            "MockLayerPassOutput {\"items\": [...]}",
        )
        .await
        .expect("the structured pass core succeeds");
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(rejections.load(Ordering::Relaxed), 0);
        assert_eq!(result.outputs[0].items.len(), 2);

        let server = make_server();
        let summary = store_mock_pass_layer(&server, &result.outputs[0], &result.model);
        assert_eq!(summary.items_count, 2);
        let driver = &**server.gallery_store.driver();
        let record = crate::layer_store::get_layer(driver, &summary.id)
            .expect("get layer")
            .expect("stored layer present");
        assert_eq!(record.items[0].item.end_sec, Some(1.0));
    }

    /// t3 AC1b: a plan larger than `max_chunks` processes the bounded slice
    /// and returns the resume cursor naming the next chunk — resuming
    /// completes the plan with no re-processing.
    #[tokio::test]
    async fn layer_pass_returns_a_resume_cursor_when_input_exceeds_max_chunks() {
        let inference = mock_layer_inference(mock_layer_response(), String::new(), false);
        let schema = mock_layer_schema();
        let prompts: Vec<String> = (0..3)
            .map(|index| format!("analyze chunk {index}"))
            .collect();
        let (attempts, rejections) = (AtomicU64::new(0), AtomicU64::new(0));
        let first = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &inference,
            &schema,
            PassMode::PromptSchema,
            Some("test-pass-model"),
            &prompts,
            0,
            2,
            &attempts,
            &rejections,
            "MockLayerPassOutput {\"items\": [...]}",
        )
        .await
        .expect("the first bounded slice succeeds");
        assert_eq!(first.outputs.len(), 2, "only the bounded slice runs");
        assert_eq!(
            first.resume_from,
            Some(2),
            "the cursor names the next chunk"
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 2);

        let (attempts, rejections) = (AtomicU64::new(0), AtomicU64::new(0));
        let second = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &inference,
            &schema,
            PassMode::PromptSchema,
            Some("test-pass-model"),
            &prompts,
            first.resume_from.expect("cursor"),
            2,
            &attempts,
            &rejections,
            "MockLayerPassOutput {\"items\": [...]}",
        )
        .await
        .expect("resuming from the cursor succeeds");
        assert_eq!(second.outputs.len(), 1);
        assert_eq!(second.resume_from, None, "the plan is complete");
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(rejections.load(Ordering::Relaxed), 0);
    }

    /// t3 AC2: model resolution follows the per-call override, then
    /// `HKASK_MEDIA_PASS_MODEL`, then `HKASK_MEDIA_VISION_MODEL`; nothing
    /// configured surfaces the typed NotConfigured error naming the env
    /// vars — never a hidden constant.
    #[tokio::test]
    async fn layer_pass_model_resolution_chain_prefers_override_then_pass_env_then_vision_env() {
        let _env_lock = MODEL_ENV_LOCK.lock().await;
        let inference = mock_layer_inference(mock_layer_response(), String::new(), false);
        let schema = mock_layer_schema();
        let prompts = vec!["analyze chunk 0".to_string()];
        let expected = "MockLayerPassOutput {\"items\": [...]}";

        {
            let _env = ModelEnvGuard::set(Some("pass-env-model"), Some("vision-env-model"));
            let result = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
                &inference,
                &schema,
                PassMode::PromptSchema,
                Some("per-call-override"),
                &prompts,
                0,
                1,
                &AtomicU64::new(0),
                &AtomicU64::new(0),
                expected,
            )
            .await
            .expect("override resolves");
            assert_eq!(result.model, "per-call-override", "the override wins");
        }
        {
            let _env = ModelEnvGuard::set(Some("pass-env-model"), Some("vision-env-model"));
            let result = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
                &inference,
                &schema,
                PassMode::PromptSchema,
                None,
                &prompts,
                0,
                1,
                &AtomicU64::new(0),
                &AtomicU64::new(0),
                expected,
            )
            .await
            .expect("pass env resolves");
            assert_eq!(
                result.model, "pass-env-model",
                "HKASK_MEDIA_PASS_MODEL wins"
            );
        }
        {
            let _env = ModelEnvGuard::set(None, Some("vision-env-model"));
            let result = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
                &inference,
                &schema,
                PassMode::PromptSchema,
                None,
                &prompts,
                0,
                1,
                &AtomicU64::new(0),
                &AtomicU64::new(0),
                expected,
            )
            .await
            .expect("vision fallback resolves");
            assert_eq!(
                result.model, "vision-env-model",
                "the vision fallback serves"
            );
        }
        {
            let _env = ModelEnvGuard::set(None, None);
            let error = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
                &inference,
                &schema,
                PassMode::PromptSchema,
                None,
                &prompts,
                0,
                1,
                &AtomicU64::new(0),
                &AtomicU64::new(0),
                expected,
            )
            .await
            .expect_err("nothing configured fails visibly");
            let message = error.to_string();
            assert!(
                message.contains("HKASK_MEDIA_PASS_MODEL"),
                "names the pass env var: {message}"
            );
            assert!(
                message.contains("HKASK_MEDIA_VISION_MODEL"),
                "names the vision fallback env var: {message}"
            );
        }
    }

    /// t3 AC3: unparseable model output surfaces as the typed
    /// UnparseableOutput error — naming the expected shape, carrying the
    /// truncated raw text, and ticking the rejection counter — never a
    /// silent empty result. The structured mode's text-less chat_json
    /// response is the same class.
    #[tokio::test]
    async fn layer_pass_unparseable_output_surfaces_as_a_typed_error_with_counters() {
        let inference = mock_layer_inference("not-json {{{".to_string(), String::new(), false);
        let schema = mock_layer_schema();
        let prompts = vec!["analyze chunk 0".to_string()];
        let (attempts, rejections) = (AtomicU64::new(0), AtomicU64::new(0));
        let error = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &inference,
            &schema,
            PassMode::PromptSchema,
            Some("test-pass-model"),
            &prompts,
            0,
            1,
            &attempts,
            &rejections,
            "MockLayerPassOutput {\"items\": [...]}",
        )
        .await
        .expect_err("unparseable output must surface");
        let message = error.to_string();
        assert!(
            message.contains("failed to parse"),
            "names the unparseable-output class: {message}"
        );
        assert!(
            message.contains("MockLayerPassOutput"),
            "names the expected shape: {message}"
        );
        assert!(
            message.contains("not-json"),
            "carries the raw model output: {message}"
        );
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(rejections.load(Ordering::Relaxed), 1);

        // A chat_json response with no "text" member — the structured
        // mode's unparseable class.
        let broken = mock_layer_inference(
            String::new(),
            serde_json::json!({"unexpected": true}).to_string(),
            true,
        );
        let error = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &broken,
            &schema,
            PassMode::Structured,
            Some("test-pass-model"),
            &prompts,
            0,
            1,
            &AtomicU64::new(0),
            &AtomicU64::new(0),
            "MockLayerPassOutput {\"items\": [...]}",
        )
        .await
        .expect_err("a text-less chat_json response must surface");
        assert!(
            error.to_string().contains("chat_json"),
            "names the structured-mode contract: {error}"
        );
    }

    /// The chunk-plan bounds are the caller's contract with the core —
    /// violations surface as the typed ChunkPlan error, never a panic.
    #[tokio::test]
    async fn layer_pass_rejects_invalid_chunk_plans() {
        let inference = mock_layer_inference(mock_layer_response(), String::new(), false);
        let schema = mock_layer_schema();
        let prompts = vec!["analyze chunk 0".to_string()];
        let expected = "MockLayerPassOutput {\"items\": [...]}";
        for (resume_from, max_chunks, name) in [
            (0usize, 0usize, "max_chunks zero"),
            (2, 1, "resume past plan"),
        ] {
            let error = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
                &inference,
                &schema,
                PassMode::PromptSchema,
                Some("test-pass-model"),
                &prompts,
                resume_from,
                max_chunks,
                &AtomicU64::new(0),
                &AtomicU64::new(0),
                expected,
            )
            .await
            .expect_err(name);
            assert!(
                error.to_string().contains("invalid chunk plan"),
                "{name} surfaces as ChunkPlan: {error}"
            );
        }
        let empty: Vec<String> = Vec::new();
        let error = crate::layer_pass::run_layer_pass::<MockLayerPassOutput>(
            &inference,
            &schema,
            PassMode::PromptSchema,
            Some("test-pass-model"),
            &empty,
            0,
            1,
            &AtomicU64::new(0),
            &AtomicU64::new(0),
            expected,
        )
        .await
        .expect_err("empty plan rejected");
        assert!(
            error.to_string().contains("no chunks"),
            "the empty plan names the caller's duty: {error}"
        );
    }

    // ── Semantic vision pass contract tests (t4) ───────────────────────

    /// The vision-capable layer-pass mock: the vision arm serves the canned
    /// chunk output — the capability `layers_semantic_pass` requires (its
    /// text arm is never reached and fails closed if it were).
    struct VisionLayerMockPort {
        vision_response: String,
    }

    crate::test_support::media_inference_stub! {
        VisionLayerMockPort,
        generate(self, _prompt): {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "text generate unused — the vision pass calls generate_vision".into(),
                ))
            })
        },
        generate_vision(self, _prompt, _images): {
            let result = hkask_types::InferenceResult {
                text: self.vision_response.clone(),
                model: "mock-vision-model".to_string(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                    reported: true,
                },
                finish_reason: "stop".to_string(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            };
            Box::pin(async move { Ok(result) })
        },
    }

    /// The vision-pass environment: the media test env, a real source.mp4,
    /// and a server over the gallery whose inference port serves the
    /// canned vision response — the `video_server_env` construction with
    /// the pass capability swapped in (the capability-carrying constructor).
    /// The tuple's reverse drop order keeps the guard discipline: the env
    /// restores before the lock releases.
    async fn vision_pass_env(
        vision_response: String,
    ) -> Result<
        (
            tokio::sync::MutexGuard<'static, ()>,
            tempfile::TempDir,
            ArtifactsEnvGuard,
            tempfile::TempDir,
            std::path::PathBuf,
            MediaServer,
        ),
        Box<dyn std::error::Error>,
    > {
        let (env_lock, artifacts, env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("source.mp4");
        create_real_video(&source).await?;
        let port: Arc<dyn hkask_types::InferencePort> =
            Arc::new(VisionLayerMockPort { vision_response });
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            port,
            Arc::new(std::sync::Mutex::new(Some(GalleryState {
                path: gallery_root.path().to_path_buf(),
                mode: GalleryMode::ReadOnly,
                gallery_id: Some(gallery.id.clone()),
            }))),
            store.clone(),
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        );
        Ok((env_lock, artifacts, env, gallery_root, source, server))
    }

    /// The canned one-chunk vision output: caption, one event, one object
    /// with an in-range box, one OCR item — the shape every kind's
    /// normalization consumes.
    fn canned_vision_response() -> String {
        serde_json::json!({
            "caption": {"text": "a solid blue test frame", "confidence": 0.9},
            "events": [{"label": "static scene", "confidence": 0.8}],
            "objects": [{
                "label": "blue square",
                "confidence": 0.7,
                "box": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4},
                "frame_index": 0
            }],
            "ocr": [{
                "text": "TEST-2026",
                "confidence": 0.6,
                "box": {"x": 0.0, "y": 0.0, "w": 0.5, "h": 0.1},
                "frame_index": 0
            }]
        })
        .to_string()
    }

    fn semantic_pass_request(media_path: String) -> crate::types::LayersSemanticPassRequest {
        crate::types::LayersSemanticPassRequest {
            media_path,
            gallery_asset_id: None,
            time_range: Some(crate::types::LayersTimeRangeRequest {
                start_sec: 0.0,
                end_sec: Some(2.0),
            }),
            shot_layer_id: None,
            frames_per_chunk: None,
            chunk_duration_secs: None,
            max_chunks: None,
            resume_from: None,
            structured: None,
            model: Some("mock-vision-model".to_string()),
        }
    }

    // ── Audio pass contract tests (t5) ────────────────────────────────

    /// A real audio fixture with one distinctive non-speech sound: 0.75s
    /// silence, a 0.5s 880 Hz beep, 0.75s of trailing silence — exactly
    /// 2.0s, the beep at source-PTS [0.75, 1.25].
    async fn create_beep_wav(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let output = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=0.5",
                "-af",
                "adelay=750|750,apad=pad_dur=0.75",
                "-ar",
                "16000",
                "-ac",
                "1",
            ])
            .arg(path)
            .output()
            .await?;
        if !output.status.success() {
            return Err(format!(
                "beep fixture creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    /// The audio-capable layer-pass mock: the media_generate arm serves
    /// the canned chunk output — the capability `layers_audio_pass`
    /// requires (its text and vision arms are never reached and fail
    /// closed if they were).
    struct AudioLayerMockPort {
        audio_response: String,
    }

    crate::test_support::media_inference_stub! {
        AudioLayerMockPort,
        generate(self, _prompt): {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "text generate unused — the audio pass calls media_generate".into(),
                ))
            })
        },
        media_generate(self, _op, _params): {
            let text = self.audio_response.clone();
            Box::pin(
                async move {
                    Ok(serde_json::json!({ "text": text, "model": "mock-audio-model" }))
                },
            )
        },
    }

    /// The audio-pass environment: the shared media test env, a real
    /// beep fixture, and a server over the audio mock port. The tuple's
    /// reverse drop order keeps the guard discipline (env restores before
    /// the lock releases).
    async fn audio_pass_env(
        audio_response: String,
    ) -> Result<
        (
            tokio::sync::MutexGuard<'static, ()>,
            tempfile::TempDir,
            ArtifactsEnvGuard,
            tempfile::TempDir,
            std::path::PathBuf,
            MediaServer,
        ),
        Box<dyn std::error::Error>,
    > {
        let (env_lock, artifacts, env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("beep-source.wav");
        create_beep_wav(&source).await?;
        let port: Arc<dyn hkask_types::InferencePort> =
            Arc::new(AudioLayerMockPort { audio_response });
        let server = MediaServer::new(
            hkask_types::WebID::new(),
            port,
            Arc::new(std::sync::Mutex::new(Some(GalleryState {
                path: gallery_root.path().to_path_buf(),
                mode: GalleryMode::ReadOnly,
                gallery_id: Some(gallery.id.clone()),
            }))),
            store.clone(),
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        );
        Ok((env_lock, artifacts, env, gallery_root, source, server))
    }

    /// The bounded-range audio-pass request: the range starts at 0.5s, so
    /// the beep's chunk-relative span [0.25, 0.75) maps onto source-PTS
    /// [0.75, 1.25) — the offset mapping is the assertion's point.
    fn audio_pass_request(media_path: String) -> crate::types::LayersAudioPassRequest {
        crate::types::LayersAudioPassRequest {
            media_path,
            gallery_asset_id: None,
            time_range: Some(crate::types::LayersTimeRangeRequest {
                start_sec: 0.5,
                end_sec: Some(2.0),
            }),
            chunk_duration_secs: Some(3.0),
            max_chunks: None,
            resume_from: None,
            structured: None,
            model: Some("mock-audio-model".to_string()),
        }
    }

    /// t5 AC1: the fixture's distinctive non-speech sound yields a labeled
    /// event span inside the probed duration — the chunk-relative span is
    /// mapped onto the source timeline — and the layer's provenance
    /// records the model and template (AC3). The chunk's extracted wav is
    /// gone after the pass (the hermetic intermediate observation).
    #[tokio::test]
    async fn layers_audio_pass_labels_a_distinctive_sound_inside_duration()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, _gallery_root, source, server) = audio_pass_env(
            serde_json::json!({
                "events": [{
                    "label": "beep",
                    "start_sec": 0.25,
                    "end_sec": 0.75,
                    "confidence": 0.9,
                }],
            })
            .to_string(),
        )
        .await?;
        let temp_before = temp_media_files(server.ffmpeg.temp_dir())?;
        let media_path = source.to_string_lossy().into_owned();

        let result = content_of(
            &server
                .layers_audio_pass(Parameters(audio_pass_request(media_path.clone())))
                .await?,
        );
        assert_eq!(result["status"], serde_json::json!("completed"));
        assert_eq!(result["layer"]["kind"], serde_json::json!("audio_event"));
        assert_eq!(result["coverage"]["events_count"].as_u64(), Some(1));
        let layer_id = result["layer"]["id"]
            .as_str()
            .ok_or("layer id")?
            .to_string();

        let layer = content_of(
            &server
                .layers_get(Parameters(crate::types::LayersGetRequest {
                    layer_id: layer_id.clone(),
                }))
                .await?,
        )["layer"]
            .clone();
        let item = layer["items"][0].clone();
        assert_eq!(item["start_sec"], serde_json::json!(0.75));
        assert_eq!(item["end_sec"], serde_json::json!(1.25));
        assert_eq!(item["searchable_text"], serde_json::json!("beep"));
        assert_eq!(
            layer["provenance"]["model"],
            serde_json::json!("mock-audio-model")
        );
        assert_eq!(
            layer["provenance"]["prompt_template"],
            serde_json::json!("layers_audio_pass"),
            "AC3: provenance records the model and template"
        );

        let temp_after = temp_media_files(server.ffmpeg.temp_dir())?;
        assert_eq!(
            temp_after.difference(&temp_before).count(),
            0,
            "the extracted chunk wav leaked: {:?}",
            temp_after.difference(&temp_before).collect::<Vec<_>>()
        );
        Ok(())
    }

    /// t5 AC2: media without an audio stream returns a named degradation,
    /// never silent empty success — and nothing is stored.
    #[tokio::test]
    async fn layers_audio_pass_without_an_audio_stream_is_a_named_degradation()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let video = gallery_root.path().join("no-audio-source.mp4");
        create_real_video(&video).await?;
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());

        let error = server
            .layers_audio_pass(Parameters(audio_pass_request(
                video.to_string_lossy().into_owned(),
            )))
            .await
            .expect_err("a video-only file must fail the audio pass");
        assert!(
            error.to_string().contains("audio stream"),
            "the degradation names the requirement: {error}"
        );

        let listed = content_of(
            &server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(video.to_string_lossy().into_owned()),
                    kind: Some(crate::layer_store::MediaLayerKind::AudioEvent),
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(listed["count"].as_u64(), Some(0), "nothing was stored");
        Ok(())
    }

    /// A pass that finds no events returns a named no_events note and
    /// stores nothing — never silent empty success.
    #[tokio::test]
    async fn layers_audio_pass_with_no_events_is_a_named_no_events_note()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, _gallery_root, source, server) =
            audio_pass_env(serde_json::json!({"events": []}).to_string()).await?;
        let media_path = source.to_string_lossy().into_owned();

        let result = content_of(
            &server
                .layers_audio_pass(Parameters(audio_pass_request(media_path.clone())))
                .await?,
        );
        assert_eq!(result["status"], serde_json::json!("no_events"));
        assert!(
            result["note"]
                .as_str()
                .is_some_and(|note| note.contains("no audio events")),
            "the note names the degradation: {}",
            result["note"]
        );

        let listed = content_of(
            &server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path),
                    kind: Some(crate::layer_store::MediaLayerKind::AudioEvent),
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(listed["count"].as_u64(), Some(0), "nothing was stored");
        Ok(())
    }

    /// t6 AC1: one query returns hits from at least two distinct layer
    /// kinds plus a transcript hit, each with time range and layer
    /// provenance.
    #[tokio::test]
    async fn layers_search_hits_two_layer_kinds_plus_a_transcript()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let server = server_with_gallery(store.clone(), gallery.id.clone(), gallery_root.path());
        let unified_recording = gallery_root.path().join("unified-recording.wav");
        create_real_audio(&unified_recording).await?;
        let media_path = unified_recording.to_string_lossy().into_owned();
        // Two distinct layer kinds over the same recording.
        for kind in [
            crate::layer_store::MediaLayerKind::Researcher,
            crate::layer_store::MediaLayerKind::AudioEvent,
        ] {
            let stored = content_of(
                &server
                    .layers_store(Parameters(LayersStoreRequest {
                        media_path: media_path.to_string(),
                        gallery_asset_id: None,
                        kind,
                        label: None,
                        provenance: LayersProvenanceRequest {
                            model: "test".to_string(),
                            prompt_template: "layers_search-test".to_string(),
                        },
                        items: vec![LayersStoreItemRequest {
                            start_sec: 0.5,
                            end_sec: Some(1.5),
                            searchable_text: "beep one".to_string(),
                            payload: hkask_types::AnyJsonValue(
                                serde_json::json!({ "label": "beep" }),
                            ),
                            frame_asset_ids: None,
                        }],
                    }))
                    .await?,
            );
            let _ = stored;
        }
        // A transcript over the same recording whose working text contains
        // the query twice.
        let bundle = crate::transcript::TranscriptBundle {
            words: vec![
                crate::transcript::TimedWord {
                    word: "beep".to_string(),
                    start_ms: 250,
                    end_ms: 500,
                    confidence: None,
                },
                crate::transcript::TimedWord {
                    word: "one".to_string(),
                    start_ms: 600,
                    end_ms: 750,
                    confidence: None,
                },
            ],
            ..crate::transcript::TranscriptBundle::new(
                media_path.to_string(),
                1.0,
                "beep one".to_string(),
            )
        };
        let _ = content_of(
            &server
                .educt_store_transcript(Parameters(crate::types::EductStoreTranscriptRequest {
                    transcript: hkask_types::AnyJsonValue(serde_json::to_value(&bundle).unwrap()),
                    gallery_asset_id: None,
                }))
                .await?,
        );
        let result = content_of(
            &server
                .layers_search(Parameters(crate::types::LayersSearchRequest {
                    query: "beep".to_string(),
                    mode: "lexical".to_string(),
                    limit: Some(10),
                    recording_limit: Some(10),
                    recording_offset: None,
                    start_byte: None,
                    media_path: Some(media_path.to_string()),
                    kind: None,
                }))
                .await?,
        );
        assert_eq!(result["count"], serde_json::json!(3));
        let sources: Vec<&str> = result["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["source"].as_str().unwrap())
            .collect();
        assert!(sources.contains(&"layer"));
        assert_eq!(
            sources
                .iter()
                .filter(|source| source.contains(&"transcript"))
                .count(),
            1
        );
        let layer_kinds: Vec<&str> = result["hits"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|hit| hit["source"] == "layer")
            .map(|hit| hit["kind"].as_str().unwrap())
            .collect();
        assert!(layer_kinds.contains(&"researcher"));
        assert!(layer_kinds.contains(&"audio_event"));
        for hit in result["hits"].as_array().unwrap().iter() {
            assert!(hit["start_byte"].is_u64());
            assert!(hit["end_byte"].is_u64());
            if hit["source"] == serde_json::json!("layer") {
                assert!(hit["provenance"].is_object());
            }
        }
        Ok(())
    }

    /// t6 AC2: matching, cap and resume behavior match
    /// educt_search_transcripts — over a corpus with no layers the hits
    /// and cursor are identical; a layer item prefix shifts the
    /// transcript hits' stream bytes by exactly its length.
    #[tokio::test]
    async fn layers_search_matching_matches_educt_search_transcripts()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let server = server_with_gallery(store.clone(), gallery.id.clone(), gallery_root.path());
        let educt_parity = gallery_root.path().join("educt-parity.wav");
        create_real_audio(&educt_parity).await?;
        let media_path = educt_parity.to_string_lossy().into_owned();
        let bundle = crate::transcript::TranscriptBundle {
            words: vec![
                crate::transcript::TimedWord {
                    word: "beep".to_string(),
                    start_ms: 250,
                    end_ms: 500,
                    confidence: None,
                },
                crate::transcript::TimedWord {
                    word: "one".to_string(),
                    start_ms: 600,
                    end_ms: 750,
                    confidence: None,
                },
            ],
            ..crate::transcript::TranscriptBundle::new(
                media_path.clone(),
                1.0,
                "beep one".to_string(),
            )
        };
        let stored = content_of(
            &server
                .educt_store_transcript(Parameters(crate::types::EductStoreTranscriptRequest {
                    transcript: hkask_types::AnyJsonValue(serde_json::to_value(&bundle).unwrap()),
                    gallery_asset_id: None,
                }))
                .await?,
        );
        let transcript_id = stored["id"].as_str().unwrap().to_string();
        let educt = content_of(
            &server
                .educt_search_transcripts(Parameters(
                    crate::tools::educt::EductSearchTranscriptsRequest {
                        query: "beep".to_string(),
                        mode: "lexical".to_string(),
                        limit: Some(1),
                        recording_limit: Some(10),
                        recording_offset: None,
                        start_byte: None,
                        gallery_asset_id: None,
                    },
                ))
                .await?,
        );
        let unified = content_of(
            &server
                .layers_search(Parameters(crate::types::LayersSearchRequest {
                    query: "beep".to_string(),
                    mode: "lexical".to_string(),
                    limit: Some(1),
                    recording_limit: Some(10),
                    recording_offset: None,
                    start_byte: None,
                    media_path: Some(media_path.clone()),
                    kind: None,
                }))
                .await?,
        );
        assert_eq!(educt["count"], unified["count"]);
        // The unified hit is the educt citation plus the source field; the
        // citation's matching fields are identical.
        let educt_hit = &educt["hits"][0];
        let unified_hit = &unified["hits"][0];
        assert_eq!(educt_hit["matched_text"], unified_hit["matched_text"]);
        assert_eq!(educt_hit["start_byte"], unified_hit["start_byte"]);
        assert_eq!(educt_hit["end_byte"], unified_hit["end_byte"]);
        assert_eq!(educt_hit["word_range"], unified_hit["word_range"]);
        assert_eq!(educt_hit["time_range_ms"], unified_hit["time_range_ms"]);
        assert_eq!(educt_hit["alignment"], unified_hit["alignment"]);
        assert_eq!(
            educt_hit["correction_layer_id"],
            unified_hit["correction_layer_id"]
        );
        assert_eq!(
            educt_hit["realignment_layer_id"],
            unified_hit["realignment_layer_id"]
        );
        // Now a layer item over the same recording: the stream shifts the
        // transcript hit by exactly the item text's length.
        let stored_layer = content_of(
            &server
                .layers_store(Parameters(LayersStoreRequest {
                    media_path: media_path.clone(),
                    gallery_asset_id: None,
                    kind: crate::layer_store::MediaLayerKind::Researcher,
                    label: None,
                    provenance: LayersProvenanceRequest {
                        model: "test".to_string(),
                        prompt_template: "layers_search-shift-test".to_string(),
                    },
                    items: vec![LayersStoreItemRequest {
                        start_sec: 0.0,
                        end_sec: Some(0.5),
                        searchable_text: "shift".to_string(),
                        payload: hkask_types::AnyJsonValue(serde_json::json!({ "label": "shift" })),
                        frame_asset_ids: None,
                    }],
                }))
                .await?,
        );
        let _ = stored_layer;
        let shifted = content_of(
            &server
                .layers_search(Parameters(crate::types::LayersSearchRequest {
                    query: "beep".to_string(),
                    mode: "lexical".to_string(),
                    limit: Some(5),
                    recording_limit: Some(10),
                    recording_offset: None,
                    start_byte: None,
                    media_path: Some(media_path.clone()),
                    kind: None,
                }))
                .await?,
        );
        let shift = "shift".len();
        let hit = &shifted["hits"][0];
        assert_eq!(hit["source"], serde_json::json!("transcript"));
        assert_eq!(
            hit["start_byte"],
            serde_json::json!(educt["hits"][0]["start_byte"].as_u64().unwrap() + shift as u64)
        );
        assert_eq!(hit["transcript_id"], serde_json::json!(transcript_id));
        Ok(())
    }

    /// t6 AC3: recordings lacking layers or a transcript return their
    /// available hits with an explicit scope note naming what was
    /// skipped — never a silent skip.
    #[tokio::test]
    async fn layers_search_names_missing_sides_in_scope_notes()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let server = server_with_gallery(store.clone(), gallery.id.clone(), gallery_root.path());
        let layers_only_path = gallery_root.path().join("layers-only.wav");
        create_real_audio(&layers_only_path).await?;
        let transcript_only_path = gallery_root.path().join("transcript-only.wav");
        create_real_audio(&transcript_only_path).await?;
        let layers_only = layers_only_path.to_string_lossy().into_owned();
        let transcript_only = transcript_only_path.to_string_lossy().into_owned();
        let _ = content_of(
            &server
                .layers_store(Parameters(LayersStoreRequest {
                    media_path: layers_only.clone(),
                    gallery_asset_id: None,
                    kind: crate::layer_store::MediaLayerKind::Researcher,
                    label: None,
                    provenance: LayersProvenanceRequest {
                        model: "test".to_string(),
                        prompt_template: "layers_search-scope-test".to_string(),
                    },
                    items: vec![LayersStoreItemRequest {
                        start_sec: 0.0,
                        end_sec: Some(0.5),
                        searchable_text: "beep one".to_string(),
                        payload: hkask_types::AnyJsonValue(serde_json::json!({ "label": "beep" })),
                        frame_asset_ids: None,
                    }],
                }))
                .await?,
        );
        let bundle = crate::transcript::TranscriptBundle {
            words: vec![
                crate::transcript::TimedWord {
                    word: "beep".to_string(),
                    start_ms: 250,
                    end_ms: 500,
                    confidence: None,
                },
                crate::transcript::TimedWord {
                    word: "two".to_string(),
                    start_ms: 600,
                    end_ms: 750,
                    confidence: None,
                },
            ],
            ..crate::transcript::TranscriptBundle::new(
                transcript_only.clone(),
                1.0,
                "beep two".to_string(),
            )
        };
        let _ = content_of(
            &server
                .educt_store_transcript(Parameters(crate::types::EductStoreTranscriptRequest {
                    transcript: hkask_types::AnyJsonValue(serde_json::to_value(&bundle).unwrap()),
                    gallery_asset_id: None,
                }))
                .await?,
        );
        let result = content_of(
            &server
                .layers_search(Parameters(crate::types::LayersSearchRequest {
                    query: "beep".to_string(),
                    mode: "lexical".to_string(),
                    limit: Some(10),
                    recording_limit: Some(10),
                    recording_offset: None,
                    start_byte: None,
                    media_path: None,
                    kind: None,
                }))
                .await?,
        );
        let scope_notes: Vec<String> = result["scope_notes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|note| note.as_str().map(str::to_string))
            .collect();
        let notes_joined = scope_notes.join("; ");
        assert!(
            notes_joined.contains(&format!("no transcript for {layers_only}")),
            "scope notes must name the skipped side: {}",
            notes_joined
        );
        assert!(
            notes_joined.contains(&format!("no layers for {transcript_only}"))
                || notes_joined.contains(&format!("no researcher layers for {transcript_only}")),
            "scope notes must name the recording with no layers: {}",
            notes_joined
        );
        assert_eq!(
            result["hits"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|hit| hit["media_path"] == serde_json::json!(layers_only))
                .count(),
            1
        );
        assert_eq!(
            result["hits"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|hit| hit["media_path"] == serde_json::json!(transcript_only))
                .count(),
            1
        );
        Ok(())
    }

    /// t4 AC1: the fixture yields an object item whose box lies within
    /// 0..1 and whose frame asset id resolves to an existing gallery asset
    /// with source_pts_sec inside the item range.
    #[tokio::test]
    async fn layers_semantic_pass_yields_objects_with_valid_boxes_and_resolvable_frames()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, _gallery_root, source, server) =
            vision_pass_env(canned_vision_response()).await?;
        let result = content_of(
            &server
                .layers_semantic_pass(Parameters(semantic_pass_request(
                    source.to_string_lossy().into_owned(),
                )))
                .await?,
        );
        assert_eq!(result["status"], serde_json::json!("completed"));
        assert_eq!(
            result["coverage"]["chunking_mode"],
            serde_json::json!("time_range")
        );
        let object_layer = result["layers"]
            .as_array()
            .ok_or("layers array")?
            .iter()
            .find(|layer| layer["kind"] == serde_json::json!("object"))
            .ok_or("an object layer was stored")?;
        let layer_id = object_layer["id"]
            .as_str()
            .ok_or("object layer id")?
            .to_string();

        let fetched = content_of(
            &server
                .layers_get(Parameters(crate::types::LayersGetRequest {
                    layer_id: layer_id.clone(),
                }))
                .await?,
        );
        let items = fetched["layer"]["items"].as_array().ok_or("object items")?;
        assert_eq!(items.len(), 1);
        let item = &items[0];
        let r#box = &item["payload"]["box"];
        for name in ["x", "y", "w", "h"] {
            let value = r#box[name].as_f64().ok_or("box value")?;
            assert!(
                (0.0..=1.0).contains(&value),
                "box {name}={value} lies within 0..1"
            );
        }
        assert_eq!(
            r#box["x"].as_f64().unwrap_or(0.0) + r#box["w"].as_f64().unwrap_or(0.0),
            0.4
        );
        assert_eq!(item["payload"]["confidence"], serde_json::json!(0.7));

        let frame_asset_id = item["frame_asset_ids"][0]
            .as_str()
            .ok_or("frame asset ref")?
            .to_string();
        let frame_record = server
            .gallery_store
            .get_by_id(&frame_asset_id)
            .map_err(|e| format!("frame asset lookup: {e}"))?;
        assert!(!frame_record.missing, "the referenced frame asset exists");

        let item_start = item["start_sec"].as_f64().ok_or("item start")?;
        let item_end = item["end_sec"].as_f64().ok_or("item end")?;
        let frame_entry = result["coverage"]["frames"]
            .as_array()
            .ok_or("coverage frames")?
            .iter()
            .find(|frame| frame["gallery_asset_id"] == serde_json::json!(frame_asset_id))
            .ok_or("the frame appears in the coverage inventory")?;
        let frame_pts = frame_entry["source_pts_sec"].as_f64().ok_or("frame pts")?;
        assert!(
            frame_pts >= item_start && frame_pts <= item_end,
            "the frame's source_pts_sec {frame_pts} lies inside the item range [{item_start}, {item_end}]"
        );
        assert!(
            result["coverage"]["items"]["ocr"]
                .as_u64()
                .is_some_and(|n| n >= 1),
            "the OCR kind produced its items too"
        );
        Ok(())
    }

    /// t4 AC2: structured=true and structured=false both produce valid
    /// stored layers through the capability-carrying constructor; the
    /// response names the mode actually used (image inputs have no
    /// provider-enforced strict schema — the structured run says so).
    #[tokio::test]
    async fn layers_semantic_pass_structured_and_prompt_modes_both_store_layers()
    -> Result<(), Box<dyn std::error::Error>> {
        for structured in [false, true] {
            let (_env_lock, _artifacts, _env, _gallery_root, source, server) =
                vision_pass_env(canned_vision_response()).await?;
            let mut request = semantic_pass_request(source.to_string_lossy().into_owned());
            request.structured = Some(structured);
            let result = content_of(
                &server
                    .layers_semantic_pass(Parameters(request))
                    .await
                    .expect("the pass stores layers in both modes"),
            );
            assert_eq!(result["status"], serde_json::json!("completed"));
            let layers = result["layers"].as_array().ok_or("layers array")?;
            assert_eq!(layers.len(), 3, "all three kinds stored: {layers:?}");
            if structured {
                assert_eq!(result["mode"]["requested"], serde_json::json!("structured"));
                assert!(
                    result["mode"]["note"].as_str().is_some_and(|note| {
                        note.contains("no provider-enforced strict-schema path")
                    }),
                    "the structured run names the image-input limitation: {}",
                    result["mode"]
                );
            } else {
                assert_eq!(
                    result["mode"]["requested"],
                    serde_json::json!("prompt_schema")
                );
            }
            for layer in layers {
                assert_eq!(
                    layer["provenance"]["parameters"]["structured"],
                    serde_json::json!(structured),
                    "provenance records the mode"
                );
            }
        }
        Ok(())
    }

    /// t4 AC3: a plan beyond max_chunks processes the bounded slice and
    /// returns the resume cursor with plan-vs-observed coverage; resuming
    /// completes the plan.
    #[tokio::test]
    async fn layers_semantic_pass_beyond_max_chunks_returns_resume_cursor_with_coverage()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, _gallery_root, source, server) =
            vision_pass_env(canned_vision_response()).await?;
        let mut request = semantic_pass_request(source.to_string_lossy().into_owned());
        request.chunk_duration_secs = Some(0.5);
        request.max_chunks = Some(2);
        let result = content_of(&server.layers_semantic_pass(Parameters(request)).await?);
        assert_eq!(
            result["resume_from"].as_u64(),
            Some(2),
            "the cursor names the next chunk: {}",
            result["resume_from"]
        );
        assert_eq!(
            result["coverage"]["planned_chunks"].as_u64(),
            Some(4),
            "plan-vs-observed reports the full plan"
        );
        assert_eq!(result["coverage"]["processed_chunks"].as_u64(), Some(2));

        let mut resume = semantic_pass_request(source.to_string_lossy().into_owned());
        resume.chunk_duration_secs = Some(0.5);
        resume.max_chunks = Some(2);
        resume.resume_from = Some(2);
        let result = content_of(&server.layers_semantic_pass(Parameters(resume)).await?);
        assert_eq!(
            result["resume_from"],
            serde_json::Value::Null,
            "the plan is complete after resuming"
        );
        assert_eq!(result["coverage"]["processed_chunks"].as_u64(), Some(2));
        Ok(())
    }

    /// Media without a video stream (audio-only) fails as a named
    /// degradation naming the requirement — never a raw ffmpeg error.
    #[tokio::test]
    async fn layers_semantic_pass_audio_only_is_a_named_degradation()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, store, _driver, gallery) =
            media_test_env().await?;
        let source = gallery_root.path().join("audio-only.wav");
        create_real_audio(&source).await?;
        let media_path = source.to_string_lossy().into_owned();
        let server = server_with_gallery(store.clone(), gallery.id, gallery_root.path());
        let error = server
            .layers_semantic_pass(Parameters(semantic_pass_request(media_path.clone())))
            .await
            .expect_err("audio-only media must fail the stream requirement");
        let message = error.to_string();
        assert!(
            message.contains("failed_precondition"),
            "classified failed_precondition: {message}"
        );
        assert!(
            message.contains("video stream"),
            "names the requirement: {message}"
        );
        let listed = content_of(
            &server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path.clone()),
                    kind: None,
                    limit: None,
                }))
                .await?,
        );
        assert_eq!(listed["count"].as_u64(), Some(0), "nothing is stored");
        Ok(())
    }

    /// A missing media file fails as not_found naming the path.
    #[tokio::test]
    async fn layers_semantic_pass_missing_media_is_not_found() {
        let server = make_server();
        let error = server
            .layers_semantic_pass(Parameters(semantic_pass_request(
                "/nonexistent/semantic-pass-probe.mp4".to_string(),
            )))
            .await
            .expect_err("missing media path fails");
        let message = error.to_string();
        assert!(
            message.contains("not_found"),
            "classified not_found: {message}"
        );
        assert!(
            message.contains("/nonexistent/semantic-pass-probe.mp4"),
            "names the missing path: {message}"
        );
    }

    /// Shot mode chunks over the stored shot layer's items, and a shot
    /// layer with no shots returns the t2-style no_shots note.
    #[tokio::test]
    async fn layers_semantic_pass_shot_mode_chunks_over_stored_shots_and_degrades_on_zero_shots()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, gallery_root, source, server) =
            vision_pass_env(canned_vision_response()).await?;
        let media_path = source.to_string_lossy().into_owned();
        // A real two-shot layer from the shot pass (the solid fixture has
        // one shot, so the plan comes from the hard-cut video). One env and
        // one store: ARTIFACTS_ENV_LOCK is a single global mutex and not
        // reentrant — a second media_test_env() here deadlocked the test
        // holding the lock forever — and the shot layer must live in the
        // store the semantic-pass server resolves shot_layer_id through.
        let hard_cut = gallery_root.path().join("hard-cut.mp4");
        create_real_hard_cut_video(&hard_cut).await?;
        let shot_layer = content_of(
            &server
                .layers_shot_pass(Parameters(crate::types::LayersShotPassRequest {
                    media_path: hard_cut.to_string_lossy().into_owned(),
                    gallery_asset_id: None,
                    threshold: None,
                    min_shot_length: None,
                }))
                .await?,
        );
        let shot_layer_id = shot_layer["layer"]["id"]
            .as_str()
            .ok_or("shot layer id")?
            .to_string();
        assert_eq!(shot_layer["layer"]["items_count"].as_u64(), Some(2));

        let mut request = semantic_pass_request(media_path.clone());
        request.time_range = None;
        request.shot_layer_id = Some(shot_layer_id.clone());
        let result = content_of(&server.layers_semantic_pass(Parameters(request)).await?);
        assert_eq!(result["status"], serde_json::json!("completed"));
        assert_eq!(
            result["coverage"]["chunking_mode"],
            serde_json::json!("shot_layer")
        );
        assert_eq!(
            result["coverage"]["processed_chunks"].as_u64(),
            Some(2),
            "each shot is one chunk"
        );

        // Zero shots in shot mode: the t2-style no_shots note, nothing stored.
        let stored = content_of(
            &server
                .layers_store(Parameters(crate::types::LayersStoreRequest {
                    media_path: media_path.clone(),
                    gallery_asset_id: None,
                    kind: crate::layer_store::MediaLayerKind::Shot,
                    label: Some("empty shot layer".to_string()),
                    provenance: crate::types::LayersProvenanceRequest {
                        model: "user".to_string(),
                        prompt_template: "researcher_annotation".to_string(),
                    },
                    items: vec![],
                }))
                .await?,
        );
        let empty_shot_layer_id = stored["id"].as_str().ok_or("stored layer id")?.to_string();
        let mut request = semantic_pass_request(media_path.clone());
        request.time_range = None;
        request.shot_layer_id = Some(empty_shot_layer_id.clone());
        let result = content_of(&server.layers_semantic_pass(Parameters(request)).await?);
        assert_eq!(result["status"], serde_json::json!("no_shots"));
        assert!(
            result["note"]
                .as_str()
                .is_some_and(|note| note.contains("no shots")),
            "the note names the degradation: {}",
            result["note"]
        );
        let listed = content_of(
            &server
                .layers_list(Parameters(crate::types::LayersListRequest {
                    media_path: Some(media_path),
                    kind: Some(crate::layer_store::MediaLayerKind::Semantic),
                    limit: None,
                }))
                .await?,
        );
        assert!(
            listed["count"].as_u64().is_some_and(|count| count >= 1),
            "the shot-mode pass stored its semantic layers"
        );
        Ok(())
    }

    /// Out-of-range boxes, confidences, and frame indices are named
    /// rejections of the whole pass — nothing is stored, never a silent
    /// clamp.
    #[tokio::test]
    async fn layers_semantic_pass_rejects_out_of_range_model_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let bad_outputs = [
            (
                "box outside 0..1",
                serde_json::json!({
                    "caption": {"text": "cap", "confidence": 0.5},
                    "objects": [{
                        "label": "bad box", "confidence": 0.5,
                        "box": {"x": 1.5, "y": 0.0, "w": 0.1, "h": 0.1},
                        "frame_index": 0
                    }]
                }),
                "outside 0..1",
            ),
            (
                "confidence outside 0..1",
                serde_json::json!({
                    "caption": {"text": "cap", "confidence": 1.7},
                }),
                "confidence 1.7 is outside 0..1",
            ),
            (
                "frame index outside attached frames",
                serde_json::json!({
                    "caption": {"text": "cap", "confidence": 0.5},
                    "ocr": [{
                        "text": "t", "confidence": 0.5,
                        "box": {"x": 0.0, "y": 0.0, "w": 0.1, "h": 0.1},
                        "frame_index": 99
                    }]
                }),
                "outside the attached frames",
            ),
        ];
        for (name, output, expected_fragment) in bad_outputs {
            let (_env_lock, _artifacts, _env, _gallery_root, source, server) =
                vision_pass_env(output.to_string()).await?;
            let error = server
                .layers_semantic_pass(Parameters(semantic_pass_request(
                    source.to_string_lossy().into_owned(),
                )))
                .await
                .expect_err(name);
            let message = error.to_string();
            assert!(
                message.contains("invalid_argument"),
                "{name} is a caller-shaped rejection: {message}"
            );
            assert!(
                message.contains(expected_fragment),
                "{name} names the bound: {message}"
            );
            let listed = content_of(
                &server
                    .layers_list(Parameters(crate::types::LayersListRequest {
                        media_path: Some(source.to_string_lossy().into_owned()),
                        kind: None,
                        limit: None,
                    }))
                    .await?,
            );
            assert_eq!(
                listed["count"].as_u64(),
                Some(0),
                "{name}: nothing is stored"
            );
        }
        Ok(())
    }

    /// Unparseable vision output surfaces as the pass core's typed error —
    /// the unparseable-output class, never a silent empty result.
    #[tokio::test]
    async fn layers_semantic_pass_unparseable_output_surfaces_typed()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_env_lock, _artifacts, _env, _gallery_root, source, server) =
            vision_pass_env("not-json {{{".to_string()).await?;
        let error = server
            .layers_semantic_pass(Parameters(semantic_pass_request(
                source.to_string_lossy().into_owned(),
            )))
            .await
            .expect_err("unparseable output must surface");
        let message = error.to_string();
        assert!(
            message.contains("unavailable"),
            "the unparseable-output class maps to unavailable (a model failure): {message}"
        );
        assert!(
            message.contains("SemanticPassOutput"),
            "names the expected shape: {message}"
        );
        assert!(
            message.contains("not-json"),
            "carries the raw model output: {message}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn gallery_refresh_before_init_returns_typed_error() {
        let server = make_server();
        let result = server
            .gallery_refresh(Parameters(GalleryRefreshRequest {
                recursive: false,
                include_faces: false,
                max_images: 10,
            }))
            .await;

        // The core wire pattern: a tool-logical error is a typed `Err` —
        // rmcp marks the wire result `is_error` and carries the kind in
        // `structured_content`. No in-band envelope to parse on the client.
        let error = result.expect_err("uninitialized gallery must yield a typed error");
        assert!(
            error.message.to_lowercase().contains("gallery"),
            "error should name the gallery state problem, got: {}",
            error.message
        );
        assert!(
            matches!(error.kind, hkask_types::McpErrorKind::FailedPrecondition),
            "gallery-not-initialized is a caller-fixable precondition (organize a gallery \
             first — the canonical classification, operator ruling 2026-10-05), got {:?}",
            error.kind
        );
    }

    /// `is_local_media_path` gates SSRF URL validation by locator syntax, not
    /// current filesystem existence. Missing local Assets must reach the
    /// filesystem/ffmpeg boundary so the visible error retains its real cause.
    #[test]
    fn is_local_media_path_preserves_missing_filesystem_causes() {
        let existing = std::env::temp_dir().join("is-local-media-path-test.txt");
        std::fs::write(&existing, b"fixture").expect("fixture write");
        let existing_str = existing.to_str().expect("utf-8 path");

        assert!(is_local_media_path(existing_str), "existing file path");
        assert!(
            is_local_media_path(&format!("file://{existing_str}")),
            "file:// URI over an existing file"
        );
        assert!(
            is_local_media_path("/nonexistent/path/audio.wav"),
            "missing absolute paths remain filesystem inputs"
        );
        assert!(
            is_local_media_path("file:///nonexistent/path/audio.wav"),
            "missing file:// targets remain filesystem inputs"
        );
        assert!(
            !is_local_media_path("https://example.com/audio.wav"),
            "network URLs are never local paths"
        );
        assert!(
            is_local_media_path("relative-audio.wav"),
            "scheme-less relative paths remain filesystem inputs"
        );
        assert!(!is_local_media_path("data:audio/wav;base64,AA=="));
    }
}

#[cfg(test)]
mod gallery_lifecycle_tests {
    use super::*;
    use crate::test_support::read_only_organize_request;
    use hkask_storage::database::sqlite::SqliteDriver;
    use rmcp::handler::server::wrapper::Parameters;
    use std::path::Path;
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn server(database: &Path, inference: Arc<dyn InferencePort>) -> MediaServer {
        unsafe { std::env::set_var(crate::models::VISION_ENV, "OpenRouter/test-vision") };
        let pool = SqliteDriver::file_pool(database.to_str().expect("UTF-8 fixture path"))
            .expect("unencrypted SQLite");
        let store =
            GalleryStore::from_driver(Arc::new(SqliteDriver::new(pool))).expect("gallery schema");
        MediaServer::new(
            hkask_types::WebID::new(),
            inference,
            Arc::new(Mutex::new(None)),
            Arc::new(store),
            templates::create_env().expect("templates"),
            FfmpegRunner::detect(),
            YtDlpRunner::detect(),
            jobs::new_job_store(),
            None,
            Default::default(),
        )
    }

    fn png(path: &Path, color: u8) {
        image::RgbImage::from_pixel(3, 2, image::Rgb([color, 0, 0]))
            .save(path)
            .expect("valid PNG fixture");
    }

    async fn organize(
        server: &MediaServer,
        root: &Path,
        recursive: bool,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let result = server
            .gallery_organize(Parameters(GalleryOrganizeRequest {
                path: root.to_string_lossy().into_owned(),
                mode: "read-only".into(),
                recursive,
                auto_analyze: false,
            }))
            .await?;
        let value: serde_json::Value = serde_json::from_str(&result)?;
        assert!(!result.contains("\"error\":"), "{result}");
        assert_eq!(value["content"]["gallery_changed"], true);
        Ok(value)
    }

    /// The transcript-linked audio-asset fixture shared by the deletion
    /// tests: a temp gallery organized in `mode`, one source.wav, one
    /// gallery asset row, and one stored transcript linked to the asset.
    /// Both deletion tests each carried this setup before the helper
    /// (2026-10-09 ratchet pass).
    struct TranscriptLinkedAsset {
        _fixture: tempfile::TempDir,
        source: std::path::PathBuf,
        server: MediaServer,
        asset: crate::gallery::ImageRecord,
        transcript: crate::transcript_store::TranscriptSummary,
    }

    async fn transcript_linked_audio_asset(
        mode: &str,
    ) -> Result<TranscriptLinkedAsset, Box<dyn std::error::Error>> {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("root");
        std::fs::create_dir(&root)?;
        let source = root.join("source.wav");
        std::fs::write(&source, b"source audio")?;
        let server = server(
            &fixture.path().join("gallery.sqlite"),
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        server
            .gallery_organize(Parameters(GalleryOrganizeRequest {
                path: root.to_string_lossy().into_owned(),
                mode: mode.into(),
                recursive: true,
                auto_analyze: false,
            }))
            .await?;
        let gallery = server.access_gallery()?;
        let asset = server.gallery_store.add_media(
            &gallery.gallery_id,
            source.to_str().ok_or("UTF-8 source path")?,
            "source-hash",
            0,
            0,
            "wav",
            12,
            "audio",
        )?;
        let bundle = crate::transcript::TranscriptBundle::new(
            source.to_string_lossy().into_owned(),
            1.0,
            "source".to_string(),
        );
        let transcript = crate::transcript_store::store_transcript(
            &**server.gallery_store.driver(),
            &bundle,
            Some(&asset.id),
        )?;
        Ok(TranscriptLinkedAsset {
            _fixture: fixture,
            source,
            server,
            asset,
            transcript,
        })
    }

    /// dcterms:identifier: `MediaServer::gallery_delete_image`
    /// expect: Gallery policy preserves originals unless destructive mode permits deletion; transcripts survive success, refusal, and partial failure.
    /// [P1] Motivating: Catalog cleanup respects the user's preservation policy.
    #[tokio::test]
    async fn gallery_deletion_mode_matrix() -> TestResult {
        for (mode, delete_file, fail_index) in [
            ("read-only", false, false),
            ("read-only", true, false),
            ("copy-on-write", false, false),
            ("copy-on-write", true, false),
            ("destructive", false, false),
            ("destructive", true, false),
            ("destructive", true, true),
        ] {
            let TranscriptLinkedAsset {
                _fixture,
                source,
                server,
                asset,
                transcript,
            } = transcript_linked_audio_asset(mode).await?;

            if fail_index {
                server.gallery_store.driver().execute_batch(
                    "CREATE TRIGGER refuse_delete BEFORE DELETE ON gallery_images
                 BEGIN SELECT RAISE(ABORT, 'injected index failure'); END;",
                )?;
            }
            let result = server
                .gallery_delete_image(Parameters(GalleryDeleteImageRequest {
                    image_index: None,
                    image_id: Some(asset.id.clone()),
                    delete_file,
                }))
                .await;

            let denied = delete_file && mode != "destructive";
            let index_preserved = denied || fail_index;
            if denied {
                let error = result.expect_err("preserving mode must refuse file deletion");
                assert!(error.message.contains("destructive"), "{error}");
            } else if fail_index {
                let error = result.expect_err("injected index failure");
                assert!(error.message.contains("source file was deleted"), "{error}");
                assert!(error.message.contains("injected index failure"), "{error}");
            } else {
                let value: serde_json::Value = serde_json::from_str(&result?)?;
                assert_eq!(value["content"]["transcripts_detached"], 1);
                assert_eq!(value["content"]["file_deleted"], delete_file);
            }
            let source_preserved = denied || !delete_file;
            assert_eq!(source.exists(), source_preserved, "{mode}, {delete_file}");
            if source_preserved {
                assert_eq!(std::fs::read(&source)?, b"source audio");
            }
            assert_eq!(
                server.gallery_store.get_by_id(&asset.id).is_ok(),
                index_preserved
            );
            let (summary, _) = crate::transcript_store::load_transcript(
                &**server.gallery_store.driver(),
                &transcript.id,
            )?
            .ok_or("transcript preserved")?;
            assert_eq!(
                summary.gallery_asset_id.as_deref(),
                index_preserved.then_some(asset.id.as_str())
            );
            assert_eq!(
                summary.source_link,
                if index_preserved {
                    crate::transcript_store::TranscriptSourceLink::Linked
                } else {
                    crate::transcript_store::TranscriptSourceLink::Detached
                }
            );
            assert_eq!(
                summary.source_availability,
                if source_preserved {
                    crate::transcript_store::TranscriptSourceAvailability::Available
                } else {
                    crate::transcript_store::TranscriptSourceAvailability::Missing
                }
            );
        }
        Ok(())
    }

    /// dcterms:identifier: `MediaServer::gallery_delete_image`
    /// expect: If source-file deletion fails, I see the filesystem cause and no durable identity is falsely removed.
    /// [P1] Motivating: Destructive operations fail closed and never report cleanup they did not perform.
    #[tokio::test]
    async fn asset_file_deletion_failure_preserves_gallery_and_transcript_identity() -> TestResult {
        let TranscriptLinkedAsset {
            _fixture,
            source,
            server,
            asset,
            transcript,
        } = transcript_linked_audio_asset("destructive").await?;
        std::fs::remove_file(&source)?;
        std::fs::create_dir(&source)?;

        let error = server
            .gallery_delete_image(Parameters(GalleryDeleteImageRequest {
                image_index: None,
                image_id: Some(asset.id.clone()),
                delete_file: true,
            }))
            .await
            .expect_err("directory cannot be removed as a source file");

        assert!(error.message.contains("delete source file"), "{error}");
        assert!(error.message.contains("source.wav"), "{error}");
        assert!(
            source.is_dir(),
            "failed unlink must leave the source path intact"
        );
        assert!(server.gallery_store.get_by_id(&asset.id).is_ok());
        let (summary, _) = crate::transcript_store::load_transcript(
            &**server.gallery_store.driver(),
            &transcript.id,
        )?
        .ok_or("transcript preserved")?;
        assert_eq!(summary.gallery_asset_id.as_deref(), Some(asset.id.as_str()));
        assert_eq!(
            summary.source_link,
            crate::transcript_store::TranscriptSourceLink::Linked
        );
        assert_eq!(
            summary.source_availability,
            crate::transcript_store::TranscriptSourceAvailability::Missing
        );
        Ok(())
    }

    /// A fresh fixture with two created subdirectories `first` and `second` —
    /// the shared setup of the two-root gallery tests, which vary in what
    /// they put inside the roots and assert after. The fixture is returned
    /// (not dropped) so its registrations outlive the test body.
    fn two_created_dir_roots()
    -> std::io::Result<(tempfile::TempDir, std::path::PathBuf, std::path::PathBuf)> {
        let fixture = tempfile::tempdir()?;
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        std::fs::create_dir(&first)?;
        std::fs::create_dir(&second)?;
        Ok((fixture, first, second))
    }

    /// expect: My gallery and mode survive restart, aliases, and switches; invalid roots never activate. [P1]
    #[tokio::test]
    async fn reopen_two_roots_aliases_and_failure_preserves_activation() -> TestResult {
        let (fixture, first, second) = two_created_dir_roots()?;
        png(&first.join("first.png"), 1);
        png(&second.join("second.png"), 2);
        let database = fixture.path().join("gallery.sqlite");
        let original;
        {
            let server = server(
                &database,
                Arc::new(BarrierVision::barrier(analysis_vision_text)),
            );
            organize(&server, &first, true).await?;
            original = server.access_gallery()?.gallery_id;
            organize(&server, &second, true).await?;
            assert_ne!(server.access_gallery()?.gallery_id, original);
        }
        let server = server(
            &database,
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        assert!(
            server.access_gallery().is_err(),
            "no automatic startup activation"
        );
        organize(&server, &first.join("../first/."), true).await?;
        assert_eq!(server.access_gallery()?.gallery_id, original);
        let result = server
            .gallery_organize(Parameters(GalleryOrganizeRequest {
                path: first.to_string_lossy().into_owned(),
                mode: "destructive".into(),
                recursive: true,
                auto_analyze: false,
            }))
            .await?;
        assert!(result.contains("\"mode_preserved\":true"), "{result}");
        assert_eq!(server.access_gallery()?.mode, "read-only");
        let invalid = fixture.path().join("absent");
        assert!(
            server
                .gallery_organize(read_only_organize_request(&invalid))
                .await
                .is_err()
        );
        assert_eq!(server.access_gallery()?.gallery_id, original);
        let rows = server
            .gallery_store
            .driver()
            .query("SELECT root_path FROM galleries", &[])?;
        assert_eq!(rows.len(), 2);
        #[cfg(unix)]
        {
            let alias = fixture.path().join("alias");
            std::os::unix::fs::symlink(&first, &alias)?;
            organize(&server, &alias, true).await?;
            assert_eq!(server.access_gallery()?.gallery_id, original);
        }
        Ok(())
    }

    /// expect: Missing files retain tags, albums and lineage; reappearance restores the same ID. [P1]
    #[tokio::test]
    async fn missing_return_changed_and_partial_coverage() -> TestResult {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("root");
        std::fs::create_dir(&root)?;
        let path = root.join("one.png");
        png(&path, 1);
        let original_bytes = std::fs::read(&path)?;
        let server = server(
            &fixture.path().join("gallery.sqlite"),
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        organize(&server, &root, true).await?;
        let gallery = server.access_gallery()?;
        let original = server
            .gallery_store
            .get_image(&gallery.gallery_id, Some(0), None)?;
        let store = &server.gallery_store;
        store.tag_image(&original.id, "caption", "My photo", 1.0, "user")?;
        let album = store.create_album(&gallery.gallery_id, "Keep", None)?;
        store.add_to_album(&album.id, &original.id)?;
        store.record_generation(
            &original.id,
            "generate_image",
            Some("original"),
            None,
            None,
            None,
            None,
            None,
            None,
        )?;
        organize(&server, &root, true).await?;
        assert_eq!(std::fs::read(&path)?, original_bytes);
        assert_eq!(store.count_assets(&gallery.gallery_id)?, 1);
        std::fs::remove_file(&path)?;
        organize(&server, &root, true).await?;
        assert_eq!(store.count_assets(&gallery.gallery_id)?, 0);
        assert!(store.list_assets(&gallery.gallery_id, 0, 10)?.is_empty());
        assert!(store.get_all_tags(&gallery.gallery_id)?.is_empty());
        assert!(store.list_album_members(&album.id)?.is_empty());
        assert!(store.get_by_id(&original.id)?.missing);
        let detail = server
            .gallery_asset_detail(Parameters(GalleryAssetDetailRequest {
                image_index: None,
                image_id: Some(original.id.clone()),
            }))
            .await?;
        assert!(detail.contains("\"missing\":true"), "{detail}");
        assert_eq!(store.get_tags(&original.id)?.len(), 1);
        assert!(store.get_generation(&original.id)?.is_some());
        std::fs::write(&path, &original_bytes)?;
        organize(&server, &root, true).await?;
        let returned = store.get_image(&gallery.gallery_id, Some(0), None)?;
        assert_eq!(returned.id, original.id);
        assert_eq!(returned.added_at, original.added_at);
        assert!(!returned.missing);
        assert_eq!(returned.metadata_stale, original.metadata_stale);
        assert_eq!(
            store.list_album_members(&album.id)?,
            vec![original.id.clone()]
        );
        png(&path, 2);
        organize(&server, &root, true).await?;
        let changed = store.get_by_id(&original.id)?;
        assert!(changed.metadata_stale);
        assert_ne!(changed.hash, original.hash);
        assert_eq!(store.get_tags(&original.id)?.len(), 1);
        assert!(!store.persist_analysis_for_tag_types(
            &original,
            &[("caption".into(), "wrong revision".into(), 1.0)],
            &["caption".to_string()],
            "fake",
            true
        )?);
        assert!(store.get_by_id(&original.id)?.metadata_stale);
        assert!(store.persist_analysis_for_tag_types(
            &changed,
            &[("caption".into(), "new revision".into(), 1.0)],
            &["caption".to_string()],
            "fake",
            true
        )?);
        assert!(!store.get_by_id(&original.id)?.metadata_stale);
        // Bad decode makes the entire scan conservative, even for another absent file.
        std::fs::remove_file(&path)?;
        std::fs::write(root.join("broken.png"), b"broken")?;
        let result = organize(&server, &root, true).await?;
        assert!(result.to_string().contains("degraded"));
        assert!(!store.get_by_id(&original.id)?.missing);
        // An unreadable/nonexistent root on refresh reports coverage, never absence.
        std::fs::rename(&root, fixture.path().join("offline"))?;
        let (scan, result) = server.rescan_gallery(&gallery, true)?;
        assert!(!scan.errors.is_empty());
        assert_eq!(result.missing, 0);
        Ok(())
    }

    /// expect: Nonrecursive scans do not hide nested images, external imports, or audio/video. [P1]
    #[tokio::test]
    async fn scope_and_identical_copies_keep_distinct_path_identities() -> TestResult {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("root");
        std::fs::create_dir_all(root.join("nested"))?;
        png(&root.join("a.png"), 1);
        png(&root.join("b.png"), 1);
        png(&root.join("nested/c.png"), 1);
        let server = server(
            &fixture.path().join("gallery.sqlite"),
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        organize(&server, &root, true).await?;
        let gallery = server.access_gallery()?;
        let store = &server.gallery_store;
        let copies = store.list_assets(&gallery.gallery_id, 0, 10)?;
        assert_eq!(copies.len(), 3);
        assert_ne!(copies[0].id, copies[1].id);
        assert_eq!(copies[0].hash, copies[1].hash);
        let external = fixture.path().join("external.png");
        png(&external, 2);
        let (external_id, _) = server.import_reference_image(&external, fixture.path())?;
        for (name, kind) in [("film.mp4", "video"), ("sound.wav", "audio")] {
            let path = root.join(name);
            std::fs::write(&path, b"fixture bytes")?;
            let response = server
                .gallery_add_media(Parameters(GalleryAddMediaRequest {
                    path: path.to_string_lossy().into_owned(),
                    media_type: kind.into(),
                    width: None,
                    height: None,
                }))
                .await?;
            let response: serde_json::Value = serde_json::from_str(&response)?;
            let payload = hkask_types::tool_response::unwrap_tool_envelope(response);
            let asset_id = payload["id"]
                .as_str()
                .ok_or("gallery_add_media response omitted Asset id")?;
            let hint = payload["display_hint"]
                .as_str()
                .ok_or("gallery_add_media response omitted display_hint")?;
            let body = hint
                .trim()
                .strip_prefix("```media")
                .and_then(|body| body.trim().strip_suffix("```"))
                .ok_or("gallery_add_media display_hint was not a media block")?;
            let body: serde_json::Value = serde_json::from_str(body)?;
            assert_eq!(body["gallery_asset_id"], asset_id);
        }
        std::fs::remove_file(root.join("nested/c.png"))?;
        organize(&server, &root, false).await?;
        assert_eq!(store.count_assets(&gallery.gallery_id)?, 5);
        organize(&server, &root, true).await?;
        assert_eq!(store.count_assets(&gallery.gallery_id)?, 4);
        // A file outside the active gallery belongs to its own folder's gallery.
        let home = store
            .containing(&external.canonicalize()?)?
            .ok_or("external home gallery")?;
        assert_ne!(home.id, gallery.gallery_id);
        assert!(!store.get_by_id(&external_id)?.missing);
        let active = store.list_assets(&gallery.gallery_id, 0, 10)?;
        for (index, asset) in active.iter().enumerate() {
            assert_eq!(
                store.get_image(&gallery.gallery_id, Some(index), None)?.id,
                asset.id
            );
            assert_eq!(
                store.list_assets(&gallery.gallery_id, index, 1)?[0].id,
                asset.id
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&external, root.join("escape.png"))?;
            std::fs::remove_file(root.join("a.png"))?;
            let (scan, result) = server.rescan_gallery(&gallery, true)?;
            assert!(scan.errors.iter().any(|error| error.contains("Symlink")));
            assert_eq!(result.missing, 0);
        }
        Ok(())
    }

    /// expect: Importing a file files it under the gallery whose folder
    /// contains it, whichever gallery happens to be active. [P1]
    #[tokio::test]
    async fn add_media_files_under_the_containing_gallery() -> TestResult {
        let fixture = tempfile::tempdir()?;
        let owner = fixture.path().join("owner");
        let active = fixture.path().join("active");
        std::fs::create_dir_all(&owner)?;
        std::fs::create_dir_all(&active)?;
        let server = server(
            &fixture.path().join("gallery.sqlite"),
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        organize(&server, &owner, false).await?;
        let owner_id = server.access_gallery()?.gallery_id;
        organize(&server, &active, false).await?;
        let active_id = server.access_gallery()?.gallery_id;
        let film = owner.join("film.mp4");
        std::fs::write(&film, b"fixture bytes")?;
        server
            .gallery_add_media(Parameters(GalleryAddMediaRequest {
                path: film.to_string_lossy().into_owned(),
                media_type: "video".into(),
                width: None,
                height: None,
            }))
            .await?;
        assert_eq!(server.gallery_store.count_assets(&owner_id)?, 1);
        assert_eq!(server.gallery_store.count_assets(&active_id)?, 0);
        Ok(())
    }

    /// expect: A failed reconciliation rolls back physical changes and missing transitions together. [P1]
    #[tokio::test]
    async fn reconciliation_rolls_back_on_injected_insert_failure() -> TestResult {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("root");
        std::fs::create_dir(&root)?;
        png(&root.join("a.png"), 1);
        png(&root.join("b.png"), 2);
        let server = server(
            &fixture.path().join("gallery.sqlite"),
            Arc::new(BarrierVision::barrier(analysis_vision_text)),
        );
        organize(&server, &root, true).await?;
        let gallery = server.access_gallery()?;
        let before = server
            .gallery_store
            .list_assets(&gallery.gallery_id, 0, 10)?;
        png(&root.join("a.png"), 3);
        std::fs::remove_file(root.join("b.png"))?;
        png(&root.join("z.png"), 4);
        server.gallery_store.driver().execute_batch("CREATE TRIGGER fail_gallery_insert BEFORE INSERT ON gallery_images WHEN NEW.relative_path = 'z.png' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;")?;
        assert!(server.rescan_gallery(&gallery, true).is_err());
        let after = server
            .gallery_store
            .list_assets(&gallery.gallery_id, 0, 10)?;
        assert_eq!(serde_json::to_value(before)?, serde_json::to_value(after)?);
        Ok(())
    }

    /// The shared vision test double: `vision_response` supplies the
    /// per-test `generate_vision` text. The `barrier` variant parks the
    /// first call on `entered`/`resume` so a test can switch gallery state
    /// mid-analysis.
    struct StubVisionPort {
        entered: tokio::sync::Notify,
        resume: tokio::sync::Notify,
        calls: std::sync::atomic::AtomicUsize,
        barrier: bool,
        vision_response: fn(&str) -> String,
    }

    impl StubVisionPort {
        fn new(vision_response: fn(&str) -> String) -> Self {
            Self {
                entered: tokio::sync::Notify::new(),
                resume: tokio::sync::Notify::new(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                barrier: false,
                vision_response,
            }
        }

        /// The first-call barrier variant — `BarrierVision` at the call sites.
        fn barrier(vision_response: fn(&str) -> String) -> Self {
            Self {
                barrier: true,
                ..Self::new(vision_response)
            }
        }
    }

    /// The barrier vision port: parks the first analysis call so a test can
    /// switch gallery state mid-inference.
    type BarrierVision = StubVisionPort;

    /// Well-formed analysis responses: empty detections, a neutral palette,
    /// a centered composition, and a plain caption.
    fn analysis_vision_text(prompt: &str) -> String {
        if prompt.contains("Return ONLY a JSON array") {
            "[]".to_string()
        } else if prompt.contains("color palette") && prompt.contains("JSON object") {
            r#"{"colors":[],"palette_style":"neutral","temperature":"balanced","saturation":"muted"}"#.to_string()
        } else if prompt.contains("JSON object") {
            r#"{"focal_point":"center","rule_of_thirds":"centered","leading_lines":"none","depth_of_field":"deep","perspective":"eye level","framing":"none","symmetry":"balanced","negative_space":"none"}"#.to_string()
        } else {
            "Test caption".to_string()
        }
    }

    /// Structurally invalid analysis responses: a missing colors field, an
    /// empty composition object, and a blank caption — while the empty
    /// object list is a legitimate detection result.
    fn invalid_analysis_vision_text(prompt: &str) -> String {
        if prompt.contains("Return ONLY a JSON array") {
            "[]".to_string()
        } else if prompt.contains("color palette") && prompt.contains("JSON object") {
            "{}".to_string()
        } else if prompt.contains("JSON object") {
            "{}".to_string()
        } else {
            "   ".to_string()
        }
    }

    crate::test_support::media_inference_stub! {
        StubVisionPort,
        generate(self, _prompt): {
            Box::pin(async {
                Err(hkask_types::InferenceError::NotConfigured(
                    "test only supports vision".into(),
                ))
            })
        },
        generate_vision(self, prompt, images): {
            assert!(!images.is_empty());
            let text = (self.vision_response)(prompt);
            Box::pin(async move {
                if self.barrier && self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
                {
                    self.entered.notify_one();
                    self.resume.notified().await;
                }
                Ok(hkask_types::InferenceResult {
                    text,
                    model: "test-vision".into(),
                    usage: Default::default(),
                    finish_reason: "stop".into(),
                    tool_calls: vec![],
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    /// expect: Switching galleries during vision never tags the new root or retargets later indices. [P1]
    #[tokio::test]
    async fn analysis_is_bound_before_await_and_rejects_changed_revision() -> TestResult {
        let (fixture, first, second) = two_created_dir_roots()?;
        for name in ["a.png", "b.png"] {
            png(&first.join(name), 1);
            png(&second.join(name), 2);
        }
        let vision = Arc::new(BarrierVision::barrier(analysis_vision_text));
        let server = Arc::new(server(
            &fixture.path().join("gallery.sqlite"),
            vision.clone(),
        ));
        organize(&server, &first, true).await?;
        let first_gallery = server.access_gallery()?;
        let records = server
            .gallery_store
            .list_assets(&first_gallery.gallery_id, 0, 10)?;
        let task = {
            let server = server.clone();
            tokio::spawn(async move {
                server
                    .run_analysis_on_indices(&first_gallery, &[0, 1], &["scene".into()])
                    .await
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), vision.entered.notified()).await?;
        organize(&server, &second, true).await?;
        let second_gallery = server.access_gallery()?;
        vision.resume.notify_one();
        let (count, errors) = task.await?;
        assert_eq!(count, 2, "{errors:?}");
        assert!(errors.is_empty());
        for record in &records {
            assert_eq!(server.gallery_store.get_tags(&record.id)?.len(), 1);
        }
        assert!(
            server
                .gallery_store
                .get_all_tags(&second_gallery.gallery_id)?
                .is_empty()
        );

        let vision = Arc::new(BarrierVision::barrier(analysis_vision_text));
        let changed_server = Arc::new(self::server(
            &fixture.path().join("gallery.sqlite"),
            vision.clone(),
        ));
        organize(&changed_server, &first, true).await?;
        let first_gallery = changed_server.access_gallery()?;
        let task = {
            let server = changed_server.clone();
            tokio::spawn(async move {
                server
                    .run_analysis_on_indices(&first_gallery, &[0], &["scene".into()])
                    .await
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), vision.entered.notified()).await?;
        png(Path::new(&records[0].absolute_path), 9);
        organize(&changed_server, &first, true).await?;
        vision.resume.notify_one();
        let (count, errors) = task.await?;
        assert_eq!(count, 0);
        assert!(!errors.is_empty());
        assert!(
            server
                .gallery_store
                .get_by_id(&records[0].id)?
                .metadata_stale
        );
        Ok(())
    }
    /// The stale-image analysis fixture shared by the reanalysis tests:
    /// one png organized, replaced (making its metadata stale), and
    /// re-organized — then a full four-pipeline analysis on index 0,
    /// returning the server, the stale image record, and the analysis
    /// (count, errors). Both tests carried this setup before the helper
    /// (2026-10-09 ratchet pass); the vision port is the varying input.
    async fn stale_image_analysis(
        vision: Arc<dyn InferencePort>,
    ) -> Result<
        (
            tempfile::TempDir,
            MediaServer,
            crate::gallery::ImageRecord,
            (u32, Vec<String>),
        ),
        Box<dyn std::error::Error>,
    > {
        let fixture = tempfile::tempdir()?;
        let root = fixture.path().join("root");
        std::fs::create_dir(&root)?;
        png(&root.join("a.png"), 1);
        let server = server(&fixture.path().join("gallery.sqlite"), vision);
        organize(&server, &root, true).await?;
        png(&root.join("a.png"), 2);
        organize(&server, &root, true).await?;
        let gallery = server.access_gallery()?;
        let image = server
            .gallery_store
            .get_image(&gallery.gallery_id, Some(0), None)?;
        assert!(image.metadata_stale);
        let (count, errors) = server
            .run_analysis_on_indices(
                &gallery,
                &[0],
                &[
                    "objects".into(),
                    "colors".into(),
                    "composition".into(),
                    "scene".into(),
                ],
            )
            .await;
        Ok((fixture, server, image, (count, errors)))
    }

    /// expect: Successful complete reanalysis clears staleness only for the analyzed revision. [P1]
    #[tokio::test]
    async fn complete_reanalysis_refreshes_matching_revision() -> TestResult {
        let vision = Arc::new(BarrierVision::barrier(analysis_vision_text));
        vision.resume.notify_one();
        let (_fixture, server, image, (count, errors)) = stale_image_analysis(vision).await?;
        assert_eq!(count, 1, "{errors:?}");
        assert!(errors.is_empty());
        assert!(!server.gallery_store.get_by_id(&image.id)?.metadata_stale);
        assert!(
            server
                .gallery_store
                .get_tags(&image.id)?
                .iter()
                .any(|tag| tag.value == "Test caption")
        );
        Ok(())
    }

    /// expect: Structurally invalid analysis output never certifies freshness. [P1]
    #[tokio::test]
    async fn invalid_analysis_outputs_retain_staleness() -> TestResult {
        let (_fixture, server, image, (count, errors)) =
            stale_image_analysis(Arc::new(StubVisionPort::new(invalid_analysis_vision_text)))
                .await?;
        assert_eq!(count, 0, "nothing was certified");
        assert!(!errors.is_empty(), "invalid shapes must surface as errors");
        assert!(
            errors.iter().any(|error| error.contains("no colors array")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("none of the recognized fields")),
            "{errors:?}"
        );
        assert!(
            errors.iter().any(|error| error.contains("blank")),
            "{errors:?}"
        );
        // Empty face/object detections are legitimate — they must not error.
        assert!(
            !errors
                .iter()
                .any(|error| error.contains("object detection")),
            "{errors:?}"
        );
        assert!(server.gallery_store.get_by_id(&image.id)?.metadata_stale);
        Ok(())
    }
}
