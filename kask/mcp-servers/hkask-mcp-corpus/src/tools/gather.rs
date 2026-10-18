//! Corpus gathering tools — discover academic works and cache extracted content.
//!
//! These tools are the "gather" stage of the unified corpus flow:
//!
//!   gather → process (chunk/tag/embed/assertions) → output (QA export | compose)
//!
//! `corpus_discover` finds an author's body of work across multiple sources
//! and generates a corpus.yaml. `corpus_cache_work` caches extracted text
//! content to disk for reuse by the embedding pipeline.

use crate::{CorpusServer, McpToolError, Parameters, execute_tool, tool, tool_router};
use hkask_mcp_server::server::{map_io_error, write_contained};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Response types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct DiscoverResult {
    manifest_id: String,
    parameters: serde_json::Value,
    summary: String,
    phases: Vec<DiscoverPhase>,
}

#[derive(Debug, Serialize)]
struct DiscoverPhase {
    ordinal: u32,
    name: String,
    description: String,
    sources: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CacheWorkResult {
    slug: String,
    path: String,
    bytes_written: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin_provider: Option<String>,
}

// ── Request types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoverRequest {
    pub author_name: String,
    #[serde(default = "default_curated")]
    pub mode: String,
    #[serde(default = "default_max_works")]
    pub max_works: u32,
    #[serde(default = "default_true")]
    pub include_transcripts: bool,
    #[serde(default = "default_true")]
    pub include_web: bool,
    pub output_path: Option<String>,
}

fn default_curated() -> String {
    "curated".to_string()
}
fn default_max_works() -> u32 {
    20
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
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
        description = "Discover an academic author's body of work and generate a corpus.yaml for style exemplar construction. Delegates to the corpus-discovery skill manifest which orchestrates multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts), content extraction, and corpus generation. Supports agentic (fully automated) and curated (human-in-the-loop) modes."
    )]
    pub async fn corpus_discover(
        &self,
        Parameters(params): Parameters<DiscoverRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_discover", async {
            let author_name = params.author_name.clone();

            let mode = match params.mode.as_str() {
                "agentic" | "curated" => params.mode.clone(),
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "Invalid mode '{}'. Use 'agentic' or 'curated'.",
                        other
                    )));
                }
            };

            let author_name_lower = author_name.to_lowercase();

            let manifest_params = serde_json::json!({
                "author_name": author_name,
                "author_name_lower": author_name_lower,
                "mode": mode,
                "max_works": params.max_works,
                "include_transcripts": params.include_transcripts,
                "include_web": params.include_web,
                "output_path": params.output_path,
            });

            let phases = vec![
                DiscoverPhase {
                    ordinal: 1,
                    name: "Name Disambiguation".into(),
                    description: "Search across multiple sources to confirm author identity".into(),
                    sources: vec!["web_search (deep)".into()],
                },
                DiscoverPhase {
                    ordinal: 2,
                    name: "Academic Paper Search".into(),
                    description: "Enumerate papers via Semantic Scholar and arXiv".into(),
                    sources: vec!["semantic_scholar".into(), "arxiv".into()],
                },
                DiscoverPhase {
                    ordinal: 3,
                    name: "Web + Institutional Content".into(),
                    description: "Find faculty pages, interviews, and open web content".into(),
                    sources: vec!["web_search (web)".into()],
                },
                DiscoverPhase {
                    ordinal: 4,
                    name: "YouTube Transcript Discovery".into(),
                    description: "Search for talks, interviews, lectures on YouTube".into(),
                    sources: vec![
                        "web_search (youtube.com)".into(),
                        "serpapi_transcript".into(),
                    ],
                },
                DiscoverPhase {
                    ordinal: 5,
                    name: "Content Extraction".into(),
                    description: "Extract full text from all discovered works".into(),
                    sources: vec!["web_extract".into(), "docproc (PDF/OCR)".into()],
                },
                DiscoverPhase {
                    ordinal: 6,
                    name: "Corpus YAML Generation".into(),
                    description: "Generate corpus.yaml from discovered works".into(),
                    sources: vec!["minijinja template".into()],
                },
            ];

            let summary = format!(
                "Discovering corpus for '{}' in {} mode. Will search Semantic Scholar, arXiv, web{}, and generate a corpus.yaml with up to {} works.",
                params.author_name,
                mode,
                if params.include_transcripts {
                    ", YouTube transcripts"
                } else {
                    ""
                },
                params.max_works,
            );

            let result = DiscoverResult {
                manifest_id: "mcp/corpus-discovery".into(),
                parameters: manifest_params,
                summary,
                phases,
            };

            let output = serde_json::to_value(&result)
                .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));

            Ok(output)
        })
        .await
    }

    #[tool(
        description = "Cache an extracted work's content to disk for reuse by the embedding pipeline. Writes content to {cache_dir}/{slug}.txt so the embedding pipeline can skip re-downloading."
    )]
    pub async fn corpus_cache_work(
        &self,
        Parameters(params): Parameters<CacheWorkRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "corpus_cache_work", async {
            if params.slug.is_empty()
                || !params
                    .slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(McpToolError::invalid_argument(format!(
                    "Invalid slug '{}': must be alphanumeric with hyphens/underscores only",
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
            let bytes = params.content.as_bytes();
            let cache_path = write_contained(cache_target_str, bytes)?;

            // Provenance sidecar (task c1d65229): the originating
            // search provider travels with the cached file so
            // `chunk_directory` can stamp it on chunk rows. Fail closed
            // — a lost sidecar breaks the downstream license exclusion.
            let provider = params
                .origin_provider
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(provider) = provider {
                let sidecar = cache_dir.join(format!("{}.origin.json", params.slug));
                let sidecar_str = sidecar.to_str().ok_or_else(|| {
                    McpToolError::invalid_argument("sidecar path is not valid UTF-8")
                })?;
                let body = serde_json::to_vec(&serde_json::json!({
                    "slug": &params.slug,
                    "origin_provider": provider,
                }))
                .map_err(|e| McpToolError::internal(format!("Serialize origin sidecar: {e}")))?;
                write_contained(sidecar_str, &body)?;
            }
            let result = CacheWorkResult {
                slug: params.slug.clone(),
                path: cache_path.to_string_lossy().to_string(),
                bytes_written: bytes.len() as u64,
                origin_provider: provider.map(str::to_string),
            };
            let output = serde_json::to_value(&result)
                .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));
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
}
