//! The ```` ```graph ```` block body model + parser.
//!
//! The shape mirrors the `scenario_quantify` tool response from
//! `hkask-mcp-scenarios` (subject, joint_probability, nodes with
//! `depends_on[].parent_event_ids`, marginal_probability, certainty_tier).
//! Fields are optional / defaulted so the parser is tolerant of partial
//! bodies and never fails on media-shaped JSON (which has no `viz` field).

use serde::Deserialize;

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
}
