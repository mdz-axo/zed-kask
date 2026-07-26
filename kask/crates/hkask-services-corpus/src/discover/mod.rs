//! DiscoveryService — Academic author corpus discovery pipeline.
//!
//! Orchestrates multi-source discovery via the MCP research server's
//! provider pool (Semantic Scholar, arXiv, Brave, Tavily, Exa, Firecrawl,
//! SerpAPI). Extracts content, caches to disk, and generates a corpus.yaml
//! ready for `EmbedService::embed_corpus()`.
//!
//! # REQ: P3 (Generative Space) — full parameter exposure, no hidden settings.
//! # expect: "The service layer enables generative access to domain capabilities"
//!
//! ## Pipeline
//! 1. Academic search via MCP web_search → Semantic Scholar + arXiv papers
//! 2. Extract search terms from paper titles (or use user-provided)
//! 3. Web search via MCP web_search → institutional pages, interviews
//! 4. YouTube transcript search via SerpAPI (requires API key)
//! 5. Content download + cache → .cache/{slug}.txt
//! 6. Concept extraction (LLM) → entities from paper titles
//! 7. Method inference (LLM) → stylometric patterns from cached passages
//! 8. Generate/augment corpus.yaml

mod cache;
mod config;
mod llm;
mod search;
mod service;
mod types;
mod utils;

#[cfg(test)]
mod tests;

pub use cache::download_and_cache;
pub use config::{default_corpus_config, generate_corpus_yaml};
pub use service::DiscoveryService;
pub use types::{DiscoverRequest, DiscoverResult, DiscoveredWork};
pub use utils::slugify;
