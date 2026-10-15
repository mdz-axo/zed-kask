//! Hard data-validation gates for provider-served financial data.
//!
//! Distinct from [`crate::data_quality`] (which scores confidence in
//! derived metrics): these are named flags for internally inconsistent
//! provider rows, attached loudly so downstream analysis cannot silently
//! consume garbled data. Flagged rows are kept and named — never silently
//! dropped.
//!
//! Every flag carries the live receipt that produced it (observed
//! 2026-10-14 against the EODHD/FMP surfaces) and a unit test pinning that
//! exact receipt as a positive fixture — a gate without a fixture reads
//! "0 caught" while broken.

/// Income-statement revenue below the same row's interest income —
/// internally impossible (interest income is a component of operating
/// revenue for any reporter that carries the line), so the row's growth
/// leg is unusable. Live receipt: IDCBY FY2024 revenue 660.387B CNY vs
/// interestIncome 1,427.948B CNY (2026-10-14 pull), which fed a phantom
/// 125% revenue "jump" into a growth benchmark.
pub(crate) const FLAG_REVENUE_BELOW_INTEREST: &str = "revenue_below_interest_income";

/// Screener dividend yield above 25% — no common stock pays that; the
/// provider has mixed units (fraction vs percent) or the line is not a
/// common stock. Live receipts: CICHF 0.3513, IDCBF 0.3513, PCCYF 0.4113,
/// BABAF 0.5277 (2026-10-14 pulls).
pub(crate) const FLAG_DIVIDEND_YIELD_IMPLAUSIBLE: &str = "dividend_yield_implausible";

/// Screener volume series internally inconsistent: nonzero 1-day average
/// volume with a zero 200-day average — a 200-day average of zero while
/// current daily volume is nonzero is impossible. Live receipts: SPCX
/// (avgvol_1d 68,780,976 / avgvol_200d 0), SKHY (22,529,628 / 0). A real
/// listing with missing volume history flags too — correct: the volume
/// data is unusable either way, and the flag says so.
pub(crate) const FLAG_VOLUME_SERIES_INCONSISTENT: &str = "volume_series_inconsistent";

/// Screener row cap above the venue's plausibility ceiling — the row's
/// magnitude exceeds any genuine issuer on that venue (venue-currency
/// units, same-currency rows only). Live receipts (2026-10-14): LSE
/// £2.18T (MOL) and £1.49T (Gedeon Richter) — HUF magnitudes labeled £,
/// while the largest genuine UK lines run £222-243B (Shell, HSBC); MX
/// ₱101T for NVDA — a US row mechanically re-denominated
/// (NVDA $221.30 × 18.4815 MXN/USD).
pub(crate) const FLAG_CAP_ABOVE_VENUE_CEILING: &str = "cap_above_venue_ceiling";

/// LSE international line carrying the £ symbol: genuine UK lines quote
/// in pence ("p" — HSBC, Shell, AstraZeneca, Rio Tinto, BP, GSK); the £
/// symbol marks international lines whose cap magnitudes are denominated
/// in the issuer's LOCAL currency (live receipts 2026-10-14: MOL HUF,
/// CEZ CZK, Geely HKD magnitudes, all labeled £) — the GBP-rate USD
/// conversion is unreliable for this class.
pub(crate) const FLAG_CAP_DENOMINATION_UNRELIABLE: &str = "cap_denomination_unreliable";

/// The exchange registry for the screen's standard universe — the
/// reconciliation contract: code → venue, quotation currency, quotation
/// symbols, expected issuer domicile, and the venue-currency cap
/// plausibility ceiling. Reconciled against live pulls 2026-10-14
/// (US/TO/MX/LSE top rows; receipts on each gate).
pub(crate) struct ExchangeVenue {
    pub code: &'static str,
    pub venue: &'static str,
    pub quotation_currency: &'static str,
    pub quotation_symbols: &'static [&'static str],
    pub expected_domicile: &'static str,
    /// Plausibility ceiling in venue-currency units (2x+ margin over the
    /// largest genuine venue cap observed live). Same-currency rows above
    /// it flag [`FLAG_CAP_ABOVE_VENUE_CEILING`] — advisory; rows kept.
    pub cap_ceiling: Option<f64>,
}

pub(crate) const EXCHANGE_REGISTRY: &[ExchangeVenue] = &[
    ExchangeVenue {
        code: "US",
        venue: "United States composite (NYSE/NASDAQ)",
        quotation_currency: "USD",
        quotation_symbols: &["$"],
        expected_domicile: "United States (ADRs: foreign issuers' US surface)",
        // Largest genuine venue cap observed live: NVDA $5.5T (2026-10-14).
        cap_ceiling: Some(1.0e13),
    },
    ExchangeVenue {
        code: "TO",
        venue: "Toronto Stock Exchange",
        quotation_currency: "CAD",
        quotation_symbols: &["C$"],
        expected_domicile: "Canada",
        // Largest genuine venue cap observed live: RY C$384B (2026-10-14).
        cap_ceiling: Some(1.0e12),
    },
    ExchangeVenue {
        code: "MX",
        venue: "Bolsa Mexicana de Valores",
        quotation_currency: "MXN",
        quotation_symbols: &["₱"],
        expected_domicile: "Mexico",
        // The venue is excluded upstream for Mexican-issuer screening
        // (analysis::UNRELIABLE_VENUES — Mexican-market lines of foreign
        // shares, Bolsa-only issuers absent); this ceiling catches
        // foreign-share lines on leak paths (no-exchange fetches).
        cap_ceiling: Some(2.0e12),
    },
    ExchangeVenue {
        code: "LSE",
        venue: "London Stock Exchange (mixed-currency: international lines quoted in their own currency)",
        quotation_currency: "GBP",
        // Genuine UK lines quote in pence ("p"); "£" marks international
        // lines (see FLAG_CAP_DENOMINATION_UNRELIABLE).
        quotation_symbols: &["£", "p"],
        expected_domicile: "United Kingdom (international lines: issuer domicile)",
        // Largest genuine UK cap observed live: HSBC £243B (2026-10-14).
        cap_ceiling: Some(5.0e11),
    },
];

pub(crate) fn venue_for(code: &str) -> Option<&'static ExchangeVenue> {
    EXCHANGE_REGISTRY.iter().find(|venue| venue.code == code)
}

/// Dividend yields above this fraction are flagged. No common stock pays
/// above 25%; the observed provider garbage ran 32–53%.
const DIVIDEND_YIELD_CEILING: f64 = 0.25;

/// A strictly positive finite field value, or `None` (zero/negative/absent
/// fields never gate).
fn positive_f64(row: &serde_json::Value, field: &str) -> Option<f64> {
    let value = row.get(field)?.as_f64()?;
    (value > 0.0).then_some(value)
}

/// Named data-quality flags for one income-statement row (provider JSON).
pub(crate) fn income_statement_row_flags(row: &serde_json::Value) -> Vec<&'static str> {
    let mut flags = Vec::new();
    match (
        positive_f64(row, "revenue"),
        positive_f64(row, "interestIncome"),
    ) {
        (Some(revenue), Some(interest)) if revenue < interest => {
            flags.push(FLAG_REVENUE_BELOW_INTEREST);
        }
        _ => {}
    }
    flags
}

/// Named data-quality flags for one screener row (provider JSON).
pub(crate) fn screener_row_flags(row: &serde_json::Value) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if let Some(yield_value) = row.get("dividend_yield").and_then(|value| value.as_f64())
        && yield_value > DIVIDEND_YIELD_CEILING
    {
        flags.push(FLAG_DIVIDEND_YIELD_IMPLAUSIBLE);
    }
    if let (Some(one_day), Some(two_hundred_day)) = (
        row.get("avgvol_1d").and_then(|value| value.as_f64()),
        row.get("avgvol_200d").and_then(|value| value.as_f64()),
    ) && one_day > 0.0
        && two_hundred_day == 0.0
    {
        flags.push(FLAG_VOLUME_SERIES_INCONSISTENT);
    }
    // Venue reconciliation gates (EXCHANGE_REGISTRY above).
    if let Some(code) = row.get("exchange").and_then(|value| value.as_str())
        && let Some(venue) = venue_for(code)
    {
        let symbol = row
            .get("currency_symbol")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        // Ceiling applies to same-currency rows only — a foreign-currency
        // line's cap is in its own currency and no venue ceiling applies.
        if venue.quotation_symbols.contains(&symbol)
            && let Some(ceiling) = venue.cap_ceiling
            && let Some(cap) = positive_f64(row, "market_capitalization")
            && cap > ceiling
        {
            flags.push(FLAG_CAP_ABOVE_VENUE_CEILING);
        }
        // LSE international lines: the £ symbol marks local-currency
        // magnitudes (genuine UK lines quote in pence).
        if code == "LSE" && symbol == "£" {
            flags.push(FLAG_CAP_DENOMINATION_UNRELIABLE);
        }
    }
    flags
}

/// Annotate an income-statement response (a JSON array of period rows) in
/// place: each flagged row gains a `data_quality_flags` array naming its
/// flags. Additive only — no row is dropped, no existing field is
/// overwritten. Returns the number of flagged rows. Non-array input is
/// left untouched (returns 0).
pub(crate) fn annotate_income_statement_rows(rows: &mut serde_json::Value) -> usize {
    let Some(periods) = rows.as_array_mut() else {
        return 0;
    };
    let mut flagged = 0;
    for period in periods.iter_mut() {
        let flags = income_statement_row_flags(period);
        if flags.is_empty() {
            continue;
        }
        flagged += 1;
        if let Some(object) = period.as_object_mut() {
            object.insert(
                "data_quality_flags".to_string(),
                serde_json::Value::Array(
                    flags
                        .into_iter()
                        .map(|flag| serde_json::Value::String(flag.to_string()))
                        .collect(),
                ),
            );
        }
    }
    flagged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Positive fixture: the exact IDCBY FY2024 receipt (2026-10-14 live
    /// pull) — revenue below the same row's interest income.
    #[test]
    fn revenue_below_interest_income_flags_idcby_fy2024_receipt() {
        let row = json!({
            "date": "2024-12-31",
            "symbol": "IDCBY",
            "reportedCurrency": "CNY",
            "revenue": 660_387_000_000.0,
            "interestIncome": 1_427_948_000_000.0,
            "netIncome": 365_863_000_000.0
        });
        assert_eq!(
            income_statement_row_flags(&row),
            vec![FLAG_REVENUE_BELOW_INTEREST]
        );
    }

    /// Negative fixture: NVDA FY2026 receipt — clean row, no flags.
    #[test]
    fn clean_income_statement_row_is_unflagged() {
        let row = json!({
            "date": "2026-01-25",
            "symbol": "NVDA",
            "reportedCurrency": "USD",
            "revenue": 215_938_000_000.0,
            "interestIncome": 2_300_000_000.0,
            "netIncome": 120_067_000_000.0
        });
        assert!(income_statement_row_flags(&row).is_empty());
    }

    /// Zero or absent fields never gate (a missing interestIncome line is
    /// normal for non-financials).
    #[test]
    fn absent_interest_income_never_gates() {
        let row = json!({
            "date": "2025-12-31",
            "symbol": "TSLA",
            "revenue": 94_827_000_000.0
        });
        assert!(income_statement_row_flags(&row).is_empty());
    }

    /// Positive fixture: CICHF dividend "yield" 0.3513 (2026-10-14 pull).
    #[test]
    fn dividend_yield_implausible_flags_cichf_receipt() {
        let row = json!({
            "code": "CICHF",
            "exchange": "US",
            "dividend_yield": 0.3513
        });
        assert_eq!(
            screener_row_flags(&row),
            vec![FLAG_DIVIDEND_YIELD_IMPLAUSIBLE]
        );
    }

    /// Negative fixture: KO 0.0231 — a real yield, unflagged.
    #[test]
    fn plausible_dividend_yield_is_unflagged() {
        let row = json!({
            "code": "KO",
            "exchange": "US",
            "dividend_yield": 0.0231
        });
        assert!(screener_row_flags(&row).is_empty());
    }

    /// Positive fixture: SPCX avgvol_1d 68,780,976 / avgvol_200d 0
    /// (2026-10-14 pull — a private company served in the listings
    /// universe with impossible volume series).
    #[test]
    fn volume_series_inconsistent_flags_spcx_receipt() {
        let row = json!({
            "code": "SPCX",
            "exchange": "US",
            "avgvol_1d": 68_780_976.0,
            "avgvol_200d": 0.0
        });
        assert_eq!(
            screener_row_flags(&row),
            vec![FLAG_VOLUME_SERIES_INCONSISTENT]
        );
    }

    /// Negative fixture: NVDA volumes — consistent series, unflagged.
    #[test]
    fn consistent_volume_series_is_unflagged() {
        let row = json!({
            "code": "NVDA",
            "exchange": "US",
            "avgvol_1d": 118_730_083.0,
            "avgvol_200d": 151_030_831.26
        });
        assert!(screener_row_flags(&row).is_empty());
    }

    /// Both screener flags on one row are reported together.
    #[test]
    fn screener_row_reports_all_applicable_flags() {
        let row = json!({
            "code": "GARBLED",
            "exchange": "MX",
            "dividend_yield": 0.5277,
            "avgvol_1d": 1_000_000.0,
            "avgvol_200d": 0.0
        });
        assert_eq!(
            screener_row_flags(&row),
            vec![
                FLAG_DIVIDEND_YIELD_IMPLAUSIBLE,
                FLAG_VOLUME_SERIES_INCONSISTENT
            ]
        );
    }

    /// The annotate helper: only the garbled row gains `data_quality_flags`;
    /// clean rows are untouched (no field added); the count is returned.
    #[test]
    fn annotate_income_statement_rows_flags_only_garbled_rows() {
        let mut rows = json!([
            {
                "date": "2024-12-31",
                "symbol": "IDCBY",
                "revenue": 660_387_000_000.0,
                "interestIncome": 1_427_948_000_000.0
            },
            {
                "date": "2025-12-31",
                "symbol": "IDCBY",
                "revenue": 1_488_798_518_000.0,
                "interestIncome": 1_298_930_373_000.0
            }
        ]);
        let flagged = annotate_income_statement_rows(&mut rows);
        assert_eq!(flagged, 1);
        let periods = rows.as_array().expect("array preserved");
        assert_eq!(
            periods[0]["data_quality_flags"],
            json!([FLAG_REVENUE_BELOW_INTEREST])
        );
        assert!(periods[1].get("data_quality_flags").is_none());
    }

    /// Non-array responses pass through untouched (the gate never breaks
    /// the provider passthrough on unexpected shapes).
    #[test]
    fn annotate_income_statement_rows_passes_non_arrays_through() {
        let mut rows = json!({"error": "provider shape change"});
        assert_eq!(annotate_income_statement_rows(&mut rows), 0);
        assert_eq!(rows, json!({"error": "provider shape change"}));
    }

    /// Positive fixture: the exact MOL 0RUK receipt (2026-10-14 live pull) —
    /// an LSE £-symbol line with a £2.18T cap (HUF magnitude), 10x the
    /// largest genuine UK cap. Both venue gates fire.
    #[test]
    fn venue_ceiling_and_denomination_flags_mol_lse_receipt() {
        let row = json!({
            "code": "0RUK", "name": "MOL Hungarian Oil & Gas PLC",
            "exchange": "LSE", "currency_symbol": "£",
            "market_capitalization": 2_181_309_000_000.0
        });
        assert_eq!(
            screener_row_flags(&row),
            vec![
                FLAG_CAP_ABOVE_VENUE_CEILING,
                FLAG_CAP_DENOMINATION_UNRELIABLE
            ]
        );
    }

    /// Positive fixture: the CEZ 0NZF receipt (2026-10-14) — £452B, a CZK
    /// magnitude below the ceiling but on a £-symbol international line:
    /// the denomination gate catches what the ceiling cannot.
    #[test]
    fn denomination_gate_flags_cez_below_ceiling() {
        let row = json!({
            "code": "0NZF", "name": "CEZ as",
            "exchange": "LSE", "currency_symbol": "£",
            "market_capitalization": 451_911_400_000.0
        });
        assert_eq!(
            screener_row_flags(&row),
            vec![FLAG_CAP_DENOMINATION_UNRELIABLE]
        );
    }

    /// Negative fixture: HSBC's genuine LSE line (2026-10-14) — pence
    /// symbol, £243B: no flags. Genuine UK lines quote in "p".
    #[test]
    fn genuine_uk_pence_line_is_unflagged() {
        let row = json!({
            "code": "HSBA", "name": "HSBC Holdings PLC",
            "exchange": "LSE", "currency_symbol": "p",
            "market_capitalization": 243_157_303_296.0
        });
        assert!(screener_row_flags(&row).is_empty());
    }

    /// Negative fixture: Toyota's LSE ¥ line (2026-10-14) — a correctly
    /// labeled foreign-currency line (¥35.4T); no venue ceiling applies
    /// and the ¥ symbol is not the £ international-line marker.
    #[test]
    fn jpy_line_on_lse_is_unflagged() {
        let row = json!({
            "code": "TYT", "name": "Toyota Motor Corp",
            "exchange": "LSE", "currency_symbol": "¥",
            "market_capitalization": 35_383_400_202_240.0
        });
        assert!(screener_row_flags(&row).is_empty());
    }

    /// Positive fixture: the NVDA MX mirror receipt (2026-10-14) — ₱101T,
    /// a US row re-denominated at the MXN rate. The venue is excluded
    /// upstream; this ceiling catches mirror rows on leak paths.
    #[test]
    fn venue_ceiling_flags_nvda_mx_mirror_receipt() {
        let row = json!({
            "code": "NVDA", "name": "NVIDIA Corporation",
            "exchange": "MX", "currency_symbol": "₱",
            "market_capitalization": 101_112_422_072_320.0
        });
        assert_eq!(screener_row_flags(&row), vec![FLAG_CAP_ABOVE_VENUE_CEILING]);
    }

    /// The registry reconciliation: every entry carries the full contract
    /// (venue, currency, symbols, domicile), every ceiling is set and
    /// positive, and the LSE symbol set carries the live-observed split
    /// (pence for genuine UK lines, £ for international lines).
    #[test]
    fn exchange_registry_contract_is_complete() {
        assert!(EXCHANGE_REGISTRY.len() >= 4);
        for venue in EXCHANGE_REGISTRY {
            assert!(!venue.venue.is_empty(), "{} venue", venue.code);
            assert!(
                !venue.quotation_currency.is_empty(),
                "{} currency",
                venue.code
            );
            assert!(
                !venue.quotation_symbols.is_empty(),
                "{} symbols",
                venue.code
            );
            assert!(
                !venue.expected_domicile.is_empty(),
                "{} domicile",
                venue.code
            );
            let ceiling = venue.cap_ceiling.expect("{} ceiling set");
            assert!(ceiling > 0.0);
        }
        let lse = venue_for("LSE").expect("LSE registered");
        assert!(lse.quotation_symbols.contains(&"p"));
        assert!(lse.quotation_symbols.contains(&"£"));
        // JP is deliberately absent — EODHD has no Japanese screener
        // exchange (the typed error names it); Japanese issuers enter via
        // LSE ¥ lines.
        assert!(venue_for("JP").is_none());
    }
}
