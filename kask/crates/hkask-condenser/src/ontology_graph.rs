//! Ontology Concept Graph — lightweight cross-domain relationship index (P5.4/P8.1).
//!
//! Encodes relationships between concepts across the domain ontologies
//! and the dual-axis framework. Built once at startup, zero dependencies,
//! no reasoners, no OWL parsing — follows the bridge-module pattern.
//!
//! The graph serves the condenser as a saliency multiplier: when compressing
//! content anchored to a concept (e.g., `pko:StepExecution`), lines
//! referencing related concepts (e.g., `pko:StepVerification`)
//! receive a bonus — structural knowledge about what "matters" in each
//! domain. The graph's internal keys are hKask salience keywords (plain
//! lowercase concept names), not ontology URIs.
//!
//! # Relationship Types
//!
//! | Edge | Meaning | Example |
//! |------|---------|---------|
//! | **PartOf** | A is a component of B | `pko:StepExecution` is part of `pko:ProcedureExecution` |
//! | **Precedes** | A must happen before B | `sumo:encoding` precedes `sumo:memory_consolidation` |
//! | **HasProperty** | A has attribute/measure B | `mlschema:Run` has `mlschema:Model` |
//! | **RelatedTo** | A and B are linked in meaning | `sumo:salience` relates to `sumo:cued_recall` |
//! | **CrossDomain** | A (domain X) maps to B (domain Y) | `pko:IssueOccurrence` may reference a `sumo:Process` |

use std::collections::HashMap;
use std::sync::OnceLock;

/// Directed relationship between two ontology concepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OntologyRelation {
    PartOf,
    Precedes,
    RelatedTo,
    CrossDomain,
}

/// A lightweight index of ontology concept relationships.
///
/// Key: a concept keyword (lowercase, e.g., "step_execution", "encoding").
/// Value: related concepts with relationship type.
/// Lookup is substring-based — a line containing the related keyword
/// adjacent to the anchored concept gets a saliency bonus.
pub(crate) struct OntologyGraph {
    edges: HashMap<&'static str, Vec<(&'static str, OntologyRelation)>>,
}

impl OntologyGraph {
    /// Build the static concept relationship graph.
    /// Called once via `graph()` — `OnceLock` guarantees single initialization.
    fn build() -> Self {
        let mut edges: HashMap<&'static str, Vec<(&'static str, OntologyRelation)>> =
            HashMap::new();

        // ── PKO process axis ─────────────────────────────────────────────
        edges.insert(
            "step_execution",
            vec![
                ("procedure_execution", OntologyRelation::PartOf),
                ("step_verification", OntologyRelation::RelatedTo),
                ("execution", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "procedure_execution",
            vec![
                ("procedure", OntologyRelation::PartOf),
                ("execution", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "step",
            vec![
                ("step_execution", OntologyRelation::RelatedTo),
                ("multistep", OntologyRelation::RelatedTo),
                ("procedure", OntologyRelation::PartOf),
            ],
        );
        edges.insert(
            "change_of_status",
            vec![
                ("procedure_execution_status", OntologyRelation::RelatedTo),
                ("status", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "issue_occurrence",
            vec![("error", OntologyRelation::RelatedTo)],
        );
        edges.insert(
            "step_verification",
            vec![
                ("verification", OntologyRelation::RelatedTo),
                ("verify", OntologyRelation::RelatedTo),
                ("step_execution", OntologyRelation::RelatedTo),
            ],
        );

        // ── SUMO cognitive (memory/process) ──────────────────────────────
        edges.insert(
            "encoding",
            vec![
                ("memory_consolidation", OntologyRelation::Precedes),
                ("consolidation", OntologyRelation::Precedes),
                ("recall", OntologyRelation::Precedes),
            ],
        );
        edges.insert(
            "salience",
            vec![
                ("cued_recall", OntologyRelation::RelatedTo),
                ("recognition_memory", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "recall",
            vec![
                ("cued_recall", OntologyRelation::RelatedTo),
                ("recognition_memory", OntologyRelation::RelatedTo),
                ("encoding", OntologyRelation::Precedes),
            ],
        );
        edges.insert("forgetting", vec![("purge", OntologyRelation::RelatedTo)]);

        // ── GOLEM narrative ──────────────────────────────────────────────
        edges.insert(
            "character",
            vec![
                ("event", OntologyRelation::RelatedTo),
                ("setting", OntologyRelation::RelatedTo),
                ("narrative_function", OntologyRelation::RelatedTo),
                ("relationship", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "event",
            vec![
                ("character", OntologyRelation::RelatedTo),
                ("narrative_function", OntologyRelation::RelatedTo),
                ("scene", OntologyRelation::RelatedTo),
            ],
        );

        // ── ML-Schema ────────────────────────────────────────────────────
        edges.insert(
            "run",
            vec![
                ("model", OntologyRelation::RelatedTo),
                ("data", OntologyRelation::RelatedTo),
                ("evaluation", OntologyRelation::RelatedTo),
                ("hyperparameter", OntologyRelation::RelatedTo),
                ("accuracy", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "model",
            vec![
                ("run", OntologyRelation::RelatedTo),
                ("hyperparameter", OntologyRelation::RelatedTo),
                ("evaluation", OntologyRelation::RelatedTo),
                ("adapter", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "training",
            vec![
                ("learning_rate", OntologyRelation::RelatedTo),
                ("batch_size", OntologyRelation::RelatedTo),
                ("epoch", OntologyRelation::RelatedTo),
                ("loss", OntologyRelation::RelatedTo),
                ("evaluation", OntologyRelation::RelatedTo),
            ],
        );

        // ── Cross-domain bridges ─────────────────────────────────────────
        // A PKO process concept that maps to a domain supplement concept
        edges.insert(
            "issue_occurrence",
            vec![("error", OntologyRelation::RelatedTo)],
        );
        // Any domain supplement concept can relate to PKO's verification
        edges.insert(
            "step_verification",
            vec![
                ("evaluation", OntologyRelation::CrossDomain), // ML-Schema
                ("verification", OntologyRelation::RelatedTo),
            ],
        );

        Self { edges }
    }

    /// Look up concepts related to a given keyword.
    /// Returns empty slice for unknown keywords.
    pub fn related(&self, keyword: &str) -> &[(&str, OntologyRelation)] {
        static EMPTY: &[(&str, OntologyRelation)] = &[];
        self.edges
            .get(keyword)
            .map(|v| v.as_slice())
            .unwrap_or(EMPTY)
    }

    /// Score a line for graph-adjacent concepts relative to an anchor keyword.
    /// Returns 0.15 for each related concept found in the line (half the direct match bonus).
    pub fn graph_adjacency_bonus(&self, line: &str, anchor_keywords: &[&str]) -> f64 {
        let lower = line.to_lowercase();
        let mut bonus: f64 = 0.0;
        for &anchor_kw in anchor_keywords {
            for (related_kw, _relation) in self.related(anchor_kw) {
                if lower.contains(*related_kw) {
                    bonus += 0.15;
                }
            }
        }
        bonus.min(0.5) // cap at 0.5 to prevent runaway bonuses
    }
}

/// Global singleton — built once, shared immutably.
static GRAPH: OnceLock<OntologyGraph> = OnceLock::new();

/// Return a reference to the global ontology concept graph.
pub fn graph() -> &'static OntologyGraph {
    GRAPH.get_or_init(OntologyGraph::build)
}

/// Extract search keywords from an ontology anchor for graph lookup.
/// Returns a list of lowercase keywords to search for related concepts.
pub(crate) fn anchor_keywords(anchor: &crate::types::OntologyAnchor) -> Vec<&'static str> {
    match anchor {
        crate::types::OntologyAnchor::Core => vec![],
        crate::types::OntologyAnchor::DualAxis {
            axis: crate::types::OntologyAxis::Pko,
            ..
        } => vec!["step", "procedure", "execution", "verification"],
        crate::types::OntologyAnchor::DualAxis {
            axis: crate::types::OntologyAxis::DcBibo,
            ..
        } => vec!["article", "dataset", "document"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Sumo,
            ..
        } => vec!["encoding", "salience", "recall"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Golem,
            ..
        } => vec!["character", "event", "narrative"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::MlSchema,
            ..
        } => vec!["run", "model", "training", "evaluation"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Sepio,
            ..
        } => vec!["hypothesis", "evidence", "falsification", "corroboration"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::DataCube,
            ..
        } => vec!["observation", "dataset", "dimension", "measure", "slice"],
        // OMC has no section in the condenser's internal concept graph yet,
        // so there is nothing for media keywords to be adjacent TO — empty
        // (like Core) rather than dead keyword config. Add keywords here
        // when the graph grows media concepts.
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Omc,
            ..
        } => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_is_singleton() {
        let g1 = graph();
        let g2 = graph();
        assert!(std::ptr::eq(g1, g2), "graph must be singleton");
    }

    #[test]
    fn sumo_recall_relates_to_encoding() {
        let g = graph();
        let related = g.related("recall");
        assert!(!related.is_empty(), "recall must have related concepts");
        let has_encoding = related.iter().any(|(kw, _)| *kw == "encoding");
        assert!(has_encoding, "recall → encoding");
    }

    #[test]
    fn pko_step_execution_part_of_procedure() {
        let g = graph();
        let related = g.related("step_execution");
        let is_part_of = related
            .iter()
            .any(|(kw, r)| *kw == "procedure_execution" && *r == OntologyRelation::PartOf);
        assert!(is_part_of);
    }

    #[test]
    fn sumo_encoding_precedes_consolidation() {
        let g = graph();
        let related = g.related("encoding");
        let precedes = related
            .iter()
            .any(|(kw, r)| *kw == "memory_consolidation" && *r == OntologyRelation::Precedes);
        assert!(precedes);
    }

    #[test]
    fn unknown_keyword_returns_empty() {
        let g = graph();
        assert!(g.related("nonexistent_concept").is_empty());
    }

    #[test]
    fn graph_adjacency_bonus_fires_on_related() {
        let g = graph();
        // "encoding" → "recall" is a RelatedTo edge
        let bonus = g.graph_adjacency_bonus(
            "Encoding completed, and cued recall returned 7 contexts",
            &["encoding"],
        );
        assert!(bonus > 0.0, "should get bonus for related concept");
        assert!(bonus <= 0.5, "bonus must be capped");
    }

    #[test]
    fn graph_adjacency_bonus_zero_for_unrelated() {
        let g = graph();
        let bonus = g.graph_adjacency_bonus("the weather is nice today", &["encoding"]);
        assert!((bonus - 0.0).abs() < 0.001);
    }

    #[test]
    fn graph_adjacency_bonus_capped() {
        let g = graph();
        // Many related keywords in one line — should cap at 0.5
        let bonus = g.graph_adjacency_bonus(
            "encoding memory_consolidation consolidation recall salience cued_recall",
            &["encoding", "salience"],
        );
        assert!(
            (bonus - 0.5).abs() < 0.001,
            "bonus must cap at 0.5, got {bonus}"
        );
    }

    #[test]
    fn anchor_keywords_sumo_returns_memory() {
        use crate::types::{OntologyAnchor, OntologyNamespace};
        let anchor = OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Sumo,
            concept: "sumo".into(),
        };
        let kws = anchor_keywords(&anchor);
        assert!(kws.contains(&"encoding"));
        assert!(kws.contains(&"recall"));
    }

    #[test]
    fn anchor_keywords_core_returns_empty() {
        use crate::types::OntologyAnchor;
        let kws = anchor_keywords(&OntologyAnchor::Core);
        assert!(kws.is_empty());
    }
}
