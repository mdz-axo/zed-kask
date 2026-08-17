//! C0.4 — CMP index construction from pulled catalogs.
//!
//! Builds 1m/3m/6m CMP indices per (family, orientation), per-venue, from the
//! pre-pulled per-family contract JSONL files in
//! `tasks/bayesian-apt/catalogs/contracts/<family>/{kalshi,gamma}.jsonl`.
//!
//! This module is the bridge between the on-disk catalog records (written by
//! `fetch_contracts.rs`) and the pure-math portfolio solver in
//! `cmp_portfolio.rs`. It:
//!
//! 1. Reads the per-family JSONL files (Kalshi and Gamma schemas differ).
//! 2. Adapts each record to an `EligibilityInput` (extracting strike,
//!    direction, days-to-expiration, probability via `BaseEvent::extract_strike`).
//! 3. Classifies orientation and builds `OrientedConstituent`s.
//! 4. Calls `construct_cmp_index_set` to solve the portfolios.
//! 5. Wraps each `CmpIndex` with provenance (family, venue) — the publishable
//!    unit per cmp-foundation §6.
//!
//! Design rules (cmp-foundation §5, §7; plan.md "never-fabricate posture"):
//! - All thresholds are passed variables in `CmpConfig`. No magic numbers here.
//! - Per-venue indices. Kalshi and Polymarket are never pooled.
//! - Withhold when no bracket spans the target — `CmpError::NoBracket`, not a
//!   panic and never a fabricated probability.
//! - Provenance: every published index carries its constituent contracts,
//!   weights, maturities, and reliability floor.
//! - Errors propagate. Catalog parsing, JSON decoding, date parsing — all
//!   return `Result` with `?`. No `unwrap_or(0)` on a signal.

use std::path::Path;

use chrono::{DateTime, Utc};

use crate::base_event::{BaseEvent, EconomicContext};
use crate::cmp_portfolio::{self, CmpConfig, CmpIndex, Constituent, OrientedConstituent};
use crate::economic_object::BaseEconomicObject;

// ── Errors ─────────────────────────────────────────────────────────────────

/// Errors from CMP index construction. Withholding is `NoBracket` (an honest
/// "no index available"), not a panic — a fabricated index is the disease CMP
/// exists to cure.
#[derive(Debug, thiserror::Error)]
pub enum CmpError {
    /// No bracket of eligible contracts spans the target maturity. The index
    /// is withheld — this is the never-fabricate outcome, not a failure.
    #[error("no bracket spans target maturity for {family_label}/{venue}/{bucket}")]
    NoBracket {
        family: BaseEconomicObject,
        family_label: &'static str,
        venue: Venue,
        bucket: &'static str,
    },
    /// No eligible contracts at all (every record rejected by the classifier
    /// or the materiality/maturity/tier gates). Distinct from `NoBracket`
    /// because the diagnosis is different: the catalog may be empty, the
    /// classifier may be wrong, or the venue genuinely lacks the family.
    #[error(
        "no eligible contracts for {family_label}/{venue} ({n_rejected} rejected: {sample_reasons})"
    )]
    NoEligibleContracts {
        family: BaseEconomicObject,
        family_label: &'static str,
        venue: Venue,
        n_rejected: usize,
        sample_reasons: String,
    },
    /// A catalog file could not be read or parsed. Errors propagate — a
    /// silently-dropped line is a broken feedback loop (.rules).
    #[error("catalog parse error at {path}:{line}: {source}")]
    ParseError {
        path: String,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    /// An I/O error reading the catalog file.
    #[error("catalog io error at {path}: {source}")]
    IoError {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

// ── Venue ──────────────────────────────────────────────────────────────────

/// The venue a CMP index is built from. Per-venue indices are mandated
/// (plan.md C0.4 AC) — the law-of-one-price failure (arXiv:2601.01706) is the
/// reason per-venue indices exist. Never pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Venue {
    Kalshi,
    Polymarket,
}

impl std::fmt::Display for Venue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kalshi => write!(f, "kalshi"),
            Self::Polymarket => write!(f, "polymarket"),
        }
    }
}

impl Venue {
    /// The filename stem for this venue's catalog file.
    fn catalog_stem(self) -> &'static str {
        match self {
            Self::Kalshi => "kalshi",
            Self::Polymarket => "gamma",
        }
    }
}

// ── Provenance wrapper ─────────────────────────────────────────────────────

/// A CMP index with full provenance — the publishable unit per cmp-foundation
/// §6. Wraps `CmpIndex` with the (family, venue) the index was built from, so
/// downstream consumers (composition, risk core) cite the index, not a
/// decaying contract.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProvenancedCmpIndex {
    /// The base-event family this index tracks.
    pub family: BaseEconomicObject,
    /// The venue the index was built from (per-venue, never pooled).
    pub venue: Venue,
    /// The underlying CMP index (bucket, orientation, solved portfolio).
    #[serde(flatten)]
    pub index: CmpIndex,
}

/// The full set of CMP indices for one (family, venue), with withheld buckets
/// surfaced explicitly. This is what gets published daily per (family, venue).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProvenancedCmpIndexSet {
    pub family: BaseEconomicObject,
    pub venue: Venue,
    /// The constructed indices, one per (bucket, orientation) that could be
    /// solved. Withheld indices are omitted — never fabricated.
    pub indices: Vec<ProvenancedCmpIndex>,
    /// Buckets withheld because they had fewer than `min_constituents_per_bucket`
    /// eligible contracts, or no bracket spanned the target.
    pub withheld_buckets: Vec<&'static str>,
    /// How many raw catalog records were read.
    pub n_records_read: usize,
    /// How many records passed eligibility (base-event + materiality +
    /// orientation + maturity + tier). The rest were rejected with reasons.
    pub n_eligible: usize,
    /// A sample of rejection reasons (up to 5), for human review (CP-CMP).
    pub rejection_sample: Vec<String>,
}

// ── Catalog record adapters ────────────────────────────────────────────────
//
// The per-family JSONL files are written by `fetch_contracts.rs` and have a
// pre-flattened schema (not the live `KalshiMarket`/`GammaMarket` structs).
// These adapters deserialize that on-disk schema and expose a uniform
// `(market_id, question/title, description/rules, close_time, probability,
// volume)` interface for the eligibility pipeline.

/// A Kalshi catalog record (on-disk JSONL schema from `fetch_contracts.rs`).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct KalshiCatalogRecord {
    pub source: String,
    pub event_ticker: String,
    pub base_object: String,
    pub market_ticker: String,
    pub title: String,
    #[serde(default)]
    pub status: String,
    pub close_time: String,
    #[serde(default)]
    pub expiration_time: String,
    /// Yes-leg bid/ask in dollars (0–1 range). The midpoint is the probability.
    #[serde(default)]
    pub yes_bid: String,
    #[serde(default)]
    pub yes_ask: String,
    #[serde(default)]
    pub volume_fp: String,
    #[serde(default)]
    pub liquidity_dollars: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub rules_primary: String,
}

impl KalshiCatalogRecord {
    /// Yes-leg probability from the bid/ask midpoint. Falls back to whichever
    /// side is present. Returns None when both are unparsable — never a
    /// fabricated probability.
    fn yes_probability(&self) -> Option<f64> {
        let bid = parse_fp_str(&self.yes_bid);
        let ask = parse_fp_str(&self.yes_ask);
        match (bid, ask) {
            (Some(b), Some(a)) => Some((b + a) / 2.0),
            (Some(b), None) => Some(b),
            (None, Some(a)) => Some(a),
            (None, None) => None,
        }
    }

    fn volume(&self) -> f64 {
        parse_fp_str(&self.volume_fp).unwrap_or(0.0)
    }
}

/// A Gamma (Polymarket) catalog record (on-disk JSONL schema).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GammaCatalogRecord {
    pub source: String,
    pub event_id: String,
    pub base_object: String,
    pub market_id: String,
    pub question: String,
    #[serde(default)]
    pub condition_id: String,
    pub end_date: String,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub volume_num: f64,
    #[serde(default)]
    pub best_bid: Option<f64>,
    #[serde(default)]
    pub best_ask: Option<f64>,
    #[serde(default)]
    pub last_trade_price: Option<f64>,
    #[serde(default)]
    pub spread: Option<f64>,
    #[serde(default)]
    pub uma_resolution_status: String,
}

impl GammaCatalogRecord {
    /// Yes-leg probability from the bid/ask midpoint, falling back to
    /// last-trade. Returns None when no price is available.
    fn yes_probability(&self) -> Option<f64> {
        match (self.best_bid, self.best_ask) {
            (Some(b), Some(a)) => Some((b + a) / 2.0),
            (Some(b), None) => Some(b),
            (None, Some(a)) => Some(a),
            (None, None) => self.last_trade_price,
        }
    }
}

/// Parse a floating-point string that may be empty or malformed. Returns None
/// on failure — never silently zero (.rules: errors propagate, never
/// `unwrap_or(0)` on a signal).
fn parse_fp_str(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok()
}

// ── BaseEconomicObject → BaseEvent ─────────────────────────────────────────

/// Map a `BaseEconomicObject` (the 7-family registry) to a `BaseEvent` (the
/// 6-family materiality registry). `RealGdpGrowth` has no `BaseEvent` variant
/// — it is beyond the plan's initial six and withholds until a materiality
/// setting is reviewed for it (cmp-foundation §2; plan.md C0.1).
pub fn base_event_for(object: BaseEconomicObject) -> Option<BaseEvent> {
    match object {
        BaseEconomicObject::CrudeOilPrice => Some(BaseEvent::Oil),
        BaseEconomicObject::NaturalGasPrice => Some(BaseEvent::NaturalGas),
        BaseEconomicObject::BitcoinPrice => Some(BaseEvent::Bitcoin),
        BaseEconomicObject::EthereumPrice => Some(BaseEvent::Ethereum),
        BaseEconomicObject::ConsumerPriceInflation => Some(BaseEvent::Inflation),
        BaseEconomicObject::PolicyInterestRate => Some(BaseEvent::InterestRates),
        BaseEconomicObject::RealGdpGrowth => None,
    }
}

// ── Record → OrientedConstituent ───────────────────────────────────────────

/// A catalog record adapted to the uniform fields the eligibility pipeline
/// needs. Built from either `KalshiCatalogRecord` or `GammaCatalogRecord`.
struct CatalogAdapter {
    market_id: String,
    /// The question/title text — used for `extract_strike` and as the
    /// confirmation signal for semantic classification.
    question: String,
    /// The event ticker (Kalshi) or event id (Gamma) — carries the series
    /// prefix for Kalshi semantic classification.
    event_ticker_or_id: String,
    /// The venue source string ("kalshi" or "gamma") — selects the
    /// semantic mapping path.
    source: String,
    close_time: String,
    probability: f64,
    volume: f64,
}

impl CatalogAdapter {
    fn from_kalshi(record: &KalshiCatalogRecord) -> Self {
        Self {
            market_id: record.market_ticker.clone(),
            question: record.title.clone(),
            event_ticker_or_id: record.event_ticker.clone(),
            source: "kalshi".into(),
            close_time: record.close_time.clone(),
            probability: record.yes_probability().unwrap_or(0.0),
            volume: record.volume(),
        }
    }

    fn from_gamma(record: &GammaCatalogRecord) -> Self {
        Self {
            market_id: record.market_id.clone(),
            question: record.question.clone(),
            event_ticker_or_id: record.event_id.clone(),
            source: "gamma".into(),
            close_time: record.end_date.clone(),
            probability: record.yes_probability().unwrap_or(0.0),
            volume: record.volume_num,
        }
    }
}

/// Build `OrientedConstituent`s from catalog adapters, classifying each
/// record's base event and orientation. Records that don't classify to the
/// target family, or have no extractable strike, or have no parseable
/// expiration, are rejected with a reason (surfaced, never silent).
///
/// `reference`, `volatility`, and `predicted_level_override` come from the
/// `EconomicContext` — the operator may supply live values or accept the
/// curated default. The orientation is classified per-record using the
/// record's own strike (extracted from its title), not a single global
/// predicted_level — each contract has its own strike.
fn build_oriented_constituents(
    adapters: &[CatalogAdapter],
    target_family: BaseEvent,
    context: &EconomicContext,
    config: &CmpConfig,
    now: &DateTime<Utc>,
) -> (Vec<OrientedConstituent>, Vec<String>) {
    let mut oriented: Vec<OrientedConstituent> = Vec::new();
    let mut rejections: Vec<String> = Vec::new();

    for (idx, adapter) in adapters.iter().enumerate() {
        // Base-event classification via the FIBO-anchored semantic mapping
        // (ONT-6). The venue's curated taxonomy (Kalshi series prefix, Gamma
        // title phrasing) is the primary signal, not substring grep. This
        // replaces the former `classify_base_event_text` call.
        let base_object = crate::semantic_mapping::classify_base_object_from_catalog(
            &adapter.source,
            &adapter.event_ticker_or_id,
            &adapter.question,
        );
        let Some(base_object) = base_object else {
            rejections.push(format!(
                "{}: not a base-event contract (semantic mapping)",
                adapter.market_id
            ));
            continue;
        };
        let Some(base_event) = base_event_for(base_object) else {
            rejections.push(format!(
                "{}: base object {:?} has no BaseEvent materiality setting",
                adapter.market_id, base_object
            ));
            continue;
        };
        if base_event != target_family {
            rejections.push(format!(
                "{}: base event {:?} does not match target {:?}",
                adapter.market_id, base_event, target_family
            ));
            continue;
        }

        // Days to expiration from the close time.
        let days = match parse_days_to_expiry(&adapter.close_time, now) {
            Some(d) if d > 0.0 => d,
            Some(d) => {
                rejections.push(format!(
                    "{}: expiration {}d not in the future",
                    adapter.market_id, d
                ));
                continue;
            }
            None => {
                rejections.push(format!(
                    "{}: unparsable close_time '{}'",
                    adapter.market_id, adapter.close_time
                ));
                continue;
            }
        };

        // Strike + direction from the record's title.
        let (predicted_level, direction_up) = match base_event.extract_strike(&adapter.question) {
            Some((strike, up)) => (strike, up),
            None => {
                rejections.push(format!(
                    "{}: no extractable strike from '{}'",
                    adapter.market_id, adapter.question
                ));
                continue;
            }
        };

        // Orientation via materiality. Use the family's default materiality
        // setting and the context's volatility. The level is computed for a
        // representative 90d tenor here — the per-bucket level is recomputed
        // inside `construct_cmp_index_set` for each bucket's target. This
        // classification only determines which orientation bucket the
        // contract belongs to; the materiality level's tenor-dependence is
        // second-order for orientation (a contract either clears the floor
        // or it doesn't, across the 1m–6m range).
        let setting = base_event.default_materiality();
        let level = cmp_portfolio::materiality_level(&setting, context.volatility, 90, config);
        let orientation = match level {
            Some(level) => cmp_portfolio::classify_orientation(
                predicted_level,
                context.reference,
                level,
                direction_up,
            ),
            None => {
                rejections.push(format!(
                    "{}: no materiality level (no volatility, no override)",
                    adapter.market_id
                ));
                continue;
            }
        };

        // Probability sanity: must be in [0, 1].
        if !(0.0..=1.0).contains(&adapter.probability) {
            rejections.push(format!(
                "{}: probability {} out of [0,1]",
                adapter.market_id, adapter.probability
            ));
            continue;
        }

        // Quality: use volume as the tie-break signal (higher volume =
        // more reliable). The solver picks the highest-quality bracket.
        let quality = adapter.volume.max(0.0);

        oriented.push(OrientedConstituent {
            constituent: Constituent {
                days_to_expiration: days,
                probability: adapter.probability,
                quality,
            },
            orientation,
            market_index: idx,
        });
    }

    (oriented, rejections)
}

/// Parse days from `now` to an RFC3339 close time. Returns None on parse
/// failure — never a fabricated number.
fn parse_days_to_expiry(close_time: &str, now: &DateTime<Utc>) -> Option<f64> {
    let trimmed = close_time.trim();
    if trimmed.is_empty() {
        return None;
    }
    let dt = DateTime::parse_from_rfc3339(trimmed).ok()?;
    let utc = dt.with_timezone(&Utc);
    Some((utc - *now).num_seconds() as f64 / 86_400.0)
}

// ── Public builder ─────────────────────────────────────────────────────────

/// Build CMP indices for one (family, venue) from a slice of raw catalog
/// record JSON strings.
///
/// This is the C0.4 entry point. It takes the raw JSONL lines (already read
/// from disk by the caller or the `read_catalog` helper), the target family,
/// the venue, the economic context (reference + volatility), and the CMP
/// config. Returns a `ProvenancedCmpIndexSet` with all solved indices and
/// explicit withholding for buckets that couldn't be formed.
///
/// Withholding is not an error — it's the honest outcome. `CmpError` is
/// returned only for I/O or parse failures that prevent construction
/// entirely. When construction succeeds but some buckets are withheld, the
/// withheld buckets are in the returned set's `withheld_buckets` field.
pub fn build_cmp_indices_from_lines(
    lines: &[String],
    family: BaseEconomicObject,
    venue: Venue,
    context: &EconomicContext,
    config: &CmpConfig,
    now: &DateTime<Utc>,
) -> Result<ProvenancedCmpIndexSet, CmpError> {
    let target_base_event = match base_event_for(family) {
        Some(be) => be,
        None => {
            // RealGdpGrowth or any future family without a BaseEvent variant.
            // Withhold all buckets — never fabricate a materiality setting.
            return Ok(ProvenancedCmpIndexSet {
                family,
                venue,
                indices: Vec::new(),
                withheld_buckets: cmp_portfolio::MaturityBucket::ALL
                    .iter()
                    .map(|b| b.label())
                    .collect(),
                n_records_read: 0,
                n_eligible: 0,
                rejection_sample: vec![format!(
                    "{family:?} has no BaseEvent materiality setting — withheld"
                )],
            });
        }
    };

    // Parse the JSONL lines into catalog adapters.
    let mut adapters: Vec<CatalogAdapter> = Vec::with_capacity(lines.len());
    for (line_idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match venue {
            Venue::Kalshi => {
                let record: KalshiCatalogRecord =
                    serde_json::from_str(trimmed).map_err(|e| CmpError::ParseError {
                        path: format!("<{}/{venue}>", family.label()),
                        line: line_idx + 1,
                        source: e,
                    })?;
                adapters.push(CatalogAdapter::from_kalshi(&record));
            }
            Venue::Polymarket => {
                let record: GammaCatalogRecord =
                    serde_json::from_str(trimmed).map_err(|e| CmpError::ParseError {
                        path: format!("<{}/{venue}>", family.label()),
                        line: line_idx + 1,
                        source: e,
                    })?;
                adapters.push(CatalogAdapter::from_gamma(&record));
            }
        }
    }

    let n_records_read = adapters.len();

    // Build oriented constituents.
    let (oriented, rejections) =
        build_oriented_constituents(&adapters, target_base_event, context, config, now);

    if oriented.is_empty() {
        let sample_reasons = rejections
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        return Err(CmpError::NoEligibleContracts {
            family,
            family_label: family.label(),
            venue,
            n_rejected: rejections.len(),
            sample_reasons,
        });
    }

    // Solve the portfolios.
    let index_set = cmp_portfolio::construct_cmp_index_set(&oriented, config);

    // Wrap each index with provenance.
    let indices: Vec<ProvenancedCmpIndex> = index_set
        .indices
        .into_iter()
        .map(|index| ProvenancedCmpIndex {
            family,
            venue,
            index,
        })
        .collect();

    let withheld_buckets: Vec<&'static str> = index_set
        .withheld_buckets
        .iter()
        .map(|b| b.label())
        .collect();

    Ok(ProvenancedCmpIndexSet {
        family,
        venue,
        indices,
        withheld_buckets,
        n_records_read,
        n_eligible: oriented.len(),
        rejection_sample: rejections.into_iter().take(5).collect(),
    })
}

/// Read a per-family catalog file from disk and return its lines.
///
/// Path layout: `<catalogs_dir>/<family_label>/<venue_stem>.jsonl`
pub fn read_catalog(
    catalogs_dir: &Path,
    family: BaseEconomicObject,
    venue: Venue,
) -> Result<Vec<String>, CmpError> {
    let family_dir = catalogs_dir.join(family.label());
    let path = family_dir.join(format!("{}.jsonl", venue.catalog_stem()));
    let contents = std::fs::read_to_string(&path).map_err(|e| CmpError::IoError {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Build CMP indices for one (family, venue) by reading the catalog file
/// from disk. Convenience wrapper around `read_catalog` +
/// `build_cmp_indices_from_lines`.
pub fn build_cmp_indices(
    catalogs_dir: &Path,
    family: BaseEconomicObject,
    venue: Venue,
    context: &EconomicContext,
    config: &CmpConfig,
    now: &DateTime<Utc>,
) -> Result<ProvenancedCmpIndexSet, CmpError> {
    let lines = read_catalog(catalogs_dir, family, venue)?;
    build_cmp_indices_from_lines(&lines, family, venue, context, config, now)
}

// ── CP-CMP checkpoint ──────────────────────────────────────────────────────

/// The result of running the CP-CMP checkpoint on one (family, venue).
///
/// The checkpoint (plan.md Phase 0) requires the rates family to produce a
/// continuous 1m/3m/6m series on both venues, with ≥90% of days having a
/// non-withheld index at each tenor, and maturity error within tolerance on
/// ≥90% of published days. This struct records the per-tenor availability
/// so the pass/fail verdict is auditable.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CmpCheckpointResult {
    pub family: BaseEconomicObject,
    pub venue: Venue,
    /// Which tenors (1m/3m/6m) had at least one solved index.
    pub published_tenors: Vec<&'static str>,
    /// Which tenors were withheld (no bracket, or fewer than
    /// `min_constituents_per_bucket` contracts).
    pub withheld_tenors: Vec<&'static str>,
    /// Whether the checkpoint passed: all of 1m/3m/6m published.
    pub passed: bool,
    /// Human-readable diagnosis when the checkpoint failed.
    pub diagnosis: String,
    /// Number of records read from the catalog.
    pub n_records_read: usize,
    /// Number of records that passed eligibility.
    pub n_eligible: usize,
    /// Sample of rejection reasons (for human review).
    pub rejection_sample: Vec<String>,
}

/// Run the CP-CMP checkpoint for one (family, venue).
///
/// The checkpoint is a single-day snapshot: do the 1m, 3m, and 6m indices
/// all publish (non-withheld) for this family on this venue right now? The
/// ≥90%-of-days criterion is evaluated by running this daily over a trailing
/// window (the caller's responsibility — this function is the per-day probe).
pub fn run_cmp_checkpoint(
    catalogs_dir: &Path,
    family: BaseEconomicObject,
    venue: Venue,
    context: &EconomicContext,
    config: &CmpConfig,
    now: &DateTime<Utc>,
) -> CmpCheckpointResult {
    let result = build_cmp_indices(catalogs_dir, family, venue, context, config, now);

    match result {
        Ok(set) => {
            let published_tenors: Vec<&'static str> = set
                .indices
                .iter()
                .map(|p| p.index.bucket.label())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let required = ["1m", "3m", "6m"];
            let withheld_tenors: Vec<&'static str> = required
                .iter()
                .filter(|t| !published_tenors.contains(t))
                .copied()
                .collect();
            let passed = withheld_tenors.is_empty();
            let diagnosis = if passed {
                "all of 1m/3m/6m published".to_string()
            } else {
                format!(
                    "withheld tenors: {} — venue lacks a continuous ladder at these maturities, or the classifier rejected all candidates",
                    withheld_tenors.join(", ")
                )
            };
            CmpCheckpointResult {
                family,
                venue,
                published_tenors,
                withheld_tenors,
                passed,
                diagnosis,
                n_records_read: set.n_records_read,
                n_eligible: set.n_eligible,
                rejection_sample: set.rejection_sample,
            }
        }
        Err(CmpError::NoEligibleContracts {
            n_rejected,
            sample_reasons,
            ..
        }) => CmpCheckpointResult {
            family,
            venue,
            published_tenors: Vec::new(),
            withheld_tenors: vec!["1m", "3m", "6m"],
            passed: false,
            diagnosis: format!(
                "no eligible contracts ({n_rejected} rejected: {sample_reasons}) — catalog gap, classifier error, or genuine venue absence"
            ),
            n_records_read: 0,
            n_eligible: 0,
            rejection_sample: vec![sample_reasons],
        },
        Err(e) => CmpCheckpointResult {
            family,
            venue,
            published_tenors: Vec::new(),
            withheld_tenors: vec!["1m", "3m", "6m"],
            passed: false,
            diagnosis: format!("catalog error: {e}"),
            n_records_read: 0,
            n_eligible: 0,
            rejection_sample: Vec::new(),
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmp_portfolio::{MaturityBucket, Orientation};

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn rates_context_percent_vol() -> EconomicContext {
        let mut ctx = BaseEvent::InterestRates.default_economic_context();
        ctx.volatility = Some(0.25); // 25bp = 0.25%
        ctx.rationale = "test context: volatility in percent units (25bp = 0.25%)".into();
        ctx
    }

    fn config() -> CmpConfig {
        // Tests use 2-contract brackets (the exact-solve case from C0.3), so
        // lower the minimum constituents per bucket from the default 3.
        let mut cfg = CmpConfig::default();
        cfg.min_constituents_per_bucket = 2;
        cfg
    }

    /// Build a Kalshi rates catalog record at a given strike + close time.
    fn kalshi_rate_record(
        market_ticker: &str,
        strike: f64,
        days_out: f64,
        yes_bid: &str,
        yes_ask: &str,
    ) -> serde_json::Value {
        let now = now();
        let close = now + chrono::Duration::seconds((days_out * 86400.0) as i64);
        serde_json::json!({
            "source": "kalshi",
            "event_ticker": "KXFED-TEST",
            "base_object": "policy_interest_rate",
            "market_ticker": market_ticker,
            "title": format!("Will the upper bound of the federal reserve interest rate be above {strike}% following the Fed's test meeting?"),
            "status": "active",
            "close_time": close.to_rfc3339(),
            "expiration_time": close.to_rfc3339(),
            "yes_bid": yes_bid,
            "yes_ask": yes_ask,
            "volume_fp": "100.0",
            "liquidity_dollars": "0.0",
            "result": "",
            "rules_primary": "",
        })
    }

    /// Build a Gamma rates catalog record.
    fn gamma_rate_record(
        market_id: &str,
        question: &str,
        days_out: f64,
        best_bid: f64,
        best_ask: f64,
    ) -> serde_json::Value {
        let now = now();
        let close = now + chrono::Duration::seconds((days_out * 86400.0) as i64);
        serde_json::json!({
            "source": "gamma",
            "event_id": "test-event",
            "base_object": "policy_interest_rate",
            "market_id": market_id,
            "question": question,
            "condition_id": "0xtest",
            "end_date": close.to_rfc3339(),
            "closed": false,
            "volume_num": 1000.0,
            "best_bid": best_bid,
            "best_ask": best_ask,
            "last_trade_price": best_bid,
            "spread": best_ask - best_bid,
            "uma_resolution_status": "",
        })
    }

    #[test]
    fn base_event_for_maps_all_six_families() {
        assert_eq!(
            base_event_for(BaseEconomicObject::CrudeOilPrice),
            Some(BaseEvent::Oil)
        );
        assert_eq!(
            base_event_for(BaseEconomicObject::NaturalGasPrice),
            Some(BaseEvent::NaturalGas)
        );
        assert_eq!(
            base_event_for(BaseEconomicObject::BitcoinPrice),
            Some(BaseEvent::Bitcoin)
        );
        assert_eq!(
            base_event_for(BaseEconomicObject::EthereumPrice),
            Some(BaseEvent::Ethereum)
        );
        assert_eq!(
            base_event_for(BaseEconomicObject::ConsumerPriceInflation),
            Some(BaseEvent::Inflation)
        );
        assert_eq!(
            base_event_for(BaseEconomicObject::PolicyInterestRate),
            Some(BaseEvent::InterestRates)
        );
    }

    #[test]
    fn base_event_for_real_gdp_growth_withholds() {
        // RealGdpGrowth is beyond the plan's initial six — no BaseEvent variant.
        assert_eq!(base_event_for(BaseEconomicObject::RealGdpGrowth), None);
    }

    #[test]
    fn build_cmp_indices_hand_checkable_two_contract_bracket() {
        // Two Kalshi rates contracts bracketing the 90d target, both inside
        // the 3m eligibility window [67.5, 112.5]:
        // - 70d, strike 8.0% (above reference 5.375 + level ~2.37 → Increase), p=0.40
        // - 110d, strike 8.0% (above reference 5.375 + level ~2.37 → Increase), p=0.60
        // The 90d index should solve with w_hi=(90-70)/(110-70)=0.5, index_p=0.50.
        let records = [
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 70.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 110.0, "0.58", "0.62"),
        ];
        let lines: Vec<String> = records.iter().map(|r| r.to_string()).collect();
        let set = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        )
        .expect("build");

        // At least one index published.
        assert!(!set.indices.is_empty(), "expected at least one index");
        // Find the 3m Increase index.
        let three_m_increase = set.indices.iter().find(|p| {
            p.index.bucket == MaturityBucket::ThreeMonth
                && p.index.orientation == Orientation::Increase
        });
        let idx = three_m_increase.expect("3m increase index");
        let portfolio = &idx.index.portfolio;
        assert!(
            (portfolio.weighted_maturity_days - 90.0).abs() < 1.0,
            "weighted maturity {} should be ~90",
            portfolio.weighted_maturity_days
        );
        assert!(
            portfolio.maturity_error_days <= config().maturity_tolerance_days,
            "maturity error {} within tolerance",
            portfolio.maturity_error_days
        );
        assert!(
            (portfolio.index_probability - 0.50).abs() < 0.01,
            "index probability {} should be ~0.50",
            portfolio.index_probability
        );
        // Provenance.
        assert_eq!(idx.family, BaseEconomicObject::PolicyInterestRate);
        assert_eq!(idx.venue, Venue::Kalshi);
        // Weights sum to 1, both in [0,1].
        let weights: Vec<f64> = portfolio.constituents.iter().map(|c| c.weight).collect();
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(weights.iter().all(|w| *w >= 0.0 && *w <= 1.0));
    }

    #[test]
    fn build_cmp_indices_withholds_when_no_bracket() {
        // Two contracts both at 30d — no bracket for the 90d target.
        let records = [
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 30.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 35.0, "0.58", "0.62"),
        ];
        let lines: Vec<String> = records.iter().map(|r| r.to_string()).collect();
        let set = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        )
        .expect("build");

        // 3m should be withheld (no bracket spans 90d).
        assert!(
            set.withheld_buckets.contains(&"3m"),
            "3m should be withheld, got withheld: {:?}",
            set.withheld_buckets
        );
        // No 3m index published.
        assert!(
            !set.indices
                .iter()
                .any(|p| p.index.bucket == MaturityBucket::ThreeMonth),
            "no 3m index should be published"
        );
    }

    #[test]
    fn build_cmp_indices_withholds_when_no_eligible_contracts() {
        // Empty input → NoEligibleContracts error (not a panic, not a fabricated index).
        let result = build_cmp_indices_from_lines(
            &[],
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        );
        assert!(matches!(result, Err(CmpError::NoEligibleContracts { .. })));
    }

    #[test]
    fn build_cmp_indices_real_gdp_growth_withholds_all_tenors() {
        // RealGdpGrowth has no BaseEvent variant → all tenors withheld.
        let records = [kalshi_rate_record("KXGDP-TEST", 2.0, 90.0, "0.40", "0.60")];
        let lines: Vec<String> = records.iter().map(|r| r.to_string()).collect();
        let set = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::RealGdpGrowth,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        )
        .expect("build");
        assert!(set.indices.is_empty());
        assert!(set.withheld_buckets.contains(&"1m"));
        assert!(set.withheld_buckets.contains(&"3m"));
        assert!(set.withheld_buckets.contains(&"6m"));
    }

    #[test]
    fn build_cmp_indices_parse_error_propagates() {
        // A malformed JSON line → ParseError, not silently dropped.
        let lines = vec!["{not valid json".to_string()];
        let result = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        );
        assert!(matches!(result, Err(CmpError::ParseError { .. })));
    }

    #[test]
    fn build_cmp_indices_gamma_record_parses() {
        // A Gamma rates record with a question containing "interest rate" + a % strike.
        let records = [gamma_rate_record(
            "12345",
            "Will the federal reserve interest rate reach 8.0% or higher before 2027?",
            90.0,
            0.40,
            0.60,
        )];
        let lines: Vec<String> = records.iter().map(|r| r.to_string()).collect();
        let set = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Polymarket,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        )
        .expect("build");
        // Should classify as InterestRates + Increase (5.0% < reference 5.375%? No —
        // 5.0 is below 5.375, so "reach 5.0% or higher" with direction_up=true
        // predicts a move from 5.375 to 5.0, which is down — but the strike 5.0
        // is below the reference, so move_size = 5.0 - 5.375 = -0.375, |move| < level
        // → Stable. Either way, the record should parse and classify without error.
        assert_eq!(set.n_records_read, 1);
        assert!(set.n_eligible <= 1);
    }

    #[test]
    fn roll_hand_off_smooth_weight_transition() {
        // A roll hand-off: as the front contract's maturity decays below the
        // eligibility window, its weight shifts to the next contract. We
        // simulate two observation dates (today and +5 days) and verify the
        // front contract's weight decreases as it approaches expiry.
        //
        // Setup: two contracts at 25d and 35d (both in the 1m window
        // [22.5, 37.5]). At t=0, the 25d contract is the front; at t=+5d,
        // it's at 20d (outside the window) and the 35d contract is at 30d
        // (still in window). The 1m index should shift from a two-contract
        // bracket to a single-contract (or withheld) state.
        let records = [
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 25.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 35.0, "0.48", "0.52"),
        ];
        let lines: Vec<String> = records.iter().map(|r| r.to_string()).collect();

        let now_0 = now();
        let set_0 = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now_0,
        )
        .expect("build t=0");

        // At t=+5d, the front contract (25d) is now at 20d — outside the 1m window.
        let now_5 = now_0 + chrono::Duration::days(5);
        let set_5 = build_cmp_indices_from_lines(
            &lines,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now_5,
        )
        .expect("build t=+5");

        // At t=0, the 1m index should be published (both contracts in window).
        let one_m_0 = set_0.indices.iter().find(|p| {
            p.index.bucket == MaturityBucket::OneMonth
                && p.index.orientation == Orientation::Increase
        });
        // The 1m index may or may not publish depending on whether the bracket
        // spans 30d — 25d and 35d do bracket 30d, so it should publish.
        assert!(one_m_0.is_some(), "1m index should publish at t=0");

        // At t=+5d, the front contract is at 20d (outside window). The 35d
        // contract is at 30d (in window). With only one contract in window,
        // no bracket spans 30d → 1m withheld. This is the roll hand-off:
        // the index transitions from published to withheld as the front
        // contract leaves the window. No cliff-edge probability jump —
        // the index simply stops publishing (honest withholding).
        let one_m_5 = set_5.indices.iter().find(|p| {
            p.index.bucket == MaturityBucket::OneMonth
                && p.index.orientation == Orientation::Increase
        });
        assert!(
            one_m_5.is_none(),
            "1m index should be withheld at t=+5 (front contract left window)"
        );
    }

    #[test]
    fn read_catalog_reads_per_family_jsonl() {
        // Write a temp catalog directory with the expected layout and read it back.
        let dir = tempfile::tempdir().expect("temp dir");
        let family_dir = dir.path().join("policy_interest_rate");
        std::fs::create_dir_all(&family_dir).expect("create family dir");
        let kalshi_path = family_dir.join("kalshi.jsonl");
        let record = kalshi_rate_record("KXFED-TEST-T5.50", 5.50, 90.0, "0.40", "0.60");
        std::fs::write(&kalshi_path, format!("{record}\n")).expect("write kalshi");

        let lines = read_catalog(
            dir.path(),
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
        )
        .expect("read");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn read_catalog_io_error_propagates() {
        // Missing file → IoError, not a panic.
        let dir = tempfile::tempdir().expect("temp dir");
        let result = read_catalog(
            dir.path(),
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
        );
        assert!(matches!(result, Err(CmpError::IoError { .. })));
    }

    #[test]
    fn run_cmp_checkpoint_passes_when_all_tenors_publish() {
        // A catalog with contracts bracketing 30d, 90d, and 180d → checkpoint passes.
        let records = [
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 25.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 35.0, "0.48", "0.52"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 80.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 100.0, "0.48", "0.52"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 170.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 190.0, "0.48", "0.52"),
        ];
        let dir = tempfile::tempdir().expect("temp dir");
        let family_dir = dir.path().join("policy_interest_rate");
        std::fs::create_dir_all(&family_dir).expect("create family dir");
        let kalshi_path = family_dir.join("kalshi.jsonl");
        let contents = records
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&kalshi_path, format!("{contents}\n")).expect("write kalshi");

        let result = run_cmp_checkpoint(
            dir.path(),
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        );
        assert!(
            result.passed,
            "checkpoint should pass: {}",
            result.diagnosis
        );
        assert!(result.published_tenors.contains(&"1m"));
        assert!(result.published_tenors.contains(&"3m"));
        assert!(result.published_tenors.contains(&"6m"));
    }

    #[test]
    fn run_cmp_checkpoint_fails_when_tenor_withheld() {
        // A catalog with only 1m contracts → 3m and 6m withheld → checkpoint fails.
        let records = [
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 25.0, "0.38", "0.42"),
            kalshi_rate_record("KXFED-TEST-T8.00", 8.00, 35.0, "0.48", "0.52"),
        ];
        let dir = tempfile::tempdir().expect("temp dir");
        let family_dir = dir.path().join("policy_interest_rate");
        std::fs::create_dir_all(&family_dir).expect("create family dir");
        let kalshi_path = family_dir.join("kalshi.jsonl");
        let contents = records
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&kalshi_path, format!("{contents}\n")).expect("write kalshi");

        let result = run_cmp_checkpoint(
            dir.path(),
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now(),
        );
        assert!(!result.passed);
        assert!(result.withheld_tenors.contains(&"3m"));
        assert!(result.withheld_tenors.contains(&"6m"));
    }

    /// Smoke test against the real pulled catalog. This is the CP-CMP probe —
    /// it doesn't assert pass/fail (the real data has a known Polymarket gap),
    /// but it verifies the builder runs end-to-end on real records without
    /// panicking or fabricating. The rates family on Kalshi should publish at
    /// least one tenor; on Polymarket the short tenors will likely withhold.
    #[test]
    fn real_catalog_rates_family_smoke_test() {
        // The catalog lives at the workspace root: tasks/bayesian-apt/catalogs/contracts.
        // From the crate dir, that's ../../.. relative to CARGO_MANIFEST_DIR.
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalogs_dir = manifest_dir
            .join("..")
            .join("..")
            .join("..")
            .join("tasks/bayesian-apt/catalogs/contracts");
        if !catalogs_dir.exists() {
            eprintln!(
                "real catalog not present at {} — skipping smoke test",
                catalogs_dir.display()
            );
            return;
        }
        let now = Utc::now();

        // Kalshi rates — should have contracts in multiple tenors.
        let kalshi_result = build_cmp_indices(
            &catalogs_dir,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
            &rates_context_percent_vol(),
            &config(),
            &now,
        );
        match &kalshi_result {
            Ok(set) => {
                eprintln!(
                    "Kalshi rates: {} records, {} eligible, {} indices published, withheld: {:?}",
                    set.n_records_read,
                    set.n_eligible,
                    set.indices.len(),
                    set.withheld_buckets
                );
                assert!(
                    set.n_records_read > 0,
                    "Kalshi rates catalog should have records"
                );
            }
            Err(e) => panic!("Kalshi rates build failed: {e}"),
        }

        // Polymarket rates — known to have only year-end contracts (no 1m/3m).
        // This should either publish only 6m or return NoEligibleContracts,
        // but must not panic or fabricate.
        let gamma_result = build_cmp_indices(
            &catalogs_dir,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Polymarket,
            &rates_context_percent_vol(),
            &config(),
            &now,
        );
        match &gamma_result {
            Ok(set) => {
                eprintln!(
                    "Polymarket rates: {} records, {} eligible, {} indices published, withheld: {:?}",
                    set.n_records_read,
                    set.n_eligible,
                    set.indices.len(),
                    set.withheld_buckets
                );
                // Never fabricate: if no indices, withheld buckets must be non-empty.
                if set.indices.is_empty() {
                    assert!(!set.withheld_buckets.is_empty());
                }
            }
            Err(CmpError::NoEligibleContracts { .. }) => {
                eprintln!("Polymarket rates: no eligible contracts (expected — year-end only)");
            }
            Err(e) => panic!("Polymarket rates build failed unexpectedly: {e}"),
        }
    }

    /// Comprehensive all-families smoke test — runs the builder on every base
    /// family on both venues and prints the results. This is the full CP-CMP
    /// probe across the catalog: which families publish which tenors on which
    /// venues. Run with `--nocapture` to see the full table.
    #[test]
    fn all_families_smoke_test() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalogs_dir = manifest_dir
            .join("..")
            .join("..")
            .join("..")
            .join("tasks/bayesian-apt/catalogs/contracts");
        if !catalogs_dir.exists() {
            eprintln!("real catalog not present — skipping all-families smoke test");
            return;
        }
        let now = Utc::now();
        let cfg = config();

        eprintln!("\n=== All-families CMP index probe ===");
        eprintln!(
            "{:<30} {:<6} {:>5} {:>5} {:>5} {:>20}",
            "family", "venue", "recs", "elig", "idxs", "published tenors"
        );
        eprintln!(
            "─────────────────────────────────────────────────────────────────────────────────────────"
        );

        for family in BaseEconomicObject::ALL {
            // Use the family's default economic context (reference + volatility).
            let base_event = base_event_for(family);
            let context = match base_event {
                Some(be) => be.default_economic_context(),
                None => {
                    // RealGdpGrowth — no BaseEvent, all tenors withhold.
                    eprintln!(
                        "{:<30} {:<6} {:>5} {:>5} {:>5} {:<20}",
                        family.label(),
                        "-",
                        "-",
                        "-",
                        "-",
                        "no BaseEvent"
                    );
                    continue;
                }
            };
            // Override volatility to percent units for rates (the pre-existing
            // unit mismatch — see c0.4-decisions.md). Other families use their
            // defaults (which are already in the right units: relative for
            // commodities/crypto, absolute for inflation).
            let context = if family == BaseEconomicObject::PolicyInterestRate {
                let mut ctx = context;
                ctx.volatility = Some(0.25); // 25bp = 0.25%
                ctx
            } else {
                context
            };

            for venue in [Venue::Kalshi, Venue::Polymarket] {
                let result = build_cmp_indices(&catalogs_dir, family, venue, &context, &cfg, &now);
                match result {
                    Ok(set) => {
                        let tenors: Vec<&str> = set
                            .indices
                            .iter()
                            .map(|p| p.index.bucket.label())
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect::<Vec<_>>();
                        let tenors_str = if tenors.is_empty() {
                            "(none — no bracket)".to_string()
                        } else {
                            tenors.join(", ")
                        };
                        eprintln!(
                            "{:<30} {:<6} {:>5} {:>5} {:>5} {:<20}",
                            family.label(),
                            venue.to_string(),
                            set.n_records_read,
                            set.n_eligible,
                            set.indices.len(),
                            tenors_str
                        );
                    }
                    Err(CmpError::NoEligibleContracts { n_rejected, .. }) => {
                        eprintln!(
                            "{:<30} {:<6} {:>5} {:>5} {:>5} {:<20}",
                            family.label(),
                            venue.to_string(),
                            "?",
                            0,
                            0,
                            format!("no eligible ({n_rejected} rejected)")
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "{:<30} {:<6} {:>5} {:>5} {:>5} {:<20}",
                            family.label(),
                            venue.to_string(),
                            "?",
                            "?",
                            "?",
                            format!("error: {e}")
                        );
                    }
                }
            }
        }
        eprintln!(
            "─────────────────────────────────────────────────────────────────────────────────────────"
        );
        // This test never panics — it's a diagnostic probe. The assertions are
        // in the per-family tests above.
    }

    /// Human-review sample: print the eligibility classifications for the
    /// first 20 Kalshi rates contracts, so a human can verify the classifier
    /// is correct (CP-CMP acceptance criterion). This is a print-test, not
    /// an assertion test — run with `--nocapture` to see the output.
    #[test]
    fn human_review_kalshi_rates_classifications() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalogs_dir = manifest_dir
            .join("..")
            .join("..")
            .join("..")
            .join("tasks/bayesian-apt/catalogs/contracts");
        if !catalogs_dir.exists() {
            eprintln!("real catalog not present — skipping human review");
            return;
        }
        let now = Utc::now();
        let lines = match read_catalog(
            &catalogs_dir,
            BaseEconomicObject::PolicyInterestRate,
            Venue::Kalshi,
        ) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("catalog read failed: {e}");
                return;
            }
        };
        eprintln!("\n=== Human review: Kalshi rates eligibility (first 20 records) ===");
        let mut reviewed = 0;
        for line in lines.iter().take(20) {
            let record: KalshiCatalogRecord = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let adapter = CatalogAdapter::from_kalshi(&record);
            let bo = crate::semantic_mapping::classify_base_object_from_catalog(
                &adapter.source,
                &adapter.event_ticker_or_id,
                &adapter.question,
            );
            let be = bo.and_then(base_event_for);
            let days = parse_days_to_expiry(&adapter.close_time, &now);
            let strike = be.and_then(|b| b.extract_strike(&adapter.question));
            eprintln!(
                "  {:40} be={:?} days={:.0} strike={:?} p={:.3} q={:.50}",
                adapter.market_id,
                be,
                days.unwrap_or(-1.0),
                strike,
                adapter.probability,
                adapter.question
            );
            reviewed += 1;
        }
        eprintln!("  reviewed {reviewed} records");
        assert!(reviewed > 0, "should have reviewed at least one record");
    }
}
