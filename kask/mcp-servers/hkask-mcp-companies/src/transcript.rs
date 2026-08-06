//! FMP earnings-call transcript fetch — `company_transcript` (earnings mode).
//!
//! Design: `kask/docs/explanation/earnings-transcript-analysis-design.md` §(a).
//! Fetches FMP `/stable/earning-call-transcript` for a window of quarters.
//! Coverage-honest: per-quarter failures collected into `coverage.missing`,
//! not propagated as whole-tool failure; the tool fails only when zero
//! quarters succeed.
//!
//! Probe-verified (earnings doc A2, 2026-08-05):
//! - Endpoint: `GET /stable/earning-call-transcript?symbol=&year=&quarter=`
//! - Response: array of `{symbol, period, year, date, content}`; empty `[]` = no call.
//! - `date` is UNRELIABLE (AAPL 2023Q1 → `date: "2012-03-19"`). `(year, quarter)`
//!   is the temporal key, never `date`.
//! - Legacy v3 endpoint is 403 — `/stable/` only.

use hkask_mcp_server::server::{McpToolError, classify_http_error};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const FMP_TRANSCRIPT_PATH: &str = "/earning-call-transcript";

// ── Public types (the result shape) ────────────────────────────────────────

/// One fetched earnings-call transcript. `(year, quarter)` is the canonical
/// temporal key; `date` is display-only (probe-verified unreliable).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptRecord {
    pub symbol: String,
    pub year: u32,
    pub quarter: u8,
    /// FMP `period` label verbatim (e.g. `"Q1"`). Diagnostics only.
    pub period: String,
    /// FMP `date` field verbatim. UNRELIABLE — display only.
    pub date: String,
    pub content: String,
    /// Provenance for the corpus pipeline's `dc:source`.
    pub source_endpoint: String,
}

/// Why a requested quarter is missing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MissingReason {
    /// FMP returned an empty array — no call that quarter.
    NoCall,
    /// FMP returned a non-2xx status.
    HttpError { status: u16, message: String },
    /// FMP returned 2xx but the body could not be parsed.
    ParseError { message: String },
}

/// One entry in `coverage.missing`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MissingQuarter {
    pub year: u32,
    pub quarter: u8,
    pub reason: MissingReason,
}

/// Coverage accounting — the honesty surface. Gaps reported, never filled.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptCoverage {
    pub requested_quarters: u32,
    pub retrieved_quarters: u32,
    pub missing: Vec<MissingQuarter>,
}

/// The full `company_transcript` result envelope.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TranscriptResult {
    pub transcripts: Vec<TranscriptRecord>,
    pub coverage: TranscriptCoverage,
}

// ── Quarter arithmetic ──────────────────────────────────────────────────────

/// A `(year, quarter)` pair — the temporal key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YearQuarter {
    pub year: u32,
    pub quarter: u8,
}

impl YearQuarter {
    pub fn new(year: u32, quarter: u8) -> Option<Self> {
        if !(1..=4).contains(&quarter) {
            return None;
        }
        Some(Self { year, quarter })
    }

    /// The previous calendar quarter. `None` only on year-0 underflow.
    pub fn previous(self) -> Option<Self> {
        if self.quarter == 1 {
            self.year
                .checked_sub(1)
                .map(|year| Self { year, quarter: 4 })
        } else {
            Some(Self {
                year: self.year,
                quarter: self.quarter - 1,
            })
        }
    }

    /// The `quarters_back` quarters ending at `self` (inclusive), most-recent first.
    /// Stops early if `year` underflows.
    pub fn window(self, quarters_back: u32) -> Vec<YearQuarter> {
        let mut out = Vec::with_capacity(quarters_back as usize);
        let mut current = Some(self);
        for _ in 0..quarters_back {
            let Some(q) = current else { break };
            out.push(q);
            current = q.previous();
        }
        out
    }
}

// ── Fetch + coverage ────────────────────────────────────────────────────────

/// Fetch a window of quarters ending at `end`, collecting per-quarter failures
/// into `coverage.missing`. The tool fails only when zero quarters succeed.
pub async fn fetch_transcript_window(
    client: &reqwest::Client,
    symbol: &str,
    end: YearQuarter,
    quarters_back: u32,
    fmp_api_key: &str,
) -> TranscriptResult {
    let window = end.window(quarters_back);
    let requested = window.len() as u32;
    let mut transcripts = Vec::new();
    let mut missing = Vec::new();

    for quarter in window {
        match fetch_one_quarter(client, symbol, quarter.year, quarter.quarter, fmp_api_key).await {
            Ok(Some(record)) => transcripts.push(record),
            Ok(None) => missing.push(MissingQuarter {
                year: quarter.year,
                quarter: quarter.quarter,
                reason: MissingReason::NoCall,
            }),
            Err(reason) => missing.push(MissingQuarter {
                year: quarter.year,
                quarter: quarter.quarter,
                reason,
            }),
        }
    }

    let retrieved_quarters = transcripts.len() as u32;
    TranscriptResult {
        transcripts,
        coverage: TranscriptCoverage {
            requested_quarters: requested,
            retrieved_quarters,
            missing,
        },
    }
}

/// Fetch one quarter from FMP. `Ok(None)` = empty array (no call); `Err` =
/// fetch/parse failure classified into a `MissingReason`.
async fn fetch_one_quarter(
    client: &reqwest::Client,
    symbol: &str,
    year: u32,
    quarter: u8,
    fmp_api_key: &str,
) -> Result<Option<TranscriptRecord>, MissingReason> {
    let url = format!("https://financialmodelingprep.com/stable{FMP_TRANSCRIPT_PATH}");
    let year_str = year.to_string();
    let quarter_str = quarter.to_string();
    let query: Vec<(&str, &str)> = vec![
        ("symbol", symbol),
        ("year", &year_str),
        ("quarter", &quarter_str),
        ("apikey", fmp_api_key),
    ];

    let response = client
        .get(&url)
        .query(&query)
        .send()
        .await
        .map_err(|error| MissingReason::HttpError {
            status: 0,
            message: format!("FMP transcript request failed: {error}"),
        })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| MissingReason::HttpError {
            status: status.as_u16(),
            message: format!("FMP transcript body read failed: {error}"),
        })?;

    if !status.is_success() {
        return Err(classify_fmp_status(status, &body));
    }
    if body.trim() == "[]" {
        return Ok(None);
    }
    parse_fmp_body(&body, symbol, year, quarter).map_err(|error| MissingReason::ParseError {
        message: error.to_string(),
    })
}

/// Classify an FMP non-2xx status into a `MissingReason`. 404 and empty-`[]`
/// bodies are `NoCall`; other statuses use `classify_http_error` for the message.
fn classify_fmp_status(status: reqwest::StatusCode, body: &str) -> MissingReason {
    if status.as_u16() == 404 || body.trim() == "[]" {
        MissingReason::NoCall
    } else {
        MissingReason::HttpError {
            status: status.as_u16(),
            message: classify_http_error("FMP", status, body).to_string(),
        }
    }
}

/// Parse the FMP response body for one quarter. The requested `year`/`quarter`
/// are authoritative for the temporal key (the FMP `date`/`year` labels are
/// unreliable). `year` may be a string or number in the response; both coerce.
fn parse_fmp_body(
    body: &str,
    symbol: &str,
    year: u32,
    quarter: u8,
) -> Result<Option<TranscriptRecord>, McpToolError> {
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct FmpEntry {
        symbol: String,
        period: String,
        year: serde_json::Value,
        date: String,
        content: String,
    }

    let entries: Vec<FmpEntry> = serde_json::from_str(body).map_err(|error| {
        McpToolError::unavailable(format!(
            "FMP transcript parse failed for {symbol} {year}Q{quarter}: {error}"
        ))
    })?;
    let Some(entry) = entries.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(TranscriptRecord {
        symbol: entry.symbol,
        year,
        quarter,
        period: entry.period,
        date: entry.date,
        content: entry.content,
        source_endpoint: format!(
            "fmp:/stable{FMP_TRANSCRIPT_PATH}?symbol={symbol}&year={year}&quarter={quarter}"
        ),
    }))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Quarter arithmetic ──────────────────────────────────────────────────

    #[test]
    fn year_quarter_rejects_invalid_quarter() {
        assert!(YearQuarter::new(2024, 0).is_none());
        assert!(YearQuarter::new(2024, 5).is_none());
        assert!(YearQuarter::new(2024, 1).is_some());
        assert!(YearQuarter::new(2024, 4).is_some());
    }

    #[test]
    fn previous_quarter_wraps_at_year_boundary() {
        let q1_2024 = YearQuarter::new(2024, 1).unwrap();
        assert_eq!(q1_2024.previous(), Some(YearQuarter::new(2023, 4).unwrap()));
        let q3_2024 = YearQuarter::new(2024, 3).unwrap();
        assert_eq!(q3_2024.previous(), Some(YearQuarter::new(2024, 2).unwrap()));
    }

    #[test]
    fn previous_quarter_underflows_at_year_zero() {
        assert_eq!(
            YearQuarter {
                year: 0,
                quarter: 1
            }
            .previous(),
            None
        );
    }

    #[test]
    fn window_iterates_most_recent_first() {
        let end = YearQuarter::new(2024, 2).unwrap();
        assert_eq!(
            end.window(5),
            vec![
                YearQuarter::new(2024, 2).unwrap(),
                YearQuarter::new(2024, 1).unwrap(),
                YearQuarter::new(2023, 4).unwrap(),
                YearQuarter::new(2023, 3).unwrap(),
                YearQuarter::new(2023, 2).unwrap(),
            ]
        );
    }

    #[test]
    fn window_stops_at_year_underflow() {
        assert_eq!(YearQuarter::new(0, 2).unwrap().window(10).len(), 2);
    }

    // ── FMP response parsing ────────────────────────────────────────────────

    const FMP_SAMPLE: &str = r#"[{"symbol":"AAPL","period":"Q1","year":"2023","date":"2012-03-19","content":"Operator: Good day. Tim Cook: Good morning."}]"#;

    #[test]
    fn parse_populated_quarter_yields_record() {
        let record = parse_fmp_body(FMP_SAMPLE, "AAPL", 2023, 1)
            .expect("parse")
            .expect("non-empty");
        assert_eq!(record.symbol, "AAPL");
        assert_eq!(record.year, 2023);
        assert_eq!(record.quarter, 1);
        assert_eq!(record.date, "2012-03-19"); // carried verbatim, unreliable
        assert!(record.content.contains("Tim Cook"));
    }

    #[test]
    fn parse_empty_array_yields_none() {
        assert!(
            parse_fmp_body("[]", "AAPL", 2005, 2)
                .expect("empty array is not an error")
                .is_none()
        );
    }

    #[test]
    fn parse_malformed_body_is_an_error() {
        assert!(parse_fmp_body("not json", "AAPL", 2023, 1).is_err());
    }

    #[test]
    fn parse_year_as_number_accepted() {
        let body =
            r#"[{"symbol":"MSFT","period":"Q4","year":2024,"date":"2024-07-30","content":"x"}]"#;
        let record = parse_fmp_body(body, "MSFT", 2024, 4)
            .expect("parse")
            .expect("record");
        assert_eq!(record.year, 2024);
    }

    // ── Error mapping ────────────────────────────────────────────────────────

    #[test]
    fn classify_404_is_no_call() {
        assert_eq!(
            classify_fmp_status(reqwest::StatusCode::NOT_FOUND, "Not Found"),
            MissingReason::NoCall
        );
    }

    #[test]
    fn classify_403_is_http_error() {
        let reason = classify_fmp_status(
            reqwest::StatusCode::FORBIDDEN,
            "Legacy Endpoint only available for legacy users",
        );
        assert!(matches!(
            reason,
            MissingReason::HttpError { status: 403, .. }
        ));
    }

    #[test]
    fn classify_429_is_http_error() {
        let reason = classify_fmp_status(reqwest::StatusCode::TOO_MANY_REQUESTS, "rate limited");
        assert!(matches!(
            reason,
            MissingReason::HttpError { status: 429, .. }
        ));
    }

    // ── Coverage accounting ──────────────────────────────────────────────────

    #[test]
    fn coverage_reports_gaps_not_silently_drops_them() {
        let coverage = TranscriptCoverage {
            requested_quarters: 3,
            retrieved_quarters: 1,
            missing: vec![
                MissingQuarter {
                    year: 2023,
                    quarter: 2,
                    reason: MissingReason::NoCall,
                },
                MissingQuarter {
                    year: 2023,
                    quarter: 1,
                    reason: MissingReason::HttpError {
                        status: 429,
                        message: "rate limited".to_string(),
                    },
                },
            ],
        };
        assert_eq!(coverage.missing.len(), 2);
        assert!(matches!(coverage.missing[0].reason, MissingReason::NoCall));
    }
}

// ── Property tests ──────────────────────────────────────────────────────────
//
// Integrates the hkask-test-harness in two ways:
// 1. `oracle_invariant` wraps the temporal-key contract so `harness-optimize`
//    sees structured oracle variety (not just inline `prop_assert!`).
// 2. `write_trace` emits a `TraceEntry` per proptest run to the trace filesystem
//    (resolved from `HKASK_TRACE_DIR`), so `harness-optimize` can see the runs.
//    Trace emission is best-effort: if `HKASK_TRACE_DIR` is unset, traces are
//    skipped (tests still run, just not recorded). This keeps tests green in
//    environments without the trace filesystem.

#[cfg(test)]
mod proptests {
    use super::*;
    use hkask_test_harness::{OracleVerdict, TraceEntry, oracle_invariant, write_trace};
    use proptest::prelude::*;
    use std::time::Instant;

    fn arb_year_quarter() -> impl Strategy<Value = YearQuarter> {
        (any::<u32>(), 1u8..=4).prop_map(|(year, quarter)| YearQuarter { year, quarter })
    }

    /// Resolve the trace dir from `HKASK_TRACE_DIR`. Returns `None` if unset —
    /// trace emission is skipped, not failed.
    fn trace_dir() -> Option<std::path::PathBuf> {
        std::env::var("HKASK_TRACE_DIR")
            .ok()
            .map(std::path::PathBuf::from)
    }

    /// Emit a trace entry for a proptest run. Best-effort: errors are logged to
    /// stderr but never fail the test.
    fn emit_trace(name: &str, result: &str, duration_ms: u64, oracle_type: &str) {
        let Some(dir) = trace_dir() else { return };
        let entry = TraceEntry {
            kind: "proptest".to_string(),
            name: name.to_string(),
            result: result.to_string(),
            duration_ms,
            shrunk_counterexample: String::new(),
            oracle_type: oracle_type.to_string(),
            metadata: serde_json::json!({
                "crate": "hkask-mcp-companies",
                "target": "transcript"
            }),
        };
        let run_id =
            std::env::var("HKASK_TRACE_RUN_ID").unwrap_or_else(|_| "standalone".to_string());
        if let Err(error) = write_trace(&dir, &run_id, &entry) {
            eprintln!("warn: trace emission failed for {name}: {error}");
        }
    }

    proptest! {
        /// P4 (panic-freedom): `previous()` never panics on any valid `YearQuarter`.
        #[test]
        fn previous_never_panics(q in arb_year_quarter()) {
            let start = Instant::now();
            let _ = q.previous();
            emit_trace(
                "previous_never_panics",
                "pass",
                start.elapsed().as_millis() as u64,
                "", // panic-freedom: no oracle
            );
        }

        /// P1 (invariant): `previous()` is None (year-0 only) or strictly earlier.
        #[test]
        fn previous_is_strictly_earlier_or_none(q in arb_year_quarter()) {
            match q.previous() {
                None => prop_assert_eq!(q.year, 0),
                Some(prev) => prop_assert!((prev.year, prev.quarter) < (q.year, q.quarter)),
            }
            emit_trace(
                "previous_is_strictly_earlier_or_none",
                "pass",
                0,
                "invariant",
            );
        }

        /// P1 (invariant): `window(n)` never exceeds `n` and is strictly decreasing.
        #[test]
        fn window_bounded_and_decreasing(end in arb_year_quarter(), n in 0u32..64) {
            let window = end.window(n);
            prop_assert!(window.len() as u32 <= n);
            for pair in window.windows(2) {
                prop_assert!((pair[0].year, pair[0].quarter) > (pair[1].year, pair[1].quarter));
            }
            emit_trace("window_bounded_and_decreasing", "pass", 0, "invariant");
        }

        /// P4 (panic-freedom): the parser never panics on arbitrary string input.
        #[test]
        fn parse_never_panics(body in r"\PC*") {
            let _ = parse_fmp_body(&body, "TEST", 2024, 1);
            emit_trace("parse_never_panics", "pass", 0, "");
        }

        /// P1 (invariant, temporal-key contract): when the parser returns
        /// Some(record), record.year/quarter equal the requested inputs, never
        /// FMP's labels. Uses `oracle_invariant` so `harness-optimize` sees
        /// structured oracle variety.
        #[test]
        fn parse_carries_requested_temporal_key(
            body_value in hkask_test_harness::arb_json_value(),
            requested_year in 2000u32..=2030,
            requested_quarter in 1u8..=4
        ) {
            let oracle = oracle_invariant(|input: &serde_json::Value, output: &serde_json::Value| {
                // input: {body, year, quarter}; output: the parsed record (or Null)
                let requested_year = input.get("year").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let requested_quarter = input.get("quarter").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
                if output.is_null() {
                    return Ok(()); // parse returned None or Err — no record to check
                }
                let record_year = output.get("year").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as u32;
                let record_quarter = output.get("quarter").and_then(|v| v.as_u64()).unwrap_or(u64::MAX) as u8;
                if record_year != requested_year {
                    return Err(format!(
                        "record.year {record_year} != requested {requested_year}"
                    ));
                }
                if record_quarter != requested_quarter {
                    return Err(format!(
                        "record.quarter {record_quarter} != requested {requested_quarter}"
                    ));
                }
                Ok(())
            });

            let body = serde_json::to_string(&body_value).unwrap_or_default();
            let input = serde_json::json!({
                "year": requested_year,
                "quarter": requested_quarter,
            });
            let output = match parse_fmp_body(&body, "TEST", requested_year, requested_quarter) {
                Ok(Some(record)) => serde_json::to_value(&record).unwrap_or(serde_json::Value::Null),
                _ => serde_json::Value::Null,
            };
            match oracle.verify(&input, &output) {
                OracleVerdict::Pass => {}
                OracleVerdict::Fail(message) => prop_assert!(false, "{message}"),
                OracleVerdict::Inconclusive => {}
            }
            emit_trace("parse_carries_requested_temporal_key", "pass", 0, "invariant");
        }
    }
}
