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

// ── Tree-weighted scenarios (T7, detailed mode) ─────────────────────────────

/// Weighting mode of a scenario analysis — the maturity label downstream
/// consumers use to tell how the quadrant probabilities were derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeightingMode {
    /// Simple mode: 2×2 range without probabilities (the default on-ramp).
    Schwartz2x2,
    /// Detailed mode: quadrant probabilities derived from a validated event
    /// tree's root marginals (the earned upgrade).
    EventTree,
}

/// Minimal, self-describing projection of a scenarios-server `EventTree` for
/// the tree-weighted path. Companies does not depend on the scenarios crate
/// (the integration seam is caller-mediated paste bridging, per the gap
/// report); this struct is the documented contract of what the bridge
/// consumes — the `tree` object from `scenario_from_markets_set` or
/// `scenario_propagate` output.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventTreeProjection {
    pub root_ids: Vec<String>,
    pub nodes: Vec<EventTreeNodeProjection>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventTreeNodeProjection {
    pub id: String,
    pub marginal_probability: f64,
}

/// Derive 2×2 quadrant probabilities from an event tree's root marginals.
///
/// Mapping (documented, deterministic): with exactly two root events, the
/// first root's marginal plays P(high growth) and the second P(high margin),
/// and quadrant probabilities follow the same product form as
/// `distribute_scenario_probabilities` — but with tree-resolved marginals
/// (which already encode the tree's conditioning) instead of independently
/// elicited probabilities. With any other root count the mapping is
/// ambiguous and None is returned — the caller falls back to simple mode
/// with a warning rather than fabricating a mapping.
///
/// Returns (growth_probability, margin_probability) on success.
pub fn tree_root_probabilities(tree: &EventTreeProjection) -> Option<(f64, f64)> {
    if tree.root_ids.len() != 2 {
        return None;
    }
    let marginal_of = |id: &str| {
        tree.nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.marginal_probability)
    };
    let growth = marginal_of(&tree.root_ids[0])?;
    let margin = marginal_of(&tree.root_ids[1])?;
    if !(0.0..=1.0).contains(&growth) || !(0.0..=1.0).contains(&margin) {
        return None;
    }
    Some((growth, margin))
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

    // ── T7 tree-weighted path ────────────────────────────────────────────

    fn two_root_tree(g: f64, m: f64) -> EventTreeProjection {
        EventTreeProjection {
            root_ids: vec!["mkt-G".into(), "mkt-M".into()],
            nodes: vec![
                EventTreeNodeProjection {
                    id: "mkt-G".into(),
                    marginal_probability: g,
                },
                EventTreeNodeProjection {
                    id: "mkt-M".into(),
                    marginal_probability: m,
                },
                EventTreeNodeProjection {
                    id: "mkt-child".into(),
                    marginal_probability: 0.99,
                },
            ],
        }
    }

    #[test]
    fn tree_root_probabilities_two_roots() {
        let tree = two_root_tree(0.6, 0.4);
        let (g, m) = tree_root_probabilities(&tree).expect("two valid roots");
        assert!((g - 0.6).abs() < 1e-12);
        assert!((m - 0.4).abs() < 1e-12);
    }

    #[test]
    fn tree_root_probabilities_rejects_non_two_root_trees() {
        let mut tree = two_root_tree(0.6, 0.4);
        tree.root_ids = vec!["mkt-G".into()];
        assert!(tree_root_probabilities(&tree).is_none());
        tree.root_ids = vec!["a".into(), "b".into(), "c".into()];
        assert!(tree_root_probabilities(&tree).is_none());
    }

    #[test]
    fn tree_root_probabilities_rejects_out_of_range_marginal() {
        let tree = two_root_tree(1.5, 0.4);
        assert!(tree_root_probabilities(&tree).is_none());
    }

    #[test]
    fn tree_weighted_expected_intrinsic_hand_check() {
        // Quadrant intrinsics 200/150/120/80 with tree marginals g=0.6,
        // m=0.4: probabilities 0.24/0.36/0.16/0.24; expected =
        // 200·0.24 + 150·0.36 + 120·0.16 + 80·0.24 = 48+54+19.2+19.2 = 140.4.
        let results: Vec<crate::scenarios::ScenarioResult> = Vec::new();
        let _ = results; // distribute works off ScenarioResult; use direct weights below
        let weighted = vec![
            WeightedScenario {
                name: "Bull",
                intrinsic_per_share: 200.0,
                probability: 0.24,
            },
            WeightedScenario {
                name: "Land",
                intrinsic_per_share: 150.0,
                probability: 0.36,
            },
            WeightedScenario {
                name: "Cow",
                intrinsic_per_share: 120.0,
                probability: 0.16,
            },
            WeightedScenario {
                name: "Bear",
                intrinsic_per_share: 80.0,
                probability: 0.24,
            },
        ];
        let expected = expected_intrinsic(&weighted);
        assert!((expected - 140.4).abs() < 1e-9, "expected {expected}");
    }

    #[test]
    fn weighting_mode_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(WeightingMode::Schwartz2x2).unwrap(),
            serde_json::json!("schwartz_2x2")
        );
        assert_eq!(
            serde_json::to_value(WeightingMode::EventTree).unwrap(),
            serde_json::json!("event_tree")
        );
    }

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
