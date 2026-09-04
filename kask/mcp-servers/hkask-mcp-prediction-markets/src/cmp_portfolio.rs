//! Constant-Maturity Prediction (CMP) indices — weighted-portfolio construction.
//!
//! A CMP index is a weighted portfolio of real prediction contracts, built so
//! the portfolio's weighted-average maturity matches a constant target
//! maturity to within a tolerance. Because it holds actual contracts with
//! weights, it is a constructible, holdable instrument (an ETF-like basket),
//! not a curve read-off — the time axis is taken out of the equation so the
//! only thing that moves is the probability.
//!
//! Per base event and per constant-maturity target, three orientation
//! indices are built: increase, decline, stable. Eligibility is a semantic
//! match (base event + orientation + materiality), and materiality is
//! volatility-based with a (type, level) setting per base event.
//!
//! Design rule: every threshold is a passed variable (`CmpConfig`). No magic
//! numbers are embedded in the logic — adjusting the config adjusts the
//! composition procedure.

use crate::cmp::CmpMethod;
use crate::types::ReliabilityTier;

// ── Configuration (all passed variables) ────────────────────────────────────

/// Materiality type: how the threshold is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialityType {
    /// Percent change from the reference (unit-free).
    Relative,
    /// Absolute change in the base contract's own units (bp, pp, $).
    Absolute,
}

/// How the materiality level scales with the target maturity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenorScaling {
    /// level ∝ √tenor (volatility scales with the square root of time).
    SqrtTenor,
    /// level ∝ tenor.
    Linear,
    /// level is tenor-independent.
    None,
}

/// Stable-index construction preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StablePreference {
    /// Use direct no-change contracts when available; balance increase vs
    /// decline as the fallback.
    DirectFirst,
    /// Always build Stable by balancing increase and decline contracts.
    SyntheticOnly,
}

/// Per-base-event materiality setting (type, level derivation, reviewed
/// override). The level is volatility-based by default; the override records
/// a reviewed per-contract adjustment with rationale.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MaterialitySetting {
    pub materiality_type: MaterialityType,
    /// k in `level = k × volatility × tenor_scaling(target)`.
    pub k: f64,
    /// Reviewed override for the level (in the family's type units). When
    /// present, it replaces the volatility-derived level.
    pub level_override: Option<f64>,
    /// Rationale for the setting / override (provenance for the review).
    pub rationale: String,
}

/// All tunables for CMP index construction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CmpConfig {
    /// Trailing window (days) for the underlying's volatility measurement.
    pub vol_window_days: u32,
    /// How the materiality level scales with target maturity.
    pub tenor_scaling: TenorScaling,
    /// Index maturity-matching error tolerance (days) for bracket interpolation.
    pub maturity_tolerance_days: f64,
    /// Eligibility window: absolute floor (days).
    pub maturity_window_abs_days: f64,
    /// Eligibility window: fraction of the target maturity.
    pub maturity_window_rel: f64,
    /// |net orientation| bound for synthetic Stable balance.
    pub stable_net_orientation_tol: f64,
    /// Stable construction preference.
    pub stable_preference: StablePreference,
    /// Days over which weight shifts from front to next contract at roll.
    pub roll_handoff_days: u32,
    /// Weakest constituent tier allowed.
    pub min_tier: ReliabilityTier,
    /// Minimum number of eligible contracts required to form a maturity
    /// bucket. Buckets with fewer contracts are withheld (never-fabricate
    /// posture). Default 3 — enough for a bracket pair plus one tie-breaker.
    pub min_constituents_per_bucket: u32,
    /// C0.5: max distance (days) from the nearest cohort to the target for
    /// single-cohort (`BucketedSparse`) publication. When no bracket spans
    /// the target but eligible contracts exist in the window, the builder
    /// publishes a degraded index at the nearest cohort's maturity, with the
    /// maturity error surfaced. If the nearest cohort is farther than this
    /// tolerance from the target, the index withholds.
    ///
    /// The effective tolerance is `max(cohort_tolerance_days, window_half_width)`
    /// — any contract in the eligibility window is publishable as a cohort.
    /// This ensures the cohort fallback fires whenever there are eligible
    /// contracts in the window, regardless of the bucket's target maturity.
    /// Set to 0 to disable the fallback (bracket-only publication, the
    /// pre-C0.5 behavior).
    pub cohort_tolerance_days: f64,
}

impl Default for CmpConfig {
    /// Default starting values — labeled defaults, not constants. Tune freely.
    fn default() -> Self {
        Self {
            vol_window_days: 90,
            tenor_scaling: TenorScaling::SqrtTenor,
            maturity_tolerance_days: 0.5,
            maturity_window_abs_days: 7.0,
            maturity_window_rel: 0.25,
            stable_net_orientation_tol: 0.05,
            stable_preference: StablePreference::DirectFirst,
            roll_handoff_days: 3,
            min_tier: ReliabilityTier::Medium,
            min_constituents_per_bucket: 3,
            // C0.5: default to the 1m window half-width (7.5 days) so any
            // contract in the shortest bucket's window is publishable as a
            // cohort. This is the widest practical tolerance — tighten for
            // stricter publication.
            cohort_tolerance_days: 7.5,
        }
    }
}

// ── Maturity buckets ─────────────────────────────────────────────────────

/// The constant-maturity targets a CMP index can be built for.
///
/// The bucket structure adapts per base object: front-loaded objects (oil,
/// gas, crypto) may only have enough contracts for 1m/2m; longer-horizon
/// objects (rates, GDP) populate 3m/6m. A bucket is only formed when enough
/// eligible contracts exist (see `select_available_buckets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaturityBucket {
    /// 1-month forward (≈30 days).
    OneMonth,
    /// 2-month forward (≈60 days).
    TwoMonth,
    /// 3-month forward (≈90 days).
    ThreeMonth,
    /// 6-month forward (≈180 days).
    SixMonth,
}

impl MaturityBucket {
    /// All buckets, ordered from shortest to longest maturity.
    pub const ALL: [MaturityBucket; 4] = [
        MaturityBucket::OneMonth,
        MaturityBucket::TwoMonth,
        MaturityBucket::ThreeMonth,
        MaturityBucket::SixMonth,
    ];

    /// Target maturity in days.
    pub fn target_days(self) -> u32 {
        match self {
            MaturityBucket::OneMonth => 30,
            MaturityBucket::TwoMonth => 60,
            MaturityBucket::ThreeMonth => 90,
            MaturityBucket::SixMonth => 180,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            MaturityBucket::OneMonth => "1m",
            MaturityBucket::TwoMonth => "2m",
            MaturityBucket::ThreeMonth => "3m",
            MaturityBucket::SixMonth => "6m",
        }
    }
}

/// Select which maturity buckets can be formed from a set of eligible
/// constituents.
///
/// A bucket is available when at least `min_constituents_per_bucket`
/// constituents fall within its eligibility window. Contracts can be re-used
/// across multiple buckets — a 45-day contract is eligible for both the 1m
/// and 2m buckets if both windows contain it.
///
/// Returns the available buckets in maturity order (shortest first). Empty
/// buckets are withheld, not fabricated.
pub fn select_available_buckets(
    constituents: &[Constituent],
    config: &CmpConfig,
) -> Vec<MaturityBucket> {
    let min = config.min_constituents_per_bucket as usize;
    MaturityBucket::ALL
        .iter()
        .filter(|bucket| {
            let target = bucket.target_days();
            let (lo, hi) = maturity_window(target, config);
            let count = constituents
                .iter()
                .filter(|c| c.days_to_expiration >= lo && c.days_to_expiration <= hi)
                .count();
            count >= min
        })
        .copied()
        .collect()
}

// ── Orientation ─────────────────────────────────────────────────────────────

/// The orientation of a contract relative to the reference level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Increase,
    Decline,
    Stable,
}

impl std::fmt::Display for Orientation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Increase => write!(f, "increase"),
            Self::Decline => write!(f, "decline"),
            Self::Stable => write!(f, "stable"),
        }
    }
}

/// Classify a contract's orientation from its predicted level relative to the
/// reference, given the materiality level (in the family's type units).
///
/// `predicted_level` is the contract's strike/threshold in the base
/// contract's units; `reference` is the current factor level; `level` is the
/// materiality threshold (already computed for the target maturity).
/// `direction_up` is whether the contract predicts the factor ends above its
/// strike (true for "above X" contracts, false for "below X").
///
/// Orientation is the direction the contract says the factor will move from
/// the reference: a contract predicting a finish above (reference + level)
/// is Increase; below (reference − level) is Decline; within the floor is
/// Stable. `direction_up` resolves the side for contracts whose strike is on
/// the opposite side of the reference from the move they predict (e.g. a
/// "below $90" contract when the reference is $100 predicts a Decline even
/// though $90 < $100).
pub fn classify_orientation(
    predicted_level: f64,
    reference: f64,
    level: f64,
    direction_up: bool,
) -> Orientation {
    // The level the contract says the factor will cross, in the direction of
    // the predicted move, measured from the reference.
    let move_size = if direction_up {
        predicted_level - reference
    } else {
        reference - predicted_level
    };
    if move_size.abs() < level {
        return Orientation::Stable;
    }
    if direction_up {
        Orientation::Increase
    } else {
        Orientation::Decline
    }
}

// ── Materiality level ───────────────────────────────────────────────────────

/// Compute the materiality level for a target maturity from the underlying's
/// volatility, honoring a reviewed override.
///
/// `level = k × volatility × scaling(target_days)` unless `level_override`
/// is set. `volatility` is the underlying's volatility over the configured
/// trailing window, in the family's type units (absolute: bp/pp/$; relative:
/// as a fraction). Returns None when volatility is unavailable and no
/// override is set — never a fabricated level.
pub fn materiality_level(
    setting: &MaterialitySetting,
    volatility: Option<f64>,
    target_days: u32,
    config: &CmpConfig,
) -> Option<f64> {
    if let Some(override_level) = setting.level_override {
        return Some(override_level);
    }
    let vol = volatility?;
    let tenor = f64::from(target_days);
    let scaling = match config.tenor_scaling {
        TenorScaling::SqrtTenor => tenor.sqrt(),
        TenorScaling::Linear => tenor,
        TenorScaling::None => 1.0,
    };
    Some(setting.k * vol * scaling)
}

// ── Eligibility ─────────────────────────────────────────────────────────────

/// Why a contract was excluded — surfaced, never silent.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EligibilityRejection {
    pub market_id: String,
    pub reason: String,
}

/// The eligibility window around a target maturity.
pub fn maturity_window(target_days: u32, config: &CmpConfig) -> (f64, f64) {
    let target = f64::from(target_days);
    let half = config
        .maturity_window_abs_days
        .max(config.maturity_window_rel * target);
    (target - half, target + half)
}

// ── Portfolio weighting ─────────────────────────────────────────────────────

/// One eligible constituent with its maturity and probability.
#[derive(Debug, Clone, Copy)]
pub struct Constituent {
    pub days_to_expiration: f64,
    pub probability: f64,
    /// Tie-break signal: higher is preferred (e.g. liquidity, tier rank).
    pub quality: f64,
}

/// A weighted constituent of the index portfolio.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WeightedConstituent {
    pub market_index: usize,
    pub weight: f64,
    pub days_to_expiration: f64,
    pub probability: f64,
}

/// The solved index portfolio.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexPortfolio {
    pub constituents: Vec<WeightedConstituent>,
    /// Weighted-average maturity of the portfolio (days).
    pub weighted_maturity_days: f64,
    /// |weighted maturity − target| — the maturity-matching error.
    pub maturity_error_days: f64,
    /// The index probability (weighted average of constituent probabilities).
    pub index_probability: f64,
    /// C0.5: the construction method — `Interpolated` (bracket pair) or
    /// `BucketedSparse` (single-cohort fallback). Downstream consumers use
    /// this to weight the index appropriately: a `BucketedSparse` index has
    /// wider uncertainty (the maturity error is the distance from the cohort
    /// to the target, not a residual from exact interpolation).
    pub method: CmpMethod,
}

/// Solve portfolio weights so the weighted-average maturity matches the
/// target within the configured tolerance.
///
/// Construction: choose the pair of constituents bracketing the target with
/// the highest combined quality and solve the two-weight system exactly;
/// when no bracket spans the target, fall back to `solve_portfolio_cohort`
/// (C0.5 single-cohort publication). Returns `None` only when both the
/// bracket solver and the cohort solver fail — never fabricates.
pub fn solve_portfolio(
    constituents: &[Constituent],
    target_days: u32,
    config: &CmpConfig,
) -> Option<IndexPortfolio> {
    let target = f64::from(target_days);
    // Bracketing pair: one at-or-below, one at-or-above the target.
    let mut best: Option<(usize, usize, f64)> = None; // (lo_idx, hi_idx, quality)
    for (i, a) in constituents.iter().enumerate() {
        for (j, b) in constituents.iter().enumerate() {
            if i == j {
                continue;
            }
            let (lo, hi, lo_i, hi_i) = if a.days_to_expiration <= b.days_to_expiration {
                (a, b, i, j)
            } else {
                (b, a, j, i)
            };
            if lo.days_to_expiration <= target
                && hi.days_to_expiration >= target
                && (hi.days_to_expiration - lo.days_to_expiration) > 1e-9
            {
                let quality = lo.quality + hi.quality;
                if best.map_or(true, |(_, _, q)| quality > q) {
                    best = Some((lo_i, hi_i, quality));
                }
            }
        }
    }
    if let Some((lo_i, hi_i, _)) = best {
        let lo = constituents[lo_i];
        let hi = constituents[hi_i];
        let span = hi.days_to_expiration - lo.days_to_expiration;
        let w_hi = (target - lo.days_to_expiration) / span;
        let w_lo = 1.0 - w_hi;

        let weighted_maturity = w_lo * lo.days_to_expiration + w_hi * hi.days_to_expiration;
        let maturity_error = (weighted_maturity - target).abs();
        if maturity_error > config.maturity_tolerance_days {
            // Bracket exists but can't meet tolerance — try the cohort fallback
            // before withholding. The cohort may be closer to the target than
            // the bracket's residual.
            return solve_portfolio_cohort(constituents, target_days, config);
        }
        let index_probability = w_lo * lo.probability + w_hi * hi.probability;

        return Some(IndexPortfolio {
            constituents: vec![
                WeightedConstituent {
                    market_index: lo_i,
                    weight: w_lo,
                    days_to_expiration: lo.days_to_expiration,
                    probability: lo.probability,
                },
                WeightedConstituent {
                    market_index: hi_i,
                    weight: w_hi,
                    days_to_expiration: hi.days_to_expiration,
                    probability: hi.probability,
                },
            ],
            weighted_maturity_days: weighted_maturity,
            maturity_error_days: maturity_error,
            index_probability,
            method: CmpMethod::Interpolated,
        });
    }
    // No bracket spans the target — try the single-cohort fallback (C0.5).
    solve_portfolio_cohort(constituents, target_days, config)
}

/// C0.5: single-cohort fallback. When no bracket pair spans the target, pick
/// the highest-quality cohort (group of contracts at the same maturity) and
/// publish a degraded index at that maturity. The maturity error is the
/// distance from the cohort to the target — surfaced, not hidden.
///
/// This is the honest degraded publication the plan anticipated (cmp-foundation
/// §5: "sparse coverage degrades honestly"). The `BucketedSparse` method flag
/// tells downstream consumers the index has wider uncertainty than a bracket-
/// interpolated index.
///
/// Returns `None` when the nearest cohort is farther than
/// `cohort_tolerance_days` from the target, or when `cohort_tolerance_days` is
/// 0 (fallback disabled). Never fabricates a probability.
fn solve_portfolio_cohort(
    constituents: &[Constituent],
    target_days: u32,
    config: &CmpConfig,
) -> Option<IndexPortfolio> {
    if config.cohort_tolerance_days <= 0.0 || constituents.is_empty() {
        return None; // fallback disabled, or no constituents
    }
    let target = f64::from(target_days);
    // The effective cohort tolerance is max(cohort_tolerance_days, window_half_width)
    // — any contract in the eligibility window is publishable as a cohort.
    let window_half = config
        .maturity_window_abs_days
        .max(config.maturity_window_rel * target);
    let effective_tolerance = config.cohort_tolerance_days.max(window_half);
    // Group constituents into cohorts by maturity (within 1 day of each other).
    // A cohort is a set of contracts at the same maturity — their mean
    // probability is the cohort value, their combined quality is the tie-break.
    // Sort indices by maturity so cohorts are contiguous.
    let mut order: Vec<usize> = (0..constituents.len()).collect();
    order.sort_by(|&a, &b| {
        constituents[a]
            .days_to_expiration
            .partial_cmp(&constituents[b].days_to_expiration)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut cohorts: Vec<(f64, f64, f64, Vec<usize>)> = Vec::new(); // (maturity, prob_sum, quality_sum, indices)
    for idx in order {
        let c = &constituents[idx];
        if let Some(last) = cohorts.last_mut()
            && (last.0 - c.days_to_expiration).abs() < 1.0
        {
            last.1 += c.probability;
            last.2 += c.quality;
            last.3.push(idx);
            continue;
        }
        cohorts.push((c.days_to_expiration, c.probability, c.quality, vec![idx]));
    }
    // Find the cohort closest to the target. Ties (equal distance) are broken
    // by iteration order — since cohorts are sorted by maturity ascending, the
    // shorter-maturity cohort wins. This is the conservative tie-break: a
    // shorter-maturity cohort has less time for the probability to drift from
    // the index value, so it's the marginally more reliable choice.
    let best_cohort = cohorts.iter().min_by(|a, b| {
        let dist_a = (a.0 - target).abs();
        let dist_b = (b.0 - target).abs();
        dist_a
            .partial_cmp(&dist_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    })?;
    let cohort_maturity = best_cohort.0;
    let maturity_error = (cohort_maturity - target).abs();
    if maturity_error > effective_tolerance {
        return None; // nearest cohort too far from target — withhold
    }
    // Build the portfolio: equal weights within the cohort.
    let n = best_cohort.3.len();
    let weight = 1.0 / n as f64;
    let index_probability = best_cohort.1 / n as f64; // mean probability
    let constituents_out: Vec<WeightedConstituent> = best_cohort
        .3
        .iter()
        .map(|&idx| {
            let c = &constituents[idx];
            WeightedConstituent {
                market_index: idx,
                weight,
                days_to_expiration: c.days_to_expiration,
                probability: c.probability,
            }
        })
        .collect();
    Some(IndexPortfolio {
        constituents: constituents_out,
        weighted_maturity_days: cohort_maturity,
        maturity_error_days: maturity_error,
        index_probability,
        method: CmpMethod::BucketedSparse,
    })
}

// ── CMP index construction (ties buckets + orientation + portfolio) ────────

/// A constituent tagged with its orientation, for per-orientation index
/// construction. The orientation is computed by `classify_orientation`
/// during eligibility; this struct carries the result so the index builder
/// can filter by orientation without re-classifying.
#[derive(Debug, Clone, Copy)]
pub struct OrientedConstituent {
    pub constituent: Constituent,
    pub orientation: Orientation,
    /// Index into the original market-records slice, for provenance.
    pub market_index: usize,
}

/// One CMP index — a single (base object, maturity bucket, orientation)
/// triple with its solved portfolio. This is the publishable unit: the daily
/// index probability, constituent weights, and maturity-matching error.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CmpIndex {
    /// The maturity bucket this index is built for.
    pub bucket: MaturityBucket,
    /// The orientation (increase, decline, stable).
    pub orientation: Orientation,
    /// The solved portfolio.
    pub portfolio: IndexPortfolio,
}

/// The full set of CMP indices for one base object across all available
/// maturity buckets and all three orientations. This is what gets published
/// daily per base object.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmpIndexSet {
    /// Which maturity buckets were available (had enough contracts).
    pub available_buckets: Vec<MaturityBucket>,
    /// The constructed indices, one per (bucket, orientation) that could be
    /// solved. Indices that couldn't be solved (no bracket, tolerance
    /// failure) are omitted — withheld, not fabricated.
    pub indices: Vec<CmpIndex>,
    /// Buckets that were withheld because they had fewer than
    /// `min_constituents_per_bucket` eligible contracts.
    pub withheld_buckets: Vec<MaturityBucket>,
}

/// Construct the full CMP index set for one base object.
///
/// The procedure:
/// 1. Select available maturity buckets (≥ `min_constituents_per_bucket`
///    constituents in each bucket's eligibility window).
/// 2. For each available bucket, filter constituents into the bucket's
///    maturity window and group by orientation (increase, decline, stable).
/// 3. For each (bucket, orientation) pair, solve the portfolio weights so
///    the weighted-average maturity matches the bucket's target. C0.5: when
///    the bracket solver fails (no bracket pair), fall back to the single-
///    cohort solver (`BucketedSparse`) — a degraded but honest publication
///    with the maturity error surfaced.
/// 4. Withhold any index that can't be solved (no bracket AND no cohort within
///    tolerance) — never fabricate.
///
/// Contracts are re-used across buckets and across orientations: the same
/// contract may appear in the 1m increase index and the 2m increase index,
/// or in the 1m increase and 1m decline indices (though a single contract
/// has exactly one orientation, so it appears in at most one orientation per
/// bucket).
pub fn construct_cmp_index_set(
    oriented: &[OrientedConstituent],
    config: &CmpConfig,
) -> CmpIndexSet {
    // Step 1: select available buckets from all constituents regardless of
    // orientation — the bucket is available if the object has enough contracts
    // in the maturity window, even if they split across orientations.
    let all_constituents: Vec<Constituent> = oriented.iter().map(|oc| oc.constituent).collect();
    let available = select_available_buckets(&all_constituents, config);

    let withheld: Vec<MaturityBucket> = MaturityBucket::ALL
        .iter()
        .filter(|b| !available.contains(b))
        .copied()
        .collect();

    let mut indices: Vec<CmpIndex> = Vec::new();

    for bucket in &available {
        let target = bucket.target_days();
        let (lo, hi) = maturity_window(target, config);

        // Filter constituents into this bucket's maturity window.
        let in_window: Vec<&OrientedConstituent> = oriented
            .iter()
            .filter(|oc| {
                oc.constituent.days_to_expiration >= lo && oc.constituent.days_to_expiration <= hi
            })
            .collect();

        // For each orientation, collect the constituents and solve the portfolio.
        for orientation in [
            Orientation::Increase,
            Orientation::Decline,
            Orientation::Stable,
        ] {
            let orientation_constituents: Vec<Constituent> = in_window
                .iter()
                .filter(|oc| oc.orientation == orientation)
                .map(|oc| oc.constituent)
                .collect();

            if orientation_constituents.is_empty() {
                continue; // no contracts for this orientation in this bucket
            }

            // Solve the portfolio for this (bucket, orientation).
            if let Some(portfolio) = solve_portfolio(&orientation_constituents, target, config) {
                indices.push(CmpIndex {
                    bucket: *bucket,
                    orientation,
                    portfolio,
                });
            }
            // If solve_portfolio returns None (no bracket, tolerance failure),
            // the index is withheld — not fabricated.
        }
    }

    CmpIndexSet {
        available_buckets: available,
        indices,
        withheld_buckets: withheld,
    }
}
