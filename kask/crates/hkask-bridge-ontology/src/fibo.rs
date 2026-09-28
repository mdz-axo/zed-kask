//! FIBO (Financial Industry Business Ontology) vocabulary bridge.
//!
//! The named constants are checked against the complete pinned EDMC FIBO
//! Q2 Release index. All Release modules are compiled into `published`, not
//! just the named constants. Provisional and Informative are not loaded.
//!
//! Historical verification (2026-08-29, FIBO master branch) found that 63 of the 70
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
//! <https://github.com/EDMCouncil/FIBO> (Q2 tag commit
//! f59157fe156e3d91b1c045222d0a7dc06b7d78a2).
//!
//! Pattern: named constant facade over the source-backed published index.

/// A FIBO concept URI (prefixed canonical form, e.g. `fibo-be-le-cb:Corporation`).
pub type FiboConcept = &'static str;

/// Defines named constants and registers every one in `ALL_TERMS`,
/// so the source-backed Release test covers each constant by construction.
macro_rules! fibo_terms {
    ($($(#[$doc:meta])* $name:ident = $uri:literal),* $(,)?) => {
        $($(#[$doc])* pub const $name: FiboConcept = $uri;)*

        /// Named FIBO terms used directly by callers; the compiled Release
        /// index contains all published terms, including those not named here.
        /// New named constants must go through this macro.
        #[cfg(test)]
        const ALL_TERMS: &[FiboConcept] = &[$($name),*];
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

    /// A term structure — a structured collection of rates or bond yields
    /// with different terms to maturity from which a yield curve may be
    /// constructed (FIBO's published definition, IND/Indicators L230,
    /// mechanically verified 2026-09-20, FIBO master).
    TERM_STRUCTURE = "fibo-ind-ind-ind:TermStructure",

    /// An interest rate — the amount charged, expressed as a percentage of
    /// principal, in exchange for the use of assets (FIBO's published
    /// definition, FND/Accounting/CurrencyAmount L258, mechanically verified
    /// 2026-09-20, FIBO master).
    INTEREST_RATE = "fibo-fnd-acc-cur:InterestRate",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Named constants remain a thin facade over the entire pinned Release.
    #[test]
    fn all_terms_are_official() {
        for term in ALL_TERMS {
            let published = crate::published::get(term)
                .unwrap_or_else(|| panic!("{term} is not in the pinned FIBO Release"));
            assert_eq!(published.namespace, "FIBO");
            assert!(!published.source.is_empty());
        }
    }

    /// expect: the full Release inventory, not the named constants, is in
    /// the same published index used by term resolution; no provisional
    /// ontology leaks in through the production import list.
    #[test]
    fn release_is_complete_and_provisional_is_excluded() {
        let inventory = include_str!("../fibo-release-modules.tsv");
        let modules: std::collections::HashSet<&str> = inventory
            .lines()
            .filter_map(|line| line.strip_prefix("module\t"))
            .filter_map(|line| line.split('\t').next())
            .collect();
        assert_eq!(modules.len(), 157);
        let release: Vec<_> = crate::published::terms()
            .iter()
            .filter(|term| term.namespace == "FIBO")
            .collect();
        assert!(
            release.len() > 6_000,
            "Release vocabulary truncated: {}",
            release.len()
        );
        let indexed_modules: std::collections::HashSet<_> = release
            .iter()
            .filter_map(|term| term.source.split(" (EDMCouncil/").next())
            .collect();
        assert_eq!(
            indexed_modules, modules,
            "each Release module contributes terms"
        );
        assert!(release.iter().all(|term| {
            term.concept.starts_with("fibo-")
                && term
                    .source
                    .contains("f59157fe156e3d91b1c045222d0a7dc06b7d78a2")
        }));
        let omitted_from_production = crate::published::get("fibo-be-corp-corp:BoardAgreement")
            .expect("Release maturity, despite absence from AboutFIBOProd imports");
        assert_eq!(
            omitted_from_production.definition, "",
            "no publisher definition"
        );
        assert_eq!(omitted_from_production.status, "deprecated");
        let resolved = crate::term_resolution::resolve_term("fibo-be-corp-corp:BoardAgreement");
        assert_eq!(resolved.definition, None);
        assert_eq!(resolved.status.as_deref(), Some("deprecated"));
        assert_eq!(
            omitted_from_production.source.split(" (").next(),
            Some("BE/Corporations/Corporations.rdf")
        );
        assert!(
            !crate::published::contains("fibo-sec-fund-civ:AccumulatingShareClass"),
            "Provisional ontology entered Release index"
        );
        assert!(
            !crate::published::contains("fibo-fbc-fct-mkti:Facility-21XX"),
            "production import without Release maturity entered index"
        );
    }
}
