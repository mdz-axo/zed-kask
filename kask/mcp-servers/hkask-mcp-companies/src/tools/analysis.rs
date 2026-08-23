//! MAIA analysis and research tools.
use crate::{
    CompaniesServer, analysis, fibo, providers, research, screener,
    types::{self, SymbolLimitRequest, SymbolRequest},
    validate_symbol,
};
use hkask_mcp_server::server::execute_tool_semantic;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = analysis_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Analyze competitive moat using MAIA framework: gross margin stability and working capital market power signal"
    )]
    pub async fn moat_check(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> String {
        execute_tool_semantic(
            self,
            "moat_check",
            Self::ontology_anchor("moat_check"),
            async {
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
            },
        )
        .await
    }

    #[tool(
        description = "CEO capital allocation scorecard (MAIA framework): rates how well management allocates capital by comparing returns on capital vs invested capital over time"
    )]
    pub async fn management_scorecard(
        &self,
        Parameters(SymbolRequest { symbol }): Parameters<SymbolRequest>,
    ) -> String {
        execute_tool_semantic(self, "management_scorecard", Self::ontology_anchor("management_scorecard"), async {
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
    ) -> String {
        execute_tool_semantic(self, "working_capital_cycle", Self::ontology_anchor("working_capital_cycle"), async {
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
        description = "Company screener powered by EODHD Screener API. Parses natural language prompts into EODHD filter triples and returns a data table with all criteria values for each matching company. Supports: market cap, price, volume, average volume, EPS, dividend yield, sector, industry, exchange, daily/weekly price change. Post-screen criteria (revenue growth, ROIC, ROE, P/E, debt/equity, price/book, beta) are parsed and returned but require per-company fundamentals — use key_metrics for those. Use criteria_overrides to adjust parsed criteria. Paginates automatically to exhaust the full universe beyond the 1,000-result offset limit."
    )]
    pub async fn company_screener(
        &self,
        Parameters(req): Parameters<types::ScreenerRequest>,
    ) -> String {
        execute_tool_semantic(self, "company_screener", Self::ontology_anchor("company_screener"), async {
            // Parse the natural language prompt into structured criteria
            let mut criteria = screener::parse_screening_prompt(&req.prompt);

            // Apply user overrides — merge override fields into parsed criteria
            if !req.criteria_overrides.is_null()
                && let Some(override_obj) = req.criteria_overrides.as_object()
                && let Some(crit_obj) = criteria.as_object_mut()
            {
                for (k, v) in override_obj {
                    crit_obj.insert(k.clone(), v.clone());
                }
            }

            // Split criteria into EODHD screener filters and post-screen filters
            let (screener_filters, post_screen_filters) = screener::split_criteria(&criteria);

            let filter_count = screener_filters.len();
            let post_screen_count = post_screen_filters.as_object().map(|m| m.len()).unwrap_or(0);

            // Call EODHD Screener API with the screener-compatible filters
            let rows = if filter_count > 0 {
                providers::fetch_eodhd_screener(
                    &self.client,
                    &self.eodhd_api_key,
                    &screener_filters,
                )
                .await?
            } else {
                // No screener filters — fetch the full universe sorted by market cap
                providers::fetch_eodhd_screener(
                    &self.client,
                    &self.eodhd_api_key,
                    &[],
                )
                .await?
            };

            let count = rows.len();

            let mut output = serde_json::json!({
                "prompt": req.prompt,
                "parsed_criteria": criteria,
                "screener_filters": screener_filters,
                "post_screen_filters": post_screen_filters,
                "count": count,
                "results": rows,
                "fibo": {
                    "screener": fibo::STOCK_SCREENER,
                    "market_capitalization": fibo::MARKET_CAPITALIZATION,
                    "price_earnings_ratio": fibo::PRICE_EARNINGS_RATIO,
                    "dividend_yield": fibo::DIVIDEND_YIELD,
                },
                "framework": "EODHD Screener API. Parses natural language screening prompts into EODHD filter triples ([field, operation, value], AND-combined). Screener-compatible fields: market_capitalization, adjusted_close, avgvol_1d, avgvol_200d, earnings_share, dividend_yield, sector, industry, exchange, refund_1d_p, refund_5d_p. Post-screen fields (revenue_growth, roic, roe, pe_ratio, debt_equity, price_book, beta) are parsed and returned in post_screen_filters but require per-company fundamentals from key_metrics. Paginates with market cap band splitting to exhaust the full universe beyond the 1,000-result offset limit.",
                "source": "EODHD Screener API"
            });

            if post_screen_count > 0 {
                if let Some(obj) = output.as_object_mut() {
                    obj.insert(
                        "warning".to_string(),
                        serde_json::Value::String(
                            "Post-screen criteria require per-company fundamentals. Use key_metrics for each result to apply these filters."
                                .to_string(),
                        ),
                    );
                }
            }

            Ok(fibo::enrich_with_ontology(output, "company_screener"))
        })
        .await
    }

    #[tool(
        description = "Exhaustive stock universe listing from EODHD Screener API. Returns ALL stocks on the specified exchange with market cap above the threshold, paginating through market cap bands to exhaust the full universe. Each row includes symbol, name, exchange, price, market cap, sector, and industry. Use this as Stage 1 of a multi-stage screen (financial filter, then expectations gap). Default: US exchange, market cap above $500M."
    )]
    pub async fn stock_universe(
        &self,
        Parameters(req): Parameters<types::StockUniverseRequest>,
    ) -> String {
        execute_tool_semantic(self, "stock_universe", Self::ontology_anchor("company_screener"), async {
            let listings = providers::fetch_eodhd_screener_listing(
                &self.client,
                &self.eodhd_api_key,
                &req.exchange,
                req.min_market_cap,
            )
            .await?;

            let count = listings.len();

            let output = serde_json::json!({
                "exchange": req.exchange,
                "min_market_cap": req.min_market_cap,
                "count": count,
                "results": listings,
                "fibo": {
                    "screener": fibo::STOCK_SCREENER,
                    "market_capitalization": fibo::MARKET_CAPITALIZATION,
                },
                "source": "EODHD Screener API",
            });

            Ok(fibo::enrich_with_ontology(output, "company_screener"))
        })
        .await
    }
    pub async fn research_search(
        &self,
        Parameters(req): Parameters<types::ResearchSearchRequest>,
    ) -> String {
        execute_tool_semantic(self, "research_search", Self::ontology_anchor("research_search"), async {
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

            Ok(fibo::enrich_with_ontology(output, "research_search"))
        }).await
    }

    // ── Portfolio tools ──
}
