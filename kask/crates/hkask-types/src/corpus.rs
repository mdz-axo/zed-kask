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

/// Cheaply-computed stylometric signals for a passage.
///
/// All fields are derived from simple text analysis — no model inference.
/// These constitute the "how" (methods/techniques) dimension of the 5W1H
/// metadata layer.

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct MethodSignals {
    /// Ratio of coordinating conjunctions (and, but, or) to total
    /// conjunctions. High = paratactic (Hemingway). Low = hypotactic (Wilde).
    pub parataxis_ratio: f32,

    /// Approximate adjective count per 100 words. Uses suffix heuristics
    /// (-y, -ous, -ful, -less, -ive, -able, -al, -ent, -ic, -ish).
    pub adjective_density: f32,

    /// Words ending in -ly per 100 words (filtered for common false
    /// positives like "only", "early", "family").
    pub adverb_density: f32,

    /// Ratio of "was/were `<verb>ed`" patterns to total verbs.
    pub passive_voice_ratio: f32,

    /// Words inside double-quote characters divided by total words.
    pub dialogue_ratio: f32,

    /// Standard deviation of sentence lengths within the passage.
    pub sentence_length_variance: f32,

    /// Hedge words ("perhaps", "maybe", "seemed", "almost", "rather",
    /// "quite") per 100 words. Indicates qualification/uncertainty.
    pub hedge_density: f32,

    /// Intensifiers ("very", "really", "absolutely", "extremely",
    /// "utterly", "completely") per 100 words.
    pub intensifier_density: f32,

    /// Tangible/concrete nouns ÷ abstract nouns (rough suffix heuristic).
    /// High = sensory, concrete. Low = abstract, conceptual.
    pub concrete_noun_ratio: f32,

    /// Sensory words (sight, sound, touch, taste, smell) per 100 words.
    pub sensory_word_ratio: f32,

    /// Total word count of the passage.
    pub word_count: usize,

    /// Number of sentences in the passage.
    pub sentence_count: usize,

    /// Average sentence length (words/sentences).
    pub avg_sentence_length: f32,

    // ── Academic-specific signals ───────────────────────────────────────
    /// Citation count per 1000 words. Detects patterns like "(Author, Year)"
    /// and ``[1]``, ``[2,3]`` reference markers.
    pub citation_density: f32,
    /// Ratio of formal notation (math, code, LaTeX) characters to total
    /// characters. High in quantitative/CS papers, low in humanities.
    pub formalism_ratio: f32,
    /// Domain-specific terminology per 100 words. Detects multi-syllable
    /// words with Greek/Latin roots, acronyms, and technical compounds.
    pub technical_term_density: f32,
}

/// Dublin Core + PKO provenance and computed method metadata for a chunk.
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
    /// Deterministic 5W1H "how" metrics; absent on legacy, untagged records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method_signals: Option<MethodSignals>,
}

/// A chunk annotated with multi-dimensional ontology tags.
///
/// This is the canonical type that flows through the entire corpus pipeline.
/// The MCP server (hkask-mcp-corpus) uses this struct — no local duplicates.
///
/// Design: open-world ontology tagging.
/// - 5W1H dimensions and Dublin Core are structural (every chunk has them)
/// - Domain-specific ontologies (FIBO, GOLEM, PKO, etc.) are stored in
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
    ///   {"fibo": ["competitive advantage", "ROIC"], "golem": ["metaphor"], "pko": ["analysis"]}
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

    /// Dublin Core + PKO metadata and method signals from tagging/consolidation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ontology: Option<ChunkOntology>,
}
