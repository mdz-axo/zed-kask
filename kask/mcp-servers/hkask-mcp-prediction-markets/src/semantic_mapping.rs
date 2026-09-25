//! Semantic event classification — maps a prediction-market record to its
//! base economic object and FIBO concept (process axis) with Dublin Core
//! subjects (state axis). The venue's curated taxonomy (Kalshi series
//! prefix, Gamma tags/question) is the primary signal, never substring grep.
//! Used by the calibration buckets and the CMP catalog path.

use crate::economic_object::BaseEconomicObject;
use hkask_bridge_ontology::fibo;

/// Classify a catalog record's base economic object using the FIBO-anchored
/// semantic mapping. This bridges catalog records and the
/// `BaseEconomicObject` registry — the venue's curated taxonomy is the primary signal, not
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
/// (catalog records do not carry event-level tags). The
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

/// The calibration bucket a Kalshi market's snapshot accrues under.
/// Family-first: a base-event market (KXFED*, KXCPI*, KXWTI*, …) accrues
/// under its semantic family label — the bucket `market_calibration`
/// readers and the methodology's W2 leg query (e.g. "policy_interest_rate").
/// Non-base markets fall back to the series prefix of the event ticker
/// (lowercased) — a per-SERIES bucket, never a per-event one; a per-event key
/// like "kxfeddecision-26oct" shards one family across its meetings and the
/// bucket can never reach a demotion threshold.
pub fn calibration_bucket_for_kalshi(event_ticker: &str, title: &str) -> String {
    if let Some(family) = classify_base_object_from_catalog("kalshi", event_ticker, title) {
        return family.label().to_string();
    }
    let series_prefix = event_ticker
        .split('-')
        .next()
        .unwrap_or(event_ticker)
        .to_lowercase();
    crate::types::canonical_bucket(&series_prefix)
}

/// The calibration bucket a Polymarket Gamma market's snapshot accrues
/// under. Family-first through the gamma classification (question text);
/// non-base markets keep the historical slug-keyed bucket.
pub fn calibration_bucket_for_gamma(question: &str, slug: &str) -> String {
    if let Some(family) = classify_base_object_from_catalog("gamma", "", question) {
        return family.label().to_string();
    }
    crate::types::canonical_bucket(slug)
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
            fibo::REFERENCE_INTEREST_RATE,
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
            fibo::INTEREST_RATE_BENCHMARK,
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
            fibo::ECONOMIC_INDICATOR,
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
            fibo::ECONOMIC_INDICATOR,
            NaturalGasPrice,
            vec!["natural gas".into(), "henry hub".into(), "commodity".into()],
        )));
    }
    // Bitcoin.
    if series.starts_with("KXBTC") {
        return Some(Some((
            fibo::REFERENCE_INDEX,
            BitcoinPrice,
            vec!["bitcoin".into(), "btc".into(), "cryptocurrency".into()],
        )));
    }
    // Ethereum.
    if series.starts_with("KXETH") {
        return Some(Some((
            fibo::REFERENCE_INDEX,
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

/// Resolve a Gamma event to its FIBO concept + base object + DC subjects.
///
/// Gamma has no controlled series ticker like Kalshi; the tags and title
/// are the semantic signal. The mapping checks for specific economic-object
/// signals in the title (the venue's curated naming) and the tags.
///
/// Rates coverage handles the full Polymarket phrasing variety: "upper bound", "federal
/// funds rate" (full phrase, not just "fed funds"), "rates hit/stay above",
/// and non-Fed central banks (ECB, BoE, BoC) which are the same economic
/// object (a central-bank policy rate) under the FIBO `PolicyInterestRate`
/// concept. The materiality defaults remain Fed-specific (the reference and
/// volatility are Fed funds); non-Fed central bank contracts will classify
/// correctly but use Fed-centric economic context until per-bank contexts
/// are added.
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
            fibo::REFERENCE_INTEREST_RATE,
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
            fibo::ECONOMIC_INDICATOR,
            CrudeOilPrice,
            vec!["crude oil".into(), "wti".into(), "commodity".into()],
        ));
    }
    // Natural gas.
    if title.contains("natural gas") || title.contains("henry hub") {
        return Some((
            fibo::ECONOMIC_INDICATOR,
            NaturalGasPrice,
            vec!["natural gas".into(), "commodity".into()],
        ));
    }
    // Bitcoin.
    if title.contains("bitcoin") || title.contains(" btc") || title.starts_with("btc") {
        return Some((
            fibo::REFERENCE_INDEX,
            BitcoinPrice,
            vec!["bitcoin".into(), "btc".into(), "cryptocurrency".into()],
        ));
    }
    // Ethereum.
    if title.contains("ethereum") || title.contains(" eth") {
        return Some((
            fibo::REFERENCE_INDEX,
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
                fibo::REFERENCE_INDEX,
                BitcoinPrice,
                vec!["bitcoin".into(), "btc".into(), "cryptocurrency".into()],
            ));
        }
        if title.contains("ethereum") || title.contains("eth") {
            return Some((
                fibo::REFERENCE_INDEX,
                EthereumPrice,
                vec!["ethereum".into(), "eth".into(), "cryptocurrency".into()],
            ));
        }
    }
    None
}
