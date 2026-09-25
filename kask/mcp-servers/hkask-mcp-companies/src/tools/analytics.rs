//! Company DCF valuation and scenario tools.
use super::notes::run_store;
use crate::{
    CompaniesServer, StoredForecast, fibo, financial_model, research_store::PersistedForecast,
    resolve_current_price, scenarios, superforecast, types, validate_symbol,
};
use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::time::now_rfc3339;
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};
use uuid::Uuid;

#[tool_router(router = analytics_router, vis = "pub")]
impl CompaniesServer {
    #[tool(
        description = "Two-stage DCF valuation. Projects income statement, balance sheet, and cash flow line items to derive free cash flow, then discounts back to enterprise value and intrinsic equity per share. Projects 11 line items per period (revenue, COGS, gross profit, D&A, EBIT, tax, NOPAT, capex, change in NWC, FCF, PV). Returns a forecast_id for later decomposition via forecast_record. Default: 10yr model, 3yr stage 1, 7yr stage 2, 10% WACC, 2.5% terminal growth."
    )]
    pub async fn dcf_valuation(
        &self,
        Parameters(req): Parameters<types::DcfValuationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "dcf_valuation", async {
            validate_symbol(&req.symbol)?;
            if let Some(ref revision_of) = req.revision_of {
                let revision_of = revision_of.clone();
                let symbol = req.symbol.clone();
                run_store(self.research.clone(), move |portfolio| {
                    portfolio.validate_forecast_revision(&revision_of, &symbol)
                })
                .await?;
            }

            let profile = self.fetch_profile(&req.symbol).await?;
            let prepared = match crate::valuation_service::prepare_dcf(
                self,
                &req.symbol,
                &profile,
                types::ProjectionAssumptionOverrides::from(&req),
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => return error.into_tool_result(),
            };
            let crate::valuation_service::PreparedDcf {
                history: hist,
                signal_quality,
                assumptions,
                model,
                current_price,
                provenance,
                warnings,
            } = prepared;
            let shares = hist.shares_outstanding;

            // Emit the same model quality carried by the comparable overlay.
            crate::data_quality::emit_data_quality_span(
                &req.symbol,
                "dcf_valuation",
                &signal_quality,
            );

            // Generate forecast ID for later decomposition
            let forecast_id = Uuid::new_v4().to_string();

            // Persist the forecast model for later decomposition across restarts.
            let stored = StoredForecast {
                model: model.clone(),
                assumptions: assumptions.clone(),
                current_price,
                intrinsic_per_share: model.intrinsic_per_share,
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

            // The response assembly is pure — delegate to `valuation_service`
            // so it is testable without HTTP/API keys. The tool handler retains
            // only fetch, validate, persist, and the span.
            let mut output = crate::valuation_service::build_dcf_response(
                &req.symbol,
                &forecast_id,
                &req.revision_of,
                &model,
                &assumptions,
                &hist,
                &signal_quality,
                current_price,
                shares,
            );

            output["provenance"] = provenance;
            output["warnings"] = serde_json::json!(warnings);
            Ok(fibo::enrich_with_ontology(output, "dcf_valuation"))
        })
        .await
    }

    #[tool(
        description = "Reverse DCF (Mauboussin's Expectations Investing). Solves for the revenue growth rate implied by the current stock price. \"What growth does the market expect?\" - compare to your own estimate to find mispricing. Default: 10yr model, 3yr stage 1, 7yr stage 2, 10% WACC."
    )]
    pub async fn reverse_dcf(
        &self,
        Parameters(req): Parameters<types::ReverseDcfRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "reverse_dcf", async {
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

            let income_arr = income.as_array();
            let balance_arr = balance.as_array();
            let cf_arr = cf.as_array();
            let metrics_arr = metrics.as_array();
            let profile_obj = profile.raw().as_array().and_then(|a| a.first());

            let (Some(income_data), Some(balance_data), Some(cf_data), Some(profile_data)) = (
                income_arr.filter(|a| !a.is_empty()),
                balance_arr.filter(|a| !a.is_empty()),
                cf_arr.filter(|a| !a.is_empty()),
                profile_obj,
            )
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "reverse_dcf") {
                return Ok(err);
            }

            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol, "reverse_dcf", &signal_quality,
            );

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
                self.investor_required_return,
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            // FMP profiles can carry a current price; EODHD-routed profiles
            // generally require the quote fallback. The raw security price is
            // then converted from the listing currency into the normalized
            // statement currency before entering the valuation model.
            let (raw_price, raw_price_source) =
                match resolve_current_price(profile.raw(), None) {
                    Some((price, source)) => (price, source),
                    None => {
                        let quote = self.fetch("stock_quote", &req.symbol, &[]).await?;
                        match resolve_current_price(profile.raw(), Some(&quote)) {
                            Some((price, source)) => (price, source),
                            None => {
                                return Err(McpToolError::invalid_argument(
                                    "current price must be positive for reverse DCF",
                                ))
                            }
                        }
                    }
                };
            let (current_price, normalization) = self
                .normalize_price_for_financials(raw_price, profile.raw(), &income)
                .await?;
            let price_source = format!("{raw_price_source}; {normalization}");

            // Solve via the shared bisection in `financial_model` — the single
            // source of truth for the search direction, shared with
            // `expectations_gap`. Report the out-of-bracket cases distinctly so
            // the caller learns which bound was violated.
            let implied_growth = match financial_model::implied_growth(
                &hist,
                &assumptions,
                current_price,
            ) {
                Some(growth) => growth,
                None => {
                    let at = |growth: f64| {
                        financial_model::project_financial_model(
                            &hist,
                            &financial_model::ProjectionAssumptions {
                                revenue_growth: growth,
                                ..assumptions.clone()
                            },
                        )
                        .map(|model| model.intrinsic_per_share)
                        .map_err(|error| McpToolError::invalid_argument(error.to_string()))
                    };
                    let lo_intrinsic = at(financial_model::IMPLIED_GROWTH_LO)?;
                    if lo_intrinsic > current_price {
                        return Err(McpToolError::invalid_argument(format!(
                            "price ({current_price:.2}) below intrinsic ({lo_intrinsic:.2}) at -50% growth - stock may be distressed or data inconsistent"
                        )));
                    }
                    let hi_intrinsic = at(financial_model::IMPLIED_GROWTH_HI)?;
                    return Err(McpToolError::invalid_argument(format!(
                        "price ({current_price:.2}) implies growth > 100% - intrinsic at +100% growth is {hi_intrinsic:.2}"
                    )));
                }
            };

            // Final model at implied growth
            let mut final_a = assumptions.clone();
            final_a.revenue_growth = implied_growth;
            let result = financial_model::project_financial_model(&hist, &final_a)
                .map_err(|error| McpToolError::invalid_argument(error.to_string()))?;

            let output = serde_json::json!({
                "symbol": req.symbol,
                "current_price": current_price,
                "price_source": price_source,
                "implied_growth_rate": implied_growth,
                "intrinsic_at_implied": result.intrinsic_per_share,
                "enterprise_value": result.enterprise_value,
                "config": {
                    "stage1_years": assumptions.stage1_years,
                    "stage2_years": assumptions.total_years - assumptions.stage1_years,
                    "discount_rate": assumptions.discount_rate,
                    "terminal_growth": assumptions.terminal_growth,
                },
                "interpretation": {
                    "implied_growth_pct": format!("{:.1}%", implied_growth * 100.0),
                    "signal": if implied_growth < 0.05 { "low_expectations" } else if implied_growth > 0.15 { "high_expectations" } else { "moderate_expectations" },
                    "mauboussin_framework": "The current stock price implies a revenue growth rate. Compare this to your own estimate of sustainable growth. If your estimate is higher, the stock may be undervalued. If lower, it may be overvalued. The gap between implied and expected growth is the expectations gap - the core of Expectations Investing (Mauboussin & Rappaport, 2001).",
                },
            });

            Ok(fibo::enrich_with_ontology(output, "reverse_dcf"))
        }).await
    }

    #[tool(
        description = "Schwartz 2x2 scenario analysis. Projects four scenarios (Bull, Land Grab, Cash Cow, Bear) based on revenue growth x profit margin axes. Runs DCF under each scenario and returns the intrinsic value range. Default axes: revenue_growth x profit_margin. Adjustable multipliers let you tune scenario severity. Detailed mode (event_tree supplied) also emits the T8a risk core: a probability-weighted risk measure (expected return, sigma_scenario), APT-style factor loadings (beta per axis) over the branch revaluations, and — when the tree is CMP-built — the R4 CMP-provenance risk measure (cmp_controlled)."
    )]
    pub async fn scenario_analysis(
        &self,
        Parameters(req): Parameters<types::ScenarioAnalysisRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_analysis", async {
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

            let income_arr = income.as_array();
            let balance_arr = balance.as_array();
            let cf_arr = cf.as_array();
            let metrics_arr = metrics.as_array();
            let profile_obj = profile.raw().as_array().and_then(|a| a.first());

            let (Some(income_data), Some(balance_data), Some(cf_data), Some(profile_data)) = (
                income_arr.filter(|a| !a.is_empty()),
                balance_arr.filter(|a| !a.is_empty()),
                cf_arr.filter(|a| !a.is_empty()),
                profile_obj,
            )
            else {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient data"}));
            };
            let metrics_data: &[serde_json::Value] = metrics_arr.map_or(&[], |v| v);

            let hist = financial_model::HistoricalSnapshot::from_api_json(
                income_data, balance_data, cf_data, metrics_data, profile_data,
            );

            if hist.revenue.len() < 2 {
                return Ok(serde_json::json!({"symbol": req.symbol, "error": "insufficient historical data - need at least 2 years of revenue"}));
            }

            if let Some(err) = financial_model::financial_sector_guard(&profile, &req.symbol, "scenario_analysis") {
                return Ok(err);
            }

            let assumptions = financial_model::ProjectionAssumptions::from_history_with_overrides(
                &hist,
                types::ProjectionAssumptionOverrides::from(&req),
                self.investor_required_return,
            )
            .map_err(|err| McpToolError::invalid_argument(err.to_string()))?;

            let current_price = profile.price().unwrap_or(0.0);

            let matrix = scenarios::ScenarioMatrix::growth_x_margin(assumptions.revenue_growth, assumptions.gross_margin);
            let results = scenarios::run_scenario_analysis(&hist, &assumptions, &matrix)
                .map_err(|error| McpToolError::invalid_argument(error.to_string()))?;

            let summary = scenarios::scenario_summary(&results);

            // Optional tree-weighted path (detailed mode). The 2×2 range
            // above is always computed; when the caller pastes a validated
            // event tree, quadrant probabilities are derived from its root
            // marginals and an expected intrinsic value is produced.
            let mut weighting_mode = superforecast::WeightingMode::Schwartz2x2;
            let mut weighted_output: Option<serde_json::Value> = None;
            let mut tree_warning: Option<String> = None;
            if let Some(tree_json) = &req.event_tree {
                match serde_json::from_str::<superforecast::EventTreeProjection>(tree_json) {
                    Ok(tree) => match superforecast::tree_root_probabilities(&tree) {
                        Some((growth_p, margin_p)) => {
                            let weighted = superforecast::distribute_scenario_probabilities(
                                growth_p, margin_p, &results,
                            );
                            let expected = superforecast::expected_intrinsic(&weighted);

                            // T8a risk core: probability-weighted risk measure
                            // and APT-style factor loadings over the branch
                            // revaluations. The branch return is the annualized
                            // return from the current price to the branch's
                            // intrinsic value over the DCF horizon. Skipped with
                            // a named reason (never silently) when a return is
                            // undefined.
                            let horizon_years = results
                                .first()
                                .map_or(0.0, |r| r.model.periods.len() as f64);
                            let negative_intrinsic =
                                weighted.iter().any(|w| w.intrinsic_per_share < 0.0);
                            let risk_skip_reason = if current_price <= 0.0 {
                                Some("current price is not positive")
                            } else if horizon_years <= 0.0 {
                                Some("DCF horizon is zero")
                            } else if negative_intrinsic {
                                Some("a branch intrinsic value is negative")
                            } else {
                                None
                            };
                            // `weighted` is built from `results` by index
                            // (distribute_scenario_probabilities), so the
                            // node-true vectors zip cleanly onto the branches.
                            let branches: Vec<hkask_forecast::BranchOutcome> =
                                weighted.iter().map(|w| {
                                    let branch_return = if w.intrinsic_per_share > 0.0 {
                                        (w.intrinsic_per_share / current_price)
                                            .powf(1.0 / horizon_years)
                                            - 1.0
                                    } else {
                                        // Zero intrinsic: total loss of the position.
                                        -1.0
                                    };
                                    hkask_forecast::BranchOutcome {
                                        probability: w.probability,
                                        branch_return,
                                    }
                                }).collect();
                            let risk_measure = if risk_skip_reason.is_none() {
                                hkask_forecast::scenario_risk_measure(&branches)
                            } else {
                                None
                            };
                            let growth_node_true: Vec<bool> = results
                                .iter()
                                .map(|r| r.scenario.axis1_multiplier > 1.0)
                                .collect();
                            let margin_node_true: Vec<bool> = results
                                .iter()
                                .map(|r| r.scenario.axis2_multiplier > 1.0)
                                .collect();
                            let growth_loading = if risk_skip_reason.is_none() {
                                hkask_forecast::scenario_node_loading(
                                    &branches, &growth_node_true,
                                )
                            } else {
                                None
                            };
                            let margin_loading = if risk_skip_reason.is_none() {
                                hkask_forecast::scenario_node_loading(
                                    &branches, &margin_node_true,
                                )
                            } else {
                                None
                            };
                            let factor_loading_note =
                                if risk_skip_reason.is_none()
                                    && (growth_loading.is_none()
                                        || margin_loading.is_none())
                                {
                                    Some("a conditioning set has zero probability mass — that axis loading is undefined")
                                } else {
                                    None
                                };

                            // R4: the same branches with CMP provenance. A
                            // quadrant probability derives from both tree
                            // roots, so the branch is CMP-controlled only when
                            // BOTH roots are CMP indices (a single raw root
                            // contaminates every quadrant — the measure's
                            // cmp_controlled flag then reports the confound).
                            let roots_cmp_controlled: Vec<bool> = tree
                                .root_ids
                                .iter()
                                .map(|id| {
                                    tree.cmp_provenance.iter().any(|c| c.id == *id)
                                })
                                .collect();
                            let cmp_source = match (
                                roots_cmp_controlled.first(),
                                roots_cmp_controlled.get(1),
                            ) {
                                (Some(true), Some(true)) => {
                                    tree.root_ids
                                        .first()
                                        .zip(tree.root_ids.get(1))
                                        .map(|(a, b)| format!("{a}+{b}"))
                                }
                                _ => None,
                            };
                            let cmp_branches: Vec<hkask_forecast::CmpBranchOutcome> =
                                branches.iter().map(|b| {
                                    hkask_forecast::CmpBranchOutcome {
                                        probability: b.probability,
                                        branch_return: b.branch_return,
                                        cmp_source: cmp_source.clone(),
                                    }
                                }).collect();
                            let cmp_risk_measure = if risk_skip_reason.is_none() {
                                hkask_forecast::cmp_scenario_risk_measure(&cmp_branches)
                            } else {
                                None
                            };
                            let cmp_controlled_note = match &cmp_risk_measure {
                                Some(rm) if !rm.cmp_controlled => Some(
                                    "at least one tree root is not a CMP index — the risk measure carries the maturity-transformation confound",
                                ),
                                _ => None,
                            };

                            weighting_mode = superforecast::WeightingMode::EventTree;
                            let cmp_provenance = if tree.cmp_provenance.is_empty() {
                                None
                            } else {
                                Some(&tree.cmp_provenance)
                            };
                            weighted_output = Some(serde_json::json!({
                                "growth_probability": growth_p,
                                "margin_probability": margin_p,
                                "expected_intrinsic_per_share": expected,
                                "weighted_scenarios": weighted.iter().map(|w| serde_json::json!({
                                    "name": w.name,
                                    "intrinsic_per_share": w.intrinsic_per_share,
                                    "probability": w.probability,
                                })).collect::<Vec<_>>(),
                                // T8a risk core (hkask_forecast::scenario_risk_measure).
                                "risk_measure": risk_measure.map(|rm| serde_json::json!({
                                    "expected_return": rm.expected_return,
                                    "sigma_scenario": rm.sigma_scenario,
                                    "branch_count": rm.branch_count,
                                    "probability_mass": rm.probability_mass,
                                })),
                                "risk_measure_note": risk_skip_reason.map(|reason| format!(
                                    "{reason} — scenario risk measure undefined (never fabricated)"
                                )),
                                // T8a factor exposures (hkask_forecast::scenario_node_loading):
                                // β(axis) = E[r | axis high] − E[r | axis low].
                                "factor_loadings": {
                                    "revenue_growth_beta": growth_loading,
                                    "gross_margin_beta": margin_loading,
                                },
                                "factor_loadings_note": factor_loading_note,
                                // R4 (hkask_forecast::cmp_scenario_risk_measure): the
                                // risk measure with CMP provenance.
                                "cmp_risk_measure": cmp_risk_measure.map(|rm| serde_json::json!({
                                    "expected_return": rm.inner.expected_return,
                                    "sigma_scenario": rm.inner.sigma_scenario,
                                    "branch_count": rm.inner.branch_count,
                                    "probability_mass": rm.inner.probability_mass,
                                    "cmp_controlled": rm.cmp_controlled,
                                    "cmp_branch_count": rm.cmp_branch_count,
                                })),
                                "cmp_controlled_note": cmp_controlled_note,
                                // R3: cite CMP provenance when the tree was built from CMP indices.
                                "cmp_provenance": cmp_provenance.map(|p| p.iter().map(|c| serde_json::json!({
                                    "id": c.id,
                                    "family": c.family,
                                    "tenor": c.tenor,
                                    "orientation": c.orientation,
                                    "venue": c.venue,
                                    "method": c.method,
                                    "maturity_error_days": c.maturity_error_days,
                                })).collect::<Vec<_>>()),
                            }));
                        }
                        None => {
                            tree_warning = Some(
                                "event tree does not have exactly two roots with valid marginals - falling back to simple 2x2 mode (no probabilities)".into()
                            );
                        }
                    },
                    Err(e) => {
                        tree_warning = Some(format!(
                            "event_tree JSON did not match the scenarios-server tree projection ({e}) - falling back to simple 2x2 mode"
                        ));
                    }
                }
            }

            // Compute signal quality and emit Regulation span
            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol, "scenario_analysis", &signal_quality,
            );

            let scenario_output: Vec<serde_json::Value> = results.iter().map(|r| {
                serde_json::json!({
                    "name": r.scenario.name,
                    "description": r.scenario.description,
                    "applied_growth": r.applied_growth,
                    "applied_margin": r.applied_margin,
                    "intrinsic_per_share": r.intrinsic_per_share,
                    "enterprise_value": r.model.enterprise_value,
                    "margin_of_safety": if current_price > 0.0 { (r.intrinsic_per_share - current_price) / current_price } else { 0.0 },
                })
            }).collect();

            let output = serde_json::json!({
                "symbol": req.symbol,
                "weighting_mode": weighting_mode,
                "tree_weighted": weighted_output,
                "tree_warning": tree_warning,
                "axes": {
                    "axis1": {"name": matrix.axis1.name, "metric": matrix.axis1.metric, "baseline": matrix.axis1.baseline},
                    "axis2": {"name": matrix.axis2.name, "metric": matrix.axis2.metric, "baseline": matrix.axis2.baseline},
                },
                "scenarios": scenario_output,
                "summary": {
                    "intrinsic_range": [summary.intrinsic_range.0, summary.intrinsic_range.1],
                    "intrinsic_average": summary.intrinsic_average,
                    "current_price": current_price,
                    "upside_pct": summary.upside_pct,
                    "downside_pct": summary.downside_pct,
                    "range_spread_pct": summary.range_spread_pct,
                },
                "data_quality": {
                    "overall_confidence": signal_quality.overall_confidence,
                    "quality_warning": signal_quality.quality_warning,
                },
                "framework": "Schwartz 2x2 scenario matrix: revenue growth x gross margin. Four scenarios: Bull (high/high), Land Grab (high/low), Cash Cow (low/high), Bear (low/low). Each scenario runs through the two-stage DCF model. The range of intrinsic values represents the uncertainty around the single-point DCF estimate. Simple mode (default) returns the range without probabilities; detailed mode (event_tree supplied) derives quadrant probabilities from the tree's root marginals - the earned upgrade on the analyst maturity ladder.",
            });

            Ok(fibo::enrich_with_ontology(output, "scenario_analysis"))
        }).await
    }
}
