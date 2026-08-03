//! Layered DAG layout for the event-tree.
//!
//! Nodes are assigned to topological layers (roots at layer 0; a node's layer
//! is one past its deepest parent). Within a layer, nodes are spaced evenly.
//! Positions are in graph-space pixels (PADDING + layer*COLUMN_WIDTH,
//! PADDING-spread within the layer), consumed directly by the canvas and the
//! absolutely-positioned label/hit-area divs (no runtime transform — the graph
//! renders at its natural size, clipped by the container; pan/zoom is a
//! follow-up).

use std::collections::{HashMap, VecDeque};

use anyhow::{Result, bail};
use gpui::{Pixels, Point, px};

use crate::block::{GraphBlockBody, NodeBody};

const COLUMN_WIDTH: f32 = 240.0;
const ROW_HEIGHT: f32 = 84.0;
const PADDING: f32 = 56.0;

/// A laid-out node: id + graph-space position + display data.
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: String,
    pub name: String,
    pub question: Option<String>,
    pub marginal_probability: Option<f64>,
    pub certainty_tier: Option<String>,
    pub parents: Vec<String>,
    pub position: Point<Pixels>,
}

/// A resolved layered layout: nodes in input order with positions, edges as
/// `(parent_index, child_index)` pairs, and the graph's pixel extent.
#[derive(Debug, Clone)]
pub struct LayeredLayout {
    pub nodes: Vec<LayoutNode>,
    pub edges: Vec<(usize, usize)>,
    /// Node indices in topological (Kahn) order — the order marginal
    /// probabilities must be recomputed in (parents before children).
    pub topo_order: Vec<usize>,
    pub width: Pixels,
    pub height: Pixels,
}

impl LayeredLayout {
    /// An empty layout (used when computation fails, so the widget can render a
    /// placeholder rather than panic).
    pub fn empty() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            topo_order: Vec::new(),
            width: px(0.0),
            height: px(0.0),
        }
    }
}

/// Compute a layered layout from the parsed block body.
///
/// Validates node ids (unique, parents resolve), detects cycles (Kahn's
/// algorithm drains incompletely → `CycleDetected` analogue), assigns layers
/// by longest path from roots, and positions nodes in graph-space pixels.
pub fn compute_layout(body: &GraphBlockBody) -> Result<LayeredLayout> {
    if body.nodes.is_empty() {
        bail!("graph block has no nodes");
    }

    // Index nodes by id; reject duplicates.
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, node) in body.nodes.iter().enumerate() {
        if index.insert(node.id.clone(), i).is_some() {
            bail!("duplicate node id: {}", node.id);
        }
    }

    // Build edges (parent → child) from the child-side parent lists.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (child_idx, node) in body.nodes.iter().enumerate() {
        for parent_id in node.parent_ids() {
            match index.get(&parent_id) {
                Some(&parent_idx) => edges.push((parent_idx, child_idx)),
                None => bail!(
                    "node '{}' references unknown parent '{}'",
                    node.id,
                    parent_id
                ),
            }
        }
    }

    // Validate conditional tables. The scenarios server requires
    // `conditionals.len() == 2^parent_event_ids.len()`; a missing/short table
    // silently yields P≈0 in `propagate::recompute_marginals` (each missing entry
    // contributes 0). Warn so a malformed block is visible rather than rendering a
    // misleading 0% node.
    for node in &body.nodes {
        for dep in &node.depends_on {
            let n_parents = dep.parent_event_ids.len();
            if n_parents > 20 {
                log::warn!(
                    "hkask-graph-widget: node '{}' dependency has {} parents (>20); \
                     conditional table too large to validate",
                    node.id,
                    n_parents
                );
                continue;
            }
            let expected = 1usize << n_parents;
            if dep.conditionals.len() != expected {
                log::warn!(
                    "hkask-graph-widget: node '{}' dependency has {} conditionals, \
                     expected {} (2^{} parents) — marginals for this branch are incomplete",
                    node.id,
                    dep.conditionals.len(),
                    expected,
                    n_parents
                );
            }
        }
    }

    // Kahn's topological sort: layer[n] = 0 for roots, else max(layer[parent]) + 1.
    // If the queue drains before visiting every node, a cycle exists.
    let n = body.nodes.len();
    let mut in_degree = vec![0usize; n];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(parent, child) in &edges {
        in_degree[child] += 1;
        children[parent].push(child);
    }
    let mut layer = vec![0usize; n];
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut visited = 0usize;
    let mut topo_order: Vec<usize> = Vec::with_capacity(n);
    while let Some(node) = queue.pop_front() {
        topo_order.push(node);
        visited += 1;
        for &child in &children[node] {
            layer[child] = layer[child].max(layer[node] + 1);
            in_degree[child] -= 1;
            if in_degree[child] == 0 {
                queue.push_back(child);
            }
        }
    }
    if visited != n {
        bail!("event dependency graph has a cycle");
    }

    // Group node indices by layer; keep input order within a layer for stability.
    let max_layer = *layer.iter().max().unwrap_or(&0);
    let mut by_layer: Vec<Vec<usize>> = vec![Vec::new(); max_layer + 1];
    for (idx, &node_layer) in layer.iter().enumerate() {
        by_layer[node_layer].push(idx);
    }

    // Graph extent.
    let max_in_layer = by_layer.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let graph_w = PADDING * 2.0 + (max_layer as f32) * COLUMN_WIDTH;
    let graph_h = PADDING * 2.0 + (max_in_layer as f32 - 1.0) * ROW_HEIGHT;

    // Positions: x by layer, y spread within the layer.
    let mut positions = vec![Point::new(px(0.0), px(0.0)); n];
    for (node_layer, members) in by_layer.iter().enumerate() {
        let count = members.len();
        let x = PADDING + node_layer as f32 * COLUMN_WIDTH;
        for (rank, &idx) in members.iter().enumerate() {
            let y = if count == 1 {
                graph_h / 2.0
            } else {
                PADDING + (rank as f32 / (count as f32 - 1.0)) * (graph_h - PADDING * 2.0)
            };
            positions[idx] = Point::new(px(x), px(y));
        }
    }

    let nodes = body
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, node)| layout_node(node, positions[idx]))
        .collect();

    Ok(LayeredLayout {
        nodes,
        edges,
        topo_order,
        width: px(graph_w),
        height: px(graph_h),
    })
}

fn layout_node(node: &NodeBody, position: Point<Pixels>) -> LayoutNode {
    LayoutNode {
        id: node.id.clone(),
        name: node.name.clone().unwrap_or_else(|| node.id.clone()),
        question: node.question.clone(),
        marginal_probability: node.marginal_probability,
        // Derive the tier from the marginal (canonical thresholds live in
        // `hkask_forecast::certainty_tier`) rather than trusting a body field,
        // which may be absent — without this, nodes without an emitted tier
        // would render neutral grey even though their probability is known.
        certainty_tier: Some(
            hkask_forecast::certainty_tier(node.marginal_probability.unwrap_or(0.0)).to_string(),
        ),
        parents: node.parent_ids(),
        position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::DependencyBody;

    fn body(nodes: &[(&str, &[&str])]) -> GraphBlockBody {
        GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes: nodes
                .iter()
                .map(|(id, parents)| NodeBody {
                    id: (*id).into(),
                    name: Some((*id).into()),
                    question: None,
                    marginal_probability: None,
                    depends_on: parents
                        .iter()
                        .map(|p| DependencyBody {
                            parent_event_ids: vec![(*p).into()],
                            conditionals: Vec::new(),
                        })
                        .collect(),
                    parents: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn layers_a_simple_chain() {
        let body = body(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
        let layout = compute_layout(&body).expect("chain lays out");
        assert_eq!(layout.nodes.len(), 3);
        // x increases with layer: a < b < c.
        assert!(layout.nodes[0].position.x < layout.nodes[1].position.x);
        assert!(layout.nodes[1].position.x < layout.nodes[2].position.x);
    }

    #[test]
    fn rejects_a_cycle() {
        let body = body(&[("a", &["b"]), ("b", &["a"])]);
        assert!(compute_layout(&body).is_err());
    }

    #[test]
    fn rejects_unknown_parent() {
        let body = body(&[("a", &["missing"])]);
        assert!(compute_layout(&body).is_err());
    }
}
