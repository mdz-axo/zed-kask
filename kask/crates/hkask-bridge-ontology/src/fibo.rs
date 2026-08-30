//! FIBO (Financial Industry Business Ontology) vocabulary bridge.
//!
//! Every URI in this module is mechanically verified against the FIBO
//! master ontology (EDMCouncil/FIBO, <https://spec.edmcouncil.org/fibo/>)
//! — `fixtures/fibo-verified-terms.txt` pins the term list with its
//! defining module, and `all_terms_are_official` fails the build if a
//! term drifts from it. Do not add a term that is not in that fixture.
//!
//! Verification (2026-08-29, FIBO master branch) found that 63 of the 70
//! terms formerly carried here were fabricated: the `fibo-fbc-fct-ra`
//! "Financial Ratios" module prefix never existed in FIBO (no such file
//! in the repository or its git history), and FIBO publishes no terms
//! for financial ratios, DCF line items, valuation methods, portfolio
//! transactions, or the analysis-family tools (Brier scoring, Monte
//! Carlo, screeners, scenario probability). Per the operator decision
//! (2026-08-29), concepts with no real FIBO equivalent fall back to
//! Dublin Core at the consumer (analysis outputs anchor on
//! `bibo:Report`, data outputs on `dcterms:Dataset`) — never an invented
//! URI inside FIBO's namespace. Internal metric identifiers (used by the
//! companies server's concept cache and financial model) are plain
//! hKask-internal keys defined in that server, not ontology URIs.
//!
//! Reference: EDM Council / OMG, Financial Industry Business Ontology.
//! <https://spec.edmcouncil.org/fibo/> — source repository
//! <https://github.com/EDMCouncil/FIBO> (master, fetched 2026-08-29).
//!
//! Pattern: thin mapping layer — canonical URI constants, no
//! dependencies, no reasoners, no overhead. Mirrors the dc_bibo, pko,
//! golem, and sepio modules in this crate.

/// A FIBO concept URI (prefixed canonical form, e.g. `fibo-be-le-cb:Corporation`).
pub type FiboConcept = &'static str;

/// Defines the vocabulary constants and registers every one in `ALL_TERMS`,
/// so the fixture test covers each constant by construction.
macro_rules! fibo_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: FiboConcept = $uri;)*

        /// Every term in this module. The fixture test asserts each appears
        /// in the official FIBO term list — a fabricated URI cannot pass.
        /// New terms must go through this macro.
        pub const ALL_TERMS: &[FiboConcept] = &[$($name),*];
    };
}

fibo_terms! {
    /// A corporation — a legal entity that is formally incorporated.
    /// FIBO: BE/LegalEntities/CorporateBodies.
    CORPORATION = "fibo-be-le-cb:Corporation",

    /// A ticker symbol identifying a listed security.
    /// FIBO: SEC/Securities/SecuritiesIdentification.
    TICKER_SYMBOL = "fibo-sec-sec-id:TickerSymbol",

    /// A portfolio — a collection of holdings treated as a unit.
    /// FIBO: SEC/Securities/SecurityAssets.
    PORTFOLIO = "fibo-sec-sec-ast:Portfolio",

    /// The market capitalization of a security.
    /// FIBO: IND/MarketIndices/BasketIndices.
    MARKET_CAPITALIZATION = "fibo-ind-mkt-bas:MarketCapitalization",

    /// The internal rate of return of a financial instrument.
    /// FIBO: FBC/FinancialInstruments/InstrumentPricing.
    INTERNAL_RATE_OF_RETURN = "fibo-fbc-fi-ip:InternalRateOfReturn",

    /// A consumer price index — the headline inflation measure.
    /// FIBO: IND/EconomicIndicators.
    CONSUMER_PRICE_INDEX = "fibo-ind-ei-ei:ConsumerPriceIndex",

    /// A producer price index.
    /// FIBO: IND/EconomicIndicators.
    PRODUCER_PRICE_INDEX = "fibo-ind-ei-ei:ProducerPriceIndex",

    /// Gross domestic product.
    /// FIBO: IND/EconomicIndicators.
    GROSS_DOMESTIC_PRODUCT = "fibo-ind-ei-ei:GrossDomesticProduct",

    /// An economic indicator — the FIBO cover for indicator families FIBO
    /// does not model individually (e.g. commodity price indices).
    /// FIBO: IND/EconomicIndicators.
    ECONOMIC_INDICATOR = "fibo-ind-ei-ei:EconomicIndicator",

    /// A reference index — the FIBO cover for market/asset price indices
    /// FIBO does not model individually (e.g. crypto price indices).
    /// FIBO: IND/MarketIndices/BasketIndices.
    REFERENCE_INDEX = "fibo-ind-mkt-bas:ReferenceIndex",

    /// A reference interest rate set by an authority — the FIBO cover for
    /// central bank policy rates (Fed funds, ECB refi, BoE bank rate).
    /// FIBO: IND/InterestRates.
    REFERENCE_INTEREST_RATE = "fibo-ind-ir-ir:ReferenceInterestRate",

    /// An interest rate benchmark — the FIBO cover for yields at a
    /// specific maturity (e.g. Treasury yields).
    /// FIBO: IND/InterestRates.
    INTEREST_RATE_BENCHMARK = "fibo-ind-ir-ir:InterestRateBenchmark",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fabrication guard: every term in this module must appear in the
    /// official FIBO term list checked in as a fixture (source URL and
    /// fetch date in the fixture header). A term that is not in the
    /// published ontology fails here — pin tests on the constants alone
    /// cannot catch a plausible-looking invented URI.
    #[test]
    fn all_terms_are_official() {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/fibo-verified-terms.txt"
        );
        let fixture = std::fs::read_to_string(fixture_path)
            .unwrap_or_else(|e| panic!("failed to read {fixture_path}: {e}"));
        let official: std::collections::HashSet<&str> = fixture
            .lines()
            .map(|line| line.split('\t').next().unwrap_or("").trim())
            .filter(|term| !term.is_empty() && !term.starts_with('#'))
            .collect();
        assert!(
            !official.is_empty(),
            "fixture {fixture_path} contains no terms"
        );
        for term in ALL_TERMS {
            assert!(
                official.contains(term),
                "{term} is not in the official FIBO term list ({fixture_path}) — \
                 it must be verified against https://spec.edmcouncil.org/fibo/ before use"
            );
        }
    }
}
