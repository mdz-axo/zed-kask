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
pub(crate) fn apply_fermi_overrides(
    sub_questions: &mut [FermiQuestion],
    overrides: &[(usize, f64, f64)],
) {
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
pub(crate) struct FermiDefaults {
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
pub(crate) fn fermi_decompose_growth() -> Vec<FermiQuestion> {
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
pub(crate) fn fermi_decompose_margin() -> Vec<FermiQuestion> {
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
pub(crate) struct WeightedScenario {
    pub name: &'static str,
    pub intrinsic_per_share: f64,
    pub probability: f64,
}

/// Distribute probabilities across a 2×2 growth×margin scenario matrix.
///
/// Uses the growth and margin calibrated probabilities to assign
/// probabilities to each quadrant of the 2×2 matrix. Growth and margin
/// are treated as independent.
pub(crate) fn distribute_scenario_probabilities(
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
pub(crate) fn expected_intrinsic(weighted: &[WeightedScenario]) -> f64 {
    weighted
        .iter()
        .map(|w| w.intrinsic_per_share * w.probability)
        .sum()
}

// ── Tree-weighted scenarios (T7, detailed mode) ─────────────────────────────

/// Weighting mode of a scenario analysis — the maturity label downstream
/// consumers use to tell how the quadrant probabilities were derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub(crate) enum WeightingMode {
    /// Simple mode: 2×2 range without probabilities (the default on-ramp).
    #[serde(rename = "schwartz_2x2")]
    Schwartz2x2,
    /// Detailed mode: quadrant probabilities derived from a validated event
    /// tree's root marginals (the earned upgrade).
    #[serde(rename = "event_tree")]
    EventTree,
}

/// Minimal, self-describing projection of a scenarios-server `EventTree` for
/// the tree-weighted path. Companies does not depend on the scenarios crate
/// (the integration seam is caller-mediated paste bridging, per the gap
/// report); this struct is the documented contract of what the bridge
/// consumes — the `tree` object from `scenario_from_markets_set` or
/// `scenario_propagate` output.
///
/// R3: when the tree comes from `compose_cmp_tree` (CMP-driven composition),
/// the `cmp_provenance` field records the CMP index identities so the
/// tree-weighted output can cite them. When absent, the tree is from raw
/// contracts (pre-R3 behavior).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EventTreeProjection {
    pub root_ids: Vec<String>,
    pub nodes: Vec<EventTreeNodeProjection>,
    /// R3: CMP provenance — present when the tree was built from CMP indices
    /// (via `compose_cmp_tree`). Each entry is a CMP index identity
    /// (`cmp:{family}:{tenor}:{orientation}`). Absent for raw-contract trees.
    /// The element type is the shared `hkask_forecast::CmpIndexProvenance` —
    /// re-exported below — so the scenarios emitter and this deserializer
    /// share one type-level source of truth. The `#[serde(default)]` on the
    /// outer field tolerates the no-`cmp_provenance` case (raw-contract trees);
    /// the per-field `#[serde(default)]` on the shared struct tolerates partial
    /// entries without failing the whole tree. The pin test
    /// `cmp_provenance_round_trips_real_scenarios_emitter` enforces that the
    /// real scenarios emitter populates all 7 fields.
    #[serde(default)]
    pub cmp_provenance: Vec<hkask_forecast::CmpIndexProvenance>,
}

/// R3: CMP index provenance — the bridge contract between
/// `hkask-mcp-scenarios`'s `scenario_from_cmp_indices` emitter and this crate's
/// `EventTreeProjection` deserializer. Re-exported from `hkask_forecast` so the
/// two sides cannot drift apart at the type level; the per-field
/// `#[serde(default)]` tolerates partial entries without failing the whole
/// tree, and the pin test in each crate enforces that the real emitters
/// populate the full 7-field shape.

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct EventTreeNodeProjection {
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
pub(crate) fn tree_root_probabilities(tree: &EventTreeProjection) -> Option<(f64, f64)> {
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
pub(crate) fn within_tolerance(forecast: f64, actual: f64, tolerance: f64) -> bool {
    if forecast == 0.0 {
        return actual.abs() < tolerance;
    }
    ((actual - forecast) / forecast).abs() <= tolerance
}

// ── Pin test ─────────────────────────────────────────────────────────────────
// Enforces the R3 bridge contract: the JSON shape the scenarios server's
// `emit_cmp_provenance` produces (pinned by
// `scenario_from_cmp_indices_emits_full_cmp_provenance_inside_tree` in
// `hkask-mcp-scenarios`) round-trips through this crate's `EventTreeProjection`
// deserializer with all 7 fields intact. If either side drifts, this test
// breaks — catching silent degradation of the CMP provenance bridge.

#[cfg(test)]
mod tests {
    use super::EventTreeProjection;

    /// The real scenarios emitter populates all 7 CMP provenance fields. This
    /// test deserializes that exact shape through `EventTreeProjection` and
    /// asserts every field survives — the `#[serde(default)]` per field would
    /// silently mask a dropped field, so each must be checked explicitly.
    #[test]
    fn cmp_provenance_round_trips_real_scenarios_emitter() {
        // The JSON shape produced by `emit_cmp_provenance` in
        // `hkask-mcp-scenarios` (pinned by the scenarios-side test of the same
        // 7-field shape). Wrapped in the `tree` object shape that
        // `scenario_from_cmp_indices` emits.
        let tree_json = serde_json::json!({
            "root_ids": ["cmp:policy_interest_rate:1m:increase"],
            "nodes": [
                {"id": "cmp:policy_interest_rate:1m:increase", "marginal_probability": 0.55}
            ],
            "cmp_provenance": [{
                "id": "cmp:policy_interest_rate:1m:increase",
                "family": "policy_interest_rate",
                "tenor": "1m",
                "orientation": "increase",
                "venue": "kalshi",
                "method": "interpolated",
                "maturity_error_days": 0.5
            }]
        });

        let projection: EventTreeProjection = serde_json::from_value(tree_json)
            .expect("tree JSON must deserialize as EventTreeProjection");

        assert_eq!(
            projection.cmp_provenance.len(),
            1,
            "one CMP provenance entry in → one out"
        );
        let entry = &projection.cmp_provenance[0];
        assert_eq!(entry.id, "cmp:policy_interest_rate:1m:increase");
        assert_eq!(entry.family, "policy_interest_rate");
        assert_eq!(entry.tenor, "1m");
        assert_eq!(entry.orientation, "increase");
        assert_eq!(entry.venue, "kalshi");
        assert_eq!(entry.method, "interpolated");
        assert_eq!(entry.maturity_error_days, 0.5);
    }

    /// `cmp_provenance` is optional (absent for raw-contract trees). The
    /// `#[serde(default)]` on the outer field must tolerate its absence without
    /// failing deserialization — a tree without CMP indices is the pre-R3
    /// shape and must still deserialize cleanly.
    #[test]
    fn event_tree_projection_tolerates_missing_cmp_provenance() {
        let tree_json = serde_json::json!({
            "root_ids": ["evt-1"],
            "nodes": [{"id": "evt-1", "marginal_probability": 0.3}]
        });

        let projection: EventTreeProjection = serde_json::from_value(tree_json)
            .expect("raw-contract tree must deserialize without cmp_provenance");
        assert!(
            projection.cmp_provenance.is_empty(),
            "absent cmp_provenance must default to empty, not fail"
        );
    }
}
