//! Superforecasting domain layer for the companies MCP server.
//!
//! The pure-math Tetlock primitives (Fermi averaging, shrinkage, Bayes,
//! Brier) live in the `hkask-forecast` crate — this module holds only
//! companies-specific composition: default Fermi sub-questions, override
//! application, the 2×2 growth×margin scenario distribution, and
//! intrinsic-value aggregation. Call sites invoke `hkask_forecast::*`
//! directly for the canonical math; this module adds no pass-through
//! wrappers around it.
//!
//! See `registry/templates/superforecasting/README.md` (Deterministic
//! Primitives contract) and `docs/explanation/forecasting-and-scenarios.md`
//! for the layered architecture.

use crate::scenarios::ScenarioResult;
use hkask_forecast::FermiQuestion;

// ── Fermi configuration ────────────────────────────────────────────────────

/// Apply user overrides to a set of Fermi sub-questions.
/// `overrides`: list of (index, estimate, confidence) tuples.
/// Only overrides for valid indices are applied; others are ignored.
pub fn apply_fermi_overrides(sub_questions: &mut [FermiQuestion], overrides: &[(usize, f64, f64)]) {
    for (idx, est, conf) in overrides {
        if *idx < sub_questions.len() {
            sub_questions[*idx].estimate = *est;
            sub_questions[*idx].confidence = *conf;
        }
    }
}

/// Server-level default Fermi estimates.
/// Overridable via environment variable HKASK_FERMI_DEFAULTS as JSON.
/// Each deployment can set its own seed/bootstrap estimates.
#[derive(Debug, Clone)]
pub struct FermiDefaults {
    pub growth_questions: Vec<FermiQuestion>,
    pub margin_questions: Vec<FermiQuestion>,
}

impl Default for FermiDefaults {
    fn default() -> Self {
        Self {
            growth_questions: fermi_decompose_growth(),
            margin_questions: fermi_decompose_margin(),
        }
    }
}

impl FermiDefaults {
    /// Load from HKASK_FERMI_DEFAULTS environment variable as JSON.
    /// Falls back to hardcoded defaults if unset or invalid.
    /// Expected format: {"growth": [{"estimate": 0.65, "confidence": 0.7}, ...], "margin": [...]}
    pub fn from_env() -> Self {
        if let Ok(json_str) = std::env::var("HKASK_FERMI_DEFAULTS")
            && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str)
        {
            let growth = parsed.get("growth").and_then(|g| g.as_array());
            let margin = parsed.get("margin").and_then(|m| m.as_array());
            if let (Some(g_arr), Some(m_arr)) = (growth, margin) {
                let parse_questions = |arr: &[serde_json::Value]| -> Vec<FermiQuestion> {
                    arr.iter()
                        .map(|v| FermiQuestion {
                            question: v
                                .get("question")
                                .and_then(|q| q.as_str())
                                .unwrap_or("")
                                .into(),
                            estimate: v.get("estimate").and_then(|e| e.as_f64()).unwrap_or(0.5),
                            confidence: v.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.5),
                        })
                        .collect()
                };
                return FermiDefaults {
                    growth_questions: parse_questions(g_arr),
                    margin_questions: parse_questions(m_arr),
                };
            }
        }
        Self::default()
    }
}

// ── Fermi decomposition ────────────────────────────────────────────────────

/// Decompose a revenue growth forecast into Fermi sub-questions.
pub fn fermi_decompose_growth() -> Vec<FermiQuestion> {
    vec![
        FermiQuestion {
            question: "Will TAM (total addressable market) grow? (0=shrink, 0.5=flat, 1=grow)"
                .into(),
            estimate: 0.65,
            confidence: 0.7,
        },
        FermiQuestion {
            question:
                "Will the company maintain or gain market share? (0=lose, 0.5=maintain, 1=gain)"
                    .into(),
            estimate: 0.55,
            confidence: 0.6,
        },
        FermiQuestion {
            question: "Will unit economics improve? (0=degrade, 0.5=flat, 1=improve)".into(),
            estimate: 0.55,
            confidence: 0.5,
        },
        FermiQuestion {
            question:
                "Will macro conditions support growth? (0=headwinds, 0.5=neutral, 1=tailwinds)"
                    .into(),
            estimate: 0.50,
            confidence: 0.4,
        },
    ]
}

/// Decompose a profit margin forecast into Fermi sub-questions.
pub fn fermi_decompose_margin() -> Vec<FermiQuestion> {
    vec![
        FermiQuestion {
            question: "Will input costs decrease? (0=increase, 0.5=flat, 1=decrease)".into(),
            estimate: 0.45,
            confidence: 0.5,
        },
        FermiQuestion {
            question: "Will pricing power increase? (0=erode, 0.5=flat, 1=strengthen)".into(),
            estimate: 0.55,
            confidence: 0.6,
        },
        FermiQuestion {
            question: "Will operating leverage improve? (0=decline, 0.5=flat, 1=improve)".into(),
            estimate: 0.55,
            confidence: 0.5,
        },
        FermiQuestion {
            question: "Will competitive intensity decrease? (0=intensify, 0.5=flat, 1=ease)".into(),
            estimate: 0.50,
            confidence: 0.4,
        },
    ]
}

// ── Scenario probability distribution ──────────────────────────────────────

/// Probability-weighted scenario.
#[derive(Debug, Clone)]
pub struct WeightedScenario {
    pub name: &'static str,
    pub intrinsic_per_share: f64,
    pub probability: f64,
}

/// Distribute probabilities across a 2×2 growth×margin scenario matrix.
///
/// Uses the growth and margin calibrated probabilities to assign
/// probabilities to each quadrant of the 2×2 matrix. Growth and margin
/// are treated as independent.
pub fn distribute_scenario_probabilities(
    growth_probability: f64, // P(high growth)
    margin_probability: f64, // P(high margin)
    scenario_results: &[ScenarioResult],
) -> Vec<WeightedScenario> {
    let p_bull = growth_probability * margin_probability;
    let p_land = growth_probability * (1.0 - margin_probability);
    let p_cow = (1.0 - growth_probability) * margin_probability;
    let p_bear = (1.0 - growth_probability) * (1.0 - margin_probability);

    let probs = [p_bull, p_land, p_cow, p_bear];

    scenario_results
        .iter()
        .enumerate()
        .map(|(i, r)| WeightedScenario {
            name: r.scenario.name,
            intrinsic_per_share: r.intrinsic_per_share,
            probability: probs[i],
        })
        .collect()
}

/// Compute expected intrinsic value from probability-weighted scenarios.
pub fn expected_intrinsic(weighted: &[WeightedScenario]) -> f64 {
    weighted
        .iter()
        .map(|w| w.intrinsic_per_share * w.probability)
        .sum()
}

/// Check if an actual value falls within a tolerance band of the forecast.
pub fn within_tolerance(forecast: f64, actual: f64, tolerance: f64) -> bool {
    if forecast == 0.0 {
        return actual.abs() < tolerance;
    }
    ((actual - forecast) / forecast).abs() <= tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_probabilities_sum_to_one() {
        let probs = [
            WeightedScenario {
                name: "Bull",
                intrinsic_per_share: 200.0,
                probability: 0.3,
            },
            WeightedScenario {
                name: "Land",
                intrinsic_per_share: 150.0,
                probability: 0.2,
            },
            WeightedScenario {
                name: "Cow",
                intrinsic_per_share: 120.0,
                probability: 0.3,
            },
            WeightedScenario {
                name: "Bear",
                intrinsic_per_share: 80.0,
                probability: 0.2,
            },
        ];
        let sum: f64 = probs.iter().map(|w| w.probability).sum();
        assert!((sum - 1.0).abs() < 0.01, "probabilities sum to 1.0");
    }

    #[test]
    fn expected_intrinsic_computation() {
        let probs = [
            WeightedScenario {
                name: "Bull",
                intrinsic_per_share: 200.0,
                probability: 0.25,
            },
            WeightedScenario {
                name: "Bear",
                intrinsic_per_share: 100.0,
                probability: 0.75,
            },
        ];
        let expected = expected_intrinsic(&probs);
        assert!((expected - 125.0).abs() < 0.01, "0.25*200 + 0.75*100 = 125");
    }

    #[test]
    fn tolerance_bands() {
        // Within 10% band → classified correctly
        assert!(within_tolerance(100.0, 105.0, 0.10));
        assert!(within_tolerance(100.0, 95.0, 0.10));
        // Outside 10% band
        assert!(!within_tolerance(100.0, 112.0, 0.10));
        assert!(!within_tolerance(100.0, 88.0, 0.10));
    }
}
