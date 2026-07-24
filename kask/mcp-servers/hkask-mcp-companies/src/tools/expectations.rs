//! Expectations gap analysis — market-implied vs management-guidance vs analyst consensus.
//!
//! Integrates three data sources (FinGPT §3.4 expectational analysis):
//! 1. Market-implied growth from reverse DCF (what the stock price bakes in)
//! 2. Management guidance from classified research claims (what the company says)
//! 3. User's own growth estimate (what the analyst believes)
//!
//! Produces a structured gap report showing where consensus diverges from
//! market pricing — the core of Mauboussin's Expectations Investing framework.
use crate::{CompaniesServer, fibo, financial_model, research, types, validate_symbol};
use hkask_mcp_server::server::execute_tool;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

#[tool_router(router = expectations_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Expectations gap analysis (Mauboussin's Expectations Investing). Compares three growth estimates: (1) market-implied growth from reverse DCF — what the stock price bakes in, (2) management guidance extracted from recent research claims, (3) your own estimate. Produces a structured gap report showing whether the market is pricing in more or less growth than management guidance and your thesis. Use with research_search to populate claims, then provide your own growth estimate to see the gaps."
    )]
    pub async fn expectations_gap(
        &self,
        Parameters(req): Parameters<types::ExpectationsGapRequest>,
    ) -> String {
        execute_tool(self, "expectations_gap", async {
            validate_symbol(&req.symbol)?;

            // ── 1. Fetch financial data for reverse DCF ──────────────────

            let req_income = self
                .fetch("income_statement", &req.symbol, &[("limit", "5")])
                .await;
            let req_balance = self
                .fetch("balance_sheet", &req.symbol, &[("limit", "5")])
                .await;
            let req_cf = self
                .fetch("cash_flow_statement", &req.symbol, &[("limit", "5")])
                .await;
            let req_metrics = self
                .fetch("key_metrics", &req.symbol, &[("limit", "5")])
                .await;
            let req_profile = self.fetch("company_profile", &req.symbol, &[]).await;

            // ── 2. Compute market-implied growth via reverse DCF ──────────

            let market_implied_growth = match (
                &req_income,
                &req_balance,
                &req_cf,
                &req_metrics,
                &req_profile,
            ) {
                (Ok(inc), Ok(bal), Ok(cf), Ok(met), Ok(prof)) => {
                    compute_implied_growth(inc, bal, cf, met, prof).unwrap_or(f64::NAN)
                }
                _ => f64::NAN,
            };

            // ── 3. Fetch research claims for management guidance ──────────

            let company_name = match &req_profile {
                Ok(prof) => prof
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|p| p.get("companyName").and_then(|v| v.as_str()))
                    .unwrap_or(&req.symbol)
                    .to_string(),
                _ => req.symbol.clone(),
            };

            let research = research::search_fundamental(
                &self.client,
                &req.symbol,
                &company_name,
                "revenue guidance forecast growth outlook",
                self.exa_api_key.as_deref(),
                self.tavily_api_key.as_deref(),
                self.brave_api_key.as_deref(),
            )
            .await;

            let claims = research::ResearchClaimClassifier::classify_all(&research);

            // Extract growth numbers from revenue/earnings guidance claims
            let management_growth = extract_management_growth(&claims.claims);
            let management_narrative: Vec<String> = claims
                .claims
                .iter()
                .filter(|c| {
                    matches!(
                        c.category,
                        research::ClaimCategory::RevenueGuidance
                            | research::ClaimCategory::EarningsGuidance
                    )
                })
                .map(|c| c.text.clone())
                .collect();

            // ── 4. User estimate ─────────────────────────────────────────

            let user_growth = req.growth_estimate.unwrap_or(0.05);

            // ── 5. Build gap analysis ────────────────────────────────────

            let analysis = build_gap_analysis(
                &req.symbol,
                market_implied_growth,
                &management_growth,
                user_growth,
                &management_narrative,
                claims.claims.len(),
            );

            let output = serde_json::json!(analysis);

            self.record_experience(
                "expectations_gap",
                &format!("symbol={}", req.symbol),
                "success",
                output.clone(),
            );
            Ok(output)
        })
        .await
    }
}

// ── Reverse DCF: compute market-implied growth ────────────────────────────────

fn compute_implied_growth(
    income: &serde_json::Value,
    balance: &serde_json::Value,
    cf: &serde_json::Value,
    metrics: &serde_json::Value,
    profile: &serde_json::Value,
) -> Option<f64> {
    let income_arr = income.as_array()?;
    let balance_arr = balance.as_array()?;
    let cf_arr = cf.as_array()?;
    let metrics_arr = metrics.as_array();
    let profile_obj = profile.as_array()?.first()?;

    if income_arr.is_empty() || balance_arr.is_empty() || cf_arr.is_empty() {
        return None;
    }

    let hist = financial_model::HistoricalSnapshot::from_api_json(
        income_arr,
        balance_arr,
        cf_arr,
        metrics_arr.map_or(&[], |v| v),
        profile_obj,
    );

    if hist.revenue.len() < 2 {
        return None;
    }

    let current_price = profile_obj.get("price").and_then(|v| v.as_f64())?;
    if current_price <= 0.0 {
        return None;
    }

    let assumptions = financial_model::ProjectionAssumptions::from_history(&hist);

    // Verify price bounds
    let lo_check = financial_model::project_model(
        &hist,
        &financial_model::ProjectionAssumptions {
            revenue_growth: -0.50,
            ..assumptions.clone()
        },
        current_price,
    );
    let hi_check = financial_model::project_model(
        &hist,
        &financial_model::ProjectionAssumptions {
            revenue_growth: 1.00,
            ..assumptions.clone()
        },
        current_price,
    );

    if lo_check.intrinsic_per_share > current_price || hi_check.intrinsic_per_share < current_price
    {
        return None;
    }

    // Binary search for implied growth
    let mut lo = -0.50_f64;
    let mut hi = 1.00_f64;
    let mut implied = 0.0_f64;

    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        let mut a = assumptions.clone();
        a.revenue_growth = mid;
        let model = financial_model::project_model(&hist, &a, current_price);
        if (model.intrinsic_per_share - current_price).abs() < 0.0001 {
            implied = mid;
            break;
        }
        if model.intrinsic_per_share > current_price {
            hi = mid; // intrinsic too high → growth too high → shrink from top
        } else {
            lo = mid; // intrinsic too low → growth too low → shrink from bottom
        }
        implied = mid;
    }

    // Check if implied is near bounds (low-confidence signal)
    if implied <= -0.49 || implied >= 0.99 {
        return None;
    }

    Some(implied)
}

// ── Management growth extraction from classified claims ───────────────────────

fn extract_management_growth(claims: &[research::ExtractedClaim]) -> Vec<f64> {
    claims
        .iter()
        .filter(|c| {
            matches!(
                c.category,
                research::ClaimCategory::RevenueGuidance
                    | research::ClaimCategory::EarningsGuidance
            )
        })
        .flat_map(|c| &c.numeric_values)
        .filter(|n| n.unit == "%" || n.unit == "percent" || n.unit == "pct")
        .map(|n| n.value)
        .collect()
}

// ── Gap analysis builder ──────────────────────────────────────────────────────

fn build_gap_analysis(
    symbol: &str,
    market_implied: f64,
    management_growth: &[f64],
    user_growth: f64,
    narrative: &[String],
    total_claims: usize,
) -> serde_json::Value {
    let market_pct = if market_implied.is_finite() {
        format!("{:.1}%", market_implied * 100.0)
    } else {
        "unavailable".to_string()
    };

    let mgmt_median = if management_growth.is_empty() {
        f64::NAN
    } else {
        let mut sorted = management_growth.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = sorted.len() / 2;
        if sorted.len().is_multiple_of(2) {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[mid]
        }
    };

    let mgmt_pct = if mgmt_median.is_finite() {
        format!("{:.1}%", mgmt_median * 100.0)
    } else {
        "no guidance found".to_string()
    };

    let user_pct = format!("{:.1}%", user_growth * 100.0);

    // Gaps
    let market_vs_mgmt = if mgmt_median.is_finite() && market_implied.is_finite() {
        Some((market_implied - mgmt_median) * 100.0)
    } else {
        None
    };
    let market_vs_user = if market_implied.is_finite() {
        Some((market_implied - user_growth) * 100.0)
    } else {
        None
    };
    let mgmt_vs_user = if mgmt_median.is_finite() {
        Some((mgmt_median - user_growth) * 100.0)
    } else {
        None
    };

    // Signal interpretation
    let signal = if let Some(gap) = market_vs_mgmt {
        if gap > 3.0 {
            "market_expects_more"
        } else if gap < -3.0 {
            "market_expects_less"
        } else {
            "aligned"
        }
    } else {
        "insufficient_data"
    };

    let interpretation = match signal {
        "market_expects_more" => {
            "The market is pricing in higher growth than management guidance suggests. Either the market sees catalysts management hasn't disclosed, or expectations are too optimistic. If your thesis aligns with management, the stock may be overvalued."
        }
        "market_expects_less" => {
            "The market is pricing in lower growth than management guidance. Either the market is skeptical of management's outlook, or the stock is undervalued relative to achievable growth. If your thesis aligns with management, this may represent an opportunity."
        }
        "aligned" => {
            "Market-implied growth is broadly consistent with management guidance. The stock is fairly priced relative to the information consensus. Your edge must come from a differentiated view on moat durability, margin trajectory, or competitive dynamics beyond simple growth rates."
        }
        _ => {
            "Insufficient data to assess the gap between market expectations and management guidance. Try running research_search with a revenue-guidance query to populate claims."
        }
    };

    serde_json::json!({
        "symbol": symbol,
        "framework": "Mauboussin's Expectations Investing (Rappaport & Mauboussin, 2001). Stock prices reflect market expectations about future growth. The expectations gap is the difference between what the market expects, what management guides, and what you believe. The gap IS the investment thesis.",
        "growth_estimates": {
            "market_implied": {
                "value": market_implied,
                "display": market_pct,
                "source": "reverse DCF: growth rate that equates DCF intrinsic value to current stock price",
            },
            "management_guidance": {
                "median": mgmt_median,
                "display": mgmt_pct,
                "samples": management_growth.len(),
                "values": management_growth.iter().map(|v| format!("{:.1}%", v * 100.0)).collect::<Vec<_>>(),
                "source": "extracted from classified research claims (RevenueGuidance + EarningsGuidance categories)",
            },
            "user_estimate": {
                "value": user_growth,
                "display": user_pct,
                "source": "analyst-provided estimate",
            },
        },
        "gaps": {
            "market_vs_management_pct": market_vs_mgmt,
            "market_vs_user_pct": market_vs_user,
            "management_vs_user_pct": mgmt_vs_user,
        },
        "signal": signal,
        "interpretation": interpretation,
        "management_narrative": narrative,
        "data_quality": {
            "total_research_claims": total_claims,
            "guidance_claims_found": management_growth.len(),
            "market_implied_available": market_implied.is_finite(),
        },
        "fibo": {
            "market_implied_growth": fibo::REVENUE_GROWTH_RATE,
            "discount_rate": fibo::DISCOUNT_RATE,
            "intrinsic_value_per_share": fibo::INTRINSIC_VALUE_PER_SHARE,
        },
    })
}
