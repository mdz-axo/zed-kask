//! Request / result types for the discovery pipeline.

use serde::{Deserialize, Serialize};

pub(crate) const USER_AGENT: &str = "hkask-discovery/0.27";

/// Parameters for corpus discovery.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscoverRequest {
    /// Full name of the academic author (e.g., "David Dunning")
    pub author_name: String,
    /// Maximum number of works to include
    #[serde(default = "default_max_works")]
    pub max_works: usize,
    /// Directory for caching extracted content
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    /// Directory to write the generated corpus.yaml
    pub output_dir: Option<String>,
    /// SerpAPI key for YouTube transcript search (web search uses MCP providers)
    #[serde(default)]
    pub serpapi_key: Option<String>,
    /// Whether to search for YouTube transcripts
    #[serde(default = "default_true")]
    pub include_transcripts: bool,
    /// Whether to search the web for institutional pages and interviews
    #[serde(default = "default_true")]
    pub include_web: bool,
    /// Curated mode: present web + YouTube results for user confirmation before including
    #[serde(default = "default_true")]
    pub curated: bool,
    /// Optional search terms for web + YouTube queries.
    /// If absent, terms are extracted from academic paper titles.
    #[serde(default)]
    pub web_search_terms: Option<String>,
    /// Augment an existing corpus rather than creating a new one.
    /// When true, loads the existing corpus.yaml and merges new works into it.
    #[serde(default)]
    pub augment: bool,
    /// Whether to run LLM-based concept extraction and method inference.
    /// Default: true (quality & precision first; set false for cheap/fast runs).
    #[serde(default = "default_true")]
    pub include_methods: bool,
    /// Optional biographical details for author disambiguation.
    /// Examples: "professor of psychology at Cornell University",
    /// "machine learning researcher at Stanford, PhD from MIT".
    /// Used to refine search queries and disambiguate common names.
    #[serde(default)]
    pub biographical_details: Option<String>,
}

fn default_max_works() -> usize {
    20
}
fn default_cache_dir() -> String {
    "./.cache".to_string()
}
fn default_true() -> bool {
    true
}

/// Result of a discovery run.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverResult {
    /// Author slug (e.g., "david-dunning")
    pub author_slug: String,
    /// Number of academic works discovered (Semantic Scholar + arXiv)
    pub works_found: usize,
    /// Number of works successfully cached
    pub works_cached: usize,
    /// Path to the generated corpus.yaml
    pub config_path: String,
    /// Sources used
    pub sources: Vec<String>,
    /// Web search candidates for curation (only populated when curated=true)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub web_candidates: Vec<DiscoveredWork>,
    /// YouTube transcript candidates for curation (only populated when curated=true)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub youtube_candidates: Vec<DiscoveredWork>,
    /// Extracted concepts, places, and events (populated when include_methods=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<crate::embed::EntityConfig>,
    /// Inferred methodological patterns (populated when include_methods=true)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<hkask_memory::salience::DeclaredMethod>,
}

/// A discovered work with metadata.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredWork {
    pub title: String,
    pub slug: String,
    pub url: String,
    pub year: Option<u16>,
    pub source: String,
    pub work_type: String,
    /// Abstract or snippet from the search result (when available).
    /// Used for LLM concept extraction.
    #[serde(default)]
    pub abstract_text: Option<String>,
}
