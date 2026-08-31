#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
//! hKask MCP Media — AI media generation (image, video, voice via centralized inference router)
//!
//! Tool families:
//! - Gallery: organize, search, status
//! - Image: describe, remove_background, apply_style, create_collage
//! - Video: clip, to_gif, image_to_video, add_caption, remix, concat, from_images
//! - Generation: generate_image, transform_image, upscale_image, generate_video
//! - Voice: voice_design, generate_speech
//! - Audio: transcribe, transcribe_bundle, audio_capture, record_and_transcribe

mod assets;
mod error;
mod faces;
mod gallery;
mod images;
pub mod jobs;
pub mod media_block;
pub mod omc;
mod templates;
pub mod video;

pub use error::{
    MediaError, classify_embedding_error, classify_inference_error, map_gallery_store_error,
    map_image_open_error, map_media_error,
};

// Bridge crates: shared ontological vocabulary (P5.4 dual-axis framework)

use gallery::GalleryState;
use gallery::vision::{self};
use hkask_mcp_server::server::{McpToolError, execute_tool, validate_tool_url_with_dns};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_storage::{GalleryMode, GalleryStore, GalleryStoreError};
use hkask_types::InferencePort;
use hkask_types::VoiceDesign;

use crate::transcript::{TimedWord, TranscriptBundle, TranscriptSegment};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
pub mod text;
pub mod tools;

// Extracted implementation modules (C2 split). Re-exported so the
// `use crate::*` imports in tools/ keep resolving without churn.
pub(crate) use assets::persist_generated_asset;
pub(crate) use faces::default_face_folder;
pub(crate) use text::{draw_text_mut, load_meme_font, measure_text};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Maximum image file size for base64 encoding (32 MiB). Gallery images larger
/// than this are rejected to prevent OOM — the image is read into memory and
/// base64-encoded, which triples the size. A multi-GB image would exhaust the
/// process's address space.
const MAX_IMAGE_READ_BYTES: u64 = 32 * 1024 * 1024;
use video::FfmpegRunner;
use video::YtDlpRunner;

// ── Model configuration ───────────────────────────────────────────────

/// Default open-weight models for media processing.
/// All can be overridden via environment variables.
///
/// The default values are `const` references to the single source of truth in
/// `hkask_inference::model_constants` — do not duplicate the model ids here.
pub mod models {
    /// Default TTS model: Kokoro-82M via DeepInfra
    pub const TTS_DEFAULT: &str = hkask_inference::model_constants::DEFAULT_TTS_MODEL;
    pub const TTS_ENV: &str = "HKASK_MEDIA_TTS_MODEL";

    /// Default STT model: Whisper Large v3 via DeepInfra
    pub const STT_DEFAULT: &str = hkask_inference::model_constants::DEFAULT_STT_MODEL;
    pub const STT_ENV: &str = "HKASK_MEDIA_STT_MODEL";

    /// Default transcript-pass model (classifier tier) for Educt LLM layers
    pub const PASS_DEFAULT: &str = hkask_inference::model_constants::DEFAULT_CLASSIFIER_MODEL;
    pub const PASS_ENV: &str = "HKASK_MEDIA_PASS_MODEL";

    /// Default audio-input chat model (the speaker pass's primary source)
    pub const AUDIO_CHAT_DEFAULT: &str = hkask_inference::model_constants::DEFAULT_AUDIO_CHAT_MODEL;
    pub const AUDIO_CHAT_ENV: &str = "HKASK_MEDIA_AUDIO_CHAT_MODEL";

    /// Default structured-outputs pass model (the v2 spike's opt-in mode)
    pub const STRUCTURED_PASS_DEFAULT: &str =
        hkask_inference::model_constants::DEFAULT_STRUCTURED_PASS_MODEL;
    pub const STRUCTURED_PASS_ENV: &str = "HKASK_MEDIA_STRUCTURED_PASS_MODEL";

    /// Default vision model: Qwen3-VL (Apache 2.0) via OpenRouter
    pub const VISION_DEFAULT: &str = hkask_inference::model_constants::DEFAULT_VISION_MODEL;
    pub const VISION_ENV: &str = "HKASK_MEDIA_VISION_MODEL";

    /// Default image generation model: FLUX-2-klein-4B via DeepInfra
    pub const IMAGE_GEN_DEFAULT: &str = hkask_inference::model_constants::DEFAULT_IMAGE_GEN_MODEL;
    pub const IMAGE_GEN_ENV: &str = "HKASK_MEDIA_IMAGE_GEN_MODEL";

    /// Resolve a model name from env var or default.
    pub fn resolve(env_key: &str, default: &str) -> String {
        std::env::var(env_key).unwrap_or_else(|_| default.to_string())
    }

    pub fn tts_model() -> String {
        resolve(TTS_ENV, TTS_DEFAULT)
    }
    pub fn stt_model() -> String {
        resolve(STT_ENV, STT_DEFAULT)
    }
    pub fn vision_model() -> String {
        resolve(VISION_ENV, VISION_DEFAULT)
    }
    pub fn image_gen_model() -> String {
        resolve(IMAGE_GEN_ENV, IMAGE_GEN_DEFAULT)
    }
}

/// Lock-free snapshot of gallery state — safe to hold across .await points.
struct GalleryAccess {
    gallery_id: String,
    image_count: u64,
    root_path: PathBuf,
}

hkask_mcp_server::mcp_server!(
    pub struct MediaServer {
        /// Inference port for vision/chat/embed calls routed through zed's
        /// LanguageModelRegistry via the IPC bridge, and for media generation
        /// handled child-locally: `media_generate`
        /// (image/video/speech/transcription) dispatches to the local
        /// `MediaRouter` with this process's env-injected keys — no IPC
        /// round-trip. `generate_vision` handles
        /// face/object/color/composition/caption analysis; `embed` and
        /// `list_vision_models` handle the gallery embedding and
        /// model-resolution paths.
        pub vision_port: Arc<dyn InferencePort>,
        pub gallery_state: Arc<Mutex<Option<GalleryState>>>,
        pub gallery_store: Arc<GalleryStore>,
        pub template_env: minijinja::Environment<'static>,
        pub ffmpeg: FfmpegRunner,
        /// yt-dlp runner for video downloading (video_fetch tool).
        pub ytdlp: YtDlpRunner,
        /// In-memory generation job store for async job tracking (OMC `Task`).
        pub job_store: jobs::JobStore,
    }
);

mod style;
pub mod transcript;
pub mod transcript_layers;
pub mod transcript_pass;
pub mod transcript_select;
pub mod transcript_store;
pub mod types;
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
    /// Lock the gallery and extract essential state. Drops the lock before
    /// returning, so the result is safe to hold across .await points.
    fn access_gallery(&self) -> Result<GalleryAccess, MediaError> {
        let guard = self
            .gallery_state
            .lock()
            .map_err(|e| MediaError::Io(format!("Gallery state lock error: {}", e)))?;
        let state = guard.as_ref().ok_or(MediaError::GalleryNotInitialized)?;
        let access = GalleryAccess {
            gallery_id: state
                .gallery_id
                .clone()
                .ok_or(MediaError::GalleryNotInitialized)?,
            image_count: state.image_count,
            root_path: state.path.clone(),
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

    /// Return the best available vision model or an error if none is configured.
    async fn require_vision(&self) -> Result<(&'static str, &'static str), McpToolError> {
        self.resolve_vision_model().await.ok_or_else(|| {
            McpToolError::permission_denied(
                "No vision-capable provider configured. Vision LLMs route through zed's \
                 LanguageModelRegistry via the inference IPC bridge — enable a vision \
                 model in the kask inference provider settings.",
            )
        })
    }

    /// Embed a single text via the inference port's `embed` method.
    ///
    /// Resolves the embedding model from `HKASK_EMBEDDING_MODEL` (default
    /// `ollama/nomic-embed-text`) and returns the first (only)
    /// embedding vector. Used by gallery similarity search.
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, McpToolError> {
        let model = hkask_inference::model_constants::embedding_model();
        let vectors = self
            .vision_port
            .embed(&model, std::slice::from_ref(&text.to_string()))
            .await
            .map_err(|e| {
                classify_embedding_error(
                    "Embedding model unavailable. Configure a cloud provider",
                    e,
                )
            })?;
        vectors
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
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::gallery_router()
            + Self::processing_router()
            + Self::audio_router()
            + Self::generation_router()
            + Self::models_router()
            + Self::jobs_router()
            + Self::workflows_router()
            + Self::educt_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for MediaServer {}

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // Pins the registered tool-surface count end-to-end. Catches silent
    // registration drops — a `#[tool]` impl block without `#[tool_router]`, or
    // a sub-router missing from `combined_router()`, silently registers nothing
    // (`cargo check` passes on an unwired orphan). Mirrors the swarm pin.
    #[test]
    fn tool_surface_is_exactly_81_registered_tools() {
        let n = MediaServer::combined_router().list_all().len();
        assert_eq!(n, 81, "media registered tool surface changed; got {n}");
    }

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
        let creative = omc::tool_to_omc("generate_image");
        let version = omc::tool_to_omc("transform_image");
        let scene = omc::tool_to_omc("describe_image");
        let asset = omc::tool_to_omc("gallery_organize");
        let source = omc::tool_to_omc("generate_speech");
        let sequence = omc::tool_to_omc("video_clip");
        let shot = omc::tool_to_omc("video_extract_frames");
        let task = omc::tool_to_omc("gallery_record_generation");
        let participant = omc::tool_to_omc("model_list");
        // Nine distinct concepts across nine tool families.
        let concepts = [
            creative,
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
        assert_eq!(creative, Some(omc::CREATIVE_WORK));
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
    // Do NOT call `dotenvy::dotenv()` here — it mutates the process
    // environment via `set_var`, which contradicts the `load_dotenv()`
    // design. `run_server` calls `load_dotenv` internally.

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
            Ok(MediaServer::new(
                ctx.webid,
                vision_port.clone(),
                Arc::new(Mutex::new(None)),
                gallery_store.clone(),
                template_env,
                FfmpegRunner::detect(),
                YtDlpRunner::detect(),
                jobs::new_job_store(),
            ))
        },
        vec![hkask_mcp_server::CredentialRequirement::optional(
            "OPENROUTER_API_KEY",
            "OpenRouter API key for vision LLMs",
        )],
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
    use hkask_storage::{GalleryMode, GalleryStore};
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
            .create(
                &temp.path().to_string_lossy(),
                hkask_storage::GalleryMode::ReadOnly,
            )
            .expect("create gallery");
        assert_eq!(gallery.image_count, 0);

        let mut state = GalleryState::new(temp.path().to_path_buf(), GalleryMode::ReadOnly);
        let scan = state.scan(false, None);
        assert_eq!(scan.added, 3);

        for entry in &scan.entries {
            store
                .add_image(
                    &gallery.id,
                    &entry.relative_path,
                    &temp.path().join(&entry.relative_path).to_string_lossy(),
                    &entry.checksum,
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

        let cell_w = (canvas_w - spacing * (cols + 1)) / cols;
        let cell_h = (canvas_h - spacing * (rows + 1)) / rows;

        let mut canvas = image::DynamicImage::new_rgba8(canvas_w, canvas_h);
        let bg = image::Rgba([30u8, 30u8, 30u8, 255u8]);
        for pixel in canvas.as_mut_rgba8().unwrap().pixels_mut() {
            *pixel = bg;
        }

        for (i, img) in images.iter().enumerate() {
            let col = i as u32 % cols;
            let row = i as u32 / cols;
            let scaled = img.resize_exact(
                cell_w.saturating_sub(spacing),
                cell_h.saturating_sub(spacing),
                image::imageops::FilterType::Lanczos3,
            );
            let x = spacing
                + col * (cell_w + spacing)
                + (cell_w.saturating_sub(spacing) - scaled.width()) / 2;
            let y = spacing
                + row * (cell_h + spacing)
                + (cell_h.saturating_sub(spacing) - scaled.height()) / 2;
            image::imageops::overlay(&mut canvas, &scaled, x as i64, y as i64);
        }

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
            .create(
                &temp.path().to_string_lossy(),
                hkask_storage::GalleryMode::ReadOnly,
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
        use hkask_storage::GalleryMode;
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
            .create("/tmp/test-gallery", GalleryMode::ReadOnly)
            .expect("create gallery");
        let img = store
            .add_image(
                &gallery.id,
                "alice.jpg",
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
            .register_face("Alice", "Chen", &img.id, None, "valid", "Frontal portrait")
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
            .create("/tmp/test-gallery", GalleryMode::ReadOnly)
            .expect("create gallery");
        let img1 = store
            .add_image(
                &gallery.id,
                "a.jpg",
                "/tmp/a.jpg",
                "h1",
                100,
                100,
                "jpg",
                1000,
            )
            .expect("add img1");
        let img2 = store
            .add_image(
                &gallery.id,
                "b.jpg",
                "/tmp/b.jpg",
                "h2",
                100,
                100,
                "jpg",
                1000,
            )
            .expect("add img2");

        store
            .register_face("Alice", "A", &img1.id, None, "valid", "")
            .unwrap();
        store
            .register_face("Bob", "B", &img2.id, None, "rejected", "Too dark")
            .unwrap();

        let valid = store.list_faces(Some("valid")).unwrap();
        assert_eq!(valid.len(), 1);
        assert_eq!(valid[0].first_name, "Alice");

        let rejected = store.list_faces(Some("rejected")).unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].first_name, "Bob");

        let pending = store.list_faces(Some("pending")).unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn gallery_lineage_record_and_replay_round_trip() {
        let (store, temp) = setup_store();
        let gallery = store
            .create(&temp.path().to_string_lossy(), GalleryMode::ReadOnly)
            .unwrap();
        create_test_image(temp.path(), "gen.png", 10, 20, 30);
        let img = store
            .add_image(
                &gallery.id,
                "gen.png",
                temp.path().join("gen.png").to_str().unwrap(),
                "hash-gen",
                64,
                64,
                "png",
                1024,
            )
            .unwrap();

        // Record the lineage a generation tool would attach after producing
        // this image (the gallery_record_generation tool wraps this call).
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
        // The stored params JSON round-trips — this is what gallery_reproduce
        // deserializes to replay the generation.
        let replay: serde_json::Value =
            serde_json::from_str(lineage.params.as_deref().unwrap()).unwrap();
        assert_eq!(replay["size"], "1024x1024");

        // No lineage for an unrelated image.
        create_test_image(temp.path(), "other.png", 1, 2, 3);
        let other = store
            .add_image(
                &gallery.id,
                "other.png",
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
    use crate::types::GalleryRefreshRequest;
    use rmcp::handler::server::wrapper::Parameters;
    use std::sync::Arc;

    fn make_server() -> MediaServer {
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let gallery_store =
            Arc::new(GalleryStore::from_driver(driver).expect("gallery store init"));
        MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(NoopInferencePort),
            Arc::new(std::sync::Mutex::new(None)),
            gallery_store,
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
        )
    }

    /// A no-op inference port — gallery_refresh without face analysis never
    /// reaches inference, so this only satisfies the struct field.
    struct NoopInferencePort;

    impl hkask_types::ports::InferencePort for NoopInferencePort {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "noop inference port — not configured for contract tests".into(),
                ))
            })
        }
    }

    /// Parse a tool result's `{"content": …}` envelope into its JSON value.
    fn content_of(result: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(result)
            .expect("tool result is JSON")
            .get("content")
            .cloned()
            .expect("tool result has the content envelope")
    }

    /// A mock inference port returning canned responses on both paths —
    /// `generate` (the text passes) and `media_generate` (the audio-chat
    /// path) — so the full pass pipelines run end-to-end without a live
    /// model.
    struct MockInferencePort {
        response: String,
        media_response: String,
    }

    impl hkask_types::ports::InferencePort for MockInferencePort {
        fn generate(
            &self,
            _: &str,
            _: &hkask_types::template::LLMParameters,
            _: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            let result = hkask_types::InferenceResult {
                text: self.response.clone(),
                model: "mock-model".to_string(),
                usage: hkask_types::InferenceUsage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
                finish_reason: "stop".to_string(),
                tool_calls: Vec::new(),
                reasoning: None,
                cost_usd: None,
            };
            Box::pin(async move { Ok(result) })
        }

        fn media_generate<'a>(
            &'a self,
            _op: &str,
            _params: &hkask_types::MediaGenerateParams,
        ) -> hkask_types::MediaFuture<'a> {
            let text = self.media_response.clone();
            Box::pin(async move { Ok(serde_json::json!({ "text": text, "model": "mock-model" })) })
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
        let driver = hkask_storage::database::sqlite::SqliteDriver::in_memory_driver();
        let gallery_store =
            Arc::new(GalleryStore::from_driver(driver).expect("gallery store init"));
        MediaServer::new(
            hkask_types::WebID::new(),
            Arc::new(MockInferencePort {
                response,
                media_response,
            }),
            Arc::new(std::sync::Mutex::new(None)),
            gallery_store,
            templates::create_env().expect("media templates must compile"),
            video::ffmpeg::FfmpegRunner::detect(),
            video::ytdlp::YtDlpRunner::detect(),
            jobs::new_job_store(),
        )
    }

    /// Store a two-word transcript and return its ID.
    async fn store_two_word_transcript(server: &MediaServer) -> String {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::types::EductStoreTranscriptRequest;
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
            ..TranscriptBundle::new("/tmp/a.wav".to_string(), 1.0, "alpha beta".to_string())
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

    #[tokio::test]
    async fn educt_paragraph_pass_stores_a_valid_layer() {
        let server = make_pass_server("{\"breaks_after\": [0]}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let result = server
            .educt_paragraph_pass(Parameters(crate::types::EductParagraphPassRequest {
                transcript_id: transcript_id.clone(),
                model: None,
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
                model: None,
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
                model: None,
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
                model: None,
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
                model: None,
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
                model: None,
                source: Some("text".to_string()),
                structured: None,
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
                model: None,
                source: Some("text".to_string()),
                structured: None,
            }))
            .await
            .expect_err("overlapping spans must be rejected");
        assert!(
            error.message.contains("overlapping"),
            "the named invariant must surface: {}",
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
                model: None,
                source: None,
                structured: None,
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
                model: None,
                source: Some("bogus".to_string()),
                structured: None,
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
                model: None,
                source: Some("audio".to_string()),
                structured: None,
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
                model: None,
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
                model: None,
                structured: None,
            }))
            .await
            .expect_err("out-of-bounds edit must be rejected");
        assert!(
            error.message.contains("out of bounds"),
            "the named invariant must surface: {}",
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
                model: None,
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
                model: None,
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
                model: None,
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
                model: None,
                source: None,
                structured: Some(true),
            }))
            .await
            .expect_err("structured+audio must be rejected, never a silent no-op");
        assert!(
            error.message.contains("structured outputs apply"),
            "the named rejection must surface: {}",
            error.message
        );
    }

    #[tokio::test]
    async fn educt_edl_from_highlights_merges_overlapping_selections() {
        use crate::transcript_layers::{
            HighlightEntry, HighlightLayer, LayerProvenance, TranscriptLayer,
        };
        use crate::types::EductStoreLayerRequest;

        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        // Two overlapping highlights over a 2-word transcript: [0,1] and
        // [1,1] → union [0,1] → one Keep op.
        let highlight = TranscriptLayer::Highlight(HighlightLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-08-30T00:00:00Z".to_string(),
            },
            highlights: vec![
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
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.clone(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&highlight).expect("serialize highlight"),
                ),
            }))
            .await
            .expect("highlight stored");

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
        use crate::transcript_layers::{
            HighlightEntry, HighlightLayer, LayerProvenance, TranscriptLayer,
        };
        use crate::types::EductStoreLayerRequest;

        let server = make_pass_server("{}".to_string());
        let transcript_id = store_two_word_transcript(&server).await;
        let highlight = TranscriptLayer::Highlight(HighlightLayer {
            provenance: LayerProvenance {
                model: "test".to_string(),
                prompt_template: "test".to_string(),
                created_at: "2026-08-30T00:00:00Z".to_string(),
            },
            highlights: vec![HighlightEntry {
                start_word: 0,
                end_word: 1,
                label: "a".to_string(),
                note: String::new(),
            }],
        });
        server
            .educt_store_layer(Parameters(EductStoreLayerRequest {
                transcript_id: transcript_id.clone(),
                layer: hkask_types::AnyJsonValue(
                    serde_json::to_value(&highlight).expect("serialize highlight"),
                ),
            }))
            .await
            .expect("highlight stored");
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

    #[tokio::test]
    async fn educt_render_edl_produces_a_clip_from_a_real_media_file() {
        use crate::transcript::{TimedWord, TranscriptBundle};
        use crate::transcript_layers::{EdlLayer, LayerProvenance, TranscriptLayer};
        use crate::transcript_select::{EdlEntry, EdlOp, WordRange};
        use crate::types::{
            EductRenderEdlRequest, EductStoreLayerRequest, EductStoreTranscriptRequest,
        };

        let server = make_pass_server("{}".to_string());
        // Generate a real 2-second WAV via ffmpeg — the same binary the
        // render path uses; the test proves the full loop (EDL → clip plan
        // → stream-copy render) against real media.
        let wav = std::env::temp_dir().join("hkask_educt_render_test.wav");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-ac",
                "1",
                "-ar",
                "16000",
            ])
            .arg(&wav)
            .output()
            .await
            .expect("ffmpeg runs");
        assert!(status.status.success(), "ffmpeg must generate the test WAV");
        let wav_path = wav.display().to_string();

        // A transcript over it: two words, 0-1s and 1-2s.
        let bundle = TranscriptBundle {
            words: vec![
                TimedWord {
                    word: "alpha".to_string(),
                    start_ms: 0,
                    end_ms: 1000,
                    confidence: None,
                },
                TimedWord {
                    word: "beta".to_string(),
                    start_ms: 1000,
                    end_ms: 2000,
                    confidence: None,
                },
            ],
            ..TranscriptBundle::new(wav_path.clone(), 2.0, "alpha beta".to_string())
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
            ops: vec![EdlEntry {
                range: WordRange::new(0, 0),
                op: EdlOp::Keep,
            }],
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

        let rendered = server
            .educt_render_edl(Parameters(EductRenderEdlRequest {
                transcript_id,
                layer_id: None,
            }))
            .await
            .expect("render succeeds");
        let content = content_of(&rendered);
        assert_eq!(content["clips"].as_u64(), Some(1));
        assert_eq!(
            content["clip_plan"],
            serde_json::json!([[0.0, 1.0]]),
            "the clip plan is the word range mapped to seconds"
        );
        let output = content["output"].as_str().expect("output path").to_string();
        assert!(
            std::path::Path::new(&output).exists(),
            "rendered clip exists at {output}"
        );
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
            ..TranscriptBundle::new("/tmp/a.wav".to_string(), 1.0, "alpha beta".to_string())
        };

        // Store.
        let stored = server
            .educt_store_transcript(Parameters(EductStoreTranscriptRequest {
                transcript: hkask_types::AnyJsonValue(
                    serde_json::to_value(&bundle).expect("serialize bundle"),
                ),
                gallery_asset_id: Some("asset-1".to_string()),
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
                gallery_asset_id: Some("asset-1".to_string()),
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
            matches!(error.kind, hkask_types::McpErrorKind::InvalidArgument),
            "gallery-not-initialized is a caller-fixable error, got {:?}",
            error.kind
        );
    }
}
