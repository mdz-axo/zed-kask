//! Forecast math — pure deterministic functions: Fermi decomposition, event-tree
//! conditional probability propagation, Brier scoring, sensitivity ranking, and
//! framing-document construction. No I/O, no `ForecastStore` state — the stateful
//! orchestration (assessment, composition, conversion, persistence) remains in
//! `superforecast.rs`.
//!
//! Extracted from `superforecast.rs` (deep-module split).

use std::collections::{HashMap, HashSet};

use hkask_forecast as forecast;

use crate::types::{
    EventTree, EventTreeNode, ForecastOutcome, FramingDocument, ScenarioError, ScenarioEvent,
    ScenarioType, StakeholderConfig, SubQuestion, TimeHorizon, UseCase,
};
// ── Fermi decomposition ────────────────────────────────────────────────────

/// Fermi decomposition calibration. Converts SubQuestion to FermiQuestion and
/// delegates to the shared hkask-forecast engine.
#[must_use = "calibration result should be used or the error handled"]
pub fn calibrate_from_fermi(sub_questions: &[SubQuestion]) -> Result<f64, ScenarioError> {
    let fqs: Vec<forecast::FermiQuestion> = sub_questions
        .iter()
        .map(|sq| forecast::FermiQuestion::new(sq.question.clone(), sq.estimate, sq.confidence))
        .collect();
    Ok(forecast::calibrate_from_fermi(&fqs)?)
}

// ── Brier scoring (multi) ──────────────────────────────────────────────────

/// Average Brier score across multiple events. Delegates to the shared engine;
/// ForecastError converts to ScenarioError via #[from].
#[must_use = "multi-score should be used or recorded"]
pub(crate) fn brier_score_multi(
    probabilities: &[f64],
    outcomes: &[bool],
) -> Result<f64, ScenarioError> {
    Ok(forecast::brier_score_multi(probabilities, outcomes)?)
}

// ── Event tree computation ─────────────────────────────────────────────────

/// Compute marginal probabilities for all events in a dependency tree
/// via full joint conditional-table marginalization under parent independence.
///
/// Root events (no parents) use their stored probability.
/// Dependent events marginalize over the full joint truth-assignment space
/// of each dependency group:
///
///   P_g(E) = Sum_a P_g(E | a) * Product_i P(p_i)^{a_i} * (1-P(p_i))^{1-a_i}
///
/// where a ranges over the 2^n bitmap of group g's parent truth assignments,
/// and parent probabilities P(p_i) are assumed independent.
///
/// # Multi-group combination rule (noisy-OR)
///
/// When `depends_on` holds more than one dependency group, each group is
/// marginalized independently and the per-group marginals are combined by
/// noisy-OR:
///
///   P(E) = 1 - Product_g (1 - P_g(E))
///
/// Rationale: disjoint groups (overlap is rejected at validation) model
/// separate sufficient causal mechanisms for E, each of which produces E
/// with probability P_g(E) on its own; E occurs iff at least one mechanism
/// fires, and the mechanisms are assumed independent — the same independence
/// assumption the per-group formula already makes between parents. Noisy-OR
/// is the standard closed form under that assumption and reduces exactly to
/// the single-group formula when there is one group (1-(1-p) == p in IEEE-754),
/// so single-group results are unchanged.
///
/// Returns a map of event_id -> resolved marginal probability.
pub(crate) fn compute_marginal_probabilities(
    events: &[ScenarioEvent],
    topo_order: &[String],
) -> HashMap<String, f64> {
    let event_map: HashMap<&str, &ScenarioEvent> =
        events.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut resolved: HashMap<String, f64> = HashMap::new();

    for id in topo_order {
        let event = match event_map.get(id.as_str()) {
            Some(e) => e,
            None => continue,
        };

        if event.depends_on.is_empty() {
            // Root node: use own probability
            resolved.insert(id.clone(), event.probability);
        } else {
            // Full joint marginalization per dependency group under parent
            // independence, delegated to the shared `hkask_forecast::marginalize`
            // so this and the graph widget's re-propagation stay one source of
            // truth for the per-group formula. Groups are then combined by
            // noisy-OR (see the doc comment above for the rule and rationale).
            let group_marginals: Vec<f64> = event
                .depends_on
                .iter()
                .map(|dep| {
                    let parent_probs: Vec<f64> = dep
                        .parent_event_ids
                        .iter()
                        .map(|pid| {
                            resolved.get(pid).copied().unwrap_or_else(|| {
                                tracing::warn!(
                                    parent_id = %pid,
                                    event_id = %id,
                                    "Parent not found in resolved map; defaulting to 0.0"
                                );
                                0.0
                            })
                        })
                        .collect();
                    hkask_forecast::marginalize(&parent_probs, &dep.conditionals)
                })
                .collect();

            let marginal = combine_independent_channels(&group_marginals);
            resolved.insert(id.clone(), marginal.clamp(0.0, 1.0));
        }
    }

    resolved
}

/// Noisy-OR combination of independent causal channels:
/// P(E) = 1 - Product_g (1 - P_g(E)).
///
/// For a single channel this is the identity (1-(1-p) == p exactly in
/// IEEE-754), so single-dependency-group behavior is unchanged.
fn combine_independent_channels(channel_probabilities: &[f64]) -> f64 {
    let survival = channel_probabilities
        .iter()
        .fold(1.0, |acc, &probability| acc * (1.0 - probability));
    1.0 - survival
}

/// Build a full event tree from a list of events.
/// Topologically sorts based on dependencies, computes marginal probabilities,
/// and produces an EventTree with resolved nodes.
#[must_use = "tree should be used or error inspected"]
pub fn build_event_tree(events: &[ScenarioEvent]) -> Result<EventTree, ScenarioError> {
    if events.is_empty() {
        return Err(ScenarioError::NoEvents);
    }

    // Validate all events
    for event in events {
        event.validate()?;
    }

    // Topological sort by Kahn's algorithm
    let toposort = topological_sort(events)?;

    // Identify root nodes (no dependencies)
    let root_ids: Vec<String> = events
        .iter()
        .filter(|e| e.depends_on.is_empty())
        .map(|e| e.id.clone())
        .collect();

    // Compute marginal probabilities
    let marginals = compute_marginal_probabilities(events, &toposort);

    // Build tree nodes
    let event_map: HashMap<&str, &ScenarioEvent> =
        events.iter().map(|e| (e.id.as_str(), e)).collect();

    let mut nodes: Vec<EventTreeNode> = Vec::new();
    let mut joint_prob = 1.0;

    for id in &toposort {
        let event = event_map
            .get(id.as_str())
            .ok_or_else(|| ScenarioError::EventNotFound(id.clone()))?;
        let marginal = marginals.get(id).copied().unwrap_or(event.probability);
        let joint_factor = if event.depends_on.is_empty() {
            marginal
        } else {
            // All-parents-true conditional per group: conditionals[last] =
            // P_g(E | all of group g's parents true). With multiple groups,
            // combine by the same noisy-OR rule as the marginal computation
            // so the joint factor is P(E | every parent in every group true).
            let all_parents_true: Vec<f64> = event
                .depends_on
                .iter()
                .map(|dep| dep.conditionals.last().copied().unwrap_or(0.0))
                .collect();
            combine_independent_channels(&all_parents_true)
        };

        // Build path from root to this node
        let paths = build_path(id, events);

        // Variance contribution: |P - 0.5| — how far from coin-flip
        let variance_contribution = (marginal - 0.5).abs() * 2.0; // scale to [0, 1]

        nodes.push(EventTreeNode {
            event: (*event).clone(),
            marginal_probability: marginal,
            paths,
            variance_contribution,
        });

        // For dependent events, the all-events-occur joint factor is
        // P(E | all parents true), drawn from the conditional table.
        joint_prob *= joint_factor;
    }

    let subject = events
        .first()
        .map(|e| e.subject.clone())
        .unwrap_or_default();
    let time_horizon = events
        .first()
        .map(|e| e.time_horizon)
        .unwrap_or(TimeHorizon::Strategic);
    let scenario_type = events
        .first()
        .map(|e| e.scenario_type)
        .unwrap_or(ScenarioType::CompanyAnalysis);

    Ok(EventTree {
        subject,
        time_horizon,
        scenario_type,
        nodes,
        root_ids,
        topo_order: toposort,
        joint_probability: joint_prob,
    })
}

/// Kahn's algorithm for topological sort of events by dependency graph.
fn topological_sort(events: &[ScenarioEvent]) -> Result<Vec<String>, ScenarioError> {
    let event_ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
    let id_set: HashSet<&str> = event_ids.iter().map(|s| s.as_str()).collect();

    // Build adjacency list and in-degree map
    let mut in_degree: HashMap<String, u32> = event_ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut adjacency: HashMap<String, Vec<String>> = event_ids
        .iter()
        .map(|id| (id.clone(), Vec::new()))
        .collect();

    for event in events {
        for dep in &event.depends_on {
            for parent_id in &dep.parent_event_ids {
                if !id_set.contains(parent_id.as_str()) {
                    return Err(ScenarioError::UnknownParent(
                        event.id.clone(),
                        parent_id.clone(),
                    ));
                }
                adjacency
                    .get_mut(parent_id)
                    .ok_or_else(|| ScenarioError::EventNotFound(parent_id.clone()))?
                    .push(event.id.clone());
                *in_degree
                    .get_mut(&event.id)
                    .ok_or_else(|| ScenarioError::EventNotFound(event.id.clone()))? += 1;
            }
        }
    }

    // Kahn's algorithm
    let mut queue: Vec<String> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut sorted: Vec<String> = Vec::new();

    while let Some(node) = queue.pop() {
        sorted.push(node.clone());
        if let Some(children) = adjacency.get(&node) {
            for child in children {
                if let Some(deg) = in_degree.get_mut(child) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(child.clone());
                    }
                }
            }
        }
    }

    if sorted.len() != events.len() {
        return Err(ScenarioError::CycleDetected);
    }

    Ok(sorted)
}

/// Build all paths from root events to a given event ID.
/// For single-parent events, returns one path. For multi-parent events,
/// returns one path per parent (recursively collected).
fn build_path(target_id: &str, events: &[ScenarioEvent]) -> Vec<Vec<String>> {
    let event_map: HashMap<&str, &ScenarioEvent> =
        events.iter().map(|e| (e.id.as_str(), e)).collect();

    // Recursively collect all paths from an event to root nodes.
    fn collect_paths(
        current: &str,
        event_map: &HashMap<&str, &ScenarioEvent>,
        visited: &mut HashSet<String>,
    ) -> Vec<Vec<String>> {
        // Guard against cycles (should not happen after topological sort)
        if !visited.insert(current.to_string()) {
            return vec![vec![current.to_string()]];
        }

        let event = match event_map.get(current) {
            Some(e) => e,
            None => return vec![vec![current.to_string()]],
        };

        if event.depends_on.is_empty() {
            return vec![vec![current.to_string()]];
        }

        let mut all_paths = Vec::new();
        for dep in &event.depends_on {
            for parent_id in &dep.parent_event_ids {
                let parent_paths = collect_paths(parent_id, event_map, visited);
                for parent_path in parent_paths {
                    let mut full_path = parent_path;
                    full_path.push(current.to_string());
                    all_paths.push(full_path);
                }
            }
        }
        all_paths
    }

    let mut paths = collect_paths(target_id, &event_map, &mut HashSet::new());
    if paths.is_empty() {
        paths.push(vec![target_id.to_string()]);
    }
    paths
}

// ── Sensitivity: which events drive outcome variance ───────────────────────

/// Rank events by their contribution to outcome uncertainty.
/// Uses |P - 0.5| as a proxy — events closer to 0.5 contribute
/// more uncertainty because they're closer to a coin flip.
/// Higher score = more uncertainty.
#[must_use = "ranking result should be used"]
pub fn sensitivity_ranking(tree: &EventTree) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = tree
        .nodes
        .iter()
        .map(|n| (n.event.id.clone(), 1.0 - n.variance_contribution))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

// ── Forecast record helpers ────────────────────────────────────────────────

/// Score a forecast against known outcomes and produce a ForecastOutcome.
/// Also computes per-event update suggestions for closing the feedback loop.
pub fn score_forecast(
    forecast_id: &str,
    events: &[ScenarioEvent],
    outcomes: &[(String, bool)],
    forecast_date: chrono::NaiveDate,
) -> ForecastOutcome {
    let event_map: HashMap<&str, &ScenarioEvent> =
        events.iter().map(|e| (e.id.as_str(), e)).collect();

    let mut probs = Vec::new();
    let mut outs = Vec::new();
    let mut event_outcomes = Vec::new();

    for (event_id, occurred) in outcomes {
        if let Some(event) = event_map.get(event_id.as_str()) {
            probs.push(event.probability);
            outs.push(*occurred);
            event_outcomes.push((event_id.clone(), *occurred));
        } else {
            tracing::warn!(
                target: "hkask.mcp.scenarios",
                event_id = %event_id,
                "outcome has no matching event, skipped"
            );
        }
    }

    let bs = match brier_score_multi(&probs, &outs) {
        Ok(bs) => bs,
        Err(error) => {
            tracing::warn!(
                target: "hkask.mcp.scenarios",
                %error,
                "Brier multi-score failed, defaulting to climatology 0.33"
            );
            0.33
        }
    };

    ForecastOutcome {
        forecast_id: forecast_id.to_string(),
        subject: events
            .first()
            .map(|e| e.subject.clone())
            .unwrap_or_default(),
        forecast_date,
        outcome_date: chrono::Utc::now().date_naive(),
        event_outcomes,
        brier_score: bs,
        brier_interpretation: forecast::brier_interpretation(bs).to_string(),
    }
}

/// Compute per-event Bayesian update suggestions based on forecast error direction.
/// Positive delta means probability should be raised; negative means lowered.
pub fn auto_update_suggestions(
    events: &[ScenarioEvent],
    outcomes: &[(String, bool)],
) -> Vec<serde_json::Value> {
    let event_map: HashMap<&str, &ScenarioEvent> =
        events.iter().map(|e| (e.id.as_str(), e)).collect();

    outcomes
        .iter()
        .filter_map(|(event_id, occurred)| {
            let event = event_map.get(event_id.as_str())?;
            let error = event.probability - if *occurred { 1.0 } else { 0.0 };
            // Suggest a modest correction in the error's direction
            let adjustment = (-error * 0.25).clamp(-0.15, 0.15);
            let suggested = (event.probability + adjustment).clamp(0.01, 0.99);
            Some(serde_json::json!({
                "event_id": event_id,
                "event_name": event.name,
                "forecast_probability": event.probability,
                "outcome": occurred,
                "error": error,
                "suggested_adjustment": adjustment,
                "suggested_probability": suggested,
            }))
        })
        .collect()
}

// Brainstorming and framing templates moved to `templates` module.

/// Structure a completed framing conversation into a FramingDocument.
/// Takes the subject and a JSON blob of conversation answers, validates them,
/// and produces a typed FramingDocument suitable for feeding into scenario_brainstorm.
pub fn structure_framing_document(
    subject: &str,
    answers: &serde_json::Value,
) -> Result<FramingDocument, ScenarioError> {
    if subject.trim().is_empty() {
        return Err(ScenarioError::EmptyInput("subject".into()));
    }
    let get_str = |key: &str| -> String {
        answers
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let get_list = |key: &str| -> Vec<String> {
        answers
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };

    let time_horizon = match get_str("time_horizon").to_lowercase().as_str() {
        "tactical" | "12-18 months" | "tactical (12-18 months)" => TimeHorizon::Tactical,
        "strategic" | "3-5 years" | "strategic (3-5 years)" => TimeHorizon::Strategic,
        "long-term" | "7-10 years" | "long-term (7-10 years)" => TimeHorizon::LongTerm,
        _ => TimeHorizon::Strategic,
    };

    let use_case = match get_str("use_case").to_lowercase().as_str() {
        "strategic decision" | "strategic_decision" => UseCase::StrategicDecision,
        "investment thesis" | "investment_thesis" => UseCase::InvestmentThesis,
        "monitoring dashboard" | "monitoring_dashboard" => UseCase::MonitoringDashboard,
        "landscape exploration" | "landscape_exploration" => UseCase::LandscapeExploration,
        "contingency planning" | "contingency_planning" => UseCase::ContingencyPlanning,
        _ => UseCase::LandscapeExploration,
    };

    let stakeholders: Vec<StakeholderConfig> = answers
        .get("stakeholders")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|s| StakeholderConfig {
                    role: s
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    primary_concern: s
                        .get("primary_concern")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    likely_blind_spots: s
                        .get("likely_blind_spots")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    include_as_persona: s
                        .get("include_as_persona")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(FramingDocument {
        focal_question: get_str("focal_question"),
        decision_at_stake: get_str("decision_at_stake"),
        time_horizon,
        action_deadline: {
            let d = get_str("action_deadline");
            if d.is_empty() { None } else { Some(d) }
        },
        in_scope: get_list("in_scope"),
        out_of_scope: get_list("out_of_scope"),
        stakeholders,
        use_case,
        success_criteria: get_list("success_criteria"),
        constraints: get_list("constraints"),
        surfaced_assumptions: get_list("surfaced_assumptions"),
        exploration_prompts: get_list("exploration_prompts"),
    })
}
