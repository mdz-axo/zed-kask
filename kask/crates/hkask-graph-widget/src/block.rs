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
    #[serde(default)]
    pub variance_contribution: Option<f64>,
    #[serde(default)]
    pub certainty_tier: Option<String>,
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
    /// `parent_event_ids`.
    pub fn parent_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.parents.clone();
        for dep in &self.depends_on {
            ids.extend(dep.parent_event_ids.iter().cloned());
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
