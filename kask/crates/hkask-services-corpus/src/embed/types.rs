//! Configuration and result types for the embedding pipeline.

use hkask_memory::salience::{BudgetConfig, DeclaredMethod};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ── Progress types ────────────────────────────────────────────────────────

/// Progress callback — called every 3 seconds during embedding.
pub type ProgressFn = Arc<dyn Fn(&EmbedProgress) + Send + Sync>;

/// Live progress state shared between the embed loop and the heartbeat task.
#[derive(Debug, Clone)]
pub struct EmbedProgress {
    pub phase: EmbedPhase,
    pub author: String,
    pub current_work: String,
    pub total_passages: usize,
    pub completed_passages: usize,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EmbedPhase {
    Parsing,
    Tagging,
    Embedding,
    Triples,
    Centroid,
    Done,
}

impl EmbedProgress {
    /// \[P7\] Motivating: Evolutionary Architecture — display formatting emerges from usage.
    /// pre:  self is a valid EmbedProgress
    /// post: returns formatted "TODO [N pages · X%] ::: DONE [N pages · Y%]" string
    #[must_use]
    pub fn format_page_progress(&self) -> String {
        let todo = self.total_passages.saturating_sub(self.completed_passages);
        let todo_pct = if self.total_passages > 0 {
            (todo as f64 / self.total_passages as f64) * 100.0
        } else {
            0.0
        };
        let done_pct = if self.total_passages > 0 {
            (self.completed_passages as f64 / self.total_passages as f64) * 100.0
        } else {
            0.0
        };
        format!(
            "TODO [{todo} pages · {todo_pct:.0}%] ::: DONE [{done} pages · {done_pct:.0}%]",
            todo = todo,
            todo_pct = todo_pct,
            done = self.completed_passages,
            done_pct = done_pct,
        )
    }

    /// \[P7\] Motivating: Evolutionary Architecture — full status formatting.
    /// pre:  self is a valid EmbedProgress
    /// post: returns formatted `[phase]` author — work — page_progress string
    #[must_use]
    pub fn format_full(&self) -> String {
        let phase_label = match self.phase {
            EmbedPhase::Parsing => "Parsing",
            EmbedPhase::Tagging => "Tagging",
            EmbedPhase::Embedding => "Embedding",
            EmbedPhase::Triples => "Triples",
            EmbedPhase::Centroid => "Centroid",
            EmbedPhase::Done => "Done",
        };
        format!(
            "[{phase_label}] {} — {}",
            self.author,
            if self.current_work.is_empty() {
                self.format_page_progress()
            } else {
                format!("{} — {}", self.current_work, self.format_page_progress())
            }
        )
    }
}

// ── Configuration ──────────────────────────────────────────────────────────

/// Corpus configuration — defines the author, works, embedding model,
/// chunking parameters, entity declarations, method declarations,
/// budget settings, and validation constraints.
#[derive(Debug, Deserialize, Serialize)]
pub struct CorpusConfig {
    pub author: String,
    pub embedding: EmbeddingConfig,
    pub works: Vec<Work>,
    pub foundational_rules: Vec<FoundationalRule>,
    pub chunking: ChunkingConfig,
    pub centroid_entity_ref: String,
    pub validation: ValidationConfig,

    /// Budget for h_mem storage per corpus (default: 3,750 h_mems/100 pages).
    #[serde(default)]
    pub budget: BudgetConfig,

    /// Entity declarations for tagging (who, where, what, why).
    #[serde(default)]
    pub entities: EntityConfig,

    /// Declared methods with signal thresholds (how).
    #[serde(default)]
    pub methods: Vec<DeclaredMethod>,

    /// Corpus type discriminator: "literary" or "academic".
    /// Determines which entity categories are active and which method
    /// signals are computed during embedding. Default: "literary".
    #[serde(default = "default_corpus_type")]
    pub corpus_type: String,

    /// Per-dimension centroid configuration with weights.
    /// Keys are dimension names (gentle, schriver, hopper, lovelace).
    /// Compute one centroid per dimension, then derive composite at query time.
    #[serde(default)]
    pub dimension_centroids: Vec<DimensionCentroid>,

    /// Orthogonal tag sets for multi-axis passage tagging.
    #[serde(default)]
    pub tag_sets: Vec<TagSet>,

    /// Per-document-type tag weight overrides.
    /// Maps document type (specification, guide, reference, etc.) to
    /// per-dimension weights. Applied at query time when comparing
    /// documents of a specific type against the embedding space.
    #[serde(default)]
    pub tag_weights: HashMap<String, HashMap<String, f64>>,

    /// Classifier config name (references registry/classify/{name}.yaml).
    /// If empty, section_type defaults to "Statement" for all passages.
    #[serde(default)]
    pub classifier: String,

    /// HMem extractor classifier config name (references registry/classify/{name}.yaml).
    /// Defaults to "h_mem-extractor". Set to empty string to disable.
    /// Model selection, Few-Shot strategy, and fallback are documented in the YAML config.
    #[serde(default = "default_triple_classifier")]
    pub triple_classifier: String,

    /// Fusion config for multi-model triple extraction. When present, the
    /// corpus pipeline routes triple extraction through the fusion
    /// orchestrator (typically `judge: algo` for algorithmic merge). Panel
    /// models are specified here; the algo judge merges their JSON responses.
    #[serde(default)]
    pub fusion: Option<hkask_types::fusion::FusionConfig>,
}

fn default_corpus_type() -> String {
    "literary".to_string()
}

fn default_triple_classifier() -> String {
    "h_mem-extractor".to_string()
}

/// Entity declarations for corpus-specific tagging.
///
/// Shared fields (both literary and academic): places, events, concepts.
/// Literary-only: characters.
/// Academic-only: co_authors, venues, topics, paradigms.
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
pub struct EntityConfig {
    /// Literary: named characters in the author's works.
    #[serde(default)]
    pub characters: Vec<Entity>,
    /// Shared: geographic/institutional places.
    #[serde(default)]
    pub places: Vec<Entity>,
    /// Shared: named events, studies, experiments.
    #[serde(default)]
    pub events: Vec<Entity>,
    /// Shared: key ideas, theories, frameworks.
    #[serde(default)]
    pub concepts: Vec<Entity>,

    // ── Academic-specific categories ──────────────────────────────────────
    /// Academic: co-authors and collaborators.
    #[serde(default)]
    pub co_authors: Vec<Entity>,
    /// Academic: journals, conferences, publishers.
    #[serde(default)]
    pub venues: Vec<Entity>,
    /// Academic: research areas and subfields.
    #[serde(default)]
    pub topics: Vec<Entity>,
    /// Academic: theoretical frameworks, paradigms, schools of thought.
    #[serde(default)]
    pub paradigms: Vec<Entity>,
}

/// A declared entity with name and optional per-work scoping.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Entity {
    pub name: String,
    /// Restrict to specific work slugs (empty = all works).
    #[serde(default)]
    pub appears_in: Vec<String>,
}

impl Entity {
    pub(crate) fn matches_work(&self, work_slug: &str) -> bool {
        self.appears_in.is_empty() || self.appears_in.iter().any(|w| w == work_slug)
    }

    pub(crate) fn name_strings(entities: &[Entity], work_slug: &str) -> Vec<String> {
        entities
            .iter()
            .filter(|e| e.matches_work(work_slug))
            .map(|e| e.name.clone())
            .collect()
    }
}

/// Embedding model and dimension configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct EmbeddingConfig {
    pub model: String,
    pub dim: usize,
    pub batch_size: usize,
}

/// A work (text) to download and embed.
#[derive(Debug, Deserialize, Serialize)]
pub struct Work {
    pub title: String,
    pub slug: String,
    pub url: String,
    /// Local file path for pre-downloaded works (takes precedence over url).
    #[serde(default)]
    pub local_path: Option<String>,
    /// Source format: "text", "pdf", or "web". Determines ingestion path.
    #[serde(default = "default_format")]
    pub format: String,
    /// Document type per MDS_SCAFFOLD.md §2: specification, adr, guide, reference, plan, status, research-paper, book-chapter.
    #[serde(default, alias = "type")]
    pub document_type: Option<String>,
    /// Dimension tags this work contributes to: ["Gentle"], ["Schriver"], ["Hopper"], ["Lovelace"].
    #[serde(default)]
    pub dimensions: Vec<String>,
    /// Section types present in this work: Statement, Evidence, Diagram, Implications.
    #[serde(default)]
    pub section_types: Vec<String>,
    /// MDS categories per MDS.md §1: domain, composition, trust, lifecycle, curation.
    #[serde(default)]
    pub mds_categories: Vec<String>,
}

fn default_format() -> String {
    "text".to_string()
}

/// A foundational rule to include as a passage.
#[derive(Debug, Deserialize, Serialize)]
pub struct FoundationalRule {
    pub slug: String,
    pub text: String,
    /// Dimension tags for this rule.
    #[serde(default)]
    pub dimensions: Vec<String>,
    /// Section type for this rule.
    #[serde(default)]
    pub section_type: Option<String>,
}

/// Chunking parameters for passage splitting.
#[derive(Debug, Deserialize, Serialize)]
pub struct ChunkingConfig {
    pub min_words: usize,
    pub max_words: usize,
    pub sentence_boundary: String,
}

/// Validation constraints for centroid distance and exemplar counts.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ValidationConfig {
    pub centroid_distance_max: f64,
    pub exemplar_count_min: usize,
    pub exemplar_count_max: usize,
}

/// Per-dimension centroid configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DimensionCentroid {
    /// Dimension name: "gentle", "schriver", "hopper", "lovelace".
    pub name: String,
    /// Entity ref for storing the centroid vector.
    pub ref_name: String,
    /// Weight in the composite centroid (should sum to 1.0 across all dimensions).
    pub weight: f64,
    /// Human-readable description of this dimension.
    #[serde(default)]
    pub description: String,
}

/// Orthogonal tag set definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagSet {
    /// Tag axis name: "section_type", "mds_category", "document_type", "dimension".
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Allowed values for this tag axis.
    #[serde(default)]
    pub values: Vec<String>,
}

// ── Result ─────────────────────────────────────────────────────────────────

/// Result of the embedding pipeline with budget statistics.
/// Summary of a single dimension centroid computation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DimensionCentroidResult {
    pub name: String,
    pub ref_name: String,
    pub passage_count: usize,
}

#[derive(Debug)]
pub struct EmbedResult {
    pub author: String,
    pub purged: usize,
    pub total_passages: usize,
    pub centroid_ref: String,
    pub passage_count: usize,
    pub centroid_stored: bool,
    pub validation: ValidationConfig,
    /// Total h_mem budget for this corpus.
    pub budget: usize,
    /// Number of passages that earned h_mem storage.
    pub tagged_passages: usize,
    /// Triples actually stored.
    pub triples_stored: usize,
    /// Passages that got embeddings only (below budget cutoff).
    pub embedding_only: usize,
    /// Per-dimension centroid results (empty if single-centroid path).
    pub dimension_centroids: Vec<DimensionCentroidResult>,
}

// ── Constants ──────────────────────────────────────────────────────────────

pub(crate) const USER_AGENT: &str = concat!("hkask-mcp-research/", env!("CARGO_PKG_VERSION"));
pub(crate) const CURATOR_PERSONA: &[u8] = b"Curator";
