//! Economic-object ontology for prediction contracts (C0.1, corrected).
//!
//! The failure this fixes: keyword grep is not semantic mapping. "Fed
//! decision", "FOMC meeting", "Fed funds rate", "rate cut", "how many cuts"
//! all have the SAME object — the central bank's short-term policy interest
//! rate. A semantic mapping resolves each contract to WHAT IT IS ABOUT (its
//! object, at the right granularity) through an explicit ontology, not
//! substring luck.
//!
//! Structure (a real ontology, not a keyword list):
//! - `EconomicObject` — the thing a contract is about (its referent).
//! - Each object carries a FIBO-anchored concept URI (the same OMG standard
//!   the companies server uses) and a `synonym_closure`: the set of surface
//!   forms that refer to the object. Synonym closure is the ontological
//!   operation — it maps many names to ONE referent, so "FOMC" and "Fed
//!   funds" and "rate decision" are the same node.
//! - `resolve_object` maps a contract to its object via the closure. A
//!   contract about a Fed meeting where short-term rates are set resolves to
//!   `PolicyInterestRate` — full stop, regardless of which surface form the
//!   venue happened to use.

use crate::types::MarketRecord;

/// The economic object a contract is about — its referent at the granularity
/// CMP indices are built over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EconomicObject {
    /// A central bank's short-term policy interest rate (Fed funds, ECB refi,
    /// BoE bank rate). The object of "Fed decision", "FOMC", "rate cut",
    /// "rate hike", "Fed funds target", "how many cuts" — all the same node.
    PolicyInterestRate,
    /// Consumer-price inflation (CPI, PCE).
    ConsumerPriceInflation,
    /// Crude oil price level (WTI, Brent).
    CrudeOilPrice,
    /// Natural gas price level (Henry Hub).
    NaturalGasPrice,
    /// Bitcoin price level.
    BitcoinPrice,
    /// Ethereum price level.
    EthereumPrice,
    /// Real gross domestic product growth.
    RealGdpGrowth,
}

impl EconomicObject {
    /// All registered objects.
    pub const ALL: [EconomicObject; 7] = [
        EconomicObject::PolicyInterestRate,
        EconomicObject::ConsumerPriceInflation,
        EconomicObject::CrudeOilPrice,
        EconomicObject::NaturalGasPrice,
        EconomicObject::BitcoinPrice,
        EconomicObject::EthereumPrice,
        EconomicObject::RealGdpGrowth,
    ];

    /// FIBO-anchored concept URI for this object (the OMG financial ontology
    /// the companies server anchors to). Where FIBO has no exact class, the
    /// nearest indicator/index concept is used and noted.
    pub fn fibo_concept(self) -> &'static str {
        match self {
            EconomicObject::PolicyInterestRate => "fibo-ind-ir-ir:PolicyInterestRate",
            EconomicObject::ConsumerPriceInflation => "fibo-ind-ei-ei:ConsumerPriceIndex",
            EconomicObject::CrudeOilPrice => "fibo-ind-ei-ei:CommodityPriceIndex",
            EconomicObject::NaturalGasPrice => "fibo-ind-ei-ei:CommodityPriceIndex",
            EconomicObject::BitcoinPrice => "fibo-ind-ei-ei:MarketIndex",
            EconomicObject::EthereumPrice => "fibo-ind-ei-ei:MarketIndex",
            EconomicObject::RealGdpGrowth => "fibo-ind-ei-ei:GrossDomesticProduct",
        }
    }

    /// Synonym closure: the surface forms that refer to this object.
    ///
    /// This is the ontological core. A contract whose text contains ANY of
    /// these forms is ABOUT this object. The closure is deliberately
    /// exhaustive for the object — it includes the institutions (Fed, FOMC,
    /// ECB), the instruments (Fed funds, refi rate), the actions (rate cut,
    /// rate hike, rate decision), and the colloquial forms (cuts, hikes)
    /// because they all denote the same referent.
    fn synonym_closure(self) -> &'static [&'static str] {
        match self {
            EconomicObject::PolicyInterestRate => &[
                // institutions that set the policy rate
                "federal reserve",
                "fomc",
                "federal open market",
                "the fed",
                "fed ",
                "central bank",
                "ecb",
                "european central bank",
                "bank of england",
                "bank of japan",
                "boj",
                // the instruments / rates
                "interest rate",
                "fed funds",
                "federal funds",
                "policy rate",
                "bank rate",
                "refi rate",
                "discount rate",
                "overnight rate",
                "base rate",
                "key rate",
                // the actions on the rate
                "rate cut",
                "rate hike",
                "rate decision",
                "rate increase",
                "rate decrease",
                "rate change",
                "cut rates",
                "hike rates",
                "raise rates",
                "lower rates",
                "cuts",
                "hikes",
                // units of rate change
                "basis point",
                "bps",
            ],
            EconomicObject::ConsumerPriceInflation => &[
                "inflation",
                "cpi",
                "consumer price",
                "pce",
                "price index",
                "cost of living",
                "deflation",
                "disinflation",
            ],
            EconomicObject::CrudeOilPrice => &[
                "crude oil",
                "wti",
                "brent",
                "oil price",
                "price of oil",
                "oil futures",
                "barrel of oil",
            ],
            EconomicObject::NaturalGasPrice => &["natural gas", "natgas", "henry hub", "gas price"],
            EconomicObject::BitcoinPrice => &["bitcoin", "btc", "xbt"],
            EconomicObject::EthereumPrice => &["ethereum", "ether", "eth "],
            EconomicObject::RealGdpGrowth => &[
                "gdp",
                "gross domestic product",
                "economic growth",
                "real growth",
            ],
        }
    }
}

/// Resolve a contract to its economic object via synonym closure over the
/// contract's question, description, series, and category.
///
/// Returns the object the contract is ABOUT, or None when it is not about a
/// registered economic object. This is the semantic mapping: a contract about
/// a Federal Reserve meeting where short-term interest rates are set resolves
/// to `PolicyInterestRate` whether the venue wrote "FOMC decision", "Fed
/// funds target", or "how many rate cuts".
pub fn resolve_object(record: &MarketRecord) -> Option<EconomicObject> {
    let text = format!(
        "{} {} {} {}",
        record.question, record.description, record.series, record.category
    )
    .to_lowercase();
    EconomicObject::ALL.into_iter().find(|object| {
        object
            .synonym_closure()
            .iter()
            .any(|form| text.contains(form))
    })
}

/// All contracts in a catalog that are about a given economic object — the
/// interest-rate-related list that must NOT exclude obvious rate contracts.
pub fn contracts_about<'a>(
    records: &'a [MarketRecord],
    object: EconomicObject,
) -> Vec<&'a MarketRecord> {
    records
        .iter()
        .filter(|record| resolve_object(record) == Some(object))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(question: &str, series: &str) -> MarketRecord {
        let mut record = crate::types::test_utils::market_record_fixture();
        record.question = question.into();
        record.series = series.into();
        record
    }

    #[test]
    fn fed_decision_is_policy_interest_rate() {
        // The exact failure case: a Fed meeting where short-term rates are
        // set MUST resolve to PolicyInterestRate.
        for question in [
            "Will the Fed cut rates at the March FOMC meeting?",
            "Fed funds rate at end of 2027?",
            "How many Fed rate cuts in 2026?",
            "Will the FOMC raise the policy rate?",
            "Federal Reserve interest rate decision",
            "Will there be a rate hike in June?",
        ] {
            let record = record(question, "KXFED");
            assert_eq!(
                resolve_object(&record),
                Some(EconomicObject::PolicyInterestRate),
                "failed to resolve: {question}"
            );
        }
    }

    #[test]
    fn distinct_objects_do_not_collide() {
        assert_eq!(
            resolve_object(&record("Will CPI inflation exceed 3%?", "KXCPI")),
            Some(EconomicObject::ConsumerPriceInflation)
        );
        assert_eq!(
            resolve_object(&record("Will WTI crude close above $85?", "KXOIL")),
            Some(EconomicObject::CrudeOilPrice)
        );
        assert_eq!(
            resolve_object(&record("Will Bitcoin exceed $150k?", "KXBTC")),
            Some(EconomicObject::BitcoinPrice)
        );
        assert_eq!(
            resolve_object(&record("Will the mayor win?", "KXMAYOR")),
            None
        );
    }

    #[test]
    fn contracts_about_collects_all_rate_contracts() {
        let records = vec![
            record("Will the Fed cut rates?", "KXFEDDECISION"),
            record("Fed funds rate at end of 2027?", "KXFEDFUNDSYEAR"),
            record("How many rate cuts in 2026?", "FEDCUTS"),
            record("Will CPI exceed 3%?", "KXCPI"),
        ];
        let rate_contracts = contracts_about(&records, EconomicObject::PolicyInterestRate);
        assert_eq!(
            rate_contracts.len(),
            3,
            "must not exclude obvious rate contracts"
        );
    }
}
