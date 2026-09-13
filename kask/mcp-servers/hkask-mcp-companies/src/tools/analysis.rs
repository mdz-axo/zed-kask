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
        description = "Company screener powered by the EODHD Screener API. Parses natural-language prompts into EODHD filter triples and returns a data table with all criteria values for each matching company. Keywords are field names in space or underscore form (market cap / market_capitalization, price, volume, average volume, eps, dividend yield, sector, industry, daily/weekly change). Operators: above/over/more than/greater than/higher than/>/>=/at least; below/under/less than/lower than/fewer than/</<=/at most; between X and Y; equals/is/= for string fields. Values accept $, thousands commas, and B/M/K/T or billion/million/thousand suffixes. Geography (US, Japan, Canada, Mexico, Europe, UK, Germany, ...) maps to EODHD exchange codes and fans out one query per exchange, interleaved round-robin so every exchange is represented; results are listings, so a cross-listed company appears once per exchange. USD-stated market-cap bounds are sent unconverted — EODHD's market_capitalization filter compares USD-denominated values while its returned field is listing-currency — and rows carry market_capitalization_usd from EODHD FOREX daily closes (cached 24h), rank by USD cap, with the band enforced client-side; lines quoted in a foreign currency are dropped when their home market is also screened, otherwise kept and converted at their own rate (Japanese companies enter via London ¥ lines — EODHD has no Japanese exchange, and London is queried unbounded so those lines are not lost). Exchanges without a currency mapping or FX rate are dropped and named in exchange_errors. Parsed criteria are echoed in parsed_criteria — verify them and correct with criteria_overrides (keys: market_capitalization_min/_max and the other _min/_max bounds, exchanges: [codes], sector, industry). Post-screen criteria (revenue growth, ROIC, ROE, P/E, debt/equity, price/book, beta) require per-company fundamentals — use key_metrics for those. Paginates automatically beyond the 1,000-result offset limit."
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

            // Exchange and enrichment controls are handler-owned. They never
            // enter EODHD screener filter triples.
            let exchange_codes = screener::extract_exchange_codes(&criteria);
            let liquidity_min_usd = criteria
                .get("liquidity_min_usd")
                .and_then(|value| value.as_f64());
            if liquidity_min_usd.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(McpToolError::invalid_argument(
                    "liquidity_min_usd must be a positive finite number",
                ));
            }
            let liquidity_window_days = criteria
                .get("liquidity_window_days")
                .and_then(|value| value.as_u64())
                .unwrap_or(60);
            if liquidity_window_days == 0 {
                return Err(McpToolError::invalid_argument(
                    "liquidity_window_days must be greater than zero",
                ));
            }
            let result_offset = criteria
                .get("result_offset")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let result_offset = usize::try_from(result_offset).map_err(|_| {
                McpToolError::invalid_argument("result_offset exceeds the platform range")
            })?;
            let mut filter_criteria = criteria.clone();
            if let Some(filter_object) = filter_criteria.as_object_mut() {
                for key in [
                    "exchange",
                    "exchanges",
                    "liquidity_min_usd",
                    "liquidity_window_days",
                    "result_offset",
                ] {
                    filter_object.remove(key);
                }
            }
            let (screener_filters, post_screen_filters) =
                screener::split_criteria(&filter_criteria);
            let post_screen_count = post_screen_filters
                .as_object()
                .map(|object| object.len())
                .unwrap_or(0);

            // FX context: EODHD's screener market_capitalization filter
            // compares USD-denominated values (its returned field is
            // listing-currency), so USD-stated bounds are sent unconverted
            // and the FX rates (FOREX daily closes, cached 24h) annotate rows
            // with market_capitalization_usd and enforce the band client-side.
            let cap_min = criteria
                .get("market_capitalization_min")
                .and_then(|value| value.as_f64());
            let cap_max = criteria
                .get("market_capitalization_max")
                .and_then(|value| value.as_f64());
            let cap_bounds_present = cap_min.is_some() || cap_max.is_some();
            let fx = if cap_bounds_present && !exchange_codes.is_empty() {
                if exchange_codes.iter().all(|code| code == "US") {
                    Some(Ok(ScreenerFx::usd_only(&exchange_codes)))
                } else {
                    Some(self.acquire_screener_fx(&exchange_codes).await)
                }
            } else {
                None
            };

            let mut warnings: Vec<String> = Vec::new();
            if parsed_count == 0 && !overrides_applied {
                warnings.push(
                    "No criteria parsed from the prompt — returning the full universe sorted by market cap. Rephrase using advertised field names (e.g. 'market capitalization between 2 billion and 200 billion', 'sector Technology', 'US, Japan and Europe listed') or pass criteria_overrides (keys: market_capitalization_min/_max, exchanges, sector, industry, ...).".to_string(),
                );
            }
            match &fx {
                // Conversion active — the fx output object documents rates;
                // no local-currency warning.
                Some(Ok(_)) => {}
                Some(Err(reason)) => {
                    tracing::warn!(
                        target: "hkask.mcp.companies.screener",
                        "USD conversion unavailable: {reason}"
                    );
                    warnings.push(format!(
                        "USD conversion unavailable ({reason}) — market-cap bounds are applied in each listing's local currency, so USD-stated bounds on non-US exchanges select a different band than intended."
                    ));
                }
                None => {
                    if cap_bounds_present && exchange_codes.is_empty() {
                        warnings.push(
                            "Market-cap bounds with no exchange restriction apply in each listing's local currency (EODHD reports local-currency market caps). Add geography (e.g. 'US, Japan and Europe listed') to enable USD conversion per exchange.".to_string(),
                        );
                    }
                }
            }

            let mut exchange_errors = serde_json::Map::new();
            let mut row_stats = RowFilterStats::default();
            let mut extra_rates = serde_json::Map::new();
            let (merged_rows, exchange_counts) = if exchange_codes.is_empty() {
                let mut rows = providers::fetch_eodhd_screener(
                    &self.client,
                    &self.eodhd_api_key,
                    &screener_filters,
                )
                .await?;
                filter_non_common(&mut rows, &mut row_stats);
                (rows, serde_json::Map::new())
            } else {
                // Build per-exchange queries. EODHD's screener
                // market_capitalization filter compares USD-denominated
                // values (its returned field is listing-currency), so USD
                // bounds are sent unconverted on every exchange; the FX
                // context annotates rows with market_capitalization_usd and
                // enforces the band client-side. Exchanges without a
                // currency mapping or rate are dropped and named.
                let mut queries: Vec<(String, f64, Vec<serde_json::Value>)> = Vec::new();
                match &fx {
                    Some(Ok(context)) => {
                        for code in &exchange_codes {
                            match context.rate_for(code) {
                                Ok(rate) => {
                                    // Mixed-currency exchanges (London's IOB
                                    // hosts ¥/kr/Ft lines) are queried with
                                    // NO cap bounds at all: EODHD's filter
                                    // denomination is verified USD only for
                                    // local-currency listings, and an
                                    // unverified bound applied to a
                                    // foreign-currency line (a ¥9.2T cap)
                                    // could numerically exclude every
                                    // Japanese row, silently losing Japan.
                                    // Selection for these exchanges happens
                                    // entirely in the row-currency pass and
                                    // the client-side band enforcement.
                                    let mixed = MIXED_CURRENCY_EXCHANGES
                                        .contains(&code.as_str());
                                    let mut filters = if mixed {
                                        screener_filters
                                            .iter()
                                            .filter(|filter| {
                                                filter
                                                    .as_array()
                                                    .and_then(|parts| parts.first())
                                                    .and_then(|field| field.as_str())
                                                    != Some("market_capitalization")
                                            })
                                            .cloned()
                                            .collect()
                                    } else {
                                        screener_filters.clone()
                                    };
                                    filters.push(serde_json::json!(["exchange", "=", code]));
                                    queries.push((code.clone(), rate, filters));
                                }
                                Err(reason) => {
                                    exchange_errors.insert(
                                        code.clone(),
                                        serde_json::Value::String(reason.to_string()),
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        for code in &exchange_codes {
                            let mut filters = screener_filters.clone();
                            filters.push(serde_json::json!(["exchange", "=", code]));
                            queries.push((code.clone(), 1.0, filters));
                        }
                    }
                }
                if queries.is_empty() {
                    let reasons: Vec<String> = exchange_errors
                        .iter()
                        .map(|(code, reason)| format!("{code}: {reason}"))
                        .collect();
                    return Err(McpToolError::unavailable(format!(
                        "all exchange queries dropped before fetching: {}",
                        reasons.join("; ")
                    )));
                }

                let fetches: Vec<_> = queries
                    .iter()
                    .map(|(code, rate, filters)| {
                        let filters = filters.clone();
                        let code = code.clone();
                        async move {
                            let outcome = providers::fetch_eodhd_screener(
                                &self.client,
                                &self.eodhd_api_key,
                                &filters,
                            )
                            .await;
                            (code, *rate, outcome)
                        }
                    })
                    .collect();
                // Bounded concurrency: EODHD rate-limits (~17 req/s); a
                // 27-exchange fan-out at full concurrency can burst past it.
                // Eight in flight keeps the fan-out well under the limit
                // while staying concurrent.
                const SCREENER_FANOUT_CONCURRENCY: usize = 8;
                use futures::StreamExt as _;
                let results = futures::stream::iter(fetches)
                    .buffered(SCREENER_FANOUT_CONCURRENCY)
                    .collect::<Vec<_>>()
                    .await;

                // A partial exchange failure keeps the surviving exchanges
                // and surfaces the failure; a total failure propagates.
                if results.iter().all(|(_, _, outcome)| outcome.is_err()) {
                    let error = results
                        .into_iter()
                        .find_map(|(_, _, outcome)| outcome.err())
                        .unwrap_or_else(|| {
                            McpToolError::internal("all exchange fetches failed")
                        });
                    return Err(error);
                }

                let mut buckets: Vec<(String, f64, Vec<serde_json::Value>)> = Vec::new();
                for (code, rate, outcome) in results {
                    match outcome {
                        Ok(rows) => {
                            let mut rows = rows;
                            filter_non_common(&mut rows, &mut row_stats);
                            buckets.push((code, rate, rows));
                        }
                        Err(error) => {
                            exchange_errors
                                .insert(code, serde_json::Value::String(error.to_string()));
                        }
                    }
                }

                // Row-currency pass: classify each row by its
                // currency_symbol against the exchange's currency, annotate
                // a USD market cap where resolvable, drop foreign lines whose
                // home market this screen also covers, and enforce the
                // requested USD band client-side.
                let annotate = matches!(&fx, Some(Ok(_)));
                let buckets: Vec<(String, Vec<serde_json::Value>)> =
                    if let Some(Ok(context)) = &fx {
                        let (kept, stats, rates) = self
                            .screener_row_currency_pass(
                                buckets,
                                context,
                                cap_min,
                                cap_max,
                            )
                            .await;
                        row_stats.foreign_lines_dropped += stats.foreign_lines_dropped;
                        row_stats.out_of_band_dropped += stats.out_of_band_dropped;
                        row_stats.unconverted_rows += stats.unconverted_rows;
                        row_stats.non_common_dropped += stats.non_common_dropped;
                        extra_rates = rates;
                        kept
                    } else {
                        buckets
                            .into_iter()
                            .map(|(code, _, rows)| (code, rows))
                            .collect()
                    };

                // Merge: with USD conversion, rank by market_capitalization_usd
                // (the cap-comparable order the truncation contract
                // promises); without it, round-robin interleave so every
                // exchange is represented under `limit` truncation.
                let mut merged: Vec<serde_json::Value> = if annotate {
                    let mut all: Vec<serde_json::Value> = buckets
                        .iter()
                        .flat_map(|(_, rows)| rows.iter().cloned())
                        .collect();
                    all.sort_by(|a, b| {
                        let a_usd = a
                            .get("market_capitalization_usd")
                            .and_then(|value| value.as_f64());
                        let b_usd = b
                            .get("market_capitalization_usd")
                            .and_then(|value| value.as_f64());
                        match (a_usd, b_usd) {
                            (Some(x), Some(y)) => {
                                y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal)
                            }
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    });
                    all
                } else {
                    let mut interleaved: Vec<serde_json::Value> = Vec::new();
                    let longest = buckets
                        .iter()
                        .map(|(_, rows)| rows.len())
                        .max()
                        .unwrap_or(0);
                    for index in 0..longest {
                        for (_, rows) in &buckets {
                            if let Some(row) = rows.get(index) {
                                interleaved.push(row.clone());
                            }
                        }
                    }
                    interleaved
                };

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
                (merged, exchange_counts)
            };

            if !exchange_errors.is_empty() {
                let dropped: Vec<String> = exchange_errors.keys().cloned().collect();
                warnings.push(format!(
                    "Exchanges dropped from results: {} — {} of {} exchanges excluded; reasons in exchange_errors.",
                    dropped.join(", "),
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

            // The cap-qualified universe is stable and sorted before paging.
            // Liquidity enrichment runs only over this bounded page so callers
            // can exhaust large screens without exceeding the MCP timeout.
            let total_matches = merged_rows.len();
            let page_size = usize::try_from(req.limit)
                .map_err(|_| McpToolError::invalid_argument("limit exceeds the platform range"))?;
            let candidate_rows: Vec<serde_json::Value> = merged_rows
                .into_iter()
                .skip(result_offset)
                .take(page_size)
                .collect();
            let candidates_processed = candidate_rows.len();
            let next_result_offset = result_offset
                .checked_add(candidates_processed)
                .filter(|next| *next < total_matches);
            let (rows, liquidity_excluded, liquidity_errors) =
                if let Some(minimum_usd) = liquidity_min_usd {
                    self.screen_liquidity_page(
                        candidate_rows,
                        minimum_usd,
                        liquidity_window_days,
                    )
                    .await?
                } else {
                    (candidate_rows, 0, serde_json::Map::new())
                };
            let count = rows.len();

            let mut output = serde_json::json!({
                "prompt": req.prompt,
                "parsed_criteria": criteria,
                "screener_filters": screener_filters,
                "exchange_match_counts": serde_json::Value::Object(exchange_counts),
                "exchange_errors": serde_json::Value::Object(exchange_errors),
                "foreign_lines_dropped": row_stats.foreign_lines_dropped,
                "out_of_band_dropped": row_stats.out_of_band_dropped,
                "unconverted_rows": row_stats.unconverted_rows,
                "non_common_dropped": row_stats.non_common_dropped,
                "post_screen_filters": post_screen_filters,
                "warnings": warnings,
                "count": count,
                "total_matches": total_matches,
                "result_offset": result_offset,
                "candidates_processed": candidates_processed,
                "next_result_offset": next_result_offset,
                "liquidity_excluded": liquidity_excluded,
                "liquidity_errors": serde_json::Value::Object(liquidity_errors),
                "results": rows,
                "fibo": {
                    "market_capitalization": fibo::MARKET_CAPITALIZATION,
                },
                "framework": "EODHD Screener API. Parses natural-language prompts into EODHD filter triples ([field, operation, value], AND-combined). Keywords: field names in space or underscore form (market cap / market_capitalization, price / adjusted_close, volume / avgvol_1d, average volume / avgvol_200d, eps / earnings_share, dividend yield, sector, industry, daily change / refund_1d_p, weekly change / refund_5d_p). Operators: above/over/more than/greater than/higher than/>/>=/at least; below/under/less than/lower than/fewer than/</<=/at most; between X and Y; equals/is/= for string fields. Values accept $, thousands commas, and B/M/K/T or billion/million/thousand suffixes — the suffix binds to its number ('between 2 and 200 billion' parses as min 2, max 2e11). Geography: country/region names and major exchange codes map to EODHD exchange codes under parsed_criteria.exchanges and fan out one query per exchange; results are listings (a cross-listed company appears once per exchange). USD-stated market-cap bounds are sent unconverted — EODHD's market_capitalization filter compares USD-denominated values while its returned field is listing-currency — and rows carry market_capitalization_usd from EODHD FOREX daily closes (cached 24h; the fx object carries rates and the as-of date), ranked by USD cap. Row currency follows the row's currency_symbol: lines quoted in another currency are dropped when that currency's home market is also screened (the company appears via its home exchange) and otherwise kept and converted at their own currency's rate — Japanese companies enter this way (EODHD has no Japanese exchange; their London ¥ lines are the surface). The requested band is enforced client-side (out_of_band_dropped counts rows EODHD returned outside it; foreign_lines_dropped counts dropped foreign lines; unconverted_rows counts rows without a USD conversion, sorted last; non_common_dropped counts ETFs/preferreds/notes/CDRs dropped — EODHD has no type filter). Mixed-currency exchanges (London) are queried unbounded — their foreign-currency lines (¥, kr, Ft) are selected by the row-currency rules instead of server-side bounds. Without an FX context (warned), rows carry no market_capitalization_usd and the band cannot be enforced client-side — the unconverted server-side bound stands alone. Sector/industry are single-value. Verify parsed_criteria and correct with criteria_overrides (keys: market_capitalization_min/_max and other _min/_max bounds, exchanges: [codes], sector, industry). Post-screen fields (revenue_growth, roic, roe, pe_ratio, debt_equity, price_book, beta) require per-company fundamentals from key_metrics. Paginates automatically beyond the 1,000-result offset limit.",
                "source": "EODHD Screener API"
            });

            // FX transparency: rates, as-of date, and the exchange→currency
            // mapping actually used (only when at least one rate resolved).
            if let Some(Ok(context)) = &fx {
                let mut exchange_currencies = serde_json::Map::new();
                let mut usd_rates = serde_json::Map::new();
                for code in &exchange_codes {
                    if let Some(currency) = context.currency_by_exchange.get(code) {
                        exchange_currencies.insert(code.clone(), serde_json::json!(currency));
                        if let Some(rate) = context.rate_by_currency.get(currency) {
                            usd_rates.insert(currency.clone(), serde_json::json!(rate));
                        }
                    }
                    for (currency, rate) in &extra_rates {
                        usd_rates.insert(currency.clone(), rate.clone());
                    }
                }
                if !usd_rates.is_empty()
                    && let Some(object) = output.as_object_mut()
                {
                    object.insert(
                        "fx".to_string(),
                        serde_json::json!({
                            "as_of": context.as_of,
                            "exchange_currencies": serde_json::Value::Object(exchange_currencies),
                            "usd_rates": serde_json::Value::Object(usd_rates),
                            "note": "USD market-cap bounds sent unconverted (EODHD's filter is USD-denominated); EODHD FOREX daily closes annotate rows with market_capitalization_usd."
                        }),
                    );
                }
            }

            Ok(fibo::enrich_with_ontology(output, "company_screener"))
        })
        .await
    }

    /// Acquire the USD conversion context for a fan-out: the cached
    /// exchange→currency map plus concurrent cached USD rates for every
    /// distinct non-USD currency among the codes. A failed exchanges-list
    /// fetch degrades the whole conversion (Err); individual rate failures
    /// surface per-exchange via [`ScreenerFx::rate_for`].
    async fn acquire_screener_fx(&self, codes: &[String]) -> Result<ScreenerFx, ScreenerFxError> {
        let list = self
            .cached_exchanges_list()
            .await
            .map_err(ScreenerFxError::ExchangesList)?;

        let mut currency_by_exchange = std::collections::HashMap::new();
        if let Some(entries) = list.as_array() {
            for entry in entries {
                if let (Some(code), Some(currency)) = (
                    entry.get("Code").and_then(|value| value.as_str()),
                    entry.get("Currency").and_then(|value| value.as_str()),
                ) {
                    currency_by_exchange.insert(code.to_uppercase(), currency.to_uppercase());
                }
            }
        }

        let mut currencies: Vec<String> = codes
            .iter()
            .filter_map(|code| currency_by_exchange.get(code))
            .filter(|currency| *currency != "USD")
            .cloned()
            .collect();
        currencies.sort();
        currencies.dedup();

        let fetches = currencies
            .iter()
            .map(|currency| self.cached_forex_rate(currency));
        let rate_results = futures::future::join_all(fetches).await;

        let mut rate_by_currency = std::collections::HashMap::new();
        let mut as_of = String::new();
        for (currency, outcome) in currencies.iter().zip(rate_results) {
            if let Ok((date, rate)) = outcome {
                if date > as_of {
                    as_of = date;
                }
                rate_by_currency.insert(currency.clone(), rate);
            }
        }

        Ok(ScreenerFx {
            as_of,
            currency_by_exchange,
            rate_by_currency,
        })
    }

    /// expect: [P5] A liquidity-qualified listing carries the exact EODHD
    /// trailing-window mean of daily close × volume in USD; unavailable data
    /// is surfaced per symbol and never treated as zero or as passing.
    /// dcterms:identifier: CompaniesServer::screen_liquidity_page
    async fn screen_liquidity_page(
        &self,
        rows: Vec<serde_json::Value>,
        minimum_usd: f64,
        window_days: u64,
    ) -> Result<
        (
            Vec<serde_json::Value>,
            u64,
            serde_json::Map<String, serde_json::Value>,
        ),
        McpToolError,
    > {
        let window_days_i64 = i64::try_from(window_days).map_err(|_| {
            McpToolError::invalid_argument("liquidity_window_days exceeds the calendar range")
        })?;
        let to = chrono::Utc::now().date_naive();
        let from = to
            .checked_sub_signed(chrono::Duration::days(window_days_i64))
            .ok_or_else(|| {
                McpToolError::invalid_argument("liquidity window underflows the calendar")
            })?;
        let from = from.format("%Y-%m-%d").to_string();
        let to = to.format("%Y-%m-%d").to_string();

        let mut currencies: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                row.get("currency_symbol")
                    .and_then(|value| value.as_str())
                    .and_then(currency_for_symbol)
            })
            .filter(|currency| *currency != "USD")
            .map(str::to_string)
            .collect();
        currencies.sort();
        currencies.dedup();
        let fx_fetches = currencies.iter().cloned().map(|currency| {
            let from = &from;
            let to = &to;
            async move {
                let symbol = format!("USD{currency}.FOREX");
                let outcome = providers::fetch_eodhd_eod_history(
                    &self.client,
                    &self.eodhd_api_key,
                    &symbol,
                    from,
                    to,
                )
                .await
                .map_err(|error| error.to_string());
                (currency, outcome)
            }
        });
        let fx_histories: std::collections::HashMap<_, _> = futures::future::join_all(fx_fetches)
            .await
            .into_iter()
            .collect();

        use futures::StreamExt as _;
        const LIQUIDITY_CONCURRENCY: usize = 8;
        let measurements = futures::stream::iter(rows.into_iter().map(|row| {
            let from = from.clone();
            let to = to.clone();
            let fx_histories = &fx_histories;
            async move {
                let symbol = listing_symbol(&row);
                let measurement = self
                    .measure_usd_liquidity(row, minimum_usd, window_days, &from, &to, fx_histories)
                    .await;
                (symbol, measurement)
            }
        }))
        .buffered(LIQUIDITY_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        let mut eligible = Vec::new();
        let mut excluded = 0_u64;
        let mut errors = serde_json::Map::new();
        for (symbol, measurement) in measurements {
            match measurement {
                Ok(Some(row)) => eligible.push(row),
                Ok(None) => excluded += 1,
                Err(reason) => {
                    excluded += 1;
                    errors.insert(symbol, serde_json::Value::String(reason));
                }
            }
        }
        Ok((eligible, excluded, errors))
    }

    async fn measure_usd_liquidity(
        &self,
        mut row: serde_json::Value,
        minimum_usd: f64,
        window_days: u64,
        from: &str,
        to: &str,
        fx_histories: &std::collections::HashMap<String, Result<serde_json::Value, String>>,
    ) -> Result<Option<serde_json::Value>, String> {
        let symbol = listing_symbol(&row);
        let currency_symbol = row
            .get("currency_symbol")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let currency = currency_for_symbol(currency_symbol).ok_or_else(|| {
            format!("currency symbol {currency_symbol:?} has no unique USD conversion")
        })?;
        let fx_history = if currency == "USD" {
            None
        } else {
            Some(
                fx_histories
                    .get(currency)
                    .ok_or_else(|| format!("USD{currency}.FOREX history was not requested"))?
                    .as_ref()
                    .map_err(|reason| reason.clone())?,
            )
        };
        let history = providers::fetch_eodhd_eod_history(
            &self.client,
            &self.eodhd_api_key,
            &symbol,
            from,
            to,
        )
        .await
        .map_err(|error| error.to_string())?;
        let (average_usd, observations) =
            trailing_average_dollar_volume_usd(&history, currency_symbol, fx_history)?;
        if average_usd < minimum_usd {
            return Ok(None);
        }
        let fundamentals =
            providers::fetch_eodhd_fundamentals(&self.client, &self.eodhd_api_key, &symbol)
                .await
                .map_err(|error| error.to_string())?;
        let general = fundamentals
            .get("General")
            .and_then(|value| value.as_object())
            .ok_or_else(|| "EODHD fundamentals has no General object".to_string())?;
        let instrument_type = general
            .get("Type")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "EODHD General.Type is missing".to_string())?;
        if instrument_type != "Common Stock" {
            return Err(format!(
                "EODHD General.Type {instrument_type:?} is not an eligible common share or ADR"
            ));
        }

        let Some(object) = row.as_object_mut() else {
            return Err("screener row is not an object".to_string());
        };
        object.insert(
            "average_daily_dollar_volume_usd".to_string(),
            serde_json::json!(average_usd),
        );
        object.insert(
            "liquidity_observations".to_string(),
            serde_json::json!(observations),
        );
        object.insert(
            "liquidity_window_days".to_string(),
            serde_json::json!(window_days),
        );
        object.insert("liquidity_eligible".to_string(), serde_json::json!(true));
        let source = if currency == "USD" {
            "EODHD EOD close × volume".to_string()
        } else {
            format!("EODHD EOD close × volume; date-matched USD{currency}.FOREX")
        };
        object.insert("liquidity_source".to_string(), serde_json::json!(source));
        for (output_key, general_key) in [
            ("instrument_type", "Type"),
            ("issuer_lei", "LEI"),
            ("security_isin", "ISIN"),
            ("primary_ticker", "PrimaryTicker"),
            ("home_category", "HomeCategory"),
            ("other_listings", "Listings"),
        ] {
            object.insert(
                output_key.to_string(),
                general
                    .get(general_key)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            );
        }
        object.insert(
            "identity_source".to_string(),
            serde_json::json!("EODHD General"),
        );
        Ok(Some(row))
    }

    /// The EODHD exchange inventory, cached 24h.
    async fn cached_exchanges_list(&self) -> Result<serde_json::Value, McpToolError> {
        const ENDPOINT: &str = "screener_exchanges_list";
        if let Some(cache) = self.fibo_cache.as_ref()
            && let Some(cached) = cache.get_raw("EXCHANGES", ENDPOINT, "none")
        {
            return Ok(cached);
        }
        let list = providers::fetch_eodhd_exchanges(&self.client, &self.eodhd_api_key).await?;
        if let Some(cache) = self.fibo_cache.as_ref() {
            cache.store_raw("EXCHANGES", ENDPOINT, "none", &list, "EODHD");
        }
        Ok(list)
    }

    /// The latest USD→currency FOREX close, cached 24h. Shared by the
    /// screener's market-cap conversion and valuation price normalization.
    pub(crate) async fn cached_forex_rate(
        &self,
        currency: &str,
    ) -> Result<(String, f64), McpToolError> {
        const ENDPOINT: &str = "screener_forex_rate";
        let symbol = format!("USD{currency}.FOREX");
        if let Some(cache) = self.fibo_cache.as_ref()
            && let Some(cached) = cache.get_raw(&symbol, ENDPOINT, "none")
            && let (Some(date), Some(rate)) = (
                cached.get("date").and_then(|value| value.as_str()),
                cached.get("close").and_then(|value| value.as_f64()),
            )
        {
            return Ok((date.to_string(), rate));
        }
        let (date, rate) =
            providers::fetch_eodhd_forex_rate(&self.client, &self.eodhd_api_key, currency).await?;
        if let Some(cache) = self.fibo_cache.as_ref() {
            cache.store_raw(
                &symbol,
                ENDPOINT,
                "none",
                &serde_json::json!({"date": date, "close": rate}),
                "EODHD",
            );
        }
        Ok((date, rate))
    }

    /// Row-level currency handling for the screener fan-out.
    ///
    /// EODHD rows carry a per-row `currency_symbol` that can differ from the
    /// exchange's default currency (London IOB lines quote Japanese stocks in
    /// ¥, Nordic stocks in kr). Classification per row:
    /// - symbol matches the exchange currency → convert at the exchange rate;
    /// - symbol maps to another currency whose home rows survived in this
    ///   screen → drop (the company's home-exchange line is in another
    ///   bucket, correctly converted);
    /// - symbol maps to a currency with no surviving home rows → keep and
    ///   convert at that currency's USD rate (how Japanese companies enter:
    ///   EODHD has no Japanese exchange, so their London ¥ lines are the only
    ///   surface — and how Hungarian companies survive an empty Budapest
    ///   bucket);
    /// - ambiguous symbol ("kr" = SEK/NOK/DKK) → drop when any candidate
    ///   currency kept home rows, else keep unconverted;
    /// - unknown symbol or missing cap → keep unconverted (no USD field).
    ///
    /// Home coverage counts only rows that survive the band enforcement — a
    /// home bucket that fetched nothing (or whose rows all fell outside the
    /// requested band) must not orphan the company's foreign line
    /// (live-observed 2026-09-10: BUD kept zero rows, so Hungarian companies'
    /// London Ft lines were dropped with nothing replacing them).
    ///
    /// Non-common-stock instruments (ETFs, preferreds, notes — EODHD's
    /// screener has no type filter) are dropped first and counted in
    /// `non_common_dropped`.
    ///
    /// Rows with a resolved USD cap are enforced against the requested band
    /// client-side — EODHD's server-side filter application is inconsistent
    /// for some exchanges (verified live 2026-09-09: MX ignores the upper
    /// bound), so the band is guaranteed here.
    ///
    /// Returns the kept rows per exchange, drop/keep counters, and any extra
    /// USD rates fetched for foreign-currency rows (for the fx object).
    async fn screener_row_currency_pass(
        &self,
        buckets: Vec<(String, f64, Vec<serde_json::Value>)>,
        context: &ScreenerFx,
        cap_min: Option<f64>,
        cap_max: Option<f64>,
    ) -> (
        Vec<(String, Vec<serde_json::Value>)>,
        RowFilterStats,
        serde_json::Map<String, serde_json::Value>,
    ) {
        enum RowClass {
            SameCurrency(f64),
            Foreign(String),
            Ambiguous(&'static [&'static str]),
            Unconverted,
        }
        struct PendingRow {
            exchange: String,
            exchange_currency: String,
            row: serde_json::Value,
            class: RowClass,
        }

        // Classify every row; drop non-common instruments first, counted.
        let mut stats = RowFilterStats::default();
        let mut pending: Vec<PendingRow> = Vec::new();
        let mut exchange_order: Vec<String> = Vec::new();
        for (code, rate, rows) in buckets {
            let exchange_currency = context
                .currency_by_exchange
                .get(&code)
                .cloned()
                .unwrap_or_default();
            if !exchange_order.contains(&code) {
                exchange_order.push(code.clone());
            }
            for row in rows {
                // Non-common instruments were already filtered upstream
                // (filter_non_common runs on every fetch path, with or
                // without an FX context).
                let symbol = row
                    .get("currency_symbol")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let class = if symbols_for(&exchange_currency).contains(&symbol) {
                    RowClass::SameCurrency(rate)
                } else if let Some(currency) = currency_for_symbol(symbol) {
                    RowClass::Foreign(currency.to_string())
                } else if let Some((_, candidates)) = AMBIGUOUS_SYMBOLS
                    .iter()
                    .find(|(ambiguous, _)| *ambiguous == symbol)
                {
                    RowClass::Ambiguous(candidates)
                } else {
                    RowClass::Unconverted
                };
                pending.push(PendingRow {
                    exchange: code.clone(),
                    exchange_currency: exchange_currency.clone(),
                    row,
                    class,
                });
            }
        }

        let in_band =
            |usd: f64| cap_min.is_none_or(|min| usd >= min) && cap_max.is_none_or(|max| usd <= max);

        // Same-currency pass: annotate + band-enforce. The currencies that
        // keep at least one row here are the home-covered set.
        let mut kept_currencies: std::collections::HashSet<String> = Default::default();
        let mut kept: Vec<PendingRow> = Vec::new();
        let mut foreign_pending: Vec<PendingRow> = Vec::new();
        for mut entry in pending {
            let RowClass::SameCurrency(rate) = entry.class else {
                foreign_pending.push(entry);
                continue;
            };
            let cap = entry
                .row
                .get("market_capitalization")
                .and_then(|value| value.as_f64());
            match cap.map(|cap| cap / rate) {
                Some(usd) => {
                    if !in_band(usd) {
                        stats.out_of_band_dropped += 1;
                        continue;
                    }
                    annotate_usd(&mut entry.row, usd);
                    kept_currencies.insert(entry.exchange_currency.clone());
                    kept.push(entry);
                }
                None => {
                    // No cap → unconverted, but the listing exists: it still
                    // counts as home coverage.
                    stats.unconverted_rows += 1;
                    kept_currencies.insert(entry.exchange_currency.clone());
                    kept.push(entry);
                }
            }
        }

        // Foreign/ambiguous decision: drop only when the home currency
        // actually kept rows in this screen; otherwise keep — the foreign
        // line is the company's only surface.
        let mut foreign_kept: Vec<PendingRow> = Vec::new();
        let mut needed_currencies: Vec<String> = Vec::new();
        for entry in foreign_pending {
            let drop = match &entry.class {
                RowClass::Foreign(currency) => kept_currencies.contains(currency),
                RowClass::Ambiguous(candidates) => candidates
                    .iter()
                    .any(|currency| kept_currencies.contains(*currency)),
                _ => false,
            };
            if drop {
                stats.foreign_lines_dropped += 1;
                continue;
            }
            match &entry.class {
                RowClass::Foreign(currency) => {
                    needed_currencies.push(currency.clone());
                    foreign_kept.push(entry);
                }
                _ => {
                    stats.unconverted_rows += 1;
                    kept.push(entry);
                }
            }
        }

        // Fetch the extra currency rates (concurrent, cached 24h).
        let mut distinct = needed_currencies;
        distinct.sort();
        distinct.dedup();
        let fetches = distinct
            .iter()
            .map(|currency| self.cached_forex_rate(currency));
        let mut extra_rates: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for (currency, outcome) in distinct
            .iter()
            .zip(futures::future::join_all(fetches).await)
        {
            if let Ok((_, rate)) = outcome {
                extra_rates.insert(currency.clone(), rate);
            }
        }

        // Foreign pass: annotate + band-enforce at the foreign currency's
        // own rate. A missing rate keeps the row unconverted (never dropped
        // silently).
        for mut entry in foreign_kept {
            let currency = match &entry.class {
                RowClass::Foreign(currency) => currency.clone(),
                _ => continue,
            };
            let cap = entry
                .row
                .get("market_capitalization")
                .and_then(|value| value.as_f64());
            let usd = extra_rates
                .get(&currency)
                .and_then(|rate| cap.map(|cap| cap / rate));
            match usd {
                Some(usd) => {
                    if !in_band(usd) {
                        stats.out_of_band_dropped += 1;
                        continue;
                    }
                    annotate_usd(&mut entry.row, usd);
                    kept.push(entry);
                }
                None => {
                    stats.unconverted_rows += 1;
                    kept.push(entry);
                }
            }
        }

        // Regroup by exchange, preserving the original bucket order.
        let mut kept_buckets: Vec<(String, Vec<serde_json::Value>)> = exchange_order
            .into_iter()
            .map(|code| (code, Vec::new()))
            .collect();
        for entry in kept {
            if let Some((_, rows)) = kept_buckets
                .iter_mut()
                .find(|(code, _)| *code == entry.exchange)
            {
                rows.push(entry.row);
            }
        }

        let extra_rates_json: serde_json::Map<String, serde_json::Value> = extra_rates
            .iter()
            .map(|(currency, rate)| (currency.clone(), serde_json::json!(rate)))
            .collect();
        (kept_buckets, stats, extra_rates_json)
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

// ── Screener USD conversion ─────────────────────────────────────────────

/// USD-conversion context failures for the screener: the exchanges-list
/// failure degrades the whole conversion; per-exchange failures surface
/// per-exchange via [`ScreenerFx::rate_for`].
#[derive(Debug, thiserror::Error)]
enum ScreenerFxError {
    #[error("EODHD exchanges list fetch failed: {0}")]
    ExchangesList(#[source] McpToolError),
    #[error("no currency mapping for exchange {code} — not in the EODHD exchange list")]
    NoCurrencyMapping { code: String },
    #[error("FX rate USD{currency} unavailable")]
    RateUnavailable { currency: String },
}

/// USD conversion context for the screener: the exchange→currency map from
/// the EODHD Exchanges API and USD→currency rates from EODHD FOREX EOD
/// closes, both cached 24h in the fibo cache.
struct ScreenerFx {
    as_of: String,
    currency_by_exchange: std::collections::HashMap<String, String>,
    rate_by_currency: std::collections::HashMap<String, f64>,
}

impl ScreenerFx {
    /// Build conversion context for a USD-only screen without an exchange-list
    /// or FOREX request. The row pass still validates each row's currency
    /// symbol before annotating its USD market capitalization.
    fn usd_only(codes: &[String]) -> Self {
        Self {
            as_of: String::new(),
            currency_by_exchange: codes
                .iter()
                .map(|code| (code.clone(), "USD".to_string()))
                .collect(),
            rate_by_currency: std::collections::HashMap::new(),
        }
    }

    /// USD→listing-currency rate for an exchange code (1.0 for USD).
    /// Errors name the reason: an unmapped code (not in the EODHD exchange
    /// list) or a missing rate for its currency.
    fn rate_for(&self, code: &str) -> Result<f64, ScreenerFxError> {
        match self.currency_by_exchange.get(code) {
            None => Err(ScreenerFxError::NoCurrencyMapping {
                code: code.to_string(),
            }),
            Some(currency) if currency == "USD" => Ok(1.0),
            Some(currency) => self.rate_by_currency.get(currency).copied().ok_or_else(|| {
                ScreenerFxError::RateUnavailable {
                    currency: currency.clone(),
                }
            }),
        }
    }
}

/// Currency → the row `currency_symbol` values EODHD uses for it.
const CURRENCY_SYMBOLS: &[(&str, &[&str])] = &[
    ("USD", &["$"]),
    ("CAD", &["C$"]),
    ("MXN", &["₱"]),
    ("GBP", &["£", "p"]),
    ("EUR", &["€"]),
    ("JPY", &["¥"]),
    ("CHF", &["CHF"]),
    ("SEK", &["kr"]),
    ("NOK", &["kr"]),
    ("DKK", &["kr"]),
    ("PLN", &["zł"]),
    ("CZK", &["Kč"]),
    ("HUF", &["Ft"]),
    ("VND", &["₫"]),
];

/// The row symbols of a currency (empty for unknown currencies).
fn symbols_for(currency: &str) -> &'static [&'static str] {
    CURRENCY_SYMBOLS
        .iter()
        .find(|(known, _)| *known == currency)
        .map(|(_, symbols)| *symbols)
        .unwrap_or(&[])
}

/// Row `currency_symbol` → currency code, for symbols that map to exactly
/// one currency. "kr" is deliberately absent (SEK/NOK/DKK).
fn currency_for_symbol(symbol: &str) -> Option<&'static str> {
    match symbol {
        "$" => Some("USD"),
        "C$" => Some("CAD"),
        "₱" => Some("MXN"),
        "£" | "p" => Some("GBP"),
        "€" => Some("EUR"),
        "¥" => Some("JPY"),
        "CHF" => Some("CHF"),
        "zł" => Some("PLN"),
        "Kč" => Some("CZK"),
        "Ft" => Some("HUF"),
        "₫" => Some("VND"),
        _ => None,
    }
}

/// Symbols that map to several currencies, with their candidates.
const AMBIGUOUS_SYMBOLS: &[(&str, &[&str])] = &[("kr", &["SEK", "NOK", "DKK"])];

/// Exchanges whose screener rows are quoted in multiple currencies —
/// London's International Order Book hosts Japanese ¥, Nordic kr, and
/// Hungarian Ft lines alongside £ lines. Server-side cap bounds converted
/// into the exchange's currency would numerically exclude every
/// foreign-currency row (a ¥9.2T cap sits above any GBP ceiling), so these
/// exchanges are queried unbounded and selection happens in the
/// row-currency pass plus client-side band enforcement. Verified live
/// 2026-09-09: the bounded LSE query returned zero ¥ rows.
const MIXED_CURRENCY_EXCHANGES: &[&str] = &["LSE"];

fn listing_symbol(row: &serde_json::Value) -> String {
    let code = row
        .get("code")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let exchange = row
        .get("exchange")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    format!("{code}.{exchange}")
}

fn trailing_average_dollar_volume_usd(
    history: &serde_json::Value,
    currency_symbol: &str,
    fx_history: Option<&serde_json::Value>,
) -> Result<(f64, u64), String> {
    let rows = history
        .as_array()
        .ok_or_else(|| "EODHD EOD history is not an array".to_string())?;
    let major_per_price_unit = if currency_symbol == "p" { 0.01 } else { 1.0 };
    let fx_by_date = match fx_history {
        Some(history) => {
            let fx_rows = history
                .as_array()
                .ok_or_else(|| "EODHD FOREX history is not an array".to_string())?;
            let mut rates = std::collections::HashMap::new();
            for row in fx_rows {
                if let (Some(date), Some(rate)) = (
                    row.get("date").and_then(|value| value.as_str()),
                    row.get("close").and_then(|value| value.as_f64()),
                ) && rate.is_finite()
                    && rate > 0.0
                {
                    rates.insert(date.to_string(), rate);
                }
            }
            Some(rates)
        }
        None => None,
    };

    let mut total = 0.0;
    let mut observations = 0_u64;
    for row in rows {
        let Some(close) = row.get("close").and_then(|value| value.as_f64()) else {
            continue;
        };
        let Some(volume) = row.get("volume").and_then(|value| value.as_f64()) else {
            continue;
        };
        if !close.is_finite() || close <= 0.0 || !volume.is_finite() || volume < 0.0 {
            continue;
        }
        let units_per_usd = match &fx_by_date {
            Some(rates) => {
                let date = row
                    .get("date")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| "non-USD EOD row has no date for FX matching".to_string())?;
                *rates
                    .get(date)
                    .ok_or_else(|| format!("no date-matched EODHD FOREX close for {date}"))?
            }
            None => 1.0,
        };
        let dollar_volume = close * major_per_price_unit * volume / units_per_usd;
        if !dollar_volume.is_finite() {
            continue;
        }
        total += dollar_volume;
        observations += 1;
    }
    if observations == 0 {
        return Err("EODHD EOD history has no valid close-volume observations".to_string());
    }
    Ok((total / observations as f64, observations))
}

/// Row-currency pass counters, surfaced in the tool output.
#[derive(Default)]
struct RowFilterStats {
    foreign_lines_dropped: u64,
    out_of_band_dropped: u64,
    unconverted_rows: u64,
    non_common_dropped: u64,
}

/// Stamp a row with its USD market cap.
fn annotate_usd(row: &mut serde_json::Value, usd: f64) {
    if let Some(number) = serde_json::Number::from_f64(usd)
        && let Some(object) = row.as_object_mut()
    {
        object.insert(
            "market_capitalization_usd".to_string(),
            serde_json::Value::Number(number),
        );
    }
}

/// Drop non-common-stock instruments from a row set, counting every drop.
/// Applied on every fetch path (with or without an FX context) — EODHD's
/// screener has no type filter.
fn filter_non_common(rows: &mut Vec<serde_json::Value>, stats: &mut RowFilterStats) {
    rows.retain(|row| {
        let name = row
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let ticker = row
            .get("code")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let common = !is_non_common_instrument(ticker, name);
        if !common {
            stats.non_common_dropped += 1;
        }
        common
    });
}

/// Non-common-stock instruments the EODHD screener cannot exclude
/// server-side (it has no type filter): ETFs/ETPs, Canadian depositary
/// receipts (a derivative wrapper duplicating the underlying's home
/// listing), preferred shares (FMP-style `-P*` ticker suffixes), and
/// notes/bonds (coupon-bearing names). Dropped client-side and counted in
/// `non_common_dropped` — never silent. ADRs are deliberately KEPT: they
/// are the US surface of foreign companies, not wrappers of a screened
/// listing. European dual-class tickers (`-A`/`-B`/`-C`, e.g. `MAERSK-B`)
/// are common shares and do not match the preferred pattern.
fn is_non_common_instrument(ticker: &str, name: &str) -> bool {
    let upper = name.to_uppercase();
    if upper.contains("ETF")
        || upper.contains("ETP")
        || upper.contains("EXCHANGE TRADED")
        || upper.contains(" CDR (")
        || upper.contains("JUNIOR SUBORDINATE")
        || upper.contains("PREFERRED")
        || upper.contains(" PFD")
        || upper.contains(" PREF ")
        || upper.contains("NOTES")
        || upper.contains('%')
    {
        return true;
    }
    let base = ticker.split('.').next().unwrap_or(ticker);
    if let Some((_, suffix)) = base.rsplit_once('-')
        && suffix.len() <= 2
        && suffix.starts_with('P')
        && suffix
            .chars()
            .all(|character| character.is_ascii_uppercase())
    {
        return true;
    }
    false
}
