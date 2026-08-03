//! Marginal-probability recomputation with evidence overrides.
//!
//! Mirrors `hkask_mcp_scenarios::superforecast::compute_marginal_probabilities`
//! (full joint-table marginalization under parent independence) and adds an
//! evidence override: a node in the `evidence` map is treated as observed (its
//! marginal is fixed to the set value) rather than marginalized from parents.
//! This lets the widget re-propagate the tree interactively when a user sets
//! evidence on a node, without coupling the UI crate to the MCP server.
//!
//! TODO: consolidate this with the server's implementation into a shared
//! `hkask-forecast` crate so there is one source of truth for the math.

use std::collections::HashMap;

use crate::block::GraphBlockBody;

/// Recompute every node's marginal probability given the current base
/// probabilities (root nodes' `marginal_probability`) and a set of evidence
/// overrides. Returns marginals indexed by node position in `body.nodes`.
///
/// A node in `evidence` uses its set value verbatim (observed). A root node
/// (no parents) uses its stored `marginal_probability`. A dependent node
/// marginalizes over the full joint truth-assignment space of its
/// `depends_on[0]` parents:
///
/// `P(E) = Σ_a P(E|a) · Π_i P(p_i)^a_i · (1 − P(p_i))^(1 − a_i)`
///
/// where `a` ranges over the `2^n` bitmap of parent truth assignments and
/// parent marginals are assumed independent. This matches the server's
/// computation. Only `depends_on[0]` is consumed (the engine's documented
/// limitation).
pub fn recompute_marginals(
    body: &GraphBlockBody,
    topo_order: &[usize],
    evidence: &HashMap<usize, f64>,
) -> Vec<f64> {
    let n = body.nodes.len();
    let mut marginals = vec![0.0f64; n];
    for &idx in topo_order {
        if let Some(&value) = evidence.get(&idx) {
            marginals[idx] = value.clamp(0.0, 1.0);
            continue;
        }
        let node = &body.nodes[idx];
        let parents = node.parent_ids();
        if parents.is_empty() {
            marginals[idx] = node.marginal_probability.unwrap_or(0.0).clamp(0.0, 1.0);
            continue;
        }
        // Only depends_on[0] drives the math (engine limitation); use its
        // conditionals + parent marginals.
        let dep = match node.depends_on.first() {
            Some(dep) => dep,
            None => {
                marginals[idx] = node.marginal_probability.unwrap_or(0.0).clamp(0.0, 1.0);
                continue;
            }
        };
        let parent_marginals: Vec<f64> = dep
            .parent_event_ids
            .iter()
            .map(|pid| {
                body.nodes
                    .iter()
                    .position(|n| &n.id == pid)
                    .and_then(|pi| marginals.get(pi).copied())
                    .unwrap_or(0.0)
            })
            .collect();
        let n_parents = dep.parent_event_ids.len();
        // Guard against pathological fan-in (the bitmap would overflow).
        if n_parents > 20 {
            marginals[idx] = node.marginal_probability.unwrap_or(0.0).clamp(0.0, 1.0);
            continue;
        }
        // Delegate the joint-marginalization formula to the shared
        // `hkask_forecast::marginalize` so this re-propagation cannot drift from
        // `hkask-mcp-scenarios::compute_marginal_probabilities`.
        let marginal = hkask_forecast::marginalize(&parent_marginals, &dep.conditionals);
        marginals[idx] = marginal.clamp(0.0, 1.0);
    }
    marginals
}

/// The certainty tier for a marginal probability, matching the scenarios
/// server's `CertaintyTier::from_probability`: proximate (≥67%), probable
/// (33–66%), possible (<33%).
pub fn certainty_tier(probability: f64) -> &'static str {
    if probability >= 0.67 {
        "proximate"
    } else if probability >= 0.33 {
        "probable"
    } else {
        "possible"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{DependencyBody, GraphBlockBody, NodeBody};

    fn node(id: &str, prob: f64, parents: &[&str]) -> NodeBody {
        NodeBody {
            id: id.into(),
            name: Some(id.into()),
            question: None,
            marginal_probability: Some(prob),
            variance_contribution: None,
            certainty_tier: None,
            depends_on: parents
                .iter()
                .map(|p| DependencyBody {
                    parent_event_ids: vec![(*p).into()],
                    conditionals: Vec::new(),
                })
                .collect(),
            parents: Vec::new(),
        }
    }

    fn body(nodes: Vec<NodeBody>, topo: Vec<usize>) -> (GraphBlockBody, Vec<usize>) {
        (
            GraphBlockBody {
                viz: Some("event_tree".into()),
                subject: None,
                joint_probability: None,
                nodes,
            },
            topo,
        )
    }

    #[test]
    fn root_uses_its_base_probability() {
        let (body, topo) = body(vec![node("a", 0.7, &[])], vec![0]);
        let m = recompute_marginals(&body, &topo, &HashMap::new());
        assert_eq!(m, vec![0.7]);
    }

    #[test]
    fn dependent_marginalizes_over_parents() {
        // a (0.8) -> b, with P(b|¬a)=0.1, P(b|a)=0.6 → P(b)=0.1*0.2 + 0.6*0.8 = 0.5
        let (body, topo) = body(
            vec![node("a", 0.8, &[]), node("b", 0.0, &["a"])],
            vec![0, 1],
        );
        let mut b = body.nodes[1].clone();
        b.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["a".into()],
            conditionals: vec![0.1, 0.6],
        }];
        let body = GraphBlockBody {
            nodes: vec![body.nodes[0].clone(), b],
            ..body
        };
        let m = recompute_marginals(&body, &topo, &HashMap::new());
        assert!((m[1] - 0.5).abs() < 1e-9, "got {}", m[1]);
    }

    #[test]
    fn evidence_overrides_a_node_and_propagates_to_children() {
        // a (0.2) -> b (conditionals [0.1, 0.6]). Base P(b)=0.1*0.8+0.6*0.2=0.2.
        // Set evidence a=0.9 → P(b)=0.1*0.1+0.6*0.9=0.55.
        let (body, topo) = body(
            vec![node("a", 0.2, &[]), node("b", 0.0, &["a"])],
            vec![0, 1],
        );
        let mut b = body.nodes[1].clone();
        b.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["a".into()],
            conditionals: vec![0.1, 0.6],
        }];
        let body = GraphBlockBody {
            nodes: vec![body.nodes[0].clone(), b],
            ..body
        };
        let mut evidence = HashMap::new();
        evidence.insert(0, 0.9);
        let m = recompute_marginals(&body, &topo, &evidence);
        assert!((m[0] - 0.9).abs() < 1e-9);
        assert!((m[1] - 0.55).abs() < 1e-9, "got {}", m[1]);
    }

    #[test]
    fn certainty_tier_thresholds() {
        assert_eq!(certainty_tier(0.9), "proximate");
        assert_eq!(certainty_tier(0.5), "probable");
        assert_eq!(certainty_tier(0.1), "possible");
    }
}
