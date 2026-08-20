//! Schwartz 2x2 scenario planning for company valuation.
//!
//! Two axes (default: revenue growth x gross margin) produce four scenarios.
//! Each scenario runs through the 11-line-item financial model to produce
//! a range of intrinsic values.
//!
//! Reference: Schwartz, "The Art of the Long View"

use crate::fibo::{GROSS_PROFIT_MARGIN, REVENUE_GROWTH_RATE};
use crate::financial_model::{self, HistoricalSnapshot, ProjectedModel, ProjectionAssumptions};

// ── 2x2 Matrix ────────────────────────────────────────────────────────────

/// A scenario quadrant in the 2x2 matrix.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: &'static str,
    pub description: &'static str,
    /// Multiplier applied to the primary axis (e.g., revenue growth).
    pub axis1_multiplier: f64,
    /// Multiplier applied to the secondary axis (e.g., gross margin).
    pub axis2_multiplier: f64,
}

/// Scenario axis definition.
#[derive(Debug, Clone)]
pub struct ScenarioAxis {
    /// Human-readable name.
    pub name: &'static str,
    /// FIBO concept (for ontology anchoring).
    pub fibo_concept: &'static str,
    /// Baseline value pulled from company fundamentals.
    pub baseline: f64,
    /// "High" end of the range (multiplier applied to baseline).
    pub high_multiplier: f64,
    /// "Low" end of the range.
    pub low_multiplier: f64,
}

/// Schwartz 2x2 scenario matrix.
#[derive(Debug, Clone)]
pub struct ScenarioMatrix {
    pub axis1: ScenarioAxis,
    pub axis2: ScenarioAxis,
    pub scenarios: [Scenario; 4],
}

impl ScenarioMatrix {
    /// Build a 2x2 matrix around revenue growth (axis1) and gross margin (axis2).
    pub fn growth_x_margin(hist_revenue_growth: f64, gross_margin: f64) -> Self {
        let axis1 = ScenarioAxis {
            name: "Revenue Growth",
            fibo_concept: REVENUE_GROWTH_RATE,
            baseline: hist_revenue_growth,
            high_multiplier: 1.5,
            low_multiplier: 0.5,
        };
        let axis2 = ScenarioAxis {
            name: "Gross Margin",
            fibo_concept: GROSS_PROFIT_MARGIN,
            baseline: gross_margin,
            high_multiplier: 1.2,
            low_multiplier: 0.8,
        };

        let scenarios = [
            Scenario {
                name: "Bull Case",
                description: "Strong revenue growth with expanding margins. The company executes well in a favorable environment.",
                axis1_multiplier: axis1.high_multiplier,
                axis2_multiplier: axis2.high_multiplier,
            },
            Scenario {
                name: "Land Grab",
                description: "Revenue grows fast but margins compress. The company invests aggressively for market share at the expense of profitability.",
                axis1_multiplier: axis1.high_multiplier,
                axis2_multiplier: axis2.low_multiplier,
            },
            Scenario {
                name: "Cash Cow",
                description: "Slow revenue growth with strong margins. The company harvests its position, generating steady cash flow.",
                axis1_multiplier: axis1.low_multiplier,
                axis2_multiplier: axis2.high_multiplier,
            },
            Scenario {
                name: "Bear Case",
                description: "Weak revenue and compressed margins. Competitive pressure or macro headwinds erode both top and bottom line.",
                axis1_multiplier: axis1.low_multiplier,
                axis2_multiplier: axis2.low_multiplier,
            },
        ];

        ScenarioMatrix {
            axis1,
            axis2,
            scenarios,
        }
    }
}

// ── Scenario projection ────────────────────────────────────────────────────

/// A projected model under a specific scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub scenario: Scenario,
    pub model: ProjectedModel,
    pub applied_growth: f64,
    pub applied_margin: f64,
    pub intrinsic_per_share: f64,
}

/// Run the 11-line-item financial model under all four scenarios.
pub fn run_scenario_analysis(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    matrix: &ScenarioMatrix,
) -> Vec<ScenarioResult> {
    let mut results = Vec::with_capacity(4);

    for scenario in &matrix.scenarios {
        let applied_growth = matrix.axis1.baseline * scenario.axis1_multiplier;
        let applied_margin = (matrix.axis2.baseline * scenario.axis2_multiplier).clamp(0.01, 0.80);

        let mut assumptions = base_assumptions.clone();
        assumptions.revenue_growth = applied_growth;
        assumptions.gross_margin = applied_margin;

        let model = financial_model::project_model(hist, &assumptions, 0.0);

        results.push(ScenarioResult {
            scenario: scenario.clone(),
            intrinsic_per_share: model.intrinsic_per_share,
            model,
            applied_growth,
            applied_margin,
        });
    }

    results
}

/// Summarize scenario results with range and dispersion.
pub fn scenario_summary(results: &[ScenarioResult]) -> ScenarioSummary {
    let intrinsics: Vec<f64> = results.iter().map(|r| r.intrinsic_per_share).collect();
    let min_val = intrinsics.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_val = intrinsics.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let avg_val = intrinsics.iter().sum::<f64>() / intrinsics.len() as f64;

    let upside = if avg_val > 0.0 {
        (max_val - avg_val) / avg_val
    } else {
        0.0
    };
    let downside = if avg_val > 0.0 {
        (avg_val - min_val) / avg_val
    } else {
        0.0
    };
    let range_spread = if avg_val > 0.0 {
        (max_val - min_val) / avg_val
    } else {
        0.0
    };

    ScenarioSummary {
        intrinsic_range: (min_val, max_val),
        intrinsic_average: avg_val,
        upside_pct: upside,
        downside_pct: downside,
        range_spread_pct: range_spread,
    }
}

#[derive(Debug, Clone)]
pub struct ScenarioSummary {
    pub intrinsic_range: (f64, f64),
    pub intrinsic_average: f64,
    pub upside_pct: f64,
    pub downside_pct: f64,
    pub range_spread_pct: f64,
}
