//! Superforecasting computation engine (Tetlock GJP methodology).
//!
//! Four-stage pipeline:
//! 1. Fermi decomposition — break forecast into sub-questions
//! 2. Outside view — base rate calibration from reference class
//! 3. Inside view — case-specific adjustments
//! 4. Bayesian updating — revise probabilities as evidence arrives
//!
//! Plus: event tree computation (conditional probability propagation)
//! and Brier scoring for calibration tracking.

use crate::types::{
    AssessInput, CalibrationBin, CalibrationCurve, CrossValidation, DragonflySynthesis, EventTree,
    EventTreeNode, ForecastOutcome, FramingDocument, Perspective, PhaseScore, ProjectAssessment,
    ScenarioError, ScenarioEvent, ScenarioType, StakeholderConfig, StoredForecastRecord,
    SubQuestion, SubQuestionDivergence, TimeHorizon, TriageAssessment, UseCase,
};
use std::collections::{HashMap, HashSet};

use hkask_forecast as forecast;

// ── Re-exports from hkask-forecast (pure pass-throughs eliminated) ───────
pub use forecast::{bayesian_update, brier_interpretation, brier_score, outside_view_adjustment};

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
/// Dependent events marginalize over the full joint truth-assignment space:
///
///   P(E) = Sum_a P(E | a) * Product_i P(p_i)^{a_i} * (1-P(p_i))^{1-a_i}
///
/// where a ranges over the 2^n bitmap of parent truth assignments,
/// and parent probabilities P(p_i) are assumed independent.
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
            // Full joint marginalization under parent independence.
            // P(E) = Sum_a P(E|a) * Product_i P(p_i)^{a_i} * (1-P(p_i))^{1-a_i}
            let dep = &event.depends_on[0];
            let n_assignments = 1usize << dep.parent_event_ids.len();

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

            let mut marginal = 0.0;
            for assignment in 0..n_assignments {
                let mut assignment_prob = 1.0;
                for (j, &p_prob) in parent_probs.iter().enumerate() {
                    let bit_set = (assignment >> j) & 1 == 1;
                    assignment_prob *= if bit_set { p_prob } else { 1.0 - p_prob };
                }
                if let Some(&cond) = dep.conditionals.get(assignment) {
                    marginal += cond * assignment_prob;
                }
            }

            resolved.insert(id.clone(), marginal.clamp(0.0, 1.0));
        }
    }

    resolved
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
            // All-parents-true conditional: conditionals[last] = P(E | all parents true)
            event.depends_on[0]
                .conditionals
                .last()
                .copied()
                .unwrap_or(0.0)
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
                adjacency.get_mut(parent_id).unwrap().push(event.id.clone());
                *in_degree.get_mut(&event.id).unwrap() += 1;
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
        }
    }

    let bs = brier_score_multi(&probs, &outs).unwrap_or(0.33);

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
        brier_interpretation: brier_interpretation(bs).to_string(),
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

// ── Chermack Project Assessment (P5) ──────────────────────────────────────

/// Phase-score tiers for Chermack assessment. Each phase scores 0-1 based on
/// count/threshold gates. These are heuristic tiers, not empirically calibrated
/// weights — see Chermack (2011), Ch. 9 for the assessment framework.
mod assess_tiers {
    pub const PREP_STRONG: f64 = 0.8;
    pub const PREP_PERSPECTIVE_HIGH: usize = 3;
    pub const EXP_STRONG: f64 = 0.75;
    pub const EXP_ADEQUATE: f64 = 0.5;
    pub const EXP_WEAK: f64 = 0.2;
    pub const EXP_EVENT_HIGH: usize = 5;
    pub const EXP_EVENT_MID: usize = 3;
    pub const EXP_DISAGREEMENT_HIGH: f64 = 0.1;
    pub const EXP_DISAGREEMENT_SIGNIFICANT: f64 = 0.2;
    pub const EXP_DISAGREEMENT_GROUPTHINK: f64 = 0.05;
    pub const DEV_STRONG: f64 = 0.8;
    pub const DEV_ADEQUATE: f64 = 0.5;
    pub const DEV_WEAK: f64 = 0.3;
    pub const DEV_RATIO_HIGH: f64 = 0.3;
    pub const DEV_RATIO_MID: f64 = 0.1;
    pub const DEV_EVENT_MIN: usize = 4;
    pub const IMPL_STRONG: f64 = 0.85;
    pub const IMPL_ADEQUATE: f64 = 0.5;
    pub const IMPL_WEAK: f64 = 0.1;
    pub const ASSESS_STRONG: f64 = 0.8;
    pub const ASSESS_ADEQUATE: f64 = 0.5;
    pub const ASSESS_WEAK: f64 = 0.2;
    pub const ASSESS_RESOLVED_MIN: u64 = 5;
    pub const ASSESS_RESOLVED_SUFFICIENT: u64 = 10;
    pub const OVERALL_STRONG: f64 = 0.7;
    pub const OVERALL_ADEQUATE: f64 = 0.5;
    pub const OVERALL_FOUNDATIONAL: f64 = 0.3;
    pub const RECOMMENDATION_THRESHOLD: f64 = 0.6;
}

/// Assess a scenario project across Chermack's five performance phases.
///
/// Evaluates whether the scenario project was worth doing — not just
/// whether forecasts were accurate. Combines quantitative metrics
/// (Brier scores, disagreement, calibration) with qualitative assessment
/// of preparation, exploration, implementation, and learning.
///
/// Reference: Chermack, T.J. (2011). Scenario Planning in Organizations:
/// How to Create, Use, and Assess Scenarios. Berrett-Koehler.
pub fn assess_project(input: &AssessInput) -> ProjectAssessment {
    let project_id = input.project_id;
    let subject = input.subject;
    let perspective_count = input.perspective_count;
    let disagreement_score = input.disagreement_score;
    let event_count = input.event_count;
    let events_with_deps = input.events_with_deps;
    let calibration_curve = input.calibration_curve;
    let strategies_generated = input.strategies_generated;
    let strategies_implemented = input.strategies_implemented;
    let learning_events = &input.learning_events;
    let has_early_warning_indicators = input.has_early_warning_indicators;
    // ── Phase 1: Preparation ──────────────────────────────────────
    // (Chermack, Ch. 5): Scope clarity, stakeholder engagement, resource allocation
    let prep_score = if perspective_count >= assess_tiers::PREP_PERSPECTIVE_HIGH {
        assess_tiers::PREP_STRONG
    } else if perspective_count >= 2 {
        0.6
    } else {
        0.3
    };
    let mut prep_strengths = Vec::new();
    let mut prep_gaps = Vec::new();
    if perspective_count >= 3 {
        prep_strengths.push("Multiple perspectives engaged".into());
    } else if perspective_count == 0 {
        prep_gaps.push(
            "No perspectives recorded — project may lack stakeholder engagement (Chermack Phase 1)"
                .into(),
        );
    } else {
        prep_gaps.push(format!("Only {} perspective(s) — consider engaging more diverse viewpoints (Chermack: stakeholder dialogue)", perspective_count));
    }

    // ── Phase 2: Exploration ─────────────────────────────────────
    // (Chermack, Ch. 6): Driving forces identified, trends mapped, uncertainties surfaced
    let exp_score = if event_count >= assess_tiers::EXP_EVENT_HIGH
        && disagreement_score > assess_tiers::EXP_DISAGREEMENT_HIGH
    {
        assess_tiers::EXP_STRONG
    } else if event_count >= assess_tiers::EXP_EVENT_MID {
        assess_tiers::EXP_ADEQUATE
    } else {
        assess_tiers::EXP_WEAK
    };
    let mut exp_strengths = Vec::new();
    let mut exp_gaps = Vec::new();
    if disagreement_score > assess_tiers::EXP_DISAGREEMENT_SIGNIFICANT {
        exp_strengths.push(format!("Significant disagreement ({:.0}%) detected — healthy diversity of views (Chermack: conversation quality)", disagreement_score * 100.0));
    }
    if event_count >= assess_tiers::EXP_EVENT_HIGH {
        exp_strengths.push(format!(
            "{} events identified — comprehensive force mapping",
            event_count
        ));
    } else {
        exp_gaps.push(format!(
            "Only {} events — consider deeper STEEP force mapping",
            event_count
        ));
    }
    if disagreement_score < assess_tiers::EXP_DISAGREEMENT_GROUPTHINK && event_count > 0 {
        exp_gaps.push("Very low disagreement — potential groupthink. Chermack warns against false consensus in scenario exploration.".into());
    }

    // ── Phase 3: Development ─────────────────────────────────────
    // (Chermack, Ch. 7): Scenario logic, internal consistency, narrative quality
    let dep_ratio = if event_count > 0 {
        events_with_deps as f64 / event_count as f64
    } else {
        0.0
    };
    let dev_score =
        if dep_ratio > assess_tiers::DEV_RATIO_HIGH && event_count >= assess_tiers::DEV_EVENT_MIN {
            assess_tiers::DEV_STRONG
        } else if dep_ratio > assess_tiers::DEV_RATIO_MID {
            assess_tiers::DEV_ADEQUATE
        } else {
            assess_tiers::DEV_WEAK
        };
    let mut dev_strengths = Vec::new();
    let mut dev_gaps = Vec::new();
    if dep_ratio > assess_tiers::DEV_RATIO_HIGH {
        dev_strengths.push(format!("{:.0}% of events have conditional dependencies — structured causal reasoning (Chermack: internal consistency)", dep_ratio * 100.0));
    } else {
        dev_gaps.push("Most events lack dependency links. Chermack requires internal consistency: events should form a causal chain, not a list.".into());
    }
    if event_count < assess_tiers::DEV_EVENT_MIN {
        dev_gaps.push("Fewer than 4 events — scenarios may lack sufficient structure for meaningful narratives.".into());
    }

    // ── Phase 4: Implementation ──────────────────────────────────
    // (Chermack, Ch. 8): Strategies applied, wind-tunneling, early warning systems
    let impl_score = if strategies_implemented > 0 && has_early_warning_indicators {
        assess_tiers::IMPL_STRONG
    } else if strategies_generated > 0 {
        assess_tiers::IMPL_ADEQUATE
    } else {
        assess_tiers::IMPL_WEAK
    };
    let mut impl_strengths = Vec::new();
    let mut impl_gaps = Vec::new();
    if strategies_implemented > 0 {
        impl_strengths.push(format!(
            "{} strategies implemented — scenario insights drove action (Chermack Phase 4)",
            strategies_implemented
        ));
    }
    if strategies_generated > 0 && strategies_implemented == 0 {
        impl_gaps.push(format!("{} strategies generated but none implemented — the scenario-to-action gap (Chermack's critical Phase 4)", strategies_generated));
    }
    if !has_early_warning_indicators {
        impl_gaps.push("No early warning indicators defined. Chermack: scenarios without tripwires are stories without sensors.".into());
    }

    // ── Phase 5: Project Assessment ──────────────────────────────
    // (Chermack, Ch. 9): Did the project improve decision quality? Learning outcomes?
    let assess_score = if !learning_events.is_empty()
        && calibration_curve
            .is_some_and(|c| c.resolved_forecasts >= assess_tiers::ASSESS_RESOLVED_MIN)
    {
        assess_tiers::ASSESS_STRONG
    } else if !learning_events.is_empty() {
        assess_tiers::ASSESS_ADEQUATE
    } else {
        assess_tiers::ASSESS_WEAK
    };
    let mut assess_strengths = Vec::new();
    let mut assess_gaps = Vec::new();
    if !learning_events.is_empty() {
        assess_strengths.push(format!("{} learning events recorded — evidence of mental model change (Chermack: organizational learning)", learning_events.len()));
    } else {
        assess_gaps.push("No learning events recorded. Chermack's key metric: did the project change how participants think?".into());
    }
    if let Some(curve) = calibration_curve {
        if curve.resolved_forecasts >= assess_tiers::ASSESS_RESOLVED_SUFFICIENT {
            assess_strengths.push(format!(
                "{} resolved forecasts — sufficient data for calibration assessment",
                curve.resolved_forecasts
            ));
        } else if curve.resolved_forecasts > 0 {
            assess_gaps.push(format!(
                "Only {} resolved forecasts — need ≥10 for reliable calibration assessment",
                curve.resolved_forecasts
            ));
        }
    } else {
        assess_gaps.push("No calibration data. Chermack + Tetlock: without outcome tracking, you cannot know if the project improved forecast accuracy.".into());
    }

    // ── Composite ─────────────────────────────────────────────────
    let overall = (prep_score + exp_score + dev_score + impl_score + assess_score) / 5.0;

    let assessment_text = if overall >= assess_tiers::OVERALL_STRONG {
        "Strong scenario project. Preparation was thorough, exploration surfaced diverse views, scenarios are causally structured, insights drove action, and learning is being tracked. Continue deepening the calibration loop."
    } else if overall >= assess_tiers::OVERALL_ADEQUATE {
        "Adequate scenario project with room for improvement. Strengthen the weakest phases (see per-phase gaps below). Focus on closing the implementation gap: scenarios without action are entertainment."
    } else if overall >= assess_tiers::OVERALL_FOUNDATIONAL {
        "Foundational scenario project. Core elements are present but significant gaps remain. Priority: engage more perspectives (Phase 1), add conditional dependencies (Phase 3), and track outcomes (Phase 5)."
    } else {
        "Early-stage scenario project. The scaffolding exists but lacks depth. Start with Phase 1 (preparation): define the focal question clearly and engage multiple perspectives before building scenarios."
    };

    let mut recommendations = Vec::new();
    if prep_score < assess_tiers::RECOMMENDATION_THRESHOLD {
        recommendations.push("Phase 1 (Preparation): Engage at least 3 diverse perspectives. Chermack: 'The quality of the conversation determines the quality of the scenarios.'".into());
    }
    if exp_score < assess_tiers::RECOMMENDATION_THRESHOLD {
        recommendations.push("Phase 2 (Exploration): Map more driving forces. Use scenario_research to gather external data. Chermack: systematic STEEP analysis prevents blind spots.".into());
    }
    if dev_score < assess_tiers::RECOMMENDATION_THRESHOLD {
        recommendations.push("Phase 3 (Development): Link events with conditional dependencies. Scenarios must form causal chains, not lists. Chermack: internal consistency is the quality gate.".into());
    }
    if impl_score < assess_tiers::RECOMMENDATION_THRESHOLD {
        recommendations.push("Phase 4 (Implementation): Define early-warning indicators and track which strategies get implemented. Chermack: 'Scenario planning without implementation is intellectual tourism.'".into());
    }
    if assess_score < assess_tiers::RECOMMENDATION_THRESHOLD {
        recommendations.push("Phase 5 (Assessment): Record learning events and track calibration. Use scenario_score to resolve forecasts and scenario_calibration to measure improvement over time.".into());
    }

    ProjectAssessment {
        project_id: project_id.to_string(),
        subject: subject.to_string(),
        preparation: PhaseScore {
            phase: "Phase 1: Preparation".into(),
            score: prep_score,
            strengths: prep_strengths,
            gaps: prep_gaps,
        },
        exploration: PhaseScore {
            phase: "Phase 2: Exploration".into(),
            score: exp_score,
            strengths: exp_strengths,
            gaps: exp_gaps,
        },
        development: PhaseScore {
            phase: "Phase 3: Development".into(),
            score: dev_score,
            strengths: dev_strengths,
            gaps: dev_gaps,
        },
        implementation: PhaseScore {
            phase: "Phase 4: Implementation".into(),
            score: impl_score,
            strengths: impl_strengths,
            gaps: impl_gaps,
        },
        project_assessment: PhaseScore {
            phase: "Phase 5: Project Assessment".into(),
            score: assess_score,
            strengths: assess_strengths,
            gaps: assess_gaps,
        },
        overall_score: overall,
        overall_assessment: assessment_text.to_string(),
        learning_evidence: input.learning_events.clone(),
        recommendations,
    }
}
// ── Dragonfly-Eye Synthesis (P1) ──────────────────────────────────────────

/// Synthesize multiple independent perspectives on an event into one
/// aggregated probability with disagreement scoring.
///
/// Uses empirical-Bayes weighting: perspectives with lower historical Brier
/// scores get higher weight. If no historical scores are available, all
/// perspectives are weighted equally.
///
/// Returns an error if fewer than 2 perspectives are provided.
pub fn synthesize_perspectives(
    event_id: &str,
    perspectives: &[Perspective],
) -> Result<DragonflySynthesis, ScenarioError> {
    if perspectives.len() < 2 {
        return Err(ScenarioError::InsufficientPerspectives);
    }

    // Compute weights: inverse-Brier if available, else uniform
    let has_historical = perspectives.iter().any(|p| p.historical_brier.is_some());

    let weights: Vec<f64> = if has_historical {
        let raw: Vec<f64> = perspectives
            .iter()
            .map(|p| {
                let brier = p.historical_brier.unwrap_or(0.25);
                1.0 / (brier + 0.01)
            })
            .collect();
        let total: f64 = raw.iter().sum();
        raw.iter().map(|w| w / total).collect()
    } else {
        let w = 1.0 / perspectives.len() as f64;
        vec![w; perspectives.len()]
    };

    // Weighted average probability
    let aggregated: f64 = perspectives
        .iter()
        .zip(weights.iter())
        .map(|(p, w)| p.probability * w)
        .sum();

    // Disagreement score: normalized standard deviation
    let mean = perspectives.iter().map(|p| p.probability).sum::<f64>() / perspectives.len() as f64;
    let variance = perspectives
        .iter()
        .map(|p| (p.probability - mean).powi(2))
        .sum::<f64>()
        / perspectives.len() as f64;
    let disagreement = (variance / 0.25).sqrt().min(1.0);

    // Identify dissenting perspective
    let (dissent_idx, _) = perspectives
        .iter()
        .enumerate()
        .map(|(i, p)| (i, (p.probability - aggregated).abs()))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or((0, 0.0));

    let dissent_summary = if disagreement > 0.3 {
        perspectives.get(dissent_idx).and_then(|p| {
            p.rationale.as_ref().map(|r| {
                format!(
                    "Dissenting view ({}: {:.0}%): {}",
                    p.source,
                    p.probability * 100.0,
                    r
                )
            })
        })
    } else {
        None
    };

    let quality = if disagreement < 0.1 {
        "high_consensus"
    } else if disagreement < 0.3 {
        "moderate_consensus"
    } else if disagreement < 0.5 {
        "significant_disagreement"
    } else {
        "polarized"
    };

    let perspective_weights: Vec<(String, f64)> = perspectives
        .iter()
        .zip(weights.iter())
        .map(|(p, w)| (p.source.clone(), *w))
        .collect();

    Ok(DragonflySynthesis {
        event_id: event_id.to_string(),
        perspectives: perspectives.to_vec(),
        aggregated_probability: aggregated,
        disagreement_score: disagreement,
        dissent_summary,
        perspective_weights,
        synthesis_quality: quality.to_string(),
    })
}

// ── Calibration Tracking ────────────────────────────────────────────────────

/// Compute a calibration curve from stored forecasts.
pub fn compute_calibration_curve(store: &ForecastStore) -> Result<CalibrationCurve, ScenarioError> {
    let resolved: Vec<&StoredForecastRecord> = store.resolved();

    if resolved.is_empty() {
        return Err(ScenarioError::NoForecastData);
    }

    let mut bins: Vec<(u64, u64, f64)> = vec![(0, 0, 0.0); 10];
    let mut total_brier = 0.0;

    for record in &resolved {
        let occurred = record.outcome.unwrap_or(false);
        let bin_idx = ((record.probability * 10.0) as usize).min(9);
        bins[bin_idx].0 += 1;
        if occurred {
            bins[bin_idx].1 += 1;
        }
        bins[bin_idx].2 += record.probability;
        total_brier += brier_score(record.probability, occurred);
    }

    let n = resolved.len() as f64;
    let overall_brier = total_brier / n;

    let calibration_bins: Vec<CalibrationBin> = bins
        .iter()
        .enumerate()
        .map(|(i, &(count, hits, probability_sum))| {
            let low = i as f64 * 0.1;
            let high = (i + 1) as f64 * 0.1;
            let hit_rate = if count > 0 {
                hits as f64 / count as f64
            } else {
                f64::NAN
            };
            let expected = if count > 0 {
                probability_sum / count as f64
            } else {
                (low + high) / 2.0
            };
            CalibrationBin {
                probability_range: format!("{:.0}–{:.0}%", low * 100.0, high * 100.0),
                forecast_count: count,
                hit_rate,
                expected_rate: expected,
                bias: if count > 0 { expected - hit_rate } else { 0.0 },
            }
        })
        .collect();

    let mut weighted_bias = 0.0;
    let mut bias_weight = 0.0;
    for bin in &calibration_bins {
        if bin.forecast_count >= 5 {
            weighted_bias += bin.bias * bin.forecast_count as f64;
            bias_weight += bin.forecast_count as f64;
        }
    }
    let overconfidence = if bias_weight > 0.0 {
        weighted_bias / bias_weight
    } else {
        0.0
    };

    let interpretation = if overconfidence > 0.10 {
        "systematically_overconfident"
    } else if overconfidence < -0.10 {
        "systematically_underconfident"
    } else if overconfidence.abs() < 0.05 {
        "well_calibrated"
    } else {
        "moderately_calibrated"
    };

    Ok(CalibrationCurve {
        bins: calibration_bins,
        total_forecasts: store.len() as u64,
        resolved_forecasts: resolved.len() as u64,
        overall_brier,
        overconfidence_score: overconfidence,
        interpretation: interpretation.to_string(),
    })
}

// ── Triage (P4) ────────────────────────────────────────────────────────────

/// Triage a forecasting question to determine if it's worth the full pipeline.
#[must_use = "triage result should be used"]
pub fn triage_question(
    question: &str,
    has_deadline: bool,
    has_reference_class: bool,
    has_resolution_criteria: bool,
) -> TriageAssessment {
    let word_count = question.split_whitespace().count();
    let has_specifics = word_count > 5;

    let clarity = if has_deadline && has_specifics {
        0.8
    } else if has_deadline || has_specifics {
        0.5
    } else {
        0.2
    };

    let data_avail = if has_reference_class { 0.8 } else { 0.3 };
    let resolution = if has_resolution_criteria { 0.9 } else { 0.2 };

    let overall = (clarity + data_avail + resolution) / 3.0;

    let (difficulty, recommend, forecastable) = if overall >= assess_tiers::OVERALL_STRONG {
        (
            "clocklike",
            "Well-specified with clear resolution criteria. Simple base-rate extrapolation may suffice — consider whether the full superforecasting pipeline is worth the effort.",
            true,
        )
    } else if overall >= 0.4 {
        (
            "goldilocks",
            "In the Goldilocks zone — difficult enough to reward careful analysis, specific enough to be scored. Run the full pipeline: Fermi decomposition → outside view → Bayesian updating.",
            true,
        )
    } else {
        (
            "cloudlike",
            "Too vague or lacks clear resolution criteria. Refine: add a specific deadline, define what counts as 'yes', and identify a reference class.",
            false,
        )
    };

    TriageAssessment {
        question: question.to_string(),
        is_forecastable: forecastable,
        difficulty: difficulty.to_string(),
        clarity_score: clarity,
        data_availability_score: data_avail,
        resolution_criteria_clarity: resolution,
        recommendation: recommend.to_string(),
    }
}

// ── Persistence ──────────────────────────────────────────────────────────

use std::fs;
use std::io::Write;
use std::path::PathBuf;

/// File-backed persistence using append-only journal + periodic snapshot compaction.
/// Each mutation appends one JSON line to the journal (O(1) write). On load, the
/// snapshot is loaded first, then journal entries are replayed on top (last write wins).
/// After JOURNAL_COMPACT_THRESHOLD entries, the journal is compacted into a full snapshot.
const JOURNAL_COMPACT_THRESHOLD: usize = 100;

#[derive(Debug, Default)]
pub struct ForecastStore {
    pub records: HashMap<String, StoredForecastRecord>,
    pub data_path: Option<PathBuf>,
    journal_path: Option<PathBuf>,
    journal_count: usize,
}

impl ForecastStore {
    /// Create a new store, loading snapshot + journal replay from disk.
    pub fn new(data_path: Option<PathBuf>) -> Self {
        let journal_path = data_path.as_ref().map(|p| {
            let mut jp = p.clone();
            jp.set_extension("json.journal");
            jp
        });
        let mut store = Self {
            records: HashMap::new(),
            data_path,
            journal_path,
            journal_count: 0,
        };
        store.load();
        store
    }

    /// Load: snapshot first, then replay journal on top (last write wins).
    fn load(&mut self) {
        if let Some(ref path) = self.data_path
            && path.exists()
            && let Ok(data) = fs::read_to_string(path)
            && let Ok(records) =
                serde_json::from_str::<HashMap<String, StoredForecastRecord>>(&data)
        {
            self.records = records;
        }
        if let Some(ref jp) = self.journal_path
            && jp.exists()
            && let Ok(data) = fs::read_to_string(jp)
        {
            for line in data.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<serde_json::Value>(trimmed)
                    && let (Some(key), Some(record)) = (
                        entry.get("key").and_then(|v| v.as_str()),
                        entry.get("record"),
                    )
                    && let Ok(rec) = serde_json::from_value::<StoredForecastRecord>(record.clone())
                {
                    self.records.insert(key.to_string(), rec);
                    self.journal_count += 1;
                }
            }
        }
    }

    /// Append a single record entry to the journal (O(1) write per mutation).
    /// Only writes the changed record, not the full dataset.
    fn save_entry(&self, key: &str, record: &StoredForecastRecord) {
        if let (Some(jp), Some(dp)) = (&self.journal_path, &self.data_path) {
            if let Some(parent) = dp.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(jp)
                && let Ok(line) = serde_json::to_string(&serde_json::json!({
                    "key": key,
                    "record": record
                }))
            {
                let _ = writeln!(file, "{}", line);
            }
        }
    }

    /// Insert a record and persist via single-entry journal append.
    pub fn insert(&mut self, key: String, record: StoredForecastRecord) {
        self.save_entry(&key, &record);
        self.records.insert(key, record);
        self.journal_count += 1;
        if self.journal_count >= JOURNAL_COMPACT_THRESHOLD {
            self.compact();
        }
    }

    pub fn get(&self, key: &str) -> Option<&StoredForecastRecord> {
        self.records.get(key)
    }

    /// Get mutable reference. Caller must call persist() after modification.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut StoredForecastRecord> {
        self.records.get_mut(key)
    }

    /// Persist all changes (writes full snapshot, truncates journal).
    pub fn persist(&self) {
        self.compact();
    }

    /// Compact: write full snapshot, truncate journal.
    fn compact(&self) {
        if let Some(ref dp) = self.data_path {
            if let Some(parent) = dp.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(data) = serde_json::to_string_pretty(&self.records) {
                let _ = fs::write(dp, data);
                if let Some(ref jp) = self.journal_path {
                    let _ = fs::write(jp, "");
                }
            }
        }
    }

    /// Force compaction regardless of threshold.
    pub fn force_compact(&self) {
        self.compact();
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if the forecast store contains no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &StoredForecastRecord> {
        self.records.values()
    }

    pub fn resolved(&self) -> Vec<&StoredForecastRecord> {
        self.records
            .values()
            .filter(|r| r.outcome.is_some())
            .collect()
    }

    pub(crate) fn filtered_by_subject(&self, subject: &str) -> Self {
        Self {
            records: self
                .records
                .iter()
                .filter(|(_, record)| record.subject == subject)
                .map(|(key, record)| (key.clone(), record.clone()))
                .collect(),
            data_path: None,
            journal_path: None,
            journal_count: 0,
        }
    }
}

// ── Cross-Validation ──────────────────────────────────────────────────────

/// Cross-validate two probability estimates for the same event.
///
/// Typically compares an LLM-generated estimate (from the superforecasting
/// skill) against a server-computed estimate (from scenario_calibrate).
///
/// Computes per-sub-question divergence to identify where the estimates
/// differ most. Flags for review when overall divergence exceeds the
/// threshold (default 0.15).
///
/// This closes the learning loop between LLM reasoning and computational
/// verification — the key bridge between the superforecasting skill and
/// the scenarios MCP server.
#[allow(clippy::too_many_arguments)]
#[must_use = "validation result should be inspected"]
pub fn cross_validate(
    event_id: &str,
    source_a: &str,
    estimate_a: f64,
    sub_questions_a: &[SubQuestion],
    source_b: &str,
    estimate_b: f64,
    sub_questions_b: &[SubQuestion],
    threshold: Option<f64>,
) -> CrossValidation {
    let review_threshold = threshold.unwrap_or(0.15);
    let divergence = (estimate_a - estimate_b).abs();
    let requires_review = divergence > review_threshold;

    // Match sub-questions by index (best-effort alignment)
    let max_sq = sub_questions_a.len().max(sub_questions_b.len());
    let mut sq_divergences = Vec::new();
    for i in 0..max_sq {
        let sq_a = sub_questions_a.get(i);
        let sq_b = sub_questions_b.get(i);
        let question = sq_a
            .map(|s| s.question.as_str())
            .or_else(|| sq_b.map(|s| s.question.as_str()))
            .unwrap_or("unknown");
        let est_a = sq_a.map(|s| s.estimate).unwrap_or(0.5);
        let est_b = sq_b.map(|s| s.estimate).unwrap_or(0.5);
        let sq_div = (est_a - est_b).abs();
        sq_divergences.push(SubQuestionDivergence {
            question: question.to_string(),
            estimate_a: est_a,
            estimate_b: est_b,
            divergence: sq_div,
        });
    }

    let recommendation = if !requires_review {
        format!(
            "Estimates are consistent (divergence {:.3} <= threshold {:.3}). No review needed.",
            divergence, review_threshold
        )
    } else {
        let max_sq_div = sq_divergences
            .iter()
            .map(|d| d.divergence)
            .fold(0.0_f64, f64::max);
        let top_sq = sq_divergences
            .iter()
            .max_by(|a, b| {
                a.divergence
                    .partial_cmp(&b.divergence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|d| d.question.as_str())
            .unwrap_or("unknown");
        format!(
            "Estimates diverge ({:.3} > {:.3}). Largest sub-question divergence ({:.3}) on '{}'. Activate grill-me skill.",
            divergence, review_threshold, max_sq_div, top_sq
        )
    };

    let grill_me_questions: Vec<String> = if requires_review {
        let mut questions = vec![format!(
            "What hidden assumptions could explain the {:.1}% divergence between '{}' and '{}' on event '{}'?",
            divergence * 100.0,
            source_a,
            source_b,
            event_id
        )];
        for sq in sq_divergences.iter().take(3) {
            if sq.divergence > 0.05 {
                questions.push(format!(
                    "Sub-question '{}': why does {} estimate {:.0}% while {} estimates {:.0}%?",
                    sq.question,
                    source_a,
                    sq.estimate_a * 100.0,
                    source_b,
                    sq.estimate_b * 100.0
                ));
            }
        }
        questions
    } else {
        Vec::new()
    };

    CrossValidation {
        event_id: event_id.to_string(),
        estimate_a,
        source_a: source_a.to_string(),
        estimate_b,
        source_b: source_b.to_string(),
        divergence,
        requires_review,
        review_threshold,
        sub_question_divergences: sq_divergences,
        recommendation,
        grill_me_questions,
    }
}

// ── Companies Server Bridge ────────────────────────────────────────────────

/// Convert a companies server calibrate_forecast output into ScenarioEvents
/// that can be quantified by the scenarios pipeline.
///
/// The companies server produces Schwartz 2×2 scenario results with
/// intrinsic values per quadrant. This function converts those into
/// binomial events with Fermi sub-questions, ready for scenario_quantify
/// and scenario_calibrate.
///
/// Bridge path: companies.calibrate_forecast → this function → scenario_quantify → scenario_synthesize
pub fn convert_companies_output(
    symbol: &str,
    companies_json: &serde_json::Value,
    time_horizon: TimeHorizon,
) -> Result<Vec<ScenarioEvent>, ScenarioError> {
    let scenarios = companies_json
        .get("scenarios")
        .and_then(|s| s.as_array())
        .ok_or(ScenarioError::NoEvents)?;

    let mut events = Vec::new();
    // Derive deadline from time horizon
    let today = chrono::Utc::now().date_naive();
    let deadline = match time_horizon {
        TimeHorizon::Tactical => today + chrono::TimeDelta::days(540),
        TimeHorizon::Strategic => today + chrono::TimeDelta::days(1460),
        TimeHorizon::LongTerm => today + chrono::TimeDelta::days(2920),
    };

    for (i, scenario) in scenarios.iter().enumerate() {
        let name = scenario
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");
        let intrinsic = scenario
            .get("intrinsic_per_share")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let current_price = companies_json
            .get("current_price")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        let upside = if current_price > 0.0 {
            (intrinsic - current_price) / current_price
        } else {
            0.0
        };

        let question = format!(
            "Will {} trade within 20% of the {} scenario intrinsic value ({:.2}) by {}",
            symbol,
            name.to_lowercase(),
            intrinsic,
            deadline.format("%Y-%m-%d")
        );

        let growth = scenario.get("applied_growth").and_then(|v| v.as_f64());
        let margin = scenario.get("applied_margin").and_then(|v| v.as_f64());

        let mut sub_questions = Vec::new();
        if let Some(g) = growth {
            sub_questions.push(SubQuestion {
                question: format!("Will revenue growth reach {:.0}%?", g * 100.0),
                estimate: if g > 0.1 { 0.6 } else { 0.4 },
                confidence: 0.5,
            });
        }
        if let Some(m) = margin {
            sub_questions.push(SubQuestion {
                question: format!("Will gross margins hold at {:.0}%?", m * 100.0),
                estimate: if m > 0.4 { 0.6 } else { 0.4 },
                confidence: 0.5,
            });
        }

        // Probability: Fermi-calibrate from sub-questions when available.
        let prob = if !sub_questions.is_empty() {
            calibrate_from_fermi(&sub_questions).unwrap_or_else(|_| {
                if upside > 0.2 {
                    0.65
                } else if upside > 0.0 {
                    0.55
                } else if upside > -0.2 {
                    0.40
                } else {
                    0.25
                }
            })
        } else if upside > 0.2 {
            0.65
        } else if upside > 0.0 {
            0.55
        } else if upside > -0.2 {
            0.40
        } else {
            0.25
        };

        events.push(ScenarioEvent {
            id: format!("comp-{}-{}", symbol, i),
            name: format!("{} {}", symbol, name),
            question,
            deadline,
            time_horizon,
            scenario_type: ScenarioType::CompanyAnalysis,
            subject: symbol.to_string(),
            probability: prob,
            basis: Some("financial_model".into()),
            depends_on: vec![],
            sub_questions,
            base_rate: None,
            reference_class: Some("Company DCF scenario analysis, 2×2 Schwartz matrix".into()),
            brier_score: None,
            update_count: 0,
        });
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CertaintyTier;
    use crate::types::EventDependency;

    fn make_event(id: &str, prob: f64, deps: Vec<EventDependency>) -> ScenarioEvent {
        ScenarioEvent {
            id: id.into(),
            name: format!("Event {}", id),
            question: format!("Will {} occur?", id),
            deadline: chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
            time_horizon: TimeHorizon::Strategic,
            scenario_type: ScenarioType::CompanyAnalysis,
            subject: "TEST".into(),
            probability: prob,
            basis: None,
            depends_on: deps,
            sub_questions: vec![],
            base_rate: None,
            reference_class: None,
            brier_score: None,
            update_count: 0,
        }
    }

    #[test]
    fn test_calibrate_from_fermi_simple() {
        let sqs = vec![
            SubQuestion {
                question: "a".into(),
                estimate: 0.8,
                confidence: 0.9,
            },
            SubQuestion {
                question: "b".into(),
                estimate: 0.2,
                confidence: 0.1,
            },
        ];
        let result = calibrate_from_fermi(&sqs).unwrap();
        assert!((result - 0.74).abs() < 0.001);
    }

    #[test]
    fn test_calibrate_empty_returns_neutral() {
        assert_eq!(calibrate_from_fermi(&[]).unwrap(), 0.5);
    }

    #[test]
    fn test_calibrate_nan_rejected() {
        let sqs = vec![SubQuestion {
            question: "nan".into(),
            estimate: f64::NAN,
            confidence: 0.5,
        }];
        let result = calibrate_from_fermi(&sqs);
        assert!(result.is_err());
    }

    #[test]
    fn test_calibrate_inf_rejected() {
        let sqs = vec![SubQuestion {
            question: "inf".into(),
            estimate: f64::INFINITY,
            confidence: 0.5,
        }];
        let result = calibrate_from_fermi(&sqs);
        assert!(result.is_err());
    }

    #[test]
    fn test_outside_view_high_reference_count() {
        let (prob, conf) = outside_view_adjustment(0.7, 0.3, 1000);
        assert!(prob > 0.6);
        assert!(conf > 0.7);
    }

    #[test]
    fn test_outside_view_low_reference_count() {
        let (prob, _conf) = outside_view_adjustment(0.9, 0.5, 1);
        assert!((prob - 0.55).abs() < 0.01);
    }

    #[test]
    fn test_bayesian_update_positive_evidence() {
        let posterior = bayesian_update(0.3, 0.9, 0.3);
        assert!((posterior - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_bayesian_update_with_negative_evidence() {
        let posterior = bayesian_update(0.7, 0.1, 0.4);
        assert!((posterior - 0.175).abs() < 0.01);
    }

    #[test]
    fn test_brier_perfect() {
        assert_eq!(brier_score(1.0, true), 0.0);
        assert_eq!(brier_score(0.0, false), 0.0);
    }

    #[test]
    fn test_brier_worst() {
        assert_eq!(brier_score(0.0, true), 1.0);
        assert_eq!(brier_score(1.0, false), 1.0);
    }

    #[test]
    fn test_brier_mid() {
        assert_eq!(brier_score(0.5, true), 0.25);
        assert_eq!(brier_score(0.5, false), 0.25);
    }

    #[test]
    fn test_brier_multi_ok() {
        let result = brier_score_multi(&[0.8, 0.2], &[true, false]).unwrap();
        // (0.8-1)^2=0.04, (0.2-0)^2=0.04, avg=0.04
        assert!((result - 0.04).abs() < 0.001);
    }

    #[test]
    fn test_brier_multi_mismatch_err() {
        let result = brier_score_multi(&[0.8], &[true, false]);
        assert!(result.is_err());
    }

    #[test]
    fn test_brier_multi_empty_err() {
        let result = brier_score_multi(&[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_brier_interpretation_excellent() {
        assert_eq!(brier_interpretation(0.03), "excellent");
    }

    #[test]
    fn test_brier_interpretation_worse() {
        assert_eq!(brier_interpretation(0.5), "worse_than_climatology");
    }

    #[test]
    fn test_event_tree_no_deps() {
        let events = vec![make_event("A", 0.8, vec![]), make_event("B", 0.6, vec![])];
        let tree = build_event_tree(&events).unwrap();
        assert_eq!(tree.nodes.len(), 2);
        assert_eq!(tree.root_ids.len(), 2);
        assert!((tree.joint_probability - 0.48).abs() < 0.01);
    }

    #[test]
    fn test_event_tree_with_dependency() {
        let dep = vec![EventDependency {
            parent_event_ids: vec!["A".into()],
            conditionals: vec![0.2, 0.9], // [P(E|not A), P(E|A)]
        }];
        let events = vec![make_event("A", 0.5, vec![]), make_event("B", 0.7, dep)];
        let tree = build_event_tree(&events).unwrap();
        assert_eq!(tree.nodes.len(), 2);
        let b_node = tree.nodes.iter().find(|n| n.event.id == "B").unwrap();
        assert!((b_node.marginal_probability - 0.55).abs() < 0.01);
        assert!((tree.joint_probability - 0.45).abs() < 0.01);
    }

    #[test]
    fn test_event_tree_cycle_detection() {
        let dep_a = vec![EventDependency {
            parent_event_ids: vec!["B".into()],
            conditionals: vec![0.3, 0.8],
        }];
        let dep_b = vec![EventDependency {
            parent_event_ids: vec!["A".into()],
            conditionals: vec![0.3, 0.8],
        }];
        let events = vec![make_event("A", 0.5, dep_a), make_event("B", 0.5, dep_b)];
        let result = build_event_tree(&events);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ScenarioError::CycleDetected));
    }

    #[test]
    fn test_event_tree_multi_parent_independence() {
        // Two independent root events A and B, with child C depending on both.
        // P(A) = 0.5, P(B) = 0.8
        // Bitmap convention: bit j = parent_event_ids[j] (0=A, 1=B)
        //   conditionals[0b00] = P(C | ¬A, ¬B) = 0.05
        //   conditionals[0b01] = P(C |  A, ¬B) = 0.30
        //   conditionals[0b10] = P(C | ¬A,  B) = 0.40
        //   conditionals[0b11] = P(C |  A,  B) = 0.90
        //
        // Under parent independence:
        //   P(¬A,¬B) = 0.10, P(A,¬B) = 0.10, P(¬A,B) = 0.40, P(A,B) = 0.40
        // P(C) = 0.05*0.10 + 0.30*0.10 + 0.40*0.40 + 0.90*0.40 = 0.555
        // Joint P(all) = P(A) * P(B) * P(C | A=true, B=true) = 0.5 * 0.8 * 0.90 = 0.36
        let dep_c = vec![EventDependency {
            parent_event_ids: vec!["A".into(), "B".into()],
            // bitmap: 00=¬A¬B, 01=A¬B, 10=¬AB, 11=AB
            conditionals: vec![0.05, 0.30, 0.40, 0.90],
        }];
        let events = vec![
            make_event("A", 0.5, vec![]),
            make_event("B", 0.8, vec![]),
            make_event("C", 0.3, dep_c),
        ];
        let tree = build_event_tree(&events).unwrap();
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.root_ids.len(), 2);

        let c_node = tree.nodes.iter().find(|n| n.event.id == "C").unwrap();
        let expected_marginal = 0.555;
        assert!(
            (c_node.marginal_probability - expected_marginal).abs() < 0.001,
            "P(C) = {} expected {} under independence",
            c_node.marginal_probability,
            expected_marginal
        );

        let expected_joint = 0.36;
        assert!(
            (tree.joint_probability - expected_joint).abs() < 0.001,
            "joint = {} expected {}",
            tree.joint_probability,
            expected_joint
        );
    }

    #[test]
    fn test_sensitivity_ranking() {
        let events = vec![
            make_event("A", 0.5, vec![]),  // max uncertainty (coin flip)
            make_event("B", 0.99, vec![]), // near certainty
        ];
        let tree = build_event_tree(&events).unwrap();
        let ranked = sensitivity_ranking(&tree);
        assert_eq!(ranked[0].0, "A");
        assert_eq!(ranked[1].0, "B");
    }

    #[test]
    fn test_validate_nan_rejected() {
        let event = make_event("A", f64::NAN, vec![]);
        let result = event.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_inf_rejected() {
        let event = make_event("A", f64::INFINITY, vec![]);
        let result = event.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_wrong_conditionals_length_rejected() {
        // 2 parents require 4 conditionals, but we provide only 3
        let dep = EventDependency {
            parent_event_ids: vec!["A".into(), "B".into()],
            conditionals: vec![0.1, 0.3, 0.7], // should be length 4
        };
        let event = make_event("C", 0.5, vec![dep]);
        let result = event.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_calibration_bias_uses_mean_forecast_probability() {
        let mut store = ForecastStore::default();
        for (id, probability, outcome) in [
            ("a", 0.81, false),
            ("b", 0.83, false),
            ("c", 0.85, false),
            ("d", 0.87, false),
            ("e", 0.89, false),
        ] {
            store.insert(
                id.into(),
                StoredForecastRecord {
                    schema_version: 1,
                    forecast_id: "forecast".into(),
                    event_id: id.into(),
                    event_name: id.into(),
                    subject: "test".into(),
                    probability,
                    created_at: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                    outcome: Some(outcome),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
                },
            );
        }

        let curve = compute_calibration_curve(&store).unwrap();
        let bin = curve
            .bins
            .iter()
            .find(|bin| bin.forecast_count == 5)
            .unwrap();

        assert!((bin.expected_rate - 0.85).abs() < f64::EPSILON);
        assert!((bin.bias - 0.85).abs() < f64::EPSILON);
        assert!(curve.overconfidence_score > 0.0);
        assert_eq!(curve.interpretation, "systematically_overconfident");
    }

    #[test]
    fn test_auto_update_suggestions_correct_direction() {
        let events = vec![make_event("A", 0.3, vec![])];
        let outcomes = vec![("A".into(), true)]; // event occurred but forecast was 30%
        let suggestions = auto_update_suggestions(&events, &outcomes);
        assert_eq!(suggestions.len(), 1);
        let adj = suggestions[0]["suggested_adjustment"].as_f64().unwrap();
        assert!(adj > 0.0); // should suggest raising probability
    }

    // ── E5: CertaintyTier boundary tests ──────────────────────────────────

    #[test]
    fn test_certainty_tier_exact_boundaries() {
        // Exact boundary values: Proximate ≥ 0.67, Probable in [0.33, 0.67), Possible < 0.33
        assert!(
            matches!(
                CertaintyTier::from_probability(0.67),
                CertaintyTier::Proximate
            ),
            "0.67 should be Proximate boundary (≥ 0.67)"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.33),
                CertaintyTier::Probable
            ),
            "0.33 should be Probable (≥ 0.33 ∧ < 0.67)"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.329),
                CertaintyTier::Possible
            ),
            "0.329 should be Possible (< 0.33)"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.669),
                CertaintyTier::Probable
            ),
            "0.669 should be Probable (< 0.67)"
        );
    }

    #[test]
    fn test_certainty_tier_range() {
        assert_eq!(CertaintyTier::Proximate.range(), "67–100%");
        assert_eq!(CertaintyTier::Probable.range(), "33–66%");
        assert_eq!(CertaintyTier::Possible.range(), "0–32%");
    }

    #[test]
    fn test_certainty_tier_edges() {
        assert!(
            matches!(
                CertaintyTier::from_probability(0.0),
                CertaintyTier::Possible
            ),
            "0.0 should be Possible"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(1.0),
                CertaintyTier::Proximate
            ),
            "1.0 should be Proximate"
        );
        assert!(
            matches!(
                CertaintyTier::from_probability(0.5),
                CertaintyTier::Probable
            ),
            "0.5 should be Probable (middle of range)"
        );
    }

    // ── Persistence round-trip tests ──────────────────────────────────────

    #[test]
    fn test_journal_insert_and_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("forecasts.json");

        // Phase 1: Insert records
        {
            let mut store = ForecastStore::new(Some(path.clone()));
            store.insert(
                "fcst-1:evt-A".into(),
                StoredForecastRecord {
                    schema_version: 1,
                    forecast_id: "fcst-1".into(),
                    event_id: "evt-A".into(),
                    event_name: "Event A".into(),
                    subject: "TEST".into(),
                    probability: 0.75,
                    created_at: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                    outcome: None,
                    resolved_at: None,
                },
            );
            store.insert(
                "fcst-1:evt-B".into(),
                StoredForecastRecord {
                    schema_version: 1,
                    forecast_id: "fcst-1".into(),
                    event_id: "evt-B".into(),
                    event_name: "Event B".into(),
                    subject: "TEST".into(),
                    probability: 0.30,
                    created_at: chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                    outcome: Some(true),
                    resolved_at: Some(chrono::NaiveDate::from_ymd_opt(2025, 6, 1).unwrap()),
                },
            );
            store.force_compact(); // ensure snapshot is written
        }

        // Phase 2: Reload from disk
        {
            let store = ForecastStore::new(Some(path));
            assert_eq!(store.len(), 2, "both records should survive restart");

            let a = store.get("fcst-1:evt-A").expect("evt-A should exist");
            assert_eq!(a.probability, 0.75);
            assert!(a.outcome.is_none());

            let b = store.get("fcst-1:evt-B").expect("evt-B should exist");
            assert_eq!(b.probability, 0.30);
            assert_eq!(b.outcome, Some(true));
            assert_eq!(store.resolved().len(), 1);
        }
    }
}
