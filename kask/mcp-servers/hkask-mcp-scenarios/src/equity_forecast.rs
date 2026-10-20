//! Equity forecast persistence and outcome scoring — ported from the
//! companies MCP server (Phase A P1 of the companies/FMP/EODHD/FIBO removal,
//! commit `3439db5e00`; the executed design record is preserved in git
//! history at `kask/docs/plans/companies-fmp-eodhd-fibo-removal.md`).
//!
//! The tools that moved here are provider-free: every input arrives from
//! the caller (`forecast_price_change` and `actual_price_change` are
//! parameters, not fetched statements). The Brier math itself already
//! lived in the shared `hkask-forecast` crate — only the persistence and
//! tool surface needed a home after the companies server's deletion.
//!
//! What was deliberately NOT ported (plan §3 P1 design note): the
//! 11-line-item DCF gap decomposition. Its inputs were provider
//! statements (`financial_model::HistoricalSnapshot::from_api_json` over
//! FMP/EODHD fetches) and `ProjectedFinancialModel` — with the providers
//! gone there is no non-provider source for those inputs, so
//! `forecast_record` reports `"decomposition": "unavailable — the
//! projected-model decomposition required provider statements, which the
//! companies server owned"` as a named note, never a silent skip. Brier
//! arithmetic on an eligible declared return target is retained.
//!
//! The scenario-join fields (PR-09) survive: `scenario_project_id`,
//! `scenario_tree`, `impact_mappings_ref`, and `fused_volatility` are
//! recorded on the stored forecast so the equity forecast still carries
//! its scenario provenance.
//!
//! The N2 wrapper (`scenario_risk_measure`) exposes the shared
//! `hkask_forecast` risk functions as caller-supplied-branch tools — the
//! DCF engine that used to revalue the company per branch is gone, so
//! the caller supplies the branch returns (per operator decision N2,
//! 2026-10-18).

use hkask_types::AnyJsonValue;
use hkask_types::time::now_rfc3339;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ScenariosServer;
use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

/// A stored equity forecast awaiting or having received its outcome.
/// Schema version 2 freezes prospective eligibility; v1 remains unscored history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEquityForecast {
    /// Schema version for forward-compatible deserialization.
    #[serde(default)]
    pub schema_version: u32,
    pub forecast_id: String,
    pub symbol: String,
    pub forecast_date: String,
    pub horizon: String,
    pub forecast_multiple: Option<f64>,
    pub forecast_price: Option<f64>,
    pub current_price: Option<f64>,
    pub forecast_price_change: f64,
    pub forecast_probability: Option<f64>,
    pub revision_of: Option<String>,
    /// Scenario join (PR-09) — the equity forecast carries its scenario
    /// provenance; the join is recorded, not agent-mediated.
    pub scenario_project_id: Option<String>,
    pub scenario_tree: Option<serde_json::Value>,
    pub impact_mappings_ref: Option<String>,
    pub fused_volatility: Option<f64>,
    /// None = still pending, Some = the recorded outcome JSON.
    #[serde(default)]
    pub outcome: Option<serde_json::Value>,
    pub created_at: String,
    /// Only new prospective declarations qualify; legacy records remain history.
    #[serde(default)]
    pub declaration: Option<EquityDeclaration>,
}

/// Frozen return-accuracy proposition and caller-reported resolver rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityDeclaration {
    pub deadline: chrono::NaiveDate,
    pub resolver: String,
    pub tolerance: f64,
}

// ── Request types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EquityForecastPersistRequest {
    pub symbol: String,
    /// When the forecast was made (YYYY-MM-DD)
    pub forecast_date: String,
    /// Forecast horizon: "3mo", "6mo", "1yr", "2yr", or "3yr"
    pub horizon: String,
    /// Forecast price change over the horizon (e.g., 0.10 = 10% return).
    /// The primary Brier signal. When omitted, the tool computes it from
    /// `forecast_price` and `current_price`.
    pub forecast_price_change: Option<f64>,
    /// Forecast valuation multiple (e.g., P/E or EV/FCF). Optional.
    pub forecast_multiple: Option<f64>,
    /// Forecast price target (absolute price).
    pub forecast_price: Option<f64>,
    /// Current market price at forecast date.
    pub current_price: Option<f64>,
    /// The forecast's own probability (0.0-1.0) that the price change
    /// lands within the tolerance band. Missing probability remains unscored.
    pub forecast_probability: Option<f64>,
    /// Optional parent forecast ID for a same-symbol revision.
    pub revision_of: Option<String>,
    /// Optional caller-supplied forecast ID; the server generates a UUID
    /// when omitted.
    pub forecast_id: Option<String>,
    /// Scenario join (PR-09): the scenarios server's project record id
    /// this price target was derived under.
    pub scenario_project_id: Option<String>,
    /// Scenario join (PR-09): the resolved scenario tree snapshot (from
    /// scenario_quantify — node ids and marginals) the valuation used.
    pub scenario_tree: Option<AnyJsonValue>,
    /// Scenario join (PR-09): a reference to the per-node DCF impact
    /// mappings used (the caller's description or digest of them).
    pub impact_mappings_ref: Option<String>,
    /// Scenario join (PR-09): the fused volatility from a scenario
    /// volatility analysis, when a realized volatility was supplied.
    pub fused_volatility: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EquityForecastRecordRequest {
    pub symbol: String,
    /// When the forecast was made (YYYY-MM-DD)
    pub forecast_date: String,
    /// Forecast horizon: "3mo", "6mo", "1yr", "2yr", or "3yr"
    pub horizon: String,
    /// Forecast valuation multiple (e.g., P/E or EV/FCF)
    pub forecast_multiple: f64,
    /// Forecast price change over the horizon (e.g., 0.10 = 10% return)
    pub forecast_price_change: f64,
    /// Actual outcome date (YYYY-MM-DD)
    pub outcome_date: String,
    /// Actual valuation multiple at outcome date
    pub actual_multiple: f64,
    /// Actual price change from forecast date to outcome date
    pub actual_price_change: f64,
    /// Forecast ID from a prior forecast_persist call. When provided and
    /// the stored forecast exists, the outcome is recorded on it and the
    /// stored forecast_probability is used as the Brier prior.
    pub forecast_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EquityForecastListRequest {
    pub symbol: String,
}

/// A single scenario branch for caller-supplied risk analysis.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct BranchSpec {
    /// The branch's joint probability (product of path conditionals).
    pub probability: f64,
    /// The asset's annualized return if the branch realizes — the caller
    /// supplies the revalued return (the companies DCF engine that
    /// computed it per branch is gone; per operator decision N2).
    pub branch_return: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScenarioRiskRequest {
    /// Scenario branches: joint probability and the caller-supplied
    /// branch return for each.
    pub branches: Vec<BranchSpec>,
    /// Optional realized (historical) annual volatility, decimal
    /// (0.35 = 35%). When supplied, the tool also emits fused_volatility
    /// — the root-sum-square fusion with the scenario sigma.
    pub realized_volatility: Option<f64>,
    /// Optional scale in [0,1] for the scenario channel's contribution to
    /// the fusion — 1.0 for a fully validated tree, less for partial
    /// coverage. Defaults to 1.0 when the caller omits it.
    pub scenario_weight: Option<f64>,
}

// ── Validation ──────────────────────────────────────────────────────────

fn validate_finite(name: &str, value: f64) -> Result<(), McpToolError> {
    if !value.is_finite() {
        return Err(McpToolError::invalid_argument(format!(
            "{name} must be finite, got {value}"
        )));
    }
    Ok(())
}

fn validate_symbol(symbol: &str) -> Result<(), McpToolError> {
    let trimmed = symbol.trim();
    if trimmed.is_empty() {
        return Err(McpToolError::invalid_argument("symbol must be non-empty"));
    }
    if trimmed.len() > 32 {
        return Err(McpToolError::invalid_argument(format!(
            "symbol must be at most 32 characters, got {} ('{trimmed}')",
            trimmed.len()
        )));
    }
    Ok(())
}

fn validate_horizon(horizon: &str) -> Result<(), McpToolError> {
    match horizon {
        "3mo" | "6mo" | "1yr" | "2yr" | "3yr" => Ok(()),
        other => Err(McpToolError::invalid_argument(format!(
            "horizon must be one of: 3mo, 6mo, 1yr, 2yr, 3yr — got '{other}'"
        ))),
    }
}

fn now_rfc3339_string() -> String {
    now_rfc3339()
}

// ── Tool implementations ────────────────────────────────────────────────────

#[tool_router(router = equity_forecast_router, vis = "pub")]
impl ScenariosServer {
    /// Persist a pre-computed equity price target for later Brier
    /// scoring. Ported from the companies server's `forecast_persist`
    /// (caller-supplied inputs only — no provider fetches). The
    /// DCF-model decomposition fields were dropped with the providers
    /// (plan §3 P1); the stored snapshot is a minimal pre-computed
    /// price-target record.
    #[tool(
        description = "Persist a pre-computed equity price target for later Brier scoring. Unlike forecast_record (which requires the actual outcome), this tool stores a pending price target without an outcome. The stored forecast can later be resolved by forecast_record when the horizon passes — Brier loss uses the frozen declared return-accuracy proposition (multiple-direction probability is not modeled); the 11-line-item gap decomposition is unavailable (its inputs were provider statements, which the companies server owned — the note is surfaced, never silent). Pass forecast_probability (the forecast's own confidence that the price change lands within the tolerance band) for prospective scoring. Missing probabilities and backdated records remain unscored; raw loss does not establish calibration. Optional scenario join fields record the price target's scenario provenance: scenario_project_id (the scenarios server's project record id), scenario_tree (the scenario_quantify tree snapshot), impact_mappings_ref (the per-node impact mappings used), and fused_volatility (from a scenario volatility fusion) — so the equity forecast carries the scenario join instead of leaving it agent-mediated."
    )]
    pub async fn forecast_persist(
        &self,
        Parameters(req): Parameters<EquityForecastPersistRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_persist", async {
            validate_symbol(&req.symbol)?;
            validate_horizon(&req.horizon)?;
            if let Some(value) = req.forecast_multiple {
                validate_finite("forecast_multiple", value)?;
            }
            if let Some(value) = req.forecast_price {
                validate_finite("forecast_price", value)?;
            }
            if let Some(value) = req.current_price {
                validate_finite("current_price", value)?;
            }
            if let Some(probability) = req.forecast_probability {
                validate_finite("forecast_probability", probability)?;
                if !(0.0..=1.0).contains(&probability) {
                    return Err(McpToolError::invalid_argument(format!(
                        "forecast_probability must be within [0, 1], got {probability}"
                    )));
                }
            }
            if let Some(sigma) = req.fused_volatility {
                validate_finite("fused_volatility", sigma)?;
            }
            if let Some(project_id) = req.scenario_project_id.as_deref() {
                if project_id.trim().is_empty() {
                    return Err(McpToolError::invalid_argument(
                        "scenario_project_id must be non-empty when provided",
                    ));
                }
            }
            // Resolve the forecast price change: prefer the direct field,
            // else compute from forecast_price and current_price. Reject
            // if neither path is available — persisting a PT with no price
            // change is a broken calibration signal.
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
            if let Some(revision_of) = req.revision_of.as_deref() {
                validate_symbol(revision_of)?;
            }

            let forecast_id = req
                .forecast_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let forecast_date = chrono::NaiveDate::parse_from_str(&req.forecast_date, "%Y-%m-%d")
                .map_err(|e| McpToolError::invalid_argument(format!("invalid forecast_date: {e}")))?;
            let months = match req.horizon.as_str() {
                "3mo" => 3, "6mo" => 6, "1yr" => 12, "2yr" => 24, "3yr" => 36,
                _ => return Err(McpToolError::invalid_argument("unsupported horizon")),
            };
            let deadline = forecast_date.checked_add_months(chrono::Months::new(months))
                .ok_or_else(|| McpToolError::invalid_argument("forecast horizon overflows date"))?;
            let prospective = forecast_date == chrono::Utc::now().date_naive();
            let declaration = (prospective && req.forecast_probability.is_some()).then(|| EquityDeclaration {
                deadline,
                resolver: "caller_reported_price_return_at_horizon".to_string(),
                tolerance: 0.20,
            });
            let record = StoredEquityForecast {
                schema_version: 2,
                forecast_id: forecast_id.clone(),
                symbol: req.symbol.clone(),
                forecast_date: req.forecast_date.clone(),
                horizon: req.horizon.clone(),
                forecast_multiple: req.forecast_multiple,
                forecast_price: req.forecast_price,
                current_price: req.current_price,
                forecast_price_change,
                forecast_probability: req.forecast_probability,
                revision_of: req.revision_of.clone(),
                scenario_project_id: req.scenario_project_id.clone(),
                scenario_tree: req.scenario_tree.as_ref().map(|tree| {
                    serde_json::to_value(tree).unwrap_or(serde_json::Value::Null)
                }),
                impact_mappings_ref: req.impact_mappings_ref.clone(),
                fused_volatility: req.fused_volatility,
                outcome: None,
                created_at: now_rfc3339_string(),
                declaration,
            };
            {
                let mut store = self
                    .equity_forecast_store
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if store.get(&forecast_id).is_some() {
                    return Err(McpToolError::invalid_argument(format!(
                        "forecast_id '{forecast_id}' already exists — pass \
                         revision_of to record a revision, or a fresh id"
                    )));
                }
                store
                    .insert(forecast_id.clone(), record)
                    .map_err(|error| {
                        McpToolError::internal(format!(
                            "equity forecast journal write failed: {error}"
                        ))
                    })?;
            }

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
                "forecast_probability": req.forecast_probability,
                "scoring_eligible": prospective && req.forecast_probability.is_some(),
                "eligibility_note": "Only a probability persisted on its forecast_date is prospective. Backdated and missing-probability records remain unscored history; source is caller-reported, not authenticated.",
                "deadline": deadline,
                "note": if prospective && req.forecast_probability.is_some() {
                    "Pre-computed price target persisted without a decomposition model. Call forecast_record with this forecast_id when the horizon passes to close the Brier loop. Gap decomposition is unavailable (no provider-backed projected model)."
                } else if req.forecast_probability.is_none() {
                    "Persisted without forecast_probability; outcomes remain unscored."
                } else {
                    "Persisted as non-prospective history; outcomes remain unscored."
                },
            })
            )
        })
        .await
    }

    /// Record an equity forecast outcome and close the Brier loop.
    /// Ported from the companies server's `forecast_record` (Brier math
    /// from the shared `hkask_forecast` crate; the provider-backed gap
    /// decomposition dropped with the companies server).
    #[tool(
        description = "Record a forecast outcome to close the superforecasting loop. Forecast a valuation multiple and price change over a horizon (3mo/6mo/1yr/2yr/3yr), then record what actually happened. Computes Brier scores on price return vs a 20% tolerance band. When forecast_id names a prior forecast_persist record, the outcome is recorded on it and its stored forecast_probability is the Brier prior only for a frozen prospective declaration; missing probability and legacy records remain unscored. The 11-line-item gap decomposition from the companies server is unavailable here: its inputs were provider statements. Multiple-direction Brier is reported as null (the multiple probability is not modeled)."
    )]
    pub async fn forecast_record(
        &self,
        Parameters(req): Parameters<EquityForecastRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_record", async {
            validate_symbol(&req.symbol)?;
            validate_horizon(&req.horizon)?;
            for (name, value) in [
                ("forecast_multiple", req.forecast_multiple),
                ("forecast_price_change", req.forecast_price_change),
                ("actual_multiple", req.actual_multiple),
                ("actual_price_change", req.actual_price_change),
            ] {
                validate_finite(name, value)?;
            }

            // Keep validation and journal mutation under one lock so concurrent
            // conflicting resolutions cannot overwrite each other.
            let mut store = self.equity_forecast_store.lock().unwrap_or_else(|e| e.into_inner());
            let mut stored: Option<StoredEquityForecast> = None;
            if let Some(ref forecast_id) = req.forecast_id {
                let found = store.get(forecast_id).ok_or_else(|| {
                    McpToolError::not_found(format!(
                        "forecast '{forecast_id}' not found for this owner"
                    ))
                })?;
                if found.symbol != req.symbol {
                    return Err(McpToolError::invalid_argument(format!(
                        "forecast '{}' belongs to symbol '{}', not '{}'",
                        forecast_id, found.symbol, req.symbol
                    )));
                }
                if found.forecast_date != req.forecast_date || found.horizon != req.horizon
                    || found.forecast_price_change != req.forecast_price_change
                    || found.forecast_multiple.is_some_and(|m| m != req.forecast_multiple)
                {
                    return Err(McpToolError::invalid_argument("resolution differs from frozen forecast date, horizon or target"));
                }
                if let Some(declaration) = &found.declaration {
                    let outcome_date = chrono::NaiveDate::parse_from_str(&req.outcome_date, "%Y-%m-%d")
                        .map_err(|e| McpToolError::invalid_argument(format!("invalid outcome_date: {e}")))?;
                    let today = chrono::Utc::now().date_naive();
                    if outcome_date != declaration.deadline || today < declaration.deadline {
                        return Err(McpToolError::invalid_argument("outcome must resolve at the frozen horizon after it has elapsed"));
                    }
                }
                if let Some(outcome) = &found.outcome {
                    for (key, value) in [("outcome_date", serde_json::json!(req.outcome_date)), ("actual_multiple", serde_json::json!(req.actual_multiple)), ("actual_price_change", serde_json::json!(req.actual_price_change))] {
                        if outcome.get(key) != Some(&value) {
                            return Err(McpToolError::invalid_argument("conflicting outcome replay; corrections require a separately sourced version"));
                        }
                    }
                }
                stored = Some(found.clone());
            }
            let brier_prior = stored.as_ref().filter(|s| s.declaration.is_some()).and_then(|s| s.forecast_probability);

            // Price change: was actual return within 20% tolerance of forecast?
            // Same tolerance-band definition the companies server used (its
            // superforecast.rs within_tolerance) — inlined as this port's own
            // helper because that helper dies with the companies crate in
            // Phase C.
            let return_accurate = {
                let forecast = req.forecast_price_change;
                let actual = req.actual_price_change;
                let tolerance = stored.as_ref().and_then(|s| s.declaration.as_ref()).map_or(0.20, |d| d.tolerance);
                if forecast == 0.0 {
                    actual.abs() < tolerance
                } else {
                    ((actual - forecast) / forecast).abs() <= tolerance
                }
            };
            let return_brier = brier_prior.map(|p| hkask_forecast::brier_score(p, return_accurate));
            let combined = return_brier;

            let multiple_gap = req.actual_multiple - req.forecast_multiple;
            let return_gap = req.actual_price_change - req.forecast_price_change;
            let gap_narrative = if multiple_gap.abs() > 2.0 && return_gap.abs() > 0.05 {
                "multiple_and_return_diverged"
            } else if multiple_gap.abs() > 2.0 {
                "multiple_drove_gap"
            } else if return_gap.abs() > 0.05 {
                "return_drove_gap"
            } else {
                "forecast_accurate"
            };

            // Record the outcome on the stored forecast (journal append).
            if let (Some(ref forecast_id), Some(mut record)) =
                (req.forecast_id.as_ref(), stored.clone())
                && record.outcome.is_none()
            {
                record.outcome = Some(serde_json::json!({
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
                    "recorded_at": now_rfc3339_string(),
                }));
                store
                    .insert(forecast_id.to_string(), record)
                    .map_err(|error| {
                        McpToolError::internal(format!(
                            "equity forecast journal write failed: {error}"
                        ))
                    })?;
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
                    "sample_count": usize::from(combined.is_some()),
                    "weighting": "one_per_declared_equity_forecast_version",
                },
                "decomposition": "unavailable — the projected-model decomposition required provider statements, which the companies server owned",
                "score_convention": "binary_mean_squared_error",
                "resolver": stored.as_ref().and_then(|s| s.declaration.as_ref()).map(|d| &d.resolver),
                "baseline": null,
                "baseline_note": "No matched reference declared; raw loss does not certify calibration.",
            });
            output["scoring_status"] = serde_json::json!(if combined.is_some() { "scored" } else { "unscored" });
            if combined.is_none() {
                output["unscored_reason"] = serde_json::json!("no prospectively stored probability");
            }
            if let Some(ref fid) = req.forecast_id {
                output["forecast_id"] = serde_json::Value::String(fid.clone());
            }

            Ok(output)
        })
        .await
    }

    /// List persisted equity forecasts for a symbol.
    #[tool(
        description = "List durable equity forecasts for a symbol belonging to the authenticated owner — every prior forecast_persist record for the symbol, pending and resolved."
    )]
    pub async fn forecast_list(
        &self,
        Parameters(req): Parameters<EquityForecastListRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "forecast_list", async {
            validate_symbol(&req.symbol)?;
            let store = self
                .equity_forecast_store
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let forecasts: Vec<serde_json::Value> = store
                .values()
                .filter(|record| record.symbol == req.symbol)
                .map(|record| serde_json::to_value(record).unwrap_or(serde_json::Value::Null))
                .collect();
            Ok(serde_json::json!({
                "symbol": req.symbol,
                "forecasts": forecasts,
            }))
        })
        .await
    }

    /// Scenario risk measure over caller-supplied branches — the N2
    /// wrapper. The shared `hkask_forecast` risk functions, exposed as a
    /// tool with caller-supplied branch returns.
    #[tool(
        description = "Compute the scenario risk measure over scenario branches: probability-weighted expected return and scenario sigma over the branches, plus optional APT-style factor loadings per named node. Branch returns are caller-supplied (revalued per branch by the caller — the DCF engine that once computed them per branch is retired with the companies server). When realized_volatility is supplied, also emits fused_volatility — the root-sum-square fusion of realized and scenario sigma, weighted by the tree's probability mass coverage. A risk measure over zero probability mass is never fabricated — the tool errors naming it. Factor loadings over a node with zero mass on either conditioning set are reported as null, never estimated."
    )]
    pub async fn scenario_risk_measure(
        &self,
        Parameters(req): Parameters<ScenarioRiskRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_risk_measure", async {
            if req.branches.is_empty() {
                return Err(McpToolError::invalid_argument(
                    "branches must be non-empty — a risk measure over zero \
                     branches is undefined",
                ));
            }
            for (index, branch) in req.branches.iter().enumerate() {
                validate_finite(
                    &format!("branches[{index}].probability"),
                    branch.probability,
                )?;
                validate_finite(
                    &format!("branches[{index}].branch_return"),
                    branch.branch_return,
                )?;
                if branch.probability < 0.0 {
                    return Err(McpToolError::invalid_argument(format!(
                        "branches[{index}].probability must be non-negative, got {}",
                        branch.probability
                    )));
                }
            }
            if let Some(sigma) = req.realized_volatility {
                validate_finite("realized_volatility", sigma)?;
                if sigma < 0.0 {
                    return Err(McpToolError::invalid_argument(format!(
                        "realized_volatility must be non-negative, got {sigma}"
                    )));
                }
            }

            let branches: Vec<hkask_forecast::BranchOutcome> = req
                .branches
                .iter()
                .map(|branch| hkask_forecast::BranchOutcome {
                    probability: branch.probability,
                    branch_return: branch.branch_return,
                })
                .collect();
            let measure = hkask_forecast::scenario_risk_measure(&branches).ok_or_else(|| {
                McpToolError::invalid_argument(
                    "scenario risk measure is undefined — the branches carry \
                         zero total probability mass; a risk measure over no mass \
                         is never fabricated",
                )
            })?;

            let fused = req.realized_volatility.map(|realized| {
                let weight = req.scenario_weight.unwrap_or(1.0);
                hkask_forecast::fuse_volatility(realized, Some(measure.sigma_scenario), weight)
            });

            Ok(serde_json::json!({
                "status": "measured",
                "risk_measure": {
                    "expected_return": measure.expected_return,
                    "sigma_scenario": measure.sigma_scenario,
                    "branch_count": measure.branch_count,
                    "probability_mass": measure.probability_mass,
                },
                "fused_volatility": fused,
            }))
        })
        .await
    }
}
