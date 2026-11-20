//! Corpus gathering tools — cache extracted content for the embedding pipeline.
//!
//! These tools are the "gather" stage of the unified corpus flow:
//!
//!   gather → process (chunk/tag/embed/assertions) → output (QA export | compose)
//!
//! `corpus_cache_work` caches extracted text content to disk for reuse by the
//! embedding pipeline. (`corpus_discover` was removed 2026-11-15, CO2-14: it
//! returned a fixed six-phase plan and wrote nothing.)

use crate::{CorpusServer, McpToolError, Parameters, execute_tool, tool, tool_router};
use hkask_mcp_server::server::{map_io_error, write_contained};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CacheWorkResult {
    slug: String,
    path: String,
    bytes_written: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin_provider: Option<String>,
    origin_sidecar: String,
}

// ── Request types ───────────────────────────────────────────────────────────
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CacheWorkRequest {
    pub slug: String,
    pub content: String,
    pub cache_dir: String,
    /// Originating search provider for search-derived content (e.g. "brave").
    /// Recorded in a `{slug}.origin.json` sidecar next to the cached file so
    /// chunking can stamp it on chunk rows; Brave-derived content is excluded
    /// from QA export paths that could otherwise become model-training material
    /// outside this system (Brave API ToS clause xiii).
    #[serde(default)]
    pub origin_provider: Option<String>,
}

// ── Tool implementations ────────────────────────────────────────────────────

#[tool_router(router = gather_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Cache an extracted work's content to disk for reuse by the embedding pipeline. Writes content to {cache_dir}/{slug}.txt so the embedding pipeline can skip re-downloading. The origin sidecar ({slug}.origin.json) is always written next to the content, origin_provider recorded as null when absent."
    )]
    pub async fn corpus_cache_work(
        &self,
        Parameters(params): Parameters<CacheWorkRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_cache_work", async {
            const MAX_SLUG_LEN: usize = 128;
            if params.slug.is_empty()
                || params.slug.len() > MAX_SLUG_LEN
                || !params
                    .slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Invalid slug '{}': must be 1..={MAX_SLUG_LEN} ASCII alphanumeric/hyphen/underscore characters",
                    params.slug
                )));
            }

            let cache_dir = crate::path_safety::contain_for_write(&params.cache_dir)?;
            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                return Err(map_io_error(
                    e,
                    &format!("Failed to create cache directory '{}'", cache_dir.display()),
                ));
            }

            let cache_target = cache_dir.join(format!("{}.txt", params.slug));
            let cache_target_str = cache_target
                .to_str()
                .ok_or_else(|| McpToolError::invalid_argument("cache path is not valid UTF-8"))?;
            // Pre-validate leaf containment and reject symlinks before touching
            // either sidecar or content on disk.
            crate::path_safety::contain_for_write(cache_target_str)?;
            if let Ok(meta) = std::fs::symlink_metadata(&cache_target)
                && meta.file_type().is_symlink()
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Refusing symlink at cache destination '{}'",
                    cache_target.display()
                )));
            }

            // Provenance sidecar (task c1d65229): write and persist the sidecar
            // BEFORE the content file so a failed sidecar write can never leave
            // unstamped Brave-derived text on disk (S040-F02).
            let provider = params
                .origin_provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let sidecar = cache_dir.join(format!("{}.origin.json", params.slug));
            let sidecar_str = sidecar
                .to_str()
                .ok_or_else(|| McpToolError::invalid_argument("sidecar path is not valid UTF-8"))?;
            crate::path_safety::contain_for_write(sidecar_str)?;
            let prior_sidecar = match std::fs::symlink_metadata(&sidecar) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(McpToolError::invalid_argument(format!(
                        "Refusing symlink at sidecar destination '{}'",
                        sidecar.display()
                    )));
                }
                Ok(_) => std::fs::read(&sidecar).ok(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(map_io_error(
                        e,
                        &format!("Failed to inspect sidecar '{}'", sidecar.display()),
                    ));
                }
            };
            let body = serde_json::to_vec(&serde_json::json!({
                "slug": &params.slug,
                "origin_provider": provider,
            }))
            .map_err(|e| McpToolError::internal(format!("Serialize origin sidecar: {e}")))?;
            write_contained(sidecar_str, &body)?;

            let bytes = params.content.as_bytes();
            let cache_path = match write_contained(cache_target_str, bytes) {
                Ok(path) => path,
                Err(err) => {
                    match prior_sidecar {
                        Some(prev) => {
                            let _ = write_contained(sidecar_str, &prev);
                        }
                        None => {
                            let _ = std::fs::remove_file(&sidecar);
                        }
                    }
                    return Err(err);
                }
            };
            let result = CacheWorkResult {
                slug: params.slug.clone(),
                path: cache_path.to_string_lossy().to_string(),
                bytes_written: bytes.len() as u64,
                origin_provider: provider.map(str::to_string),
                origin_sidecar: sidecar.to_string_lossy().to_string(),
            };
            let output = serde_json::to_value(&result)
                .map_err(|e| McpToolError::internal(format!("Serialize CacheWorkResult: {e}")))?;
            Ok(output)
        })
        .await
    }
}

#[cfg(test)]
mod gather_path_safety_tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;

    /// `corpus_cache_work` touches only the filesystem; this port is never
    /// called, so it fails loudly if that assumption ever stops holding.
    struct UnusedInferencePort;
    impl hkask_types::InferencePort for UnusedInferencePort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::template::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            unreachable!("corpus_cache_work must never call the inference port")
        }
    }

    /// A minimal server construction sufficient for `corpus_cache_work`,
    /// which touches only the filesystem — no database or inference port.
    fn server() -> CorpusServer {
        crate::helpers::seed_test_passphrase();
        let port: std::sync::Arc<dyn hkask_types::InferencePort> =
            std::sync::Arc::new(UnusedInferencePort);
        let ocr = std::sync::Arc::new(crate::ocr::llm_ocr::LlmOcrExecutor::new(port.clone()));
        CorpusServer::new(
            hkask_types::WebID::new(),
            None,
            port,
            Default::default(),
            ocr,
        )
    }

    /// Isolated fixture directory under the real crate CWD — `contain_for_write`
    /// reads `std::env::current_dir()` directly and has no injectable override,
    /// so the fixture must sit under it rather than an unrelated tempdir.
    fn fixture() -> tempfile::TempDir {
        let directory = std::env::current_dir()
            .expect("cwd")
            .join("target/gather-test");
        std::fs::create_dir_all(&directory).expect("fixture directory");
        tempfile::tempdir_in(directory).expect("isolated fixture")
    }

    /// The origin sidecar is ALWAYS written — the same dual-axis T5 shape
    /// corpus_cache uses: a provider-less cache_work records null, and the
    /// response says where the sidecar is.
    #[tokio::test]
    async fn cache_work_always_writes_its_origin_sidecar() {
        let fixture = fixture();
        let server = server();
        let cache_dir = fixture.path().join("cache");
        let response = server
            .corpus_cache_work(Parameters(CacheWorkRequest {
                slug: "work-1".into(),
                content: "cached work text".into(),
                cache_dir: cache_dir.to_string_lossy().into(),
                origin_provider: None,
            }))
            .await
            .expect("cache_work");
        let value = hkask_types::tool_response::unwrap_tool_envelope(
            serde_json::from_str(&response).expect("json"),
        );
        let sidecar_path = value["origin_sidecar"].as_str().expect("sidecar path");
        let sidecar = std::fs::read_to_string(sidecar_path).expect("sidecar exists");
        assert!(
            sidecar.contains("\"origin_provider\":null"),
            "a provider-less row records null, not an absent sidecar: {sidecar}"
        );
    }

    /// F4: an existing symlink at the exact `{slug}.txt` leaf must not
    /// redirect the write outside the cache directory. Before this fix,
    /// `corpus_cache_work` ran `contain_for_write` on `cache_dir` only, then
    /// joined the leaf afterward and wrote through it unvalidated — a leaf
    /// symlink pointing outside the cache dir would redirect the write with
    /// no rejection. The corrected call re-validates the joined leaf path.
    #[cfg(unix)]
    #[tokio::test]
    async fn existing_symlink_leaf_is_rejected_not_followed() {
        let fixture = fixture();
        let cache_dir = fixture.path().join("cache");
        std::fs::create_dir(&cache_dir).expect("cache dir");
        // Genuinely outside every allowed root (CWD, data dir, artifacts
        // dir): a standalone system tempdir, not one nested under the
        // crate's CWD the way `fixture()` (and therefore `cache_dir`) is.
        let outside_dir = tempfile::tempdir().expect("standalone outside dir");
        let outside_target = outside_dir.path().join("planted.txt");
        std::fs::write(&outside_target, "pre-existing outside content").expect("plant target");

        // Plant a symlink at the exact leaf `corpus_cache_work` will compute
        // for this slug, pointing outside the cache directory.
        let leaf = cache_dir.join("escape.txt");
        std::os::unix::fs::symlink(&outside_target, &leaf).expect("plant symlink leaf");

        let server = server();
        let result = server
            .corpus_cache_work(Parameters(CacheWorkRequest {
                slug: "escape".to_string(),
                cache_dir: cache_dir.to_string_lossy().into_owned(),
                content: "attacker-controlled content".to_string(),
                origin_provider: None,
            }))
            .await;

        assert!(
            result.is_err(),
            "a symlink leaf pointing outside the cache dir must be rejected, not followed"
        );
        assert_eq!(
            std::fs::read_to_string(&outside_target).expect("outside target still readable"),
            "pre-existing outside content",
            "the outside file must be unchanged — the write must never have followed the symlink"
        );
    }

    /// Control: an ordinary, non-symlinked leaf inside the cache dir still
    /// writes normally — the fix must not break the common case.
    #[tokio::test]
    async fn ordinary_leaf_still_writes() {
        let fixture = fixture();
        let cache_dir = fixture.path().join("cache");
        std::fs::create_dir(&cache_dir).expect("cache dir");

        let server = server();
        let result = server
            .corpus_cache_work(Parameters(CacheWorkRequest {
                slug: "ordinary".to_string(),
                cache_dir: cache_dir.to_string_lossy().into_owned(),
                content: "ordinary content".to_string(),
                origin_provider: None,
            }))
            .await
            .expect("ordinary write succeeds");
        assert!(result.contains("ordinary"), "{result}");
        assert_eq!(
            std::fs::read_to_string(cache_dir.join("ordinary.txt")).expect("written file"),
            "ordinary content"
        );
    }

    /// S040-F02: an overlong slug (e.g. 248 chars) or unwriteable sidecar must
    /// never leave Brave-derived text unstamped in cache_dir.
    #[tokio::test]
    async fn overlong_slug_rejected_before_writing_unstamped_brave_text() {
        let fixture = fixture();
        let cache_dir = fixture.path().join("cache");
        std::fs::create_dir(&cache_dir).expect("cache dir");
        let slug = "a".repeat(248);

        let server = server();
        let result = server
            .corpus_cache_work(Parameters(CacheWorkRequest {
                slug: slug.clone(),
                cache_dir: cache_dir.to_string_lossy().into_owned(),
                content: "brave search content".to_string(),
                origin_provider: Some("brave".to_string()),
            }))
            .await;
        assert!(result.is_err(), "248-char slug must be rejected");
        assert!(
            !cache_dir.join(format!("{slug}.txt")).exists(),
            "unstamped Brave .txt must never remain on disk when sidecar/slug fails"
        );
    }
}
