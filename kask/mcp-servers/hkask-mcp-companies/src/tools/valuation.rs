//! Valuation and forecasting tools.
use super::notes::run_store;
use crate::{
    CompaniesServer, CompanyProfile, KeyMetrics, Provider, StoredForecast,
    current_price_from_multiple, fibo, financial_model, parse_symbol_from_query,
    projected_terminal_multiple, providers, research_store::PersistedForecast, scenarios,
    superforecast, types, validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::time::now_rfc3339;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use uuid::Uuid;

/// Result of auto-peer discovery for comparable analysis.
struct DiscoveredPeers {
    peers: Vec<String>,
    source: String,
}

fn validate_finite(name: &str, value: f64) -> Result<(), McpToolError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(McpToolError::invalid_argument(format!(
            "{name} must be finite"
        )))
    }
}

/// Classify ScenarioImpactError per variant, not blanket `internal`.
/// All variants are invalid-argument (malformed input tree/mappings).
fn map_scenario_impact_error(err: financial_model::ScenarioImpactError) -> McpToolError {
    McpToolError::invalid_argument(err.to_string())
}

fn validate_unit_interval(name: &str, value: f64) -> Result<(), McpToolError> {
    validate_finite(name, value)?;
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(McpToolError::invalid_argument(format!(
            "{name} must be within 0.0..=1.0"
        )))
    }
}

/// Extract non-empty financial statement arrays and the profile object from
/// the raw provider responses. Returns `None` if any required array is empty
/// or missing, so callers can surface an "insufficient data" error without
/// panicky `unwrap()` calls on guarded `Option<&[Value]>`.
fn extract_historical_arrays<'a>(
    income: &'a serde_json::Value,
    balance: &'a serde_json::Value,
    cf: &'a serde_json::Value,
    metrics: &'a serde_json::Value,
    profile: &'a serde_json::Value,
) -> Option<(
    &'a [serde_json::Value],
    &'a [serde_json::Value],
    &'a [serde_json::Value],
    &'a [serde_json::Value],
    &'a serde_json::Value,
)> {
    let income_data = income.as_array().filter(|a| !a.is_empty())?;
    let balance_data = balance.as_array().filter(|a| !a.is_empty())?;
    let cf_data = cf.as_array().filter(|a| !a.is_empty())?;
    let metrics_data: &[serde_json::Value] = metrics.as_array().map_or(&[], |v| v);
    let profile_data = profile.as_array().and_then(|a| a.first())?;
    Some((
        income_data,
        balance_data,
        cf_data,
        metrics_data,
        profile_data,
    ))
}

#[tool_router(router = valuation_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Comparable company analysis. Gathers valuation multiples (P/E, P/B, P/S, EV/EBITDA) from peer companies in the same industry, alongside a DCF intrinsic value overlay for the target. Multiples provide market-relative context; DCF provides fundamentals-anchored valuation. Accepts optional comma-separated peer list. When no peers are provided, auto-discovers peers using the EODHD Screener API — same sector, similar market cap (0.25x–4x of target), ranked by market-cap proximity. Methodology: CFA Institute (Knudsen et al. 2017), Damodaran (NYU Stern), IB best practices (5–12 peers)."
    )]
    pub async fn comparable_analysis(
        &self,
        Parameters(req): Parameters<types::ComparableAnalysisRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "comparable_analysis", async {
            validate_symbol(&req.symbol)?;

            // 1. Fetch target company profile and key_metrics as typed views.
            //    A missing field is `None`, not a silent zero — the field-name
            //    knowledge lives in the `CompanyProfile`/`KeyMetrics` accessors.
            let profile = self.fetch_profile(&req.symbol).await?;
            let metrics = self.fetch_key_metrics(&req.symbol, 1).await?;

            let Some(profile_data) = profile.raw().as_array().and_then(|a| a.first()) else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "company profile not found"}));
            };

            // 2. Parse peers (comma-separated) or auto-discover
            let peers: Vec<String> = req
                .peers
                .as_ref()
                .map(|s| {
                    s.split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect()
                })
                .unwrap_or_default();

            // Auto-discover peers when none are provided.
            // Uses EODHD Screener to find same-sector companies with
            // similar market cap (0.25x–4x of target), ranked by proximity.
            // Methodology: CFA Institute (Knudsen et al. 2017) — 6 peers
            // minimum; IB best practices — 5–12 peers.
            let (peers, peer_source) = if peers.is_empty() {
                let discovered = self.discover_peers(&req.symbol, &profile).await?;
                (discovered.peers, discovered.source)
            } else {
                (peers, "user-provided".to_string())
            };

            // 3. Fetch peer profiles and metrics concurrently. A failed peer
            //    fetch yields an empty `CompanyProfile` (raw `Null`), so its
            //    accessors return `None` and the peer row carries no multiples.
            //    Concurrent fetch keeps the total wall-clock time under the
            //    MCP 60s cap (sequential = 8 peers × 2 calls × ~1s = 16s+;
            //    concurrent = max single fetch ≈ 2s).
            let mut peer_tasks = tokio::task::JoinSet::new();
            for peer_sym in &peers {
                let peer_sym = peer_sym.clone();
                let client = self.client.clone();
                let fmp_key = self.fmp_api_key.clone();
                let eodhd_key = self.eodhd_api_key.clone();
                peer_tasks.spawn(async move {
                    let profile_resp = providers::companies_get(
                        &client, "company_profile", &peer_sym, &fmp_key, &eodhd_key, &[], None,
                    )
                    .await
                    .unwrap_or_else(|_| providers::ProviderResponse {
                        value: serde_json::Value::Null,
                        provider: providers::Provider::Fmp,
                    });
                    let pp = CompanyProfile::from_raw(profile_resp.value);
                    let metrics_resp = providers::fetch_key_metrics(
                        &client, &peer_sym, 1, &fmp_key, &eodhd_key, None,
                    )
                    .await
                    .unwrap_or_else(|_| KeyMetrics::from_raw(serde_json::Value::Array(vec![])));
                    (peer_sym, pp, metrics_resp)
                });
            }
            let mut peer_data: Vec<(String, CompanyProfile, KeyMetrics)> = Vec::new();
            while let Some(result) = peer_tasks.join_next().await {
                if let Ok(entry) = result {
                    peer_data.push(entry);
                }
            }

            // 4. Build comparison table. Field-name knowledge lives in the
            //    `CompanyProfile`/`KeyMetrics` accessors, not inline here.
            let build_row = |sym: &str, profile: &CompanyProfile, metrics: &KeyMetrics| -> serde_json::Value {
                let mut row = serde_json::json!({
                    "symbol": sym,
                    "name": profile.company_name().unwrap_or(""),
                });
                if let Some(v) = profile.price() {
                    row["price"] = serde_json::json!(v);
                }
                if let Some(v) = profile.market_cap() {
                    row["market_cap"] = serde_json::json!(v);
                }
                if let Some(v) = metrics.pe_ratio() {
                    row["pe_ratio"] = serde_json::json!(v);
                }
                if let Some(v) = metrics.price_to_book() {
                    row["price_to_book"] = serde_json::json!(v);
                }
                if let Some(v) = metrics.price_to_sales() {
                    row["price_to_sales"] = serde_json::json!(v);
                }
                if let Some(v) = metrics.ev_to_ebitda() {
                    row["ev_to_ebitda"] = serde_json::json!(v);
                }
                if let Some(v) = metrics.dividend_yield() {
                    row["dividend_yield"] = serde_json::json!(v);
                }
                if let Some(v) = metrics.revenue_growth() {
                    row["revenue_growth"] = serde_json::json!(v);
                }
                row
            };

            let mut comparison = vec![build_row(&req.symbol, &profile, &metrics)];
            for (sym, pp, pm) in &peer_data {
                comparison.push(build_row(sym, pp, pm));
            }

            // 5. DCF overlay on target
            let dcf_overlay = self.build_dcf_overlay(&req, profile_data).await?;

            let company_name = profile.company_name().unwrap_or("");
            let sector = profile.sector().unwrap_or("");
            let industry = profile.industry().unwrap_or("");

            let output = serde_json::json!({
                "symbol": req.symbol,
                "company_name": company_name,
                "sector": sector,
                "industry": industry,
                "peers": peers,
                "peer_source": peer_source,
                "dcf_overlay": dcf_overlay,
                "comparison": comparison,
                "framework": "Comparable company analysis. Valuation multiples (P/E, P/B, P/S) from peer companies alongside DCF intrinsic value. Multiples provide market-relative context; DCF provides fundamentals-anchored valuation. Auto-peer discovery: CFA Institute (Knudsen et al. 2017) SARD approach, Damodaran (NYU Stern) growth/risk/return matching, IB best practices (5–12 peers).",
            });

            Ok(output)
        })
        .await
    }

    async fn build_dcf_overlay(
        &self,
        req: &types::ComparableAnalysisRequest,
        profile_data: &serde_json::Value,
    ) -> Result<serde_json::Value, McpToolError> {
        let inc_res = self
            .fetch("income_statement", &req.symbol, &[("limit", "5")])
            .await;
        let bal_res = self
            .fetch("balance_sheet", &req.symbol, &[("limit", "5")])
            .await;
        let cf_res = self
            .fetch("cash_flow_statement", &req.symbol, &[("limit", "5")])
            .await;
        let km_res = self
            .fetch("key_metrics", &req.symbol, &[("limit", "5")])
            .await;

        match (inc_res, bal_res, cf_res, km_res) {
            (Ok(inc), Ok(bal), Ok(cf), Ok(km)) => {
                if let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                    extract_historical_arrays(&inc, &bal, &cf, &km, profile_data)
                {
                    let hist = financial_model::HistoricalSnapshot::from_api_json(
                        income_data,
                        balance_data,
                        cf_data,
                        metrics_data,
                        profile_data,
                    );

                    if hist.revenue.len() < 2 {
                        return Ok(serde_json::json!({"error": "insufficient historical data"}));
                    }

                    let overlay_profile = CompanyProfile::from_raw(profile_data.clone());
                    if let Some(err) = financial_model::financial_sector_guard(
                        &overlay_profile,
                        &req.symbol,
                        "dcf_valuation",
                    ) {
                        return Ok(err);
                    }

                    let assumptions =
                        financial_model::ProjectionAssumptions::from_history_with_overrides(
                            &hist,
                            types::ProjectionAssumptionOverrides::from(req),
                        )
                        .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;
                    let current_price = profile_data
                        .get("price")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let model = financial_model::project_model(&hist, &assumptions, current_price);
                    let margin_of_safety = if current_price > 0.0 {
                        (model.intrinsic_per_share - current_price) / current_price
                    } else {
                        0.0
                    };
                    Ok(serde_json::json!({
                        "intrinsic_per_share": model.intrinsic_per_share,
                        "current_price": current_price,
                        "margin_of_safety": margin_of_safety,
                    }))
                } else {
                    Ok(serde_json::json!({"error": "insufficient data for DCF"}))
                }
            }
            _ => Ok(serde_json::json!({"error": "DCF overlay unavailable"})),
        }
    }

    /// Auto-discover peer companies using the EODHD Screener API.
    ///
    /// Methodology (CFA Institute Knudsen et al. 2017, Damodaran NYU Stern,
    /// IB best practices):
    /// 1. Get target's sector + market cap from company profile
    /// 2. Screen EODHD for same-sector companies with market cap in 0.25x–4x range
    /// 3. Exclude the target itself, deduplicate, rank by market-cap proximity
    /// 4. Return top 8 peers (CFA: 6 minimum, IB: 5–12 range)
    async fn discover_peers(
        &self,
        target_symbol: &str,
        profile: &CompanyProfile,
    ) -> Result<DiscoveredPeers, McpToolError> {
        let target_sym_upper = target_symbol.to_uppercase();
        let target_mcap = profile.market_cap().unwrap_or(0.0);
        let sector = profile.sector().unwrap_or("");

        if sector.is_empty() || target_mcap == 0.0 {
            return Ok(DiscoveredPeers {
                peers: Vec::new(),
                source: "auto-discovery failed: missing sector or market cap".to_string(),
            });
        }

        // Market cap band: 0.25x to 4x of target (IB best practice: similar size)
        let mcap_lower = target_mcap * 0.25;
        let mcap_upper = target_mcap * 4.0;

        // EODHD Screener filters: same sector + market cap band
        let filters = vec![
            serde_json::json!(["sector", "=", sector]),
            serde_json::json!(["market_capitalization", ">=", mcap_lower]),
            serde_json::json!(["market_capitalization", "<", mcap_upper]),
            serde_json::json!(["exchange", "=", "US"]),
        ];

        let rows =
            providers::fetch_eodhd_screener(&self.client, &self.eodhd_api_key, &filters).await?;

        // Rank by market-cap proximity to target (closest first)
        let mut candidates: Vec<(String, f64)> = rows
            .iter()
            .filter_map(|row| {
                let code = row.get("code").and_then(|v| v.as_str())?;
                let mcap = row.get("market_capitalization").and_then(|v| v.as_f64())?;
                if code.eq_ignore_ascii_case(&target_sym_upper) {
                    return None;
                }
                let proximity = (mcap - target_mcap).abs() / target_mcap;
                Some((code.to_string(), proximity))
            })
            .collect();

        candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top 8 (CFA: 6 min, IB: 5–12 range)
        let peers: Vec<String> = candidates.iter().take(8).map(|(s, _)| s.clone()).collect();

        let source = format!(
            "auto-discovered via EODHD Screener (sector={}, mcap band ${:.0}B–${:.0}B, {} candidates, top 8)",
            sector,
            mcap_lower / 1e9,
            mcap_upper / 1e9,
            candidates.len(),
        );

        Ok(DiscoveredPeers { peers, source })
    }

    #[tool(
        description = "Tornado chart sensitivity analysis. Varies each DCF driver (revenue growth, gross margin, D&A, capex, NWC, discount rate) by +/- range_pct (default 10%) while holding others constant. Returns drivers ranked by impact on intrinsic value per share. Identifies which assumptions most affect the valuation."
    )]
    pub async fn sensitivity_analysis(
        &self,
        Parameters(req): Parameters<types::SensitivityAnalysisRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "sensitivity_analysis", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

            let (income, balance, cf, metrics, profile) =
                match (income_result, balance_result, cf_result, metrics_result, profile_result) {
                    (Ok(inc), Ok(bal), Ok(cf), Ok(m), Ok(p)) => (inc, bal, cf, m, p),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => {
                        return Err(e);
                    }
                };

            let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                extract_historical_arrays(&income, &balance, &cf, &metrics, profile.raw())
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data for sensitivity analysis"}));
            };

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data — need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "sensitivity_analysis") {
                return Ok(err);
            }

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            financial_model::validate_sensitivity_range(req.range_pct)
                .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let current_price = profile.price().unwrap_or(0.0);

            let base_model = financial_model::project_model(&hist, &assumptions, current_price);
            let base_intrinsic = base_model.intrinsic_per_share;

            let sensitivity_results =
                financial_model::sensitivity_analysis(&hist, &assumptions, req.range_pct);

            let drivers: Vec<serde_json::Value> = sensitivity_results.iter().map(|r| {
                serde_json::json!({
                    "driver": r.driver,
                    "label": r.label,
                    "base_value": r.base_value,
                    "low_value": r.low_value,
                    "high_value": r.high_value,
                    "intrinsic_low": r.intrinsic_low,
                    "intrinsic_high": r.intrinsic_high,
                    "delta_pct": r.delta_pct,
                    "metric": r.metric,
                })
            }).collect();

            let mut metric_map = serde_json::Map::new();
            for r in &sensitivity_results {
                metric_map.insert(
                    r.driver.clone(),
                    serde_json::Value::String(r.metric.to_string()),
                );
            }

            let output = serde_json::json!({
                "symbol": req.symbol,
                "base_intrinsic": base_intrinsic,
                "current_price": current_price,
                "range_pct": req.range_pct,
                "drivers": drivers,
                "metric": metric_map,
                "framework": "Tornado chart sensitivity analysis. Varies each DCF driver by +/- range_pct while holding others constant. Drivers ranked by impact on intrinsic value per share. Identifies which assumptions most affect the valuation.",
            });

            Ok(output)
        }).await
    }

    #[tool(
        description = "Equity duration (Macaulay-style, years) of a company's projected free cash flows: D = Σ t·PV(CF_t) / Σ PV(CF_t) over the projection plus the terminal value timed at the horizon year. Also reports terminal/stage-1/stage-2 PV shares — the maturity profile of the equity claim — and cmp_tenor_gaps, the R2 maturity-transformation gap of the duration against the fixed CMP tenors (1m/3m/6m). Pair with prediction-market time_to_maturity (hkask-mcp-prediction-markets) for duration-matching across horizons."
    )]
    pub async fn equity_duration(
        &self,
        Parameters(req): Parameters<types::EquityDurationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "equity_duration", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

            let (income, balance, cf, metrics, profile) =
                match (income_result, balance_result, cf_result, metrics_result, profile_result) {
                    (Ok(inc), Ok(bal), Ok(cf), Ok(m), Ok(p)) => (inc, bal, cf, m, p),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => {
                        return Err(e);
                    }
                };

            let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                extract_historical_arrays(&income, &balance, &cf, &metrics, profile.raw())
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data — need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "equity_duration") {
                return Ok(err);
            }

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;
            let current_price = profile.price().unwrap_or(0.0);
            let model = financial_model::project_model(&hist, &assumptions, current_price);

            let stage1_years = req.stage1_years.unwrap_or(3);
            let duration = financial_model::equity_duration(&model, stage1_years);

            let output = match duration {
                Some(d) => {
                    // R2: maturity-transformation gap against the fixed CMP tenors
                    // (1m/3m/6m). `duration_vs_cmp_tenors` returns None for a
                    // non-positive duration — surfaced as a note, never silently
                    // dropped.
                    let cmp_tenor_gaps =
                        hkask_forecast::duration_vs_cmp_tenors(d.macaulay_duration_years);
                    serde_json::json!({
                        "symbol": req.symbol,
                        "macaulay_duration_years": d.macaulay_duration_years,
                        "terminal_pv_share": d.terminal_pv_share,
                        "stage1_pv_share": d.stage1_pv_share,
                        "stage2_pv_share": d.stage2_pv_share,
                        "total_pv": d.total_pv,
                        "horizon_years": d.horizon_years,
                        "cmp_tenor_gaps": cmp_tenor_gaps.as_ref().map(|gaps| gaps
                            .iter()
                            .map(|g| serde_json::json!({
                                "tenor": g.tenor_label,
                                "tenor_years": g.tenor_years,
                                "gap_years": g.gap_years,
                                "ratio": g.ratio,
                            }))
                            .collect::<Vec<_>>()),
                        "cmp_tenor_gaps_note": if cmp_tenor_gaps.is_none() {
                            Some("macaulay duration is not positive — CMP tenor comparison undefined (never fabricated)")
                        } else {
                            None
                        },
                        "interpretation": format!(
                            "Equity duration {:.1}y — {:.0}% of value sits in the terminal value at year {}.",
                            d.macaulay_duration_years,
                            d.terminal_pv_share * 100.0,
                            d.horizon_years
                        ),
                        "framework": "Macaulay-style equity duration over projected FCF (terminal value timed at the horizon year). cmp_tenor_gaps is the R2 maturity-transformation gap against the fixed CMP tenors (hkask_forecast::duration_vs_cmp_tenors). Compare against prediction-market time_to_maturity for maturity-transformation analysis.",
                    })
                }
                None => serde_json::json!({
                    "symbol": req.symbol,
                    "error": "total PV is zero — equity duration undefined (never fabricated)",
                }),
            };

            Ok(output)
        }).await
    }

    #[tool(
        description = "Monte Carlo DCF simulation. Runs N simulations (default 1000, clamped 100-10000) with each DCF assumption randomized uniformly within its +/- configured range. Returns intrinsic value distribution (percentiles p10/p25/median/p75/p90, histogram), probability of undervaluation, and base case comparison. Quantifies valuation uncertainty from assumption ranges."
    )]
    pub async fn monte_carlo_dcf(
        &self,
        Parameters(req): Parameters<types::MonteCarloDcfRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "monte_carlo_dcf", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

            let (income, balance, cf, metrics, profile) =
                match (income_result, balance_result, cf_result, metrics_result, profile_result) {
                    (Ok(inc), Ok(bal), Ok(cf), Ok(m), Ok(p)) => (inc, bal, cf, m, p),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => {
                        return Err(e);
                    }
                };

            let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                extract_historical_arrays(&income, &balance, &cf, &metrics, profile.raw())
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data — need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "monte_carlo_dcf") {
                return Ok(err);
            }

            let current_price = profile.price().unwrap_or(0.0);

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;
            let ranges = financial_model::McRange {
                revenue_growth: req.range_revenue_growth,
                gross_margin: req.range_gross_margin,
                da_to_revenue: req.range_da,
                capex_to_revenue: req.range_capex,
                nwc_to_revenue: req.range_nwc,
                discount_rate: req.range_discount_rate,
            };

            ranges
                .validate()
                .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let mut rng = rand::rng();
            let sims = req.simulations.clamp(100, 10_000) as usize;
            let result = financial_model::monte_carlo_dcf(&hist, &assumptions, sims, &ranges, current_price, &mut rng);

            let histogram: Vec<serde_json::Value> = result.histogram.iter().map(|(bucket, count)| {
                serde_json::json!({"bucket": bucket, "count": count})
            }).collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "current_price": current_price,
                "simulations": result.simulations,
                "distribution": {
                    "base_intrinsic": result.base_intrinsic,
                    "mean": result.mean_intrinsic,
                    "std_dev": result.std_dev,
                    "min": result.min_intrinsic,
                    "p10": result.p10,
                    "p25": result.p25,
                    "median": result.median,
                    "p75": result.p75,
                    "p90": result.p90,
                    "max": result.max_intrinsic,
                    "prob_undervalued": result.prob_undervalued,
                    "histogram": histogram,
                },
                "framework": "Monte Carlo DCF. Runs N simulations with each assumption sampled uniformly within +/- configured ranges. Produces intrinsic value distribution (percentiles), probability of undervaluation, and histogram. Quantifies valuation uncertainty from assumption ranges."
            });

            Ok(output)
        }).await
    }

    #[tool(
        description = "Scenario impact valuation. Takes a resolved scenario event tree (from hkask-mcp-scenarios `scenario_quantify`) and per-node impact mappings, then runs DCF under each scenario path. For each scenario node, the user maps how its Yes/No outcome additively changes the company's DCF assumptions (revenue growth, gross margin, capex, etc.). Enumerates all 2^N leaf paths, computes each path's probability from the conditional probability tables, applies stacked deltas, runs DCF, and weights by path probability. Returns probability-weighted intrinsic value, per-node sensitivity (which scenario nodes drive the most valuation variance), the intrinsic value distribution (percentiles, prob-undervalued), the T8a risk core (probability-weighted expected return and sigma_scenario over the paths, plus per-node beta loadings), and — when realized_volatility is supplied — the fused volatility (root-sum-square of realized and scenario-implied sigma). Max 12 scenario nodes. This is the scenario scenario events drive the company's financial forecast, not the other way around."
    )]
    pub async fn scenario_impact_valuation(
        &self,
        Parameters(req): Parameters<types::ScenarioImpactValuationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_impact_valuation", async {
            validate_symbol(&req.symbol)?;

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;

            let (income, balance, cf, metrics, profile) =
                match (income_result, balance_result, cf_result, metrics_result, profile_result) {
                    (Ok(inc), Ok(bal), Ok(cf), Ok(m), Ok(p)) => (inc, bal, cf, m, p),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => {
                        return Err(e);
                    }
                };

            let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                extract_historical_arrays(&income, &balance, &cf, &metrics, profile.raw())
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data — need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "scenario_impact_valuation") {
                return Ok(err);
            }

            let current_price = profile.price().unwrap_or(0.0);

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            // Parse the scenario tree JSON from scenario_quantify.
            // Normalize the scenario server's EventTree format (nested
            // event fields, topo_order alias) into the flat format.
            let normalized_tree_json = financial_model::normalize_scenario_tree_json(&req.scenario_tree)
                .map_err(|e| McpToolError::invalid_argument(format!("invalid scenario_tree JSON: {e}")))?;
            let tree: financial_model::ScenarioTreeInput = serde_json::from_str(&normalized_tree_json)
                .map_err(|e| McpToolError::invalid_argument(format!("invalid scenario_tree JSON: {e}")))?;

            // Parse the per-node impact mappings.
            let impacts: Vec<financial_model::ScenarioNodeImpact> =
                serde_json::from_str(&req.impact_mappings)
                    .map_err(|e| McpToolError::invalid_argument(format!("invalid impact_mappings JSON: {e}")))?;

            let result = financial_model::scenario_impact_dcf(
                &hist, &assumptions, &tree, &impacts, current_price,
            )
            .map_err(map_scenario_impact_error)?;

            // T8a risk core over the enumerated leaf paths. Each path is a
            // branch: its probability (from the CPTs) and its annualized
            // return from the current price to the path's intrinsic value
            // over the DCF horizon. Skipped with a named reason (never
            // silently) when a return is undefined.
            let horizon_years = f64::from(assumptions.total_years);
            let negative_intrinsic =
                result.paths.iter().any(|p| p.intrinsic_per_share < 0.0);
            let risk_skip_reason = if current_price <= 0.0 {
                Some("current price is not positive")
            } else if negative_intrinsic {
                Some("a path intrinsic value is negative")
            } else {
                None
            };
            let branches: Vec<hkask_forecast::BranchOutcome> = result.paths.iter().map(|p| {
                let branch_return = if p.intrinsic_per_share > 0.0 {
                    (p.intrinsic_per_share / current_price)
                        .powf(1.0 / horizon_years)
                        - 1.0
                } else {
                    // Zero intrinsic: total loss of the position.
                    -1.0
                };
                hkask_forecast::BranchOutcome {
                    probability: p.probability,
                    branch_return,
                }
            }).collect();
            let risk_measure = if risk_skip_reason.is_none() {
                hkask_forecast::scenario_risk_measure(&branches)
            } else {
                None
            };

            // T8a factor loadings per scenario node: β(node) =
            // E[r | node Yes] − E[r | node No], from the path masks (bit i
            // set = node i Yes). Complements node_sensitivities (which are
            // unweighted intrinsic spreads); these are probability-weighted
            // return exposures.
            let factor_loadings: Vec<serde_json::Value> = if risk_skip_reason.is_none() {
                tree.nodes.iter().enumerate().map(|(i, node)| {
                    let node_true: Vec<bool> = result.paths.iter()
                        .map(|p| (p.path_mask >> i) & 1 == 1)
                        .collect();
                    let beta = hkask_forecast::scenario_node_loading(&branches, &node_true);
                    serde_json::json!({
                        "node_id": node.id,
                        "beta": beta,
                        "beta_note": if beta.is_none() {
                            Some("one conditioning set has zero probability mass — loading undefined")
                        } else {
                            None
                        },
                    })
                }).collect()
            } else {
                Vec::new()
            };

            // Volatility fusion: realized (caller-supplied) σ fused with the
            // scenario-implied σ via root-sum-square. The scenario channel is
            // weighted by the tree's total probability mass — partial tree
            // coverage down-weights the scenario channel (a tree covering
            // half the mass contributes half-weighted scenario risk).
            let fused_volatility = match (req.realized_volatility, risk_measure) {
                (Some(realized), Some(rm)) => Some(
                    hkask_forecast::fuse_volatility(
                        realized,
                        Some(rm.sigma_scenario),
                        result.total_probability,
                    )
                ),
                _ => None,
            };
            let fused_volatility_note = if req.realized_volatility.is_some() && fused_volatility.is_none() {
                Some("realized_volatility supplied but the scenario risk measure is undefined — no fusion emitted (never fabricated)")
            } else if req.realized_volatility.is_none() {
                Some("no realized_volatility supplied — fusion not computed; supply it to fuse realized and scenario-implied σ")
            } else {
                None
            };

            let node_sensitivities: Vec<serde_json::Value> = result.node_sensitivities.iter()
                .map(|s| serde_json::json!({
                    "node_id": s.node_id,
                    "node_name": s.node_name,
                    "intrinsic_if_yes": s.intrinsic_if_yes,
                    "intrinsic_if_no": s.intrinsic_if_no,
                    "sensitivity": s.sensitivity,
                    "marginal_probability": s.marginal_probability,
                }))
                .collect();

            let max_output_paths = 50;
            let paths_truncated = result.paths.len() > max_output_paths;
            let mut sorted_paths = result.paths.clone();
            sorted_paths.sort_by(|a, b| {
                b.probability
                    .partial_cmp(&a.probability)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let output_paths: Vec<serde_json::Value> = sorted_paths.iter()
                .take(max_output_paths)
                .map(|p| {
                    let outcomes: Vec<serde_json::Value> = p.outcomes.iter()
                        .map(|o| serde_json::json!({
                            "node_id": o.node_id,
                            "outcome": o.outcome,
                        }))
                        .collect();
                    serde_json::json!({
                        "probability": p.probability,
                        "intrinsic_per_share": p.intrinsic_per_share,
                        "applied_growth": p.applied_growth,
                        "applied_margin": p.applied_margin,
                        "outcomes": outcomes,
                    })
                })
                .collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "current_price": current_price,
                "base_intrinsic": result.base_intrinsic,
                "probability_weighted_intrinsic": result.probability_weighted_intrinsic,
                "path_count": result.path_count,
                "total_probability": result.total_probability,
                "paths_truncated": paths_truncated,
                "distribution": {
                    "min": result.distribution.min,
                    "p10": result.distribution.p10,
                    "p25": result.distribution.p25,
                    "median": result.distribution.median,
                    "p75": result.distribution.p75,
                    "p90": result.distribution.p90,
                    "max": result.distribution.max,
                    "prob_undervalued": result.distribution.prob_undervalued,
                },
                "node_sensitivities": node_sensitivities,
                // T8a risk core (hkask_forecast::scenario_risk_measure) over
                // the leaf paths.
                "risk_measure": risk_measure.map(|rm| serde_json::json!({
                    "expected_return": rm.expected_return,
                    "sigma_scenario": rm.sigma_scenario,
                    "branch_count": rm.branch_count,
                    "probability_mass": rm.probability_mass,
                })),
                "risk_measure_note": risk_skip_reason.map(|reason| format!(
                    "{reason} — scenario risk measure undefined (never fabricated)"
                )),
                // T8a factor loadings (hkask_forecast::scenario_node_loading):
                // β(node) = E[r | node Yes] − E[r | node No].
                "factor_loadings": factor_loadings,
                // Volatility fusion (hkask_forecast::fuse_volatility): realized
                // σ (caller-supplied) ⊕ scenario-implied σ, scenario channel
                // weighted by tree coverage.
                "fused_volatility": fused_volatility,
                "fused_volatility_note": fused_volatility_note,
                "paths": output_paths,
                "bridge_note": "Scenario scenario events (from hkask-mcp-scenarios scenario_quantify) drive the company financial forecast. Each scenario node Yes/No outcome maps to additive deltas on DCF assumptions. The tool enumerates all 2^N leaf paths, computes path probabilities from the CPTs, applies stacked deltas, runs DCF under each path, and weights by path probability.",
                "pipeline": [
                    "1. scenario_quantify (hkask-mcp-scenarios) → resolved event tree",
                    "2. User authors per-node impact mappings (node_id → yes_deltas, no_deltas)",
                    "3. scenario_impact_valuation (this tool) → probability-weighted DCF",
                    "4. Compare probability_weighted_intrinsic vs base_intrinsic vs current_price",
                ],
                "framework": "Scenario impact valuation. Exogenous scenario events drive the company's financial forecast via per-node additive deltas on DCF assumptions. Enumerates all 2^N leaf paths through the event tree, computes each path's probability from the conditional probability tables, applies stacked deltas, runs DCF under each modified assumption set, and weights by path probability. Returns probability-weighted intrinsic value, per-node sensitivity, and intrinsic value distribution.",
            });

            Ok(fibo::enrich_with_ontology(output, "scenario_impact_valuation"))
        }).await
    }

    #[tool(
        description = "Calibrated superforecast. Runs Fermi decomposition on growth and margin estimates, applies outside view (base rate) and inside view adjustments, then distributes probabilities across the four Schwartz scenarios. Produces a probability-weighted intrinsic value and compares it to the market price. Anchored to Tetlock's GJP methodology. Collaborative — you provide base rates and reference counts; the tool computes calibrations."
    )]
    pub async fn calibrate_forecast(
        &self,
        Parameters(req): Parameters<types::CalibrateForecastRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "calibrate_forecast", async {
            validate_symbol(&req.symbol)?;
            if let Some(ref revision_of) = req.revision_of {
                let revision_of = revision_of.clone();
                let symbol = req.symbol.clone();
                run_store(self.research.clone(), move |portfolio| {
                    portfolio.validate_forecast_revision(&revision_of, &symbol)
                })
                .await?;
            }
            for (name, value) in [
                ("growth_estimate", req.growth_estimate),
                ("margin_estimate", req.margin_estimate),
            ] {
                if let Some(value) = value {
                    validate_unit_interval(name, value)?;
                }
            }
            for (name, overrides) in [
                ("growth_fermi_overrides", &req.growth_fermi_overrides),
                ("margin_fermi_overrides", &req.margin_fermi_overrides),
            ] {
                for override_value in overrides {
                    validate_unit_interval(&format!("{name}.estimate"), override_value.estimate)?;
                    validate_unit_interval(
                        &format!("{name}.confidence"),
                        override_value.confidence,
                    )?;
                }
            }

            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;

            let (income, balance, metrics, profile, cf) =
                match (income_result, balance_result, metrics_result, profile_result, cf_result) {
                    (Ok(inc), Ok(bal), Ok(m), Ok(p), Ok(c)) => (inc, bal, m, p, c),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => { return Err(e); }
                };

            let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                extract_historical_arrays(&income, &balance, &cf, &metrics, profile.raw())
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data — need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "calibrate_forecast") {
                return Ok(err);
            }

            let current_price = profile.price().unwrap_or(0.0);
            let hist_revenue_growth = hist.revenue_cagr();

            let mut assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            // Run scenarios
            let matrix = scenarios::ScenarioMatrix::growth_x_margin(hist_revenue_growth, assumptions.gross_margin);
            let results = scenarios::run_scenario_analysis(&hist, &assumptions, &matrix);

            // Build Fermi estimates from server-level defaults, apply user overrides
            let mut growth_fermi = self.fermi_defaults.growth_questions.clone();
            let mut margin_fermi = self.fermi_defaults.margin_questions.clone();

            if !req.growth_fermi_overrides.is_empty() {
                let o: Vec<(usize, f64, f64)> = req.growth_fermi_overrides.iter()
                    .map(|ov| (ov.index, ov.estimate, ov.confidence)).collect();
                superforecast::apply_fermi_overrides(&mut growth_fermi, &o);
            }
            if !req.margin_fermi_overrides.is_empty() {
                let o: Vec<(usize, f64, f64)> = req.margin_fermi_overrides.iter()
                    .map(|ov| (ov.index, ov.estimate, ov.confidence)).collect();
                superforecast::apply_fermi_overrides(&mut margin_fermi, &o);
            }

            let growth_inside = match req.growth_estimate {
                Some(e) => e,
                None => hkask_forecast::calibrate_from_fermi(&growth_fermi)
                    .map_err(|e| McpToolError::invalid_argument(e.to_string()))?,
            };
            let margin_inside = match req.margin_estimate {
                Some(e) => e,
                None => hkask_forecast::calibrate_from_fermi(&margin_fermi)
                    .map_err(|e| McpToolError::invalid_argument(e.to_string()))?,
            };

            let ref_class = req.reference_class.unwrap_or_else(|| "S&P 500 large-cap, 2015-2025".into());
            let ref_count = req.reference_count.unwrap_or(500);

            let (growth_calibrated, growth_conf) = hkask_forecast::outside_view_adjustment(
                0.55, growth_inside, ref_count,
            );
            let (margin_calibrated, margin_conf) = hkask_forecast::outside_view_adjustment(
                0.50, margin_inside, ref_count,
            );

            // Distribute probabilities across scenarios
            let weighted = superforecast::distribute_scenario_probabilities(
                growth_calibrated, margin_calibrated, &results,
            );
            let expected_value = superforecast::expected_intrinsic(&weighted);
            let market_gap = if current_price > 0.0 { (expected_value - current_price) / current_price } else { 0.0 };

            // Generate a durable calibrated projection for later decomposition.
            let forecast_id = Uuid::new_v4().to_string();
            assumptions = assumptions
                .with_overrides(types::ProjectionAssumptionOverrides {
                    revenue_growth: Some(growth_calibrated),
                    gross_margin: Some(margin_calibrated),
                    ..Default::default()
                })
                .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;
            let model = financial_model::project_model(&hist, &assumptions, current_price);
            let stored = StoredForecast {
                model,
                assumptions: assumptions.clone(),
                current_price,
                intrinsic_per_share: expected_value,
            };
            self.save_forecast(PersistedForecast {
                id: forecast_id.clone(),
                symbol: req.symbol.clone(),
                revision_of: req.revision_of.clone(),
                snapshot: stored.snapshot(),
                outcomes: Vec::new(),
                created_at: now_rfc3339(),
            })
            .await?;

            let fermi_output: Vec<serde_json::Value> = growth_fermi.iter().zip(margin_fermi.iter()).map(|(g, m)| {
                serde_json::json!({
                    "growth_sub_q": g.question, "growth_estimate": g.estimate, "growth_confidence": g.confidence,
                    "margin_sub_q": m.question, "margin_estimate": m.estimate, "margin_confidence": m.confidence,
                })
            }).collect();

            let scenario_output: Vec<serde_json::Value> = weighted.iter().map(|w| {
                serde_json::json!({"name": w.name, "intrinsic": w.intrinsic_per_share, "probability": w.probability})
            }).collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "forecast_id": forecast_id,
                "revision_of": req.revision_of,
                "current_price": current_price,
                "calibration": {
                    "growth": {"inside_estimate": growth_inside, "calibrated": growth_calibrated, "confidence": growth_conf},
                    "margin": {"inside_estimate": margin_inside, "calibrated": margin_calibrated, "confidence": margin_conf},
                    "reference_class": ref_class,
                    "reference_count": ref_count,
                    "method": "Fermi decomposition + outside/inside view calibration",
                },
                "fermi_decomposition": fermi_output,
                "scenarios": scenario_output,
                "expected_intrinsic": expected_value,
                "market_gap_pct": market_gap,
                "interpretation": if market_gap > 0.10 { "significantly_undervalued" } else if market_gap > 0.0 { "modestly_undervalued" } else if market_gap > -0.10 { "fairly_valued" } else { "overvalued" },
                "framework": "Tetlock GJP Superforecasting pipeline: Fermi decomposition → outside/inside view calibration → Bayesian-ready probability estimates → scenario-weighted intrinsic value. Probabilities are probability-weighted scenario intrinsic values compared to market price. Brier score tracking available when outcomes are recorded via result_feedback.",
            });

            Ok(output)
        }).await
    }

    #[tool(
        description = "Retrieve one durable forecast and its recorded outcomes for the authenticated owner"
    )]
    pub async fn forecast_get(
        &self,
        Parameters(req): Parameters<types::ForecastGetRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_get", async {
            let forecast = self
                .get_persisted_forecast(req.forecast_id)
                .await?
                .ok_or_else(|| {
                    McpToolError::invalid_argument("forecast not found for this owner")
                })?;
            Ok(serde_json::json!(forecast))
        })
        .await
    }

    #[tool(
        description = "List durable forecasts for a symbol belonging to the authenticated owner"
    )]
    pub async fn forecast_list(
        &self,
        Parameters(req): Parameters<types::ForecastListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_list", async {
            validate_symbol(&req.symbol)?;
            let forecasts = self.list_persisted_forecasts(req.symbol.clone()).await?;
            Ok(serde_json::json!({"symbol": req.symbol, "forecasts": forecasts}))
        })
        .await
    }

    #[tool(
        description = "Persist a pre-computed price target for later Brier scoring. Unlike calibrate_forecast (which runs its own Fermi decomposition) and forecast_record (which requires the actual outcome), this tool stores a pending price target without an outcome and without a decomposition model. The stored forecast can later be resolved by forecast_record when the horizon passes — Brier scoring runs on the recorded multiple and price change; gap decomposition is unavailable (no projected model). Use this when a skill valuation step (e.g., company-research-flash step 16) produces a price target that should be tracked for calibration."
    )]
    pub async fn forecast_persist(
        &self,
        Parameters(req): Parameters<types::ForecastPersistRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_persist", async {
            validate_symbol(&req.symbol)?;
            if let Some(value) = req.forecast_multiple {
                validate_finite("forecast_multiple", value)?;
            }
            if let Some(value) = req.forecast_price {
                validate_finite("forecast_price", value)?;
            }
            if let Some(value) = req.current_price {
                validate_finite("current_price", value)?;
            }
            // Resolve the forecast price change: prefer the direct field, else
            // compute from forecast_price and current_price. Reject if neither
            // path is available — persisting a PT with no price change is a
            // broken calibration signal (per .rules: no silent fallbacks on
            // regulation signals).
            let forecast_price_change = match req.forecast_price_change {
                Some(change) => {
                    validate_finite("forecast_price_change", change)?;
                    change
                }
                None => {
                    let fp = req.forecast_price.ok_or_else(|| {
                        McpToolError::invalid_argument(
                            "forecast_price_change or forecast_price+current_price is required",
                        )
                    })?;
                    let cp = req.current_price.ok_or_else(|| {
                        McpToolError::invalid_argument(
                            "forecast_price_change or forecast_price+current_price is required",
                        )
                    })?;
                    if cp <= 0.0 {
                        return Err(McpToolError::invalid_argument(format!(
                            "current_price must be positive to compute forecast_price_change, got {cp}"
                        )));
                    }
                    (fp - cp) / cp
                }
            };
            if let Some(ref revision_of) = req.revision_of {
                let revision_of = revision_of.clone();
                let symbol = req.symbol.clone();
                run_store(self.research.clone(), move |portfolio| {
                    portfolio.validate_forecast_revision(&revision_of, &symbol)
                })
                .await?;
            }

            let forecast_id = req
                .forecast_id
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string());

            // Store a minimal snapshot — no projected model, so gap
            // decomposition is unavailable when forecast_record later looks
            // up this ID. Brier scoring on the recorded multiple and price
            // change still runs. The snapshot carries the forecast inputs so
            // forecast_list consumers can see what was persisted.
            let snapshot = serde_json::json!({
                "kind": "precomputed_price_target",
                "symbol": req.symbol,
                "forecast_date": req.forecast_date,
                "horizon": req.horizon,
                "forecast_multiple": req.forecast_multiple,
                "forecast_price": req.forecast_price,
                "current_price": req.current_price,
                "forecast_price_change": forecast_price_change,
            });

            self.save_forecast(PersistedForecast {
                id: forecast_id.clone(),
                symbol: req.symbol.clone(),
                revision_of: req.revision_of.clone(),
                snapshot,
                outcomes: Vec::new(),
                created_at: now_rfc3339(),
            })
            .await?;

            Ok(serde_json::json!({
                "status": "persisted",
                "symbol": req.symbol,
                "forecast_id": forecast_id,
                "revision_of": req.revision_of,
                "forecast_date": req.forecast_date,
                "horizon": req.horizon,
                "forecast_multiple": req.forecast_multiple,
                "forecast_price": req.forecast_price,
                "current_price": req.current_price,
                "forecast_price_change": forecast_price_change,
                "note": "Pre-computed price target persisted without a decomposition model. Call forecast_record with this forecast_id when the horizon passes to close the Brier loop. Gap decomposition will be unavailable (no projected model).",
            }))
        })
        .await
    }

    #[tool(
        description = "Record a forecast outcome to close the superforecasting loop. Forecast a valuation multiple and price change over a horizon (3mo/6mo/1yr/2yr/3yr), then record what actually happened. Computes Brier scores on multiple direction and price return vs a tolerance band. When forecast_id is provided (from dcf_valuation, calibrate_forecast, or forecast_persist), looks up the stored 11-line-item projection model and decomposes the return gap into revenue growth, gross margin, D&A, capex, NWC, multiple expansion, and net debt contributions. Pre-computed PTs from forecast_persist carry no decomposition model — Brier scoring still runs, decomposition is skipped."
    )]
    pub async fn forecast_record(
        &self,
        Parameters(req): Parameters<types::ForecastRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_record", async {
            validate_symbol(&req.symbol)?;
            for (name, value) in [
                ("forecast_multiple", req.forecast_multiple),
                ("forecast_price_change", req.forecast_price_change),
                ("actual_multiple", req.actual_multiple),
                ("actual_price_change", req.actual_price_change),
            ] {
                validate_finite(name, value)?;
            }

            // Brier scores on binary outcomes
            // Multiple direction: the multiple probability is not modeled, so a
            // coin-flip prior (p=0.5) would always yield 0.25 — uninformative.
            // Excluded from the combined score; reported as null below.
            // Price change: was actual return within 20% tolerance of forecast?
            let return_accurate = superforecast::within_tolerance(
                req.forecast_price_change, req.actual_price_change, 0.20,
            );
            // FIX (H7): Use the forecast's own probability from the stored
            // snapshot, not a hardcoded 0.7. The forecast probability is the
            // calibrated confidence from calibrate_forecast or driver_forecast.
            // When no stored forecast is available, fall back to 0.7 (the
            // historical default) and warn so the operator knows the Brier
            // score is not measuring the forecast's own calibration.
            //
            // Fetch the persisted forecast once — reused below for gap
            // decomposition to avoid a redundant DB call.
            let persisted_forecast: Option<crate::research_store::PersistedForecast> = if let Some(ref forecast_id) = req.forecast_id {
                Some(
                    self.get_persisted_forecast(forecast_id.clone())
                        .await?
                        .ok_or_else(|| McpToolError::invalid_argument("forecast not found for this owner"))?
                )
            } else {
                None
            };
            // Validate symbol match before using the forecast.
            if let Some(ref pf) = persisted_forecast {
                if pf.symbol != req.symbol {
                    return Err(McpToolError::invalid_argument(format!(
                        "forecast '{}' belongs to symbol '{}', not '{}'",
                        req.forecast_id.as_ref().unwrap(), pf.symbol, req.symbol
                    )));
                }
            }
            let forecast_probability = persisted_forecast
                .as_ref()
                .and_then(|pf| pf.snapshot.get("forecast_probability"))
                .and_then(|v| v.as_f64())
                .or_else(|| {
                    // Also check the calibration block from calibrate_forecast,
                    // which stores confidence as a proxy for probability.
                    persisted_forecast
                        .as_ref()
                        .and_then(|pf| pf.snapshot.get("calibration"))
                        .and_then(|c| c.get("growth"))
                        .and_then(|g| g.get("confidence"))
                        .and_then(|v| v.as_f64())
                })
                .unwrap_or_else(|| {
                    tracing::warn!(
                        target: "hkask.mcp.companies",
                        "forecast_record: no forecast_probability in snapshot — \
                         using 0.7 fallback. Brier score does not measure the \
                         forecast's own calibration."
                    );
                    0.7
                });
            let return_brier = hkask_forecast::brier_score(forecast_probability, return_accurate);
            let combined = return_brier;

            // Gap decomposition: reuse the already-fetched persisted forecast.
            let stored_forecast = if let Some(ref pf) = persisted_forecast {
                // Pre-computed PTs from forecast_persist carry a minimal
                // snapshot (kind: "precomputed_price_target") with no
                // projected model — StoredForecast::from_snapshot fails on
                // those. Fall back to None (no decomposition) so Brier
                // scoring still runs. Per .rules: don't collapse to None via
                // .ok()? on a fallible operation silently — log the skip so
                // the operator can distinguish "no model" from "broken."
                match StoredForecast::from_snapshot(&pf.snapshot) {
                    Ok(stored) => Some(stored),
                    Err(e) => {
                        tracing::warn!(
                            "forecast_record: forecast '{}' snapshot is not a full StoredForecast — \
                             gap decomposition unavailable, Brier scoring still runs. Error: {}",
                            req.forecast_id.as_deref().unwrap_or(""), e
                        );
                        None
                    }
                }
            } else {
                None
            };
            let mut decomposition: Option<serde_json::Value> = None;
            if let Some(stored) = stored_forecast {
                    // Fetch actual financials at the outcome date for decomposition
                    let actual_income = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
                    let actual_balance = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
                    let actual_cf = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;
                    let actual_metrics = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
                    let actual_profile = self.fetch_profile(&req.symbol).await;

                    if let (Ok(inc), Ok(bal), Ok(cf), Ok(metrics), Ok(prof)) =
                        (&actual_income, &actual_balance, &actual_cf, &actual_metrics, &actual_profile)
                    {
                        if let Some((inc_data, bal_data, cf_data, met_data, prof_data)) =
                            extract_historical_arrays(inc, bal, cf, metrics, prof.raw())
                        {
                            let actual_hist = financial_model::HistoricalSnapshot::from_api_json(
                                inc_data,
                                bal_data,
                                cf_data,
                                met_data,
                                prof_data,
                            );

                            // Run decomposition
                            let gap = financial_model::decompose_gap(
                                &stored.model,
                                &stored.assumptions,
                                &actual_hist,
                                current_price_from_multiple(req.actual_multiple, &actual_hist),
                                req.actual_multiple,
                                stored.intrinsic_per_share,
                                stored.current_price,
                            );

                            decomposition = Some(serde_json::json!({
                                "total_return_gap": gap.total_return_gap,
                                "components": {
                                    "revenue_growth": {
                                        "contribution": gap.revenue_growth_contribution,
                                        "projected_growth": stored.assumptions.revenue_growth,
                                        "actual_growth": actual_hist.revenue_cagr(),
                                    },
                                    "gross_margin": {
                                        "contribution": gap.gross_margin_contribution,
                                        "projected": stored.assumptions.gross_margin,
                                        "actual": actual_hist.gross_margin(),
                                    },
                                    "da": {
                                        "contribution": gap.da_contribution,
                                        "projected": stored.assumptions.da_to_revenue,
                                        "actual": actual_hist.da_to_revenue(),
                                    },
                                    "capex": {
                                        "contribution": gap.capex_contribution,
                                        "projected": stored.assumptions.capex_to_revenue,
                                        "actual": actual_hist.capex_to_revenue(),
                                    },
                                    "nwc": {
                                        "contribution": gap.nwc_contribution,
                                        "projected": stored.assumptions.nwc_to_revenue,
                                        "actual": actual_hist.nwc_to_revenue(),
                                    },
                                    "multiple": {
                                        "contribution": gap.multiple_contribution,
                                        "projected": projected_terminal_multiple(&stored.model),
                                        "actual": req.actual_multiple,
                                    },
                                    "net_debt": {
                                        "contribution": gap.net_debt_contribution,
                                        "projected": stored.model.net_debt,
                                        "actual": actual_hist.net_debt(),
                                    },
                                },
                                "residual": gap.residual,
                            }));
                        }
                    }
                }

            // Legacy gap narrative (used when no forecast_id or decomposition fails)
            let multiple_gap = req.actual_multiple - req.forecast_multiple;
            let return_gap = req.actual_price_change - req.forecast_price_change;
            let gap_narrative = if decomposition.is_some() {
                "full_decomposition"
            } else if multiple_gap.abs() > 2.0 && return_gap.abs() > 0.05 {
                "multiple_and_return_diverged"
            } else if multiple_gap.abs() > 2.0 {
                "multiple_drove_gap"
            } else if return_gap.abs() > 0.05 {
                "return_drove_gap"
            } else {
                "forecast_accurate"
            };

            if let Some(ref forecast_id) = req.forecast_id {
                self.record_persisted_forecast_outcome(
                    forecast_id.clone(),
                    serde_json::json!({
                        "forecast_date": req.forecast_date,
                        "horizon": req.horizon,
                        "forecast_multiple": req.forecast_multiple,
                        "forecast_price_change": req.forecast_price_change,
                        "outcome_date": req.outcome_date,
                        "actual_multiple": req.actual_multiple,
                        "actual_price_change": req.actual_price_change,
                        "multiple_brier": serde_json::Value::Null,
                        "return_brier": return_brier,
                        "combined_brier": combined,
                        "decomposition": decomposition,
                        "recorded_at": now_rfc3339(),
                    }),
                )
                .await?;
            }

            let mut output = serde_json::json!({
                "status": "recorded",
                "symbol": req.symbol,
                "horizon": req.horizon,
                "forecast": {
                    "multiple": req.forecast_multiple,
                    "price_change_pct": req.forecast_price_change,
                },
                "actual": {
                    "multiple": req.actual_multiple,
                    "price_change_pct": req.actual_price_change,
                },
                "gaps": {
                    "multiple_gap": multiple_gap,
                    "return_gap": return_gap,
                    "narrative": gap_narrative,
                },
                "brier": {
                    "multiple_direction": serde_json::Value::Null,
                    "return_accuracy": return_brier,
                    "combined": combined,
                    "interpretation": hkask_forecast::brier_interpretation(combined),
                },
                "framework": "Forecast-Record-Score (Tetlock GJP). Brier scores on binary outcomes: multiple direction and return accuracy within 20% tolerance. When forecast_id is provided, runs full 11-line-item decomposition (revenue growth, gross margin, D&A, capex, NWC, multiple, net debt).",
            });

            if let Some(dec) = decomposition {
                output["decomposition"] = dec;
            }
            if let Some(ref fid) = req.forecast_id {
                output["forecast_id"] = serde_json::Value::String(fid.clone());
            }

            Ok(output)
        }).await
    }

    #[tool(
        description = "Rate a previous tool result on a 1–5 scale with optional comments. Score: 5 = exceeded expectations, 3 = met expectations, 1 = completely missed. Both score and comments are optional — provide either, both, or neither to acknowledge you saw the result. Optional `provider` field explicitly names the data provider (e.g. \"fmp\", \"eodhd\") that produced the result; when omitted, the provider is inferred from the symbol/query. Feeds the learning loop."
    )]
    pub async fn result_feedback(
        &self,
        Parameters(types::ResultFeedbackRequest {
            tool,
            query,
            score,
            comments,
            provider,
        }): Parameters<types::ResultFeedbackRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "result_feedback", async {
            // Validate score range if provided
            if let Some(s) = score
                && !(1..=5).contains(&s)
            {
                return Err(McpToolError::invalid_argument(format!(
                    "score must be 1–5, got {s}"
                )));
            }

            // Accept empty feedback as an acknowledgment (no score, no comments = "I saw it")
            let has_feedback = score.is_some() || !comments.is_empty();

            // Record feedback as an experience linked to the original tool.

            // Kanban-style learning: feedback updates in-process state.
            // Extracts symbol from query to track per-symbol provider quality.
            if let Some(sym) = parse_symbol_from_query(&query)
                && let Ok(mut state) = self.learning.lock()
            {
                let prov = if let Some(ref p) = provider {
                    match p.to_ascii_lowercase().as_str() {
                        "eodhd" => Provider::Eodhd,
                        "fmp" => Provider::Fmp,
                        _ => {
                            // Unknown explicit provider: fall back to heuristic.
                            if comments.contains("provider=eodhd") {
                                Provider::Eodhd
                            } else if comments.contains("provider=fmp") {
                                Provider::Fmp
                            } else if sym.contains('.') {
                                Provider::Eodhd
                            } else {
                                Provider::Fmp
                            }
                        }
                    }
                } else if comments.contains("provider=eodhd") {
                    Provider::Eodhd
                } else if comments.contains("provider=fmp") {
                    Provider::Fmp
                } else if sym.contains('.') {
                    Provider::Eodhd
                } else {
                    Provider::Fmp
                };
                state.record(&sym, prov, score);
            }

            let summary = if has_feedback {
                if let Some(s) = score {
                    format!("score {s}/5")
                } else {
                    "comments only".to_string()
                }
            } else {
                "acknowledged".to_string()
            };

            Ok(serde_json::json!({
                "status": "recorded",
                "tool": tool,
                "query": query,
                "summary": summary,
            }))
        })
        .await
    }

    #[tool(
        description = "Driver-based linked three-statement financial forecast. Projects income statement, balance sheet, and cash flow from five key drivers (revenue growth, profit margins, capex vs depreciation, net working capital, debt/equity issuance). Each driver supports percent change, percent of revenue, and explicit adjustment. Balance sheet identity (A = L + E) enforced every period. Financial-sector companies use equity-based residual income (ROE/COE) per Damodaran Applied Corporate Finance Ch. 19. Output is forecast_persist-compatible JSON."
    )]
    pub async fn driver_forecast(
        &self,
        Parameters(req): Parameters<types::DriverForecastRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "driver_forecast", async {
            validate_symbol(&req.symbol)?;

            // Fetch historical financial data
            let income_result = self.fetch("income_statement", &req.symbol, &[("limit", "5")]).await;
            let balance_result = self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]).await;
            let metrics_result = self.fetch("key_metrics", &req.symbol, &[("limit", "5")]).await;
            let profile_result = self.fetch_profile(&req.symbol).await;
            let cf_result = self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]).await;

            let (income, balance, metrics, profile, cf) =
                match (income_result, balance_result, metrics_result, profile_result, cf_result) {
                    (Ok(inc), Ok(bal), Ok(m), Ok(p), Ok(c)) => (inc, bal, m, p, c),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => { return Err(e); }
                };

            let Some((income_data, balance_data, cf_data, metrics_data, profile_data)) =
                extract_historical_arrays(&income, &balance, &cf, &metrics, profile.raw())
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 && !financial_model::is_financial_sector(&profile) {
                return Ok(serde_json::json!({
                    "symbol": req.symbol,
                    "error": "insufficient historical data — need at least 2 years of revenue"
                }));
            }

            // Build driver assumptions from history with user overrides
            let mut assumptions = financial_model::DriverAssumptions::from_history(&hist);
            assumptions.is_financial_sector = financial_model::is_financial_sector(&profile);

            // Apply user overrides
            if let Some(g) = req.revenue_growth {
                assumptions.revenue_growth.percent = Some(g);
            }
            if let Some(adj) = req.revenue_explicit {
                assumptions.revenue_explicit.explicit = Some(adj);
            }
            if let Some(gm) = req.gross_margin {
                assumptions.gross_margin.percent = Some(gm);
            }
            if let Some(sga) = req.sga_pct {
                assumptions.sga_pct.percent = Some(sga);
            }
            if let Some(da) = req.da_pct {
                assumptions.da_pct.percent = Some(da);
            }
            if let Some(tr) = req.tax_rate {
                assumptions.tax_rate = tr;
            }
            if let Some(cp) = req.capex_pct {
                assumptions.capex_pct.percent = Some(cp);
            }
            if let Some(adj) = req.capex_explicit {
                assumptions.capex_explicit.explicit = Some(adj);
            }
            if let Some(ratio) = req.capex_da_ratio {
                assumptions.capex_da_ratio = Some(ratio);
            }
            if let Some(ref method) = req.nwc_method {
                assumptions.nwc_method = match method.to_lowercase().as_str() {
                    "days" => financial_model::NwcMethod::Days,
                    "percent_of_revenue" | "pct" => financial_model::NwcMethod::PercentOfRevenue,
                    "explicit" => financial_model::NwcMethod::Explicit,
                    _ => financial_model::NwcMethod::PercentOfRevenue,
                };
            }
            if let Some(dso) = req.dso_days {
                assumptions.dso_days = dso;
            }
            if let Some(dio) = req.dio_days {
                assumptions.dio_days = dio;
            }
            if let Some(dpo) = req.dpo_days {
                assumptions.dpo_days = dpo;
            }
            if let Some(nwc_pct) = req.nwc_pct {
                assumptions.nwc_pct = nwc_pct;
            }
            if let Some(adj) = req.nwc_explicit {
                assumptions.nwc_explicit.explicit = Some(adj);
            }
            if let Some(di) = req.debt_issuance {
                assumptions.debt_issuance = di;
            }
            if let Some(dr) = req.debt_repayment {
                assumptions.debt_repayment = dr;
            }
            if let Some(de) = req.target_debt_equity {
                assumptions.target_debt_equity = Some(de);
            }
            if let Some(ir) = req.interest_rate {
                assumptions.interest_rate = ir;
            }
            if let Some(ei) = req.equity_issuance {
                assumptions.equity_issuance = ei;
            }
            if let Some(dpr) = req.dividend_payout_ratio {
                assumptions.dividend_payout_ratio = dpr;
            }
            if let Some(dr) = req.discount_rate {
                assumptions.discount_rate = dr;
            }
            if let Some(tg) = req.terminal_growth {
                assumptions.terminal_growth = tg;
            }
            if let Some(yrs) = req.total_years {
                assumptions.total_years = yrs;
            }
            if let Some(coe) = req.cost_of_equity {
                assumptions.cost_of_equity = coe;
            }

            // Validate and project
            assumptions.validate()
                .map_err(|e| McpToolError::invalid_argument(e.to_string()))?;

            let current_price = profile.price().unwrap_or(0.0);
            let model = financial_model::project_driver_model(&hist, &assumptions)
                .map_err(|e| McpToolError::invalid_argument(e.to_string()))?;

            // Build forecast_persist-compatible snapshot
            let forecast_id = Uuid::new_v4().to_string();
            let snapshot = serde_json::json!({
                "kind": "driver_forecast",
                "symbol": req.symbol,
                "model": model,
                "assumptions": assumptions,
                "current_price": current_price,
                "intrinsic_per_share": model.intrinsic_per_share,
            });

            self.save_forecast(crate::research_store::PersistedForecast {
                id: forecast_id.clone(),
                symbol: req.symbol.clone(),
                revision_of: None,
                snapshot,
                outcomes: Vec::new(),
                created_at: now_rfc3339(),
            })
            .await?;

            let margin_of_safety = if current_price > 0.0 {
                (model.intrinsic_per_share - current_price) / current_price
            } else {
                0.0
            };

            let mut output = serde_json::json!({
                "symbol": req.symbol,
                "forecast_id": forecast_id,
                "current_price": current_price,
                "intrinsic_per_share": model.intrinsic_per_share,
                "margin_of_safety": margin_of_safety,
                "enterprise_value": model.enterprise_value,
                "equity_value": model.equity_value,
                "terminal_value": model.terminal_value,
                "terminal_pv_share": if model.enterprise_value > 0.0 {
                    model.terminal_pv / model.enterprise_value
                } else {
                    0.0
                },
                "is_financial_sector": model.is_financial_sector,
                "valuation_method": if model.is_financial_sector {
                    "equity_residual_income (ROE/COE)"
                } else {
                    "firm_level_dcf (WACC)"
                },
                "periods": model.periods,
                "assumptions": assumptions,
                "framework": "Driver-based linked three-statement projection. Five drivers: revenue growth, profit margins (incl. SG&A), capex vs depreciation, net working capital, debt/equity issuance. Balance sheet identity enforced via cash plug. Financial-sector path: residual income = (ROE - COE) × equity (Damodaran Ch. 19). Sources: Damodaran Investment Valuation, Fabozzi Financial Management & Analysis.",
            });

            if req.markdown_report {
                output["markdown_report"] = serde_json::json!(
                    financial_model::generate_markdown_report(
                        &model, &assumptions, &req.symbol, current_price
                    )
                );
            }

            Ok(output)
        }).await
    }
}
