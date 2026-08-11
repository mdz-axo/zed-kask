//! Composition — build event trees from market/CMP inputs and propagate prior
//! updates through the tree (Bayesian propagation). Defines the dependency,
//! composition-warning, and propagation result/value structs.
//!
//! Extracted from `superforecast.rs` (deep-module split).

use std::collections::{HashMap, HashSet};

use super::ForecastStore;
use super::build_event_tree;
use super::convert_market_record;

use crate::types::{EventTree, ScenarioError, ScenarioEvent, ScenarioType, TimeHorizon};
// ── Markets-set composition (T4a) ───────────────────────────────────────────

/// One caller-specified dependency edge for `compose_market_tree`.
///
/// The conditionals are caller-authored: the platform computes marginals and
/// joints but never invents the conditional probabilities themselves (the
/// never-fabricate rule applied to the composition layer).
#[derive(Debug, Clone)]
pub struct DependencySpec {
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
pub const MAX_PARENTS_PER_GROUP: usize = 4;

/// Jaccard token-overlap threshold above which two market questions are
/// flagged as potential duplicates (same underlying event, not a dependency).
const DUPLICATE_OVERLAP_THRESHOLD: f64 = 0.65;

/// A warning emitted during composition — surfaced to the caller, never
/// silently dropped.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompositionWarning {
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
pub fn compose_market_tree(
    records: &[hkask_mcp_prediction_markets::types::MarketRecord],
    match_confidences: &[Option<String>],
    dependency_specs: &[DependencySpec],
    store: Option<&ForecastStore>,
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
        let (event, record_warnings) = convert_market_record(record, confidence.as_deref(), store)?;
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

// ── R1: Composition over CMP inputs ─────────────────────────────────────────
//
// Re-points the composition machinery at CMP index probabilities instead of
// raw contract probabilities. A CMP index is a constant-maturity, constant-
// orientation synthetic portfolio — its probability is controlled (the time
// axis is taken out), so it's the right input for scenario trees. The tree
// cites the index (family, orientation, tenor, venue), not a decaying contract.

/// Convert a CMP index into a ScenarioEvent for tree composition.
///
/// The CMP index probability becomes the event's prior. Unlike
/// `convert_market_record`, no domain-bias correction or reliability-tier
/// gating is applied — the CMP index is already a controlled, portfolio-weighted
/// probability with its own reliability floor and construction method surfaced
/// in the provenance. The event ID is `cmp:{family}:{tenor}:{orientation}` —
/// the index identity, not a decaying contract ID.
///
/// `observation_date` is the date the CMP index was built (the "today" of the
/// index). The event deadline is `observation_date + target_maturity_days` —
/// the honest deadline for the constant-maturity target, not a fabricated
/// placeholder.
pub fn convert_cmp_index(
    index: &hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex,
    observation_date: chrono::NaiveDate,
) -> ScenarioEvent {
    use hkask_mcp_prediction_markets::cmp::CmpMethod;

    let family_label = index.family.label();
    let tenor = index.index.bucket.label();
    let orientation = index.index.orientation.to_string();
    let venue = index.venue.to_string();
    let method = match index.index.portfolio.method {
        CmpMethod::Interpolated => "interpolated",
        CmpMethod::BucketedSparse => "bucketed_sparse",
    };
    let id = format!("cmp:{family_label}:{tenor}:{orientation}");
    let name = format!("{family_label} {tenor} {orientation} ({venue}, {method})");
    let question = format!(
        "CMP index: {family_label} {orientation} at {tenor} forward maturity, \
         venue={venue}, method={method}, p={:.3}, maturity_error={:.1}d, constituents={}",
        index.index.portfolio.index_probability,
        index.index.portfolio.maturity_error_days,
        index.index.portfolio.constituents.len()
    );
    // The deadline is the observation date + the target maturity — the honest
    // deadline for the constant-maturity target. The CMP index is a rolling
    // synthetic, so this deadline advances with each observation.
    let target_days = index.index.bucket.target_days() as i64;
    let deadline = observation_date + chrono::Duration::days(target_days);
    let probability = index.index.portfolio.index_probability;
    ScenarioEvent {
        id,
        name,
        question,
        deadline,
        time_horizon: TimeHorizon::Strategic,
        scenario_type: ScenarioType::EmergingEconomic,
        subject: family_label.to_string(),
        probability,
        basis: Some(format!("cmp_index:{method}")),
        depends_on: vec![],
        sub_questions: vec![],
        base_rate: Some(probability),
        reference_class: Some(format!(
            "CMP {family_label} {orientation} {tenor} ({venue}); \
             method={method}, maturity_error={:.1}d, constituents={}",
            index.index.portfolio.maturity_error_days,
            index.index.portfolio.constituents.len()
        )),
        brier_score: None,
        update_count: 0,
    }
}

/// Compose a set of CMP indices into an EventTree (R1).
///
/// Each CMP index becomes a root ScenarioEvent with its index probability as
/// the prior. The tree cites the index (family, orientation, tenor, venue) in
/// the provenance — not a decaying contract. This is the re-pointed composition
/// path: same tree machinery, CMP-controlled inputs.
///
/// `observation_date` is the date the CMP indices were built. Each event's
/// deadline is `observation_date + target_maturity_days` — the honest deadline.
///
/// CMP indices are independent root events (no caller-authored dependencies
/// in the initial implementation — the tree is a flat set of CMP priors).
/// Dependency edges between CMP indices (e.g. "oil price increase → inflation
/// increase") are a future refinement (R5 coherence analysis); for now the
/// tree is a flat prior set that downstream tools (scenario_analysis,
/// scenario_propagate) consume.
pub fn compose_cmp_tree(
    indices: &[hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex],
    observation_date: chrono::NaiveDate,
) -> Result<EventTree, ScenarioError> {
    if indices.is_empty() {
        return Err(ScenarioError::EmptyInput(
            "compose_cmp_tree requires at least one CMP index".into(),
        ));
    }
    let events: Vec<ScenarioEvent> = indices
        .iter()
        .map(|idx| convert_cmp_index(idx, observation_date))
        .collect();
    // Check for duplicate IDs (same family/tenor/orientation from different venues).
    let mut seen = HashSet::new();
    for event in &events {
        if !seen.insert(event.id.clone()) {
            return Err(ScenarioError::InvalidDependency(
                event.id.clone(),
                "duplicate CMP index (same family/tenor/orientation) — \
                 merge venues or filter before composing"
                    .into(),
            ));
        }
    }
    build_event_tree(&events)
}

/// A caller-authored dependency edge between CMP indices.
///
/// `child_id` and `parent_ids` use the CMP index ID format
/// `cmp:{family}:{tenor}:{orientation}` — the same format `convert_cmp_index`
/// generates. The caller identifies the CMP indices by their (family, tenor,
/// orientation) triple and supplies the conditional probability table.
///
/// The conditionals are P(child | parent truth assignment), bitmap-ordered,
/// length 2^parent_ids.len(). The server validates structure but never invents
/// conditional probabilities — the caller authors them.
#[derive(Debug, Clone)]
pub struct CmpDependencySpec {
    /// The child CMP index ID: `cmp:{family}:{tenor}:{orientation}`.
    pub child_id: String,
    /// The parent CMP index IDs.
    pub parent_ids: Vec<String>,
    /// P(child | parent truth assignment), bitmap-ordered.
    pub conditionals: Vec<f64>,
}

/// Compose a set of CMP indices into an EventTree with caller-authored
/// dependency edges (R1 + H3 joint coherence support).
///
/// This is the extended version of `compose_cmp_tree` that supports dependency
/// edges between CMP indices — e.g. "oil price increase → inflation increase."
/// The dependency edges enable the H3 joint coherence test: the tree-implied
/// joint P(A ∧ B) can be compared against a parlay contract price.
///
/// `observation_date` is the date the CMP indices were built.
/// `dependency_specs` are caller-authored edges between CMP index IDs. Omit for
/// a flat (independent) tree — same as `compose_cmp_tree`.
pub fn compose_cmp_tree_with_deps(
    indices: &[hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex],
    observation_date: chrono::NaiveDate,
    dependency_specs: &[CmpDependencySpec],
) -> Result<EventTree, ScenarioError> {
    if indices.is_empty() {
        return Err(ScenarioError::EmptyInput(
            "compose_cmp_tree_with_deps requires at least one CMP index".into(),
        ));
    }
    let mut events: Vec<ScenarioEvent> = indices
        .iter()
        .map(|idx| convert_cmp_index(idx, observation_date))
        .collect();
    // Check for duplicate IDs.
    let seen: HashSet<String> = events.iter().map(|e| e.id.clone()).collect();
    // Wire caller-specified dependencies.
    for spec in dependency_specs {
        if spec.parent_ids.len() > MAX_PARENTS_PER_GROUP {
            return Err(ScenarioError::InvalidDependency(
                spec.child_id.clone(),
                format!(
                    "{} parents exceeds the CPT size cap of {MAX_PARENTS_PER_GROUP}",
                    spec.parent_ids.len()
                ),
            ));
        }
        for parent_id in &spec.parent_ids {
            if !seen.contains(parent_id) {
                return Err(ScenarioError::UnknownParent(
                    spec.child_id.clone(),
                    parent_id.clone(),
                ));
            }
        }
        let child = events
            .iter_mut()
            .find(|e| e.id == spec.child_id)
            .ok_or_else(|| ScenarioError::EventNotFound(spec.child_id.clone()))?;
        child.depends_on.push(crate::types::EventDependency {
            parent_event_ids: spec.parent_ids.clone(),
            conditionals: spec.conditionals.clone(),
        });
    }
    build_event_tree(&events)
}

// ── Tree-level Bayesian propagation (T5) ────────────────────────────────────

/// One step in a propagation journal: a node's marginal before and after a
/// prior update elsewhere in the tree. The journal is the tâtonnement record
/// (T10): each entry is one round of the market's one-step-ahead adjustment
/// (Bhattacharya Prop. 6, arXiv:2211.03244 — see t0-keystone-mapping.md §3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PropagationEntry {
    pub event_id: String,
    pub marginal_before: f64,
    pub marginal_after: f64,
    pub delta: f64,
}

/// Result of updating one node's prior and propagating through the tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PropagationResult {
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
pub fn propagate_prior_update(
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

