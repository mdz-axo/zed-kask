//! R6 — Falsification suite for H1–H5 (all on CMP-controlled inputs).
//!
//! Each hypothesis has a falsifier — a test that could refute it. The suite
//! runs the computable tests and records the results. Tests that need live
//! data (H1 company risk models, H5 human-in-the-loop) are recorded as
//! "blocked" — the falsification log is honest about what was tested and what
//! wasn't.
//!
//! Status vocabulary (falsifiability discipline): **corroborated** (withstood
//! a test that could have falsified it) / **refuted** / **open** /
//! **survived_by_default** (no test available) / **blocked** (test exists but
//! needs infrastructure not yet available).

use crate::{CoherenceMeasure, DurationGap, contract_price_coherence, duration_vs_cmp_tenors};

// ── Falsification log entry ────────────────────────────────────────────────

/// The status of a hypothesis after falsification testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HypothesisStatus {
    /// Withstood a test that could have falsified it.
    Corroborated,
    /// The test refuted the hypothesis.
    Refuted,
    /// No test has been run yet.
    Open,
    /// No test is available — the hypothesis survives by default.
    SurvivedByDefault,
    /// A test exists but needs infrastructure not yet available.
    Blocked,
}

impl std::fmt::Display for HypothesisStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corroborated => write!(f, "corroborated"),
            Self::Refuted => write!(f, "refuted"),
            Self::Open => write!(f, "open"),
            Self::SurvivedByDefault => write!(f, "survived_by_default"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

/// One entry in the falsification log.
#[derive(Debug, Clone)]
pub struct FalsificationEntry {
    /// The hypothesis identifier (H1–H5).
    pub hypothesis: &'static str,
    /// The hypothesis statement (short form).
    pub statement: &'static str,
    /// The test that was run (or would be run).
    pub test: &'static str,
    /// The result status.
    pub status: HypothesisStatus,
    /// The falsifier — what outcome would refute the hypothesis.
    pub falsifier: &'static str,
    /// The evidence (test result summary, or why it's blocked).
    pub evidence: String,
}

// ── H2: Duration falsification ─────────────────────────────────────────────

/// The result of the H2 duration falsification test.
#[derive(Debug, Clone)]
pub struct H2DurationResult {
    /// The equity duration (years) tested.
    pub equity_duration_years: f64,
    /// The duration gaps vs CMP tenors (1m/3m/6m).
    pub gaps: Vec<DurationGap>,
    /// Whether equity duration clusters near contract horizons (<1yr).
    /// If true, H2a is refuted (duration mismatch is not real).
    pub clusters_near_contract_horizons: bool,
    /// The minimum ratio across all CMP tenors (duration / tenor).
    /// A ratio near 1.0 means the equity duration is close to a CMP tenor.
    pub min_ratio: f64,
    /// The label of the CMP tenor with the minimum ratio (the nearest tenor).
    pub min_ratio_tenor: &'static str,
}

/// Run the H2 duration falsification test (T1: implied equity duration vs CMP tenors).
///
/// H2a: equity cash flows are back-loaded (terminal value dominates), so
/// effective duration ≫ contract durations — maturity transformation is real.
/// **Falsifier**: computed equity durations cluster near typical contract
/// horizons (<1yr) for most firms.
///
/// Returns the duration gaps and the falsification verdict.
pub fn h2_duration_test(equity_duration_years: f64) -> Option<H2DurationResult> {
    let gaps = duration_vs_cmp_tenors(equity_duration_years)?;
    // Find the gap with the minimum ratio (the nearest CMP tenor).
    let min_gap = gaps.iter().min_by(|a, b| {
        a.ratio
            .partial_cmp(&b.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let min_ratio = min_gap.map(|g| g.ratio).unwrap_or(f64::INFINITY);
    let min_ratio_tenor = min_gap.map(|g| g.tenor_label).unwrap_or("?");
    // H2a falsifier: if the minimum ratio (duration / nearest CMP tenor) is
    // near 1.0, the equity duration is close to a contract horizon. "Near"
    // means < 2.0 — the equity duration is less than 2× the longest CMP tenor.
    let clusters_near_contract_horizons = min_ratio < 2.0;
    Some(H2DurationResult {
        equity_duration_years,
        gaps,
        clusters_near_contract_horizons,
        min_ratio,
        min_ratio_tenor,
    })
}

// ── H3: Coherence falsification ────────────────────────────────────────────

/// The result of the H3 coherence falsification test.
#[derive(Debug, Clone)]
pub struct H3CoherenceResult {
    /// The coherence measures for each valid (tree_implied, market_price) pair.
    pub measures: Vec<CoherenceMeasure>,
    /// The number of coherent pairs (divergence within cost band).
    pub coherent_count: usize,
    /// The total number of valid pairs tested (excluding dropped invalid pairs).
    pub total_count: usize,
    /// The number of pairs dropped due to invalid probabilities (outside [0,1]).
    /// Surfaced — never silently discarded (.rules: errors propagate). A non-zero
    /// count indicates a data quality issue in the input pairs.
    pub dropped_count: usize,
    /// The coherence rate (coherent_count / total_count).
    pub coherence_rate: f64,
    /// Whether H3 is refuted: coherence rate < 0.5 (the tree diverges from
    /// the market more often than not).
    pub refuted: bool,
}

/// Run the H3 contract-price coherence falsification test (T1: tree-implied
/// joint vs market joint price, cost-banded).
///
/// H3: tree-implied joint probabilities from CMP-controlled composition are
/// coherent with observed contract prices within transaction costs.
/// **Falsifier**: systematic divergence beyond the cost band (coherence rate
/// < 0.5 across the tested pairs).
///
/// `pairs` is a slice of (tree_implied, market_price) tuples. `cost_band` is
/// the transaction-cost band (passed variable).
pub fn h3_coherence_test(pairs: &[(f64, f64)], cost_band: f64) -> Option<H3CoherenceResult> {
    if pairs.is_empty() {
        return None;
    }
    let mut measures: Vec<CoherenceMeasure> = Vec::with_capacity(pairs.len());
    let mut dropped_count: usize = 0;
    for &(tree, market) in pairs {
        match contract_price_coherence(tree, market, cost_band) {
            Some(m) => measures.push(m),
            None => {
                dropped_count += 1;
                tracing::warn!(
                    target: "hkask.forecast.falsification",
                    tree_implied = tree,
                    market_price = market,
                    "H3 coherence pair dropped — probability outside [0,1] (data quality issue)"
                );
            }
        }
    }
    if measures.is_empty() {
        return None;
    }
    let coherent_count = measures.iter().filter(|c| c.coherent).count();
    let total_count = measures.len();
    let coherence_rate = coherent_count as f64 / total_count as f64;
    let refuted = coherence_rate < 0.5;
    if refuted {
        tracing::warn!(
            target: "hkask.forecast.falsification",
            coherent_count,
            total_count,
            coherence_rate,
            dropped_count,
            "H3 refuted: coherence rate < 50% — tree diverges from market beyond transaction costs"
        );
    }
    Some(H3CoherenceResult {
        measures,
        coherent_count,
        total_count,
        dropped_count,
        coherence_rate,
        refuted,
    })
}

// ── Full falsification log ─────────────────────────────────────────────────

/// Build the full falsification log for H1–H5.
///
/// H2 and H3 are computable today (the functions are in this crate). H1, H4,
/// and H5 are recorded with their statuses — blocked (needs live data) or
/// open (design-choice assessment pending).
///
/// `h2_duration` and `h3_coherence` are optional — pass `None` to record the
/// hypothesis as open/blocked.
pub fn falsification_log(
    h2_duration: Option<&H2DurationResult>,
    h3_coherence: Option<&H3CoherenceResult>,
) -> Vec<FalsificationEntry> {
    let mut log = Vec::new();

    // H1 — systemic risk capture.
    log.push(FalsificationEntry {
        hypothesis: "H1",
        statement: "Scenario-tree-augmented risk models explain more out-of-sample downside variance than factor-only baselines",
        test: "T1: tier-controlled out-of-sample downside Brier, augmented vs baseline",
        status: HypothesisStatus::Blocked,
        falsifier: "augmented model adds no predictive power (ΔR² < 0.01) even where contracts are liquid and thematically tight",
        evidence: "Blocked: needs live company data + CMP-controlled scenario trees running through the full MCP stack. The CMP foundation (Phase 0) and composition machinery (R1) are landed; the empirical test requires the companies MCP server + a Brier scoring loop over resolved events.".into(),
    });

    // H2 — duration.
    let (h2_status, h2_evidence) = match h2_duration {
        Some(result) => {
            let status = if result.clusters_near_contract_horizons {
                HypothesisStatus::Refuted
            } else {
                HypothesisStatus::Corroborated
            };
            let evidence = format!(
                "Equity duration {:.1}y vs CMP tenors: min ratio {:.1}× ({}), gaps {:?}. {}. Falsifier (duration < 2× longest CMP tenor): {}.",
                result.equity_duration_years,
                result.min_ratio,
                result.min_ratio_tenor,
                result.gaps.iter().map(|g| g.gap_years).collect::<Vec<_>>(),
                if result.clusters_near_contract_horizons {
                    "Duration clusters near contract horizons — H2a refuted (maturity transformation is not real)"
                } else {
                    "Duration far exceeds contract horizons — H2a corroborated (maturity transformation is real)"
                },
                if result.clusters_near_contract_horizons {
                    "TRIGGERED"
                } else {
                    "not triggered"
                },
            );
            (status, evidence)
        }
        None => (
            HypothesisStatus::Open,
            "No duration test run yet. Use h2_duration_test(equity_duration_years) to compute."
                .into(),
        ),
    };
    log.push(FalsificationEntry {
        hypothesis: "H2",
        statement: "Duration-matched contract selection produces more stable implied risk premia across horizons than deadline-nearest matching",
        test: "T1: implied equity duration distribution vs fixed CMP tenors (1m/3m/6m)",
        status: h2_status,
        falsifier: "computed equity durations cluster near typical contract horizons (<1yr) for most firms",
        evidence: h2_evidence,
    });

    // H3 — contract-price coherence.
    let (h3_status, h3_evidence) = match h3_coherence {
        Some(result) => {
            let status = if result.refuted {
                HypothesisStatus::Refuted
            } else {
                HypothesisStatus::Corroborated
            };
            let evidence = format!(
                "Coherence rate: {}/{} ({:.1}%). {}. Dropped: {} invalid pairs. Falsifier (coherence rate < 50%): {}.",
                result.coherent_count,
                result.total_count,
                result.coherence_rate * 100.0,
                if result.refuted {
                    "Systematic divergence — H3 refuted (composition adds no pricing coherence)"
                } else {
                    "Tree is coherent with market within costs — H3 corroborated"
                },
                result.dropped_count,
                if result.refuted {
                    "TRIGGERED"
                } else {
                    "not triggered"
                },
            );
            (status, evidence)
        }
        None => (
            HypothesisStatus::Open,
            "No coherence test run yet. Use h3_coherence_test(pairs, cost_band) to compute.".into(),
        ),
    };
    log.push(FalsificationEntry {
        hypothesis: "H3",
        statement: "Tree-implied joint probabilities from CMP-controlled composition are coherent with observed contract prices within transaction costs",
        test: "T1: tree-implied joint vs market joint price, cost-banded",
        status: h3_status,
        falsifier: "systematic divergence beyond the cost band (coherence rate < 50%)",
        evidence: h3_evidence,
    });

    // H4 — complexity allocation.
    log.push(FalsificationEntry {
        hypothesis: "H4",
        statement: "Simple time/return math + complex risk math is the right complexity allocation",
        test: "Error-concentration instrumentation on the minimal model vs the constrained allocation",
        status: HypothesisStatus::Open,
        falsifier: "forecast errors concentrate in risk-relevant dimensions (downside events) under the constrained allocation, or in time/return dimensions under the rich-time alternative",
        evidence: "Open: the error-concentration instrumentation requires a running forecast loop with resolved outcomes. The CMP foundation (Phase 0) supplies the controlled inputs; the test needs a historical backtest over resolved CMP indices.".into(),
    });

    // H5 — LLM leverage.
    log.push(FalsificationEntry {
        hypothesis: "H5",
        statement: "LLM-mediated tree construction achieves equal calibration (Brier) at lower analyst-hours per tree than the manual baseline",
        test: "Paired construction cost/calibration study (N companies, same information set, LLM vs manual)",
        status: HypothesisStatus::Blocked,
        falsifier: "LLM trees ≥30% more events but Brier worse by >0.05 → refuted in strong form",
        evidence: "Blocked: needs a human-in-the-loop paired study. The LLM-mediated construction path (compose_cmp_tree) is landed; the study requires analyst-hours tracking and a manual baseline.".into(),
    });

    log
}
