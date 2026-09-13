//! Expectations gap — price-implied expectations vs demonstrated financial
//! capability (operator ruling 2026-09-10).
//!
//! The gap is between what the current price implies (reverse-DCF-implied
//! growth and profitability) and what the company has demonstrated it can
//! do — its DuPont capability envelope: ROE = net profit margin × asset
//! turnover × equity multiplier, plus the Higgins sustainable growth rate
//! SGR = ROE × retention (the growth a company can self-fund without
//! external financing).
//!
//! Industry-aware profitability headline (operator ruling 2026-09-10): ROE
//! for financial-sector companies — solved from the justified P/B identity
//! P/B = (ROE − g)/(COE − g) — and net margin for everyone else, via the
//! FCF reverse DCF with an implied-margin solve at the sustainable growth
//! rate.
//!
//! Management guidance is CONTEXT ONLY, never the gap axis. The original
//! guidance-gap definition arrived with the hkask migration (`af7613e11a`)
//! without operator ratification and is superseded.
//!
//! Mauboussin & Rappaport (2001) Expectations Investing; DuPont analysis;
//! Higgins (1977) sustainable growth.
use crate::{
    CompaniesServer, fibo, financial_model, research, resolve_current_price, types, validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

/// Cost of equity for the financial-sector implied-ROE solve. Matches the
/// reverse-DCF default discount rate; a bank's required return is equity's,
/// not the WACC blend an FCF model uses.
const DEFAULT_COST_OF_EQUITY: f64 = 0.10;

#[tool_router(router = expectations_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Expectations gap analysis (Mauboussin's Expectations Investing). The gap is between what the price implies and what the company has DEMONSTRATED it can do: price-implied growth (reverse DCF) and profitability (implied margin at the sustainable growth rate) vs the DuPont capability envelope — net margin x asset turnover x equity multiplier = ROE, retention, and the sustainable self-funding growth rate (ROE x retention). Financial-sector companies use the equity-based implied-ROE solve (justified P/B). Management guidance is context only, never the gap axis."
    )]
    pub async fn expectations_gap(
        &self,
        Parameters(req): Parameters<types::ExpectationsGapRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "expectations_gap", async {
            validate_symbol(&req.symbol)?;

            // ── 1. Fetch financial data ────────────────────────────────
            //
            // All six fetches are independent (no data dependency between
            // them) and run concurrently via `tokio::join!`. This is not
            // `try_join!` — we intentionally tolerate partial failures:
            // a failed income_statement must not prevent fetching
            // balance_sheet. The match below handles the Ok/Err cases
            // per-fetch. Running them concurrently keeps the total under
            // the 60s MCP `tools/call` cap (worst case = max single
            // fetch timeout, not sum of all fetch timeouts). The stock
            // quote is the price fallback: EODHD-routed profiles (every
            // exchange-qualified symbol) carry no `price` field.
            let (req_income, req_balance, req_cf, req_metrics, req_profile, req_quote) =
                tokio::join! {
                    self.fetch("income_statement", &req.symbol, &[("limit", "5")]),
                    self.fetch("balance_sheet", &req.symbol, &[("limit", "5")]),
                    self.fetch("cash_flow_statement", &req.symbol, &[("limit", "5")]),
                    self.fetch_key_metrics(&req.symbol, 5),
                    self.fetch_profile(&req.symbol),
                    self.fetch("stock_quote", &req.symbol, &[]),
                };

            // ── 2. Price-implied expectations vs demonstrated capability ──

            let mut price_source = "unavailable".to_string();
            let analysis = match (
                &req_income,
                &req_balance,
                &req_cf,
                &req_metrics,
                &req_profile,
            ) {
                (Ok(inc), Ok(bal), Ok(cf), Ok(met), Ok(prof)) => {
                    match resolve_current_price(prof.raw(), req_quote.as_ref().ok()) {
                        Some((raw_price, source)) => {
                            match self
                                .normalize_price_for_financials(raw_price, prof.raw(), inc)
                                .await
                            {
                                Ok((current_price, normalization)) => {
                                    price_source = format!("{source}; {normalization}");
                                    solve_expectations(
                                        inc,
                                        bal,
                                        cf,
                                        met.raw(),
                                        prof,
                                        current_price,
                                    )
                                }
                                Err(error) => {
                                    price_source = format!(
                                        "{source}; currency_normalization_failed: {error}"
                                    );
                                    None
                                }
                            }
                        }
                        None => None,
                    }
                }
                _ => None,
            };

            // ── 3. Management guidance — context annotation only ────────

            let company_name = match &req_profile {
                Ok(prof) => prof.company_name().unwrap_or(&req.symbol).to_string(),
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
            .await?;

            let claims = research::ResearchClaimClassifier::classify_all(&research);

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

            // ── 4. User estimate — context annotation only ─────────────

            let user_growth = req.growth_estimate.unwrap_or(0.05);

            // ── 5. Assemble the report ─────────────────────────────────

            let output = build_gap_report(
                &req.symbol,
                &analysis,
                &management_growth,
                user_growth,
                &management_narrative,
                claims.claims.len(),
                &price_source,
            );

            Ok(fibo::enrich_with_ontology(
                serde_json::json!(output),
                "expectations_gap",
            ))
        })
        .await
    }
}

// ── Price-implied vs demonstrated capability solve ─────────────────────

/// The solved expectations analysis: demonstrated DuPont capability plus
/// the price-implied drivers and their gaps.
pub(crate) struct ExpectationsSolve {
    pub capability: financial_model::DuPontAnalysis,
    /// "roe" for financial-sector companies, "net_margin" otherwise.
    pub headline: &'static str,
    pub implied_growth: Option<f64>,
    /// Net margin the price demands at the sustainable growth rate: NET
    /// INCOME / revenue, after interest at demonstrated leverage and tax —
    /// the equity holder's margin (operator ruling 2026-09-10, stated
    /// three times). Solved exactly through the enterprise model's internal
    /// gross-margin parameter via `GM = NM/(1−tax) + interest% + D&A%`, so
    /// the solved value satisfies NI = (EBIT − interest) × (1 − tax) by
    /// construction. Gross margin never appears as a reported quantity.
    pub implied_net_margin_at_sgr: Option<f64>,
    pub implied_roe: Option<f64>,
    /// Implied growth − sustainable growth rate, percentage points
    /// (non-financials only).
    pub growth_gap_pp: Option<f64>,
    /// Net-margin space for non-financials; ROE percentage points for
    /// financials — the profitability leg of the gap.
    pub profitability_gap_pp: Option<f64>,
    pub book_value_per_share: Option<f64>,
    /// The demonstrated self-funding growth rate (ROE × retention) — the
    /// capability anchor the growth leg compares the price against. Not
    /// redundant with `capability`: the growth gap is defined against it,
    /// and the report surfaces it beside the implied growth it anchors.
    pub sustainable_growth_rate: f64,
}

/// Solve the expectations analysis from raw provider payloads. Returns
/// `None` when history, capability (DuPont), or the price is unavailable —
/// the report then honestly reports both legs missing rather than
/// fabricating a partial solve.
pub(crate) fn solve_expectations(
    income: &serde_json::Value,
    balance: &serde_json::Value,
    cf: &serde_json::Value,
    metrics: &serde_json::Value,
    profile: &crate::CompanyProfile,
    current_price: f64,
) -> Option<ExpectationsSolve> {
    let income_arr = income.as_array()?;
    let balance_arr = balance.as_array()?;
    let cf_arr = cf.as_array()?;
    let metrics_arr = metrics.as_array();
    let profile_obj = profile.raw().as_array()?.first()?;

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

    if !current_price.is_finite() || current_price <= 0.0 {
        return None;
    }

    let capability = hist.dupont()?;

    if financial_model::is_financial_sector(profile) {
        // Financials: the equity-based solve. The justified P/B identity
        // gives the ROE the price demands at the self-funding growth rate;
        // the gap is against demonstrated ROE. Shares must resolve without
        // the nominal fallback — a fabricated share count would fabricate
        // book value per share.
        let shares = financial_model::resolve_shares_outstanding(
            income_arr,
            metrics_arr.map_or(&[], |v| v),
            profile_obj,
        )?;
        let equity = hist.total_equity.last().map(|(_, value)| *value)?;
        if shares <= 0.0 || equity <= 0.0 {
            return None;
        }
        let book_value_per_share = equity / shares;
        let sustainable_growth_rate = capability.sustainable_growth_rate;
        let implied_roe = financial_model::implied_roe_from_price_to_book(
            current_price,
            book_value_per_share,
            DEFAULT_COST_OF_EQUITY,
            sustainable_growth_rate,
        );
        let profitability_gap_pp = implied_roe.map(|roe| (roe - capability.roe) * 100.0);
        Some(ExpectationsSolve {
            capability,
            headline: "roe",
            implied_growth: None,
            implied_net_margin_at_sgr: None,
            implied_roe,
            growth_gap_pp: None,
            profitability_gap_pp,
            book_value_per_share: Some(book_value_per_share),
            sustainable_growth_rate,
        })
    } else {
        // Non-financials: the FCF reverse DCF. Growth leg: implied growth
        // at demonstrated margins vs the sustainable growth rate.
        // Profitability leg: the NET margin (net income / revenue) the price
        // demands at the sustainable growth rate — interest at demonstrated
        // leverage and tax included, because net income is what flows to
        // equity holders. The demonstrated side is the DuPont median of
        // actual reported net income / revenue — no formula. The gap is
        // like-for-like in net-income space. The enterprise model's gross
        // margin is an internal parameter only, reached through the exact
        // identity GM = NM/(1−tax) + interest% + D&A% (operator ruling
        // 2026-09-10).
        let assumptions = financial_model::ProjectionAssumptions::from_history(&hist);
        let sustainable_growth_rate = capability.sustainable_growth_rate;
        let demonstrated_net_margin = capability.net_profit_margin;
        let implied_growth = financial_model::implied_growth(&hist, &assumptions, current_price)
            .filter(|growth| {
                *growth > financial_model::IMPLIED_GROWTH_LO + 0.01
                    && *growth < financial_model::IMPLIED_GROWTH_HI - 0.01
            });
        let implied_net_margin_at_sgr = financial_model::implied_net_margin_at_growth(
            &hist,
            &assumptions,
            sustainable_growth_rate,
            current_price,
        );
        let growth_gap_pp = implied_growth.map(|growth| (growth - sustainable_growth_rate) * 100.0);
        let profitability_gap_pp = implied_net_margin_at_sgr
            .map(|net_margin| (net_margin - demonstrated_net_margin) * 100.0);
        Some(ExpectationsSolve {
            capability,
            headline: "net_margin",
            implied_growth,
            implied_net_margin_at_sgr,
            implied_roe: None,
            growth_gap_pp,
            profitability_gap_pp,
            book_value_per_share: None,
            sustainable_growth_rate,
        })
    }
}

// ── Management growth extraction from classified claims ───────────────

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

// ── Gap report builder ─────────────────────────────────────────────────

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn display_pct(value: f64) -> String {
    if value.is_finite() {
        format!("{:.1}%", value * 100.0)
    } else {
        "unavailable".to_string()
    }
}

/// expect: [P1] The gap axis is price-implied vs demonstrated DuPont
/// capability — management guidance never appears in gaps (operator ruling
/// 2026-09-10); it is a context annotation only.
/// dcterms:identifier: build_gap_report / solve_expectations
pub(crate) fn build_gap_report(
    symbol: &str,
    analysis: &Option<ExpectationsSolve>,
    management_growth: &[f64],
    user_growth: f64,
    narrative: &[String],
    total_claims: usize,
    price_source: &str,
) -> serde_json::Value {
    let mgmt_median = median(management_growth);

    let (signal, interpretation) = match analysis {
        None => (
            "insufficient_data",
            "Demonstrated capability (DuPont: net income, assets, equity over at least two years) or the current price is unavailable — the gap cannot be solved. Check the raw financial-data tools for this symbol.",
        ),
        Some(solve) => {
            let (solve_signal, solve_interpretation) = match solve.headline {
                "roe" => match solve.profitability_gap_pp {
                    Some(gap) if gap > 2.0 => (
                        "price_demands_more_than_demonstrated",
                        "The price demands more ROE than the company has demonstrated (DuPont median). Either the market anticipates capability improvement — repricing, mix shift, better underwriting — or expectations are too hot. The gap IS the thesis: identify the mechanism that would close it.",
                    ),
                    Some(gap) if gap < -2.0 => (
                        "price_demands_less_than_demonstrated",
                        "The price demands less ROE than demonstrated — the market prices decay below the demonstrated envelope. If the demonstrated ROE is durable, this is the value candidate; if the market sees a decay the DuPont history misses (credit cycle, capital drain), the discount is earned.",
                    ),
                    Some(_) => (
                        "aligned",
                        "Price-implied ROE sits inside the demonstrated capability envelope. Fairly priced relative to demonstrated economics; edge must come from a differentiated view on the ROE trajectory.",
                    ),
                    None => (
                        "insufficient_data",
                        "The implied-ROE solve is unavailable — the justified P/B identity is not invertible at this price/book and sustainable-growth combination (price-to-book above the perpetuity bound, or unresolvable shares).",
                    ),
                },
                _ => match (solve.growth_gap_pp, solve.profitability_gap_pp) {
                    (Some(growth), Some(profitability)) if growth > 3.0 && profitability > 0.5 => (
                        "price_demands_more_than_demonstrated",
                        "The price demands more growth AND more profitability than the company has demonstrated (DuPont capability envelope). Either the market anticipates capability improvement beyond the demonstrated trend — moat strengthening, mix shift, pricing power — or expectations are too hot. The gap IS the thesis: identify the mechanism that would close it.",
                    ),
                    (Some(growth), Some(profitability))
                        if growth < -3.0 && profitability < -0.5 =>
                    {
                        (
                            "price_demands_less_than_demonstrated",
                            "The price demands less growth and less profitability than demonstrated — the market prices decay below the demonstrated envelope. If the capability is durable, this is the value candidate; if the market sees decay the DuPont history misses (secular decline, margin normalization), the discount is earned.",
                        )
                    }
                    (Some(_), Some(_)) => (
                        "mixed",
                        "The growth and profitability legs point in opposite directions — the price trades growth against margin. Investigate which leg the market is paying for before forming a thesis.",
                    ),
                    (Some(growth), None) if growth > 3.0 => (
                        "price_demands_more_than_demonstrated",
                        "The price demands more growth than the sustainable self-funding rate; the profitability leg could not be solved within model bounds (see gaps).",
                    ),
                    (Some(growth), None) if growth < -3.0 => (
                        "price_demands_less_than_demonstrated",
                        "The price demands less growth than the sustainable self-funding rate; the profitability leg could not be solved within model bounds (see gaps).",
                    ),
                    (Some(_), None) | (None, Some(_)) => (
                        "insufficient_data",
                        "Only one leg of the gap solved within model bounds — see gaps for which is missing. A one-legged expectations read is a partial view by construction.",
                    ),
                    _ => (
                        "insufficient_data",
                        "Neither leg solved within the model's validated bounds (growth bracket, margin bracket, or sector guards). The honest read is unavailable, not zero.",
                    ),
                },
            };
            (solve_signal, solve_interpretation)
        }
    };

    let capability_json = match analysis {
        Some(solve) => serde_json::json!({
            "headline_measure": solve.headline,
            "net_profit_margin": solve.capability.net_profit_margin,
            "asset_turnover": solve.capability.asset_turnover,
            "equity_multiplier": solve.capability.equity_multiplier,
            "roe": solve.capability.roe,
            "retention": solve.capability.retention,
            "sustainable_growth_rate": solve.capability.sustainable_growth_rate,
            "years": solve.capability.years,
        }),
        None => serde_json::Value::Null,
    };

    let price_implied_json = match analysis {
        Some(solve) => serde_json::json!({
            "implied_growth": solve.implied_growth.map(|value| serde_json::json!({
                "value": value,
                "display": display_pct(value),
                "source": "reverse DCF: growth rate at demonstrated margins that equates intrinsic value to the current price",
            })).unwrap_or(serde_json::Value::Null),
            "implied_net_margin_at_sustainable_growth": solve.implied_net_margin_at_sgr.map(|value| serde_json::json!({
                "value": value,
                "display": display_pct(value),
                "source": "reverse DCF at the sustainable growth rate: the net income margin (net income / revenue, interest at demonstrated leverage and tax included) that equates equity value to the current price",
            })).unwrap_or(serde_json::Value::Null),
            "implied_roe": solve.implied_roe.map(|value| serde_json::json!({
                "value": value,
                "display": display_pct(value),
                "source": format!("justified P/B identity at COE {:.0}% and the sustainable growth rate", DEFAULT_COST_OF_EQUITY * 100.0),
            })).unwrap_or(serde_json::Value::Null),
            "book_value_per_share": solve.book_value_per_share,
        }),
        None => serde_json::Value::Null,
    };

    let gaps_json = match analysis {
        Some(solve) => serde_json::json!({
            "sustainable_growth_rate": solve.sustainable_growth_rate,
            "growth_gap_pp": solve.growth_gap_pp,
            "growth_gap_basis": if solve.growth_gap_pp.is_some() {
                serde_json::json!("implied growth − sustainable growth rate (ROE × retention)")
            } else {
                serde_json::Value::Null
            },
            "profitability_gap_pp": solve.profitability_gap_pp,
            "profitability_gap_basis": if solve.headline == "roe" {
                serde_json::json!("implied ROE − demonstrated ROE (percentage points)")
            } else {
                serde_json::json!("implied net margin at SGR − demonstrated net margin (DuPont median of actual net income / revenue), percentage points")
            },
        }),
        None => serde_json::Value::Null,
    };

    serde_json::json!({
        "symbol": symbol,
        "framework": "Expectations gap = price-implied expectations vs demonstrated financial capability. Capability is the DuPont envelope: ROE = net margin × asset turnover × equity multiplier, retention, and the sustainable self-funding growth rate (ROE × retention, Higgins 1977). Financial-sector companies use the equity-based implied-ROE solve (justified P/B); everyone else the FCF reverse DCF with an implied-margin solve at the sustainable growth rate. Operator ruling 2026-09-10 — management guidance is context only, never the gap axis. Mauboussin & Rappaport (2001) Expectations Investing.",
        "capability": capability_json,
        "price_implied": price_implied_json,
        "gaps": gaps_json,
        "context": {
            "management_guidance_median": if mgmt_median.is_finite() { serde_json::json!(mgmt_median) } else { serde_json::Value::Null },
            "guidance_samples": management_growth.len(),
            "user_estimate": user_growth,
            "note": "Context annotations. The gap axis is price-implied vs demonstrated DuPont capability (operator ruling 2026-09-10).",
        },
        "signal": signal,
        "interpretation": interpretation,
        "management_narrative": narrative,
        "data_quality": {
            "total_research_claims": total_claims,
            "guidance_claims_found": management_growth.len(),
            "capability_available": analysis.is_some(),
            "growth_leg_available": analysis.as_ref().is_some_and(|solve| solve.growth_gap_pp.is_some() || solve.implied_roe.is_some()),
            "profitability_leg_available": analysis.as_ref().is_some_and(|solve| solve.profitability_gap_pp.is_some()),
            "price_source": price_source,
        },
    })
}
