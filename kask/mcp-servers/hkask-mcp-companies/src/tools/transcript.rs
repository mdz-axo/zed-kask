//! `company_transcript` tool — earnings + corpus modes.
//!
//! Thin MCP wrapper over `crate::transcript`. Earnings mode fetches FMP;
//! corpus mode fetches SerpAPI YouTube (channel-allowlisted). The deterministic,
//! provider-shaped work lives in the module; the tool validates inputs and
//! dispatches.
use crate::{
    CompaniesServer, transcript,
    types::{CompanyTranscriptRequest, TranscriptMode},
    validate_symbol,
};
use chrono::Datelike;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = transcript_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Fetch company transcripts. Earnings mode: FMP earnings-call transcripts, coverage-honest. Corpus mode: non-earnings transcripts (investor days, keynotes) via SerpAPI YouTube, channel-allowlisted. Use (year, quarter) as the temporal key for earnings — the date field is unreliable."
    )]
    pub async fn company_transcript(
        &self,
        Parameters(request): Parameters<CompanyTranscriptRequest>,
    ) -> String {
        execute_tool(self, "company_transcript", async {
            validate_symbol(&request.symbol)?;

            match request.mode {
                TranscriptMode::Earnings => {
                    let end = resolve_window_end(&request)?;
                    let result = transcript::fetch_transcript_window(
                        &self.client,
                        &request.symbol,
                        end,
                        request.quarters_back,
                        &self.fmp_api_key,
                    )
                    .await;

                    // The tool fails only when zero quarters succeed.
                    if result.transcripts.is_empty() && !result.coverage.missing.is_empty() {
                        let all_no_call = result.coverage.missing.iter().all(|missing| {
                            matches!(missing.reason, transcript::MissingReason::NoCall)
                        });
                        if all_no_call {
                            return Err(McpToolError::not_found(format!(
                                "no earnings calls found for {} in the requested {} quarter(s)",
                                request.symbol, result.coverage.requested_quarters
                            )));
                        }
                        return Err(McpToolError::unavailable(format!(
                            "all {} requested quarter(s) failed to fetch for {}; see coverage.missing",
                            result.coverage.requested_quarters, request.symbol
                        )));
                    }

                    serde_json::to_value(&result).map_err(|error| {
                        McpToolError::internal(format!(
                            "failed to serialize transcript result: {error}"
                        ))
                    })
                }
                TranscriptMode::Corpus => {
                    let query = request.query.as_deref().unwrap_or("");
                    if query.is_empty() {
                        return Err(McpToolError::invalid_argument(
                            "corpus mode requires a 'query' (e.g. \"Satya Nadella keynote\")",
                        ));
                    }
                    let Some(serpapi_key) = &self.serpapi_key else {
                        return Err(McpToolError::failed_precondition(
                            "corpus mode requires HKASK_SERPAPI_KEY to be set",
                        ));
                    };

                    let result = transcript::fetch_corpus_transcripts(
                        &self.client,
                        &request.symbol,
                        query,
                        &request.channels_allowlist,
                        request.max_results,
                        serpapi_key,
                    )
                    .await?;

                    serde_json::to_value(&result).map_err(|error| {
                        McpToolError::internal(format!(
                            "failed to serialize corpus transcript result: {error}"
                        ))
                    })
                }
            }
        })
        .await
    }
}

/// Resolve the window end `(year, quarter)` for earnings mode. Both provided →
/// use them; one provided → error; neither → infer the most recent completed
/// quarter from the current UTC date.
fn resolve_window_end(
    request: &CompanyTranscriptRequest,
) -> Result<transcript::YearQuarter, McpToolError> {
    match (request.year, request.quarter) {
        (Some(year), Some(quarter)) => {
            transcript::YearQuarter::new(year, quarter).ok_or_else(|| {
                McpToolError::invalid_argument(format!("quarter must be 1–4, got {quarter}"))
            })
        }
        (Some(_), None) | (None, Some(_)) => Err(McpToolError::invalid_argument(
            "year and quarter must both be provided, or both omitted (to infer the most recent quarter)",
        )),
        (None, None) => {
            let now = chrono::Utc::now();
            let year = now.year().try_into().unwrap_or(0);
            let month = now.month();
            let quarter = ((month - 1) / 3) + 1;
            let current = transcript::YearQuarter::new(year, quarter as u8)
                .ok_or_else(|| McpToolError::internal("failed to infer current quarter"))?;
            current
                .previous()
                .ok_or_else(|| McpToolError::internal("failed to infer previous quarter"))
        }
    }
}
