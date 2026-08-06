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
    let haystack = format!(
        "{} {} {} {}",
        record.question, record.description, record.series, record.category
    )
    .to_lowercase();
    BaseEvent::ALL.into_iter().find(|event| {
        event
            .signature()
            .iter()
            .any(|token| haystack.contains(token))
    })
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
}
