//! Schwartz 2x2 scenario planning for company valuation.
//!
//! Two axes (default: revenue growth x gross margin) produce four scenarios.
//! Each scenario runs through the 11-line-item financial model to produce
//! a range of intrinsic values.
//!
//! Reference: Schwartz, "The Art of the Long View"

use crate::financial_model::{self, HistoricalSnapshot, ProjectionAssumptions};

// ── 2x2 Matrix ────────────────────────────────────────────────────────────

/// A scenario quadrant in the 2x2 matrix.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: &'static str,
    /// Multiplier applied to the primary axis (e.g., revenue growth).
    pub axis1_multiplier: f64,
    /// Multiplier applied to the secondary axis (e.g., gross margin).
    pub axis2_multiplier: f64,
}

/// Scenario axis definition.
#[derive(Debug, Clone)]
pub(crate) struct ScenarioAxis {
    /// Baseline value pulled from company fundamentals.
    pub baseline: f64,
    /// "High" end of the range (multiplier applied to baseline).
    pub high_multiplier: f64,
    /// "Low" end of the range.
    pub low_multiplier: f64,
}

/// Schwartz 2x2 scenario matrix.
#[derive(Debug, Clone)]
pub(crate) struct ScenarioMatrix {
    pub axis1: ScenarioAxis,
    pub axis2: ScenarioAxis,
    pub scenarios: [Scenario; 4],
}

impl ScenarioMatrix {
    /// Build a 2x2 matrix around revenue growth (axis1) and gross margin (axis2).
    pub fn growth_x_margin(hist_revenue_growth: f64, gross_margin: f64) -> Self {
        let axis1 = ScenarioAxis {
            baseline: hist_revenue_growth,
            high_multiplier: 1.5,
            low_multiplier: 0.5,
        };
        let axis2 = ScenarioAxis {
            baseline: gross_margin,
            high_multiplier: 1.2,
            low_multiplier: 0.8,
        };

        let scenarios = [
            Scenario {
                name: "Bull Case",
                axis1_multiplier: axis1.high_multiplier,
                axis2_multiplier: axis2.high_multiplier,
            },
            Scenario {
                name: "Land Grab",
                axis1_multiplier: axis1.high_multiplier,
                axis2_multiplier: axis2.low_multiplier,
            },
            Scenario {
                name: "Cash Cow",
                axis1_multiplier: axis1.low_multiplier,
                axis2_multiplier: axis2.high_multiplier,
            },
            Scenario {
                name: "Bear Case",
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
pub(crate) struct ScenarioResult {
    pub scenario: Scenario,
    pub intrinsic_per_share: f64,
}

/// Run the 11-line-item financial model under all four scenarios.
pub(crate) fn run_scenario_analysis(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    matrix: &ScenarioMatrix,
) -> Result<Vec<ScenarioResult>, financial_model::ProjectionError> {
    let mut results = Vec::with_capacity(4);

    for scenario in &matrix.scenarios {
        let applied_growth = matrix.axis1.baseline * scenario.axis1_multiplier;
        let applied_margin = (matrix.axis2.baseline * scenario.axis2_multiplier).clamp(0.01, 0.80);

        let mut assumptions = base_assumptions.clone();
        assumptions.revenue_growth = applied_growth;
        assumptions.gross_margin = applied_margin;

        let model = financial_model::project_financial_model(hist, &assumptions)?;

        results.push(ScenarioResult {
            scenario: scenario.clone(),
            intrinsic_per_share: model.intrinsic_per_share,
        });
    }

    Ok(results)
}
