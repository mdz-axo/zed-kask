//! Event-tree posterior recomputation under evidence — the shared engine.
//!
//! Promoted from `hkask-graph-widget::propagate` (scenario-server redesign
//! PR-08) so the MCP surface (`hkask-mcp-scenarios::scenario_recompute_posteriors`)
//! and the interactive widget run ONE implementation and cannot drift. The
//! forward-only path delegates to [`crate::marginalize`] and
//! [`crate::combine_independent_channels`]. Posterior factor messages carry
//! both binary masses directly, including the same noisy-OR group model,
//! to avoid cancellation on rare positive events. The widget and MCP tool
//! use this one posterior implementation rather than separate copies.
//!
//! On polytrees, two-pass factor messages exclude the receiving edge, so
//! evidence is never recycled as a fresh prior. Fractional hard observations
//! remain fixed marginal constraints; incompatible or unsatisfied evidence
//! returns an error. Multiply-connected callers retain the explicitly
//! limited [`forward_marginals`] path.

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
/// marginals from the `marginals` slice for the forward-only approximation
/// and its soft-evidence update. Exact polytree conditioning uses separate
/// edge messages in [`recompute_posteriors`].
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
/// undirected graph has no cycles). This is the structural precondition
/// for the factor-message posterior engine, not a proof that an arbitrary
/// iterative algorithm is exact. Multiply-connected callers use the
/// explicitly limited [`forward_marginals`] path.
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

/// Failure to compute a posterior is not a probability estimate.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PosteriorError {
    #[error("invalid posterior model: {0}")]
    InvalidModel(String),
    #[error("invalid posterior evidence: {0}")]
    InvalidEvidence(String),
    #[error("evidence is incompatible or numerically unresolvable under the supplied model")]
    ImpossibleEvidence,
    #[error("fixed marginal evidence could not be satisfied within the numerical iteration limit")]
    UnsatisfiedMarginals,
}

type BinaryMessage = [f64; 2];

fn normalized(values: BinaryMessage) -> Result<BinaryMessage, PosteriorError> {
    let scale = values[0].max(values[1]);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PosteriorError::ImpossibleEvidence);
    }
    let values = [values[0] / scale, values[1] / scale];
    let sum = values[0] + values[1];
    Ok([values[0] / sum, values[1] / sum])
}

/// A binary factor forest: each node contributes one conditional factor.
/// Edge messages are variable-to-factor and factor-to-variable respectively.
/// No message includes the receiving edge's contribution.
struct Polytree<'a> {
    nodes: &'a [PosteriorNode],
    edges: Vec<(usize, usize)>,
    adjacent: Vec<Vec<usize>>,
    groups: Vec<Vec<Vec<usize>>>,
    parent_edge: Vec<Option<usize>>,
    order: Vec<usize>,
}

impl<'a> Polytree<'a> {
    fn new(nodes: &'a [PosteriorNode], topo: &[usize]) -> Result<Self, PosteriorError> {
        let n = nodes.len();
        let invalid = |reason: &str| PosteriorError::InvalidModel(reason.to_owned());
        let ids: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id.as_str(), i))
            .collect();
        if ids.len() != n || topo.len() != n {
            return Err(invalid(
                "node IDs and topological positions must be unique and complete",
            ));
        }
        let mut positions = vec![usize::MAX; n];
        for (position, &index) in topo.iter().enumerate() {
            if index >= n || positions[index] != usize::MAX {
                return Err(invalid("invalid topological order"));
            }
            positions[index] = position;
        }
        let mut graph = Self {
            nodes,
            edges: Vec::new(),
            adjacent: vec![Vec::new(); 2 * n],
            groups: vec![Vec::new(); n],
            parent_edge: vec![None; 2 * n],
            order: Vec::new(),
        };
        for (i, node) in nodes.iter().enumerate() {
            if !node.base_probability.is_finite() || !(0.0..=1.0).contains(&node.base_probability) {
                return Err(invalid("base probabilities must be finite and in [0,1]"));
            }
            // The factor's first edge always addresses its child variable.
            graph.connect(i, n + i);
            let mut parents = std::collections::HashSet::new();
            for dep in &node.dependencies {
                let count = dep.parent_ids.len();
                if count == 0
                    || count > 20
                    || dep.conditionals.len() != 1usize << count
                    || dep
                        .conditionals
                        .iter()
                        .any(|p| !p.is_finite() || !(0.0..=1.0).contains(p))
                {
                    return Err(invalid(
                        "dependency groups need 1..20 distinct parents and a complete finite probability table",
                    ));
                }
                let mut group = Vec::new();
                for id in &dep.parent_ids {
                    let parent = *ids
                        .get(id.as_str())
                        .ok_or_else(|| invalid("unknown parent ID"))?;
                    if !parents.insert(id.as_str()) || positions[parent] >= positions[i] {
                        return Err(invalid(
                            "parents must be distinct across groups and precede their child",
                        ));
                    }
                    group.push(graph.connect(parent, n + i));
                }
                graph.groups[i].push(group);
            }
            let declared: std::collections::HashSet<&str> =
                node.parents.iter().map(String::as_str).collect();
            if declared.len() != node.parents.len() || declared != parents {
                return Err(invalid("parent IDs must match the conditional groups"));
            }
        }
        let mut visited = vec![false; 2 * n];
        for root in 0..2 * n {
            if visited[root] {
                continue;
            }
            visited[root] = true;
            let mut stack = vec![root];
            while let Some(vertex) = stack.pop() {
                graph.order.push(vertex);
                for &edge in &graph.adjacent[vertex] {
                    if graph.parent_edge[vertex] == Some(edge) {
                        continue;
                    }
                    let next = graph.other_end(vertex, edge);
                    if visited[next] {
                        return Err(invalid("backward inference requires a polytree"));
                    }
                    visited[next] = true;
                    graph.parent_edge[next] = Some(edge);
                    stack.push(next);
                }
            }
        }
        Ok(graph)
    }

    fn connect(&mut self, variable: usize, factor: usize) -> usize {
        let edge = self.edges.len();
        self.edges.push((variable, factor));
        self.adjacent[variable].push(edge);
        self.adjacent[factor].push(edge);
        edge
    }

    fn other_end(&self, vertex: usize, edge: usize) -> usize {
        let (variable, factor) = self.edges[edge];
        if variable == vertex { factor } else { variable }
    }

    fn variable_message(
        &self,
        variable: usize,
        omitted: Option<usize>,
        likelihoods: &[BinaryMessage],
        messages: &[[BinaryMessage; 2]],
    ) -> Result<BinaryMessage, PosteriorError> {
        let mut value = normalized(likelihoods[variable])?;
        for &edge in &self.adjacent[variable] {
            if omitted == Some(edge) {
                continue;
            }
            value = normalized([
                value[0] * messages[edge][1][0],
                value[1] * messages[edge][1][1],
            ])?;
        }
        Ok(value)
    }

    /// Sum both states directly: computing a rare true mass as `1 - false`
    /// would round it to zero and incorrectly reject possible evidence.
    fn group_distribution(
        &self,
        owner: usize,
        group: usize,
        fixed: Option<(usize, usize)>,
        messages: &[[BinaryMessage; 2]],
    ) -> Result<BinaryMessage, PosteriorError> {
        let edges = &self.groups[owner][group];
        let mut mass = [0.0, 0.0];
        for (assignment, probability) in self.nodes[owner].dependencies[group]
            .conditionals
            .iter()
            .enumerate()
        {
            if fixed.is_some_and(|(slot, state)| ((assignment >> slot) & 1) != state) {
                continue;
            }
            let weight = edges
                .iter()
                .enumerate()
                .filter(|(slot, _)| fixed.is_none_or(|(fixed_slot, _)| fixed_slot != *slot))
                .fold(1.0, |weight, (slot, &edge)| {
                    weight * messages[edge][0][(assignment >> slot) & 1]
                });
            mass[0] += weight * (1.0 - probability);
            mass[1] += weight * probability;
        }
        normalized(mass)
    }

    fn combined_groups(
        &self,
        owner: usize,
        fixed: Option<(usize, usize, usize)>,
        messages: &[[BinaryMessage; 2]],
    ) -> Result<BinaryMessage, PosteriorError> {
        let mut mass = [1.0, 0.0];
        for group in 0..self.groups[owner].len() {
            let condition = fixed
                .filter(|&(g, _, _)| g == group)
                .map(|(_, slot, state)| (slot, state));
            let channel = self.group_distribution(owner, group, condition, messages)?;
            // Noisy-OR, without subtracting an almost-one failure product.
            mass = [mass[0] * channel[0], mass[1] + mass[0] * channel[1]];
        }
        normalized(mass)
    }

    fn factor_message(
        &self,
        factor: usize,
        target: usize,
        messages: &[[BinaryMessage; 2]],
    ) -> Result<BinaryMessage, PosteriorError> {
        let owner = factor - self.nodes.len();
        if self.groups[owner].is_empty() {
            let p = self.nodes[owner].base_probability;
            return Ok([1.0 - p, p]);
        }
        let child_edge = self.adjacent[factor][0];
        if target == child_edge {
            return self.combined_groups(owner, None, messages);
        }
        for (group, edges) in self.groups[owner].iter().enumerate() {
            if let Some(slot) = edges.iter().position(|&edge| edge == target) {
                let child = messages[child_edge][0];
                if child[0] == child[1] {
                    return Ok([0.5, 0.5]);
                }
                let mut values = [0.0, 0.0];
                for (state, value) in values.iter_mut().enumerate() {
                    let mass = self.combined_groups(owner, Some((group, slot, state)), messages)?;
                    *value = child[0] * mass[0] + child[1] * mass[1];
                }
                return normalized(values);
            }
        }
        Err(PosteriorError::InvalidModel(
            "factor edge has no conditional parent".into(),
        ))
    }

    fn infer(&self, likelihoods: &[BinaryMessage]) -> Result<Vec<BinaryMessage>, PosteriorError> {
        let n = self.nodes.len();
        let mut messages = vec![[[1.0, 1.0]; 2]; self.edges.len()];
        // Collect inward, then distribute outward. A tree needs no belief
        // fixpoint: every directed edge is evaluated exactly once per pass.
        for &vertex in self.order.iter().rev() {
            if let Some(edge) = self.parent_edge[vertex] {
                messages[edge][usize::from(vertex >= n)] = if vertex < n {
                    self.variable_message(vertex, Some(edge), likelihoods, &messages)?
                } else {
                    self.factor_message(vertex, edge, &messages)?
                };
            }
        }
        for &vertex in &self.order {
            for &edge in &self.adjacent[vertex] {
                let next = self.other_end(vertex, edge);
                if self.parent_edge[next] != Some(edge) {
                    continue;
                }
                messages[edge][usize::from(vertex >= n)] = if vertex < n {
                    self.variable_message(vertex, Some(edge), likelihoods, &messages)?
                } else {
                    self.factor_message(vertex, edge, &messages)?
                };
            }
        }
        (0..n)
            .map(|i| self.variable_message(i, None, likelihoods, &messages))
            .collect()
    }
}

/// Condition a binary polytree using edge-excluding sum-product messages.
/// Hard 0/1 observations and soft likelihood ratios contribute evidence once.
/// Fractional `Hard(q)` retains the API's fixed-marginal contract: likelihood
/// weights are adjusted until all requested marginals agree within 1e-12.
/// Each adjustment rescales the current joint by target/current probability,
/// rather than feeding posterior beliefs back as fresh observations.
/// Impossible evidence or unsatisfied constraints return an explicit error.
/// Multiply-connected callers must retain the surfaced forward-only path.
pub fn recompute_posteriors(
    nodes: &[PosteriorNode],
    topo_order: &[usize],
    evidence: &HashMap<usize, Evidence>,
) -> Result<Vec<f64>, PosteriorError> {
    let graph = Polytree::new(nodes, topo_order)?;
    let mut likelihoods = vec![[1.0, 1.0]; nodes.len()];
    for (&index, &kind) in evidence {
        if index >= nodes.len() {
            return Err(PosteriorError::InvalidEvidence("unknown node index".into()));
        }
        match kind {
            Evidence::Hard(q) if q.is_finite() && (0.0..=1.0).contains(&q) => {
                if q == 0.0 || q == 1.0 {
                    likelihoods[index] = [1.0 - q, q];
                }
            }
            Evidence::Soft(lr) if lr.is_finite() && lr >= 0.0 => {
                likelihoods[index] = normalized([1.0, lr])?;
            }
            _ => {
                return Err(PosteriorError::InvalidEvidence(
                    "probabilities must be in [0,1] and likelihood ratios finite and nonnegative"
                        .into(),
                ));
            }
        }
    }
    let constraints: Vec<(usize, f64)> = topo_order
        .iter()
        .filter_map(|&i| match evidence.get(&i) {
            Some(Evidence::Hard(q)) if *q > 0.0 && *q < 1.0 => Some((i, *q)),
            _ => None,
        })
        .collect();
    let mut beliefs = graph.infer(&likelihoods)?;
    // This bounds fitting of uncertain marginal overrides, not ordinary
    // Bayesian inference (which uses exactly one two-pass tree traversal).
    const MAX_CONSTRAINT_SWEEPS: usize = 100;
    for _ in 0..MAX_CONSTRAINT_SWEEPS {
        if constraints
            .iter()
            .all(|&(i, q)| (beliefs[i][1] - q).abs() <= 1e-12)
        {
            return Ok(beliefs.iter().map(|p| p[1]).collect());
        }
        for &(i, q) in &constraints {
            if (beliefs[i][1] - q).abs() <= 1e-12 {
                continue;
            }
            if beliefs[i][0] == 0.0 || beliefs[i][1] == 0.0 {
                return Err(PosteriorError::ImpossibleEvidence);
            }
            let logs = [
                likelihoods[i][0].ln() + (1.0 - q).ln() - beliefs[i][0].ln(),
                likelihoods[i][1].ln() + q.ln() - beliefs[i][1].ln(),
            ];
            let scale = logs[0].max(logs[1]);
            likelihoods[i] = normalized([(logs[0] - scale).exp(), (logs[1] - scale).exp()])?;
            beliefs = graph.infer(&likelihoods)?;
        }
    }
    if constraints
        .iter()
        .all(|&(i, q)| (beliefs[i][1] - q).abs() <= 1e-12)
    {
        return Ok(beliefs.iter().map(|p| p[1]).collect());
    }
    Err(PosteriorError::UnsatisfiedMarginals)
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

    /// The a → b → c chain fixture — P(a)=0.5, P(b|¬a)=0.1, P(b|a)=0.6,
    /// P(c|¬b)=0.2, P(c|b)=0.7 — shared by the polytree and
    /// backward-inference tests. The two-node fixtures keep their own
    /// constructions (their priors and shapes are each test's own).
    fn chain_nodes() -> Vec<PosteriorNode> {
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.6])];
        let mut c = node("c", 0.0);
        c.parents = vec!["b".into()];
        c.dependencies = vec![dep("b", &[0.2, 0.7])];
        vec![node("a", 0.5), b, c]
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
        let chain = chain_nodes();
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

    fn two_nodes() -> Vec<PosteriorNode> {
        let mut b = node("b", 0.5);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.2, 0.8])];
        vec![node("a", 0.5), b]
    }

    #[test]
    fn observed_false_uses_complementary_likelihoods() {
        let actual = recompute_posteriors(
            &two_nodes(),
            &[0, 1],
            &HashMap::from([(1, Evidence::Hard(0.0))]),
        )
        .expect("possible false observation");
        assert!(
            (actual[0] - 0.2).abs() < 1e-12 && actual[1] == 0.0,
            "got {actual:?}"
        );
    }

    #[test]
    fn absent_evidence_preserves_priors() {
        let actual =
            recompute_posteriors(&two_nodes(), &[0, 1], &HashMap::new()).expect("no observations");
        assert!(
            (actual[0] - 0.5).abs() < 1e-12 && (actual[1] - 0.5).abs() < 1e-12,
            "got {actual:?}"
        );
    }

    #[test]
    fn fractional_observation_clamps_and_updates_ancestors() {
        let actual = recompute_posteriors(
            &two_nodes(),
            &[0, 1],
            &HashMap::from([(1, Evidence::Hard(0.9))]),
        )
        .expect("compatible marginal");
        // 0.9 * P(a|b) + 0.1 * P(a|not b) = .9*.8 + .1*.2.
        assert!(
            (actual[0] - 0.74).abs() < 1e-12 && (actual[1] - 0.9).abs() < 1e-12,
            "got {actual:?}"
        );
    }

    #[test]
    fn multiple_fixed_marginals_do_not_drift() {
        let mut nodes = two_nodes();
        let mut sibling = node("c", 0.5);
        sibling.parents = vec!["a".into()];
        sibling.dependencies = vec![dep("a", &[0.1, 0.9])];
        nodes.push(sibling);
        let actual = recompute_posteriors(
            &nodes,
            &[0, 1, 2],
            &HashMap::from([(0, Evidence::Hard(0.8)), (1, Evidence::Hard(0.6))]),
        )
        .expect("compatible marginals");
        for (actual, expected) in actual.iter().zip([0.8, 0.6, 0.74]) {
            assert!(
                (actual - expected).abs() < 1e-12,
                "got {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn rare_but_possible_evidence_is_not_rejected() {
        let mut nodes = two_nodes();
        nodes[1].dependencies[0].conditionals = vec![1e-20, 2e-20];
        let actual =
            recompute_posteriors(&nodes, &[0, 1], &HashMap::from([(1, Evidence::Hard(1.0))]))
                .expect("small positive likelihoods still define a posterior");
        assert!((actual[0] - 2.0 / 3.0).abs() < 1e-12, "got {actual:?}");
    }

    #[test]
    fn impossible_observation_is_not_a_probability() {
        let nodes = vec![node("a", 0.0)];
        assert_eq!(
            recompute_posteriors(&nodes, &[0], &HashMap::from([(0, Evidence::Hard(1.0))])),
            Err(PosteriorError::ImpossibleEvidence)
        );
    }

    #[test]
    fn incompatible_fractional_constraints_are_surfaced() {
        let mut nodes = two_nodes();
        nodes[1].dependencies[0].conditionals = vec![0.0, 1.0];
        assert!(matches!(
            recompute_posteriors(
                &nodes,
                &[0, 1],
                &HashMap::from([(0, Evidence::Hard(0.2)), (1, Evidence::Hard(0.8))])
            ),
            Err(PosteriorError::ImpossibleEvidence | PosteriorError::UnsatisfiedMarginals)
        ));
    }

    #[test]
    fn incomplete_conditional_table_is_rejected() {
        let mut nodes = two_nodes();
        nodes[1].dependencies[0].conditionals.truncate(1);
        assert!(matches!(
            recompute_posteriors(&nodes, &[0, 1], &HashMap::new()),
            Err(PosteriorError::InvalidModel(_))
        ));
    }

    /// Independent oracle: enumerate the joint product of the supplied CPTs,
    /// condition on observations, then (optionally) mix one uncertain observed
    /// partition. It does not implement message passing or constraint fitting.
    fn enumerated(nodes: &[PosteriorNode], evidence: &HashMap<usize, Evidence>) -> Vec<f64> {
        let index: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let mut joint: Vec<(usize, f64)> = (0..1usize << nodes.len())
            .map(|state| {
                let mut mass = 1.0;
                for (i, node) in nodes.iter().enumerate() {
                    let p = if node.dependencies.is_empty() {
                        node.base_probability
                    } else {
                        1.0 - node
                            .dependencies
                            .iter()
                            .map(|group| {
                                let assignment = group.parent_ids.iter().enumerate().fold(
                                    0,
                                    |bits, (slot, id)| {
                                        bits | (((state >> index[id.as_str()]) & 1) << slot)
                                    },
                                );
                                1.0 - group.conditionals[assignment]
                            })
                            .product::<f64>()
                    };
                    mass *= if (state >> i) & 1 == 1 { p } else { 1.0 - p };
                }
                for (&i, &kind) in evidence {
                    match kind {
                        Evidence::Hard(q) if q == 0.0 || q == 1.0 => {
                            if ((state >> i) & 1) as f64 != q {
                                mass = 0.0;
                            }
                        }
                        Evidence::Soft(lr) if (state >> i) & 1 == 1 => mass *= lr,
                        _ => {}
                    }
                }
                (state, mass)
            })
            .collect();
        let fractional: Vec<_> = evidence
            .iter()
            .filter_map(|(&i, &kind)| match kind {
                Evidence::Hard(q) if q > 0.0 && q < 1.0 => Some((i, q)),
                _ => None,
            })
            .collect();
        assert!(
            fractional.len() <= 1,
            "oracle intentionally covers one uncertain partition only"
        );
        if let Some(&(i, q)) = fractional.first() {
            let total: f64 = joint.iter().map(|(_, mass)| mass).sum();
            let p: f64 = joint
                .iter()
                .filter(|(state, _)| (state >> i) & 1 == 1)
                .map(|(_, mass)| mass)
                .sum::<f64>()
                / total;
            for (state, mass) in &mut joint {
                *mass *= if (*state >> i) & 1 == 1 {
                    q / p
                } else {
                    (1.0 - q) / (1.0 - p)
                };
            }
        }
        let total: f64 = joint.iter().map(|(_, mass)| mass).sum();
        (0..nodes.len())
            .map(|i| {
                joint
                    .iter()
                    .filter(|(state, _)| (state >> i) & 1 == 1)
                    .map(|(_, mass)| mass)
                    .sum::<f64>()
                    / total
            })
            .collect()
    }

    #[test]
    fn collider_observation_explains_away_without_recycling() {
        let mut c = node("c", 0.5);
        c.parents = vec!["a".into(), "b".into()];
        c.dependencies = vec![PosteriorDependency {
            parent_ids: c.parents.clone(),
            conditionals: vec![0.0, 1.0, 1.0, 1.0],
        }];
        let actual = recompute_posteriors(
            &[node("a", 0.5), node("b", 0.5), c],
            &[0, 1, 2],
            &HashMap::from([(2, Evidence::Hard(1.0))]),
        )
        .expect("OR observation is possible");
        assert!(
            (actual[0] - 2.0 / 3.0).abs() < 1e-12
                && (actual[1] - 2.0 / 3.0).abs() < 1e-12
                && actual[2] == 1.0,
            "got {actual:?}"
        );
    }

    #[test]
    fn noisy_or_groups_match_joint_enumeration() {
        let mut c = node("c", 0.5);
        c.parents = vec!["a".into(), "b".into()];
        c.dependencies = vec![dep("a", &[0.1, 0.6]), dep("b", &[0.2, 0.8])];
        let nodes = vec![node("a", 0.5), node("b", 0.4), c];
        let evidence = HashMap::from([(2, Evidence::Hard(1.0))]);
        let actual = recompute_posteriors(&nodes, &[0, 1, 2], &evidence)
            .expect("possible noisy-OR evidence");
        for (actual, expected) in actual.iter().zip(enumerated(&nodes, &evidence)) {
            assert!(
                (actual - expected).abs() < 1e-12,
                "got {actual}, expected {expected}"
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn supported_polytrees_match_independent_joint_enumeration(
            priors in proptest::array::uniform2(0.05f64..0.95),
            table in proptest::array::uniform4(0.05f64..0.95),
            left in proptest::array::uniform2(0.05f64..0.95),
            right in proptest::array::uniform2(0.05f64..0.95),
            observed in proptest::bool::ANY,
            target in 0.05f64..0.95,
            ratio in 0.1f64..10.0,
        ) {
            let mut c = node("c", 0.5);
            c.parents = vec!["a".into(), "b".into()];
            c.dependencies = vec![PosteriorDependency { parent_ids: c.parents.clone(), conditionals: table.to_vec() }];
            let mut d = node("d", 0.5); d.parents = vec!["c".into()]; d.dependencies = vec![dep("c", &left)];
            let mut e = node("e", 0.5); e.parents = vec!["c".into()]; e.dependencies = vec![dep("c", &right)];
            let nodes = vec![node("a", priors[0]), node("b", priors[1]), c, d, e];
            let evidence = HashMap::from([(0, Evidence::Hard(target)), (3, Evidence::Hard(f64::from(observed))), (4, Evidence::Soft(ratio))]);
            let actual = recompute_posteriors(&nodes, &[0,1,2,3,4], &evidence).expect("strictly positive finite model");
            let expected = enumerated(&nodes, &evidence);
            for (actual, expected) in actual.iter().zip(&expected) { proptest::prop_assert!((actual-expected).abs()<1e-10, "got {}, expected {}", actual, expected); }
            let reversed: Vec<_> = nodes.iter().rev().cloned().collect();
            let reordered_evidence = evidence.iter().map(|(&i,&v)| (4-i,v)).collect();
            let reordered = recompute_posteriors(&reversed, &[4,3,2,1,0], &reordered_evidence).expect("same model reordered");
            for (actual, reordered) in actual.iter().zip(reordered.iter().rev()) { proptest::prop_assert!((actual-reordered).abs()<1e-10); }
        }
    }

    #[test]
    fn observed_child_true_uses_evidence_once() {
        let nodes = two_nodes();
        let evidence = HashMap::from([(1, Evidence::Hard(1.0))]);
        let actual =
            recompute_posteriors(&nodes, &[0, 1], &evidence).expect("valid two-node evidence");
        // P(a | b) = (0.5 * 0.8) / (0.5 * 0.8 + 0.5 * 0.2).
        assert!((actual[0] - 0.8).abs() < 1e-12, "got {actual:?}");
    }

    #[test]
    fn backward_inference_updates_parent_on_leaf_evidence() {
        // Chain a → b → c. Priors: P(a)=0.5, P(b|¬a)=0.1, P(b|a)=0.6,
        // P(c|¬b)=0.2, P(c|b)=0.7. Forward: P(b)=0.35, P(c)=0.375.
        // Evidence c=0.9 (high) → P(b) and P(a) should both increase.
        let nodes = chain_nodes();
        let mut evidence = HashMap::new();
        evidence.insert(2, Evidence::Hard(0.9));
        let posteriors = recompute_posteriors(&nodes, &[0, 1, 2], &evidence)
            .expect("compatible polytree evidence");
        // Mix P(node | c) and P(node | not c) using the fixed .9/.1 partition.
        for (actual, expected) in posteriors.iter().zip([0.64, 0.6048, 0.9]) {
            assert!(
                (actual - expected).abs() < 1e-12,
                "got {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn backward_inference_updates_sibling_on_evidence() {
        // Branching polytree: a → b, a → c. Evidence on b updates a
        // (backward), then an edge-excluding message reaches sibling c
        // (forward). Neither child's evidence is counted a second time.
        let mut b = node("b", 0.0);
        b.parents = vec!["a".into()];
        b.dependencies = vec![dep("a", &[0.1, 0.9])];
        let mut c = node("c", 0.0);
        c.parents = vec!["a".into()];
        c.dependencies = vec![dep("a", &[0.2, 0.8])];
        let nodes = vec![node("a", 0.5), b, c];
        let mut evidence = HashMap::new();
        evidence.insert(1, Evidence::Hard(0.9));
        let posteriors = recompute_posteriors(&nodes, &[0, 1, 2], &evidence)
            .expect("compatible polytree evidence");
        for (actual, expected) in posteriors.iter().zip([0.82, 0.9, 0.692]) {
            assert!(
                (actual - expected).abs() < 1e-12,
                "got {actual}, expected {expected}"
            );
        }
    }
}
