//! Marginal-probability recomputation with evidence overrides.
//!
//! The engine lives in `hkask_forecast::posterior` (promoted there by the
//! scenario-server redesign, PR-08, so the MCP surface — the scenarios
//! server's `scenario_recompute_posteriors` tool — and this widget run ONE
//! implementation and cannot drift). This module is the widget-side
//! adapter: `GraphBlockBody` → the engine's neutral posterior nodes,
//! plus the block-level conditional-table validation warn (a widget
//! concern — the server validates at its own boundary).
//!
//! The promotion also unifies multi-group combination on the server's
//! documented noisy-OR rule (`hkask_forecast::combine_independent_channels`):
//! the widget's former local product rule silently disagreed with
//! `scenario_quantify` on multi-group nodes.

use std::collections::HashMap;

use hkask_forecast::posterior::{self as engine, Evidence, PosteriorDependency, PosteriorNode};

use crate::block::{EvidenceKind, GraphBlockBody};

/// Convert the parsed block body into the engine's neutral node list:
/// `base_probability` is the block's last-propagated marginal (clamped),
/// `parents` is the deduplicated parent-id set across both edge
/// representations, `dependencies` are the conditional tables.
fn neutral_nodes(body: &GraphBlockBody) -> Vec<PosteriorNode> {
    body.nodes
        .iter()
        .map(|node| PosteriorNode {
            id: node.id.clone(),
            base_probability: node.marginal_probability.unwrap_or(0.0).clamp(0.0, 1.0),
            parents: node.parent_ids(),
            dependencies: node
                .depends_on
                .iter()
                .map(|dep| PosteriorDependency {
                    parent_ids: dep.parent_event_ids.clone(),
                    conditionals: dep.conditionals.clone(),
                })
                .collect(),
        })
        .collect()
}

/// Convert the widget's evidence map to the engine's evidence.
fn neutral_evidence(evidence: &HashMap<usize, EvidenceKind>) -> HashMap<usize, Evidence> {
    evidence
        .iter()
        .map(|(&index, kind)| {
            (
                index,
                match *kind {
                    EvidenceKind::Hard(value) => Evidence::Hard(value),
                    EvidenceKind::Soft(likelihood_ratio) => Evidence::Soft(likelihood_ratio),
                },
            )
        })
        .collect()
}

/// Recompute every node's marginal probability given the current base
/// probabilities (root nodes' `marginal_probability`) and a set of evidence
/// overrides. Returns marginals indexed by node position in `body.nodes`.
///
/// A node in `evidence` uses its set value verbatim (observed). A root node
/// (no parents) uses its stored `marginal_probability`. A dependent node
/// marginalizes over the full joint truth-assignment space of each
/// `depends_on` group under parent independence and combines groups by
/// the shared noisy-OR rule — matching the server's computation exactly.
/// Delegates to `hkask_forecast::posterior::forward_marginals` (PR-08).
pub fn recompute_marginals(
    body: &GraphBlockBody,
    topo_order: &[usize],
    evidence: &HashMap<usize, EvidenceKind>,
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
    engine::forward_marginals(
        &neutral_nodes(body),
        topo_order,
        &neutral_evidence(evidence),
    )
}

/// Detect whether the DAG is a polytree (singly-connected: its underlying
/// undirected graph has no cycles). Backward inference is exact and linear
/// on polytrees; on multiply-connected DAGs it double-counts evidence
/// along multiple paths, so the caller falls back to forward-only
/// marginalization. Delegates to `hkask_forecast::posterior::is_polytree`
/// (PR-08).
pub fn is_polytree(body: &GraphBlockBody) -> bool {
    engine::is_polytree(&neutral_nodes(body))
}

/// Recompute node marginals with backward inference for polytree DAGs:
/// evidence on a node propagates both forward to children (causal) and
/// backward to parents (diagnostic), answering the forecasting question
/// "given the leaf was observed, what's the posterior on the root?" that
/// forward-only marginalization cannot.
///
/// **Scope: polytrees only.** For multiply-connected DAGs the caller must
/// fall back to [`recompute_marginals`] (forward-only) — this function
/// double-counts evidence along multiple paths if called on a
/// non-polytree.
///
/// Delegates to `hkask_forecast::posterior::recompute_posteriors` (PR-08) —
/// the same engine the scenarios server's `scenario_recompute_posteriors`
/// tool exposes.
pub fn recompute_posteriors(
    body: &GraphBlockBody,
    topo_order: &[usize],
    evidence: &HashMap<usize, EvidenceKind>,
) -> Vec<f64> {
    engine::recompute_posteriors(
        &neutral_nodes(body),
        topo_order,
        &neutral_evidence(evidence),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{DependencyBody, EvidenceKind, GraphBlockBody, NodeBody};

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
                ontology: None,
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
        evidence.insert(0, EvidenceKind::Hard(0.9));
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
            parent_event_ids: parents,
            // Full 2^21 table is intractable; the guard fires before any
            // indexing, so the table contents don't matter for this test.
            conditionals: Vec::new(),
        }];
        nodes.push(high_fan);
        let topo: Vec<usize> = (0..nodes.len()).collect();
        let (body, topo) = body(nodes, topo);
        // Set evidence on a parent so a propagated child would differ from 0.3.
        let mut evidence = HashMap::new();
        evidence.insert(0, EvidenceKind::Hard(0.9));
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
        // Combined by the shared noisy-OR rule (PR-08 engine promotion):
        // 1 − (1−0.5)(1−0.45) = 0.725 — matching scenario_quantify's
        // documented multi-group marginalization. The widget's former
        // local product rule (0.225) silently disagreed with the server's
        // own marginal for the same tree.
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
        assert!((m[2] - 0.725).abs() < 1e-9, "got {}", m[2]);
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
            ontology: None,
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
            ontology: None,
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
            ontology: None,
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
            ontology: None,
        };
        let topo = vec![0, 1, 2];
        let mut evidence = HashMap::new();
        evidence.insert(2, EvidenceKind::Hard(0.9)); // evidence on c (the leaf)
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

    #[test]
    fn soft_evidence_applies_bayesian_update() {
        // Soft evidence with LR=1.0 is a no-op; LR=3.0 on prior 0.5 yields 0.75.
        // P' = P·LR / (P·LR + (1−P)) = 0.5·3 / (0.5·3 + 0.5) = 1.5 / 2.0 = 0.75.
        let a = node("a", 0.5, &[]);
        let (body, topo) = body(vec![a], vec![0]);
        let mut evidence = HashMap::new();
        evidence.insert(0, EvidenceKind::Soft(1.0));
        let m = recompute_marginals(&body, &topo, &evidence);
        assert!((m[0] - 0.5).abs() < 1e-9, "LR=1.0 no-op, got {}", m[0]);
        let mut evidence = HashMap::new();
        evidence.insert(0, EvidenceKind::Soft(3.0));
        let m = recompute_marginals(&body, &topo, &evidence);
        assert!(
            (m[0] - 0.75).abs() < 1e-9,
            "LR=3.0 on 0.5 → 0.75, got {}",
            m[0]
        );
    }

    #[test]
    fn hard_evidence_clamps_no_regression() {
        // Hard evidence clamps the marginal to the set value (original behavior).
        let a = node("a", 0.5, &[]);
        let (body, topo) = body(vec![a], vec![0]);
        let mut evidence = HashMap::new();
        evidence.insert(0, EvidenceKind::Hard(0.9));
        let m = recompute_marginals(&body, &topo, &evidence);
        assert!((m[0] - 0.9).abs() < 1e-9, "hard clamp, got {}", m[0]);
    }

    #[test]
    fn backward_inference_updates_sibling_on_evidence() {
        // Branching polytree: a → b, a → c. Evidence on b should update a
        // (backward), and the updated a should re-propagate to c (forward
        // re-pass). Without the fixpoint iteration, c stays at its forward
        // value — the sibling-stale bug.
        // Priors: P(a)=0.5. P(b|¬a)=0.1, P(b|a)=0.9. P(c|¬a)=0.2, P(c|a)=0.8.
        // Forward: P(b) = 0.1*0.5 + 0.9*0.5 = 0.5. P(c) = 0.2*0.5 + 0.8*0.5 = 0.5.
        // Set evidence b = 0.9 (high). Backward: P(a|b=0.9) should increase
        // (b is more likely when a is true). Then forward re-pass: P(c) should
        // increase too (c is more likely when a is true, and a just went up).
        let mut a = node("a", 0.5, &[]);
        let mut b = node("b", 0.0, &["a"]);
        b.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["a".into()],
            conditionals: vec![0.1, 0.9],
        }];
        let mut c = node("c", 0.0, &["a"]);
        c.depends_on = vec![DependencyBody {
            parent_event_ids: vec!["a".into()],
            conditionals: vec![0.2, 0.8],
        }];
        let _ = &mut a;
        let body = GraphBlockBody {
            viz: Some("event_tree".into()),
            subject: None,
            joint_probability: None,
            nodes: vec![a, b, c],
            ontology: None,
        };
        let topo = vec![0, 1, 2];
        let mut evidence = HashMap::new();
        evidence.insert(1, EvidenceKind::Hard(0.9)); // evidence on b
        let posteriors = recompute_posteriors(&body, &topo, &evidence);
        // P(a) forward was 0.5. With b observed high, P(a) should increase.
        assert!(
            posteriors[0] > 0.5,
            "backward inference should increase P(a) above 0.5, got {}",
            posteriors[0]
        );
        // P(c) forward was 0.5. The fixpoint re-pass should propagate the
        // updated P(a) to c, increasing P(c). This is the assertion that
        // catches the sibling-stale bug — the old two-pass code left c at 0.5.
        assert!(
            posteriors[2] > 0.5,
            "fixpoint should re-propagate updated P(a) to sibling c, increasing it above 0.5, got {}",
            posteriors[2]
        );
    }
}
