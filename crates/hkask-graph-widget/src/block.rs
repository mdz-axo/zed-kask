//! The ```` ```graph ```` block body model + parser.
//!
//! The shape mirrors the `scenario_quantify` tool response from
//! `hkask-mcp-scenarios` (subject, joint_probability, nodes with
//! `depends_on[].parent_event_ids`, marginal_probability, certainty_tier).
//! Fields are optional / defaulted so the parser is tolerant of partial
//! bodies and never fails on media-shaped JSON (which has no `viz` field).

use serde::Deserialize;

/// Evidence kind for interactive what-if overrides. Hard evidence clamps a
/// node's marginal to an observed value (the original click-to-set-0.9/0.1
/// behavior). Soft evidence applies a Bayesian likelihood-ratio update — the
/// superforecasting-standard input shape — so a user can express "I observed
/// X with likelihood 3:1" without fixing the marginal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvidenceKind {
    /// Observed probability: the node's marginal is clamped to this value.
    Hard(f64),
    /// Likelihood ratio P(evidence | node=true) / P(evidence | node=false).
    /// The posterior is P' = P·LR / (P·LR + (1−P)), then propagated.
    Soft(f64),
}

impl EvidenceKind {
    /// Apply the evidence to a prior marginal, returning the posterior.
    /// Hard evidence clamps; soft evidence applies the Bayesian update.
    pub fn apply(self, prior: f64) -> f64 {
        match self {
            EvidenceKind::Hard(value) => value.clamp(0.0, 1.0),
            EvidenceKind::Soft(likelihood_ratio) => {
                let p = prior.clamp(0.0, 1.0);
                if p <= 0.0 || p >= 1.0 {
                    return p;
                }
                let lr = likelihood_ratio.max(0.0);
                (p * lr / (p * lr + (1.0 - p))).clamp(0.0, 1.0)
            }
        }
    }
}

/// The discriminator-tagged body of a ```` ```graph ```` block.
///
/// `viz` selects the renderer; `"event_tree"` renders the MAIA event-tree DAG.
#[derive(Debug, Clone, Deserialize)]
pub struct GraphBlockBody {
    #[serde(default)]
    pub viz: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub joint_probability: Option<f64>,
    #[serde(default)]
    pub nodes: Vec<NodeBody>,
}

/// One node of the event tree. Edges are child-side: a node lists its parents
/// in `depends_on[].parent_event_ids` (the `scenario_quantify` shape) or, as a
/// tolerant fallback, in a flat `parents` array.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeBody {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub question: Option<String>,
    #[serde(default)]
    pub marginal_probability: Option<f64>,
    // `certainty_tier` and `variance_contribution` are intentionally NOT parsed
    // here: the widget derives the tier from `marginal_probability` via
    // `hkask_forecast::certainty_tier` (one source of truth, no drift from the
    // server), and never displays variance contribution. Any such fields the
    // agent emits are silently ignored (no `deny_unknown_fields`).
    #[serde(default)]
    pub depends_on: Vec<DependencyBody>,
    #[serde(default)]
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DependencyBody {
    #[serde(default)]
    pub parent_event_ids: Vec<String>,
    /// Full joint conditional table P(this | parent truth assignment), indexed
    /// by bitmap across the parents (`len == 2^parent_event_ids.len()`). Matches
    /// `scenario_quantify`'s `depends_on[].conditionals`.
    #[serde(default)]
    pub conditionals: Vec<f64>,
}

impl NodeBody {
    /// All parent ids for this node, deduplicated, from either edge
    /// representation. Order: `parents` first, then each dependency's
    /// `parent_event_ids` (first occurrence wins; later duplicates are dropped).
    pub fn parent_ids(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut ids: Vec<String> = Vec::new();
        for id in self.parents.iter().chain(
            self.depends_on
                .iter()
                .flat_map(|dep| dep.parent_event_ids.iter()),
        ) {
            if seen.insert(id.clone()) {
                ids.push(id.clone());
            }
        }
        ids
    }
}

/// Parse a ```` ```graph ```` block body. Tolerant: missing `viz`/`nodes` default
/// to `None`/empty rather than erroring, so media-shaped JSON parses (and is
/// then rejected by the renderer on the `viz` check) instead of being logged
/// as a malformed graph block.
pub fn parse_graph_body(body: &str) -> anyhow::Result<GraphBlockBody> {
    Ok(serde_json::from_str(body.trim())?)
}

/// A conditional-table mismatch found by [`validate_conditionals`].
///
/// `expected == 2^parent_event_ids.len()`; a `conditionals` vector whose length
/// differs yields a warning. The marginalization engine treats missing entries
/// as 0 (see `hkask_forecast::marginalize`), so a short table silently produces
/// a near-0 marginal rather than erroring — the warning makes that visible.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionalWarning {
    pub node_id: String,
    pub dependency_index: usize,
    pub n_parents: usize,
    pub expected: usize,
    pub actual: usize,
}

/// Validate every node's conditional tables. Returns one warning per
/// `depends_on` entry whose `conditionals.len() != 2^parent_event_ids.len()`.
///
/// This is the S4 (intelligence) layer the widget was missing: it runs at both
/// the parse boundary (`layout::compute_layout`) and the math boundary
/// (`propagate::recompute_marginals`) so a malformed block is signalled at the
/// point of consumption, not just at layout time. High-fan-in nodes (>20
/// parents) are skipped here — the bitmap would overflow `usize` and the
/// propagate engine falls back to the base marginal with its own warn.
pub fn validate_conditionals(body: &GraphBlockBody) -> Vec<ConditionalWarning> {
    let mut warnings = Vec::new();
    for node in &body.nodes {
        for (dep_idx, dep) in node.depends_on.iter().enumerate() {
            let n_parents = dep.parent_event_ids.len();
            if n_parents > 20 {
                continue;
            }
            let expected = 1usize << n_parents;
            let actual = dep.conditionals.len();
            if actual != expected {
                warnings.push(ConditionalWarning {
                    node_id: node.id.clone(),
                    dependency_index: dep_idx,
                    n_parents,
                    expected,
                    actual,
                });
            }
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_with_parents<'a>(
        id: &'a str,
        parents: &'a [&'a str],
        dep_parents: &'a [&'a str],
    ) -> NodeBody {
        NodeBody {
            id: id.into(),
            name: Some(id.into()),
            question: None,
            marginal_probability: None,
            depends_on: vec![DependencyBody {
                parent_event_ids: dep_parents.iter().map(|p| (*p).to_string()).collect(),
                conditionals: Vec::new(),
            }],
            parents: parents.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    #[test]
    fn parent_ids_deduplicates_overlapping_parents_and_deps() {
        // `parents` has "a" and "b"; `depends_on[0].parent_event_ids` has "b"
        // and "c". Dedup should yield ["a", "b", "c"] (first occurrence wins).
        let node = node_with_parents("n", &["a", "b"], &["b", "c"]);
        let ids = node.parent_ids();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn parent_ids_preserves_order_first_occurrence() {
        // "a" appears in both parents and deps — parents entry wins (first).
        let node = node_with_parents("n", &["a"], &["a", "b"]);
        let ids = node.parent_ids();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parent_ids_empty_when_no_parents() {
        let node = node_with_parents("n", &[], &[]);
        assert!(node.parent_ids().is_empty());
    }

    fn body_with_nodes(nodes: Vec<NodeBody>) -> GraphBlockBody {
        GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes,
        }
    }

    #[test]
    fn validate_conditionals_passes_full_table() {
        // 1 parent → 2 conditionals. Full table → no warnings.
        let node = NodeBody {
            id: "n".into(),
            name: None,
            question: None,
            marginal_probability: None,
            depends_on: vec![DependencyBody {
                parent_event_ids: vec!["p".into()],
                conditionals: vec![0.1, 0.6],
            }],
            parents: Vec::new(),
        };
        let body = body_with_nodes(vec![node]);
        assert!(validate_conditionals(&body).is_empty());
    }

    #[test]
    fn validate_conditionals_warns_on_short_table() {
        // 2 parents → expect 4 conditionals; only 2 provided.
        let node = NodeBody {
            id: "n".into(),
            name: None,
            question: None,
            marginal_probability: None,
            depends_on: vec![DependencyBody {
                parent_event_ids: vec!["a".into(), "b".into()],
                conditionals: vec![0.1, 0.2],
            }],
            parents: Vec::new(),
        };
        let body = body_with_nodes(vec![node]);
        let warnings = validate_conditionals(&body);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].node_id, "n");
        assert_eq!(warnings[0].dependency_index, 0);
        assert_eq!(warnings[0].n_parents, 2);
        assert_eq!(warnings[0].expected, 4);
        assert_eq!(warnings[0].actual, 2);
    }

    #[test]
    fn validate_conditionals_skips_high_fan_in() {
        // 21 parents → skipped (bitmap would overflow; propagate falls back
        // to base marginal with its own warn). No validation warning here.
        let parents: Vec<String> = (0..21).map(|i| format!("p{i}")).collect();
        let node = NodeBody {
            id: "n".into(),
            name: None,
            question: None,
            marginal_probability: None,
            depends_on: vec![DependencyBody {
                parent_event_ids: parents,
                conditionals: Vec::new(),
            }],
            parents: Vec::new(),
        };
        let body = body_with_nodes(vec![node]);
        assert!(validate_conditionals(&body).is_empty());
    }
}
