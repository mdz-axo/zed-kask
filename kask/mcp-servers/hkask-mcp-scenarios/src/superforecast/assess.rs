//! Assessment — Chermack project assessment (P5), Dragonfly-Eye perspective
//! synthesis (P1), calibration-curve tracking, and question triage (P4). Pure
//! scoring/synthesis over a `ForecastStore`; no I/O of its own.
//!
//! Extracted from `superforecast.rs` (deep-module split).

use super::ForecastStore;
use super::brier_score;

use crate::types::{
    AssessInput, CalibrationBin, CalibrationCurve, DragonflySynthesis, Perspective, PhaseScore,
    ProjectAssessment, ScenarioError, StoredForecastRecord, TriageAssessment,
};
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

/// Names of the seven quantitative metrics the caller did not report, in
/// `AssessInput` declaration order.
fn unreported_metric_names(input: &AssessInput) -> Vec<&'static str> {
    let mut missing = Vec::new();
    // With a framing document, Phase 1 scores from its typed fields —
    // perspective_count is not required (PR-01).
    if input.perspective_count.is_none() && input.framing_document.is_none() {
        missing.push("perspective_count");
    }
    if input.disagreement_score.is_none() {
        missing.push("disagreement_score");
    }
    if input.event_count.is_none() {
        missing.push("event_count");
    }
    if input.events_with_deps.is_none() {
        missing.push("events_with_dependencies");
    }
    if input.strategies_generated.is_none() {
        missing.push("strategies_generated");
    }
    if input.strategies_implemented.is_none() {
        missing.push("strategies_implemented");
    }
    if input.has_early_warning_indicators.is_none() {
        missing.push("has_early_warning_indicators");
    }
    missing
}

/// The subset of `dependent` metrics the caller did not report, in the order
/// given. Empty when everything the phase needs was reported.
fn unreported_subset(unreported: &[&'static str], dependent: &[&'static str]) -> Vec<&'static str> {
    dependent
        .iter()
        .copied()
        .filter(|name| unreported.contains(name))
        .collect()
}

/// The gap text for a phase withheld because its dependent metrics were not
/// reported: names the missing metrics so the caller knows what to supply.
fn insufficient_data_gap(missing: &[&str]) -> String {
    format!(
        "Score withheld — insufficient data: {} not reported by the caller",
        missing.join(", ")
    )
}

/// Assess a scenario project across Chermack's five performance phases.
///
/// Evaluates whether the scenario project was worth doing — not just
/// whether forecasts were accurate. Combines quantitative metrics
/// (Brier scores, disagreement, calibration) with qualitative assessment
/// of preparation, exploration, implementation, and learning.
///
/// A quantitative metric the caller did not report is not a zero
/// measurement: the phase whose score depends on it is withheld as
/// insufficient data (null score + a gap naming the metric), the output
/// names every unreported metric, and the overall score averages only the
/// phases with reported data. Phase 1 (Preparation) is scored from the
/// project's framing document when one is supplied — the typed fields are
/// the preparation evidence — with perspective_count as the fallback
/// proxy when no framing document exists.
///
/// Reference: Chermack, T.J. (2011). Scenario Planning in Organizations:
/// How to Create, Use, and Assess Scenarios. Berrett-Koehler.
pub(crate) fn assess_project(input: &AssessInput) -> ProjectAssessment {
    let project_id = input.project_id;
    let subject = input.subject;
    let learning_events = &input.learning_events;
    let calibration_curve = input.calibration_curve;
    let unreported = unreported_metric_names(input);

    // ── Phase 1: Preparation ──────────────────────────────────
    // (Chermack, Ch. 5): Scope clarity, stakeholder engagement, resource
    // allocation. Scored from the project's framing document when present
    // (PR-01) — the typed fields ARE the preparation evidence;
    // perspective_count remains the fallback proxy when no framing
    // document exists.
    let (prep_score, prep_strengths, prep_gaps) = if let Some(framing) = input.framing_document {
        let mut score = 0.0_f64;
        let mut strengths: Vec<String> = Vec::new();
        let mut gaps: Vec<String> = Vec::new();
        // Scope clarity: a framed focal question with both-side bounds.
        if !framing.focal_question.trim().is_empty() {
            score += 0.3;
            strengths.push("Focal question framed".into());
        } else {
            gaps.push(
                "No focal question — the project lacks a decision anchor (Chermack Phase 1)".into(),
            );
        }
        if !framing.in_scope.is_empty() && !framing.out_of_scope.is_empty() {
            score += 0.3;
            strengths.push("Scope bounded on both sides (in and out)".into());
        } else {
            gaps.push("Scope not bounded on both sides — in_scope/out_of_scope incomplete".into());
        }
        // Stakeholder engagement.
        if framing.stakeholders.len() >= 2 {
            score += 0.2;
            strengths.push(format!(
                "{} stakeholders engaged",
                framing.stakeholders.len()
            ));
        } else {
            gaps.push(
                "Fewer than 2 stakeholders — limited engagement diversity (Chermack: stakeholder dialogue)"
                    .into(),
            );
        }
        // Decision timing.
        if framing.action_deadline.is_some() {
            score += 0.2;
            strengths.push("Action deadline set".into());
        } else {
            gaps.push("No action deadline — the decision timing is unbounded".into());
        }
        (Some(score), strengths, gaps)
    } else {
        match input.perspective_count {
            Some(perspective_count) => {
                let score = if perspective_count >= assess_tiers::PREP_PERSPECTIVE_HIGH {
                    assess_tiers::PREP_STRONG
                } else if perspective_count >= 2 {
                    0.6
                } else {
                    0.3
                };
                let mut strengths = Vec::new();
                let mut gaps = Vec::new();
                if perspective_count >= 3 {
                    strengths.push("Multiple perspectives engaged".into());
                } else if perspective_count == 0 {
                    gaps.push(
                        "No perspectives recorded — project may lack stakeholder engagement (Chermack Phase 1)"
                            .into(),
                    );
                } else {
                    gaps.push(format!("Only {} perspective(s) — consider engaging more diverse viewpoints (Chermack: stakeholder dialogue)", perspective_count));
                }
                (Some(score), strengths, gaps)
            }
            None => (
                None,
                Vec::new(),
                vec![insufficient_data_gap(&unreported_subset(
                    &unreported,
                    &["perspective_count"],
                ))],
            ),
        }
    };

    // ── Phase 2: Exploration ─────────────────────────────
    // (Chermack, Ch. 6): Driving forces identified, trends mapped, uncertainties surfaced
    let (exp_score, exp_strengths, exp_gaps) = match (input.event_count, input.disagreement_score) {
        (Some(event_count), Some(disagreement_score)) => {
            let score = if event_count >= assess_tiers::EXP_EVENT_HIGH
                && disagreement_score > assess_tiers::EXP_DISAGREEMENT_HIGH
            {
                assess_tiers::EXP_STRONG
            } else if event_count >= assess_tiers::EXP_EVENT_MID {
                assess_tiers::EXP_ADEQUATE
            } else {
                assess_tiers::EXP_WEAK
            };
            let mut strengths = Vec::new();
            let mut gaps = Vec::new();
            if disagreement_score > assess_tiers::EXP_DISAGREEMENT_SIGNIFICANT {
                strengths.push(format!("Significant disagreement ({:.0}%) detected — healthy diversity of views (Chermack: conversation quality)", disagreement_score * 100.0));
            }
            if event_count >= assess_tiers::EXP_EVENT_HIGH {
                strengths.push(format!(
                    "{} events identified — comprehensive force mapping",
                    event_count
                ));
            } else {
                gaps.push(format!(
                    "Only {} events — consider deeper STEEP force mapping",
                    event_count
                ));
            }
            if disagreement_score < assess_tiers::EXP_DISAGREEMENT_GROUPTHINK && event_count > 0 {
                gaps.push("Very low disagreement — potential groupthink. Chermack warns against false consensus in scenario exploration.".into());
            }
            (Some(score), strengths, gaps)
        }
        _ => (
            None,
            Vec::new(),
            vec![insufficient_data_gap(&unreported_subset(
                &unreported,
                &["event_count", "disagreement_score"],
            ))],
        ),
    };

    // ── Phase 3: Development ─────────────────────────────
    // (Chermack, Ch. 7): Scenario logic, internal consistency, narrative quality
    let (dev_score, dev_strengths, dev_gaps) = match (input.event_count, input.events_with_deps) {
        (Some(event_count), Some(events_with_deps)) => {
            let dep_ratio = if event_count > 0 {
                events_with_deps as f64 / event_count as f64
            } else {
                0.0
            };
            let score = if dep_ratio > assess_tiers::DEV_RATIO_HIGH
                && event_count >= assess_tiers::DEV_EVENT_MIN
            {
                assess_tiers::DEV_STRONG
            } else if dep_ratio > assess_tiers::DEV_RATIO_MID {
                assess_tiers::DEV_ADEQUATE
            } else {
                assess_tiers::DEV_WEAK
            };
            let mut strengths = Vec::new();
            let mut gaps = Vec::new();
            if dep_ratio > assess_tiers::DEV_RATIO_HIGH {
                strengths.push(format!("{:.0}% of events have conditional dependencies — structured causal reasoning (Chermack: internal consistency)", dep_ratio * 100.0));
            } else {
                gaps.push("Most events lack dependency links. Chermack requires internal consistency: events should form a causal chain, not a list.".into());
            }
            if event_count < assess_tiers::DEV_EVENT_MIN {
                gaps.push("Fewer than 4 events — scenarios may lack sufficient structure for meaningful narratives.".into());
            }
            (Some(score), strengths, gaps)
        }
        _ => (
            None,
            Vec::new(),
            vec![insufficient_data_gap(&unreported_subset(
                &unreported,
                &["event_count", "events_with_dependencies"],
            ))],
        ),
    };

    // ── Phase 4: Implementation ──────────────────────────
    // (Chermack, Ch. 8): Strategies applied, wind-tunneling, early warning systems
    let (impl_score, impl_strengths, impl_gaps) = match (
        input.strategies_generated,
        input.strategies_implemented,
        input.has_early_warning_indicators,
    ) {
        (
            Some(strategies_generated),
            Some(strategies_implemented),
            Some(has_early_warning_indicators),
        ) => {
            let score = if strategies_implemented > 0 && has_early_warning_indicators {
                assess_tiers::IMPL_STRONG
            } else if strategies_generated > 0 {
                assess_tiers::IMPL_ADEQUATE
            } else {
                assess_tiers::IMPL_WEAK
            };
            let mut strengths = Vec::new();
            let mut gaps = Vec::new();
            if strategies_implemented > 0 {
                strengths.push(format!(
                    "{} strategies implemented — scenario insights drove action (Chermack Phase 4)",
                    strategies_implemented
                ));
            }
            if strategies_generated > 0 && strategies_implemented == 0 {
                gaps.push(format!("{} strategies generated but none implemented — the scenario-to-action gap (Chermack's critical Phase 4)", strategies_generated));
            }
            if !has_early_warning_indicators {
                gaps.push("No early warning indicators defined. Chermack: scenarios without tripwires are stories without sensors.".into());
            }
            (Some(score), strengths, gaps)
        }
        _ => (
            None,
            Vec::new(),
            vec![insufficient_data_gap(&unreported_subset(
                &unreported,
                &[
                    "strategies_generated",
                    "strategies_implemented",
                    "has_early_warning_indicators",
                ],
            ))],
        ),
    };

    // ── Phase 5: Project Assessment ──────────────────────
    // (Chermack, Ch. 9): Did the project improve decision quality? Learning outcomes?
    // Scored from learning_events (a caller-supplied list) and the optional
    // calibration curve — neither is one of the seven quantitative metrics,
    // so this phase always reports.
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

    // ── Composite ─────────────────────────
    // Average only over phases with reported data; a withheld phase
    // contributes nothing (its score is null, not a fabricated zero).
    let reported_scores: Vec<f64> = [
        prep_score,
        exp_score,
        dev_score,
        impl_score,
        Some(assess_score),
    ]
    .into_iter()
    .flatten()
    .collect();
    let overall = if reported_scores.is_empty() {
        None
    } else {
        Some(reported_scores.iter().sum::<f64>() / reported_scores.len() as f64)
    };

    let assessment_text = match overall {
        Some(overall) if overall >= assess_tiers::OVERALL_STRONG => {
            "Strong scenario project. Preparation was thorough, exploration surfaced diverse views, scenarios are causally structured, insights drove action, and learning is being tracked. Continue deepening the calibration loop."
        }
        Some(overall) if overall >= assess_tiers::OVERALL_ADEQUATE => {
            "Adequate scenario project with room for improvement. Strengthen the weakest phases (see per-phase gaps below). Focus on closing the implementation gap: scenarios without action are entertainment."
        }
        Some(overall) if overall >= assess_tiers::OVERALL_FOUNDATIONAL => {
            "Foundational scenario project. Core elements are present but significant gaps remain. Priority: engage more perspectives (Phase 1), add conditional dependencies (Phase 3), and track outcomes (Phase 5)."
        }
        Some(_) => {
            "Early-stage scenario project. The scaffolding exists but lacks depth. Start with Phase 1 (preparation): define the focal question clearly and engage multiple perspectives before building scenarios."
        }
        None => {
            "Insufficient data — no phase could be scored. Report the quantitative metrics named in unreported_metrics so the withheld phases can be assessed."
        }
    };

    let mut recommendations = Vec::new();
    if prep_score.is_some_and(|score| score < assess_tiers::RECOMMENDATION_THRESHOLD) {
        recommendations.push("Phase 1 (Preparation): Engage at least 3 diverse perspectives. Chermack: 'The quality of the conversation determines the quality of the scenarios.'".into());
    }
    if exp_score.is_some_and(|score| score < assess_tiers::RECOMMENDATION_THRESHOLD) {
        recommendations.push("Phase 2 (Exploration): Map more driving forces. Run web searches to gather external data. Chermack: systematic STEEP analysis prevents blind spots.".into());
    }
    if dev_score.is_some_and(|score| score < assess_tiers::RECOMMENDATION_THRESHOLD) {
        recommendations.push("Phase 3 (Development): Link events with conditional dependencies. Scenarios must form causal chains, not lists. Chermack: internal consistency is the quality gate.".into());
    }
    if impl_score.is_some_and(|score| score < assess_tiers::RECOMMENDATION_THRESHOLD) {
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
            score: Some(assess_score),
            strengths: assess_strengths,
            gaps: assess_gaps,
        },
        unreported_metrics: unreported.iter().map(|m| m.to_string()).collect(),
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
pub(crate) fn synthesize_perspectives(
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
pub(crate) fn triage_question(
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
            "well_specified",
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
            "needs_refinement",
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
