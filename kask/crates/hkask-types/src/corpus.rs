//! Corpus pipeline types — shared between MCP server (hkask-mcp-corpus)
//! and pipeline tools.
//!
//! Single source of truth for the TaggedChunk type that flows through the
//! pipeline: tag → dedup → consolidate → build-prompts → ingest-qa.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Expertise level supported by the corpus pipeline.
///
/// Closed enum — the LLM may produce arbitrary strings, but the tagging
/// validation (`validate_ontology_tags`) maps invalid values to `Analyst`
/// before they enter a `TaggedChunk`. This makes invalid states
/// unrepresentable in the persistent record.
///
/// Serialization is lowercase to match the JSONL format produced by the
/// tagging template (`tag-chunks.j2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ExpertiseLevel {
    Practitioner,
    #[default]
    Analyst,
    Researcher,
}

impl ExpertiseLevel {
    /// Parse a string into an `ExpertiseLevel`. Unknown values fall back to
    /// `Analyst` (the default). Case-insensitive.
    pub fn from_str_fallback(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "practitioner" => Self::Practitioner,
            "researcher" => Self::Researcher,
            _ => Self::Analyst,
        }
    }

    /// Return the lowercase string form used in JSONL and template variables.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Practitioner => "practitioner",
            Self::Analyst => "analyst",
            Self::Researcher => "researcher",
        }
    }

    /// Numeric rank for consolidation: researcher > analyst > practitioner.
    /// Used by `corpus_consolidate_chunks` to take the highest expertise level
    /// across cluster members.
    pub fn rank(&self) -> u8 {
        match self {
            Self::Practitioner => 1,
            Self::Analyst => 2,
            Self::Researcher => 3,
        }
    }

    /// Construct from a numeric rank (inverse of `rank`).
    pub fn from_rank(rank: u8) -> Self {
        match rank {
            3 => Self::Researcher,
            1 => Self::Practitioner,
            _ => Self::Analyst,
        }
    }
}

impl std::fmt::Display for ExpertiseLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ExpertiseLevel {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_str_fallback(s))
    }
}

impl Serialize for ExpertiseLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ExpertiseLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_str_fallback(&s))
    }
}

/// Dublin Core + PKO metadata attached to consolidated chunks.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ChunkOntology {
    /// Dublin Core type (always "bibo:Document" for consolidated chunks).
    pub dc_type: String,
    /// Dublin Core subject — the concepts as ontology terms.
    pub dc_subject: Vec<String>,
    /// Dublin Core source — the original source file.
    pub dc_source: String,
    /// PKO provenance — wasExtractedFrom the original chunk refs.
    pub pko_extracted_from: Vec<String>,
}

/// A chunk annotated with multi-dimensional ontology tags.
///
/// This is the canonical type that flows through the entire corpus pipeline.
/// The MCP server (hkask-mcp-corpus) uses this struct — no local duplicates.
///
/// Design: open-world ontology tagging.
/// - 5W1H dimensions and Dublin Core are structural (every chunk has them)
/// - Domain-specific ontologies (FIBO, GOLEM, OMC, PKO, etc.) are stored in
///   `ontology_tags` — a flexible map keyed by namespace. Adding a new
///   ontology doesn't require changing this struct.
/// - `concepts` is a convenience cache = union of all ontology_tags values.
///
/// Pipeline flow:
///   tag-chunks → writes TaggedChunk to JSONL
///   dedup-chunks → reads TaggedChunk, writes subset
///   consolidate-chunks → reads TaggedChunk, writes merged TaggedChunk
///   build-prompts → reads TaggedChunk, generates QA prompts
///   ingest-qa → reads TaggedChunk metadata for sidecar annotations
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TaggedChunk {
    /// Unique entity reference (e.g., "corpus:researcher:Damodaran-ROIC_pdf_txt:119").
    pub entity_ref: String,
    /// Source file name (e.g., "Damodaran-ROIC.pdf.txt").
    pub source: String,
    /// The chunk text content.
    pub text: String,
    /// Word count from the original chunking phase.
    ///
    /// Populated by the corpus pipeline (`hkask-mcp-corpus`) during extraction
    /// and persisted as part of the corpus schema. Available for quality
    /// metrics and display; not consumed by the current retrieval path.
    #[serde(default)]
    pub word_count: usize,

    // ── Structural tags (always present) ─────────────────────────────────
    /// 5W1H interrogatory dimensions (Who/What/When/Where/Why/How). Multiple allowed.
    /// Universal ground — every chunk answers at least one interrogatory.
    #[serde(default)]
    pub dimensions: Vec<String>,

    /// Dublin Core BIBO type (e.g., "bibo:Book", "bibo:Article").
    #[serde(default)]
    pub dc_type: String,

    /// Dublin Core subject keywords — general topic classification.
    #[serde(default)]
    pub dc_subject: Vec<String>,

    /// Expertise level supported by the passage.
    ///
    /// Stored as `ExpertiseLevel` so invalid values are impossible in the
    /// persistent record. The custom serde deserializer maps unknown strings
    /// to `Analyst` (the default), matching the `validate_ontology_tags`
    /// runtime allowlist behavior.
    #[serde(default)]
    pub expertise_level: ExpertiseLevel,

    // ── Flexible ontology tags (open-world) ──────────────────────────────
    /// Domain-specific ontology concepts, keyed by namespace.
    /// Examples:
    ///   {"fibo": ["competitive advantage", "ROIC"], "golem": ["metaphor"], "omc": ["scene"], "pko": ["analysis"]}
    ///
    /// Adding a new ontology is just a new key — no struct change needed.
    /// The tagging LLM determines which ontologies are relevant per passage.
    #[serde(default)]
    pub ontology_tags: HashMap<String, Vec<String>>,

    /// Union of all ontology_tags values — convenience cache for downstream
    /// consumers that need the flat concept list without caring which ontology
    /// each concept came from.
    #[serde(default)]
    pub concepts: Vec<String>,

    // ── Computed scores ──────────────────────────────────────────────────
    /// Graph-centrality salience score [0.0, 1.0].
    #[serde(default)]
    pub salience: f32,

    // ── Consolidation provenance ─────────────────────────────────────────
    /// Original chunk refs this chunk was consolidated from (pko:wasExtractedFrom).
    /// Empty for pass-through (singleton) chunks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consolidated_from: Vec<String>,

    /// Dublin Core + PKO metadata for consolidated chunks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<ChunkOntology>,
}

impl TaggedChunk {
    /// Get concepts from a specific ontology namespace.
    /// Returns empty slice if the namespace isn't present.
    pub fn ontology_concepts(&self, namespace: &str) -> &[String] {
        self.ontology_tags
            .get(namespace)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Which ontology namespaces are present in this chunk's tags?
    pub fn ontology_namespaces(&self) -> impl Iterator<Item = &str> {
        self.ontology_tags.keys().map(|k| k.as_str())
    }

    /// Does this chunk have tags from the given ontology namespace?
    pub fn has_ontology(&self, namespace: &str) -> bool {
        self.ontology_tags.contains_key(namespace)
    }
}
