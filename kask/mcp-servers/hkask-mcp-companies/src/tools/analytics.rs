//! Company DCF valuation and scenario tools.
use crate::{
    CompaniesServer, StoredForecast, fibo, financial_model, resolve_current_price,
    screen_store::PersistedForecast, types, validate_symbol,
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
            self.validate_revision_of(&req.symbol, req.revision_of.as_deref())
                .await?;

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

            let (history, assumptions) =
                match crate::valuation_service::load_projection(
                    self,
                    &req.symbol,
                    "reverse_dcf",
                    &req,
                )
                .await
                {
                    Ok(loaded) => loaded,
                    Err(error) => return error.into_tool_result(),
                };
            let crate::valuation_service::FinancialHistory { hist, profile, income } = history;

            let signal_quality = hist.signal_quality();
            crate::data_quality::emit_data_quality_span(
                &req.symbol, "reverse_dcf", &signal_quality,
            );

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
        })
        .await
    }
}
