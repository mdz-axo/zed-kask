//! Request types for hkask-mcp-replica MCP tools.
//!
//! Extracted from main.rs — these are the tool input structs that derive
//! Deserialize + JsonSchema for MCP parameter deserialization.

use schemars::JsonSchema;
use serde::Deserialize;

// ── Build types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildRequest {
    pub config_path: String,
    pub db_path: String,
    #[serde(default)]
    pub passphrase: Option<String>,
}

// ── Compose types ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ComposeRequest {
    pub prompt: String,
    pub author: String,
    pub db_path: String,
    pub passphrase: String,
    #[serde(default = "default_false")]
    pub no_validate: bool,
}

fn default_false() -> bool {
    false
}

// ── Compare types ─────────────────────────────────────────────────────────

fn default_compare_mode() -> String {
    "per-dimension".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompareRequest {
    pub db_path: String,
    pub passphrase: String,
    /// Scope comparison to a specific persona's centroids (e.g., "gentle-lovelace").
    /// When set, only centroids under style:{persona}: are considered.
    #[serde(default)]
    pub persona: Option<String>,
    /// Document content to embed and compare against centroids.
    /// When set, compares document embedding to persona centroids instead
    /// of doing pairwise author comparison.
    #[serde(default)]
    pub document_content: Option<String>,
    /// Comparison mode: "per-dimension" returns scores for each dimension
    /// centroid + composite; "composite" returns only the weighted composite.
    #[serde(default = "default_compare_mode")]
    pub compare_mode: String,
}

// ── Mashup types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MashupRequest {
    pub prompt: String,
    pub author_a: String,
    pub author_b: String,
    #[serde(default = "default_half")]
    pub blend: f64,
    pub db_path: String,
    pub passphrase: String,
}

fn default_half() -> f64 {
    0.5
}

// ── Registry types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum RegistryAction {
    List,
    Remove { author: String },
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RegistryRequest {
    #[serde(flatten)]
    pub action: RegistryAction,
    pub db_path: String,
    pub passphrase: String,
}

// ── Discovery types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscoverRequest {
    /// Full name of the academic author to research (e.g., "David Dunning")
    pub author_name: String,
    /// Discovery mode: "agentic" (fully automated) or "curated" (human-in-the-loop)
    #[serde(default = "default_curated")]
    pub mode: String,
    /// Maximum number of works to include in the corpus
    #[serde(default = "default_max_works")]
    pub max_works: u32,
    /// Whether to search for and include YouTube transcripts
    #[serde(default = "default_true")]
    pub include_transcripts: bool,
    /// Whether to include institutional pages and open web content
    #[serde(default = "default_true")]
    pub include_web: bool,
    /// Optional path to write the generated corpus.yaml
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

// ── Rewrite types ──────────────────────────────────────────────────────────

/// Request to rewrite a passage or code snippet in an author's style,
/// optimized for a specific Gentle Lovelace quality dimension.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RewriteRequest {
    /// The passage or code snippet to rewrite.
    pub content: String,
    /// The target author/persona for style (e.g., "gentle-lovelace").
    #[serde(default = "default_rewrite_author")]
    pub author: String,
    /// Which Gentle Lovelace dimension to optimize for:
    /// "composite", "gentle", "schriver", "hopper", or "lovelace".
    #[serde(default = "default_rewrite_dimension")]
    pub dimension: String,
    /// Path to the per-agent semantic database.
    pub db_path: String,
    /// Passphrase for opening the database.
    pub passphrase: String,
    /// Whether to skip centroid-distance validation.
    #[serde(default = "default_false")]
    pub no_validate: bool,
}

fn default_rewrite_author() -> String {
    "gentle-lovelace".to_string()
}

fn default_rewrite_dimension() -> String {
    "composite".to_string()
}

// ── Cache Work types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CacheWorkRequest {
    /// Work slug (used as filename: {slug}.txt)
    pub slug: String,
    /// Extracted markdown/text content to cache
    pub content: String,
    /// Cache directory path (e.g., "./.cache")
    pub cache_dir: String,
}
