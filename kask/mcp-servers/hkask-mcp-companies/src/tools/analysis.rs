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
        description = "Company screener powered by the EODHD Screener API. Parses natural-language prompts into EODHD filter triples and returns a data table with all criteria values for each matching company. Keywords are field names in space or underscore form (market cap / market_capitalization, price, volume, average volume, eps, dividend yield, sector, industry, daily/weekly change). Operators: above/over/more than/greater than/higher than/>/>=/at least; below/under/less than/lower than/fewer than/</<=/at most; between X and Y; equals/is/= for string fields. Values accept $, thousands commas, and B/M/K/T or billion/million/thousand suffixes. Geography (US, Japan, Canada, Mexico, Europe, UK, Germany, ...) maps to EODHD exchange codes and fans out one query per exchange, interleaved round-robin so every exchange is represented; results are listings, so a cross-listed company appears once per exchange. USD-stated market-cap bounds are converted into each exchange's listing currency using EODHD FOREX daily closes (cached 24h): rows carry market_capitalization_usd and results rank by USD cap; exchanges without a currency mapping or FX rate are dropped and named in exchange_errors. Parsed criteria are echoed in parsed_criteria — verify them and correct with criteria_overrides (keys: market_capitalization_min/_max and the other _min/_max bounds, exchanges: [codes], sector, industry). Post-screen criteria (revenue growth, ROIC, ROE, P/E, debt/equity, price/book, beta) require per-company fundamentals — use key_metrics for those. Paginates automatically beyond the 1,000-result offset limit."
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
            // exchange code. Filter triples never carry the exchange
            // dimension.
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

            // USD conversion: EODHD reports market caps in each listing's
            // currency, so USD-stated bounds must be converted per exchange
            // (FOREX daily closes, cached 24h) to select the intended band.
            let cap_bounds_present = ["market_capitalization_min", "market_capitalization_max"]
                .iter()
                .any(|key| criteria.get(*key).is_some());
            let conversion_active =
                cap_bounds_present && exchange_codes.iter().any(|code| code != "US");
            let fx = if conversion_active {
                Some(self.acquire_screener_fx(&exchange_codes).await)
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
            let (merged_rows, exchange_counts) = if exchange_codes.is_empty() {
                let rows = providers::fetch_eodhd_screener(
                    &self.client,
                    &self.eodhd_api_key,
                    &screener_filters,
                )
                .await?;
                (rows, serde_json::Map::new())
            } else {
                // Build per-exchange queries. With an FX context, cap bounds
                // are converted into each exchange's listing currency
                // (rate 1.0 for USD); exchanges without a currency mapping
                // or rate are dropped and named. Without one, bounds pass
                // through unconverted.
                let mut queries: Vec<(String, f64, Vec<serde_json::Value>)> = Vec::new();
                match &fx {
                    Some(Ok(context)) => {
                        for code in &exchange_codes {
                            match context.rate_for(code) {
                                Ok(rate) => {
                                    let mut filters = screener_filters.clone();
                                    if rate != 1.0 {
                                        filters = convert_cap_filters(&filters, rate);
                                    }
                                    filters.push(serde_json::json!(["exchange", "=", code]));
                                    queries.push((code.clone(), rate, filters));
                                }
                                Err(reason) => {
                                    exchange_errors.insert(
                                        code.clone(),
                                        serde_json::Value::String(reason),
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

                let fetches = queries.iter().map(|(code, rate, filters)| {
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
                });
                let results = futures::future::join_all(fetches).await;

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
                        Ok(rows) => buckets.push((code, rate, rows)),
                        Err(error) => {
                            exchange_errors
                                .insert(code, serde_json::Value::String(error.to_string()));
                        }
                    }
                }

                // Annotate rows with a USD market cap so cross-exchange
                // comparison and ranking are currency-consistent.
                let annotate = matches!(&fx, Some(Ok(_)));
                if annotate {
                    for (_, rate, rows) in &mut buckets {
                        for row in rows.iter_mut() {
                            if let Some(cap) = row
                                .get("market_capitalization")
                                .and_then(|value| value.as_f64())
                                && let Some(usd) = serde_json::Number::from_f64(cap / *rate)
                                && let Some(object) = row.as_object_mut()
                            {
                                object.insert(
                                    "market_capitalization_usd".to_string(),
                                    serde_json::Value::Number(usd),
                                );
                            }
                        }
                    }
                }

                // Merge: with USD conversion, rank by market_capitalization_usd
                // (the cap-comparable order the truncation contract
                // promises); without it, round-robin interleave so every
                // exchange is represented under `limit` truncation.
                let mut merged: Vec<serde_json::Value> = if annotate {
                    let mut all: Vec<serde_json::Value> = buckets
                        .iter()
                        .flat_map(|(_, _, rows)| rows.iter().cloned())
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
                        .map(|(_, _, rows)| rows.len())
                        .max()
                        .unwrap_or(0);
                    for index in 0..longest {
                        for (_, _, rows) in &buckets {
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
                    .map(|(code, _, rows)| {
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

            let mut output = serde_json::json!({
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
                "framework": "EODHD Screener API. Parses natural-language prompts into EODHD filter triples ([field, operation, value], AND-combined). Keywords: field names in space or underscore form (market cap / market_capitalization, price / adjusted_close, volume / avgvol_1d, average volume / avgvol_200d, eps / earnings_share, dividend yield, sector, industry, daily change / refund_1d_p, weekly change / refund_5d_p). Operators: above/over/more than/greater than/higher than/>/>=/at least; below/under/less than/lower than/fewer than/</<=/at most; between X and Y; equals/is/= for string fields. Values accept $, thousands commas, and B/M/K/T or billion/million/thousand suffixes — the suffix binds to its number ('between 2 and 200 billion' parses as min 2, max 2e11). Geography: country/region names and major exchange codes map to EODHD exchange codes under parsed_criteria.exchanges and fan out one query per exchange; results are listings (a cross-listed company appears once per exchange). USD-stated market-cap bounds are converted per exchange into the listing currency using EODHD FOREX daily closes (cached 24h; the fx object carries rates and the as-of date) and rows carry market_capitalization_usd, ranked by USD cap; without geography, or when the FX context is unavailable (warned), bounds apply in each listing's local currency. Sector/industry are single-value. Verify parsed_criteria and correct with criteria_overrides (keys: market_capitalization_min/_max and other _min/_max bounds, exchanges: [codes], sector, industry). Post-screen fields (revenue_growth, roic, roe, pe_ratio, debt_equity, price_book, beta) require per-company fundamentals from key_metrics. Paginates automatically beyond the 1,000-result offset limit.",
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
                            "note": "USD market-cap bounds converted per exchange with EODHD FOREX daily closes; rows carry market_capitalization_usd."
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
    async fn acquire_screener_fx(&self, codes: &[String]) -> Result<ScreenerFx, String> {
        let list = self
            .cached_exchanges_list()
            .await
            .map_err(|error| format!("EODHD exchanges list fetch failed: {error}"))?;

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

    /// The latest USD→currency FOREX close, cached 24h.
    async fn cached_forex_rate(&self, currency: &str) -> Result<(String, f64), McpToolError> {
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

/// USD conversion context for the screener: the exchange→currency map from
/// the EODHD Exchanges API and USD→currency rates from EODHD FOREX EOD
/// closes, both cached 24h in the fibo cache.
struct ScreenerFx {
    as_of: String,
    currency_by_exchange: std::collections::HashMap<String, String>,
    rate_by_currency: std::collections::HashMap<String, f64>,
}

impl ScreenerFx {
    /// USD→listing-currency rate for an exchange code (1.0 for USD).
    /// Errors name the reason: an unmapped code (not in the EODHD exchange
    /// list) or a missing rate for its currency.
    fn rate_for(&self, code: &str) -> Result<f64, String> {
        match self.currency_by_exchange.get(code) {
            None => Err(format!(
                "no currency mapping for exchange {code} — not in the EODHD exchange list"
            )),
            Some(currency) if currency == "USD" => Ok(1.0),
            Some(currency) => self
                .rate_by_currency
                .get(currency)
                .copied()
                .ok_or_else(|| format!("FX rate USD{currency} unavailable")),
        }
    }
}

/// Convert market_capitalization filter bounds into a listing currency
/// (multiply by the USD→currency rate); other filters pass through.
fn convert_cap_filters(filters: &[serde_json::Value], rate: f64) -> Vec<serde_json::Value> {
    filters
        .iter()
        .map(|filter| {
            let Some(parts) = filter.as_array() else {
                return filter.clone();
            };
            if parts.first().and_then(|field| field.as_str()) != Some("market_capitalization") {
                return filter.clone();
            }
            let Some(value) = parts.get(2).and_then(|value| value.as_f64()) else {
                return filter.clone();
            };
            let mut converted = parts.clone();
            converted[2] = serde_json::json!(value * rate);
            serde_json::Value::Array(converted)
        })
        .collect()
}
