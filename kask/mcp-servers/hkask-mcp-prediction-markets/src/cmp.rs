//! Constant Maturity Prediction (CMP) construction (T14).
//!
//! Analogous to Constant Maturity Treasury yields: prediction markets have
//! constantly-shifting deadlines, so raw prices are never comparable across
//! time. CMP synthesizes constant-tenor probability series from a base-event
//! family's markets, interpolating in log-odds space (hkask-forecast).
//!
//! Base events come only from config (`HKASK_PREDICTION_MARKETS_BASE_EVENTS`)
//! — a market can never auto-promote to benchmark status (a manipulated
//! market must not become the frame other events are priced against).

use hkask_forecast::{from_log_odds, log_odds};

/// A base event declared in config: `domain:series` pairs.
/// Format: "economics:KXFEDDECISION,politics:KXPREZ-28" (comma-separated).
pub fn parse_base_events(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .filter_map(|pair| {
            let (domain, series) = pair.split_once(':')?;
            let domain = domain.trim();
            let series = series.trim();
            if domain.is_empty() || series.is_empty() {
                return None;
            }
            Some((domain.to_string(), series.to_string()))
        })
        .collect()
}

/// One market's contribution: days-to-resolution at observation + price.
#[derive(Debug, Clone, Copy)]
pub struct TenorPoint {
    pub days_to_resolution: f64,
    pub price: f64,
}

/// Interpolation method actually used — sparse coverage degrades honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpMethod {
    /// ≥2 distinct tenor cohorts: log-odds interpolation.
    Interpolated,
    /// 1 cohort: nearest-tenor value with widened uncertainty.
    BucketedSparse,
}

/// A synthesized constant-maturity probability.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmpValue {
    pub tenor_days: u32,
    pub probability: f64,
    pub method: CmpMethod,
    /// Number of distinct tenor cohorts that informed the value.
    pub cohorts: usize,
    /// Interpolation bracket width in days (0 for exact cohort match);
    /// wider brackets ⇒ less certain. Sparse buckets report the distance to
    /// the nearest cohort.
    pub bracket_days: f64,
}

/// Synthesize the CMP probability at `tenor_days` from observed tenor points.
/// Interpolates linearly in log-odds between bracketing cohorts; extrapolates
/// flat beyond the observed range (nearest endpoint) — never invents a slope.
/// Empty input ⇒ None (no CMP without data).
pub fn constant_maturity(points: &[TenorPoint], tenor_days: u32) -> Option<CmpValue> {
    if points.is_empty() {
        return None;
    }
    let tenor = f64::from(tenor_days);
    let mut sorted: Vec<TenorPoint> = points.to_vec();
    sorted.sort_by(|a, b| {
        a.days_to_resolution
            .partial_cmp(&b.days_to_resolution)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    // Cohorts: distinct tenors (same-deadline markets share a cohort; their
    // mean log-odds is the cohort value). (sum, count) accumulation — a
    // running mean-of-2 would bias toward later points for cohorts >2.
    let mut cohorts: Vec<(f64, f64, usize)> = Vec::new(); // (tenor, log-odds sum, count)
    for point in &sorted {
        if let Some(last) = cohorts.last_mut()
            && (last.0 - point.days_to_resolution).abs() < 1.0
        {
            last.1 += log_odds(point.price);
            last.2 += 1;
            continue;
        }
        cohorts.push((point.days_to_resolution, log_odds(point.price), 1));
    }
    let cohorts: Vec<(f64, f64)> = cohorts
        .into_iter()
        .map(|(tenor, sum, count)| (tenor, sum / count as f64))
        .collect();

    if cohorts.len() == 1 {
        let (cohort_tenor, value) = cohorts[0];
        return Some(CmpValue {
            tenor_days,
            probability: from_log_odds(value),
            method: CmpMethod::BucketedSparse,
            cohorts: 1,
            bracket_days: (cohort_tenor - tenor).abs(),
        });
    }

    // Bracket the tenor.
    let first = cohorts[0];
    let last = cohorts[cohorts.len() - 1];
    if tenor <= first.0 {
        return Some(CmpValue {
            tenor_days,
            probability: from_log_odds(first.1),
            method: CmpMethod::Interpolated,
            cohorts: cohorts.len(),
            bracket_days: first.0 - tenor,
        });
    }
    if tenor >= last.0 {
        return Some(CmpValue {
            tenor_days,
            probability: from_log_odds(last.1),
            method: CmpMethod::Interpolated,
            cohorts: cohorts.len(),
            bracket_days: tenor - last.0,
        });
    }
    let idx = cohorts
        .windows(2)
        .position(|w| tenor >= w[0].0 && tenor <= w[1].0)?;
    let (t0, v0) = cohorts[idx];
    let (t1, v1) = cohorts[idx + 1];
    let fraction = if (t1 - t0).abs() < 1e-9 {
        0.5
    } else {
        (tenor - t0) / (t1 - t0)
    };
    let interpolated = v0 + fraction * (v1 - v0);
    Some(CmpValue {
        tenor_days,
        probability: from_log_odds(interpolated),
        method: CmpMethod::Interpolated,
        cohorts: cohorts.len(),
        bracket_days: t1 - t0,
    })
}

// ── CMP Index: the published curve (not just a point query) ───────────────

/// The standard tenor grid, in days — the CMT-analogous publication points.
/// Short end matters most for forecasting (horizon effects per 2602.19520);
/// the long end captures structural expectations.
pub const INDEX_TENORS_DAYS: [u32; 6] = [7, 30, 90, 180, 365, 730];

/// One point on the published index curve.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmpIndexPoint {
    pub tenor_days: u32,
    /// None when the cohort coverage cannot support this tenor (empty input,
    /// or the tenor lies beyond all observed cohorts and flat extrapolation
    /// would misrepresent the curve's reach).
    pub probability: Option<f64>,
    pub method: CmpMethod,
    pub cohorts: usize,
    pub bracket_days: f64,
}

/// A computed index curve for one base event at one observation time.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmpIndex {
    pub series: String,
    pub computed_at: String,
    pub points: Vec<CmpIndexPoint>,
}

/// Compute the full index curve for a base event from its tenor points.
/// Each grid tenor goes through `constant_maturity`; tenors with no
/// supporting coverage surface as `probability: None` rather than a
/// fabricated extrapolation.
pub fn compute_index(series: &str, points: &[TenorPoint], computed_at: &str) -> CmpIndex {
    let index_points = INDEX_TENORS_DAYS
        .iter()
        .map(|&tenor| match constant_maturity(points, tenor) {
            Some(value) => CmpIndexPoint {
                tenor_days: tenor,
                probability: Some(value.probability),
                method: value.method,
                cohorts: value.cohorts,
                bracket_days: value.bracket_days,
            },
            None => CmpIndexPoint {
                tenor_days: tenor,
                probability: None,
                method: CmpMethod::BucketedSparse,
                cohorts: 0,
                bracket_days: 0.0,
            },
        })
        .collect();
    CmpIndex {
        series: series.to_string(),
        computed_at: computed_at.to_string(),
        points: index_points,
    }
}

/// The slope of the curve between two tenors, in log-odds per year —
/// the term-structure signal (steepening/flattening of expectations).
/// None when either endpoint is unsupported.
pub fn curve_slope(index: &CmpIndex, short_tenor: u32, long_tenor: u32) -> Option<f64> {
    let short = index.points.iter().find(|p| p.tenor_days == short_tenor)?;
    let long = index.points.iter().find(|p| p.tenor_days == long_tenor)?;
    let (short_p, long_p) = (short.probability?, long.probability?);
    let years = (long_tenor as f64 - short_tenor as f64) / 365.25;
    if years <= 0.0 {
        return None;
    }
    Some((hkask_forecast::log_odds(long_p) - hkask_forecast::log_odds(short_p)) / years)
}
