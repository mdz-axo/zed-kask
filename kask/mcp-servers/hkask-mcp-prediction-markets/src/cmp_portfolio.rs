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
use crate::types::{MarketRecord, ReliabilityTier};

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

/// A contract presented for eligibility, with the semantic fields the
/// classifier needs. Extracted from `MarketRecord` + the semantic mapping.
#[derive(Debug, Clone)]
pub struct EligibilityInput<'a> {
    pub record: &'a MarketRecord,
    /// The contract's predicted level (strike) in base units, if extractable.
    pub predicted_level: Option<f64>,
    /// Whether the contract predicts the factor ends above its strike.
    pub direction_up: bool,
    /// Days from observation to the contract's expiration.
    pub days_to_expiration: Option<f64>,
}

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

/// Full eligibility pipeline for one record against one index (C0.2): base
/// event classification → materiality level → orientation → maturity window →
/// reliability floor. Returns the matched base event and orientation when
/// eligible, else a rejection with the reason.
///
/// `reference` is the current factor level; `volatility` is the underlying's
/// trailing volatility in the family's type units (None when unmeasured — the
/// materiality override then governs, or eligibility fails honestly).
pub fn evaluate_record(
    input: &EligibilityInput,
    index_base_event: crate::base_event::BaseEvent,
    index_orientation: Orientation,
    reference: f64,
    volatility: Option<f64>,
    target_days: u32,
    config: &CmpConfig,
) -> Result<(crate::base_event::BaseEvent, Orientation), EligibilityRejection> {
    let reject = |reason: String| EligibilityRejection {
        market_id: input.record.market_id.clone(),
        reason,
    };

    // Semantic base-event match.
    let base_event = crate::base_event::classify_base_event(input.record)
        .ok_or_else(|| reject("not a base-event contract".into()))?;
    if base_event != index_base_event {
        return Err(reject(format!(
            "base event {base_event:?} does not match index {index_base_event:?}"
        )));
    }

    // Materiality level for this family and target maturity.
    let setting = base_event.default_materiality();
    let level = materiality_level(&setting, volatility, target_days, config)
        .ok_or_else(|| reject("no materiality level (no volatility, no override)".into()))?;

    // Orientation + maturity + reliability.
    let orientation = check_eligibility(
        input,
        index_orientation,
        reference,
        level,
        target_days,
        config,
    )?;
    Ok((base_event, orientation))
}

/// Test a contract's eligibility for an index of the given orientation.
/// Returns Ok(orientation) when eligible, or records a rejection.
pub fn check_eligibility(
    input: &EligibilityInput,
    index_orientation: Orientation,
    reference: f64,
    level: f64,
    target_days: u32,
    config: &CmpConfig,
) -> Result<Orientation, EligibilityRejection> {
    let reject = |reason: String| EligibilityRejection {
        market_id: input.record.market_id.clone(),
        reason,
    };

    // Reliability floor.
    if tier_rank(input.record.reliability_tier) < tier_rank(config.min_tier) {
        return Err(reject(format!(
            "reliability tier {:?} below minimum {:?}",
            input.record.reliability_tier, config.min_tier
        )));
    }

    // Maturity window.
    let days = input
        .days_to_expiration
        .ok_or_else(|| reject("unparseable expiration — maturity unknown".into()))?;
    let (lo, hi) = maturity_window(target_days, config);
    if days < lo || days > hi {
        return Err(reject(format!(
            "expiration {days:.1}d outside eligibility window [{lo:.1}, {hi:.1}]"
        )));
    }

    // Orientation via materiality.
    let predicted = input
        .predicted_level
        .ok_or_else(|| reject("no extractable predicted level (strike)".into()))?;
    let orientation = classify_orientation(predicted, reference, level, input.direction_up);
    if orientation != index_orientation {
        return Err(reject(format!(
            "orientation {orientation:?} does not match index {index_orientation:?}"
        )));
    }

    Ok(orientation)
}

fn tier_rank(tier: ReliabilityTier) -> u8 {
    match tier {
        ReliabilityTier::High => 2,
        ReliabilityTier::Medium => 1,
        ReliabilityTier::Low => 0,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> CmpConfig {
        CmpConfig::default()
    }

    #[test]
    fn materiality_level_uses_override_when_set() {
        let setting = MaterialitySetting {
            materiality_type: MaterialityType::Absolute,
            k: 1.0,
            level_override: Some(0.125),
            rationale: "reviewed: 12.5bp for rates".into(),
        };
        let level = materiality_level(&setting, Some(0.01), 90, &config());
        assert_eq!(level, Some(0.125));
    }

    #[test]
    fn materiality_level_derives_from_volatility() {
        let setting = MaterialitySetting {
            materiality_type: MaterialityType::Relative,
            k: 1.0,
            level_override: None,
            rationale: String::new(),
        };
        // k=1.0 × vol=0.02 × √90 ≈ 0.1897
        let level = materiality_level(&setting, Some(0.02), 90, &config()).expect("derived");
        assert!((level - 0.1897).abs() < 0.001, "level {level}");
    }

    #[test]
    fn materiality_level_none_without_volatility_or_override() {
        let setting = MaterialitySetting {
            materiality_type: MaterialityType::Relative,
            k: 1.0,
            level_override: None,
            rationale: String::new(),
        };
        assert!(materiality_level(&setting, None, 90, &config()).is_none());
    }

    #[test]
    fn classify_orientation_respects_materiality_floor() {
        // Up contract barely above reference (inside floor) → Stable.
        assert_eq!(
            classify_orientation(101.0, 100.0, 2.0, true),
            Orientation::Stable
        );
        // Up contract well above reference → Increase.
        assert_eq!(
            classify_orientation(105.0, 100.0, 2.0, true),
            Orientation::Increase
        );
        // Down contract well below reference → Decline.
        assert_eq!(
            classify_orientation(95.0, 100.0, 2.0, false),
            Orientation::Decline
        );
        // Down contract whose strike is below reference but the predicted
        // move (down) is smaller than the floor → Stable.
        assert_eq!(
            classify_orientation(99.0, 100.0, 2.0, false),
            Orientation::Stable
        );
    }

    #[test]
    fn solve_portfolio_exact_two_contract_bracket() {
        // Target 90d; contracts at 60d (p=0.40) and 120d (p=0.60).
        // w_hi = (90−60)/(120−60) = 0.5 → index p = 0.5.
        let constituents = [
            Constituent {
                days_to_expiration: 60.0,
                probability: 0.40,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 120.0,
                probability: 0.60,
                quality: 1.0,
            },
        ];
        let portfolio = solve_portfolio(&constituents, 90, &config()).expect("bracket");
        assert!((portfolio.weighted_maturity_days - 90.0).abs() < 1e-9);
        assert!(portfolio.maturity_error_days <= 0.5);
        assert!((portfolio.index_probability - 0.50).abs() < 1e-9);
        let weights: Vec<f64> = portfolio.constituents.iter().map(|c| c.weight).collect();
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn solve_portfolio_withholds_when_no_bracket() {
        // All contracts below the target — no bracket. The nearest cohort
        // (45d) is 45d from the 90d target, beyond the default
        // cohort_tolerance_days (7.5) → withhold.
        let constituents = [
            Constituent {
                days_to_expiration: 30.0,
                probability: 0.40,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 45.0,
                probability: 0.60,
                quality: 1.0,
            },
        ];
        assert!(solve_portfolio(&constituents, 90, &config()).is_none());
    }

    #[test]
    fn solve_portfolio_cohort_fallback_when_no_bracket() {
        // C0.5: no bracket (all contracts at the same maturity), but the
        // cohort is within tolerance of the target → publish as BucketedSparse.
        // Target 90d; two contracts both at 88d (within 1d → same cohort).
        // Nearest cohort distance = |88 - 90| = 2d ≤ 7.5d → publish.
        let constituents = [
            Constituent {
                days_to_expiration: 88.0,
                probability: 0.40,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 88.0,
                probability: 0.60,
                quality: 1.0,
            },
        ];
        let portfolio = solve_portfolio(&constituents, 90, &config()).expect("cohort fallback");
        assert_eq!(portfolio.method, CmpMethod::BucketedSparse);
        assert!((portfolio.weighted_maturity_days - 88.0).abs() < 1e-9);
        assert!((portfolio.maturity_error_days - 2.0).abs() < 1e-9);
        // Mean probability of the two contracts.
        assert!((portfolio.index_probability - 0.50).abs() < 1e-9);
        // Equal weights within the cohort.
        assert!(
            portfolio
                .constituents
                .iter()
                .all(|c| (c.weight - 0.5).abs() < 1e-9)
        );
        let weights: Vec<f64> = portfolio.constituents.iter().map(|c| c.weight).collect();
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn solve_portfolio_cohort_withholds_beyond_tolerance() {
        // C0.5: no bracket, nearest cohort outside the eligibility window
        // → withhold. Target 90d; contracts at 50d (40d away, outside the
        // 3m window [67.5, 112.5]). The effective tolerance is the window
        // half-width (22.5d), so 40d > 22.5d → withhold.
        let constituents = [
            Constituent {
                days_to_expiration: 50.0,
                probability: 0.40,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 50.0,
                probability: 0.60,
                quality: 1.0,
            },
        ];
        assert!(solve_portfolio(&constituents, 90, &config()).is_none());
    }

    #[test]
    fn solve_portfolio_cohort_disabled_when_tolerance_zero() {
        // C0.5: cohort_tolerance_days = 0 disables the fallback (bracket-only).
        let mut cfg = config();
        cfg.cohort_tolerance_days = 0.0;
        // Contracts at 88d (within 2d of 90d target) — would publish as cohort
        // with default tolerance, but disabled here.
        let constituents = [
            Constituent {
                days_to_expiration: 88.0,
                probability: 0.40,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 88.0,
                probability: 0.60,
                quality: 1.0,
            },
        ];
        assert!(solve_portfolio(&constituents, 90, &cfg).is_none());
    }

    #[test]
    fn solve_portfolio_prefers_bracket_over_cohort() {
        // C0.5: when both a bracket and a cohort exist, the bracket wins
        // (it's the more precise construction). Target 90d; bracket at 70d
        // and 110d, plus a cohort at 88d. The bracket should be used.
        let constituents = [
            Constituent {
                days_to_expiration: 70.0,
                probability: 0.30,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 110.0,
                probability: 0.70,
                quality: 1.0,
            },
            Constituent {
                days_to_expiration: 88.0,
                probability: 0.50,
                quality: 1.0,
            },
        ];
        let portfolio = solve_portfolio(&constituents, 90, &config()).expect("bracket");
        assert_eq!(portfolio.method, CmpMethod::Interpolated);
        assert!((portfolio.weighted_maturity_days - 90.0).abs() < 1e-9);
        assert!((portfolio.index_probability - 0.50).abs() < 1e-9);
    }

    #[test]
    fn maturity_window_uses_max_of_abs_and_rel() {
        let c = config();
        // 1m target: max(7, 0.25×30=7.5) = 7.5 → [22.5, 37.5]
        let (lo, hi) = maturity_window(30, &c);
        assert!((lo - 22.5).abs() < 1e-9 && (hi - 37.5).abs() < 1e-9);
        // 6m target: max(7, 0.25×180=45) = 45 → [135, 225]
        let (lo, hi) = maturity_window(180, &c);
        assert!((lo - 135.0).abs() < 1e-9 && (hi - 225.0).abs() < 1e-9);
    }

    // ── C0.2 evaluate_record (semantic eligibility pipeline) ─────────────

    fn eligibility_record(question: &str, series: &str) -> crate::types::MarketRecord {
        let mut record = crate::types::test_utils::market_record_fixture();
        record.question = question.into();
        record.series = series.into();
        record
    }

    #[test]
    fn evaluate_record_accepts_matching_base_event_and_orientation() {
        let record = eligibility_record("Will the Fed raise the policy rate?", "KXFEDDECISION");
        let input = EligibilityInput {
            record: &record,
            predicted_level: Some(5.0),
            direction_up: true,
            days_to_expiration: Some(90.0),
        };
        // Reference 4.5%, level 0.25 (25bp) → 5.0 is 50bp above → Increase.
        let result = evaluate_record(
            &input,
            crate::base_event::BaseEvent::InterestRates,
            Orientation::Increase,
            4.5,
            Some(0.005),
            90,
            &config(),
        );
        assert!(result.is_ok(), "expected eligible: {result:?}");
    }

    #[test]
    fn evaluate_record_rejects_wrong_base_event() {
        let record = eligibility_record("Will Bitcoin exceed $150k?", "KXBTC");
        let input = EligibilityInput {
            record: &record,
            predicted_level: Some(150_000.0),
            direction_up: true,
            days_to_expiration: Some(90.0),
        };
        let result = evaluate_record(
            &input,
            crate::base_event::BaseEvent::InterestRates, // wrong family
            Orientation::Increase,
            100_000.0,
            Some(0.05),
            90,
            &config(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn evaluate_record_rejects_non_base_event() {
        let record = eligibility_record("Will the mayor win re-election?", "KXMAYOR");
        let input = EligibilityInput {
            record: &record,
            predicted_level: Some(1.0),
            direction_up: true,
            days_to_expiration: Some(90.0),
        };
        let result = evaluate_record(
            &input,
            crate::base_event::BaseEvent::InterestRates,
            Orientation::Increase,
            0.5,
            Some(0.01),
            90,
            &config(),
        );
        let err = result.expect_err("not a base event");
        assert!(err.reason.contains("not a base-event"));
    }

    #[test]
    fn evaluate_record_rejects_when_no_materiality_level() {
        let record = eligibility_record("Will the Fed raise the policy rate?", "KXFEDDECISION");
        let input = EligibilityInput {
            record: &record,
            predicted_level: Some(5.0),
            direction_up: true,
            days_to_expiration: Some(90.0),
        };
        // No volatility and no override → no level → honest rejection.
        let result = evaluate_record(
            &input,
            crate::base_event::BaseEvent::InterestRates,
            Orientation::Increase,
            4.5,
            None,
            90,
            &config(),
        );
        let err = result.expect_err("no level");
        assert!(err.reason.contains("no materiality level"));
    }

    // ── Maturity bucket selection tests ──────────────────────────────────

    fn constituent(days: f64, quality: f64) -> Constituent {
        Constituent {
            days_to_expiration: days,
            probability: 0.5,
            quality,
        }
    }

    #[test]
    fn bucket_1m_available_with_3_contracts_near_30d() {
        // 3 contracts within the 1m eligibility window.
        let constituents = vec![
            constituent(25.0, 1.0),
            constituent(30.0, 1.0),
            constituent(35.0, 1.0),
        ];
        let buckets = select_available_buckets(&constituents, &config());
        assert!(buckets.contains(&MaturityBucket::OneMonth));
    }

    #[test]
    fn bucket_withheld_with_fewer_than_3_contracts() {
        // Only 2 contracts near 30d — 1m bucket must be withheld.
        let constituents = vec![
            constituent(28.0, 1.0),
            constituent(32.0, 1.0),
            // 3 contracts near 90d — 3m bucket is available.
            constituent(85.0, 1.0),
            constituent(90.0, 1.0),
            constituent(95.0, 1.0),
        ];
        let buckets = select_available_buckets(&constituents, &config());
        assert!(
            !buckets.contains(&MaturityBucket::OneMonth),
            "1m must be withheld with only 2 contracts"
        );
        assert!(
            buckets.contains(&MaturityBucket::ThreeMonth),
            "3m must be available with 3 contracts"
        );
    }

    #[test]
    fn contracts_reused_across_multiple_buckets() {
        // The 1m window is [22, 38] and the 2m window is [45, 75] with
        // default config — they don't overlap. But a contract at 38d is in
        // 1m, and a contract at 45d is in 2m. Test that both buckets form
        // when each has enough contracts. Contract reuse happens at the
        // 2m/3m boundary (2m window [45,75], 3m window [68,112] — overlap
        // at [68,75]).
        let constituents = vec![
            // 1m window [22, 38]: 3 contracts
            constituent(25.0, 1.0),
            constituent(30.0, 1.0),
            constituent(35.0, 1.0),
            // 2m window [45, 75]: 3 contracts (70d also in 3m window)
            constituent(50.0, 1.0),
            constituent(60.0, 1.0),
            constituent(70.0, 1.0),
        ];
        let buckets = select_available_buckets(&constituents, &config());
        assert!(
            buckets.contains(&MaturityBucket::OneMonth),
            "1m should be available"
        );
        assert!(
            buckets.contains(&MaturityBucket::TwoMonth),
            "2m should be available"
        );
    }

    #[test]
    fn front_loaded_object_gets_1m_2m_only() {
        // Oil/gas pattern: lots of short-dated contracts, nothing beyond 150d.
        let constituents = vec![
            // 1m window [22, 38]: 3+ contracts
            constituent(25.0, 1.0),
            constituent(30.0, 1.0),
            constituent(35.0, 1.0),
            // 2m window [45, 75]: 3+ contracts
            constituent(50.0, 1.0),
            constituent(55.0, 1.0),
            constituent(60.0, 1.0),
            constituent(65.0, 1.0),
        ];
        let buckets = select_available_buckets(&constituents, &config());
        assert!(
            buckets.contains(&MaturityBucket::OneMonth),
            "1m should be available"
        );
        assert!(
            buckets.contains(&MaturityBucket::TwoMonth),
            "2m should be available"
        );
        assert!(
            !buckets.contains(&MaturityBucket::SixMonth),
            "6m should be withheld (no long-dated contracts)"
        );
    }

    #[test]
    fn long_horizon_object_gets_3m_6m() {
        // Rates/GDP pattern: contracts out to 380+ days.
        let constituents = vec![
            constituent(85.0, 1.0),
            constituent(90.0, 1.0),
            constituent(95.0, 1.0),
            constituent(170.0, 1.0),
            constituent(180.0, 1.0),
            constituent(190.0, 1.0),
        ];
        let buckets = select_available_buckets(&constituents, &config());
        assert!(
            buckets.contains(&MaturityBucket::ThreeMonth),
            "3m should be available"
        );
        assert!(
            buckets.contains(&MaturityBucket::SixMonth),
            "6m should be available"
        );
        assert!(
            !buckets.contains(&MaturityBucket::OneMonth),
            "1m should be withheld (no short-dated contracts)"
        );
    }

    #[test]
    fn min_constituents_threshold_configurable() {
        // With min=5, 3 contracts is not enough.
        let mut cfg = config();
        cfg.min_constituents_per_bucket = 5;
        let constituents = vec![
            constituent(28.0, 1.0),
            constituent(30.0, 1.0),
            constituent(32.0, 1.0),
        ];
        let buckets = select_available_buckets(&constituents, &cfg);
        assert!(
            buckets.is_empty(),
            "no buckets should be available with min=5 and only 3 contracts"
        );
    }

    #[test]
    fn maturity_bucket_target_days() {
        assert_eq!(MaturityBucket::OneMonth.target_days(), 30);
        assert_eq!(MaturityBucket::TwoMonth.target_days(), 60);
        assert_eq!(MaturityBucket::ThreeMonth.target_days(), 90);
        assert_eq!(MaturityBucket::SixMonth.target_days(), 180);
    }

    #[test]
    fn maturity_bucket_labels() {
        assert_eq!(MaturityBucket::OneMonth.label(), "1m");
        assert_eq!(MaturityBucket::TwoMonth.label(), "2m");
        assert_eq!(MaturityBucket::ThreeMonth.label(), "3m");
        assert_eq!(MaturityBucket::SixMonth.label(), "6m");
    }

    // ── construct_cmp_index_set tests ────────────────────────────────────

    fn oriented(
        days: f64,
        prob: f64,
        quality: f64,
        orientation: Orientation,
    ) -> OrientedConstituent {
        OrientedConstituent {
            constituent: Constituent {
                days_to_expiration: days,
                probability: prob,
                quality,
            },
            orientation,
            market_index: 0,
        }
    }

    #[test]
    fn construct_index_set_builds_all_orientations_for_available_bucket() {
        // 3m bucket [68, 112]: enough contracts for increase + decline.
        let oriented_constituents = vec![
            // Increase contracts bracketing 90d
            oriented(80.0, 0.60, 1.0, Orientation::Increase),
            oriented(100.0, 0.55, 1.0, Orientation::Increase),
            oriented(90.0, 0.50, 1.0, Orientation::Increase),
            // Decline contracts bracketing 90d
            oriented(75.0, 0.40, 1.0, Orientation::Decline),
            oriented(105.0, 0.45, 1.0, Orientation::Decline),
            oriented(90.0, 0.35, 1.0, Orientation::Decline),
        ];
        let set = construct_cmp_index_set(&oriented_constituents, &config());
        assert!(set.available_buckets.contains(&MaturityBucket::ThreeMonth));
        // Should have increase + decline indices for 3m.
        let has_increase = set.indices.iter().any(|i| {
            i.bucket == MaturityBucket::ThreeMonth && i.orientation == Orientation::Increase
        });
        let has_decline = set.indices.iter().any(|i| {
            i.bucket == MaturityBucket::ThreeMonth && i.orientation == Orientation::Decline
        });
        assert!(has_increase, "3m increase index should be constructed");
        assert!(has_decline, "3m decline index should be constructed");
    }

    #[test]
    fn construct_index_set_withholds_bucket_with_no_orientation() {
        // 3m bucket has enough total contracts, but all are Increase — no
        // Decline or Stable index can be built for 3m.
        let oriented_constituents = vec![
            oriented(80.0, 0.60, 1.0, Orientation::Increase),
            oriented(90.0, 0.55, 1.0, Orientation::Increase),
            oriented(100.0, 0.50, 1.0, Orientation::Increase),
        ];
        let set = construct_cmp_index_set(&oriented_constituents, &config());
        assert!(set.available_buckets.contains(&MaturityBucket::ThreeMonth));
        // Only increase index for 3m.
        assert_eq!(set.indices.len(), 1);
        assert_eq!(set.indices[0].orientation, Orientation::Increase);
        assert_eq!(set.indices[0].bucket, MaturityBucket::ThreeMonth);
    }

    #[test]
    fn construct_index_set_withholds_unavailable_buckets() {
        // Only short-dated contracts — 1m and 2m available, 3m and 6m withheld.
        let oriented_constituents = vec![
            oriented(25.0, 0.50, 1.0, Orientation::Increase),
            oriented(30.0, 0.50, 1.0, Orientation::Increase),
            oriented(35.0, 0.50, 1.0, Orientation::Increase),
            oriented(50.0, 0.50, 1.0, Orientation::Decline),
            oriented(55.0, 0.50, 1.0, Orientation::Decline),
            oriented(60.0, 0.50, 1.0, Orientation::Decline),
        ];
        let set = construct_cmp_index_set(&oriented_constituents, &config());
        assert!(set.available_buckets.contains(&MaturityBucket::OneMonth));
        assert!(set.available_buckets.contains(&MaturityBucket::TwoMonth));
        assert!(!set.available_buckets.contains(&MaturityBucket::ThreeMonth));
        assert!(!set.available_buckets.contains(&MaturityBucket::SixMonth));
        assert!(set.withheld_buckets.contains(&MaturityBucket::ThreeMonth));
        assert!(set.withheld_buckets.contains(&MaturityBucket::SixMonth));
    }

    #[test]
    fn construct_index_set_contracts_reused_across_buckets() {
        // A contract at 70d is in both 2m [45,75] and 3m [68,112].
        // With enough contracts in each window, both buckets should form.
        let oriented_constituents = vec![
            // 1m [22, 38]: 3 increase contracts
            oriented(25.0, 0.50, 1.0, Orientation::Increase),
            oriented(30.0, 0.50, 1.0, Orientation::Increase),
            oriented(35.0, 0.50, 1.0, Orientation::Increase),
            // 2m [45, 75]: 3 increase contracts (70d also in 3m window)
            oriented(50.0, 0.50, 1.0, Orientation::Increase),
            oriented(60.0, 0.50, 1.0, Orientation::Increase),
            oriented(70.0, 0.50, 1.0, Orientation::Increase),
            // 3m [68, 112]: 3 decline contracts (70d is also here but it's increase)
            oriented(80.0, 0.50, 1.0, Orientation::Decline),
            oriented(90.0, 0.50, 1.0, Orientation::Decline),
            oriented(100.0, 0.50, 1.0, Orientation::Decline),
        ];
        let set = construct_cmp_index_set(&oriented_constituents, &config());
        assert!(set.available_buckets.contains(&MaturityBucket::OneMonth));
        assert!(set.available_buckets.contains(&MaturityBucket::TwoMonth));
        assert!(set.available_buckets.contains(&MaturityBucket::ThreeMonth));
    }

    #[test]
    fn construct_index_set_cohort_fallback_when_no_bracket() {
        // C0.5: 3m bucket has 3 increase contracts but none bracket 90d —
        // all below. The cohort fallback publishes a BucketedSparse index
        // at the nearest cohort (75d), with maturity_error = 15d.
        let oriented_constituents = vec![
            oriented(70.0, 0.50, 1.0, Orientation::Increase),
            oriented(72.0, 0.50, 1.0, Orientation::Increase),
            oriented(75.0, 0.50, 1.0, Orientation::Increase),
        ];
        let set = construct_cmp_index_set(&oriented_constituents, &config());
        // 3m bucket is available (3 contracts in window [68,112])
        assert!(set.available_buckets.contains(&MaturityBucket::ThreeMonth));
        // C0.5: the cohort fallback publishes BucketedSparse indices.
        assert!(!set.indices.is_empty(), "cohort fallback should publish");
        // The 3m index should be present as a BucketedSparse cohort.
        let three_m = set
            .indices
            .iter()
            .find(|i| i.bucket == MaturityBucket::ThreeMonth)
            .expect("3m index should publish via cohort fallback");
        assert_eq!(three_m.portfolio.method, CmpMethod::BucketedSparse);
        // Nearest cohort is 75d (the contracts at 70/72/75 form separate
        // cohorts since they're >1d apart; 75d is closest to 90d).
        assert!((three_m.portfolio.weighted_maturity_days - 75.0).abs() < 1e-9);
        assert!((three_m.portfolio.maturity_error_days - 15.0).abs() < 1e-9);
    }
}
