//! Economic-object ontology for prediction contracts (C0.1, corrected).
//!
//! The failure this fixes: keyword grep is not semantic mapping. "Fed
//! decision", "FOMC meeting", "Fed funds rate", "rate cut", "how many cuts"
//! all have the SAME object — the central bank's short-term policy interest
//! rate. A semantic mapping resolves each contract to WHAT IT IS ABOUT (its
//! object, at the right granularity) through an explicit ontology, not
//! substring luck.
//!
//! The mechanism is now ontological mapping through the shared
//! `hkask-bridge-ontology` crate (see `semantic_mapping.rs`). Each base
//! object carries a FIBO concept URI (the process axis) and is paired with
//! Dublin Core state identity (the state axis). Graph proximity over the
//! two axes identifies constellations of related events — not substring
//! matching.
//!
//! This module defines the base objects (the systematic factors CMP indices
//! are built over) and their FIBO anchors. The actual event → object
//! mapping lives in `semantic_mapping.rs`, which uses the venue's curated
//! taxonomy (Kalshi `series_ticker`, Gamma `tags`) as the primary signal.

use hkask_bridge_ontology::fibo;

/// The economic object a contract is about — its referent at the granularity
/// CMP indices are built over. This is the "base event" — the systematic
/// factor family.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BaseEconomicObject {
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

impl BaseEconomicObject {
    /// All registered base objects.
    pub const ALL: [BaseEconomicObject; 7] = [
        BaseEconomicObject::PolicyInterestRate,
        BaseEconomicObject::ConsumerPriceInflation,
        BaseEconomicObject::CrudeOilPrice,
        BaseEconomicObject::NaturalGasPrice,
        BaseEconomicObject::BitcoinPrice,
        BaseEconomicObject::EthereumPrice,
        BaseEconomicObject::RealGdpGrowth,
    ];

    /// FIBO-anchored concept URI for this object (the process axis).
    ///
    /// Returns the canonical FIBO concept from the shared
    /// `hkask-bridge-ontology` crate. Where FIBO has no exact class, the
    /// nearest indicator/index concept is used.
    pub fn fibo_concept(self) -> fibo::FiboConcept {
        match self {
            BaseEconomicObject::PolicyInterestRate => fibo::POLICY_INTEREST_RATE,
            BaseEconomicObject::ConsumerPriceInflation => fibo::CONSUMER_PRICE_INDEX,
            BaseEconomicObject::CrudeOilPrice => fibo::COMMODITY_PRICE_INDEX,
            BaseEconomicObject::NaturalGasPrice => fibo::COMMODITY_PRICE_INDEX,
            BaseEconomicObject::BitcoinPrice => fibo::MARKET_INDEX,
            BaseEconomicObject::EthereumPrice => fibo::MARKET_INDEX,
            BaseEconomicObject::RealGdpGrowth => fibo::GROSS_DOMESTIC_PRODUCT,
        }
    }

    /// Human-readable label for the base object.
    pub fn label(self) -> &'static str {
        match self {
            BaseEconomicObject::PolicyInterestRate => "policy_interest_rate",
            BaseEconomicObject::ConsumerPriceInflation => "consumer_price_inflation",
            BaseEconomicObject::CrudeOilPrice => "crude_oil_price",
            BaseEconomicObject::NaturalGasPrice => "natural_gas_price",
            BaseEconomicObject::BitcoinPrice => "bitcoin_price",
            BaseEconomicObject::EthereumPrice => "ethereum_price",
            BaseEconomicObject::RealGdpGrowth => "real_gdp_growth",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fibo_concept_anchors_to_correct_uri() {
        assert_eq!(
            BaseEconomicObject::PolicyInterestRate.fibo_concept(),
            "fibo-ind-ir-ir:PolicyInterestRate"
        );
        assert_eq!(
            BaseEconomicObject::ConsumerPriceInflation.fibo_concept(),
            "fibo-ind-ei-ei:ConsumerPriceIndex"
        );
        assert_eq!(
            BaseEconomicObject::CrudeOilPrice.fibo_concept(),
            "fibo-ind-ei-ei:CommodityPriceIndex"
        );
        assert_eq!(
            BaseEconomicObject::BitcoinPrice.fibo_concept(),
            "fibo-ind-ei-ei:MarketIndex"
        );
        assert_eq!(
            BaseEconomicObject::RealGdpGrowth.fibo_concept(),
            "fibo-ind-ei-ei:GrossDomesticProduct"
        );
    }

    #[test]
    fn all_objects_have_distinct_fibo_modules() {
        // PolicyInterestRate is fibo-ind-ir-ir; all others are fibo-ind-ei-ei.
        // This is correct: rates are in the interest-rate module; inflation,
        // commodities, crypto, and GDP are in the economic-indicators module.
        let rate_module = BaseEconomicObject::PolicyInterestRate
            .fibo_concept()
            .split(':')
            .next()
            .unwrap();
        assert_eq!(rate_module, "fibo-ind-ir-ir");
        for object in BaseEconomicObject::ALL {
            if object == BaseEconomicObject::PolicyInterestRate {
                continue;
            }
            let module = object.fibo_concept().split(':').next().unwrap();
            assert_eq!(
                module, "fibo-ind-ei-ei",
                "{object:?} should be in the economic-indicators module"
            );
        }
    }

    #[test]
    fn objects_are_orderable_for_constellation_sorting() {
        let mut objects = vec![
            BaseEconomicObject::BitcoinPrice,
            BaseEconomicObject::PolicyInterestRate,
            BaseEconomicObject::CrudeOilPrice,
        ];
        objects.sort();
        // Derived enum order: PolicyInterestRate < ConsumerPriceInflation < CrudeOilPrice < ...
        // So after sorting: PolicyInterestRate, CrudeOilPrice, BitcoinPrice.
        assert_eq!(
            objects,
            vec![
                BaseEconomicObject::PolicyInterestRate,
                BaseEconomicObject::CrudeOilPrice,
                BaseEconomicObject::BitcoinPrice,
            ]
        );
    }
}
