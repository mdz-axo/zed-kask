#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
// `tokio` is in [dependencies] for the bin target's `#[tokio::main]`; the lib
// itself does not use it, so the unused_crate_dependencies lint fires on the
// lib target. This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! hKask MCP Scenarios — domain-agnostic scenario planning server.
//!
//! Event-tree forecasting with conditional dependencies (MAIA methodology).
//! The Schwartz 2x2 axis-driven mode lives in `hkask-mcp-companies`
//! (see `docs/architecture/scenarios-companies-bridge.md` for the connection path).
//!
//! Shared engine: Fermi decomposition, outside/inside view, Bayesian updating,
//! Brier scoring, dragonfly-eye synthesis, calibration tracking, cross-validation.
//!
//! ## Tools (21) — pinned by `tool_surface_is_exactly_21_registered_tools`
//! - `scenario_status` — Server state: pipeline overview, calibration curve, cached tree
//! - `scenario_frame_document` — Structure framing answers into FramingDocument
//! - `scenario_frame` — 7-turn conversational framing interview
//! - `scenario_triage` — Goldilocks zone classification
//! - `scenario_research` — Extract candidate events from web research
//! - `scenario_brainstorm` — 4-round temperature-shifting protocol
//! - `scenario_build` — Construct event tree template from research
//! - `scenario_quantify` — Resolve conditional probability tree
//! - `scenario_propagate` — Update one event's prior and propagate through descendants
//! - `scenario_calibrate` — Fermi decomposition + outside/inside view
//! - `scenario_update` — Bayesian evidence revision
//! - `scenario_synthesize` — Dragonfly-eye multi-perspective aggregation
//! - `scenario_sensitivity` — Variance contribution ranking
//! - `scenario_score` — Brier scoring + forecast store + auto-update
//! - `scenario_calibration` — Calibration curve + overconfidence detection
//! - `scenario_cross_validate` — LLM vs computation cross-validation
//! - `scenario_assess` — Chermack five-phase project evaluation
//! - `scenario_full` — Full pipeline orchestrator (single call)
//! - `scenario_from_markets` — Bridge from prediction-markets MCP server (single record)
//! - `scenario_from_markets_set` — Bridge from prediction-markets (multi-record EventTree)
//! - `scenario_from_cmp_indices` — Bridge from prediction-markets CMP indices (EventTree)

use std::collections::HashSet;

use hkask_bridge_ontology::dc_bibo;
use hkask_mcp_server::server::{McpToolError, execute_tool_semantic};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

pub mod superforecast;
pub mod templates;
pub mod types;

use types::*;

// ── Error classification ───────────────────────────────────────────────────

/// Classify a `ScenarioError` into the appropriate `McpToolError` kind.
///
/// Caller-input defects (bad probabilities, cycles, unknown parents, empty
/// input) map to `InvalidArgument`; missing entities map to `NotFound`;
/// empty-store maps to `Internal`. `Forecast` computation errors are
/// classified per variant (all current `ForecastError` variants are
/// caller-input defects).
fn map_scenario_error(error: ScenarioError) -> McpToolError {
    match &error {
        ScenarioError::EventNotFound(id) => {
            McpToolError::not_found(format!("event '{id}' not found"))
        }
        ScenarioError::NoForecastData => {
            McpToolError::internal("no stored forecasts found for calibration") // rr0044-ok: mapper-internal-arm
        }
        ScenarioError::Forecast(forecast_error) => match forecast_error {
            hkask_forecast::ForecastError::InvalidProbability(..)
            | hkask_forecast::ForecastError::BrierLengthMismatch(..)
            | hkask_forecast::ForecastError::BrierNoData
            | hkask_forecast::ForecastError::TreeMissingNode(..)
            | hkask_forecast::ForecastError::TreeMissingOutcome(..)
            | hkask_forecast::ForecastError::TreeUndefinedNode(..)
            | hkask_forecast::ForecastError::TreeUnresolvedParent(..)
            | hkask_forecast::ForecastError::TreeConditionalLength(..) => {
                McpToolError::invalid_argument(format!(
                    "forecast computation failed: {forecast_error}",
                ))
            }
        },
        // Caller-side input defects
        ScenarioError::NoEvents
        | ScenarioError::InvalidProbability(..)
        | ScenarioError::InvalidDependencyProbability(..)
        | ScenarioError::InvalidDependency(..)
        | ScenarioError::UnknownParent(..)
        | ScenarioError::CycleDetected
        | ScenarioError::InsufficientPerspectives
        | ScenarioError::EmptyInput(..) => McpToolError::invalid_argument(error.to_string()),
    }
}

mod requests;
pub use requests::*;

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct ScenariosServer {
        pub forecast_store: std::sync::Arc<std::sync::Mutex<superforecast::ForecastStore>>,
        pub tree_cache: std::sync::Mutex<Option<types::EventTree>>,
        pub called_tools: std::sync::Mutex<HashSet<String>>,
    }
);

impl ScenariosServer {
    /// Map a tool name to its ontology concept URI. The concept URI is used
    /// both as the `reg.tool.*` span ontology tag (via `execute_tool_semantic`)
    /// and as the `"ontology"` field in the tool output JSON.
    ///
    /// PKO = Procedural Knowledge Ontology (process/experience — agent's actions)
    /// Dublin Core = factual/computed outputs (probabilities, scores, trees)
    fn ontology_anchor(tool: &str) -> Option<&'static str> {
        use hkask_bridge_ontology::pko;
        match tool {
            "scenario_frame" | "scenario_brainstorm" | "scenario_build" => Some(pko::PROCEDURE),
            _ => Some(dc_bibo::DATASET),
        }
    }

    /// Expected predecessor for each pipeline-stage tool.
    /// Returns None for tools that can be called independently.
    ///
    /// The chain encodes the analyst maturity ladder: frame → brainstorm →
    /// build → quantify → calibrate → synthesize → score → calibration.
    /// The tree-based tools (`scenario_from_markets_set`, `scenario_propagate`)
    /// sit at the *top* of this ladder — an analyst earns the full event tree
    /// by working through the simpler modes first (2x2 matrix, single-market
    /// bridges, research-grounded framing). They have no predecessor enforced
    /// here (they are entry points for already-mature analyses), but the
    /// maturity note in their descriptions points at the ladder.
    fn expected_predecessor(tool: &str) -> Option<&'static str> {
        match tool {
            "scenario_frame_document" => Some("scenario_frame"),
            "scenario_brainstorm" => Some("scenario_frame_document"),
            "scenario_build" => Some("scenario_brainstorm"),
            "scenario_quantify" => Some("scenario_build"),
            "scenario_calibrate" => Some("scenario_quantify"),
            "scenario_synthesize" => Some("scenario_calibrate"),
            "scenario_score" => Some("scenario_quantify"),
            "scenario_calibration" => Some("scenario_score"),
            "scenario_assess" => Some("scenario_synthesize"),
            _ => None, // triage, research, update, sensitivity, cross_validate, full, from_companies
        }
    }

    /// Validate tool call sequence and emit Regulation warnings on violations.
    ///
    /// Tracks which tools have been called on this server instance. When a
    /// pipeline-stage tool is invoked without its expected predecessor having
    /// been called, emits a Regulation warning. Does not block execution — tool
    /// flexibility is preserved for exploratory and bypass workflows.
    ///
    /// In-memory only: tracking resets when the server process restarts.
    fn check_sequence(&self, tool: &str) {
        let mut called = self.called_tools.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(expected) = Self::expected_predecessor(tool)
            && !called.contains(expected)
        {
            tracing::warn!(
                target: "hkask.mcp.scenarios.sequence",
                tool = %tool,
                expected_predecessor = %expected,
                "Pipeline sequence violation: {} called without prior {}",
                tool, expected
            );
        }

        called.insert(tool.to_string());
    }

    /// Check pipeline sequence for this tool and emit a Regulation warning if
    /// the expected predecessor was not called.
    ///
    /// This is the pipeline-sequence validation hook — it tracks which tools
    /// have been called on this server instance and warns when a pipeline-stage
    /// tool is invoked out of order. It does NOT record or persist anything.
    fn record_experience(&self, tool: &str) {
        self.check_sequence(tool);
    }
}

// ── Tool router ────────────────────────────────────────────────────────────

impl ScenariosServer {
    fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::scenario_router()
    }
}

// ── MCP Tools ──────────────────────────────────────────────────────────────

fn build_status_response(
    total: usize,
    resolved_count: usize,
    pending_count: usize,
    overall_brier: Option<f64>,
    calibration: Option<serde_json::Value>,
    recent: Vec<serde_json::Value>,
    tree: Option<serde_json::Value>,
) -> serde_json::Value {
    let provenance_args = serde_json::Value::Object(serde_json::Map::new());
    let provenance_span_id = serde_json::Value::Null;
    serde_json::json!({
        "pipeline": {
            "forecast_count": total,
            "resolved_count": resolved_count,
            "pending_count": pending_count,
            "overall_brier": overall_brier,
            "recent_forecasts": recent
        },
        "calibration": calibration,
        "event_tree": tree,
        "provenance": {
            "tool": "scenario_status",
            "server": "hkask-mcp-scenarios",
            "args": provenance_args,
            "span_id": provenance_span_id,
            "version": SERVER_VERSION
        },
        "ontology": dc_bibo::DATASET
    })
}

#[tool_router(router = scenario_router, vis = "pub")]
impl ScenariosServer {
    /// Return the current state snapshot for TUI display.
    #[tool(
        description = "Return current scenario server state: pipeline overview, calibration curve, and cached event tree."
    )]
    pub async fn scenario_status(&self, _parameters: Parameters<StatusRequest>) -> String {
        execute_tool_semantic(self, "scenario_status", Self::ontology_anchor("scenario_status"), async {
            let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
            let total = store.len();
            let resolved: Vec<_> = store.resolved();
            let resolved_count = resolved.len();
            let pending_count = total - resolved_count;

            let overall_brier = if !resolved.is_empty() {
                // A "resolved" forecast should carry an outcome. `None` would
                // be treated as `false` (event did not occur), inflating the
                // Brier score. Filter to forecasts with a known outcome and
                // warn on any `None` so the data-integrity issue is visible.
                let scored: Vec<_> = resolved
                    .iter()
                    .filter(|r| {
                        if r.outcome.is_none() {
                            tracing::warn!(
                                target: "hkask.mcp.scenarios",
                                forecast_id = r.forecast_id,
                                event_id = r.event_id,
                                "resolved forecast has no outcome — excluding from Brier"
                            );
                            false
                        } else {
                            true
                        }
                    })
                    .collect();
                if scored.is_empty() {
                    None
                } else {
                    let probs: Vec<f64> = scored.iter().map(|r| r.probability).collect();
                    let outs: Vec<bool> =
                        scored.iter().map(|r| r.outcome.unwrap_or(false)).collect();
                    superforecast::brier_score_multi(&probs, &outs).ok()
                }
            } else {
                None
            };

            let calibration = superforecast::compute_calibration_curve(&store).ok().map(|c| serde_json::json!({
                "total_forecasts": c.total_forecasts,
                "resolved_forecasts": c.resolved_forecasts,
                "overall_brier": c.overall_brier,
                "overconfidence_score": c.overconfidence_score,
                "interpretation": c.interpretation
            }));

            let recent: Vec<_> = store.values().take(20).map(|r| serde_json::json!({
                "forecast_id": r.forecast_id,
                "event_id": r.event_id,
                "event_name": r.event_name,
                "subject": r.subject,
                "probability": r.probability,
                "created_at": r.created_at.to_string(),
                "outcome": r.outcome,
            })).collect();

            let tree = self.tree_cache.lock().unwrap_or_else(|e| e.into_inner()).clone().map(|t| serde_json::json!({
                "subject": t.subject,
                "time_horizon": serde_json::to_value(t.time_horizon).unwrap_or_default(),
                "event_count": t.nodes.len(),
                "joint_probability": t.joint_probability,
                "root_ids": t.root_ids,
                "nodes": t.nodes.iter().map(|n| serde_json::json!({
                    "id": n.event.id,
                    "name": n.event.name,
                    "question": n.event.question,
                    "probability": n.event.probability,
                    "marginal_probability": n.marginal_probability,
                    "certainty_tier": serde_json::to_value(n.event.certainty_tier()).unwrap_or_default(),
                    "basis": n.event.basis,
                    "parent_ids": n.event.depends_on.iter().flat_map(|d| d.parent_event_ids.clone()).collect::<Vec<_>>(),
                    "children": [],
                    "sub_question_count": n.event.sub_questions.len(),
                    "has_base_rate": n.event.base_rate.is_some(),
                    "brier_score": n.event.brier_score
                })).collect::<Vec<_>>()
            }));

            let output = build_status_response(
                total,
                resolved_count,
                pending_count,
                overall_brier,
                calibration,
                recent,
                tree,
            );

            self.record_experience("scenario_status");
            Ok(output)
        }).await
    }

    /// Run the full scenario pipeline in one call. Delegates computation to
    /// the superforecast engine: triage_question, build_event_tree,
    /// sensitivity_ranking, calibrate_from_fermi, outside_view_adjustment,
    /// synthesize_perspectives, assess_project — same functions called by
    /// individual tools. The pipeline assembles their outputs into one envelope.
    #[tool(description = "Run the complete scenario pipeline in a single call.")]
    pub async fn scenario_full(&self, Parameters(req): Parameters<FullPipelineRequest>) -> String {
        execute_tool_semantic(self, "scenario_full", Self::ontology_anchor("scenario_full"), async {
            let events = &req.events;
            // The pipeline anchors synthesis on the first event, so an empty
            // array is refused up front rather than indexed later. `build_event_tree`
            // also rejects empty input, but the synthesis step at step 5 is
            // reachable independently of it.
            let Some(first_event_id) = events.first().map(|e| e.id.clone()) else {
                return Err(McpToolError::invalid_argument(
                    "events must contain at least one scenario event",
                ));
            };

            // Step 1: Triage
            let triage_results: Vec<_> = events.iter().map(|e| {
                let t = superforecast::triage_question(&e.question, true, e.reference_class.is_some(), true);
                serde_json::json!({"event_id": e.id, "difficulty": t.difficulty, "is_forecastable": t.is_forecastable})
            }).collect();

            // Step 2: Quantify
            let tree = superforecast::build_event_tree(events)
                .map_err(map_scenario_error)?;

            // Step 3: Sensitivity
            let sensitivity = superforecast::sensitivity_ranking(&tree);

            // Step 4: Calibrate
            let calibration: Vec<_> = events.iter().map(|e| {
                let fermi = match superforecast::calibrate_from_fermi(&e.sub_questions) {
                    Ok(fermi_estimate) => fermi_estimate,
                    Err(error) => {
                        tracing::warn!(
                            target: "hkask.mcp.scenarios",
                            %error,
                            "Fermi calibration failed, defaulting to 0.5"
                        );
                        0.5
                    }
                };
                let (cal, conf) = if let (Some(br), Some(_)) = (e.base_rate, e.reference_class.as_ref()) {
                    superforecast::outside_view_adjustment(br, fermi, 1)
                } else { (fermi, 0.5) };
                serde_json::json!({"event_id": e.id, "calibrated": cal, "confidence": conf})
            }).collect();

            // Step 5: Synthesize (if perspectives provided)
            let synth = req.perspectives.as_deref().and_then(|perspectives| {
                if perspectives.len() < 2 {
                    tracing::warn!(
                        target: "hkask.mcp.scenarios",
                        "scenario_full: fewer than 2 perspectives provided; skipping synthesis"
                    );
                    None
                } else {
                    match superforecast::synthesize_perspectives(&first_event_id, perspectives) {
                        Ok(synthesis) => Some(synthesis),
                        Err(error) => {
                            tracing::warn!(
                                target: "hkask.mcp.scenarios",
                                %error,
                                "scenario_full: perspective synthesis failed; skipping"
                            );
                            None
                        }
                    }
                }
            });

            // Step 6: Assess
            let deps = events.iter().filter(|e| !e.depends_on.is_empty()).count();
            let learning: Vec<String> = req.learning_events.as_deref()
                .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
                .unwrap_or_default();
            let curve = { let s = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner()); superforecast::compute_calibration_curve(&s).ok() };
            let assessment = superforecast::assess_project(&types::AssessInput {
                project_id: &req.subject,
                subject: &req.subject,
                perspective_count: req.perspective_count.unwrap_or(1),
                disagreement_score: synth.as_ref().map(|s| s.disagreement_score).unwrap_or(0.0),
                event_count: events.len(),
                events_with_deps: deps,
                calibration_curve: curve.as_ref(),
                strategies_generated: req.strategies_generated.unwrap_or(0),
                strategies_implemented: req.strategies_implemented.unwrap_or(0),
                learning_events: learning,
                has_early_warning_indicators: req.has_early_warning_indicators.unwrap_or(false),
            });

            let output = serde_json::json!({
                "subject": req.subject, "pipeline": "full", "event_count": events.len(),
                "triage": triage_results,
                "quantify": {"joint_probability": tree.joint_probability},
                "sensitivity": sensitivity.iter().map(|(id, s)| serde_json::json!({"event_id": id, "score": s})).collect::<Vec<_>>(),
                "calibration": calibration,
                "synthesis": synth.map(|s| serde_json::json!({"aggregated": s.aggregated_probability, "disagreement": s.disagreement_score})),
                "assessment": {"overall": assessment.overall_score, "recommendations": assessment.recommendations},
                "provenance": {
                    "tool": "scenario_full",
                    "server": "hkask-mcp-scenarios",
                    "version": SERVER_VERSION,
                    "pipeline_steps": ["triage", "quantify", "sensitivity", "calibrate", "synthesize", "assess"],
                    "delegates_to": ["triage_question", "build_event_tree", "sensitivity_ranking", "calibrate_from_fermi", "outside_view_adjustment", "synthesize_perspectives", "assess_project"]
                },
                "ontology": dc_bibo::DATASET
            });

            self.record_experience("scenario_full");
            Ok(output)
        })
        .await
    }

    /// Bridge: convert a prediction-market record into a scenario event.
    /// Caller-mediated: the agent pastes the annotated MarketRecord JSON
    /// from hkask-mcp-prediction-markets. The domain-bias correction is
    /// applied deterministically here; low reliability or weak match
    /// confidence withholds the base rate.
    #[tool(
        description = "Convert a prediction-market record (from hkask-mcp-prediction-markets market_lookup/market_match) into a ScenarioEvent anchored on the market-implied base rate. Applies the domain-bias correction deterministically; withholds base_rate on low reliability or weak match confidence."
    )]
    pub async fn scenario_from_markets(
        &self,
        Parameters(req): Parameters<MarketsBridgeRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_from_markets", Self::ontology_anchor("scenario_from_markets"), async {
            let record: hkask_mcp_prediction_markets::types::MarketRecord =
                serde_json::from_value(req.market_record.into())
                    .map_err(|e| McpToolError::invalid_argument(format!("invalid market record JSON: {e}")))?;

            let (event, warnings) = {
                let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
                superforecast::convert_market_record(
                    &record,
                    req.match_confidence.as_deref(),
                    Some(&store),
                )
            }
            .map_err(map_scenario_error)?;

            let output = serde_json::json!({
                "event": {
                    "id": event.id,
                    "name": event.name,
                    "question": event.question,
                    "deadline": event.deadline.to_string(),
                    "probability": event.probability,
                    "base_rate": event.base_rate,
                    "basis": event.basis,
                    "reference_class": event.reference_class,
                },
                "warnings": warnings,
                "provenance": {
                    "tool": "scenario_from_markets",
                    "server": "hkask-mcp-scenarios",
                    "version": SERVER_VERSION,
                    "source": format!("{:?}:{}", record.source, record.market_id),
                    "ontology_identifier": record.ontology.state.identifier,
                },
                "bridge_note": "The prediction-markets server supplies annotated market-implied probabilities; this bridge anchors a trackable ScenarioEvent on them. base_rate is None when the reliability/match gates refuse the anchor — do not substitute the raw price.",
                "ontology": dc_bibo::DATASET
            });

            Ok(output)
        })
        .await
    }

    /// Bridge: compose a SET of prediction-market records into a dependent
    /// event tree (T4a). Per-record gates from `scenario_from_markets` apply;
    /// dependency edges are caller-authored (the server computes marginals
    /// but never invents conditional probabilities).
    ///
    /// MATURITY NOTE: this is the detailed end of the scenario-modeling
    /// ladder. The simple path — companies `scenario_analysis` (Schwartz 2x2)
    /// → `scenario_impact_valuation` on hkask-mcp-companies → single-market
    /// `scenario_from_markets` — is the intended on-ramp; an analyst typically
    /// arrives at a full tree only after research (company, industry, economy,
    /// technology, management, domain experts) reveals which events actually
    /// condition each other. The 2x2 mode is retained as a first-class citizen,
    /// not a legacy path.
    #[tool(
        description = "Compose a set of prediction-market records (from hkask-mcp-prediction-markets) into a validated EventTree with caller-authored dependency edges. Each record passes the scenario_from_markets gates; question-overlap duplicates are flagged; cycles and CPT-size violations are rejected. Returns the resolved tree (marginals, joint probability) plus composition warnings. Detailed-mode tool: the simpler 2x2 path (companies scenario_analysis → scenario_impact_valuation on hkask-mcp-companies) is the intended on-ramp before wiring full trees."
    )]
    pub async fn scenario_from_markets_set(
        &self,
        Parameters(req): Parameters<MarketsSetBridgeRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_from_markets_set", Self::ontology_anchor("scenario_from_markets_set"), async {
            let records: Vec<hkask_mcp_prediction_markets::types::MarketRecord> =
                serde_json::from_value(req.market_records.into())
                    .map_err(|e| McpToolError::invalid_argument(format!("invalid market records JSON array: {e}")))?;

            let confidences = req.match_confidences.unwrap_or_else(|| vec![None; records.len()]);
            let specs: Vec<superforecast::DependencySpec> = req
                .dependency_specs
                .unwrap_or_default()
                .into_iter()
                .map(|s| superforecast::DependencySpec {
                    child_market_id: s.child_market_id,
                    parent_market_ids: s.parent_market_ids,
                    conditionals: s.conditionals,
                })
                .collect();

            let (tree, warnings) = {
                let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
                superforecast::compose_market_tree(&records, &confidences, &specs, Some(&store))
            }
            .map_err(map_scenario_error)?;

            let nodes: Vec<serde_json::Value> = tree
                .nodes
                .iter()
                .map(|n| serde_json::json!({
                    "id": n.event.id,
                    "question": n.event.question,
                    "marginal_probability": n.marginal_probability,
                    "depends_on": n.event.depends_on.iter().map(|d| serde_json::json!({
                        "parent_event_ids": d.parent_event_ids,
                        "conditionals": d.conditionals,
                    })).collect::<Vec<_>>(),
                    "base_rate": n.event.base_rate,
                    "variance_contribution": n.variance_contribution,
                }))
                .collect();

            let output = serde_json::json!({
                "tree": {
                    "subject": tree.subject,
                    "root_ids": tree.root_ids,
                    "topo_order": tree.topo_order,
                    "joint_probability": tree.joint_probability,
                    "nodes": nodes,
                },
                "warnings": warnings,
                "provenance": {
                    "tool": "scenario_from_markets_set",
                    "server": "hkask-mcp-scenarios",
                    "version": SERVER_VERSION,
                    "market_count": records.len(),
                    "dependency_edge_count": specs.len(),
                },
                "bridge_note": "Dependency edges and their conditionals are caller-authored; the server validates structure and computes marginals/joints but never invents conditional probabilities. base_rate is None on records refused by the per-market gates.",
                "ontology": dc_bibo::DATASET
            });

            Ok(output)
        })
        .await
    }

    /// Bridge: compose CMP indices into an EventTree (R1).
    ///
    /// Takes CMP index probabilities (from hkask-mcp-prediction-markets
    /// build_cmp_indices) and composes them into a validated EventTree. Each
    /// CMP index becomes a root ScenarioEvent with its index probability as the
    /// prior. The tree cites the index (family, orientation, tenor, venue) in
    /// the provenance — not a decaying contract.
    ///
    /// Optional dependency edges between CMP indices (e.g. "oil price increase
    /// → inflation increase") enable the H3 joint coherence test.
    #[tool(
        description = "Compose CMP (Constant-Maturity Prediction) indices into a validated EventTree. Each CMP index becomes a root event with its index probability as the prior. Optional dependency edges between CMP indices (e.g. oil→inflation) enable joint probability computation for coherence testing. The tree cites the CMP index identity (family, tenor, orientation, venue) in the provenance — not a decaying contract. Input: an array of ProvenancedCmpIndex objects (from build_cmp_indices), observation date, optional dependency specs."
    )]
    pub async fn scenario_from_cmp_indices(
        &self,
        Parameters(req): Parameters<CmpBridgeRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_from_cmp_indices", Self::ontology_anchor("scenario_from_cmp_indices"), async {
            let indices: Vec<hkask_mcp_prediction_markets::cmp_index_builder::ProvenancedCmpIndex> =
                serde_json::from_value(req.cmp_indices.into())
                    .map_err(|e| McpToolError::invalid_argument(format!("invalid cmp_indices JSON array: {e}")))?;

            let observation_date = chrono::NaiveDate::parse_from_str(&req.observation_date, "%Y-%m-%d")
                .map_err(|e| McpToolError::invalid_argument(format!(
                    "invalid observation_date '{}': {e} — expected YYYY-MM-DD",
                    req.observation_date
                )))?;

            let deps: Vec<superforecast::CmpDependencySpec> = req
                .dependency_specs
                .unwrap_or_default()
                .into_iter()
                .map(|s| superforecast::CmpDependencySpec {
                    child_id: s.child_id,
                    parent_ids: s.parent_ids,
                    conditionals: s.conditionals,
                })
                .collect();

            let tree = if deps.is_empty() {
                superforecast::compose_cmp_tree(&indices, observation_date)
            } else {
                superforecast::compose_cmp_tree_with_deps(&indices, observation_date, &deps)
            }
            .map_err(map_scenario_error)?;

            let nodes: Vec<serde_json::Value> = tree
                .nodes
                .iter()
                .map(|n| serde_json::json!({
                    "id": n.event.id,
                    "question": n.event.question,
                    "marginal_probability": n.marginal_probability,
                    "depends_on": n.event.depends_on.iter().map(|d| serde_json::json!({
                        "parent_event_ids": d.parent_event_ids,
                        "conditionals": d.conditionals,
                    })).collect::<Vec<_>>(),
                    "base_rate": n.event.base_rate,
                    "basis": n.event.basis,
                    "variance_contribution": n.variance_contribution,
                }))
                .collect();

            // Extract CMP provenance from the event IDs (cmp:{family}:{tenor}:{orientation}).
            let cmp_provenance: Vec<serde_json::Value> = tree
                .nodes
                .iter()
                .filter_map(|n| {
                    let id = &n.event.id;
                    if !id.starts_with("cmp:") {
                        return None;
                    }
                    Some(serde_json::json!({
                        "id": id,
                        "basis": n.event.basis,
                        "reference_class": n.event.reference_class,
                    }))
                })
                .collect();

            let output = serde_json::json!({
                "tree": {
                    "subject": tree.subject,
                    "root_ids": tree.root_ids,
                    "topo_order": tree.topo_order,
                    "joint_probability": tree.joint_probability,
                    "nodes": nodes,
                },
                "cmp_provenance": cmp_provenance,
                "provenance": {
                    "tool": "scenario_from_cmp_indices",
                    "server": "hkask-mcp-scenarios",
                    "version": SERVER_VERSION,
                    "cmp_index_count": indices.len(),
                    "dependency_edge_count": deps.len(),
                    "observation_date": req.observation_date,
                },
                "bridge_note": "CMP indices are constant-maturity, constant-orientation synthetic portfolios. The tree cites the index identity, not a decaying contract. Dependency edges and their conditionals are caller-authored; the server validates structure and computes marginals/joints but never invents conditional probabilities.",
                "ontology": dc_bibo::DATASET
            });

            Ok(output)
        })
        .await
    }

    /// Cross-validate two probability estimates to close the learning loop.
    #[tool(
        description = "Cross-validate two probability estimates for the same event — typically an LLM-generated estimate from the superforecasting skill against a server-computed estimate from scenario_calibrate. Computes per-sub-question divergence to identify where estimates differ most. Flags for review when overall divergence exceeds threshold (default 0.15). Returns CrossValidation with divergence, per-sub-question breakdown, and recommendation. Closes the learning loop: if divergence > 0.15, activate grill-me skill to interrogate assumptions."
    )]
    pub async fn scenario_cross_validate(
        &self,
        Parameters(req): Parameters<CrossValidateRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_cross_validate", Self::ontology_anchor("scenario_cross_validate"), async {
            let validation = superforecast::cross_validate(
                &req.event_id,
                &req.source_a, req.estimate_a, &req.sub_questions_a,
                &req.source_b, req.estimate_b, &req.sub_questions_b,
                req.review_threshold,
            );

            let top_divergences: Vec<_> = {
                let mut sorted = validation.sub_question_divergences.clone();
                sorted.sort_by(|a, b| b.divergence.partial_cmp(&a.divergence).unwrap_or(std::cmp::Ordering::Equal));
                sorted.into_iter().take(3).map(|d| {
                    serde_json::json!({
                        "question": d.question,
                        "divergence": d.divergence,
                        "estimate_a": d.estimate_a,
                        "estimate_b": d.estimate_b,
                    })
                }).collect()
            };

            let output = serde_json::json!({
                "event_id": validation.event_id,
                "source_a": {"name": validation.source_a, "estimate": validation.estimate_a, "pct": format!("{:.1}%", validation.estimate_a * 100.0)},
                "source_b": {"name": validation.source_b, "estimate": validation.estimate_b, "pct": format!("{:.1}%", validation.estimate_b * 100.0)},
                "divergence": validation.divergence,
                "divergence_pct": format!("{:.1}%", validation.divergence * 100.0),
                "review_threshold": validation.review_threshold,
                "requires_review": validation.requires_review,
                "status": if validation.requires_review { "review_required" } else { "consistent" },
                "top_divergences": top_divergences,
                "sub_question_count": validation.sub_question_divergences.len(),
                "recommendation": validation.recommendation,
                "grill_me_questions": validation.grill_me_questions,
                "next_action": if validation.requires_review {
                    serde_json::json!({
                        "skill": "grill-me",
                        "reason": format!("Divergence {:.1}% exceeds threshold {:.1}%", validation.divergence * 100.0, validation.review_threshold * 100.0),
                        "questions": validation.grill_me_questions,
                        "after_grill": ["scenario_calibrate", "scenario_cross_validate"]
                    })
                } else {
                    serde_json::json!({"skill": null, "next": "scenario_synthesize"})
                },
                "next_steps": if validation.requires_review {
                    "1. Activate grill-me skill to interrogate assumptions. 2. Revisit Fermi decomposition on the most divergent sub-questions. 3. Re-run scenario_calibrate with revised sub-questions. 4. Re-cross-validate."
                } else {
                    "1. Feed the aggregated estimate into scenario_synthesize for dragonfly-eye integration. 2. Use scenario_quantify for downstream computation. 3. Track via scenario_score."
                },
                "methodology": {
                    "ontology": dc_bibo::DATASET,
                    "framework": "Cross-validation between LLM reasoning (superforecasting skill) and computational verification (scenarios server)",
                    "threshold_rationale": "0.15 divergence threshold based on Tetlock's incremental belief updating (Commandment 4): superforecasters typically move probabilities in 0.05-0.10 increments. A 0.15 divergence suggests fundamentally different assumptions, not just calibration noise.",
                    "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 5-6"
                }
            });

            self.record_experience("scenario_cross_validate");
            Ok(output)
        })
        .await
    }

    /// Generate a conversational framing protocol to scope a scenario project.
    #[tool(
        description = "Start a conversational framing session for a scenario project. Generates a 7-turn conversational protocol with natural openings (not numbered questions) designed using behavioral psychology and improv coaching postures. Turns: (1) What is on your mind? (foot-in-the-door), (2) What decision hangs on this? (curiosity gap), (3) When do you need to act? (temporal anchoring), (4) What is NOT on the table? (loss aversion — exclusions first), (5) Who would say I told you so? (social proof + contrarian), (6) What does good enough look like? (peak-end begins), (7) What assumptions could break everything? (peak-end closes). Each turn has improv mode guidance (Plussing, Yes And, Yes But, Coaching) and behavioral psychology notes. The agent acts as a coach, not an interviewer. Run this FIRST — before scenario_brainstorm."
    )]
    pub async fn scenario_frame(&self, Parameters(req): Parameters<FrameRequest>) -> String {
        execute_tool_semantic(self, "scenario_frame", Self::ontology_anchor("scenario_frame"), async {
            let protocol = templates::generate_framing_session(&req.subject);

            // If prior answers were provided, merge them into the template
            let prior: Option<serde_json::Value> = req.prior_answers.map(|v| v.into());

            let mut output = protocol;
            if let Some(ref prior) = prior
                && let Some(obj) = output.as_object_mut() {
                    obj.insert("prior_answers".to_string(), prior.clone());
                    obj.insert(
                        "note".to_string(),
                        serde_json::json!("Prior answers from a previous framing session. Use these to skip turns already covered. If a prior answer seems stale or incomplete, revisit that turn naturally. Don't reference the prior answer directly — just ask the turn's opening question again."),
                    );
                }
            // Emit the ontology anchor at the top level (consistent with
            // scenario_status and scenario_build). scenario_frame classifies
            // as "pko" (it's a procedural coaching protocol).
            if let Some(obj) = output.as_object_mut() {
                obj.insert(
                    "ontology".to_string(),
                    serde_json::json!(Self::ontology_anchor("scenario_frame").unwrap_or(dc_bibo::DATASET)),
                );
            }

            self.record_experience("scenario_frame");
            Ok(output)
        })
        .await
    }

    /// Structure completed framing conversation answers into a FramingDocument.
    #[tool(
        description = "Structure completed framing conversation answers into a typed FramingDocument. Accepts the subject and an object with answers from the 7-turn conversational protocol (from scenario_frame). Produces a validated FramingDocument with: focal_question, decision_at_stake, time_horizon, action_deadline, in_scope, out_of_scope, stakeholders (as personas for brainstorming), use_case, success_criteria, constraints, surfaced_assumptions, and exploration_prompts. The output feeds directly into scenario_brainstorm as the frame. Run AFTER the framing conversation is complete and BEFORE scenario_brainstorm."
    )]
    pub async fn scenario_frame_document(
        &self,
        Parameters(req): Parameters<FrameDocumentRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_frame_document", Self::ontology_anchor("scenario_frame_document"), async {
            let answers: serde_json::Value = req.answers.into();

            let document = superforecast::structure_framing_document(&req.subject, &answers)
                .map_err(map_scenario_error)?;

            let output = serde_json::json!({
                "subject": req.subject,
                "framing_document": {
                    "focal_question": document.focal_question,
                    "decision_at_stake": document.decision_at_stake,
                    "time_horizon": serde_json::to_value(document.time_horizon).unwrap_or_default(),
                    "action_deadline": document.action_deadline,
                    "in_scope": document.in_scope,
                    "out_of_scope": document.out_of_scope,
                    "stakeholders": document.stakeholders.iter().map(|s| serde_json::json!({
                        "role": s.role,
                        "primary_concern": s.primary_concern,
                        "likely_blind_spots": s.likely_blind_spots,
                        "include_as_persona": s.include_as_persona
                    })).collect::<Vec<_>>(),
                    "use_case": serde_json::to_value(document.use_case).unwrap_or_default(),
                    "success_criteria": document.success_criteria,
                    "constraints": document.constraints,
                    "surfaced_assumptions": document.surfaced_assumptions,
                    "exploration_prompts": document.exploration_prompts
                },
                "next_step": "Feed this framing document into scenario_brainstorm. The stakeholders become personas. The exploration_prompts guide the divergent phase. The scope boundaries from in_scope/out_of_scope keep the tree focused.",
                "pipeline": [
                    "scenario_frame → conversational framing",
                    "scenario_frame_document → structure answers (this tool)",
                    "scenario_brainstorm → multi-persona protocol",
                    "scenario_quantify → resolve conditional probability tree",
                    "scenario_calibrate → Fermi decomposition + outside view",
                    "scenario_synthesize → dragonfly-eye aggregation",
                    "scenario_assess → Chermack project evaluation"
                ]
            });

            self.record_experience("scenario_frame_document");
            Ok(output)
        })
        .await
    }

    /// Generate a multi-round brainstorming protocol for collaborative scenario construction.
    #[tool(
        description = "Generate a structured brainstorming protocol for collaborative scenario construction. Produces a 4-round protocol: (1) DIVERGE — high-temperature ideation with multiple personas (Bull, Bear, Contrarian, Systems Thinker), (2) GROUND — anchor candidates in verified facts and base rates, (3) LINK — build causal dependency chains, (4) PRUNE — converge to final tree. Each round has temperature guidance, quality gates, and output format specifications. The agent (LLM) follows this protocol round-by-round with the user. Use before scenario_quantify to generate events collaboratively."
    )]
    pub async fn scenario_brainstorm(
        &self,
        Parameters(req): Parameters<BrainstormRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_brainstorm", Self::ontology_anchor("scenario_brainstorm"), async {
            let horizon = parse_time_horizon(req.time_horizon.as_deref());
            let research = req.research_context.as_deref().unwrap_or("No research context provided. Use scenario_research to gather web search results first, or provide context manually.");

            let persona_names: Vec<String> = req
                .personas
                .as_deref()
                .map(|s| {
                    s.split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect()
                })
                .unwrap_or_default();

            let start_round = req.start_round.unwrap_or(1).clamp(1, 4);

            let protocol = templates::generate_brainstorm_protocol(
                &req.subject,
                horizon.display(),
                research,
                &persona_names,
            );

            // Filter rounds based on start_round
            let active_rounds: Vec<&BrainstormRound> = protocol
                .rounds
                .iter()
                .filter(|r| r.round >= start_round)
                .collect();

            let output = serde_json::json!({
                "subject": protocol.subject,
                "time_horizon": protocol.time_horizon,
                "protocol_type": "Multi-Round Collaborative Brainstorming",
                "total_rounds": active_rounds.len(),
                "starting_round": start_round,
                "personas": protocol.personas.iter().map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "lens": p.lens,
                        "prompt": p.prompt,
                    })
                }).collect::<Vec<_>>(),
                "rounds": active_rounds.iter().map(|r| {
                    serde_json::json!({
                        "round": r.round,
                        "name": r.name,
                        "mode": r.mode,
                        "temperature_guidance": r.temperature_guidance,
                        "output_type": r.output_type,
                        "instructions": r.instructions,
                        "quality_gate": r.quality_gate,
                    })
                }).collect::<Vec<_>>(),
                "pipeline_after_brainstorm": protocol.pipeline,
                "how_to_use": {
                    "step_1": "Run this tool to get the protocol with round-by-round instructions and persona prompts.",
                    "step_2": "For each round, follow the instructions. The agent generates events; the user reviews and refines.",
                    "step_3": format!(
                        "Round 1 (DIVERGE): Use high-temperature thinking. Each persona ({} ) generates 3-5 candidate events. User reviews, adds, removes, or merges.",
                        protocol.personas.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                    "step_4": "Round 2 (GROUND): Attach verified facts, base rates, and reference classes to each event. Discard ungrounded events.",
                    "step_5": "Round 3 (LINK): Build causal chains. Add dependency relationships with conditional probabilities.",
                    "step_6": "Round 4 (PRUNE): Merge overlaps, remove isolates, calibrate final probabilities.",
                    "step_7": "After all rounds: send the final JSON array of ScenarioEvents to scenario_quantify for conditional probability resolution.",
                    "step_8": "Use scenario_calibrate for Fermi decomposition per event, scenario_synthesize for multi-analyst aggregation, and scenario_assess for project evaluation."
                },
                "methodology": {
                    "ontology": dc_bibo::DATASET,
                    "framework": "Cognitive Process Model for Scenario Construction",
                    "divergent_phase": "High-temperature ideation from multiple personas (Schwartz: imagination + Chermack: stakeholder diversity)",
                    "grounding_phase": "Evidence anchoring and base rate calibration (Tetlock Commandments 2-3: Fermi-ize + outside view)",
                    "linking_phase": "Causal chain construction (Chermack Phase 3: internal consistency)",
                    "convergent_phase": "Pruning and calibration (Tetlock Commandment 1: triage + Chermack Phase 5: assessment)",
                    "temperature_shift": "The protocol shifts cognitive temperature from divergent (creative, unfiltered) to convergent (analytical, rigorous). This mirrors the dual-process theory of cognition (Kahneman System 1 → System 2) applied to scenario construction."
                },
                "references": {
                    "schwartz_1991": "The Art of the Long View — scenario narratives and driving forces",
                    "tetlock_2015": "Superforecasting — Fermi decomposition, base rates, Bayesian updating, Brier scoring",
                    "chermack_2011": "Scenario Planning in Organizations — five-phase performance system, stakeholder diversity, project assessment",
                    "kahneman_2011": "Thinking, Fast and Slow — System 1 (divergent) / System 2 (convergent) cognitive modes"
                }
            });

            self.record_experience("scenario_brainstorm");
            Ok(output)
        })
        .await
    }
    /// Returns a scenario event-tree scaffold/template for LLM completion.
    /// Always returns an extraction template (event schema, dependency format,
    /// certainty tiers, Tetlock commandments); the agent fills it against
    /// research_text if provided. Does not itself parse research into final events.
    #[tool(
        description = "Build a scenario event tree scaffold from web research. Returns an extraction template (not final events) with: event schema, dependency format, certainty tier definitions, and Tetlock's 10 commandments as methodology. The agent (LLM) fills in the event_extraction_prompt against research_text to produce ScenarioEvent objects. Without research_text, returns a structural template. The ultimate pipeline artifact: events with calibrated probabilities, conditional dependency chains, and connections to driver/decision factors from the framing document. Feeds into scenario_quantify for probability resolution."
    )]
    pub async fn scenario_build(&self, Parameters(req): Parameters<BuildEventsRequest>) -> String {
        execute_tool_semantic(self, "scenario_build", Self::ontology_anchor("scenario_build"), async {
            let horizon = parse_time_horizon(req.time_horizon.as_deref());
            let scenario_type = parse_scenario_type(req.scenario_type.as_deref());
            let max_events = req.max_events.unwrap_or(6);
            let context_str = req.context.as_deref().unwrap_or("");

            let deadline_hint = match horizon {
                TimeHorizon::Tactical => "within 12-18 months",
                TimeHorizon::Strategic => "within 3-5 years",
                TimeHorizon::LongTerm => "within 7-10 years",
            };

            let output = serde_json::json!({
                "subject": req.subject,
                "time_horizon": horizon.display(),
                "time_horizon_key": serde_json::to_value(horizon).unwrap_or_default(),
                "scenario_type": serde_json::to_value(scenario_type).unwrap_or_default(),
                "max_events": max_events,
                "research_context": context_str,
                "event_extraction_prompt": format!(
                    "Based on the research context above about '{}', extract up to {} key future events as binomial yes/no questions with deadlines {}. Each event should be:\n\
                     1. A specific yes/no question with a clear deadline\n\
                     2. Placed in a dependency tree (what must happen first?)\n\
                     3. Assigned a probability tier: proximate (>67%), probable (33-66%), or possible (<33%)\n\
                     4. Anchored to either technical_feasibility or scaling_distribution as the basis\n\
                     \n\
                     Format as a JSON array of ScenarioEvent objects with:\n\
                     - id: unique short identifier (e.g. 'evt-1')\n\
                     - name: short descriptive name\n\
                     - question: yes/no framed question with deadline\n\
                     - deadline: YYYY-MM-DD\n\
                     - time_horizon: 'tactical', 'strategic', or 'long_term'
\
                     - scenario_type: 'company_update', 'company_analysis', 'emerging_economic', or 'economic_potential'
\
                     - subject: the subject under analysis
\
                     - probability: 0.0-1.0 estimate
                     - basis: 'technical_feasibility' or 'scaling_distribution'\n\
                     - depends_on: [] or a single-entry array with parent_event_ids (list of parent event IDs) and conditionals (bitmap-ordered conditional probabilities, length = 2^num_parents)\n\
                     - sub_questions: 2-4 Fermi decomposition questions
\
                     - base_rate, reference_class, and brier_score: null when unavailable
\
                     - update_count: 0 for a new event
                     \n\
                     Send the completed JSON array to scenario_quantify for probability resolution.",
                    req.subject, max_events, deadline_hint
                ),
                "event_template": {
                    "id": "evt-N",
                    "name": "Short descriptive name",
                    "question": "Yes/no question with specific date/deadline",
                    "deadline": "YYYY-MM-DD",
                    "time_horizon": horizon.display(),
                    "scenario_type": serde_json::to_value(scenario_type).unwrap_or_default(),
                    "subject": req.subject,
                    "probability": 0.5,
                    "basis": "technical_feasibility or scaling_distribution",
                    "depends_on": [],
                    "sub_questions": [
                        {"question": "What enabling factor must be in place?", "estimate": 0.5, "confidence": 0.5},
                        {"question": "What competitive response is likely?", "estimate": 0.5, "confidence": 0.5},
                        {"question": "What macro condition supports/undermines this?", "estimate": 0.5, "confidence": 0.5}
                    ],
                    "base_rate": null,
                    "reference_class": null,
                    "brier_score": null,
                    "update_count": 0
                },
                "dependency_guidance": {
                    "description": "Events can depend on other events. Use depends_on to link events into a conditional probability tree. Common patterns: regulatory approval → product launch → revenue impact; technology breakthrough → cost reduction → market share shift.",
                    "format": {
                        "parent_event_ids": ["id of parent event"],
                        "conditionals": [0.3, 0.7]
                    }
                },
                "certainty_tiers": {
                    "proximate": {"range": "67-100%", "description": "Already started to happen, could stop"},
                    "probable": {"range": "33-66%", "description": "All elements exist for it to happen"},
                    "possible": {"range": "0-32%", "description": "Could happen but unlikely"}
                },
                "methodology": {
                    "ontology": Self::ontology_anchor("scenario_build").unwrap_or(dc_bibo::DATASET),
                    "framework": "MAIA event-based scenario planning (Tetlock Superforecasting + Schwartz imagination)",
                    "research_pipeline": "1. Web search (brave/firecrawl/tavily) → 2. scenario_build (this tool) → 3. scenario_quantify (resolve tree) → 4. scenario_calibrate (Fermi probabilities)",
                    "tetlock_commandments": [
                        "1. Triage: focus on Goldilocks-zone questions",
                        "2. Fermi-ize: break into tractable sub-questions",
                        "3. Balance inside and outside views",
                        "4. Incremental belief updating",
                        "5. Dragonfly-eye: synthesize multiple perspectives",
                        "6. Distinguish degrees of doubt (use full 0-100% scale)",
                        "7. Balance under/overconfidence",
                        "8. Postmortem successes and failures",
                        "9. Bring out best in others",
                        "10. Master error-balancing"
                    ],
                    "reference": "Tetlock & Gardner, Superforecasting (2015); Schwartz, The Art of the Long View (1991)"
                },
                "ontology": Self::ontology_anchor("scenario_build").unwrap_or(dc_bibo::DATASET)
            });

            self.record_experience("scenario_build");
            Ok(output)
        })
        .await
    }

    /// Extract candidate events from raw web research text.
    #[tool(
        description = "Extract candidate scenario events from raw web research text. Provide research_text (raw output from web searches about a subject) and this tool returns structured event suggestions with dependency hints. Each candidate event includes: suggested name, yes/no question framing, deadline suggestion, dependency hints, and Fermi sub-question scaffolding. The output is a draft that needs probability assignment and refinement, then feeds into scenario_quantify. Use this after web searching (brave_web_search, firecrawl_search, tavily_search) and before scenario_quantify."
    )]
    pub async fn scenario_research(&self, Parameters(req): Parameters<ResearchRequest>) -> String {
        execute_tool_semantic(self, "scenario_research", Self::ontology_anchor("scenario_research"), async {
            let horizon = parse_time_horizon(req.time_horizon.as_deref());
            let scenario_type = parse_scenario_type(req.scenario_type.as_deref());
            let max_events = req.max_events.unwrap_or(6);

            // Analyze research text for structural clues
            let text_lower = req.research_text.to_lowercase();
            let word_count = req.research_text.split_whitespace().count();

            // Heuristic: detect themes in the research
            let has_regulatory = text_lower.contains("regulation") || text_lower.contains("approval") || text_lower.contains("fda") || text_lower.contains("ban");
            let has_competition = text_lower.contains("competitor") || text_lower.contains("rival") || text_lower.contains("market share");
            let has_technology = text_lower.contains("launch") || text_lower.contains("release") || text_lower.contains("chip") || text_lower.contains("model") || text_lower.contains("platform");
            let has_financial = text_lower.contains("revenue") || text_lower.contains("earnings") || text_lower.contains("margin") || text_lower.contains("growth");
            let has_macro = text_lower.contains("rate") || text_lower.contains("inflation") || text_lower.contains("recession") || text_lower.contains("fed") || text_lower.contains("gdp");
            let has_supply_chain = text_lower.contains("supply") || text_lower.contains("shortage") || text_lower.contains("capacity") || text_lower.contains("manufacturing");

            let deadline_hint = match horizon {
                TimeHorizon::Tactical => "YYYY-MM-DD within 12-18 months from now",
                TimeHorizon::Strategic => "YYYY-MM-DD within 3-5 years from now",
                TimeHorizon::LongTerm => "YYYY-MM-DD within 7-10 years from now",
            };

            let mut theme_hints = Vec::new();
            if has_regulatory { theme_hints.push("regulatory_risk"); }
            if has_competition { theme_hints.push("competitive_dynamics"); }
            if has_technology { theme_hints.push("technology_evolution"); }
            if has_financial { theme_hints.push("financial_performance"); }
            if has_macro { theme_hints.push("macro_economic"); }
            if has_supply_chain { theme_hints.push("supply_chain"); }

            let output = serde_json::json!({
                "subject": req.subject,
                "time_horizon": horizon.display(),
                "scenario_type": serde_json::to_value(scenario_type).unwrap_or_default(),
                "research_stats": {
                    "word_count": word_count,
                    "detected_themes": theme_hints,
                },
                "event_extraction_prompt": format!(
                    "You are a superforecaster extracting scenario events from research about '{}'.\n\n\
                     RESEARCH TEXT:\n{}\n\n\
                     INSTRUCTIONS:\n\
                     Extract up to {} key future events as binomial yes/no questions. Each event must:\n\
                     1. Have a specific deadline ({})\n\
                     2. Be framed as a clear yes/no question\n\
                     3. Include dependency relationships (what must happen first?)\n\
                     4. Include an initial probability estimate\n\
                     5. Include 2-4 Fermi decomposition sub-questions\n\
                     6. Tag the basis as 'technical_feasibility' or 'scaling_distribution'\n\n\
                     Detected themes in the research: {}\n\n\
                     Return ONLY a JSON array of ScenarioEvent objects with these fields:\n\
                     id, name, question, deadline, time_horizon, scenario_type, subject, probability (0.0-1.0), basis, depends_on (array with parent_event_ids and conditionals fields), sub_questions (array of question/estimate/confidence objects), base_rate, reference_class, brier_score, update_count

\
                     Use null for unavailable base_rate, reference_class, and brier_score; use 0 for update_count.
                     The output will be sent to scenario_quantify for conditional probability resolution.",
                    req.subject, req.research_text, max_events, deadline_hint, theme_hints.join(", ")
                ),
                "detected_themes": theme_hints.iter().map(|t| {
                    match *t {
                        "regulatory_risk" => serde_json::json!({"theme": "Regulatory risk", "event_hint": "Will regulatory approval/restriction occur by [deadline]?"}),
                        "competitive_dynamics" => serde_json::json!({"theme": "Competitive dynamics", "event_hint": "Will competitor X launch/exit/gain share by [deadline]?"}),
                        "technology_evolution" => serde_json::json!({"theme": "Technology evolution", "event_hint": "Will technology Y reach milestone Z by [deadline]?"}),
                        "financial_performance" => serde_json::json!({"theme": "Financial performance", "event_hint": "Will revenue/margin/cash flow reach target T by [deadline]?"}),
                        "macro_economic" => serde_json::json!({"theme": "Macro-economic", "event_hint": "Will macro condition M change to state S by [deadline]?"}),
                        "supply_chain" => serde_json::json!({"theme": "Supply chain", "event_hint": "Will supply/capacity constraint C be resolved by [deadline]?"}),
                        _ => serde_json::json!({"theme": t, "event_hint": "Frame as specific yes/no question with deadline"})
                    }
                }).collect::<Vec<_>>(),
                "event_template": {
                    "id": "evt-N",
                    "name": "Short descriptive name",
                    "question": "Yes/no question with specific date/deadline",
                    "deadline": deadline_hint,
                    "time_horizon": horizon.display(),
                    "scenario_type": serde_json::to_value(scenario_type).unwrap_or_default(),
                    "subject": req.subject,
                    "probability": 0.5,
                    "basis": "technical_feasibility or scaling_distribution",
                    "depends_on": [],
                    "sub_questions": [
                        {"question": "What enabling factor must be in place?", "estimate": 0.5, "confidence": 0.5},
                        {"question": "What is the base rate for events of this type?", "estimate": 0.5, "confidence": 0.5},
                        {"question": "What specific evidence would confirm this is happening?", "estimate": 0.5, "confidence": 0.5}
                    ],
                    "base_rate": null,
                    "reference_class": null,
                    "brier_score": null,
                    "update_count": 0
                },
                "pipeline": {
                    "step_1": "Use this prompt with an LLM to generate the JSON array of ScenarioEvent objects",
                    "step_2": "Send the generated JSON to scenario_quantify to resolve conditional probabilities",
                    "step_3": "Use scenario_calibrate for Fermi decomposition and an outside-view base-rate blend for each event",
                    "step_4": "Use scenario_update to Bayesian-update as new evidence arrives",
                    "step_5": "Use scenario_score to Brier-score outcomes and close the calibration loop"
                },
                "methodology": {
                    "ontology": dc_bibo::DATASET,
                    "framework": "MAIA event-based scenario planning — research → events → tree → calibrate → track",
                    "reference": "Tetlock & Gardner, Superforecasting (2015) — Commandments 1-4"
                }
            });

            self.record_experience("scenario_research");
            Ok(output)
        })
        .await
    }

    /// Quantify an event tree: compute marginal probabilities, joint probability,
    /// and build the full resolved tree with sensitivity rankings.
    #[tool(
        description = "Quantify a scenario event tree. Takes an array of ScenarioEvent objects with conditional dependencies and computes: (1) topological sort of dependency graph, (2) marginal probabilities for each event via conditional probability propagation, (3) joint probability of the full event chain, (4) variance contribution per event (sensitivity proxy), (5) sensitivity ranking. Detects cycles and missing parent references. Returns the full resolved EventTree. To render this tree inline for the user, emit a fenced `graph` code block whose body is a JSON object: {\"viz\":\"event_tree\", \"subject\":..., \"joint_probability\":..., \"nodes\":[{\"id\",\"name\",\"question\",\"marginal_probability\",\"certainty_tier\",\"depends_on\":[{\"parent_event_ids\":[...],\"conditionals\":[...]}]}]} — the conversation view renders it as an interactive event-tree DAG (pan/zoom; click a node to set evidence and re-propagate). Pass through the fields this tool returns; keep `depends_on[].conditionals` intact so the DAG can re-propagate when evidence is set."
    )]
    pub async fn scenario_quantify(&self, Parameters(req): Parameters<QuantifyRequest>) -> String {
        execute_tool_semantic(self, "scenario_quantify", Self::ontology_anchor("scenario_quantify"), async {
            let tree = superforecast::build_event_tree(&req.events)
                .map_err(map_scenario_error)?;

            // Cache for TUI status queries
            *self.tree_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(tree.clone());

            let sensitivity = superforecast::sensitivity_ranking(&tree);
            let most_uncertain = sensitivity.first().map(|(id, _)| format!("Event '{}' contributes the most uncertainty", id));
            let most_certain = sensitivity.last().map(|(id, _)| format!("Event '{}' contributes the least uncertainty", id));

            let output = serde_json::json!({
                "subject": tree.subject,
                "time_horizon": serde_json::to_value(tree.time_horizon).unwrap_or_default(),
                "scenario_type": serde_json::to_value(tree.scenario_type).unwrap_or_default(),
                "event_count": tree.nodes.len(),
                "root_events": tree.root_ids,
                "topological_order": tree.topo_order,
                "joint_probability": tree.joint_probability,
                "joint_probability_pct": format!("{:.1}%", tree.joint_probability * 100.0),
                "nodes": tree.nodes.iter().map(|n| serde_json::json!({
                    "id": n.event.id,
                    "name": n.event.name,
                    "question": n.event.question,
                    "deadline": n.event.deadline.to_string(),
                    "marginal_probability": n.marginal_probability,
                    "probability_pct": format!("{:.1}%", n.marginal_probability * 100.0),
                    "certainty_tier": serde_json::to_value(n.event.certainty_tier()).unwrap_or_default(),
                    "variance_contribution": n.variance_contribution,
                    "depends_on": n.event.depends_on.iter().map(|d| serde_json::json!({
                        "parent_event_ids": d.parent_event_ids,
                        "conditionals": d.conditionals,
                    })).collect::<Vec<_>>(),
                    "paths_from_root": n.paths,
                })).collect::<Vec<_>>(),
                "sensitivity_ranking": sensitivity.into_iter().map(|(id, score)| {
                    serde_json::json!({"event_id": id, "uncertainty_score": score})
                }).collect::<Vec<_>>(),
                "interpretation": {
                    "joint_probability": format!(
                        "The probability that ALL events occur as forecast is {:.1}%",
                        tree.joint_probability * 100.0
                    ),
                    "most_uncertain": most_uncertain,
                    "most_certain": most_certain,
                },
                "framework": "Conditional probability tree. Each node's marginal is computed via full joint-table marginalization under parent independence: P(E) = Sum_a P(E|a) * Product_i P(p_i)^{a_i} * (1-P(p_i))^{1-a_i}. Root nodes use their intrinsic probability. Joint = product of all-nodes-occur conditionals."
            });

            self.record_experience("scenario_quantify");
            Ok(output)
        })
        .await
    }

    /// Tree-level Bayesian propagation (T5): update one event's prior and
    /// recompute every descendant marginal and the joint. The propagation
    /// journal is the tâtonnement record (Bhattacharya Prop. 6).
    #[tool(
        description = "Update one event's prior probability and propagate through the event tree: all descendant marginals and the joint probability are recomputed. Returns the updated tree plus a propagation journal (per-node before/after deltas) — the tâtonnement record of the update round. CPTs are untouched; only the named event's prior changes."
    )]
    pub async fn scenario_propagate(
        &self,
        Parameters(req): Parameters<PropagateRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_propagate", Self::ontology_anchor("scenario_propagate"), async {
            let result = superforecast::propagate_prior_update(&req.events, &req.event_id, req.new_prior)
                .map_err(map_scenario_error)?;

            let nodes: Vec<serde_json::Value> = result
                .tree
                .nodes
                .iter()
                .map(|n| serde_json::json!({
                    "id": n.event.id,
                    "marginal_probability": n.marginal_probability,
                    "update_count": n.event.update_count,
                }))
                .collect();

            let output = serde_json::json!({
                "tree": {
                    "subject": result.tree.subject,
                    "topo_order": result.tree.topo_order,
                    "joint_probability": result.tree.joint_probability,
                    "nodes": nodes,
                },
                "journal": result.journal,
                "joint_before": result.joint_before,
                "joint_after": result.joint_after,
                "provenance": {
                    "tool": "scenario_propagate",
                    "server": "hkask-mcp-scenarios",
                    "version": SERVER_VERSION,
                    "updated_event": req.event_id,
                    "changed_nodes": result.journal.len(),
                },
                "framework": "Tree-level Bayesian propagation: the named event's prior is revised; every descendant marginal is recomputed via CPT marginalization under parent independence; the journal records each changed node (one tâtonnement round, Bhattacharya Prop. 6, arXiv:2211.03244).",
                "ontology": dc_bibo::DATASET
            });

            Ok(output)
        })
        .await
    }

    /// Bayesian update: revise a probability with new evidence.
    #[tool(
        description = "Bayesian update for a scenario event. Apply Bayes' theorem: P(H|E) = P(E|H) × P(H) / P(E). Provide prior probability, evidence likelihood (how likely is the evidence if the hypothesis is true?), and evidence base rate (how common is this evidence in general?). Returns the posterior probability and the magnitude of the update."
    )]
    pub async fn scenario_update(&self, Parameters(req): Parameters<UpdateRequest>) -> String {
        execute_tool_semantic(self, "scenario_update", Self::ontology_anchor("scenario_update"), async {
            if !(0.0..=1.0).contains(&req.prior_probability) {
                return Err(McpToolError::invalid_argument("prior_probability must be in [0, 1]"));
            }
            if !(0.0..=1.0).contains(&req.evidence_likelihood) {
                return Err(McpToolError::invalid_argument("evidence_likelihood must be in [0, 1]"));
            }
            if req.evidence_base_rate <= 0.0 || req.evidence_base_rate > 1.0 {
                return Err(McpToolError::invalid_argument("evidence_base_rate must be in (0, 1]"));
            }

            let posterior = superforecast::bayesian_update(
                req.prior_probability,
                req.evidence_likelihood,
                req.evidence_base_rate,
            );

            let delta = posterior - req.prior_probability;
            let direction = if delta > 0.0 { "increased" } else if delta < 0.0 { "decreased" } else { "unchanged" };

            let output = serde_json::json!({
                "forecast_id": req.forecast_id,
                "event_id": req.event_id,
                "prior_probability": req.prior_probability,
                "prior_pct": format!("{:.1}%", req.prior_probability * 100.0),
                "evidence_likelihood": req.evidence_likelihood,
                "evidence_base_rate": req.evidence_base_rate,
                "posterior_probability": posterior,
                "posterior_pct": format!("{:.1}%", posterior * 100.0),
                "delta": delta,
                "delta_pct": format!("{:+.1}%", delta * 100.0),
                "direction": direction,
                "magnitude": if delta.abs() < 0.05 { "small" } else if delta.abs() < 0.15 { "moderate" } else { "large" },
                "evidence_description": req.evidence_description,
                "formula": "P(H|E) = P(E|H) × P(H) / P(E)",
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 5"
            });

            self.record_experience("scenario_update");
            Ok(output)
        })
        .await
    }

    /// Score a forecast against known outcomes using Brier scoring.
    #[tool(
        description = "Score a scenario forecast against known outcomes using Brier scoring. Takes an array of ScenarioEvent objects and an array of outcomes (each with event_id and occurred boolean). Computes Brier score per event and aggregate. Provides human-readable interpretation: excellent (<0.05), good (<0.10), fair (<0.20), poor (<0.33), worse_than_climatology (≥0.33). Calibration tracking closes the superforecasting loop."
    )]
    pub async fn scenario_score(&self, Parameters(req): Parameters<ScoreRequest>) -> String {
        execute_tool_semantic(self, "scenario_score", Self::ontology_anchor("scenario_score"), async {
            let events = &req.events;
            let outcome_pairs: Vec<(String, bool)> = req.outcomes
                .into_iter()
                .map(|o| (o.event_id, o.occurred))
                .collect();

            let forecast_date = chrono::Utc::now().date_naive();
            let result = superforecast::score_forecast(
                &req.forecast_id,
                events,
                &outcome_pairs,
                forecast_date,
            );

            let per_event: Vec<_> = result.event_outcomes.iter().map(|(eid, occurred)| {
                let event = events.iter().find(|e| &e.id == eid);
                let prob = event.map(|e| e.probability).unwrap_or(0.0);
                let bs = superforecast::brier_score(prob, *occurred);
                serde_json::json!({
                    "event_id": eid,
                    "forecast_probability": prob,
                    "forecast_pct": format!("{:.1}%", prob * 100.0),
                    "outcome": occurred,
                    "brier_score": bs,
                    "interpretation": superforecast::brier_interpretation(bs),
                })
            }).collect();

            let output = serde_json::json!({
                "forecast_id": result.forecast_id,
                "subject": result.subject,
                "forecast_date": result.forecast_date.to_string(),
                "outcome_date": result.outcome_date.to_string(),
                "event_count": result.event_outcomes.len(),
                "per_event": per_event,
                "aggregate": {
                    "brier_score": result.brier_score,
                    "interpretation": result.brier_interpretation,
                },
                "calibration_note": if result.brier_score < 0.10 {
                    "Well calibrated — keep tracking. Your forecasts are meaningfully better than climatology."
                } else if result.brier_score < 0.20 {
                    "Moderately calibrated — review your Fermi decompositions and base rates. There is room for improvement."
                } else {
                    "Poorly calibrated — your forecasts are not beating a coin flip. Revisit your outside-view base rates and inside-view adjustments."
                },
                "auto_update_suggestions": superforecast::auto_update_suggestions(events, &outcome_pairs),
                "update_guidance": "The auto_update_suggestions above show suggested probability adjustments based on forecast error direction. Apply them via scenario_update to close the feedback loop. Each adjustment is clamped to ±15% and respects [0.01, 0.99] bounds.",
                "reference": "Brier (1950). Score = (p - o)² where p = forecast probability, o = outcome (1 if occurred, 0 if not). Lower is better."
            });

            // Store forecasts and resolve outcomes for calibration tracking (P2).
            // Each new record is inserted via `insert` (which appends to the
            // journal — durable). Outcome updates on existing records are also
            // done via `insert` (not `get_mut`) so the outcome is durably
            // journaled, not just in-memory. `persist()` at the end is a
            // belt-and-suspenders full snapshot.
            {
                let mut store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
                let now = chrono::Utc::now().date_naive();
                for event in events {
                    let key = format!("{}:{}", req.forecast_id, event.id);
                    let event_outcome = outcome_pairs.iter().find(|(eid, _)| eid == &event.id);
                    if store.get(&key).is_none() {
                        store.insert(key.clone(), types::StoredForecastRecord {
                            schema_version: 2,
                            forecast_id: req.forecast_id.clone(),
                            event_id: event.id.clone(),
                            event_name: event.name.clone(),
                            subject: event.subject.clone(),
                            probability: event.probability,
                            created_at: now,
                            outcome: None,
                            resolved_at: None,
                            category: Some(event.scenario_type.as_str().to_string()),
                        });
                    }
                    if let Some((_, occurred)) = event_outcome {
                        // Re-insert the record with the outcome set — this
                        // appends to the journal (durable), unlike `get_mut`
                        // which only mutates in-memory.
                        if let Some(mut record) = store.get(&key).cloned() {
                            record.outcome = Some(*occurred);
                            record.resolved_at = Some(now);
                            store.insert(key.clone(), record);
                        }
                    }
                }
                store.persist();
            }

            self.record_experience("scenario_score");
            Ok(output)
        })
        .await
    }

    /// Calibrate a forecast using Fermi decomposition and outside/inside view.
    #[tool(
        description = "Calibrate a forecast probability using Tetlock's methodology. Four-stage: (1) Fermi decomposition — confidence-weighted average of sub-question estimates, (2) Outside view — blend with base rate from reference class using shrinkage estimator, (3) Inside view — adjust with case-specific evidence, (4) Calibration feedback — when ≥5 resolved forecasts exist in the store, apply the learned overconfidence bias to adjust the estimate (closes the Brier learning loop). Returns calibrated probability, calibration-adjusted probability, confidence bounds, and certainty tier."
    )]
    pub async fn scenario_calibrate(
        &self,
        Parameters(req): Parameters<CalibrateRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_calibrate", Self::ontology_anchor("scenario_calibrate"), async {
            if req.sub_questions.is_empty() {
                return Err(McpToolError::invalid_argument("at least one sub_question is required"));
            }

            // Stage 1: Fermi decomposition
            let fermi_estimate = superforecast::calibrate_from_fermi(&req.sub_questions)
                .map_err(map_scenario_error)?;

            // Stage 2+3: Outside/inside view blending (if base rate provided)
            let (calibrated, confidence) = if let (Some(base_rate), Some(_ref_class), Some(ref_count)) =
                (req.base_rate, req.reference_class.as_ref(), req.reference_count)
            {
                let (cal, conf) = superforecast::outside_view_adjustment(
                    base_rate,
                    fermi_estimate,
                    ref_count,
                );
                (cal, conf)
            } else {
                // No base rate: use Fermi estimate directly, lower confidence
                (fermi_estimate, 0.5)
            };

            // Close the Brier feedback loop: if enough historical resolved
            // forecasts exist, apply the learned calibration bias to adjust
            // this estimate (overconfident forecasters regress toward 0.5).
            let (calibration_adjusted, overconfidence_bias) = {
                let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
                match superforecast::compute_calibration_curve(&store) {
                    Ok(curve) if curve.resolved_forecasts >= 5 => {
                        let bias = curve.overconfidence_score;
                        (hkask_forecast::apply_calibration_adjustment(calibrated, bias), Some(bias))
                    }
                    _ => (calibrated, None),
                }
            };

            let output = serde_json::json!({
                "question": req.question,
                "fermi_estimate": fermi_estimate,
                "fermi_pct": format!("{:.1}%", fermi_estimate * 100.0),
                "reference_class": req.reference_class,
                "base_rate": req.base_rate,
                "reference_count": req.reference_count,
                "calibrated_probability": calibrated,
                "calibrated_pct": format!("{:.1}%", calibrated * 100.0),
                "confidence": confidence,
                "confidence_pct": format!("{:.1}%", confidence * 100.0),
                "certainty_tier": serde_json::to_value(CertaintyTier::from_probability(calibrated)).unwrap_or_default(),
                "calibration_adjusted_probability": calibration_adjusted,
                "calibration_adjusted_pct": format!("{:.1}%", calibration_adjusted * 100.0),
                "overconfidence_bias": overconfidence_bias,
                "feedback_loop": if overconfidence_bias.is_some() {
                    "Closed: historical Brier-scored outcomes adjusted this estimate. Positive bias (overconfident) regresses toward 0.5; negative (underconfident) pushes outward."
                } else {
                    "Open: fewer than 5 resolved forecasts — no calibration adjustment applied yet. Record outcomes via scenario_score to close the loop."
                },
                "sub_questions": req.sub_questions.iter().map(|sq| serde_json::json!({
                    "question": sq.question,
                    "estimate": sq.estimate,
                    "confidence": sq.confidence,
                })).collect::<Vec<_>>(),
                "interpretation": if confidence >= 0.7 {
                    "High-confidence estimate — strong reference class and/or consistent sub-questions."
                } else if confidence >= 0.5 {
                    "Moderate confidence — consider adding more Fermi sub-questions or finding a better reference class."
                } else {
                    "Low confidence — the estimate is close to a coin flip. Seek additional data."
                },
                "methodology": {
                    "stage_1": "Fermi decomposition: confidence-weighted average of sub-question estimates",
                    "stage_2": "Outside view: blend base rate with shrinkage estimator (regression toward 0.5 based on reference count)",
                    "stage_3": "Inside view: apply case-specific evidence through the forecasting workflow, then use scenario_update for explicit Bayesian revisions",
                    "stage_4": "Bayesian updating: use scenario_update tool to revise with new evidence",
                    "stage_5": "Calibration feedback: when >=5 resolved forecasts exist, apply the learned overconfidence bias (hkask_forecast::apply_calibration_adjustment) to close the Brier learning loop",
                },
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 4-6"
            });

            self.record_experience("scenario_calibrate");
            Ok(output)
        })
        .await
    }

    /// Sensitivity ranking: which events drive outcome uncertainty.
    #[tool(
        description = "Rank events by their contribution to outcome uncertainty. For each event, computes an uncertainty score (1 - variance contribution; higher = more uncertainty) — events closer to 50/50 contribute more uncertainty. Returns events ranked from most uncertain to most certain. Useful for identifying which events to spend calibration effort on."
    )]
    pub async fn scenario_sensitivity(
        &self,
        Parameters(req): Parameters<SensitivityRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_sensitivity", Self::ontology_anchor("scenario_sensitivity"), async {
            let events = &req.events;
            let tree = superforecast::build_event_tree(events)
                .map_err(map_scenario_error)?;

            let ranking = superforecast::sensitivity_ranking(&tree);

            let output = serde_json::json!({
                "event_count": events.len(),
                "ranking": ranking.iter().enumerate().map(|(i, (id, score))| {
                    let event = events.iter().find(|e| &e.id == id);
                    serde_json::json!({
                        "rank": i + 1,
                        "event_id": id,
                        "event_name": event.map(|e| e.name.as_str()).unwrap_or(""),
                        "probability": event.map(|e| e.probability),
                        "probability_pct": event.map(|e| format!("{:.1}%", e.probability * 100.0)),
                        "uncertainty_score": score,
                        "interpretation": if *score > 0.6 {
                                                    "high uncertainty — calibrate this event carefully"
                                                } else if *score > 0.3 {
                            "moderate uncertainty"
                        } else {
                            "low uncertainty — well-anchored estimate"
                        },
                    })
                }).collect::<Vec<_>>(),
                "guidance": "Focus calibration effort on high-uncertainty events (score > 0.6). These are the events where better Fermi decompositions, base rates, or evidence will most improve forecast accuracy.",
                "methodology": "Variance contribution proxy: |P - 0.5| × 2. Events at 50% contribute maximum uncertainty; events at 0% or 100% contribute none."
            });

            self.record_experience("scenario_sensitivity");
            Ok(output)
        })
        .await
    }

    /// Synthesize multiple perspectives into one aggregated forecast (dragonfly-eye).
    #[tool(
        description = "Dragonfly-eye synthesis (Tetlock Stage 5). Aggregates multiple independent perspectives on a single event into one calibrated probability. Uses empirical-Bayes weighting: perspectives with better historical Brier scores get higher weight. Computes disagreement score (0=consensus, 1=polarized) and identifies the strongest dissenting view. Requires at least 2 perspectives. Returns the aggregated probability, weight distribution, dissent summary, and synthesis quality assessment."
    )]
    pub async fn scenario_synthesize(
        &self,
        Parameters(req): Parameters<SynthesizeRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_synthesize", Self::ontology_anchor("scenario_synthesize"), async {
            let synthesis = superforecast::synthesize_perspectives(&req.event_id, &req.perspectives)
                .map_err(map_scenario_error)?;

            let output = serde_json::json!({
                "event_id": synthesis.event_id,
                "perspective_count": synthesis.perspectives.len(),
                "aggregated_probability": synthesis.aggregated_probability,
                "aggregated_pct": format!("{:.1}%", synthesis.aggregated_probability * 100.0),
                "disagreement_score": synthesis.disagreement_score,
                "disagreement_pct": format!("{:.0}%", synthesis.disagreement_score * 100.0),
                "synthesis_quality": synthesis.synthesis_quality,
                "dissent_summary": synthesis.dissent_summary,
                "perspective_weights": synthesis.perspective_weights.iter().map(|(source, w)| {
                    serde_json::json!({"source": source, "weight": w, "weight_pct": format!("{:.0}%", w * 100.0)})
                }).collect::<Vec<_>>(),
                "individual_perspectives": synthesis.perspectives.iter().map(|p| {
                    serde_json::json!({
                        "source": p.source,
                        "probability": p.probability,
                        "pct": format!("{:.1}%", p.probability * 100.0),
                        "historical_brier": p.historical_brier,
                        "rationale": p.rationale,
                    })
                }).collect::<Vec<_>>(),
                "methodology": {
                    "weighting": if synthesis.perspectives.iter().any(|p| p.historical_brier.is_some()) {
                        "empirical_bayes: inverse Brier score weighting"
                    } else {
                        "uniform: no historical Brier data available, all perspectives weighted equally"
                    },
                    "disagreement": "normalized standard deviation of probabilities (0=perfect consensus, 1=maximum polarization)",
                },
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 7 — Dragonfly-Eye"
            });

            self.record_experience("scenario_synthesize");
            Ok(output)
        })
        .await
    }

    /// Compute a calibration curve from stored forecasts.
    #[tool(
        description = "Compute a calibration curve from stored forecasts, optionally filtered by subject. It groups resolved forecasts into 10 probability bins and compares each bin's actual hit rate with its mean forecast probability. Positive bias means forecasts were too high; negative means too low. Use after scenario_score to build calibration history."
    )]
    pub async fn scenario_calibration(
        &self,
        Parameters(req): Parameters<CalibrationRequest>,
    ) -> String {
        execute_tool_semantic(self, "scenario_calibration", Self::ontology_anchor("scenario_calibration"), async {
            let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());

            let filtered_store = req
                .subject
                .as_deref()
                .map(|subject| store.filtered_by_subject(subject));
            let curve = superforecast::compute_calibration_curve(
                filtered_store.as_ref().unwrap_or(&store),
            )
            .map_err(map_scenario_error)?;

            let output = serde_json::json!({
                "subject": req.subject,
                "total_forecasts": curve.total_forecasts,
                "resolved_forecasts": curve.resolved_forecasts,
                "pending_forecasts": curve.total_forecasts - curve.resolved_forecasts,
                "overall_brier": curve.overall_brier,
                "brier_interpretation": superforecast::brier_interpretation(curve.overall_brier),
                "overconfidence_score": curve.overconfidence_score,
                "interpretation": curve.interpretation,
                "bins": curve.bins.iter().map(|b| {
                    serde_json::json!({
                        "range": b.probability_range,
                        "count": b.forecast_count,
                        "hit_rate": if b.hit_rate.is_finite() { Some(b.hit_rate) } else { None },
                        "hit_rate_pct": if b.hit_rate.is_finite() { Some(format!("{:.0}%", b.hit_rate * 100.0)) } else { None },
                        "expected_rate": b.expected_rate,
                        "bias": b.bias,
                        "bias_interpretation": if b.forecast_count == 0 {
                            "no_data"
                        } else if b.bias > 0.05 {
                            "overconfident"
                        } else if b.bias < -0.05 {
                            "underconfident"
                        } else {
                            "calibrated"
                        },
                    })
                }).collect::<Vec<_>>(),
                "guidance": if curve.resolved_forecasts < 10 {
                    "Insufficient data for reliable calibration — at least 10 resolved forecasts recommended. Bins with fewer than 5 forecasts are excluded from overconfidence scoring."
                } else {
                    "Calibration curve shows your forecasting accuracy across probability ranges. Use this to identify systematic biases: if your 80% forecasts only come true 60% of the time, you're overconfident in that range."
                },
                "reference": "Brier (1950); Murphy (1973) — decomposition of Brier score into reliability, resolution, and uncertainty components"
            });

            self.record_experience("scenario_calibration");
            Ok(output)
        })
        .await
    }

    /// Triage a forecasting question for the Goldilocks zone.
    #[tool(
        description = "Triage a forecasting question (Tetlock Commandment 1). Evaluates whether a question is worth the full superforecasting pipeline. Scores three dimensions: clarity (specificity + deadline), data availability (reference class exists?), and resolution criteria (will we know the answer?). Classifies into: clocklike (easy, base-rate suffices), goldilocks (worth full pipeline), cloudlike (too vague, refine the question)."
    )]
    pub async fn scenario_triage(&self, Parameters(req): Parameters<TriageRequest>) -> String {
        execute_tool_semantic(self, "scenario_triage", Self::ontology_anchor("scenario_triage"), async {
            let assessment = superforecast::triage_question(
                &req.question,
                req.has_deadline.unwrap_or(false),
                req.has_reference_class.unwrap_or(false),
                req.has_resolution_criteria.unwrap_or(false),
            );

            let output = serde_json::json!({
                "question": assessment.question,
                "is_forecastable": assessment.is_forecastable,
                "difficulty": assessment.difficulty,
                "scores": {
                    "clarity": assessment.clarity_score,
                    "data_availability": assessment.data_availability_score,
                    "resolution_criteria": assessment.resolution_criteria_clarity,
                    "overall": (assessment.clarity_score + assessment.data_availability_score + assessment.resolution_criteria_clarity) / 3.0,
                },
                "recommendation": assessment.recommendation,
                "next_steps": match assessment.difficulty.as_str() {
                    "clocklike" => "Use simple base-rate extrapolation or scenario_calibrate with a well-known reference class. The full pipeline may be overkill.",
                    "goldilocks" => "Run the full pipeline: scenario_build → scenario_calibrate → scenario_quantify. Use Fermi decomposition to break into sub-questions. Set up Bayesian updating via scenario_update.",
                    _ => "Refine the question: (1) add a specific deadline, (2) define clear resolution criteria, (3) identify a reference class. Then re-triage.",
                },
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 3 — Triage and the Goldilocks Zone"
            });

            self.record_experience("scenario_triage");
            Ok(output)
        })
        .await
    }

    /// Assess a scenario project across Chermack's five performance phases.
    #[tool(
        description = "Assess a scenario project's effectiveness (Chermack Phase 5). Evaluates the project across all five phases: Preparation (stakeholder engagement), Exploration (perspective diversity), Development (causal structure), Implementation (strategies applied), and Project Assessment (learning + calibration). Combines quantitative metrics (Brier scores, disagreement, event count, dependency ratio) with qualitative assessment. Answers Chermack's core question: did the scenario project improve decision quality? Returns per-phase scores, gaps, strengths, learning evidence, and actionable recommendations."
    )]
    pub async fn scenario_assess(&self, Parameters(req): Parameters<AssessRequest>) -> String {
        execute_tool_semantic(self, "scenario_assess", Self::ontology_anchor("scenario_assess"), async {
            let perspective_count = req.perspective_count.unwrap_or(1);
            let disagreement = req.disagreement_score.unwrap_or(0.0);
            let event_count = req.event_count.unwrap_or(0);
            let events_with_deps = req.events_with_dependencies.unwrap_or(0);
            let strategies_gen = req.strategies_generated.unwrap_or(0);
            let strategies_impl = req.strategies_implemented.unwrap_or(0);
            let has_indicators = req.has_early_warning_indicators.unwrap_or(false);

            let learning_events: Vec<String> = req
                .learning_events
                .as_deref()
                .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
                .unwrap_or_default();

            // Get calibration curve from the store if available
            let curve = {
                let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
                superforecast::compute_calibration_curve(&store).ok()
            };

            let assessment = superforecast::assess_project(&types::AssessInput {
                project_id: &req.project_id,
                subject: &req.subject,
                perspective_count,
                disagreement_score: disagreement,
                event_count,
                events_with_deps,
                calibration_curve: curve.as_ref(),
                strategies_generated: strategies_gen,
                strategies_implemented: strategies_impl,
                learning_events: learning_events,
                has_early_warning_indicators: has_indicators,
            });

            let output = serde_json::json!({
                "project_id": assessment.project_id,
                "subject": assessment.subject,
                "overall_score": assessment.overall_score,
                "overall_pct": format!("{:.0}%", assessment.overall_score * 100.0),
                "overall_assessment": assessment.overall_assessment,
                "phases": {
                    "preparation": {
                        "score": assessment.preparation.score,
                        "strengths": assessment.preparation.strengths,
                        "gaps": assessment.preparation.gaps,
                    },
                    "exploration": {
                        "score": assessment.exploration.score,
                        "strengths": assessment.exploration.strengths,
                        "gaps": assessment.exploration.gaps,
                    },
                    "development": {
                        "score": assessment.development.score,
                        "strengths": assessment.development.strengths,
                        "gaps": assessment.development.gaps,
                    },
                    "implementation": {
                        "score": assessment.implementation.score,
                        "strengths": assessment.implementation.strengths,
                        "gaps": assessment.implementation.gaps,
                    },
                    "project_assessment": {
                        "score": assessment.project_assessment.score,
                        "strengths": assessment.project_assessment.strengths,
                        "gaps": assessment.project_assessment.gaps,
                    },
                },
                "phase_scores": {
                    "preparation": assessment.preparation.score,
                    "exploration": assessment.exploration.score,
                    "development": assessment.development.score,
                    "implementation": assessment.implementation.score,
                    "project_assessment": assessment.project_assessment.score,
                },
                "learning_evidence": assessment.learning_evidence,
                "recommendations": assessment.recommendations,
                "calibration": curve.map(|c| serde_json::json!({
                    "resolved_forecasts": c.resolved_forecasts,
                    "overall_brier": c.overall_brier,
                    "interpretation": c.interpretation,
                })),
                "methodology": {
                    "ontology": dc_bibo::DATASET,
                    "framework": "Chermack's Performance-Based Scenario System (2011)",
                    "five_phases": [
                        "Phase 1: Project Preparation — scope, stakeholders, resources (Chermack, Ch. 5)",
                        "Phase 2: Scenario Exploration — driving forces, trends, uncertainties (Chermack, Ch. 6)",
                        "Phase 3: Scenario Development — narratives, logic, consistency (Chermack, Ch. 7)",
                        "Phase 4: Scenario Implementation — strategies, wind-tunneling, early warning (Chermack, Ch. 8)",
                        "Phase 5: Project Assessment — learning, performance improvement (Chermack, Ch. 9)",
                    ],
                    "integration": "Combines Chermack (project effectiveness) with Tetlock (forecast accuracy via calibration curve) and Schwartz (scenario narratives via event trees)",
                    "reference": "Chermack, T.J. (2011). Scenario Planning in Organizations: How to Create, Use, and Assess Scenarios. Berrett-Koehler."
                }
            });

            self.record_experience("scenario_assess");
            Ok(output)
        })
        .await
    }
}
// ── Tool handler registration ──────────────────────────────────────────────

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for ScenariosServer {}

#[cfg(test)]
mod tool_surface_tests {
    use super::*;

    // Pins the registered tool-surface count end-to-end. Catches silent
    // registration drops — a `#[tool]` impl block without `#[tool_router]`, or
    // a sub-router missing from `combined_router()`, silently registers nothing
    // (`cargo check` passes on an unwired orphan). Mirrors the swarm pin.
    #[test]
    fn tool_surface_is_exactly_21_registered_tools() {
        let n = ScenariosServer::combined_router().list_all().len();
        assert_eq!(n, 21, "scenarios registered tool surface changed; got {n}");
    }

    // Coverage: every registered tool must have a non-None ontology anchor.
    // Catches the silent-drop failure mode where a new tool is added to the
    // router without a corresponding arm in ontology_anchor. The count pin
    // above catches addition; this test catches anchoring.
    #[test]
    fn ontology_anchor_covers_all_registered_tools() {
        let router = ScenariosServer::combined_router();
        for tool in router.list_all() {
            assert!(
                ScenariosServer::ontology_anchor(&tool.name).is_some(),
                "ontology_anchor returned None for registered tool '{}'; \
                 add an explicit arm or adjust the fallback",
                tool.name
            );
        }
    }

    // Regression: the ontology anchor must not collapse to a single constant.
    // Pipeline-process tools (frame/brainstorm/build) anchor on PKO; computed
    // outputs (quantify/score) anchor on Dublin Core.
    #[test]
    fn ontology_anchor_distinguishes_process_from_data() {
        use hkask_bridge_ontology::{dc_bibo, pko};
        let frame = ScenariosServer::ontology_anchor("scenario_frame");
        let quantify = ScenariosServer::ontology_anchor("scenario_quantify");
        assert_ne!(
            frame, quantify,
            "scenario_frame (PKO) and scenario_quantify (DC) must anchor on distinct ontologies"
        );
        assert_eq!(
            frame,
            Some(pko::PROCEDURE),
            "scenario_frame must anchor on PKO Procedure"
        );
        assert_eq!(
            quantify,
            Some(dc_bibo::DATASET),
            "scenario_quantify must anchor on Dublin Core Dataset"
        );
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn parse_time_horizon(s: Option<&str>) -> TimeHorizon {
    match s {
        Some("tactical") | Some("12-18mo") | Some("12mo") => TimeHorizon::Tactical,
        Some("strategic") | Some("3-5yr") | Some("5yr") => TimeHorizon::Strategic,
        Some("long_term") | Some("7-10yr") | Some("10yr") => TimeHorizon::LongTerm,
        _ => TimeHorizon::Strategic,
    }
}

fn parse_scenario_type(s: Option<&str>) -> ScenarioType {
    match s {
        Some("company_update") | Some("quarterly") => ScenarioType::CompanyUpdate,
        Some("company_analysis") | Some("thesis") => ScenarioType::CompanyAnalysis,
        Some("emerging_economic") | Some("disruption") => ScenarioType::EmergingEconomic,
        Some("economic_potential") | Some("long_term") => ScenarioType::EconomicPotential,
        _ => ScenarioType::CompanyAnalysis,
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-scenarios",
        SERVER_VERSION,
        |ctx| {
            // D28 — Standardized Artifact Storage. Default data dir is
            // `{kask_data_dir}/mcp/scenarios/`. Override via
            // `HKASK_SCENARIOS_DATA`.
            let scenarios_data_dir = std::env::var("HKASK_SCENARIOS_DATA")
                .ok()
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    hkask_types::agent_paths::resolve_under_data_dir(std::path::Path::new(
                        hkask_types::agent_paths::MCP_DIR,
                    ))
                    .join("scenarios")
                });
            Ok(ScenariosServer::new(
                ctx.webid,
                std::sync::Arc::new(hkask_verification::VerificationStore::open()),
                std::sync::Arc::new(std::sync::Mutex::new(superforecast::ForecastStore::new(
                    Some(scenarios_data_dir),
                ))),
                std::sync::Mutex::new(None),
                std::sync::Mutex::new(HashSet::new()),
            ))
        },
        vec![],
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn empty_server() -> ScenariosServer {
        ScenariosServer::new(
            hkask_types::WebID::default(),
            std::sync::Arc::new(hkask_verification::VerificationStore::in_memory()),
            Arc::new(Mutex::new(superforecast::ForecastStore::new(None))),
            Mutex::new(None),
            Mutex::new(HashSet::new()),
        )
    }

    #[tokio::test]
    async fn scenario_status_emits_dispatchable_provenance() {
        let server = empty_server();
        let response = server.scenario_status(Parameters(StatusRequest {})).await;

        // Unwrap the {"content": <value>} envelope via the shared seam
        // (see the repo .rules "MCP tool responses are content envelopes" trap).
        let content = hkask_types::tool_response::parse_tool_response(&response)
            .expect("scenario_status returns a content envelope");
        let provenance = content
            .get("provenance")
            .expect("scenario_status emits a provenance block");
        let tool = provenance
            .get("tool")
            .and_then(|t| t.as_str())
            .expect("provenance.tool is a non-empty string");
        assert!(!tool.is_empty(), "provenance.tool must be non-empty");
        assert_eq!(tool, "scenario_status");
        assert_eq!(
            provenance.get("server").and_then(|s| s.as_str()),
            Some("hkask-mcp-scenarios")
        );
        // args is the (empty) StatusRequest; span_id is null (not surfaced).
        assert!(provenance.get("args").is_some(), "provenance.args present");
        assert!(
            provenance.get("span_id").is_some(),
            "provenance.span_id present"
        );
    }

    #[tokio::test]
    async fn scenario_full_refuses_empty_events_without_panicking() {
        // Two perspectives reach the synthesis step, which anchors on the first
        // event id. An empty events array previously indexed `events[0]` and
        // panicked the tool future; it must now be refused as invalid input.
        let server = empty_server();
        let response = server
            .scenario_full(Parameters(FullPipelineRequest {
                subject: "test".to_string(),
                events: vec![],
                perspectives: Some(vec![
                    Perspective {
                        source: "a".to_string(),
                        probability: 0.5,
                        fermi_sub_questions: vec![],
                        base_rate: None,
                        reference_class: None,
                        rationale: None,
                        historical_brier: None,
                    },
                    Perspective {
                        source: "b".to_string(),
                        probability: 0.6,
                        fermi_sub_questions: vec![],
                        base_rate: None,
                        reference_class: None,
                        rationale: None,
                        historical_brier: None,
                    },
                ]),
                perspective_count: None,
                strategies_generated: None,
                strategies_implemented: None,
                learning_events: None,
                has_early_warning_indicators: None,
            }))
            .await;

        assert!(
            response.contains("at least one scenario event"),
            "empty events must be refused with a clear message, got: {response}"
        );
    }

    #[tokio::test]
    async fn scenario_score_populates_category_for_per_domain_calibration() {
        // B1: `scenario_score` previously wrote `category: None` on every
        // `StoredForecastRecord`, so `resolved_by_category` always returned
        // empty and `domain_bias_delta` was permanently 0.0 — the entire
        // per-domain calibration subsystem (~90 LOC in superforecast.rs) was
        // unreachable. This test pins that the stored record carries the
        // event's `scenario_type` as its category and that ≥5 same-category
        // resolutions produce a non-zero δ.
        let server = empty_server();

        // 6 underconfident forecasts: p=0.3, all occurred (outcome=true).
        // bias = expected − hit_rate = 0.3 − 1.0 = −0.7 (underconfident) → δ > 0.
        let events: Vec<ScenarioEvent> = (0..6)
            .map(|i| ScenarioEvent {
                id: format!("evt-{i}"),
                name: format!("Event {i}"),
                question: format!("Will {i} occur?"),
                deadline: chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
                time_horizon: types::TimeHorizon::Strategic,
                scenario_type: types::ScenarioType::CompanyAnalysis,
                subject: "TEST".to_string(),
                probability: 0.3,
                basis: None,
                depends_on: vec![],
                sub_questions: vec![],
                base_rate: None,
                reference_class: None,
                brier_score: None,
                update_count: 0,
            })
            .collect();
        let outcomes: Vec<OutcomeEntry> = (0..6)
            .map(|i| OutcomeEntry {
                event_id: format!("evt-{i}"),
                occurred: true,
            })
            .collect();

        let _response = server
            .scenario_score(Parameters(ScoreRequest {
                forecast_id: "fcst-1".to_string(),
                events,
                outcomes,
            }))
            .await;

        let store = server
            .forecast_store
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Every stored record must carry the scenario_type as its category.
        assert!(
            store
                .records
                .values()
                .all(|r| r.category.as_deref() == Some("company_analysis")),
            "scenario_score must populate category from scenario_type; got: {:?}",
            store
                .records
                .values()
                .map(|r| &r.category)
                .collect::<Vec<_>>()
        );
        // ≥5 same-category resolved forecasts must yield a non-zero δ.
        let delta = superforecast::domain_bias_delta(Some(&store), "company_analysis");
        assert!(
            delta > 0.0,
            "underconfident domain with 6 resolved forecasts must get δ > 0, got {delta}"
        );
    }
}
