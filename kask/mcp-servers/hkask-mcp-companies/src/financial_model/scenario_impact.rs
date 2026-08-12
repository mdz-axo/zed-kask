//! Scenario impact valuation: compose a company's financial forecast from
//! the impact of scenario event-tree nodes on DCF assumptions.
//!
//! This replaces the deprecated `scenario_from_companies` (which converted DCF
//! output into scenario events — the wrong direction). Here, an exogenous scenario event tree —
//! built from research, prediction markets, or brainstorming — is the
//! driver, and the company's financial forecast is the system being
//! impacted. The user maps each scenario node's Yes/No outcome to additive
//! deltas on the company's DCF assumptions (revenue growth, gross margin,
//! capex, etc.). The tool enumerates all 2^N leaf paths through the tree,
//! computes each path's probability from the CPTs, applies the stacked
//! deltas, runs DCF under each modified assumption set, and weights by
//! path probability.
//!
//! # Path probability computation
//!
//! For root nodes (no `depends_on`), P(Yes) = `marginal_probability`.
//! For dependent nodes, P(Yes | parents' outcomes in this path) is computed
//! from the conditional probability tables — the same CPTs that
//! `scenario_quantify` emits. Multiple dependency entries combine by
//! independence (product), matching `combine_tree_probabilities`.
//!
//! # Per-node delta mapping
//!
//! Each scenario node carries `yes_deltas` and `no_deltas` — additive
//! changes to DCF assumptions. Deltas stack additively across all nodes
//! in a path. Modified assumptions are clamped to valid ranges before
//! running the DCF.

use super::{HistoricalSnapshot, ProjectionAssumptions, project_model};
use serde::{Deserialize, Serialize};

/// Maximum number of scenario nodes (2^N path enumeration limit).
/// 12 nodes = 4096 DCF runs — each is a closed-form 10-year projection.
pub const MAX_SCENARIO_NODES: usize = 12;

// ── Input types (deserialized from the tool request) ───────────────────────

/// Additive deltas on DCF projection assumptions.
/// Applied additively: `modified = base + delta`.
/// All fields optional — omitted deltas are zero.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AssumptionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revenue_growth_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gross_margin_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub da_to_revenue_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capex_to_revenue_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nwc_to_revenue_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_rate_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discount_rate_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_growth_delta: Option<f64>,
}

/// Per-node impact mapping: how a scenario node's outcome changes the
/// company's financial assumptions.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioNodeImpact {
    /// Scenario node ID (must match a node in the event tree).
    pub node_id: String,
    /// Deltas applied when this node resolves Yes.
    #[serde(default)]
    pub yes_deltas: AssumptionDelta,
    /// Deltas applied when this node resolves No (default: zero).
    #[serde(default)]
    pub no_deltas: AssumptionDelta,
}

/// A scenario tree node parsed from the `scenario_quantify` output JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioTreeNode {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub marginal_probability: f64,
    #[serde(default)]
    pub depends_on: Vec<ScenarioTreeDependency>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioTreeDependency {
    pub parent_event_ids: Vec<String>,
    pub conditionals: Vec<f64>,
}

/// Parsed scenario tree from `scenario_quantify` output.
#[derive(Debug, Clone, Deserialize)]
pub struct ScenarioTreeInput {
    pub nodes: Vec<ScenarioTreeNode>,
    #[serde(default)]
    pub topological_order: Vec<String>,
}

// ── Result types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScenarioImpactResult {
    pub base_intrinsic: f64,
    pub probability_weighted_intrinsic: f64,
    pub total_probability: f64,
    pub path_count: usize,
    pub paths: Vec<PathResult>,
    pub node_sensitivities: Vec<NodeSensitivity>,
    pub distribution: ImpactDistribution,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathResult {
    pub path_mask: usize,
    pub probability: f64,
    pub intrinsic_per_share: f64,
    pub applied_growth: f64,
    pub applied_margin: f64,
    pub outcomes: Vec<PathOutcome>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathOutcome {
    pub node_id: String,
    pub outcome: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeSensitivity {
    pub node_id: String,
    pub node_name: Option<String>,
    /// E[intrinsic | node = Yes] — probability-weighted intrinsic across
    /// all paths where this node is Yes.
    pub intrinsic_if_yes: f64,
    /// E[intrinsic | node = No]
    pub intrinsic_if_no: f64,
    /// |intrinsic_if_yes - intrinsic_if_no| — how much this node's
    /// resolution moves the valuation.
    pub sensitivity: f64,
    /// Marginal probability of Yes (from the tree).
    pub marginal_probability: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactDistribution {
    pub min: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub max: f64,
    pub prob_undervalued: f64,
}

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, thiserror::Error)]
pub enum ScenarioImpactError {
    #[error("no scenario nodes provided")]
    NoNodes,
    #[error("too many scenario nodes: {0} (max {MAX_SCENARIO_NODES})")]
    TooManyNodes(usize),
    #[error("node '{0}' not found in topological order")]
    NodeNotInTopoOrder(String),
    #[error("duplicate node id '{0}'")]
    DuplicateNodeId(String),
    #[error("node '{0}' in topological order not found in nodes")]
    TopoNodeMissing(String),
    #[error("impact mapping references unknown node '{0}'")]
    UnknownImpactNode(String),
    #[error("duplicate impact mapping for node '{0}'")]
    DuplicateImpactNode(String),
    #[error("conditional probability table for node '{0}' has {1} entries, expected {2}")]
    InvalidCptLength(String, usize, usize),
    #[error("node '{0}' depends on unknown parent node '{1}'")]
    UnknownParentNode(String, String),
    #[error(
        "topological order fallback invalid: node '{0}' depends on '{1}' but appears before it in the node array — provide topological_order explicitly"
    )]
    InvalidTopoOrder(String, String),
    #[error("all path probabilities are zero")]
    ZeroProbability,
}

// ── Core computation ───────────────────────────────────────────────────────

pub fn scenario_impact_dcf(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    tree: &ScenarioTreeInput,
    impacts: &[ScenarioNodeImpact],
    current_price: f64,
) -> Result<ScenarioImpactResult, ScenarioImpactError> {
    let nodes = &tree.nodes;
    let n = nodes.len();
    if n == 0 {
        return Err(ScenarioImpactError::NoNodes);
    }
    if n > MAX_SCENARIO_NODES {
        return Err(ScenarioImpactError::TooManyNodes(n));
    }

    // Use topological order if provided, otherwise use node array order.
    // When falling back, validate that parents appear before children
    // (required for correct CPT-based path probability computation).
    let topo_order: Vec<String> = if tree.topological_order.is_empty() {
        let fallback: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
        let position: std::collections::HashMap<&str, usize> = fallback
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        for node in nodes {
            for dep in &node.depends_on {
                for parent_id in &dep.parent_event_ids {
                    let parent_pos = position
                        .get(parent_id.as_str())
                        .copied()
                        .unwrap_or(usize::MAX);
                    let child_pos = position
                        .get(node.id.as_str())
                        .copied()
                        .unwrap_or(usize::MAX);
                    if parent_pos >= child_pos {
                        return Err(ScenarioImpactError::InvalidTopoOrder(
                            node.id.clone(),
                            parent_id.clone(),
                        ));
                    }
                }
            }
        }
        fallback
    } else {
        tree.topological_order.clone()
    };

    // Validate topological order references.
    let mut node_map: std::collections::HashMap<&str, &ScenarioTreeNode> =
        std::collections::HashMap::with_capacity(nodes.len());
    for node in nodes {
        if node_map.insert(node.id.as_str(), node).is_some() {
            return Err(ScenarioImpactError::DuplicateNodeId(node.id.clone()));
        }
    }
    for id in &topo_order {
        if !node_map.contains_key(id.as_str()) {
            return Err(ScenarioImpactError::TopoNodeMissing(id.clone()));
        }
    }
    for node in nodes {
        if !topo_order.iter().any(|id| id == &node.id) {
            return Err(ScenarioImpactError::NodeNotInTopoOrder(node.id.clone()));
        }
    }

    // Validate CPT lengths and impact mapping references.
    for node in nodes {
        for dep in &node.depends_on {
            let expected = 1usize << dep.parent_event_ids.len();
            if dep.conditionals.len() != expected {
                return Err(ScenarioImpactError::InvalidCptLength(
                    node.id.clone(),
                    dep.conditionals.len(),
                    expected,
                ));
            }
            // Validate that all parent IDs exist in the node set.
            for parent_id in &dep.parent_event_ids {
                if !node_map.contains_key(parent_id.as_str()) {
                    return Err(ScenarioImpactError::UnknownParentNode(
                        node.id.clone(),
                        parent_id.clone(),
                    ));
                }
            }
        }
    }
    let mut impact_map: std::collections::HashMap<&str, &ScenarioNodeImpact> =
        std::collections::HashMap::with_capacity(impacts.len());
    for impact in impacts {
        if impact_map.insert(impact.node_id.as_str(), impact).is_some() {
            return Err(ScenarioImpactError::DuplicateImpactNode(
                impact.node_id.clone(),
            ));
        }
    }
    for impact in impacts {
        if !node_map.contains_key(impact.node_id.as_str()) {
            return Err(ScenarioImpactError::UnknownImpactNode(
                impact.node_id.clone(),
            ));
        }
    }

    // Bit position for each node ID (position in the nodes array).
    let bit_positions: std::collections::HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    // Base case DCF (no deltas applied).
    let base_model = project_model(hist, base_assumptions, current_price);
    let base_intrinsic = base_model.intrinsic_per_share;

    // Enumerate all 2^n leaf paths.
    let n_paths = 1usize << n;
    let mut path_results: Vec<PathResult> = Vec::with_capacity(n_paths);
    let mut total_prob: f64 = 0.0;

    for path_mask in 0..n_paths {
        // Compute path probability using CPTs (topological order).
        let path_prob =
            compute_path_probability(&topo_order, &node_map, &bit_positions, path_mask)?;

        if path_prob <= 0.0 {
            continue;
        }
        total_prob += path_prob;

        // Apply stacked deltas for this path.
        let mut modified = base_assumptions.clone();
        for (i, node) in nodes.iter().enumerate() {
            let is_yes = (path_mask >> i) & 1 == 1;
            if let Some(impact) = impact_map.get(node.id.as_str()) {
                let deltas = if is_yes {
                    &impact.yes_deltas
                } else {
                    &impact.no_deltas
                };
                apply_delta(&mut modified, deltas);
            }
        }
        clamp_assumptions(&mut modified);

        let model = project_model(hist, &modified, current_price);

        let outcomes: Vec<PathOutcome> = nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let is_yes = (path_mask >> i) & 1 == 1;
                PathOutcome {
                    node_id: node.id.clone(),
                    outcome: is_yes,
                }
            })
            .collect();

        path_results.push(PathResult {
            path_mask,
            probability: path_prob,
            intrinsic_per_share: model.intrinsic_per_share,
            applied_growth: modified.revenue_growth,
            applied_margin: modified.gross_margin,
            outcomes,
        });
    }

    if total_prob <= 0.0 {
        return Err(ScenarioImpactError::ZeroProbability);
    }

    // Probability-weighted intrinsic value.
    let probability_weighted_intrinsic: f64 = path_results
        .iter()
        .map(|p| p.probability * p.intrinsic_per_share)
        .sum::<f64>()
        / total_prob;

    // Per-node sensitivity: E[intrinsic | Yes] vs E[intrinsic | No].
    let mut node_sensitivities = Vec::with_capacity(n);
    for (i, node) in nodes.iter().enumerate() {
        let mut yes_weight: f64 = 0.0;
        let mut yes_intrinsic: f64 = 0.0;
        let mut no_weight: f64 = 0.0;
        let mut no_intrinsic: f64 = 0.0;

        for path in &path_results {
            let is_yes = (path.path_mask >> i) & 1 == 1;
            if is_yes {
                yes_weight += path.probability;
                yes_intrinsic += path.probability * path.intrinsic_per_share;
            } else {
                no_weight += path.probability;
                no_intrinsic += path.probability * path.intrinsic_per_share;
            }
        }

        let intrinsic_if_yes = if yes_weight > 0.0 {
            yes_intrinsic / yes_weight
        } else {
            0.0
        };
        let intrinsic_if_no = if no_weight > 0.0 {
            no_intrinsic / no_weight
        } else {
            0.0
        };

        // Sensitivity is meaningful only when both outcomes have probability
        // mass. If one outcome is impossible (P=0), the node has no impact on
        // valuation uncertainty.
        let sensitivity = if yes_weight > 0.0 && no_weight > 0.0 {
            (intrinsic_if_yes - intrinsic_if_no).abs()
        } else {
            0.0
        };

        node_sensitivities.push(NodeSensitivity {
            node_id: node.id.clone(),
            node_name: node.name.clone(),
            intrinsic_if_yes,
            intrinsic_if_no,
            sensitivity,
            marginal_probability: node.marginal_probability,
        });
    }

    // Sort sensitivities by descending impact.
    node_sensitivities.sort_by(|a, b| {
        b.sensitivity
            .partial_cmp(&a.sensitivity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Distribution statistics.
    let distribution = compute_distribution(&path_results, current_price, total_prob);

    Ok(ScenarioImpactResult {
        base_intrinsic,
        probability_weighted_intrinsic,
        total_probability: total_prob,
        path_count: path_results.len(),
        paths: path_results,
        node_sensitivities,
        distribution,
    })
}

/// Compute the probability of a specific leaf path through the event tree.
///
/// For root nodes (no `depends_on`): P(Yes) = `marginal_probability`.
/// For dependent nodes: P(Yes | parents' outcomes in this path) is computed
/// from the CPTs. Multiple dependency entries combine by independence
/// (product), matching `combine_tree_probabilities`.
fn compute_path_probability(
    topo_order: &[String],
    node_map: &std::collections::HashMap<&str, &ScenarioTreeNode>,
    bit_positions: &std::collections::HashMap<&str, usize>,
    path_mask: usize,
) -> Result<f64, ScenarioImpactError> {
    let mut path_prob: f64 = 1.0;

    for node_id in topo_order {
        let node = node_map
            .get(node_id.as_str())
            .ok_or_else(|| ScenarioImpactError::TopoNodeMissing(node_id.clone()))?;
        let bit = *bit_positions
            .get(node_id.as_str())
            .ok_or_else(|| ScenarioImpactError::NodeNotInTopoOrder(node_id.clone()))?;
        let is_yes = (path_mask >> bit) & 1 == 1;

        let p_yes = if node.depends_on.is_empty() {
            node.marginal_probability
        } else {
            let mut combined = 1.0_f64;
            for dep in &node.depends_on {
                let mut bitmap = 0usize;
                for (j, parent_id) in dep.parent_event_ids.iter().enumerate() {
                    let parent_bit = *bit_positions.get(parent_id.as_str()).ok_or_else(|| {
                        ScenarioImpactError::NodeNotInTopoOrder(parent_id.clone())
                    })?;
                    if (path_mask >> parent_bit) & 1 == 1 {
                        bitmap |= 1 << j;
                    }
                }
                let conditional = dep.conditionals.get(bitmap).copied().unwrap_or(0.0);
                combined *= conditional;
            }
            combined.clamp(0.0, 1.0)
        };

        let p_outcome = if is_yes { p_yes } else { 1.0 - p_yes };
        path_prob *= p_outcome;
    }

    Ok(path_prob)
}

fn apply_delta(assumptions: &mut ProjectionAssumptions, delta: &AssumptionDelta) {
    if let Some(d) = delta.revenue_growth_delta {
        assumptions.revenue_growth += d;
    }
    if let Some(d) = delta.gross_margin_delta {
        assumptions.gross_margin += d;
    }
    if let Some(d) = delta.da_to_revenue_delta {
        assumptions.da_to_revenue += d;
    }
    if let Some(d) = delta.capex_to_revenue_delta {
        assumptions.capex_to_revenue += d;
    }
    if let Some(d) = delta.nwc_to_revenue_delta {
        assumptions.nwc_to_revenue += d;
    }
    if let Some(d) = delta.tax_rate_delta {
        assumptions.tax_rate += d;
    }
    if let Some(d) = delta.discount_rate_delta {
        assumptions.discount_rate += d;
    }
    if let Some(d) = delta.terminal_growth_delta {
        assumptions.terminal_growth += d;
    }
}

fn clamp_assumptions(assumptions: &mut ProjectionAssumptions) {
    assumptions.revenue_growth = assumptions.revenue_growth.clamp(-0.50, 1.00);
    assumptions.gross_margin = assumptions.gross_margin.clamp(0.01, 0.80);
    assumptions.da_to_revenue = assumptions.da_to_revenue.clamp(0.00, 0.20);
    assumptions.capex_to_revenue = assumptions.capex_to_revenue.clamp(0.00, 0.30);
    assumptions.nwc_to_revenue = assumptions.nwc_to_revenue.clamp(-0.20, 0.50);
    assumptions.tax_rate = assumptions.tax_rate.clamp(0.00, 1.00);
    assumptions.discount_rate = assumptions.discount_rate.clamp(0.05, 0.30);
    assumptions.terminal_growth = assumptions.terminal_growth.clamp(0.00, 0.10);
    // Guard against division by zero in the terminal value formula
    // (project_model divides by discount_rate - terminal_growth).
    if assumptions.terminal_growth >= assumptions.discount_rate {
        assumptions.terminal_growth = assumptions.discount_rate * 0.5;
    }
}

fn compute_distribution(
    paths: &[PathResult],
    current_price: f64,
    total_prob: f64,
) -> ImpactDistribution {
    let mut sorted: Vec<(f64, f64)> = paths
        .iter()
        .map(|p| (p.probability / total_prob, p.intrinsic_per_share))
        .collect();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let min = sorted.first().map(|(_, v)| *v).unwrap_or(0.0);
    let max = sorted.last().map(|(_, v)| *v).unwrap_or(0.0);

    let percentile = |p: f64| -> f64 {
        let mut cumulative: f64 = 0.0;
        for (weight, value) in &sorted {
            cumulative += weight;
            if cumulative >= p {
                return *value;
            }
        }
        max
    };

    let prob_undervalued: f64 = paths
        .iter()
        .filter(|p| p.intrinsic_per_share > current_price)
        .map(|p| p.probability / total_prob)
        .sum();

    ImpactDistribution {
        min,
        p10: percentile(0.10),
        p25: percentile(0.25),
        median: percentile(0.50),
        p75: percentile(0.75),
        p90: percentile(0.90),
        max,
        prob_undervalued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hist() -> HistoricalSnapshot {
        HistoricalSnapshot {
            revenue: vec![
                ("2022".into(), 80_000.0),
                ("2023".into(), 90_000.0),
                ("2024".into(), 100_000.0),
            ],
            cogs: vec![
                ("2022".into(), 48_000.0),
                ("2023".into(), 54_000.0),
                ("2024".into(), 60_000.0),
            ],
            da: vec![
                ("2022".into(), 3_000.0),
                ("2023".into(), 3_200.0),
                ("2024".into(), 3_500.0),
            ],
            capex: vec![
                ("2022".into(), 2_500.0),
                ("2023".into(), 2_800.0),
                ("2024".into(), 3_000.0),
            ],
            current_assets: vec![("2024".into(), 50_000.0)],
            current_liabilities: vec![("2024".into(), 30_000.0)],
            cash: vec![("2024".into(), 10_000.0)],
            long_term_debt: vec![("2024".into(), 40_000.0)],
            shares_outstanding: 1_000.0,
            tax_rate: 0.21,
        }
    }

    fn sample_tree_two_independent() -> ScenarioTreeInput {
        ScenarioTreeInput {
            nodes: vec![
                ScenarioTreeNode {
                    id: "regulation".into(),
                    name: Some("Regulation passes".into()),
                    marginal_probability: 0.3,
                    depends_on: vec![],
                },
                ScenarioTreeNode {
                    id: "competitor".into(),
                    name: Some("Competitor launches product".into()),
                    marginal_probability: 0.4,
                    depends_on: vec![],
                },
            ],
            topological_order: vec!["regulation".into(), "competitor".into()],
        }
    }

    #[test]
    fn two_independent_nodes_four_paths() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = sample_tree_two_independent();
        let impacts = vec![
            ScenarioNodeImpact {
                node_id: "regulation".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(-0.03),
                    gross_margin_delta: Some(-0.02),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
            ScenarioNodeImpact {
                node_id: "competitor".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(-0.05),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
        ];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0).unwrap();

        assert_eq!(result.path_count, 4);
        // Path probabilities should sum to 1.0 for independent nodes.
        let total: f64 = result.paths.iter().map(|p| p.probability).sum();
        assert!((total - 1.0).abs() < 1e-9, "total prob = {total}");
    }

    #[test]
    fn no_impact_returns_base_intrinsic() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = sample_tree_two_independent();
        let impacts: Vec<ScenarioNodeImpact> = vec![
            ScenarioNodeImpact {
                node_id: "regulation".into(),
                yes_deltas: AssumptionDelta::default(),
                no_deltas: AssumptionDelta::default(),
            },
            ScenarioNodeImpact {
                node_id: "competitor".into(),
                yes_deltas: AssumptionDelta::default(),
                no_deltas: AssumptionDelta::default(),
            },
        ];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0).unwrap();

        assert!(
            (result.probability_weighted_intrinsic - result.base_intrinsic).abs() < 1e-6,
            "no impact → weighted == base"
        );
    }

    #[test]
    fn bearish_impact_lowers_intrinsic() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = sample_tree_two_independent();
        let impacts = vec![
            ScenarioNodeImpact {
                node_id: "regulation".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(-0.10),
                    gross_margin_delta: Some(-0.05),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
            ScenarioNodeImpact {
                node_id: "competitor".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(-0.10),
                    gross_margin_delta: Some(-0.05),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
        ];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0).unwrap();

        assert!(
            result.probability_weighted_intrinsic < result.base_intrinsic,
            "bearish impact should lower intrinsic: {} vs {}",
            result.probability_weighted_intrinsic,
            result.base_intrinsic
        );
    }

    #[test]
    fn path_probabilities_with_dependency() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);

        let tree = ScenarioTreeInput {
            nodes: vec![
                ScenarioTreeNode {
                    id: "a".into(),
                    name: Some("Event A".into()),
                    marginal_probability: 0.6,
                    depends_on: vec![],
                },
                ScenarioTreeNode {
                    id: "b".into(),
                    name: Some("Event B".into()),
                    marginal_probability: 0.0, // not used for dependent nodes
                    depends_on: vec![ScenarioTreeDependency {
                        parent_event_ids: vec!["a".into()],
                        conditionals: vec![0.2, 0.8], // P(b|¬a)=0.2, P(b|a)=0.8
                    }],
                },
            ],
            topological_order: vec!["a".into(), "b".into()],
        };
        let impacts = vec![
            ScenarioNodeImpact {
                node_id: "a".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(0.02),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
            ScenarioNodeImpact {
                node_id: "b".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(-0.02),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
        ];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0).unwrap();

        // P(a=Y, b=Y) = 0.6 * 0.8 = 0.48
        // P(a=Y, b=N) = 0.6 * 0.2 = 0.12
        // P(a=N, b=Y) = 0.4 * 0.2 = 0.08
        // P(a=N, b=N) = 0.4 * 0.8 = 0.32
        // Sum = 1.0
        let total: f64 = result.paths.iter().map(|p| p.probability).sum();
        assert!((total - 1.0).abs() < 1e-9, "total = {total}");

        // Check individual path probabilities.
        let p_yy = result
            .paths
            .iter()
            .find(|p| p.path_mask == 0b11)
            .map(|p| p.probability)
            .unwrap_or(0.0);
        assert!((p_yy - 0.48).abs() < 1e-9, "P(Y,Y) = {p_yy}");
    }

    #[test]
    fn rejects_too_many_nodes() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = ScenarioTreeInput {
            nodes: (0..=MAX_SCENARIO_NODES)
                .map(|i| ScenarioTreeNode {
                    id: format!("n{i}"),
                    name: None,
                    marginal_probability: 0.5,
                    depends_on: vec![],
                })
                .collect(),
            topological_order: vec![],
        };

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &[], 100.0);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unknown_impact_node() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = sample_tree_two_independent();
        let impacts = vec![ScenarioNodeImpact {
            node_id: "nonexistent".into(),
            yes_deltas: AssumptionDelta {
                revenue_growth_delta: Some(-0.01),
                ..Default::default()
            },
            no_deltas: AssumptionDelta::default(),
        }];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_topo_order_fallback() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        // Child "b" appears before parent "a" in node array, no topo_order provided.
        let tree = ScenarioTreeInput {
            nodes: vec![
                ScenarioTreeNode {
                    id: "b".into(),
                    name: None,
                    marginal_probability: 0.0,
                    depends_on: vec![ScenarioTreeDependency {
                        parent_event_ids: vec!["a".into()],
                        conditionals: vec![0.2, 0.8],
                    }],
                },
                ScenarioTreeNode {
                    id: "a".into(),
                    name: None,
                    marginal_probability: 0.6,
                    depends_on: vec![],
                },
            ],
            topological_order: vec![], // empty → fallback to node array order
        };

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &[], 100.0);
        assert!(result.is_err(), "should reject invalid topo order fallback");
    }

    #[test]
    fn node_name_populated_in_sensitivity() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = sample_tree_two_independent();
        let impacts = vec![
            ScenarioNodeImpact {
                node_id: "regulation".into(),
                yes_deltas: AssumptionDelta {
                    revenue_growth_delta: Some(-0.03),
                    ..Default::default()
                },
                no_deltas: AssumptionDelta::default(),
            },
            ScenarioNodeImpact {
                node_id: "competitor".into(),
                yes_deltas: AssumptionDelta::default(),
                no_deltas: AssumptionDelta::default(),
            },
        ];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0).unwrap();

        let reg = result
            .node_sensitivities
            .iter()
            .find(|s| s.node_id == "regulation")
            .expect("regulation node in sensitivities");
        assert_eq!(reg.node_name.as_deref(), Some("Regulation passes"));
    }

    #[test]
    fn rejects_duplicate_node_ids() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = ScenarioTreeInput {
            nodes: vec![
                ScenarioTreeNode {
                    id: "dup".into(),
                    name: None,
                    marginal_probability: 0.5,
                    depends_on: vec![],
                },
                ScenarioTreeNode {
                    id: "dup".into(),
                    name: None,
                    marginal_probability: 0.5,
                    depends_on: vec![],
                },
            ],
            topological_order: vec!["dup".into()],
        };

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &[], 100.0);
        assert!(result.is_err(), "should reject duplicate node IDs");
    }

    #[test]
    fn terminal_growth_never_equals_discount_rate() {
        let hist = sample_hist();
        let assumptions = ProjectionAssumptions::from_history(&hist);
        let tree = ScenarioTreeInput {
            nodes: vec![ScenarioTreeNode {
                id: "x".into(),
                name: None,
                marginal_probability: 0.5,
                depends_on: vec![],
            }],
            topological_order: vec!["x".into()],
        };
        // Push terminal_growth above discount_rate via deltas.
        let impacts = vec![ScenarioNodeImpact {
            node_id: "x".into(),
            yes_deltas: AssumptionDelta {
                terminal_growth_delta: Some(0.20),
                discount_rate_delta: Some(-0.10),
                ..Default::default()
            },
            no_deltas: AssumptionDelta::default(),
        }];

        let result = scenario_impact_dcf(&hist, &assumptions, &tree, &impacts, 100.0).unwrap();
        // Every path should produce a finite intrinsic (no NaN/inf from div-by-zero).
        for path in &result.paths {
            assert!(
                path.intrinsic_per_share.is_finite(),
                "intrinsic is finite: {}",
                path.intrinsic_per_share
            );
        }
    }
}
