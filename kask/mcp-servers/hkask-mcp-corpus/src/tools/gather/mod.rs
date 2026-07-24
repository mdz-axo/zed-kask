//! Corpus gathering tools — discover academic works and cache extracted content.
//!
//! These tools are the "gather" stage of the unified corpus flow:
//!
//!   gather → process (chunk/tag/embed/triples) → output (QA training | persona)
//!
//! `corpus_discover` finds an author's body of work across multiple sources
//! and generates a corpus.yaml. `corpus_cache_work` caches extracted text
//! content to disk for reuse by the embedding pipeline.

use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
pub struct CacheWorkRequest {
    pub slug: String,
    pub content: String,
    pub cache_dir: String,
}

// ── Tool implementations ────────────────────────────────────────────────────

#[tool_router(router = gather_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Discover an academic author's body of work and generate a corpus.yaml for corpus_build_persona. Delegates to the replica-discovery skill manifest which orchestrates multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts), content extraction, and corpus generation. Supports agentic (fully automated) and curated (human-in-the-loop) modes."
    )]
    pub async fn corpus_discover(
        &self,
        Parameters(params): Parameters<DiscoverRequest>,
    ) -> String {
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
                manifest_id: "mcp/replica-discovery".into(),
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
        description = "Cache an extracted work's content to disk for reuse by corpus_build_persona. Writes content to {cache_dir}/{slug}.txt so the embedding pipeline can skip re-downloading."
    )]
    pub async fn corpus_cache_work(
        &self,
        Parameters(params): Parameters<CacheWorkRequest>,
    ) -> String {
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

            let cache_dir = PathBuf::from(&params.cache_dir);
            let cache_path = cache_dir.join(format!("{}.txt", params.slug));

            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                return Err(McpToolError::internal(format!(
                    "Failed to create cache directory '{}': {}",
                    cache_dir.display(),
                    e
                )));
            }

            let bytes = params.content.as_bytes();
            match std::fs::write(&cache_path, bytes) {
                Ok(()) => {
                    let result = CacheWorkResult {
                        slug: params.slug.clone(),
                        path: cache_path.to_string_lossy().to_string(),
                        bytes_written: bytes.len() as u64,
                    };
                    let output = serde_json::to_value(&result)
                        .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));
                    Ok(output)
                }
                Err(e) => Err(McpToolError::internal(format!(
                    "Failed to write cache file '{}': {}",
                    cache_path.display(),
                    e
                ))),
            }
        })
        .await
    }
}
