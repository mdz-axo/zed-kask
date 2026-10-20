//! Composition — build event trees from market inputs and propagate prior
//! updates through the tree (Bayesian propagation). Defines the dependency,
//! composition-warning, and propagation result/value structs.
//!
//! Extracted from `superforecast.rs` (deep-module split).

use std::collections::{HashMap, HashSet};

use super::build_event_tree;
use super::convert_market_record;

use crate::types::{EventTree, ScenarioError, ScenarioEvent};
// ── Markets-set composition (T4a) ───────────────────────────────────────────

/// One caller-specified dependency edge for `compose_market_tree`.
///
/// The conditionals are caller-authored: the platform computes marginals and
/// joints but never invents the conditional probabilities themselves (the
/// never-fabricate rule applied to the composition layer).
#[derive(Debug, Clone)]
pub(crate) struct DependencySpec {
    /// Child event id (must match a converted event's `mkt-{market_id}`).
    pub child_market_id: String,
    /// Parent event ids (market ids of the conditioning markets).
    pub parent_market_ids: Vec<String>,
    /// P(child | parent truth assignment), bitmap-ordered, length
    /// 2^parent_market_ids.len().
    pub conditionals: Vec<f64>,
}

/// Maximum parents per dependency group (CPT size cap — variety amplifier iv).
/// 2^4 = 16 conditional entries per group; deeper conditioning belongs in
/// multiple groups (noisy-OR channels) or signals a misspecified tree.
pub(crate) const MAX_PARENTS_PER_GROUP: usize = 4;

/// Jaccard token-overlap threshold above which two market questions are
/// flagged as potential duplicates (same underlying event, not a dependency).
const DUPLICATE_OVERLAP_THRESHOLD: f64 = 0.65;

/// A warning emitted during composition — surfaced to the caller, never
/// silently dropped.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CompositionWarning {
    pub kind: &'static str,
    pub detail: String,
}

/// Compose a set of prediction-market records into a validated `EventTree`.
///
/// Pipeline: per-record conversion (the existing `convert_market_record`
/// gates apply to each market) → caller-specified dependency wiring →
/// overlap diagnostics → `build_event_tree` (validation, cycle detection,
/// marginalization).
///
/// Dependency inference is deliberately NOT automatic: question overlap can
/// suggest *relatedness* but cannot determine the direction or strength of
/// causation, so `depends_on` edges come from the caller's `dependency_specs`.
/// Overlap above `DUPLICATE_OVERLAP_THRESHOLD` is flagged as a likely
/// duplicate (wiring two records of the same event into a tree double-counts
/// the signal).
pub(crate) fn compose_market_tree(
    records: &[hkask_mcp_prediction_markets::types::MarketRecord],
    match_confidences: &[Option<String>],
    dependency_specs: &[DependencySpec],
) -> Result<(EventTree, Vec<CompositionWarning>), ScenarioError> {
    if records.is_empty() {
        return Err(ScenarioError::EmptyInput(
            "compose_market_tree requires at least one market record".into(),
        ));
    }
    if match_confidences.len() != records.len() {
        return Err(ScenarioError::EmptyInput(format!(
            "match_confidences length {} must equal records length {} (use None per entry for direct lookups)",
            match_confidences.len(),
            records.len()
        )));
    }

    let mut warnings: Vec<CompositionWarning> = Vec::new();

    // 1. Convert each record through the existing gated bridge.
    let mut events: Vec<ScenarioEvent> = Vec::with_capacity(records.len());
    let mut seen_ids: HashSet<String> = HashSet::new();
    for (record, confidence) in records.iter().zip(match_confidences.iter()) {
        let (event, record_warnings) = convert_market_record(record, confidence.as_deref())?;
        for warning in record_warnings {
            warnings.push(CompositionWarning {
                kind: "bridge_gate",
                detail: format!("{}: {warning}", record.market_id),
            });
        }
        if !seen_ids.insert(event.id.clone()) {
            return Err(ScenarioError::InvalidDependency(
                event.id,
                "duplicate market_id in record set — each market may appear once".into(),
            ));
        }
        events.push(event);
    }

    // 2. Wire caller-specified dependencies.
    for spec in dependency_specs {
        if spec.parent_market_ids.len() > MAX_PARENTS_PER_GROUP {
            return Err(ScenarioError::InvalidDependency(
                spec.child_market_id.clone(),
                format!(
                    "{} parents exceeds the CPT size cap of {MAX_PARENTS_PER_GROUP} — split into multiple groups or respecify the tree",
                    spec.parent_market_ids.len()
                ),
            ));
        }
        let child_id = format!("mkt-{}", spec.child_market_id);
        let parent_ids: Vec<String> = spec
            .parent_market_ids
            .iter()
            .map(|id| format!("mkt-{id}"))
            .collect();
        for parent_id in &parent_ids {
            if !seen_ids.contains(parent_id) {
                #[allow(clippy::redundant_clone)]
                // child_id is used after the loop; the clone is only redundant on this exit path
                return Err(ScenarioError::UnknownParent(
                    child_id.clone(),
                    parent_id.clone(),
                ));
            }
        }
        let child = events
            .iter_mut()
            .find(|e| e.id == child_id)
            .ok_or_else(|| ScenarioError::EventNotFound(child_id.clone()))?;
        child.depends_on.push(crate::types::EventDependency {
            parent_event_ids: parent_ids,
            conditionals: spec.conditionals.clone(),
        });
    }

    // 3. Overlap diagnostics (deterministic, matcher.rs machinery).
    for (i, a) in records.iter().enumerate() {
        for b in records.iter().skip(i + 1) {
            let overlap =
                hkask_mcp_prediction_markets::matcher::token_overlap(&a.question, &b.question);
            if overlap >= DUPLICATE_OVERLAP_THRESHOLD {
                warnings.push(CompositionWarning {
                    kind: "possible_duplicate",
                    detail: format!(
                        "questions of {} and {} overlap at {overlap:.2} — likely the same underlying event; wiring both double-counts the signal",
                        a.market_id, b.market_id
                    ),
                });
            }
        }
    }

    // 4. Build the tree (validation, cycle detection, marginalization).
    let tree = build_event_tree(&events)?;
    Ok((tree, warnings))
}

// ── Tree-level Bayesian propagation ────────────────────────────────────

/// One step in a propagation journal: a node's marginal before and after a
/// prior update elsewhere in the tree. The journal is the tâtonnement record:
/// each entry is one round of the market's one-step-ahead adjustment
/// (Bhattacharya Prop. 6, arXiv:2211.03244).
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PropagationEntry {
    pub event_id: String,
    pub marginal_before: f64,
    pub marginal_after: f64,
    pub delta: f64,
}

/// Result of updating one node's prior and propagating through the tree.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct PropagationResult {
    /// The updated tree (all marginals recomputed).
    pub tree: EventTree,
    /// Every node whose marginal changed (including the updated node itself),
    /// in topological order.
    pub journal: Vec<PropagationEntry>,
    /// Joint probability before and after.
    pub joint_before: f64,
    pub joint_after: f64,
}

/// Update one event's prior probability and propagate the change through the
/// tree: every descendant marginal and the joint are recomputed.
///
/// This closes the gap identified at territory-map C30 (scalar Bayes only):
/// `scenario_update` revises a probability in isolation; this function
/// recomputes the whole tree so downstream consumers (scenario-weighted
/// valuation, factor loadings) always read a coherent joint.
///
/// The update sets the node's *prior* (its stored `probability`); CPTs are
/// untouched — conditioning structure is caller-authored and stable under
/// evidence revision. Nodes not reachable from the updated node are
/// unaffected but are re-validated with the tree (cheap, and keeps one
/// validation path).
pub(crate) fn propagate_prior_update(
    events: &[ScenarioEvent],
    updated_event_id: &str,
    new_prior: f64,
) -> Result<PropagationResult, ScenarioError> {
    if !new_prior.is_finite() || !(0.0..=1.0).contains(&new_prior) {
        return Err(ScenarioError::InvalidProbability(
            updated_event_id.to_string(),
            new_prior,
        ));
    }

    // Baseline tree (before).
    let tree_before = build_event_tree(events)?;
    let marginal_before: HashMap<String, f64> = tree_before
        .nodes
        .iter()
        .map(|n| (n.event.id.clone(), n.marginal_probability))
        .collect();

    // Apply the prior update.
    let mut updated_events = events.to_vec();
    let target = updated_events
        .iter_mut()
        .find(|e| e.id == updated_event_id)
        .ok_or_else(|| ScenarioError::EventNotFound(updated_event_id.to_string()))?;
    target.probability = new_prior;
    target.update_count += 1;

    // Rebuilt tree (after).
    let tree_after = build_event_tree(&updated_events)?;

    let mut journal: Vec<PropagationEntry> = Vec::new();
    for node in &tree_after.nodes {
        let before = marginal_before
            .get(&node.event.id)
            .copied()
            .unwrap_or(node.marginal_probability);
        let delta = node.marginal_probability - before;
        if delta.abs() > 1e-12 {
            journal.push(PropagationEntry {
                event_id: node.event.id.clone(),
                marginal_before: before,
                marginal_after: node.marginal_probability,
                delta,
            });
        }
    }

    Ok(PropagationResult {
        joint_before: tree_before.joint_probability,
        joint_after: tree_after.joint_probability,
        tree: tree_after,
        journal,
    })
}
