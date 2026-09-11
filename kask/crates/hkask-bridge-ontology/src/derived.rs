//! Derived concepts — the derived rung of the fallback ladder (P8.3):
//! recorded compositions over anchored constituents, sitting between the
//! domain supplements (rung 1) and the SUMO upper ontology (rung 3) in
//! term resolution (operator ruling 2026-09-10).
//!
//! A derived concept is NOT a fabricated ontology URI: it is a recorded
//! composition whose identity is machine-checkable and whose authority is
//! cited (an operator ruling with its date, or a published standard).
//! Constituents resolve through the ladder themselves — published term,
//! another derived concept, or a ruling — so a derived entry never
//! fabricates a constituent anchor.
//!
//! The ladder invariant (axis.rs P8.3): nothing is ever untagged. A term
//! with no published anchor resolves here; a term with no derived entry
//! either is pending its first ruling (a routing state, never a terminal
//! verdict) or falls to the upper/general layers. "Unanchored" as a
//! terminal state is the failure mode this registry exists to close
//! (observed 2026-09-10: "net margin" — the most-used term in the
//! financial-analysis vocabulary — resolved to a void because FIBO
//! publishes no ratio terms, and a pre-interest operating formula wore
//! the label through three operator corrections).

/// A derived concept: a recorded composition over anchored constituents.
pub struct DerivedConcept {
    /// Canonical term name (snake_case).
    pub term: &'static str,
    /// Resolution aliases (spaced and hyphenated forms resolve via
    /// normalization; list only genuinely different words).
    pub aliases: &'static [&'static str],
    /// The machine-checkable identity — the derivation the term denotes.
    pub identity: &'static str,
    /// The load-bearing semantics, stated so a violation is nameable.
    pub definition: &'static str,
    /// Constituent term names, each resolved through the ladder itself.
    pub constituents: &'static [&'static str],
    /// The authority record: an operator ruling (with date) or a
    /// published standard. An entry with no resolvable authority fails
    /// the build (same discipline as the FIBO fixture).
    pub authority: &'static str,
}

/// The derived-concept registry. Seeded from the operator rulings of
/// 2026-09-10 (the DuPont capability envelope and the expectations-gap
/// definition) — the session where every one of these terms was
/// contested, corrected, and ratified.
pub const DERIVED_CONCEPTS: &[DerivedConcept] = &[
    DerivedConcept {
        term: "net_margin",
        aliases: &["net profit margin", "npm"],
        identity: "net income / revenue",
        definition: "Net income — post-interest, post-tax, the equity holder's claim — divided by revenue. Never a pre-interest operating formula: interest is the line where debt holders are paid first, and omitting it removes exactly what makes net income the equity holder's number.",
        constituents: &["net income", "revenue"],
        authority: "operator ruling 2026-09-10 (stated three times)",
    },
    DerivedConcept {
        term: "gross_margin",
        aliases: &["gross profit margin"],
        identity: "(revenue - cost of revenue) / revenue",
        definition: "Gross profit over revenue. Safe for enterprise-value calculations in acquisition scenarios where fixed costs will be restructured; never a reported expectations quantity for equity value.",
        constituents: &["revenue", "cost of revenue"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "operating_margin",
        aliases: &["operating profit margin"],
        identity: "operating income / revenue",
        definition: "Operating income (EBIT) over revenue — pre-interest, pre-tax. A margin of the enterprise, not of the equity holder.",
        constituents: &["operating income", "revenue"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "return_on_equity",
        aliases: &["roe"],
        identity: "net income / equity = net margin x asset turnover x equity multiplier",
        definition: "The DuPont identity: ROE is the product of net profit margin, asset turnover, and the equity multiplier. The identity holds per period; medians hold approximately. The headline profitability measure for financial-sector companies.",
        constituents: &[
            "net income",
            "equity",
            "net margin",
            "asset turnover",
            "equity multiplier",
        ],
        authority: "operator ruling 2026-09-10 (DuPont analysis)",
    },
    DerivedConcept {
        term: "asset_turnover",
        aliases: &["total asset turnover"],
        identity: "revenue / total assets",
        definition: "Revenue generated per unit of total assets — the efficiency component of the DuPont decomposition.",
        constituents: &["revenue", "total assets"],
        authority: "operator ruling 2026-09-10 (DuPont analysis)",
    },
    DerivedConcept {
        term: "equity_multiplier",
        aliases: &["financial leverage", "leverage multiplier"],
        identity: "total assets / equity",
        definition: "Total assets over equity — the leverage component of the DuPont decomposition.",
        constituents: &["total assets", "equity"],
        authority: "operator ruling 2026-09-10 (DuPont analysis)",
    },
    DerivedConcept {
        term: "retention",
        aliases: &["retention ratio", "plowback ratio"],
        identity: "1 - dividends paid / net income, clamped to [0, 1]",
        definition: "The share of earnings retained in the business. A year paying dividends above earnings demonstrates zero retained funding, never negative — the clamp is part of the identity.",
        constituents: &["dividends paid", "net income"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "sustainable_growth_rate",
        aliases: &["sgr", "self-funding growth rate"],
        identity: "return on equity x retention",
        definition: "The Higgins sustainable growth rate: the growth a company can self-fund from internally generated earnings without external financing. The demonstrated-capability anchor of the growth leg of the expectations gap.",
        constituents: &["return on equity", "retention"],
        authority: "operator ruling 2026-09-10; Higgins (1977), 'Financial Management'",
    },
    DerivedConcept {
        term: "price_to_book",
        aliases: &["p/b", "pb ratio", "market to book"],
        identity: "price per share / book value per share",
        definition: "Market price over book equity per share. The equity-based valuation surface for financial-sector companies whose FCF is not meaningful.",
        constituents: &["book value per share"],
        authority: "operator ruling 2026-09-10",
    },
    DerivedConcept {
        term: "implied_roe",
        aliases: &["market implied roe"],
        identity: "price_to_book x (cost of equity - growth) + growth",
        definition: "The ROE the price demands, from the justified price-to-book identity P/B = (ROE - g)/(COE - g) — residual income on equity, the standard reverse solve for financial-sector companies.",
        constituents: &["price to book", "cost of equity"],
        authority: "operator ruling 2026-09-10; Damodaran, Applied Corporate Finance, Ch. 19",
    },
];

/// Resolve a term (or alias) against the derived registry.
/// Normalization matches the onto_anchor tool's: lowercase alphanumeric
/// characters only, separators and case folded away.
pub fn resolve_derived(term: &str) -> Option<&'static DerivedConcept> {
    let normalized: String = term
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    DERIVED_CONCEPTS.iter().find(|concept| {
        normalize(concept.term) == normalized
            || concept.aliases.iter().any(|a| normalize(a) == normalized)
    })
}

fn normalize(term: &str) -> String {
    term.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// expect: [P5] The contested term of the 2026-09-10 session resolves
    /// as a derived concept with its identity and authority — never a void.
    #[test]
    fn net_margin_resolves_with_identity_and_authority() {
        let concept = resolve_derived("net margin").expect("net margin is defined");
        assert_eq!(concept.term, "net_margin");
        assert_eq!(concept.identity, "net income / revenue");
        assert!(
            concept.definition.contains("post-interest"),
            "the definition carries the load-bearing semantics: {}",
            concept.definition
        );
        assert!(concept.authority.contains("2026-09-10"));
    }

    /// expect: [P5] Aliases and separator variants resolve to the same
    /// concept.
    #[test]
    fn aliases_and_separators_resolve() {
        for term in [
            "net_margin",
            "Net Margin",
            "net profit margin",
            "npm",
            "roe",
            "return on equity",
            "sustainable-growth-rate",
            "sgr",
        ] {
            assert!(
                resolve_derived(term).is_some(),
                "{term} must resolve — nothing is undefined"
            );
        }
    }

    /// expect: [P5] Every derived entry cites an authority — an entry with
    /// no authority citation is a fabricated definition, the exact disease
    /// this registry replaces.
    #[test]
    fn every_entry_cites_authority() {
        for concept in DERIVED_CONCEPTS {
            assert!(
                !concept.authority.is_empty(),
                "{} must cite its authority",
                concept.term
            );
        }
    }
}
