//! DiscoveryService — Pipeline orchestrator.

use super::llm::{extract_concepts, infer_methods};
use super::search::{mcp_search, search_youtube_transcripts};
use super::types::DiscoveredWork;
use super::utils::{extract_search_terms, slugify};
use crate::embed::EntityConfig;
use hkask_capability::DelegationToken;
use hkask_capability::ToolPort;
use hkask_services_core::{DomainKind, ErrorKind, ServiceError};
use std::path::PathBuf;

use super::cache::download_and_cache;
use super::config::{augment_corpus_yaml, generate_corpus_yaml};
use super::types::{DiscoverRequest, DiscoverResult};

pub struct DiscoveryService;

impl DiscoveryService {
    /// Run the full discovery pipeline and generate a corpus.yaml.
    ///
    /// `mcp` is the MCP dispatch port — must be connected to a running
    /// `hkask-mcp-research` server with configured providers.
    /// `token` is a delegation token for OCAP-gated tool invocation.
    ///
    /// \[P5\] Motivating: Essentialism — service-layer orchestration earns its existence; no raw domain logic.
    /// pre:  req.author_name must be non-empty; mcp must be connected; token must be valid
    /// post: returns DiscoverResult with discovered works, sources, and academic works; output and cache directories created; Err on MCP or I/O failure
    #[must_use = "result must be used"]
    pub async fn discover(
        req: &DiscoverRequest,
        mcp: &dyn ToolPort,
        token: &DelegationToken,
    ) -> Result<DiscoverResult, ServiceError> {
        // P9: Regulation span
        tracing::info!(target: "hkask.discover", operation = "discover", author = %req.author_name, max_works = req.max_works, "REG");

        let author_slug = slugify(&req.author_name);
        let output_dir = req
            .output_dir
            .clone()
            .unwrap_or_else(|| format!("./{}", author_slug));
        let output_path = PathBuf::from(&output_dir);
        let cache_dir = PathBuf::from(&req.cache_dir);

        // Ensure output and cache directories exist
        std::fs::create_dir_all(&output_path).map_err(|e| {
            let msg = format!(
                "Failed to create output directory '{}': {e}",
                output_path.display()
            );
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            let msg = format!(
                "Failed to create cache directory '{}': {e}",
                cache_dir.display()
            );
            ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: Some(Box::new(e)),
                message: msg,
            }
        })?;

        let mut works: Vec<DiscoveredWork> = Vec::new();
        let mut sources: Vec<String> = Vec::new();
        let mut academic_works: Vec<DiscoveredWork> = Vec::new();

        // ── Phase 1: Academic search via MCP web_search ────────────────────
        let academic_query = if let Some(ref bio) = req.biographical_details {
            format!("{} {}", req.author_name, bio)
        } else {
            req.author_name.clone()
        };
        tracing::info!(target: "hkask.discover", query = %academic_query, has_bio = req.biographical_details.is_some(), "Academic search query");

        match mcp_search(mcp, token, &academic_query, req.max_works, "web").await {
            Ok(results) => {
                let (academic, other): (Vec<_>, Vec<_>) = results.into_iter().partition(|w| {
                    let s = w.source.to_lowercase();
                    s == "semantic_scholar" || s == "arxiv"
                });

                let acad_count = academic.len();
                for w in &academic {
                    academic_works.push(w.clone());
                }
                works.extend(academic);
                sources.push(format!("academic_search ({acad_count} papers)"));

                // Non-academic results from the same search become web candidates
                if !other.is_empty() && req.include_web {
                    let existing_urls: Vec<&str> = works.iter().map(|w| w.url.as_str()).collect();
                    let new: Vec<DiscoveredWork> = other
                        .into_iter()
                        .filter(|w| !existing_urls.contains(&w.url.as_str()))
                        .collect();
                    if req.curated {
                        sources.push(format!(
                            "web_search ({} candidates from academic query)",
                            new.len()
                        ));
                    } else {
                        works.extend(new);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(target: "hkask.discover", error = %e, "Academic search via MCP failed — continuing");
            }
        }

        // ── Resolve web search terms ─────────────────────────────────────
        let search_terms = req.web_search_terms.clone().unwrap_or_else(|| {
            let titles: Vec<String> = academic_works.iter().map(|w| w.title.clone()).collect();
            extract_search_terms(&req.author_name, &titles)
        });
        tracing::info!(target: "hkask.discover", terms = %search_terms, "Resolved web search terms");

        // ── Phase 2: Web search via MCP ──────────────────────────────────
        let mut web_candidates: Vec<DiscoveredWork> = Vec::new();
        if req.include_web {
            match mcp_search(mcp, token, &search_terms, 5, "web").await {
                Ok(results) => {
                    let web_results: Vec<DiscoveredWork> = results
                        .into_iter()
                        .filter(|w| {
                            let s = w.source.to_lowercase();
                            s != "semantic_scholar" && s != "arxiv"
                        })
                        .collect();
                    let existing_urls: Vec<&str> = works.iter().map(|w| w.url.as_str()).collect();
                    let new: Vec<DiscoveredWork> = web_results
                        .into_iter()
                        .filter(|w| !existing_urls.contains(&w.url.as_str()))
                        .collect();
                    let added = new.len();
                    if req.curated {
                        web_candidates = new;
                        sources.push(format!(
                            "web_search ({added} candidates — awaiting curation)"
                        ));
                    } else {
                        works.extend(new);
                        sources.push(format!("web_search ({added} pages)"));
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "hkask.discover", error = %e, "Web search via MCP failed — continuing");
                }
            }
        }

        // ── Phase 3: YouTube transcript discovery (SerpAPI) ──────────────
        let mut youtube_candidates: Vec<DiscoveredWork> = Vec::new();
        if req.include_transcripts
            && let Some(ref key) = req.serpapi_key
        {
            match search_youtube_transcripts(&search_terms, key, 5).await {
                Ok(transcripts) => {
                    let count = transcripts.len();
                    if req.curated {
                        youtube_candidates = transcripts;
                        sources.push(format!(
                            "youtube_transcripts ({count} candidates — awaiting curation)"
                        ));
                    } else {
                        works.extend(transcripts);
                        sources.push(format!("youtube_transcripts ({count} videos)"));
                    }
                }
                Err(e) => {
                    tracing::warn!(target: "hkask.discover", error = %e, "YouTube transcript search failed — continuing");
                }
            }
        }

        // Check after ALL sources have been tried
        if works.is_empty() && web_candidates.is_empty() && youtube_candidates.is_empty() {
            return Err(ServiceError::Domain {
                domain: DomainKind::Wallet,
                kind: ErrorKind::ServiceUnavailable,
                source: None,
                message: format!("No works found for '{}' across any source", req.author_name),
            });
        }

        // ── Phase 4: Extract and cache content ────────────────────────────
        let mut cached = 0usize;
        for work in &works {
            let cache_path = cache_dir.join(format!("{}.txt", work.slug));
            if cache_path.exists() {
                cached += 1;
                continue;
            }

            match download_and_cache(&work.url, &cache_path).await {
                Ok(()) => cached += 1,
                Err(e) => {
                    tracing::warn!(target: "hkask.discover", slug = %work.slug, url = %work.url, error = %e, "Failed to download work — skipping");
                }
            }
        }

        // ── Phase 5a: Concept extraction (LLM) ─────────────────────────
        let mut entities: Option<EntityConfig> = None;
        let mut methods: Vec<hkask_memory::salience::DeclaredMethod> = Vec::new();

        if req.include_methods && !academic_works.is_empty() {
            match extract_concepts(&req.author_name, &academic_works).await {
                Ok(extracted) => {
                    tracing::info!(target: "hkask.discover", concepts = extracted.concepts.len(), places = extracted.places.len(), events = extracted.events.len(), "Concepts extracted");
                    entities = Some(extracted);
                }
                Err(e) => {
                    tracing::warn!(target: "hkask.discover", error = %e, "Concept extraction failed — continuing");
                }
            }
        }

        // ── Phase 5b: Method inference (LLM) ────────────────────────────
        if req.include_methods && cached > 0 {
            match infer_methods(&req.author_name, &works, &cache_dir).await {
                Ok(inferred) => {
                    tracing::info!(target: "hkask.discover", methods = inferred.len(), "Methods inferred");
                    methods = inferred;
                }
                Err(e) => {
                    tracing::warn!(target: "hkask.discover", error = %e, "Method inference failed — continuing");
                }
            }
        }

        // ── Phase 5: Generate corpus.yaml ──────────────────────────────────
        let config_path = if req.augment {
            augment_corpus_yaml(
                &author_slug,
                &works,
                &output_path,
                entities.clone(),
                &methods,
            )?
        } else {
            generate_corpus_yaml(
                &author_slug,
                &works,
                &output_path,
                entities.clone(),
                &methods,
            )?
        };

        tracing::info!(target: "hkask.discover", author = %req.author_name, slug = %author_slug, works_found = works.len(), works_cached = cached, config = %config_path.display(), "Discovery complete");

        Ok(DiscoverResult {
            author_slug,
            works_found: works.len(),
            works_cached: cached,
            config_path: config_path.to_string_lossy().to_string(),
            sources,
            web_candidates,
            youtube_candidates,
            entities,
            methods,
        })
    }
}
