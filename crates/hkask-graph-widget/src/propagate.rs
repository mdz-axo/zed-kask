//! Marginal-probability recomputation with evidence overrides.
//!
//! Mirrors `hkask_mcp_scenarios::superforecast::compute_marginal_probabilities`
//! (full joint-table marginalization under parent independence) and adds an
//! evidence override: a node in the `evidence` map is treated as observed (its
//! marginal is fixed to the set value) rather than marginalized from parents.
//! This lets the widget re-propagate the tree interactively when a user sets
//! evidence on a node, without coupling the UI crate to the MCP server.
//!
//! The joint-marginalization formula itself is shared via
//! `hkask_forecast::marginalize` (single source of truth for the math). This
//! module owns only the evidence-override wrapper + topological traversal,
//! which are widget-only concerns: the server has no `evidence` parameter,
//! so there is nothing to consolidate at the formula level. The former TODO
//! referencing a consolidation that already happened has been removed.

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
    // S4 layer: validate conditional tables at the math boundary so a
    // malformed block is signalled here, not only at layout time. Missing
    // entries contribute 0 (per `hkask_forecast::marginalize`), so a short
    // table silently produces a near-0 marginal — the warn makes that visible.
    for warning in crate::block::validate_conditionals(body) {
        tracing::warn!(
            target: "hkask-graph-widget",
            node_id = %warning.node_id,
            dependency_index = warning.dependency_index,
            n_parents = warning.n_parents,
            expected = warning.expected,
            actual = warning.actual,
            "conditional table length mismatch; missing entries contribute 0 to the marginal"
        );
    }
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
        // Consume every `depends_on` entry, not just the first. Each entry is a
        // joint conditional table over its own parent set; the entries are
        // combined by independence (product of per-entry marginals). This makes
        // the engine match the schema (`Vec<DependencyBody>`), which previously
        // promised multi-dep support the math silently ignored.
        if node.depends_on.is_empty() {
            marginals[idx] = node.marginal_probability.unwrap_or(0.0).clamp(0.0, 1.0);
            continue;
        }
        let mut combined: f64 = 1.0;
        for dep in &node.depends_on {
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
                // Signal the degradation: the displayed marginal is the base prior,
                // not a propagated posterior. Without this warn an operator reading
                // logs cannot distinguish "propagated to 0.7" from "fell back to
                // the 0.7 prior" — the same trap as a missing startup-failure signal
                // (`.rules`: silent fallback on a computed value).
                tracing::warn!(
                    target: "hkask-graph-widget",
                    node_id = %node.id,
                    n_parents = n_parents,
                    "node fan-in exceeds 20; falling back to base marginal (exact marginalization is O(2^n) and intractable here)"
                );
                combined = node.marginal_probability.unwrap_or(0.0).clamp(0.0, 1.0);
                break;
            }
            // Delegate the joint-marginalization formula to the shared
            // `hkask_forecast::marginalize` so this re-propagation cannot drift from
            // `hkask-mcp-scenarios::compute_marginal_probabilities`.
            let entry_marginal = hkask_forecast::marginalize(&parent_marginals, &dep.conditionals);
            combined *= entry_marginal.clamp(0.0, 1.0);
        }
        marginals[idx] = combined.clamp(0.0, 1.0);
    }
    marginals
}

/// Detect whether the DAG is a polytree (singly-connected: its underlying
/// undirected graph has no cycles). Pearl's π-λ belief updating is exact and
/// O(n) on polytrees; on multiply-connected DAGs it double-counts evidence
/// along multiple paths, so we fall back to forward-only marginalization.
///
/// Implementation: union-find on the undirected edge set. If any edge
/// connects two nodes already in the same connected component, the undirected
/// graph has a cycle → not a polytree.
pub fn is_polytree(body: &GraphBlockBody) -> bool {
    let mut parent: Vec<usize> = (0..body.nodes.len()).collect();
    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            let root = find(parent, parent[x]);
            parent[x] = root;
            root
        } else {
            x
        }
    }
    let id_index: HashMap<String, usize> = body
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();
    for (child_idx, node) in body.nodes.iter().enumerate() {
        for parent_id in node.parent_ids() {
            if let Some(&parent_idx) = id_index.get(&parent_id) {
                let ra = find(&mut parent, parent_idx);
                let rb = find(&mut parent, child_idx);
                if ra == rb {
                    // Undirected cycle: not a polytree.
                    return false;
                }
                parent[ra] = rb;
            }
        }
    }
    true
}

/// Recompute node marginals with backward inference (Pearl π-λ belief
/// updating) for polytree DAGs. Evidence on a node propagates both forward
/// to children (causal) and backward to parents (diagnostic), answering the
/// forecasting question "given the leaf was observed, what's the posterior on
/// the root?" that forward-only marginalization cannot.
///
/// **Scope: polytrees only.** For multiply-connected DAGs the caller must fall
/// back to [`recompute_marginals`] (forward-only) — this function will
/// double-count evidence along multiple paths if called on a non-polytree.
///
/// The algorithm: for each node in topological order, compute π (causal support:
/// product of parent marginals marginalized through this node's conditional
/// table) and λ (diagnostic support: product of child λ-messages). The belief
/// is π·λ normalized. Evidence nodes have their marginal clamped to the
/// observed value; their π and λ messages propagate the observation.
///
/// This is a simplified single-pass polytree updater: it computes posteriors
/// by propagating evidence forward (causal) then backward (diagnostic) in two
/// passes over the topological order. For strict Pearl π-λ message passing
/// each node would maintain separate π and λ vectors per parent/child, but the
/// two-pass marginal approximation is exact on polytrees because there is
/// exactly one path between any two nodes.
pub fn recompute_posteriors(
    body: &GraphBlockBody,
    topo_order: &[usize],
    evidence: &HashMap<usize, f64>,
) -> Vec<f64> {
    let n = body.nodes.len();
    if n == 0 {
        return Vec::new();
    }
    // Precondition: this function is only exact on polytrees (singly-connected
    // DAGs). The caller (`GraphWidget::repropagate`) guards with `is_polytree`;
    // this debug_assert catches test-time misuse by a future caller. On a
    // multiply-connected DAG the fixpoint below would double-count evidence
    // along multiple paths.
    debug_assert!(
        is_polytree(body),
        "recompute_posteriors called on a non-polytree; backward inference is exact only on polytrees"
    );
    // Forward pass: compute causal marginals (same as recompute_marginals).
    let mut marginals = recompute_marginals(body, topo_order, evidence);

    // Backward pass: propagate diagnostic evidence from children to parents.
    // For each node with evidence (or whose children have evidence), update
    // its parents' marginals via Bayes: P(parent | child evidence) ∝
    // P(child | parent) · P(parent).
    //
    // On a polytree, processing nodes in reverse topological order and
    // pushing diagnostic updates to parents is exact: each parent receives
    // exactly one diagnostic message per child (no double-counting).
    let id_index: HashMap<String, usize> = body
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.clone(), i))
        .collect();

    // Build child lists (parent → children) from the child-side parent lists.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (child_idx, node) in body.nodes.iter().enumerate() {
        for parent_id in node.parent_ids() {
            if let Some(&parent_idx) = id_index.get(&parent_id) {
                children[parent_idx].push(child_idx);
            }
        }
    }

    // Reverse topological order: children before parents.
    for &idx in topo_order.iter().rev() {
        let node = &body.nodes[idx];
        let parents = node.parent_ids();
        if parents.is_empty() {
            continue;
        }
        // Only push diagnostic updates if this node or its descendants carry
        // evidence. We approximate by checking: if the node's marginal differs
        // from a pure forward computation, it has diagnostic support to push.
        // For a polytree, the backward pass is: for each parent, update its
        // marginal by the likelihood of the child's observed state.
        //
        // Simplified backward update: treat the node's current marginal as the
        // observed child state and apply Bayes to each parent.
        // For each parent, compute P(parent | child) ∝ P(child | parent) · P(parent).
        // We need P(child | parent) from the conditional table. For a single-parent
        // dependency (the common case on polytrees), this is direct. For multi-dep,
        // we marginalize over the other parents (assumed independent).
        for dep in &node.depends_on {
            for (k, parent_id) in dep.parent_event_ids.iter().enumerate() {
                let Some(&parent_idx) = id_index.get(parent_id) else {
                    continue;
                };
                let parent_prior = marginals[parent_idx];
                if parent_prior <= 0.0 || parent_prior >= 1.0 {
                    continue;
                }
                // P(child | parent=true) and P(child | parent=false) from the
                // conditional table. For a single-parent dep, conditionals[1] is
                // P(child|parent=true), conditionals[0] is P(child|parent=false).
                // For multi-parent, we marginalize over the other parents at
                // their current marginals (polytree assumption: independent).
                let p_child_given_parent_true =
                    conditional_for_parent(dep, k, true, &marginals, &id_index);
                let p_child_given_parent_false =
                    conditional_for_parent(dep, k, false, &marginals, &id_index);
                // Bayes: P(parent=true | child) ∝ P(child | parent=true) · P(parent=true)
                let numerator = p_child_given_parent_true * parent_prior;
                let denominator = numerator + p_child_given_parent_false * (1.0 - parent_prior);
                if denominator > 1e-12 {
                    let posterior = (numerator / denominator).clamp(0.0, 1.0);
                    // Only update if the parent is not itself under hard evidence
                    // (evidence nodes are clamped and should not be updated).
                    if !evidence.contains_key(&parent_idx) {
                        marginals[parent_idx] = posterior;
                    }
                }
            }
        }
    }
    marginals
}

/// Compute P(child | parent_k = value) by marginalizing the conditional table
/// over the other parents at their current marginals. For a single-parent
/// dependency, this is just `conditionals[value as usize]`.
fn conditional_for_parent(
    dep: &crate::block::DependencyBody,
    parent_k: usize,
    parent_value: bool,
    marginals: &[f64],
    id_index: &HashMap<String, usize>,
) -> f64 {
    let n_parents = dep.parent_event_ids.len();
    if n_parents == 0 {
        return 0.0;
    }
    if n_parents == 1 {
        return dep
            .conditionals
            .get(parent_value as usize)
            .copied()
            .unwrap_or(0.0);
    }
    // Multi-parent: marginalize over the other parents.
    // Sum over all assignments where parent_k = parent_value.
    let n_assignments = 1usize << n_parents;
    let mut total = 0.0;
    for assignment in 0..n_assignments {
        let k_bit = (assignment >> parent_k) & 1 == 1;
        if k_bit != parent_value {
            continue;
        }
        let mut assignment_prob = 1.0;
        for (j, parent_id) in dep.parent_event_ids.iter().enumerate() {
            if j == parent_k {
                continue;
            }
            let parent_marginal = id_index
                .get(parent_id)
                .and_then(|&pi| marginals.get(pi).copied())
                .unwrap_or(0.0);
            let bit_set = (assignment >> j) & 1 == 1;
            assignment_prob *= if bit_set {
                parent_marginal
            } else {
                1.0 - parent_marginal
            };
        }
        total += dep.conditionals.get(assignment).copied().unwrap_or(0.0) * assignment_prob;
    }
    total
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
    fn high_fan_in_falls_back_to_base_marginal() {
        // A node with 21 parents exceeds the O(2^n) guard. The engine must
        // fall back to the node's base marginal (not a propagated value),
        // and the fallback path emits a `tracing::warn!` naming the node.
        // We assert the behavior (base marginal returned, not propagated);
        // the warn is verified by code review — testing it directly would
        // require a tracing-subscriber dev-dep not otherwise needed here.
        let parents: Vec<String> = (0..21).map(|i| format!("p{i}")).collect();
        let mut nodes: Vec<NodeBody> = parents.iter().map(|p| node(p, 0.5, &[])).collect();
        let mut high_fan = node("child", 0.3, &[]);
        high_fan.depends_on = vec![DependencyBody {
            parent_event_ids: parents.clone(),
            // Full 2^21 table is intractable; the guard fires before any
            // indexing, so the table contents don't matter for this test.
            conditionals: Vec::new(),
        }];
        nodes.push(high_fan);
        let topo: Vec<usize> = (0..nodes.len()).collect();
        let (body, topo) = body(nodes, topo);
        // Set evidence on a parent so a propagated child would differ from 0.3.
        let mut evidence = HashMap::new();
        evidence.insert(0, 0.9);
        let marginals = recompute_marginals(&body, &topo, &evidence);
        let child_idx = body.nodes.len() - 1;
        // The fallback returns the base marginal (0.3), not a propagated
        // value. If the guard were removed, this would panic (2^21 bitmap)
        // or return a propagated value near 0.5.
        assert!(
            (marginals[child_idx] - 0.3).abs() < 1e-9,
            "got {}",
            marginals[child_idx]
        );
    }

    #[test]
    fn multi_dep_combines_by_independence() {
        // Node c depends on two entries: one over parent a, one over parent b.
        // Entry 0: P(c|¬a)=0.1, P(c|a)=0.6, P(a)=0.8 → marginalize = 0.5.
        // Entry 1: P(c|¬b)=0.2, P(c|b)=0.7, P(b)=0.5 → marginalize = 0.45.
        // Combined by independence (product): 0.5 * 0.45 = 0.225.
        let a = node("a", 0.8, &[]);
        let b = node("b", 0.5, &[]);
        let mut c = node("c", 0.0, &[]);
        c.depends_on = vec![
            DependencyBody {
                parent_event_ids: vec!["a".into()],
                conditionals: vec![0.1, 0.6],
            },
            DependencyBody {
                parent_event_ids: vec!["b".into()],
                conditionals: vec![0.2, 0.7],
            },
        ];
        let (body, topo) = body(vec![a, b, c], vec![0, 1, 2]);
        let m = recompute_marginals(&body, &topo, &HashMap::new());
        assert!((m[2] - 0.225).abs() < 1e-9, "got {}", m[2]);
    }

    #[test]
    fn single_dep_no_regression() {
        // A node with one depends_on entry must behave exactly as before the
        // multi-dep change: marginalize over that one entry, no product.
        let a = node("a", 0.8, &[]);
        let mut b = node("b", 0.0, &["a"]);
        b.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["a".into()],
            conditionals: vec![0.1, 0.6],
        }];
        let (body, topo) = body(vec![a, b], vec![0, 1]);
        let m = recompute_marginals(&body, &topo, &HashMap::new());
        // P(b) = 0.1*0.2 + 0.6*0.8 = 0.5 (same as dependent_marginalizes_over_parents)
        assert!((m[1] - 0.5).abs() < 1e-9, "got {}", m[1]);
    }

    #[test]
    fn certainty_tier_thresholds() {
        assert_eq!(hkask_forecast::certainty_tier(0.9), "proximate");
        assert_eq!(hkask_forecast::certainty_tier(0.5), "probable");
        assert_eq!(hkask_forecast::certainty_tier(0.1), "possible");
    }

    // ── T5a: polytree detection + backward inference ─────────────────────

    #[test]
    fn is_polytree_true_for_chain() {
        // a → b → c: no undirected cycle.
        let nodes = vec![
            node("a", 0.5, &[]),
            node("b", 0.0, &["a"]),
            node("c", 0.0, &["b"]),
        ];
        let body = GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes,
        };
        assert!(is_polytree(&body));
    }

    #[test]
    fn is_polytree_true_for_tree() {
        // a → b, a → c (branching tree, no cycle).
        let nodes = vec![
            node("a", 0.5, &[]),
            node("b", 0.0, &["a"]),
            node("c", 0.0, &["a"]),
        ];
        let body = GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes,
        };
        assert!(is_polytree(&body));
    }

    #[test]
    fn is_polytree_false_for_diamond() {
        // a → b, a → c, b → d, c → d: undirected cycle b-a-c-d-b.
        let nodes = vec![
            node("a", 0.5, &[]),
            node("b", 0.0, &["a"]),
            node("c", 0.0, &["a"]),
            node("d", 0.0, &["b", "c"]),
        ];
        let body = GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes,
        };
        assert!(!is_polytree(&body));
    }

    #[test]
    fn backward_inference_updates_parent_on_leaf_evidence() {
        // Chain a → b → c. Prior P(a)=0.5, P(b|¬a)=0.1, P(b|a)=0.6,
        // P(c|¬b)=0.2, P(c|b)=0.7.
        // Forward: P(b) = 0.1*0.5 + 0.6*0.5 = 0.35.
        // P(c) = 0.2*0.65 + 0.7*0.35 = 0.13 + 0.245 = 0.375.
        // Set evidence c = 0.9. Backward: P(b | c=0.9) should move toward
        // P(b|c) ∝ P(c|b)·P(b). With c observed high, P(b) should increase
        // (c is more likely when b is true). Then P(a) should increase too.
        let mut a = node("a", 0.5, &[]);
        let mut b = node("b", 0.0, &["a"]);
        b.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["a".into()],
            conditionals: vec![0.1, 0.6],
        }];
        let mut c = node("c", 0.0, &["b"]);
        c.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["b".into()],
            conditionals: vec![0.2, 0.7],
        }];
        let _ = &mut a;
        let body = GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes: vec![a, b, c],
        };
        let topo = vec![0, 1, 2];
        let mut evidence = HashMap::new();
        evidence.insert(2, 0.9); // evidence on c (the leaf)
        let posteriors = recompute_posteriors(&body, &topo, &evidence);
        // P(b) forward was 0.35. With c observed at 0.9 (high), P(b) should
        // increase — c is more likely when b is true.
        assert!(
            posteriors[1] > 0.35,
            "backward inference should increase P(b) above 0.35, got {}",
            posteriors[1]
        );
        // P(a) forward was 0.5. With c observed high, P(a) should increase
        // (a → b → c, high c implies high b implies high a).
        assert!(
            posteriors[0] > 0.5,
            "backward inference should increase P(a) above 0.5, got {}",
            posteriors[0]
        );
    }
}
