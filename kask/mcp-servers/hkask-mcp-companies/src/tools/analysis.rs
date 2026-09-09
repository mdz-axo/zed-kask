//! MAIA analysis and research tools.
use crate::{
    CompaniesServer, analysis, fibo, providers, research, screener,
    types::{self, SymbolLimitRequest, SymbolRequest},
    validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = analysis_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Analyze competitive moat using MAIA framework: gross margin stability and working capital market power signal"
    )]
    pub async fn moat_check(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "moat_check", async {
            validate_symbol(&symbol)?;

            // Fetch 10 years of key metrics for gross margin stability analysis
            let limit = "10";
            let metrics_result = self
                .fetch("key_metrics", &symbol, &[("limit", limit)])
                .await;

            let metrics = match metrics_result {
                Ok(v) => v,
                Err(e) => {
                    return Err(e);
                }
            };

            // Fetch income statement for gross margin computation.
            // The stable key-metrics endpoint does not include grossProfitMargin,
            // so we compute it from grossProfit / revenue in the income statement.
            let income_result = self
                .fetch("income_statement", &symbol, &[("limit", limit)])
                .await;

            let income = match income_result {
                Ok(v) => v,
                Err(e) => {
                    return Err(e);
                }
            };

            let gross_margins = analysis::extract_gross_margins(&income);
            if gross_margins.is_empty() {
                let output = serde_json::json!({
                    "symbol": symbol,
                    "moat": "insufficient_data",
                    "reason": "No gross margin data available for this symbol",
                });
                return Ok(output);
            }

            let margin_values: Vec<f64> = gross_margins.iter().map(|(_, m)| *m).collect();
            let stability = analysis::gross_margin_stability(&margin_values);

            let wc_data = analysis::extract_wc_days(&metrics);
            let (wc_spread, dpo, dso) = match wc_data {
                Some((dpo_val, dso_val)) => (
                    analysis::working_capital_spread(dpo_val, dso_val),
                    Some(dpo_val),
                    Some(dso_val),
                ),
                None => (0.0, None, None),
            };

            let wc_label = analysis::wc_signal_label(wc_spread);
            let moat = analysis::classify_moat(stability, wc_spread, gross_margins.len());

            let output = serde_json::json!({
                "symbol": symbol,
                "moat": moat,
                "margin_stability": stability,
                "gross_margins": gross_margins,
                "working_capital": {
                    "spread_days": wc_spread,
                    "dpo": dpo,
                    "dso": dso,
                    "signal": wc_label,
                },
                "data_periods": gross_margins.len(),
            });
            Ok(fibo::enrich_with_ontology(output, "moat_check"))
        })
        .await
    }

    #[tool(
        description = "CEO capital allocation scorecard (MAIA framework): rates how well management allocates capital by comparing returns on capital vs invested capital over time"
    )]
    pub async fn management_scorecard(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "management_scorecard", async {
            validate_symbol(&symbol)?;

            let limit = "10";
            let metrics_result = self.fetch(
     "key_metrics",
     &symbol,
     &[("limit", limit)],
 )
            .await;

            let bs_result = self.fetch(
     "balance_sheet",
     &symbol,
     &[("limit", limit)],
 )
            .await;

            let (metrics, balance_sheets) = match (metrics_result, bs_result) {
                (Ok(m), Ok(b)) => (m, b),
                (Err(e), _) | (_, Err(e)) => {
                    return Err(e);
                }
            };

            let roic_values = analysis::extract_roic(&metrics);
            let capital_values = analysis::extract_invested_capital(&balance_sheets);

            // Align ROIC and invested capital by calendar year - they come from
            // different API endpoints and may have different year ranges.
            use std::collections::HashMap;
            let roic_by_year: HashMap<&str, f64> = roic_values
                .iter()
                .map(|(y, v)| (y.as_str(), *v))
                .collect();
            let mut aligned: Vec<(f64, f64)> = capital_values
                .iter()
                .filter_map(|(year, cap)| roic_by_year.get(year.as_str()).map(|r| (*r, *cap)))
                .collect();
            // Sort by invested capital ascending to preserve original ordering intent
            aligned.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let roic_nums: Vec<f64> = aligned.iter().map(|(r, _)| *r).collect();
            let capital_nums: Vec<f64> = aligned.iter().map(|(_, c)| *c).collect();

            let rating = analysis::ceo_capital_allocation_score(&roic_nums, &capital_nums);

            let output = serde_json::json!({
                "symbol": symbol,
                "ceo_rating": rating,
                "returns_on_capital": roic_values,
                "invested_capital": capital_values,
                "aligned_periods": aligned.len(),
                "data_periods": roic_nums.len(),
                "framework": "MAIA: Good = decreasing capital with improving returns, OR increasing capital with improving returns. Bad = increasing capital with decreasing returns.",
            });
            Ok(fibo::enrich_with_ontology(output, "management_scorecard"))
        }).await
    }

    #[tool(
        description = "Working capital cycle analysis (MAIA CFO scorecard): tracks days payable, days sales outstanding, and cash conversion cycle over time"
    )]
    pub async fn working_capital_cycle(
        &self,
        Parameters(SymbolLimitRequest { symbol, limit }): Parameters<SymbolLimitRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "working_capital_cycle", async {
            validate_symbol(&symbol)?;
            let limit_str = (limit.unwrap_or(10) as usize).min(40).to_string();

            let metrics = match self
                .fetch(
                    "key_metrics",
                    &symbol,
                    &[("limit", &limit_str)],
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    return Err(e);
                }
            };

            // Extract working capital days per period
            let arr = match metrics.as_array() {
                Some(a) => a,
                None => {
                    return Ok(serde_json::json!({"symbol": symbol, "error": "no data"}));
                }
            };

            let periods: Vec<serde_json::Value> = arr
                .iter()
                .filter_map(|entry| {
                    let year = analysis::extract_year(entry)?;
                    let period = entry
                        .get("period")
                        .and_then(|p| p.as_str())
                        .unwrap_or("");
                    let dpo = entry.get("daysOfPayablesOutstanding")?.as_f64()?;
                    let dso = entry.get("daysOfSalesOutstanding")?.as_f64()?;
                    let dio = entry
                        .get("daysOfInventoryOutstanding")
                        .and_then(|v| v.as_f64());
                    let ccc = entry
                        .get("cashConversionCycle")
                        .and_then(|v| v.as_f64());
                    Some(serde_json::json!({
                        "year": year,
                        "period": period,
                        "dpo": dpo,
                        "dso": dso,
                        "dio": dio,
                        "spread": dpo - dso,
                        "cash_conversion_cycle": ccc,
                    }))
                })
                .collect();

            // MAIA CFO score: consistency of working capital management
            let spreads: Vec<f64> = periods
                .iter()
                .filter_map(|p| p.get("spread")?.as_f64())
                .collect();
            let spread_stability = analysis::gross_margin_stability(&spreads);

            let cfo_rating = if spread_stability > 0.8 {
                "stable"
            } else if spread_stability > 0.5 {
                "moderate"
            } else {
                "volatile"
            };

            let output = serde_json::json!({
                "symbol": symbol,
                "cfo_working_capital_rating": cfo_rating,
                "spread_stability": spread_stability,
                "periods": periods,
                "data_points": periods.len(),
                "framework": "MAIA CFO scorecard: stability of working capital management through economic conditions. The level is structural; consistency is management skill.",
            });
            Ok(fibo::enrich_with_ontology(output, "working_capital_cycle"))
        }).await
    }

    #[tool(
        description = "Company screener powered by the EODHD Screener API. Parses natural-language prompts into EODHD filter triples and returns a data table with all criteria values for each matching company. Keywords are field names in space or underscore form (market cap / market_capitalization, price, volume, average volume, eps, dividend yield, sector, industry, daily/weekly change). Operators: above/over/more than/greater than/higher than/>/>=/at least; below/under/less than/lower than/</<=/at most; between X and Y; equals/is/= for string fields. Values accept $, thousands commas, and B/M/K/T or billion/million/thousand suffixes. Geography (US, Japan, Canada, Mexico, Europe, UK, Germany, ...) maps to EODHD exchange codes and fans out one query per exchange, interleaved round-robin so every exchange is represented; results are listings, so a cross-listed company appears once per exchange. Market caps are in each listing's local currency. Parsed criteria are echoed in parsed_criteria — verify them and correct with criteria_overrides (keys: market_capitalization_min/_max and the other _min/_max bounds, exchanges: [codes], sector, industry). Post-screen criteria (revenue growth, ROIC, ROE, P/E, debt/equity, price/book, beta) require per-company fundamentals — use key_metrics for those. Paginates automatically beyond the 1,000-result offset limit."
    )]
    pub async fn company_screener(
        &self,
        Parameters(req): Parameters<types::ScreenerRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "company_screener", async {
            // Parse the natural language prompt into structured criteria
            let mut criteria = screener::parse_screening_prompt(&req.prompt);
            let parsed_count = criteria.as_object().map(|object| object.len()).unwrap_or(0);

            // Apply user overrides — merged over parsed criteria (parsed
            // fields not overridden survive)
            let mut overrides_applied = false;
            if let Some(override_object) = req.criteria_overrides.as_object()
                && let Some(criteria_object) = criteria.as_object_mut()
                && !override_object.is_empty()
            {
                for (key, value) in override_object {
                    criteria_object.insert(key.clone(), value.clone());
                }
                overrides_applied = true;
            }

            // Exchange criteria are handler-owned: one EODHD query per
            // exchange code, merged round-robin. Filter triples never carry
            // the exchange dimension.
            let exchange_codes = screener::extract_exchange_codes(&criteria);
            let mut filter_criteria = criteria.clone();
            if let Some(filter_object) = filter_criteria.as_object_mut() {
                filter_object.remove("exchange");
                filter_object.remove("exchanges");
            }
            let (screener_filters, post_screen_filters) =
                screener::split_criteria(&filter_criteria);
            let post_screen_count = post_screen_filters
                .as_object()
                .map(|object| object.len())
                .unwrap_or(0);

            let mut warnings: Vec<String> = Vec::new();
            if parsed_count == 0 && !overrides_applied {
                warnings.push(
                    "No criteria parsed from the prompt — returning the full universe sorted by market cap. Rephrase using advertised field names (e.g. 'market capitalization between 2 billion and 200 billion', 'sector Technology', 'US, Japan and Europe listed') or pass criteria_overrides (keys: market_capitalization_min/_max, exchanges, sector, industry, ...).".to_string(),
                );
            }
            let has_market_cap_bounds = ["market_capitalization_min", "market_capitalization_max"]
                .iter()
                .any(|key| criteria.get(*key).is_some());
            if has_market_cap_bounds && exchange_codes.iter().any(|code| code != "US") {
                warnings.push(
                    "Market-cap bounds are applied in each listing's local currency (EODHD reports local-currency market caps), so USD-stated bounds on non-US exchanges select a different band than intended. Screen per region with converted bounds, or pass per-exchange bounds via criteria_overrides.".to_string(),
                );
            }

            // Fetch: a single query without exchange criteria, or one query
            // per exchange code (concurrent). A partial exchange failure
            // keeps the surviving exchanges and surfaces the failure; a
            // total failure propagates.
            let fanout_results: Option<
                Vec<(String, Result<Vec<serde_json::Value>, McpToolError>)>,
            > = if exchange_codes.is_empty() {
                None
            } else {
                let fetches = exchange_codes.iter().map(|code| {
                    let mut filters = screener_filters.clone();
                    filters.push(serde_json::json!(["exchange", "=", code]));
                    async move {
                        let outcome = providers::fetch_eodhd_screener(
                            &self.client,
                            &self.eodhd_api_key,
                            &filters,
                        )
                        .await;
                        (code.clone(), outcome)
                    }
                });
                Some(futures::future::join_all(fetches).await)
            };

            let (merged_rows, exchange_counts, exchange_errors) = match fanout_results {
                None => {
                    let rows = providers::fetch_eodhd_screener(
                        &self.client,
                        &self.eodhd_api_key,
                        &screener_filters,
                    )
                    .await?;
                    (rows, serde_json::Map::new(), serde_json::Map::new())
                }
                Some(results) => {
                    if results.iter().all(|(_, outcome)| outcome.is_err()) {
                        let error = results
                            .into_iter()
                            .find_map(|(_, outcome)| outcome.err())
                            .unwrap_or_else(|| {
                                McpToolError::internal("all exchange fetches failed")
                            });
                        return Err(error);
                    }
                    let mut buckets: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
                    let mut exchange_errors = serde_json::Map::new();
                    for (code, outcome) in results {
                        match outcome {
                            Ok(rows) => buckets.push((code, rows)),
                            Err(error) => {
                                exchange_errors
                                    .insert(code, serde_json::Value::String(error.to_string()));
                            }
                        }
                    }
                    // Round-robin interleave: every exchange's best matches
                    // surface before any exchange's second match, so `limit`
                    // truncation cannot empty out an exchange.
                    let mut merged: Vec<serde_json::Value> = Vec::new();
                    let longest = buckets
                        .iter()
                        .map(|(_, rows)| rows.len())
                        .max()
                        .unwrap_or(0);
                    for index in 0..longest {
                        for (_, rows) in &buckets {
                            if let Some(row) = rows.get(index) {
                                merged.push(row.clone());
                            }
                        }
                    }
                    // Listings dedup by (exchange, code) — a company
                    // cross-listed on several exchanges appears once per
                    // exchange (documented).
                    let mut seen = std::collections::HashSet::new();
                    merged.retain(|row| {
                        let exchange = row
                            .get("exchange")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        let code = row
                            .get("code")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        seen.insert((exchange.to_string(), code.to_string()))
                    });
                    let exchange_counts: serde_json::Map<String, serde_json::Value> = buckets
                        .iter()
                        .map(|(code, rows)| {
                            (code.clone(), serde_json::Value::from(rows.len() as u64))
                        })
                        .collect();
                    (merged, exchange_counts, exchange_errors)
                }
            };

            if !exchange_errors.is_empty() {
                let failed: Vec<String> = exchange_errors.keys().cloned().collect();
                warnings.push(format!(
                    "Exchange fetch failed for {} — {} of {} exchanges dropped from results.",
                    failed.join(", "),
                    exchange_errors.len(),
                    exchange_codes.len()
                ));
            }
            if !exchange_codes.is_empty()
                && merged_rows.is_empty()
                && exchange_errors.is_empty()
            {
                warnings.push(format!(
                    "All {} exchanges returned zero matches. If unexpected, check the exchange codes in parsed_criteria.exchanges and correct via criteria_overrides.",
                    exchange_codes.len()
                ));
            }
            if post_screen_count > 0 {
                warnings.push(
                    "Post-screen criteria require per-company fundamentals. Use key_metrics for each result to apply these filters.".to_string(),
                );
            }

            // Enforce the advertised `limit` — an upper bound on the returned
            // row count, not a page size (the fetch exhausts the universe
            // sorted by market cap, so truncation keeps the largest-cap
            // matches). `total_matches` preserves the untruncated count.
            let total_matches = merged_rows.len();
            let rows: Vec<serde_json::Value> = if (total_matches as u32) > req.limit {
                merged_rows.into_iter().take(req.limit as usize).collect()
            } else {
                merged_rows
            };
            let count = rows.len();

            let output = serde_json::json!({
                "prompt": req.prompt,
                "parsed_criteria": criteria,
                "screener_filters": screener_filters,
                "exchange_match_counts": serde_json::Value::Object(exchange_counts),
                "exchange_errors": serde_json::Value::Object(exchange_errors),
                "post_screen_filters": post_screen_filters,
                "warnings": warnings,
                "count": count,
                "total_matches": total_matches,
                "results": rows,
                "fibo": {
                    "market_capitalization": fibo::MARKET_CAPITALIZATION,
                },
                "framework": "EODHD Screener API. Parses natural-language prompts into EODHD filter triples ([field, operation, value], AND-combined). Keywords: field names in space or underscore form (market cap / market_capitalization, price / adjusted_close, volume / avgvol_1d, average volume / avgvol_200d, eps / earnings_share, dividend yield, sector, industry, daily change / refund_1d_p, weekly change / refund_5d_p). Operators: above/over/more than/greater than/higher than/>/>=/at least; below/under/less than/lower than/fewer than/</<=/at most; between X and Y; equals/is/= for string fields. Values accept $, thousands commas, and B/M/K/T or billion/million/thousand suffixes — the suffix binds to its number ('between 2 and 200 billion' parses as min 2, max 2e11). Geography: country/region names and major exchange codes map to EODHD exchange codes under parsed_criteria.exchanges and fan out one query per exchange, interleaved round-robin; results are listings (a cross-listed company appears once per exchange) and market caps are in each listing's local currency. Sector/industry are single-value. Verify parsed_criteria and correct with criteria_overrides (keys: market_capitalization_min/_max and other _min/_max bounds, exchanges: [codes], sector, industry). Post-screen fields (revenue_growth, roic, roe, pe_ratio, debt_equity, price_book, beta) require per-company fundamentals from key_metrics. Paginates automatically beyond the 1,000-result offset limit.",
                "source": "EODHD Screener API"
            });

            Ok(fibo::enrich_with_ontology(output, "company_screener"))
        })
        .await
    }

    #[tool(
        description = "Multi-provider fundamental research search for a company (Exa, Tavily, Brave). Returns research claims classified by category (guidance, competitive, macro, financial, risk) with numeric values, mentioned tickers, and dates extracted — the claim feed for expectations_gap's management-guidance estimate and for research notes. Coverage-honest: per-provider status is surfaced; a provider without a configured key is named in the status, never silently skipped."
    )]
    pub async fn company_research_search(
        &self,
        Parameters(req): Parameters<types::ResearchSearchRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "company_research_search", async {
            // 1. Fetch company profile for name (typed view — `companyName`
            //    knowledge lives in the `CompanyProfile` accessor).
            let profile = self.fetch_profile(&req.symbol).await?;
            let company_name = profile.company_name().unwrap_or(&req.symbol);

            // 2. Run multi-provider search
            let research = research::search_fundamental(
                &self.client,
                &req.symbol,
                company_name,
                &req.query,
                self.exa_api_key.as_deref(),
                self.tavily_api_key.as_deref(),
                self.brave_api_key.as_deref(),
            ).await?;

            // 3. Build output with claim classification (FinGPT §3.4)
            let enhanced = research::ResearchClaimClassifier::classify_all(&research);

            let claims: Vec<serde_json::Value> = enhanced.claims.iter().map(|c| {
                serde_json::json!({
                    "text": c.text,
                    "source": c.source,
                    "category": c.category,
                    "numeric_values": c.numeric_values.iter().map(|n| {
                        serde_json::json!({"value": n.value, "unit": n.unit, "context": n.context})
                    }).collect::<Vec<_>>(),
                    "tickers": c.tickers,
                    "date_mentioned": c.date_mentioned,
                })
            }).collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "query": req.query,
                "claims": claims,
                "claims_count": claims.len(),
                "category_summary": enhanced.category_summary,
                "providers": research.provider_summary.iter().map(|p| {
                    serde_json::json!({"provider": p.provider, "claims": p.claims_found, "status": p.status})
                }).collect::<Vec<_>>(),

                "framework": "Multi-provider fundamental research search (Exa, Tavily, Brave). Claims are classified by category and numeric values extracted. Use with thesis_test, scenario_weight, or guidance_check skills for structured financial analysis mapping claims to DCF assumptions."
            });

            Ok(fibo::enrich_with_ontology(output, "company_research_search"))
        }).await
    }

    // ── Portfolio tools ──
}
