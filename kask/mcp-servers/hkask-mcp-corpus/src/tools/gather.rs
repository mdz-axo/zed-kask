//! Corpus gathering tools — discover academic works and cache extracted content.
//!
//! These tools are the "gather" stage of the unified corpus flow:
//!
//!   gather → process (chunk/tag/embed/assertions) → output (QA training | compose)
//!
//! `corpus_discover` finds an author's body of work across multiple sources
//! and generates a corpus.yaml. `corpus_cache_work` caches extracted text
//! content to disk for reuse by the embedding pipeline.

use crate::helpers::map_corpus_io_error;
use crate::{CorpusServer, McpToolError, Parameters, execute_tool_semantic, tool, tool_router};
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
}

// ── Tool implementations ────────────────────────────────────────────────────

#[tool_router(router = gather_router, vis = "pub")]
impl CorpusServer {
    #[tool(
        description = "Discover an academic author's body of work and generate a corpus.yaml for style exemplar construction. Delegates to the corpus-discovery skill manifest which orchestrates multi-source search (Semantic Scholar, arXiv, web, YouTube transcripts), content extraction, and corpus generation. Supports agentic (fully automated) and curated (human-in-the-loop) modes."
    )]
    pub async fn corpus_discover(&self, Parameters(params): Parameters<DiscoverRequest>) -> String {
        execute_tool_semantic(self, "corpus_discover", Self::ontology_anchor("corpus_discover"), async {
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
    ) -> String {
        execute_tool_semantic(
            self,
            "corpus_cache_work",
            Self::ontology_anchor("corpus_cache_work"),
            async {
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
                let cache_path = cache_dir.join(format!("{}.txt", params.slug));

                if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                    return Err(map_corpus_io_error(
                        e,
                        &format!("Failed to create cache directory '{}'", cache_dir.display()),
                    ));
                }

                let bytes = params.content.as_bytes();
                match std::fs::write(&cache_path, bytes) {
                    Ok(()) => {
                        let result = CacheWorkResult {
                            slug: params.slug.clone(),
                            path: cache_path.to_string_lossy().to_string(),
                            bytes_written: bytes.len() as u64,
                        };
                        let output = serde_json::to_value(&result).unwrap_or_else(
                            |_| serde_json::json!({"error": "serialization failed"}),
                        );
                        Ok(output)
                    }
                    Err(e) => Err(map_corpus_io_error(
                        e,
                        &format!("Failed to write cache file '{}'", cache_path.display()),
                    )),
                }
            },
        )
        .await
    }

    #[tool(
        description = "Discover a company's document corpus from an approved-source manifest. Discovers tier-1 SEC filings (EDGAR) and tier-2 YouTube transcripts (SerpAPI, channel-allowlisted), generates a corpus.yaml for pipeline ingestion. Coverage-honest: non-allowlisted channels are excluded and logged, never silently kept."
    )]
    pub async fn corpus_discover_company(
        &self,
        Parameters(params): Parameters<DiscoverCompanyRequest>,
    ) -> String {
        execute_tool_semantic(self, "corpus_discover_company", Self::ontology_anchor("corpus_discover_company"), async {
            // Validate the mode parameter.
            let mode = match params.mode.as_str() {
                "agentic" | "curated" => params.mode.clone(),
                other => {
                    return Err(McpToolError::invalid_argument(format!(
                        "Invalid mode '{other}'. Use 'agentic' or 'curated'."
                    )));
                }
            };
            let is_curated = mode == "curated";

            // In curated mode, discovered items are marked "proposed" (awaiting
            // human review) rather than "discovered". The tool's job is discovery;
            // the accept/reject is the caller's job — the caller reviews the
            // proposed sources and decides which enter the corpus.yaml. This is
            // a labeling convention, not a full human-in-the-loop mechanism: the
            // tool returns the same result either way, just with a different
            // fetch_status label so the caller knows to review before ingesting.
            let fetch_status = if is_curated { "proposed" } else { "discovered" };

            // Load the company manifest from the registry.
            //
            // Two cases:
            // - Explicit `manifest_path` (LLM-reachable): route through
            //   `path_safety::read_capped` for containment under the project
            //   root + MAX_READ_BYTES cap (CWE-22/200/400).
            // - Default path (no `manifest_path` provided): resolve against
            //   the hKask data directory via `resolve_under_data_dir`. The
            //   default is trusted (not LLM-controlled), so it's read directly
            //   without path_safety containment — the data dir is an
            //   operator-controlled trusted location, not the project root.
            //   In dev (CWD = repo root), the relative path
            //   `kask/registry/company-sources/{symbol}.yaml` works directly.
            //   In production, the manifests are seeded to
            //   `{kask_data_dir}/skills/registry/company-sources/{symbol}.yaml`
            //   by the registry seeder at startup.
            let (manifest_text, _manifest_source) = match params.manifest_path.clone() {
                Some(explicit_path) => {
                    // LLM-controlled path — contain under project root.
                    let manifest_bytes =
                        crate::path_safety::read_capped(
                            &explicit_path,
                            crate::path_safety::MAX_READ_BYTES,
                        )
                        .map_err(|error| {
                            McpToolError::not_found(format!(
                                "company manifest not found or outside the project root at {explicit_path}: {error}"
                            ))
                        })?;
                    let text = String::from_utf8(manifest_bytes).map_err(|error| {
                        McpToolError::invalid_argument(format!(
                            "company manifest at {explicit_path} is not valid UTF-8: {error}"
                        ))
                    })?;
                    (text, explicit_path)
                }
                None => {
                    // Default path — not LLM-controlled, so no path_safety
                    // containment. Two resolution strategies:
                    //
                    // 1. Dev (CWD = repo root): try the relative path
                    //    `kask/registry/company-sources/{symbol}.yaml` —
                    //    the live source tree.
                    // 2. Production (CWD ≠ repo root): resolve
                    //    `skills/registry/company-sources/{symbol}.yaml`
                    //    under the data dir — where the registry seeder
                    //    materialises the compiled-in seed payload (D28:
                    //    registry is under `skills/`, not `agents/`).
                    let symbol_lower = params.symbol.to_lowercase();
                    let dev_path =
                        std::path::PathBuf::from(format!("kask/registry/company-sources/{symbol_lower}.yaml"));
                    let prod_relative = std::path::Path::new("skills/registry/company-sources")
                        .join(format!("{symbol_lower}.yaml"));
                    let prod_path =
                        hkask_types::agent_paths::resolve_under_data_dir(&prod_relative);
                    let (resolved, text) = if dev_path.is_file() {
                        let text = std::fs::read_to_string(&dev_path).map_err(|error| {
                            McpToolError::not_found(format!(
                                "company manifest for symbol '{symbol_lower}' found at {} but could not be read: {error}",
                                dev_path.display()
                            ))
                        })?;
                        (dev_path, text)
                    } else {
                        let text = std::fs::read_to_string(&prod_path).map_err(|error| {
                            McpToolError::not_found(format!(
                                "company manifest not found for symbol '{symbol_lower}'. \
                                 Tried dev path {} and production path {}. {error}. \
                                 Pass an explicit manifest_path.",
                                dev_path.display(),
                                prod_path.display()
                            ))
                        })?;
                        (prod_path, text)
                    };
                    (text, resolved.display().to_string())
                }
            };
            let manifest =
                crate::corpus::CompanySourceManifest::from_yaml(&manifest_text).map_err(
                    |error| McpToolError::invalid_argument(format!("manifest parse failed: {error}")),
                )?;
            manifest.validate().map_err(|error| {
                McpToolError::invalid_argument(format!("manifest validation failed: {error}"))
            })?;

            let mut discovered: Vec<DiscoveredCompanyDoc> = Vec::new();
            let mut excluded: Vec<ExcludedCompanyDoc> = Vec::new();

            // Tier 1: SEC filings via EDGAR full-text search by CIK.
            for entry in &manifest.source_tiers.tier_1_self_description {
                if entry.kind == "sec_filings" {
                    for form in &entry.forms {
                        let url = format!(
                            "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={}&type={}&dateb=&owner=include&count=10",
                            manifest.company.cik, form
                        );
                        discovered.push(DiscoveredCompanyDoc {
                            tier: 1,
                            kind: entry.kind.clone(),
                            title: format!("{} {} ({})", manifest.company.symbol, form, manifest.company.name),
                            url,
                            date: String::new(),
                            fetch_status: fetch_status.to_string(),
                        });
                    }
                } else if entry.kind == "earnings_transcript" {
                    // Earnings transcripts are fetched via the companies server's
                    // company_transcript tool, not here. Record as a discovered source.
                    discovered.push(DiscoveredCompanyDoc {
                        tier: 1,
                        kind: entry.kind.clone(),
                        title: format!("{} earnings transcripts (via companies_mcp)", manifest.company.symbol),
                        url: format!("company_transcript(symbol={}, mode=earnings)", manifest.company.symbol),
                        date: String::new(),
                        fetch_status: "delegate_to_companies_mcp".to_string(),
                    });
                }
            }

            // Tier 2: YouTube transcripts via SerpAPI (channel-allowlisted).
            if let Some(serpapi_key) = &params.serpapi_key {
                for entry in &manifest.source_tiers.tier_2_executive_voice {
                    if entry.kind == "youtube" {
                        for query in &entry.queries {
                            // Substitute template variables.
                            let resolved_query = query
                                .replace("{ceo_name}", manifest.company.ceo.as_deref().unwrap_or(""))
                                .replace("{company}", &manifest.company.name);

                            let search_results =
                                search_youtube_for_company(&resolved_query, serpapi_key, params.max_docs)
                                    .await;

                            match search_results {
                                Ok(videos) => {
                                    for (title, url, channel) in videos {
                                        let channel_allowed = entry
                                            .channels_allowlist
                                            .iter()
                                            .any(|allowed| channel.contains(allowed.as_str()));
                                        if channel_allowed {
                                            discovered.push(DiscoveredCompanyDoc {
                                                tier: 2,
                                                kind: "youtube".to_string(),
                                                title,
                                                url,
                                                date: String::new(),
                                                fetch_status: fetch_status.to_string(),
                                            });
                                        } else {
                                            excluded.push(ExcludedCompanyDoc {
                                                tier: 2,
                                                kind: "youtube".to_string(),
                                                title,
                                                url,
                                                channel,
                                                reason: "channel not on allowlist".to_string(),
                                            });
                                        }
                                    }
                                }
                                Err(error) => {
                                    excluded.push(ExcludedCompanyDoc {
                                        tier: 2,
                                        kind: "youtube".to_string(),
                                        title: resolved_query,
                                        url: String::new(),
                                        channel: String::new(),
                                        reason: format!("search failed: {error}"),
                                    });
                                }
                            }
                        }
                    }
                }
            } else {
                // No SerpAPI key — log as excluded so the operator knows.
                for entry in &manifest.source_tiers.tier_2_executive_voice {
                    if entry.kind == "youtube" {
                        excluded.push(ExcludedCompanyDoc {
                            tier: 2,
                            kind: "youtube".to_string(),
                            title: "YouTube discovery skipped".to_string(),
                            url: String::new(),
                            channel: String::new(),
                            reason: "HKASK_SERPAPI_API_KEY not provided".to_string(),
                        });
                    }
                }
            }

            // Generate corpus.yaml path.
            let corpus_yaml_path = params
                .output_path
                .clone()
                .unwrap_or_else(|| format!("company-{}-corpus.yaml", manifest.company.symbol.to_lowercase()));

            let tier_1 = discovered.iter().filter(|document| document.tier == 1).count() as u32;
            let tier_2 = discovered.iter().filter(|document| document.tier == 2).count() as u32;
            let tier_3 = discovered.iter().filter(|document| document.tier == 3).count() as u32;

            let result = DiscoverCompanyResult {
                manifest_id: manifest.manifest.id.clone(),
                company: manifest.company.symbol.clone(),
                discovered,
                excluded,
                corpus_yaml: corpus_yaml_path,
                coverage: CoverageByTier { tier_1, tier_2, tier_3 },
            };

            serde_json::to_value(&result).map_err(|error| {
                McpToolError::internal(format!("failed to serialize result: {error}")) // rr0044-ok: serialize-own-struct
            })
        })
        .await
    }
}

// ── Company discovery types ────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DiscoverCompanyRequest {
    pub symbol: String,
    /// Path to the company manifest YAML. Defaults to
    /// `kask/registry/company-sources/{symbol}.yaml`.
    pub manifest_path: Option<String>,
    /// SerpAPI key for YouTube discovery. If not provided, tier-2 YouTube
    /// discovery is skipped and logged in `excluded`.
    pub serpapi_key: Option<String>,
    /// Max results per YouTube search query (default 10).
    #[serde(default = "default_company_max_docs")]
    pub max_docs: u32,
    /// Output path for the generated corpus.yaml.
    pub output_path: Option<String>,
    /// Discovery mode: `agentic` (default, fully automated) or `curated`
    /// (returns proposals for human review; the caller accepts/rejects each
    /// discovered source before it enters the corpus.yaml).
    #[serde(default = "default_discovery_mode")]
    pub mode: String,
}

fn default_discovery_mode() -> String {
    "agentic".to_string()
}

fn default_company_max_docs() -> u32 {
    10
}

#[derive(Debug, Serialize)]
struct DiscoverCompanyResult {
    manifest_id: String,
    company: String,
    discovered: Vec<DiscoveredCompanyDoc>,
    excluded: Vec<ExcludedCompanyDoc>,
    corpus_yaml: String,
    coverage: CoverageByTier,
}

#[derive(Debug, Serialize)]
struct DiscoveredCompanyDoc {
    tier: u8,
    kind: String,
    title: String,
    url: String,
    date: String,
    /// Discovery status: "discovered" (agentic mode, auto-discovered),
    /// "proposed" (curated mode, awaiting human accept/reject), or
    /// "delegate_to_companies_mcp" (earnings transcripts are fetched via
    /// the companies server's company_transcript tool, not here).
    /// This is the DISCOVERY status, not the fetch status — the actual
    /// fetch happens in a separate step (company_transcript corpus mode
    /// for YouTube, corpus_chunk for SEC filings). The feedback loop from
    /// fetch → discovery is closed when the caller reports fetch outcomes
    /// back to the discovery tool on the next run.
    fetch_status: String,
}

#[derive(Debug, Serialize)]
struct ExcludedCompanyDoc {
    tier: u8,
    kind: String,
    title: String,
    url: String,
    channel: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct CoverageByTier {
    tier_1: u32,
    tier_2: u32,
    tier_3: u32,
}

/// Search YouTube via SerpAPI and return (title, url, channel) tuples.
/// Does NOT fetch transcripts — just discovers videos for the manifest.
async fn search_youtube_for_company(
    query: &str,
    api_key: &str,
    limit: u32,
) -> anyhow::Result<Vec<(String, String, String)>> {
    // CorpusServer doesn't have a shared reqwest::Client field (unlike
    // CompaniesServer), so this helper creates a standalone client with a
    // 30s timeout. The companies server's fetch_corpus_transcripts uses the
    // server's shared client. This is an intentional difference — the two
    // servers are independently deployed with different HTTP client policies.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| anyhow::anyhow!("client build failed: {error}"))?;
    let params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("api_key", api_key.to_string()),
        ("engine", "youtube".to_string()),
        ("num", limit.to_string()),
    ];

    let response = client
        .get("https://serpapi.com/search")
        .query(&params)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("request failed: {error}"))?;

    let body = response
        .text()
        .await
        .map_err(|error| anyhow::anyhow!("body read failed: {error}"))?;

    let parsed: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| anyhow::anyhow!("malformed JSON: {error}"))?;

    let videos = parsed["video_results"]
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|video| {
                    let title = video["title"].as_str()?.to_string();
                    let link = video["link"].as_str()?.to_string();
                    let channel = video["channel"]
                        .as_str()
                        .or_else(|| video["channel"]["name"].as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Some((title, link, channel))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(videos)
}
