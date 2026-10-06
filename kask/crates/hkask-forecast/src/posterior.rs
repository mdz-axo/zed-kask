//! Event-tree posterior recomputation under evidence — the shared engine.
//!
//! Promoted from `hkask-graph-widget::propagate` (scenario-server redesign
//! PR-08) so the MCP surface (`hkask-mcp-scenarios::scenario_recompute_posteriors`)
//! and the interactive widget run ONE implementation and cannot drift. The
//! joint-marginalization formula is this crate's [`crate::marginalize`]
//! (single source of truth); multi-group combination is this crate's
//! [`crate::combine_independent_channels`] — the scenarios server's
//! documented noisy-OR rule. The widget's former local product rule
//! silently disagreed with `scenario_quantify` on multi-group nodes; the
//! promotion unifies them.
//!
//! Exact on polytrees (singly-connected DAGs): there is exactly one path
//! between any two nodes, so the forward/backward fixpoint sweeps cannot
//! double-count evidence. On multiply-connected DAGs the caller must use
//! [`forward_marginals`] only — backward inference would double-count
//! evidence along multiple paths.

use std::collections::HashMap;

/// Evidence attached to one node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Evidence {
    /// Observed probability: the node's marginal is clamped to this value.
    Hard(f64),
    /// Likelihood ratio P(evidence | node=true) / P(evidence | node=false).
    /// The posterior is P' = P·LR / (P·LR + (1−P)), then propagated.
    Soft(f64),
}

impl Evidence {
    /// Apply the evidence to a prior marginal, returning the posterior.
    /// Hard evidence clamps; soft evidence applies the Bayesian update.
    #[must_use]
    pub fn apply(self, prior: f64) -> f64 {
        match self {
            Evidence::Hard(value) => value.clamp(0.0, 1.0),
            Evidence::Soft(likelihood_ratio) => {
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

/// One conditional dependency group: a parent set plus its bitmap-ordered
/// conditional table (`conditionals.len() == 2^parent_ids.len()`).
#[derive(Debug, Clone)]
pub struct PosteriorDependency {
    /// Parent node ids for this group.
    pub parent_ids: Vec<String>,
    /// P(node | parent truth assignment), bitmap-ordered (bit j ↔ parent j).
    pub conditionals: Vec<f64>,
}

/// One node of the neutral posterior tree.
#[derive(Debug, Clone)]
pub struct PosteriorNode {
    pub id: String,
    /// Root prior / fallback marginal (the caller's base value).
    pub base_probability: f64,
    /// All parent ids, deduplicated, across edge representations — the
    /// graph-structure view used for polytree detection.
    pub parents: Vec<String>,
    /// The conditional tables used for marginalization.
    pub dependencies: Vec<PosteriorDependency>,
}

/// Compute the forward marginalization for a single node, reading parent
/// marginals from the `marginals` slice. Used by soft-evidence application
/// (which needs the forward prior before applying the Bayesian update) and
/// by the fixpoint forward sweep in [`recompute_posteriors`].
///
/// Roots return their base probability. Dependents marginalize over each
/// dependency group and combine by the noisy-OR rule
/// ([`crate::combine_independent_channels`]). High-fan-in nodes (>20
/// parents in one group) fall back to the base marginal with a warn —
/// exact marginalization is O(2^n) and intractable there.
fn forward_marginal_for_node(nodes: &[PosteriorNode], idx: usize, marginals: &[f64]) -> f64 {
    let node = &nodes[idx];
    if node.parents.is_empty() || node.dependencies.is_empty() {
        return node.base_probability;
    }
    let mut group_marginals: Vec<f64> = Vec::with_capacity(node.dependencies.len());
    for dep in &node.dependencies {
        let n_parents = dep.parent_ids.len();
        if n_parents > 20 {
            tracing::warn!(
                target: "hkask.forecast",
                node_id = %node.id,
                n_parents,
                "node fan-in exceeds 20; falling back to base marginal (exact marginalization is O(2^n) and intractable here)"
            );
            return node.base_probability;
        }
        let parent_marginals: Vec<f64> = dep
            .parent_ids
            .iter()
            .map(|pid| {
                nodes
                    .iter()
                    .position(|n| n.id == *pid)
                    .and_then(|pi| marginals.get(pi).copied())
                    .unwrap_or(0.0)
            })
            .collect();
        group_marginals
            .push(crate::marginalize(&parent_marginals, &dep.conditionals).clamp(0.0, 1.0));
    }
    crate::combine_independent_channels(&group_marginals).clamp(0.0, 1.0)
}

/// Recompute every node's marginal probability given the base
/// probabilities and a set of evidence overrides. Returns marginals
/// indexed by node position.
///
/// A node in `evidence` uses its set value (hard clamps immediately; soft
/// applies a Bayesian update to the forward-computed marginal). A root
/// node uses its base probability. A dependent node marginalizes over each
/// dependency group's full joint truth-assignment space under parent
/// independence and combines groups by the noisy-OR rule — matching
/// `hkask-mcp-scenarios`' documented multi-group marginalization.
#[must_use = "marginals should be used"]
pub fn forward_marginals(
    nodes: &[PosteriorNode],
    topo_order: &[usize],
    evidence: &HashMap<usize, Evidence>,
) -> Vec<f64> {
    let n = nodes.len();
    let mut marginals = vec![0.0f64; n];
    for &idx in topo_order {
        if let Some(&kind) = evidence.get(&idx) {
            let node = &nodes[idx];
            let prior = if node.parents.is_empty() {
                node.base_probability
            } else if matches!(kind, Evidence::Hard(_)) {
                // Hard evidence clamps immediately; the forward value is
                // irrelevant.
                marginals[idx] = kind.apply(0.0);
                continue;
            } else {
                // Soft: compute the forward marginal first, then apply the
                // Bayesian update to it.
                forward_marginal_for_node(nodes, idx, &marginals)
            };
            marginals[idx] = kind.apply(prior);
            continue;
        }
        let node = &nodes[idx];
        if node.parents.is_empty() || node.dependencies.is_empty() {
            marginals[idx] = node.base_probability;
            continue;
        }
        marginals[idx] = forward_marginal_for_node(nodes, idx, &marginals);
    }
    marginals
}

/// Detect whether the DAG is a polytree (singly-connected: its underlying
/// undirected graph has no cycles). Backward inference is exact and
/// linear on polytrees; on multiply-connected DAGs it double-counts
/// evidence along multiple paths, so the caller must fall back to
/// [`forward_marginals`].
///
/// Implementation: union-find on the undirected edge set. If any edge
/// connects two nodes already in the same connected component, the
/// undirected graph has a cycle → not a polytree.
#[must_use]
pub fn is_polytree(nodes: &[PosteriorNode]) -> bool {
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            let root = find(parent, parent[x]);
            parent[x] = root;
            root
        } else {
            x
        }
    }
    let id_index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();
    for (child_idx, node) in nodes.iter().enumerate() {
        for parent_id in &node.parents {
            if let Some(&parent_idx) = id_index.get(parent_id.as_str()) {
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

/// Recompute node marginals with backward inference for polytree DAGs.
/// Evidence on a node propagates both forward to children (causal) and
/// backward to parents (diagnostic), answering the forecasting question
/// "given the leaf was observed, what's the posterior on the root?" that
/// forward-only marginalization cannot.
///
/// **Scope: polytrees only.** For multiply-connected DAGs the caller must
/// fall back to [`forward_marginals`] — this function double-counts
/// evidence along multiple paths if called on a non-polytree.
///
/// The algorithm: an initial forward pass (evidence applied), then a
/// fixpoint iteration alternating forward sweeps (re-marginalize children
/// from updated parent marginals) and backward sweeps (re-update parents
/// from updated child likelihoods) until the marginals stabilize. On a
/// polytree this converges in ≤ diameter sweeps because there is exactly
/// one path between any two nodes. Without the iteration a single
/// forward+backward pass leaves siblings of an evidence node stale.
#[must_use = "posteriors should be used"]
pub fn recompute_posteriors(
    nodes: &[PosteriorNode],
    topo_order: &[usize],
    evidence: &HashMap<usize, Evidence>,
) -> Vec<f64> {
    let n = nodes.len();
    if n == 0 {
        return Vec::new();
    }
    // Precondition: exact only on polytrees. The callers guard with
    // `is_polytree`; this debug_assert catches test-time misuse.
    debug_assert!(
        is_polytree(nodes),
        "recompute_posteriors called on a non-polytree; backward inference is exact only on polytrees"
    );
    // Forward pass: compute causal marginals with evidence applied.
    let mut marginals = forward_marginals(nodes, topo_order, evidence);

    let id_index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();

    const MAX_FIXPOINT_SWEEPS: usize = 100;
    const CONVERGENCE_EPSILON: f64 = 1e-9;
    for _sweep in 0..MAX_FIXPOINT_SWEEPS {
        let prev = marginals.clone();

        // Forward sweep: recompute children from current parent marginals.
        // Evidence nodes stay clamped; roots keep their current marginal
        // (which the backward sweep may have updated away from the static
        // prior — do not reset it).
        for &idx in topo_order {
            if evidence.contains_key(&idx) {
                continue;
            }
            let node = &nodes[idx];
            if node.parents.is_empty() || node.dependencies.is_empty() {
                continue;
            }
            marginals[idx] = forward_marginal_for_node(nodes, idx, &marginals);
        }

        // Backward sweep: re-update parents from current child marginals.
        for &idx in topo_order.iter().rev() {
            let node = &nodes[idx];
            if node.parents.is_empty() {
                continue;
            }
            for dep in &node.dependencies {
                for (k, parent_id) in dep.parent_ids.iter().enumerate() {
                    let Some(&parent_idx) = id_index.get(parent_id.as_str()) else {
                        continue;
                    };
                    let parent_prior = marginals[parent_idx];
                    if parent_prior <= 0.0 || parent_prior >= 1.0 {
                        continue;
                    }
                    let p_child_given_parent_true =
                        conditional_for_parent(dep, k, true, &marginals, &id_index);
                    let p_child_given_parent_false =
                        conditional_for_parent(dep, k, false, &marginals, &id_index);
                    let numerator = p_child_given_parent_true * parent_prior;
                    let denominator = numerator + p_child_given_parent_false * (1.0 - parent_prior);
                    if denominator > 1e-12 {
                        let posterior = (numerator / denominator).clamp(0.0, 1.0);
                        if !evidence.contains_key(&parent_idx) {
                            marginals[parent_idx] = posterior;
                        }
                    }
                }
            }
        }

        // Convergence: max abs delta across all nodes.
        let max_delta = marginals
            .iter()
            .zip(prev.iter())
            .map(|(new, old)| (new - old).abs())
            .fold(0.0_f64, f64::max);
        if max_delta < CONVERGENCE_EPSILON {
            break;
        }
    }
    marginals
}

/// Compute P(child | parent_k = value) by marginalizing the conditional
/// table over the other parents at their current marginals. For a
/// single-parent dependency, this is just `conditionals[value as usize]`.
fn conditional_for_parent(
    dep: &PosteriorDependency,
    parent_k: usize,
    parent_value: bool,
    marginals: &[f64],
    id_index: &HashMap<&str, usize>,
) -> f64 {
    let n_parents = dep.parent_ids.len();
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
        for (j, parent_id) in dep.parent_ids.iter().enumerate() {
            if j == parent_k {
                continue;
            }
            let parent_marginal = id_index
                .get(parent_id.as_str())
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

    fn node(id: &str, base: f64) -> PosteriorNode {
        PosteriorNode {
            id: id.into(),
            base_probability: base,
            parents: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    fn dep(parent: &str, conditionals: &[f64]) -> PosteriorDependency {
        PosteriorDependency {
            parent_ids: vec![parent.into()],
            conditionals: conditionals.to_vec(),
        }
    }

    #[test]
    fn root_uses_its_base_probability() {
        let nodes = vec![node("a", 0.7)];
        let m = forward_marginals(&nodes, &[0], &HashMap::new());
        assert_eq!(m, vec![0.7]);
    }

    #[test]
    fn dependent_marginalizes_over_parents() {
        // a (0.8) → b, with P(b|¬a)=0.1, P(b|a)=0.6 → P(b)=0.1*0.2 + 0.6*0.8 = 0.5
        let a = node("a", 0.8);
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.6])];
        let nodes = vec![a, b];
        let m = forward_marginals(&nodes, &[0, 1], &HashMap::new());
        assert!((m[1] - 0.5).abs() < 1e-9, "got {}", m[1]);
    }

    #[test]
    fn hard_evidence_clamps_and_propagates_to_children() {
        // a (0.2) → b [0.1, 0.6]. Base P(b)=0.1*0.8+0.6*0.2=0.2.
        // Evidence a=0.9 → P(b)=0.1*0.1+0.6*0.9=0.55.
        let a = node("a", 0.2);
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.6])];
        let nodes = vec![a, b];
        let mut evidence = HashMap::new();
        evidence.insert(0, Evidence::Hard(0.9));
        let m = forward_marginals(&nodes, &[0, 1], &evidence);
        assert!((m[0] - 0.9).abs() < 1e-9);
        assert!((m[1] - 0.55).abs() < 1e-9, "got {}", m[1]);
    }

    #[test]
    fn soft_evidence_applies_bayesian_update() {
        // LR=1.0 is a no-op; LR=3.0 on prior 0.5 yields 0.75.
        let nodes = vec![node("a", 0.5)];
        let mut evidence = HashMap::new();
        evidence.insert(0, Evidence::Soft(1.0));
        let m = forward_marginals(&nodes, &[0], &evidence);
        assert!((m[0] - 0.5).abs() < 1e-9, "LR=1.0 no-op, got {}", m[0]);
        let mut evidence = HashMap::new();
        evidence.insert(0, Evidence::Soft(3.0));
        let m = forward_marginals(&nodes, &[0], &evidence);
        assert!(
            (m[0] - 0.75).abs() < 1e-9,
            "LR=3.0 on 0.5 → 0.75, got {}",
            m[0]
        );
    }

    #[test]
    fn multi_group_combines_by_noisy_or() {
        // Node c with two single-parent groups: over a ([0.1, 0.6], P(a)=0.8
        // → 0.5) and over b ([0.2, 0.7], P(b)=0.5 → 0.45). Combined by the
        // server's documented noisy-OR rule: 1 − (1−0.5)(1−0.45) = 0.725 —
        // NOT the widget's former product (0.225), which disagreed with
        // scenario_quantify's multi-group marginalization.
        let a = node("a", 0.8);
        let b = node("b", 0.5);
        let mut c = node("c", 0.0);
        c.parents = vec!["a".into(), "b".into()];
        c.dependencies = vec![dep("a", &[0.1, 0.6]), dep("b", &[0.2, 0.7])];
        let nodes = vec![a, b, c];
        let m = forward_marginals(&nodes, &[0, 1, 2], &HashMap::new());
        assert!((m[2] - 0.725).abs() < 1e-9, "got {}", m[2]);
    }

    #[test]
    fn high_fan_in_falls_back_to_base_marginal() {
        // A node with 21 parents in one group exceeds the O(2^n) guard; the
        // engine falls back to the base marginal (warned, never silent).
        let mut nodes: Vec<PosteriorNode> = (0..21).map(|i| node(&format!("p{i}"), 0.5)).collect();
        let mut child = node("child", 0.3);
        child.parents = (0..21).map(|i| format!("p{i}")).collect();
        child.dependencies = vec![PosteriorDependency {
            parent_ids: (0..21).map(|i| format!("p{i}")).collect(),
            conditionals: Vec::new(),
        }];
        nodes.push(child);
        let topo: Vec<usize> = (0..nodes.len()).collect();
        let mut evidence = HashMap::new();
        evidence.insert(0, Evidence::Hard(0.9));
        let m = forward_marginals(&nodes, &topo, &evidence);
        let child_idx = nodes.len() - 1;
        assert!((m[child_idx] - 0.3).abs() < 1e-9, "got {}", m[child_idx]);
    }

    #[test]
    fn is_polytree_true_for_chain_false_for_diamond() {
        let chain = |nodes: Vec<PosteriorNode>| nodes;
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.6])];
        let mut c = node("c", 0.0);
        c.parents = vec!["b".into()];
        c.dependencies = vec![dep("b", &[0.2, 0.7])];
        let chain = chain(vec![node("a", 0.5), b, c]);
        assert!(is_polytree(&chain));

        // Diamond: a → b, a → c, b → d, c → d — undirected cycle.
        let mut db = node("b", 0.0);
        db.parents = vec!["a".into()];
        db.dependencies = vec![dep("a", &[0.1, 0.6])];
        let mut dc = node("c", 0.0);
        dc.parents = vec!["a".into()];
        dc.dependencies = vec![dep("a", &[0.2, 0.7])];
        let mut d = node("d", 0.0);
        d.parents = vec!["b".into(), "c".into()];
        d.dependencies = vec![dep("b", &[0.1, 0.6]), dep("c", &[0.2, 0.7])];
        let diamond = vec![node("a", 0.5), db, dc, d];
        assert!(!is_polytree(&diamond));
    }

    #[test]
    fn backward_inference_updates_parent_on_leaf_evidence() {
        // Chain a → b → c. Priors: P(a)=0.5, P(b|¬a)=0.1, P(b|a)=0.6,
        // P(c|¬b)=0.2, P(c|b)=0.7. Forward: P(b)=0.35, P(c)=0.375.
        // Evidence c=0.9 (high) → P(b) and P(a) should both increase.
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.6])];
        let mut c = node("c", 0.0);
        c.parents = vec!["b".into()];
        c.dependencies = vec![dep("b", &[0.2, 0.7])];
        let nodes = vec![node("a", 0.5), b, c];
        let mut evidence = HashMap::new();
        evidence.insert(2, Evidence::Hard(0.9));
        let posteriors = recompute_posteriors(&nodes, &[0, 1, 2], &evidence);
        assert!(
            posteriors[1] > 0.35,
            "backward inference should increase P(b) above 0.35, got {}",
            posteriors[1]
        );
        assert!(
            posteriors[0] > 0.5,
            "backward inference should increase P(a) above 0.5, got {}",
            posteriors[0]
        );
    }

    #[test]
    fn backward_inference_updates_sibling_on_evidence() {
        // Branching polytree: a → b, a → c. Evidence on b updates a
        // (backward), and the fixpoint re-propagates the updated a to c
        // (forward) — the sibling-stale bug the iteration exists to fix.
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.9])];
        let mut c = node("c", 0.0);
        c.parents = vec!["a".into()];
        c.dependencies = vec![dep("a", &[0.2, 0.8])];
        let nodes = vec![node("a", 0.5), b, c];
        let mut evidence = HashMap::new();
        evidence.insert(1, Evidence::Hard(0.9));
        let posteriors = recompute_posteriors(&nodes, &[0, 1, 2], &evidence);
        assert!(
            posteriors[0] > 0.5,
            "backward inference should increase P(a) above 0.5, got {}",
            posteriors[0]
        );
        assert!(
            posteriors[2] > 0.5,
            "fixpoint should re-propagate updated P(a) to sibling c, increasing it above 0.5, got {}",
            posteriors[2]
        );
    }
}
