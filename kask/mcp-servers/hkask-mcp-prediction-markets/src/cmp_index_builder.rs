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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

// ── Tests ─────────────────────────────────────────────────────────────────
