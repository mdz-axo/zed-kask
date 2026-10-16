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

}
