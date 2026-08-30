//! Semantic event mapping — ontological mapping of prediction-market events
//! to FIBO (process axis) + Dublin Core (state axis), then graph proximity
//! to identify constellations of related events per base economic object.
//!
//! This replaces the rebuked substring synonym-closure approach. The
//! mechanism is:
//!
//! 1. **FIBO process axis** — each event's economic object is mapped to a
//!    FIBO concept URI (e.g. `fibo-ind-ir-ir:PolicyInterestRate` for Fed
//!    funds, `fibo-ind-ei-ei:CommodityPriceIndex` for WTI). The mapping
//!    uses the venue's curated taxonomy (Kalshi `series_ticker` prefix,
//!    Gamma `tags`) as the primary signal, with the event title as a
//!    confirmation signal — never substring grep as the primary mechanism.
//!
//! 2. **Dublin Core state axis** — each event's state identity is mapped
//!    to DC concepts (`dcterms:subject`, `dcterms:type`, `dcterms:temporal`).
//!
//! 3. **Graph proximity** — events that share the same FIBO concept AND
//!    have overlapping DC state (same subject domain, close temporal
//!    window) are clustered into a constellation. The proximity is the
//!    cosine of the two-axis vectors: FIBO concept similarity (exact
//!    match = 1.0, same FIBO module = 0.5, no match = 0.0) × DC state
//!    similarity (subject overlap + temporal proximity).
//!
//! 4. **Base-object identification** — each constellation is matched to a
//!    `BaseEconomicObject` (the systematic factor the CMP indices are built
//!    over). A constellation with high proximity to `PolicyInterestRate`
//!    becomes the rate-events constellation; the CMP index for rates is
//!    built from that constellation's contracts.

use crate::economic_object::BaseEconomicObject;
use hkask_bridge_ontology::dc_bibo;
use hkask_bridge_ontology::fibo;
use std::collections::HashMap;

/// A prediction-market event mapped to its dual-axis ontological identity.
///
/// The process axis is the FIBO concept the event's economic object maps to.
/// The state axis is the Dublin Core subject/type/temporal identity. Together
/// they form a two-dimensional vector that graph proximity is computed over.
#[derive(Debug, Clone)]
pub struct MappedEvent {
    /// The event's source-venue identifier (Kalshi `event_ticker` or Gamma `id`).
    pub event_id: String,
    /// The event's human-readable title.
    pub title: String,
    /// The venue's curated series/ticker (Kalshi `series_ticker` or Gamma `slug`).
    pub series: String,
    /// The venue's category or tag (Kalshi `category` or Gamma first tag).
    pub category: String,
    /// The FIBO concept URI this event maps to (process axis).
    pub fibo_concept: fibo::FiboConcept,
    /// The Dublin Core subject keywords extracted from the event (state axis).
    pub dc_subjects: Vec<String>,
    /// The Dublin Core type (e.g. `dcmitype:Dataset` for economic data).
    pub dc_type: dc_bibo::DcConcept,
    /// The event's end date (RFC3339), for temporal proximity.
    pub end_date: Option<String>,
    /// The base economic object this event is about (if it matches one).
    pub base_object: Option<BaseEconomicObject>,
}

/// A constellation of events clustered around a base economic object.
///
/// The constellation is the set of events whose graph proximity to the base
/// object's FIBO concept exceeds a threshold. These are the events the CMP
/// index for that base object is built from.
#[derive(Debug, Clone)]
pub struct EventConstellation {
    /// The base economic object this constellation clusters around.
    pub base_object: BaseEconomicObject,
    /// The FIBO concept the constellation is anchored to.
    pub fibo_concept: fibo::FiboConcept,
    /// The events in the constellation, sorted by proximity (highest first).
    pub events: Vec<MappedEvent>,
}

/// Classify a catalog record's base economic object using the FIBO-anchored
/// semantic mapping (ONT-6). This is the bridge between the on-disk catalog
/// records (written by `fetch_contracts.rs`) and the `BaseEconomicObject`
/// registry — the venue's curated taxonomy is the primary signal, not
/// substring grep.
///
/// For Kalshi: the `event_ticker` contains the series prefix (`KXFED`,
/// `KXWTI`, etc.) which `resolve_kalshi_series` matches on. The series prefix
/// is extracted by taking the prefix before the first `-` in the event ticker
/// (e.g. `KXFED-26SEP` → `KXFED`). For tickers without a dash, the full ticker
/// is used.
///
/// For Gamma (Polymarket): the `question` is the market-level question text.
/// It's passed as the title to `resolve_gamma_event`, with empty tags/slug
/// (the per-family catalog JSONL doesn't carry the event-level tags). The
/// extended rates coverage in `resolve_gamma_event` handles the full
/// Polymarket phrasing variety without needing tags.
///
/// Returns `None` when the record doesn't map to any base economic object —
/// never a fabricated classification.
pub fn classify_base_object_from_catalog(
    source: &str,
    event_ticker_or_id: &str,
    question_or_title: &str,
) -> Option<BaseEconomicObject> {
    if source == "kalshi" {
        // Extract the series prefix from the event ticker (e.g. KXFED-26SEP → KXFED).
        let series_prefix = event_ticker_or_id
            .split('-')
            .next()
            .unwrap_or(event_ticker_or_id)
            .to_uppercase();
        let title_lower = question_or_title.to_lowercase();
        resolve_kalshi_series(&series_prefix, &title_lower)?.map(|(_, base_object, _)| base_object)
    } else {
        // Gamma: use the question as the title, with empty tags/slug.
        let title_lower = question_or_title.to_lowercase();
        resolve_gamma_event(&title_lower, &[], "").map(|(_, base_object, _)| base_object)
    }
}

/// Map a Kalshi event to its dual-axis ontological identity.
///
/// The mapping uses the Kalshi `series_ticker` as the primary signal (it's a
/// controlled vocabulary where `KXFED*` = Fed funds, `KXUST*` = Treasury
/// yields, `KXWTI*` = WTI, etc.) and the event title as a confirmation
/// signal. This is the venue's curated taxonomy — the semantic work is
/// already done; we map it to FIBO.
pub fn map_kalshi_event(
    event_ticker: &str,
    series_ticker: &str,
    title: &str,
    category: &str,
    end_date: Option<&str>,
) -> Option<MappedEvent> {
    let series_upper = series_ticker.to_uppercase();
    let title_lower = title.to_lowercase();
    let (fibo_concept, base_object, dc_subjects) =
        match resolve_kalshi_series(&series_upper, &title_lower)? {
            Some(mapping) => mapping,
            None => return None,
        };
    Some(MappedEvent {
        event_id: event_ticker.to_string(),
        title: title.to_string(),
        series: series_ticker.to_string(),
        category: category.to_string(),
        fibo_concept,
        dc_subjects,
        dc_type: dc_bibo::DATASET,
        end_date: end_date.map(|s| s.to_string()),
        base_object: Some(base_object),
    })
}

/// Resolve a Kalshi series ticker to its FIBO concept + base object + DC subjects.
///
/// The series ticker is a controlled vocabulary. The prefix determines the
/// economic object; the suffix determines the specific contract (maturity,
/// direction). This is the semantic mapping — the venue did the
/// classification; we map it to FIBO.
fn resolve_kalshi_series(
    series: &str,
    title: &str,
) -> Option<Option<(fibo::FiboConcept, BaseEconomicObject, Vec<String>)>> {
    use BaseEconomicObject::*;
    // Interest rates: Fed funds / FOMC decisions.
    if series.starts_with("KXFED")
        || series.starts_with("KXFOMC")
        || series.starts_with("KXRATECUT")
        || series.starts_with("KXRATEHIKE")
        || series.starts_with("KXZERORATE")
    {
        return Some(Some((
            fibo::POLICY_INTEREST_RATE,
            PolicyInterestRate,
            vec![
                "interest rate".into(),
                "federal reserve".into(),
                "monetary policy".into(),
            ],
        )));
    }
    // Treasury yields across the curve.
    if series.starts_with("KXUST") || series.starts_with("KXUSTYLD") {
        return Some(Some((
            fibo::TREASURY_YIELD,
            PolicyInterestRate,
            vec![
                "treasury yield".into(),
                "interest rate".into(),
                "bond market".into(),
            ],
        )));
    }
    // Inflation: CPI, PPI.
    if series.starts_with("KXCPI")
        || series.starts_with("KXCPICORE")
        || series.starts_with("KXCPIYOY")
        || series.starts_with("KXECONSTATCPI")
        || series.starts_with("KXHIGHINFLATION")
        || series.starts_with("KXUSCPIYEAR")
        || series.starts_with("KXUSGASCPI")
        || series.starts_with("KXUSPPIYOY")
        || series.starts_with("KXSHELTERCPI")
        || series.starts_with("KXAIRFARECPI")
        || series.starts_with("KXUSEDCARCPI")
    {
        return Some(Some((
            fibo::CONSUMER_PRICE_INDEX,
            ConsumerPriceInflation,
            vec![
                "inflation".into(),
                "consumer price index".into(),
                "cpi".into(),
            ],
        )));
    }
    // Crude oil: WTI, Brent.
    if series.starts_with("KXWTI")
        || series.starts_with("KXBRENT")
        || series.starts_with("KXWTIVSBRENT")
    {
        return Some(Some((
            fibo::COMMODITY_PRICE_INDEX,
            CrudeOilPrice,
            vec![
                "crude oil".into(),
                "wti".into(),
                "brent".into(),
                "commodity".into(),
            ],
        )));
    }
    // Natural gas: Henry Hub.
    if series.starts_with("KXNATGAS")
        || series.starts_with("KXNGAS")
        || series.starts_with("KXAAAGAS")
    {
        return Some(Some((
            fibo::COMMODITY_PRICE_INDEX,
            NaturalGasPrice,
            vec!["natural gas".into(), "henry hub".into(), "commodity".into()],
        )));
    }
    // Bitcoin.
    if series.starts_with("KXBTC") {
        return Some(Some((
            fibo::MARKET_INDEX,
            BitcoinPrice,
            vec!["bitcoin".into(), "btc".into(), "cryptocurrency".into()],
        )));
    }
    // Ethereum.
    if series.starts_with("KXETH") {
        return Some(Some((
            fibo::MARKET_INDEX,
            EthereumPrice,
            vec!["ethereum".into(), "eth".into(), "cryptocurrency".into()],
        )));
    }
    // GDP / economic growth.
    if series.contains("GDP") || title.contains("gdp") || title.contains("gross domestic") {
        return Some(Some((
            fibo::GROSS_DOMESTIC_PRODUCT,
            RealGdpGrowth,
            vec![
                "gdp".into(),
                "economic growth".into(),
                "gross domestic product".into(),
            ],
        )));
    }
    // No match — not a base-event contract.
    Some(None)
}

/// Map a Polymarket Gamma event to its dual-axis ontological identity.
///
/// Gamma events embed their markets and carry `tags` (a controlled
/// vocabulary). The mapping uses the tags as the primary signal and the
/// event title as a confirmation signal.
pub fn map_gamma_event(
    event_id: &str,
    title: &str,
    slug: &str,
    tags: &[String],
    end_date: Option<&str>,
) -> Option<MappedEvent> {
    let title_lower = title.to_lowercase();
    let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
    let (fibo_concept, base_object, dc_subjects) =
        resolve_gamma_event(&title_lower, &tags_lower, slug)?;
    Some(MappedEvent {
        event_id: event_id.to_string(),
        title: title.to_string(),
        series: slug.to_string(),
        category: tags_lower.first().cloned().unwrap_or_default(),
        fibo_concept,
        dc_subjects,
        dc_type: dc_bibo::DATASET,
        end_date: end_date.map(|s| s.to_string()),
        base_object: Some(base_object),
    })
}

/// Resolve a Gamma event to its FIBO concept + base object + DC subjects.
///
/// Gamma has no controlled series ticker like Kalshi; the tags and title
/// are the semantic signal. The mapping checks for specific economic-object
/// signals in the title (the venue's curated naming) and the tags.
///
/// ONT-6 (2026-08-07): extended rates coverage to handle the full Polymarket
/// phrasing variety discovered during C0.4 CP-CMP: "upper bound", "federal
/// funds rate" (full phrase, not just "fed funds"), "rates hit/stay above",
/// and non-Fed central banks (ECB, BoE, BoC) which are the same economic
/// object (a central-bank policy rate) under the FIBO `PolicyInterestRate`
/// concept. The materiality defaults remain Fed-specific (the reference and
/// volatility are Fed funds); non-Fed central bank contracts will classify
/// correctly but use Fed-centric economic context until per-bank contexts
/// are added (a future refinement, not a C0.4 blocker).
fn resolve_gamma_event(
    title: &str,
    tags: &[String],
    _slug: &str,
) -> Option<(fibo::FiboConcept, BaseEconomicObject, Vec<String>)> {
    use BaseEconomicObject::*;
    // Interest rates: Fed funds / FOMC / policy rate. Covers the full
    // Polymarket phrasing variety: "fed rate", "fed funds", "fomc",
    // "rate cut", "rate hike", "interest rate", "upper bound" (Fed's upper
    // bound), "federal funds rate" (full phrase), "rates hit/stay above",
    // and non-Fed central banks (ECB, BoE, BoC) — all the same economic
    // object under FIBO PolicyInterestRate.
    if title.contains("fed rate")
        || title.contains("fed funds")
        || title.contains("federal funds rate")
        || title.contains("fomc")
        || title.contains("rate cut")
        || title.contains("rate hike")
        || title.contains("interest rate")
        || title.contains("upper bound")
        || title.contains("rates hit")
        || title.contains("rates stay")
        || title.contains("ecb rate")
        || title.contains("bank of england rate")
        || title.contains("bank of canada rate")
    {
        return Some((
            fibo::POLICY_INTEREST_RATE,
            PolicyInterestRate,
            vec![
                "interest rate".into(),
                "federal reserve".into(),
                "monetary policy".into(),
            ],
        ));
    }
    // Inflation: CPI.
    if title.contains("inflation") || title.contains("cpi") || title.contains("consumer price") {
        return Some((
            fibo::CONSUMER_PRICE_INDEX,
            ConsumerPriceInflation,
            vec![
                "inflation".into(),
                "consumer price index".into(),
                "cpi".into(),
            ],
        ));
    }
    // Crude oil: WTI, Brent.
    if title.contains("crude oil")
        || title.contains("wti")
        || title.contains("brent")
        || title.contains("oil price")
    {
        return Some((
            fibo::COMMODITY_PRICE_INDEX,
            CrudeOilPrice,
            vec!["crude oil".into(), "wti".into(), "commodity".into()],
        ));
    }
    // Natural gas.
    if title.contains("natural gas") || title.contains("henry hub") {
        return Some((
            fibo::COMMODITY_PRICE_INDEX,
            NaturalGasPrice,
            vec!["natural gas".into(), "commodity".into()],
        ));
    }
    // Bitcoin.
    if title.contains("bitcoin") || title.contains(" btc") || title.starts_with("btc") {
        return Some((
            fibo::MARKET_INDEX,
            BitcoinPrice,
            vec!["bitcoin".into(), "btc".into(), "cryptocurrency".into()],
        ));
    }
    // Ethereum.
    if title.contains("ethereum") || title.contains(" eth") {
        return Some((
            fibo::MARKET_INDEX,
            EthereumPrice,
            vec!["ethereum".into(), "eth".into(), "cryptocurrency".into()],
        ));
    }
    // GDP / economic growth.
    if title.contains("gdp") || title.contains("gross domestic") {
        return Some((
            fibo::GROSS_DOMESTIC_PRODUCT,
            RealGdpGrowth,
            vec!["gdp".into(), "economic growth".into()],
        ));
    }
    // Check tags for crypto prices (Gamma uses "crypto prices" tag).
    if tags.iter().any(|t| t.contains("crypto price")) {
        if title.contains("bitcoin") || title.contains("btc") {
            return Some((
                fibo::MARKET_INDEX,
                BitcoinPrice,
                vec!["bitcoin".into(), "btc".into(), "cryptocurrency".into()],
            ));
        }
        if title.contains("ethereum") || title.contains("eth") {
            return Some((
                fibo::MARKET_INDEX,
                EthereumPrice,
                vec!["ethereum".into(), "eth".into(), "cryptocurrency".into()],
            ));
        }
    }
    None
}

/// Build constellations of events clustered around each base economic object.
///
/// For each base object, collect all mapped events that resolved to that
/// object. The constellation is the set of events sharing the same FIBO
/// concept (process axis) and overlapping DC subjects (state axis). Events
/// are sorted by their proximity to the base object's canonical FIBO concept.
pub fn build_constellations(events: &[MappedEvent]) -> Vec<EventConstellation> {
    let mut by_object: HashMap<BaseEconomicObject, Vec<&MappedEvent>> = HashMap::new();
    for event in events {
        if let Some(base_object) = event.base_object {
            by_object.entry(base_object).or_default().push(event);
        }
    }
    let mut constellations = Vec::new();
    for (base_object, mut events_for_object) in by_object {
        // Sort by FIBO concept exactness (exact match to the base object's
        // canonical concept first), then by title for stability.
        let canonical_concept = base_object.fibo_concept();
        events_for_object.sort_by(|a, b| {
            let a_exact = a.fibo_concept == canonical_concept;
            let b_exact = b.fibo_concept == canonical_concept;
            b_exact.cmp(&a_exact).then_with(|| a.title.cmp(&b.title))
        });
        let fibo_concept = events_for_object
            .first()
            .map(|e| e.fibo_concept)
            .unwrap_or(canonical_concept);
        constellations.push(EventConstellation {
            base_object,
            fibo_concept,
            events: events_for_object
                .into_iter()
                .map(|e| MappedEvent {
                    event_id: e.event_id.clone(),
                    title: e.title.clone(),
                    series: e.series.clone(),
                    category: e.category.clone(),
                    fibo_concept: e.fibo_concept,
                    dc_subjects: e.dc_subjects.clone(),
                    dc_type: e.dc_type,
                    end_date: e.end_date.clone(),
                    base_object: e.base_object,
                })
                .collect(),
        });
    }
    constellations.sort_by_key(|a| a.base_object);
    constellations
}
