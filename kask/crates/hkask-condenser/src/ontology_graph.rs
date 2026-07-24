//! Ontology Concept Graph — lightweight cross-domain relationship index (P5.4/P8.1).
//!
//! Encodes relationships between concepts across all 6 domain ontologies
//! and the dual-axis framework. Built once at startup, zero dependencies,
//! no reasoners, no OWL parsing — follows the `fibo.rs` bridge pattern.
//!
//! The graph serves the condenser as a saliency multiplier: when compressing
//! content anchored to a concept (e.g., `fibo:Corporation`), lines referencing
//! related concepts (e.g., `fibo:MarketCapitalization`) receive a bonus —
//! structural knowledge about what "matters" in each domain.
//!
//! # Relationship Types
//!
//! | Edge | Meaning | Example |
//! |------|---------|---------|
//! | **PartOf** | A is a component of B | `pko:StepExecution` is part of `pko:ProcedureExecution` |
//! | **Precedes** | A must happen before B | `cogat:encoding` precedes `cogat:memory_consolidation` |
//! | **HasProperty** | A has attribute/measure B | `fibo:Corporation` has `fibo:MarketCapitalization` |
//! | **RelatedTo** | A and B are semantically linked | `cogat:salience` relates to `cogat:cued_recall` |
//! | **Contains** | A structurally contains B | `omc:Scene` contains `omc:Shot` |
//! | **CrossDomain** | A (domain X) maps to B (domain Y) | `pko:IssueOccurrence` may reference a `fibo:Corporation` |

use std::collections::HashMap;
use std::sync::OnceLock;

/// Directed relationship between two ontology concepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OntologyRelation {
    PartOf,
    Precedes,
    HasProperty,
    RelatedTo,
    Contains,
    CrossDomain,
}

/// A lightweight index of ontology concept relationships.
///
/// Key: a concept keyword (lowercase, e.g., "corporation", "step_execution").
/// Value: related concepts with relationship type.
/// Lookup is substring-based — a line containing the related keyword
/// adjacent to the anchored concept gets a saliency bonus.
pub struct OntologyGraph {
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

        // ── CogAT cognitive ──────────────────────────────────────────────
        edges.insert(
            "encoding",
            vec![
                ("memory_consolidation", OntologyRelation::Precedes),
                ("consolidation", OntologyRelation::Precedes),
                ("recall", OntologyRelation::Precedes),
            ],
        );
        edges.insert(
            "episodic_memory",
            vec![
                ("semantic_memory", OntologyRelation::RelatedTo),
                ("memory_consolidation", OntologyRelation::RelatedTo),
                ("encoding", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "semantic_memory",
            vec![
                ("episodic_memory", OntologyRelation::RelatedTo),
                ("semantic_processing", OntologyRelation::RelatedTo),
                ("concept_formation", OntologyRelation::RelatedTo),
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

        // ── FIBO financial ───────────────────────────────────────────────
        edges.insert(
            "corporation",
            vec![
                ("market_capitalization", OntologyRelation::HasProperty),
                ("market cap", OntologyRelation::HasProperty),
                ("price_earnings", OntologyRelation::HasProperty),
                ("p/e", OntologyRelation::HasProperty),
                ("enterprise_value", OntologyRelation::HasProperty),
                ("pe_ratio", OntologyRelation::HasProperty),
                ("debt_to_equity", OntologyRelation::HasProperty),
                ("return_on_equity", OntologyRelation::HasProperty),
                ("return_on_assets", OntologyRelation::HasProperty),
                ("gross_profit_margin", OntologyRelation::HasProperty),
                ("net_profit_margin", OntologyRelation::HasProperty),
                ("revenue_growth", OntologyRelation::HasProperty),
                ("free_cash_flow", OntologyRelation::HasProperty),
                ("dividend_yield", OntologyRelation::HasProperty),
            ],
        );
        edges.insert(
            "portfolio",
            vec![
                ("security_holding", OntologyRelation::HasProperty),
                ("holding_weight", OntologyRelation::HasProperty),
                ("weighted_average", OntologyRelation::HasProperty),
                ("attribution_analysis", OntologyRelation::HasProperty),
                ("time_weighted_return", OntologyRelation::HasProperty),
            ],
        );
        edges.insert(
            "dcf",
            vec![
                ("free_cash_flow", OntologyRelation::HasProperty),
                ("discount_rate", OntologyRelation::HasProperty),
                ("terminal_growth", OntologyRelation::HasProperty),
                ("enterprise_value", OntologyRelation::HasProperty),
                ("equity_value", OntologyRelation::HasProperty),
                ("margin_of_safety", OntologyRelation::HasProperty),
            ],
        );

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

        // ── OMC media ────────────────────────────────────────────────────
        edges.insert(
            "scene",
            vec![
                ("shot", OntologyRelation::Contains),
                ("sequence", OntologyRelation::PartOf),
            ],
        );
        edges.insert(
            "sequence",
            vec![
                ("scene", OntologyRelation::Contains),
                ("shot", OntologyRelation::Contains),
            ],
        );
        edges.insert(
            "image",
            vec![
                ("creative_work", OntologyRelation::RelatedTo),
                ("camera_metadata", OntologyRelation::RelatedTo),
            ],
        );
        edges.insert(
            "video",
            vec![
                ("shot", OntologyRelation::Contains),
                ("scene", OntologyRelation::Contains),
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
pub fn anchor_keywords(anchor: &crate::types::OntologyAnchor) -> Vec<&'static str> {
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
            namespace: crate::types::OntologyNamespace::Fibo,
            ..
        } => vec!["corporation", "portfolio", "dcf", "market"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Cogat,
            ..
        } => vec![
            "encoding",
            "episodic_memory",
            "semantic_memory",
            "salience",
            "recall",
        ],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Golem,
            ..
        } => vec!["character", "event", "narrative"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::MlSchema,
            ..
        } => vec!["run", "model", "training", "evaluation"],
        crate::types::OntologyAnchor::DomainSupplement {
            namespace: crate::types::OntologyNamespace::Omc,
            ..
        } => vec!["scene", "sequence", "image", "video"],
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
    fn fibo_corporation_has_properties() {
        let g = graph();
        let related = g.related("corporation");
        assert!(
            !related.is_empty(),
            "corporation must have related concepts"
        );
        let has_market_cap = related.iter().any(|(kw, _)| *kw == "market_capitalization");
        assert!(has_market_cap, "corporation → market_capitalization");
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
    fn cogat_encoding_precedes_consolidation() {
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
        // "corporation" → "market_capitalization" is a HasProperty edge
        let bonus = g.graph_adjacency_bonus(
            "AAPL has a market capitalization of 3.2 trillion",
            &["corporation"],
        );
        assert!(bonus > 0.0, "should get bonus for related concept");
        assert!(bonus <= 0.5, "bonus must be capped");
    }

    #[test]
    fn graph_adjacency_bonus_zero_for_unrelated() {
        let g = graph();
        let bonus = g.graph_adjacency_bonus("the weather is nice today", &["corporation"]);
        assert!((bonus - 0.0).abs() < 0.001);
    }

    #[test]
    fn graph_adjacency_bonus_capped() {
        let g = graph();
        // Many related keywords in one line — should cap at 0.5
        let bonus = g.graph_adjacency_bonus(
            "market_capitalization price_earnings enterprise_value debt_to_equity return_on_equity",
            &["corporation"],
        );
        assert!(
            (bonus - 0.5).abs() < 0.001,
            "bonus must cap at 0.5, got {bonus}"
        );
    }

    #[test]
    fn anchor_keywords_fibo_returns_financial() {
        use crate::types::{OntologyAnchor, OntologyNamespace};
        let anchor = OntologyAnchor::DomainSupplement {
            namespace: OntologyNamespace::Fibo,
            concept: "fibo".into(),
        };
        let kws = anchor_keywords(&anchor);
        assert!(kws.contains(&"corporation"));
        assert!(kws.contains(&"portfolio"));
    }

    #[test]
    fn anchor_keywords_core_returns_empty() {
        use crate::types::OntologyAnchor;
        let kws = anchor_keywords(&OntologyAnchor::Core);
        assert!(kws.is_empty());
    }
}
