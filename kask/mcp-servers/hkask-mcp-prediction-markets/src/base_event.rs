//! Base-event registry (C0.1) — the semantic identity of the systematic
//! factors CMP indices are built over.
//!
//! A base event is a contract family where some semantic form of the contract
//! is always available on Kalshi and Polymarket, and whose subject is a
//! systematic factor in the global economy (oil, natural gas, bitcoin,
//! ethereum, inflation, interest rates). The registry maps each family's
//! semantic signature (matched over a record's question / description /
//! series / category) to its materiality setting, so eligibility is a
//! semantic match — not a keyword guess scattered across call sites.
//!
//! Continuous availability is verified live via `market_ladder` (the CP-CMP
//! checkpoint), not assumed by the registry.

use crate::cmp_portfolio::{MaterialitySetting, MaterialityType};
use crate::types::MarketRecord;

/// A systematic factor family CMP indices are built over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseEvent {
    Oil,
    NaturalGas,
    Bitcoin,
    Ethereum,
    Inflation,
    InterestRates,
}

impl BaseEvent {
    /// All registered base events.
    pub const ALL: [BaseEvent; 6] = [
        BaseEvent::Oil,
        BaseEvent::NaturalGas,
        BaseEvent::Bitcoin,
        BaseEvent::Ethereum,
        BaseEvent::Inflation,
        BaseEvent::InterestRates,
    ];

    /// The systematic factor this family tracks (provenance for the registry).
    pub fn factor(self) -> &'static str {
        match self {
            BaseEvent::Oil => "crude oil price level (energy / input cost)",
            BaseEvent::NaturalGas => "natural gas price level (energy / input cost)",
            BaseEvent::Bitcoin => "bitcoin price level (risk-asset / liquidity proxy)",
            BaseEvent::Ethereum => "ethereum price level (risk-asset / liquidity proxy)",
            BaseEvent::Inflation => "consumer-price inflation (price level)",
            BaseEvent::InterestRates => "central-bank policy rate (discount rate)",
        }
    }

    /// Semantic signature: lowercase tokens that identify this family when
    /// present in a record's question / description / series / category.
    /// Kept deliberately small and specific — a match on any token is a
    /// candidate, subject to the materiality and orientation gates downstream.
    fn signature(self) -> &'static [&'static str] {
        match self {
            BaseEvent::Oil => &["oil", "crude", "wti", "brent"],
            BaseEvent::NaturalGas => &["natural gas", "natgas", "henry hub"],
            BaseEvent::Bitcoin => &["bitcoin", "btc"],
            BaseEvent::Ethereum => &["ethereum", "eth"],
            BaseEvent::Inflation => &["inflation", "cpi", "consumer price", "pce"],
            BaseEvent::InterestRates => &[
                "interest rate",
                "fed funds",
                "federal reserve",
                "fomc",
                "policy rate",
                "rate decision",
                "rate hike",
                "rate cut",
            ],
        }
    }

    /// The default materiality setting for this family. Types follow how
    /// volatility is measured for the underlying (absolute for rates and
    /// inflation, relative for commodities and crypto); levels are reviewed
    /// per contract (user directive) and start as volatility-derived.
    pub fn default_materiality(self) -> MaterialitySetting {
        match self {
            BaseEvent::InterestRates => MaterialitySetting {
                materiality_type: MaterialityType::Absolute,
                k: 1.0,
                level_override: None,
                rationale: "absolute type — rate volatility measured in basis points; level volatility-derived pending per-contract review".into(),
            },
            BaseEvent::Inflation => MaterialitySetting {
                materiality_type: MaterialityType::Absolute,
                k: 1.0,
                level_override: None,
                rationale: "absolute type — inflation volatility measured in percentage points; level volatility-derived pending per-contract review".into(),
            },
            BaseEvent::Oil
            | BaseEvent::NaturalGas
            | BaseEvent::Bitcoin
            | BaseEvent::Ethereum => MaterialitySetting {
                materiality_type: MaterialityType::Relative,
                k: 1.0,
                level_override: None,
                rationale: "relative type — price volatility measured in return terms; level volatility-derived pending per-contract review".into(),
            },
        }
    }

    /// A curated default economic context for this family: the reference
    /// level, trailing volatility, and a representative predicted level +
    /// direction. These are well-reasoned starting points (set points) for
    /// the CMP index construction — not authoritative. The operator or the
    /// AI-assist tool may override them with live data or better estimates.
    ///
    /// The values are sourced from public macro data (FRED, Bloomberg
    /// consensus) and are intentionally conservative: the volatility is the
    /// trailing 30-day annualized figure, the reference is the latest published
    /// level, and the predicted level defaults to the reference (Stable
    /// orientation) so the first-pass index is the stable-orientation curve.
    ///
    /// This follows the zed-kask design pattern: never present a blank field
    /// — always provide a reasonable default the user can accept or override.
    pub fn default_economic_context(self) -> EconomicContext {
        match self {
            BaseEvent::InterestRates => EconomicContext {
                reference: 5.375,       // Fed funds target midpoint (5.25-5.50%), Q3 2024
                volatility: Some(25.0), // ~25bp trailing 30d annualized
                predicted_level: 5.375, // defaults to reference → Stable
                direction_up: false,
                rationale: "Fed funds target midpoint 5.375% (5.25-5.50% range); \
                            volatility 25bp (trailing 30d annualized, FRED DFF); \
                            predicted_level defaults to reference → Stable orientation. \
                            Override with the contract's strike for directional indices."
                    .into(),
            },
            BaseEvent::Inflation => EconomicContext {
                reference: 3.0,        // CPI YoY%, latest published
                volatility: Some(0.3), // ~0.3pp trailing 30d
                predicted_level: 3.0,
                direction_up: false,
                rationale: "CPI YoY 3.0% (BLS, latest); volatility 0.3pp; \
                            predicted_level defaults to reference → Stable. \
                            Override with the market's strike for directional indices."
                    .into(),
            },
            BaseEvent::Oil => EconomicContext {
                reference: 80.0,        // WTI $/bbl, approximate recent
                volatility: Some(0.25), // ~25% trailing 30d annualized
                predicted_level: 80.0,
                direction_up: false,
                rationale: "WTI crude ~$80/bbl; volatility 25% (annualized); \
                            predicted_level defaults to reference → Stable."
                    .into(),
            },
            BaseEvent::NaturalGas => EconomicContext {
                reference: 2.5,         // Henry Hub $/MMBtu
                volatility: Some(0.40), // ~40% — natgas is highly volatile
                predicted_level: 2.5,
                direction_up: false,
                rationale: "Henry Hub ~$2.50/MMBtu; volatility 40% (annualized); \
                            predicted_level defaults to reference → Stable."
                    .into(),
            },
            BaseEvent::Bitcoin => EconomicContext {
                reference: 65000.0,     // BTC USD, approximate recent
                volatility: Some(0.50), // ~50% trailing 30d annualized
                predicted_level: 65000.0,
                direction_up: false,
                rationale: "BTC ~$65,000; volatility 50% (annualized); \
                            predicted_level defaults to reference → Stable."
                    .into(),
            },
            BaseEvent::Ethereum => EconomicContext {
                reference: 3500.0,      // ETH USD, approximate recent
                volatility: Some(0.55), // ~55% trailing 30d annualized
                predicted_level: 3500.0,
                direction_up: false,
                rationale: "ETH ~$3,500; volatility 55% (annualized); \
                            predicted_level defaults to reference → Stable."
                    .into(),
            },
        }
    }
}

/// A curated default economic context for a base-event family: the reference
/// level, trailing volatility, predicted level, and direction. Used as the
/// starting point for CMP index construction when the operator doesn't supply
/// live values. See [`BaseEvent::default_economic_context`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct EconomicContext {
    /// The current level of the underlying factor.
    pub reference: f64,
    /// Trailing volatility in the family's type units (absolute: bp/pp/$;
    /// relative: as a fraction). None when unmeasured.
    pub volatility: Option<f64>,
    /// The predicted level (strike) the contracts are structured around.
    /// Defaults to the reference → Stable orientation.
    pub predicted_level: f64,
    /// Whether the contract predicts the factor ends above its strike.
    pub direction_up: bool,
    /// Human-readable reasoning for the chosen values.
    pub rationale: String,
}

/// Match a market record to a base event by semantic signature over its
/// question, description, series, and category. Returns the first matching
/// family, or None when the record is not a base-event contract.
///
/// Matching is case-insensitive substring over the concatenated text. This is
/// the semantic-match substrate; the FIBO subject mapping (a future
/// refinement) can tighten it, but the signature is deliberately readable and
/// auditable so eligibility decisions are explainable.
pub fn classify_base_event(record: &MarketRecord) -> Option<BaseEvent> {
    classify_base_event_text(
        &record.question,
        &record.description,
        &record.series,
        &record.category,
    )
}

/// Text-only base-event classification. Same matching as
/// [`classify_base_event`] but without constructing a full [`MarketRecord`] —
/// for call sites that have the raw text fields (e.g. a Kalshi market's
/// title/subtitle/series) and don't need the full annotated record.
pub fn classify_base_event_text(
    question: &str,
    description: &str,
    series: &str,
    category: &str,
) -> Option<BaseEvent> {
    let haystack = format!("{question} {description} {series} {category}").to_lowercase();
    BaseEvent::ALL.into_iter().find(|event| {
        event
            .signature()
            .iter()
            .any(|token| haystack.contains(token))
    })
}

// ── Live reference-level fetch ───────────────────────────────────────
//
// Fetches the current level of the underlying factor for each base-event
// family from public APIs. Used by `market_cmp_context_suggest` to propose
// *current* reference levels rather than the curated static defaults. Falls
// back to the static default on any failure (network error, parse error,
// missing API key) — the zed-kask pattern: always have a default, the live
// fetch is an enhancement, never a hard requirement.
//
// Sources:
// - Interest rates: FRED `FEDFUNDS` (requires HKASK_FRED_API_KEY)
// - Inflation: FRED `CPIAUCSL` YoY % (requires HKASK_FRED_API_KEY)
// - Oil (WTI): FRED `DCOILWTICO` (requires HKASK_FRED_API_KEY)
// - Natural gas: FRED `DHHNGSP` (requires HKASK_FRED_API_KEY)
// - Bitcoin: CoinGecko `/simple/price` (no key)
// - Ethereum: CoinGecko `/simple/price` (no key)

impl BaseEvent {
    /// The FRED series ID for this base-event family, when available.
    /// Crypto families use CoinGecko instead (no FRED series).
    fn fred_series_id(self) -> Option<&'static str> {
        match self {
            BaseEvent::InterestRates => Some("FEDFUNDS"),
            BaseEvent::Inflation => Some("CPIAUCSL"),
            BaseEvent::Oil => Some("DCOILWTICO"),
            BaseEvent::NaturalGas => Some("DHHNGSP"),
            BaseEvent::Bitcoin | BaseEvent::Ethereum => None,
        }
    }

    /// The CoinGecko coin id for crypto families, when applicable.
    fn coingecko_id(self) -> Option<&'static str> {
        match self {
            BaseEvent::Bitcoin => Some("bitcoin"),
            BaseEvent::Ethereum => Some("ethereum"),
            _ => None,
        }
    }

    /// Fetch the latest reference level for this base-event family from a live
    /// API. Returns `None` on any failure (network, parse, missing key) — the
    /// caller falls back to the curated static default.
    ///
    /// `fred_api_key` is required for FRED series (rates, inflation, oil, natgas);
    /// CoinGecko (crypto) needs no key. A `None` return is never an error for the
    /// caller — it means "use the curated default."
    pub async fn fetch_live_reference(
        self,
        http: &reqwest::Client,
        fred_api_key: Option<&str>,
    ) -> Option<f64> {
        if let Some(series) = self.fred_series_id() {
            let key = fred_api_key?;
            let url = format!(
                "https://api.stlouisfed.org/fred/series/observations?series_id={series}&api_key={key}&file_type=json&limit=1&sort_order=desc"
            );
            let resp = http.get(&url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let body: serde_json::Value = resp.json().await.ok()?;
            // FRED returns { observations: [ { date, value } ] }
            let value_str = body
                .get("observations")?
                .as_array()?
                .first()?
                .get("value")?
                .as_str()?;
            value_str.parse::<f64>().ok()
        } else if let Some(coin_id) = self.coingecko_id() {
            let url = format!(
                "https://api.coingecko.com/api/v3/simple/price?ids={coin_id}&vs_currencies=usd"
            );
            let resp = http.get(&url).send().await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let body: serde_json::Value = resp.json().await.ok()?;
            body.get(coin_id)?.get("usd")?.as_f64()
        } else {
            None
        }
    }

    /// Fetch a live `EconomicContext` for this family: the reference level from
    /// a live API (falling back to the curated default), with the curated
    /// volatility and a rationale explaining the source. The `predicted_level`
    /// defaults to the reference (Stable orientation) — the operator overrides
    /// for directional indices.
    pub async fn live_economic_context(
        self,
        http: &reqwest::Client,
        fred_api_key: Option<&str>,
    ) -> EconomicContext {
        let default = self.default_economic_context();
        match self.fetch_live_reference(http, fred_api_key).await {
            Some(live_ref) => EconomicContext {
                reference: live_ref,
                predicted_level: live_ref, // default → Stable
                rationale: format!(
                    "{} Live reference fetched from {}. Override predicted_level for directional indices.",
                    default.rationale,
                    self.live_source_label()
                ),
                ..default
            },
            None => default,
        }
    }

    /// Human-readable label for the live data source.
    fn live_source_label(self) -> &'static str {
        match self {
            BaseEvent::InterestRates => "FRED FEDFUNDS",
            BaseEvent::Inflation => "FRED CPIAUCSL",
            BaseEvent::Oil => "FRED DCOILWTICO",
            BaseEvent::NaturalGas => "FRED DHHNGSP",
            BaseEvent::Bitcoin => "CoinGecko",
            BaseEvent::Ethereum => "CoinGecko",
        }
    }

    /// Extract the predicted level (strike) and direction from a Kalshi
    /// market title. Returns `(predicted_level, direction_up)` when a strike
    /// is parseable, or `None` when the title has no extractable numeric
    /// strike (e.g. "Will the Fed cut the policy rate?" — directional only).
    ///
    /// The extractor handles the common Kalshi title patterns:
    /// - "... above $X" / "... at or above $X" → strike X, direction up
    /// - "... below $X" / "... at or below $X" → strike X, direction down
    /// - "... exceed $X" / "... exceed X%" → strike X, direction up
    /// - "... at $X" → strike X, direction up (default)
    /// - "... $X or more" → strike X, direction up
    /// - "... $X or less" → strike X, direction down
    ///
    /// The unit suffix ($, %, k, M) is stripped and the value normalized to
    /// the family's native units (e.g. $85 → 85.0 for oil, 5.50% → 5.50 for
    /// rates). The `k` suffix multiplies by 1,000; `M` by 1,000,000.
    pub fn extract_strike(self, title: &str) -> Option<(f64, bool)> {
        let lower = title.to_lowercase();
        // Direction keywords.
        let direction_up = !(lower.contains("below") || lower.contains("or less"));
        // Find a numeric value preceded by $ or followed by %, or a bare
        // number after a strike keyword. We scan for patterns like "$X",
        // "X%", "Xk", "XM".
        let strike = self.find_strike_in_text(&lower)?;
        Some((strike, direction_up))
    }

    /// Find the first parseable strike value in a lowercased title.
    fn find_strike_in_text(self, lower: &str) -> Option<f64> {
        // Try $-prefixed numbers first (e.g. "$85", "$150k", "$6k").
        if let Some(v) = find_dollar_number(lower) {
            return Some(v);
        }
        // Try %-suffixed numbers (e.g. "3%", "5.5%").
        if let Some(v) = find_percent_number(lower) {
            return Some(v);
        }
        // Try bare numbers after "exceed" / "above" / "below" / "at".
        for keyword in [
            "exceed",
            "above",
            "below",
            "at or above",
            "at or below",
            "at",
        ] {
            if let Some(v) = find_number_after_keyword(lower, keyword) {
                return Some(v);
            }
        }
        None
    }
}

/// Find a $-prefixed number in the text, handling k/M suffixes.
/// e.g. "$85" → 85.0, "$150k" → 150000.0, "$6k" → 6000.0.
fn find_dollar_number(text: &str) -> Option<f64> {
    let dollar_pos = text.find('$')?;
    let after = &text[dollar_pos + 1..];
    parse_leading_number(after)
}

/// Find a %-suffixed number in the text.
/// e.g. "3%" → 3.0, "5.5%" → 5.5.
fn find_percent_number(text: &str) -> Option<f64> {
    let pct_pos = text.find('%')?;
    let before = &text[..pct_pos];
    // Scan backward for the number.
    let mut end = before.len();
    while end > 0 && before.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 {
        let b = before.as_bytes()[start - 1];
        if b.is_ascii_digit() || b == b'.' {
            start -= 1;
        } else {
            break;
        }
    }
    if start < end {
        before[start..end].parse::<f64>().ok()
    } else {
        None
    }
}

/// Find a number immediately after a keyword.
fn find_number_after_keyword(text: &str, keyword: &str) -> Option<f64> {
    let pos = text.find(keyword)?;
    let after = &text[pos + keyword.len()..];
    parse_leading_number(after.trim_start())
}

/// Parse a leading number from a string, handling k/M suffixes and
/// trailing non-numeric characters.
fn parse_leading_number(s: &str) -> Option<f64> {
    let mut end = 0;
    let bytes = s.as_bytes();
    // Skip leading whitespace.
    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    let start = end;
    // Consume digits and dots.
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    if end == start {
        return None;
    }
    let num_str = &s[start..end];
    let mut value = num_str.parse::<f64>().ok()?;
    // Check for k/M suffix.
    if end < bytes.len() {
        match bytes[end] {
            b'k' => value *= 1_000.0,
            b'm' => value *= 1_000_000.0,
            _ => {}
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_with(question: &str, series: &str, category: &str) -> MarketRecord {
        let mut record = crate::types::test_utils::market_record_fixture();
        record.question = question.into();
        record.series = series.into();
        record.category = category.into();
        record
    }

    #[test]
    fn classifies_each_base_event() {
        let cases = [
            (
                "Will WTI crude close above $85?",
                "KXOIL",
                "commodities",
                BaseEvent::Oil,
            ),
            (
                "Natural gas prices in March?",
                "KXNG",
                "commodities",
                BaseEvent::NaturalGas,
            ),
            (
                "Will Bitcoin exceed $150k?",
                "KXBTC",
                "crypto",
                BaseEvent::Bitcoin,
            ),
            (
                "Ethereum above $6k by June?",
                "KXETH",
                "crypto",
                BaseEvent::Ethereum,
            ),
            (
                "Will CPI inflation exceed 3%?",
                "KXCPI",
                "economics",
                BaseEvent::Inflation,
            ),
            (
                "Will the Fed cut the policy rate?",
                "KXFEDDECISION",
                "economics",
                BaseEvent::InterestRates,
            ),
        ];
        for (question, series, category, expected) in cases {
            let record = record_with(question, series, category);
            assert_eq!(
                classify_base_event(&record),
                Some(expected),
                "question: {question}"
            );
        }
    }

    #[test]
    fn non_base_event_returns_none() {
        let record = record_with("Will the mayor win re-election?", "KXMAYOR", "politics");
        assert_eq!(classify_base_event(&record), None);
    }

    #[test]
    fn materiality_types_follow_volatility_units() {
        assert_eq!(
            BaseEvent::InterestRates
                .default_materiality()
                .materiality_type,
            MaterialityType::Absolute
        );
        assert_eq!(
            BaseEvent::Inflation.default_materiality().materiality_type,
            MaterialityType::Absolute
        );
        assert_eq!(
            BaseEvent::Oil.default_materiality().materiality_type,
            MaterialityType::Relative
        );
        assert_eq!(
            BaseEvent::Bitcoin.default_materiality().materiality_type,
            MaterialityType::Relative
        );
    }

    #[test]
    fn default_economic_context_is_non_blank_for_every_family() {
        // The zed-kask pattern: never a blank field. Every family has a
        // curated default with a non-zero reference and a rationale.
        for be in BaseEvent::ALL {
            let ctx = be.default_economic_context();
            assert!(ctx.reference != 0.0, "{:?} reference is blank", be);
            assert!(!ctx.rationale.is_empty(), "{:?} rationale is blank", be);
            assert!(
                (ctx.predicted_level - ctx.reference).abs() < 1e-9,
                "{:?} predicted_level should default to reference (Stable)",
                be
            );
        }
    }

    #[test]
    fn interest_rates_default_context_is_fed_funds_midpoint() {
        let ctx = BaseEvent::InterestRates.default_economic_context();
        assert!(
            (ctx.reference - 5.375).abs() < 1e-9,
            "reference = {}",
            ctx.reference
        );
        assert!(
            ctx.volatility.unwrap_or(0.0) > 0.0,
            "volatility should be non-zero"
        );
        assert!(
            ctx.rationale.contains("Fed funds"),
            "rationale should mention Fed funds"
        );
    }

    // ── Strike extraction tests ──────────────────────────────────────

    #[test]
    fn extract_strike_dollar_above() {
        // "Will WTI crude close above $85?" → strike 85, direction up
        let (strike, up) = BaseEvent::Oil
            .extract_strike("Will WTI crude close above $85?")
            .unwrap();
        assert!((strike - 85.0).abs() < 1e-9, "strike = {}", strike);
        assert!(up, "direction up");
    }

    #[test]
    fn extract_strike_dollar_below() {
        // "Will WTI crude close below $80?" → strike 80, direction down
        let (strike, up) = BaseEvent::Oil
            .extract_strike("Will WTI crude close below $80?")
            .unwrap();
        assert!((strike - 80.0).abs() < 1e-9, "strike = {}", strike);
        assert!(!up, "direction down");
    }

    #[test]
    fn extract_strike_dollar_with_k_suffix() {
        // "Will Bitcoin exceed $150k?" → strike 150000, direction up
        let (strike, up) = BaseEvent::Bitcoin
            .extract_strike("Will Bitcoin exceed $150k?")
            .unwrap();
        assert!((strike - 150_000.0).abs() < 1e-6, "strike = {}", strike);
        assert!(up);
    }

    #[test]
    fn extract_strike_dollar_with_k_suffix_small() {
        // "Ethereum above $6k by June?" → strike 6000, direction up
        let (strike, up) = BaseEvent::Ethereum
            .extract_strike("Ethereum above $6k by June?")
            .unwrap();
        assert!((strike - 6_000.0).abs() < 1e-6, "strike = {}", strike);
        assert!(up);
    }

    #[test]
    fn extract_strike_percent() {
        // "Will CPI inflation exceed 3%?" → strike 3.0, direction up
        let (strike, up) = BaseEvent::Inflation
            .extract_strike("Will CPI inflation exceed 3%?")
            .unwrap();
        assert!((strike - 3.0).abs() < 1e-9, "strike = {}", strike);
        assert!(up);
    }

    #[test]
    fn extract_strike_rate_at_or_above() {
        // "Will the Fed funds rate be at or above 5.50%?" → strike 5.5, up
        let (strike, up) = BaseEvent::InterestRates
            .extract_strike("Will the Fed funds rate be at or above 5.50%?")
            .unwrap();
        assert!((strike - 5.5).abs() < 1e-9, "strike = {}", strike);
        assert!(up);
    }

    #[test]
    fn extract_strike_no_strike_returns_none() {
        // "Will the Fed cut the policy rate?" — no numeric strike.
        assert!(
            BaseEvent::InterestRates
                .extract_strike("Will the Fed cut the policy rate?")
                .is_none()
        );
    }

    #[test]
    fn extract_strike_or_less_is_down() {
        // "Crude oil at $70 or less?" → strike 70, direction down
        let (strike, up) = BaseEvent::Oil
            .extract_strike("Crude oil at $70 or less?")
            .unwrap();
        assert!((strike - 70.0).abs() < 1e-9, "strike = {}", strike);
        assert!(!up, "or less → down");
    }
}
