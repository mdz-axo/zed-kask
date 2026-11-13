#![forbid(unsafe_code)]
#![warn(clippy::let_underscore_future)]
// `tokio` is in [dependencies] for the bin target's `#[tokio::main]`; the lib
// itself does not use it, so the unused_crate_dependencies lint fires on the
// lib target. This is the legitimate bin-needs-dep case.
#![allow(unused_crate_dependencies)]
//! hKask MCP Scenarios — domain-agnostic scenario planning server.
//!
//! Event-tree forecasting with conditional dependencies (MAIA methodology).
//!
//! Shared engine: Fermi decomposition, outside/inside view, Bayesian updating,
//! Brier scoring, dragonfly-eye synthesis, calibration tracking, cross-validation.
//!
//! ## Tools (22) — pinned by `tool_surface_is_exactly_22_registered_tools`
//! - `scenario_status` — Server state: pipeline overview, calibration summary, last stored tree
//! - `scenario_frame_document` — Structure framing answers into FramingDocument, persisted under the project record
//! - `scenario_frame` — 7-turn conversational framing interview
//! - `scenario_triage` — Specification-quality classification (well_specified / goldilocks / needs_refinement)
//! - `scenario_brainstorm` — 4-round temperature-shifting protocol
//! - `scenario_build` — Construct event tree template from research context
//! - `scenario_quantify` — Resolve conditional probability tree
//! - `scenario_propagate` — Update one event's prior and propagate through descendants
//! - `scenario_recompute_posteriors` — Evidence-posterior recomputation, both directions (shared engine with the graph widget)
//! - `scenario_calibrate` — Fermi decomposition + outside/inside view
//! - `scenario_update` — Bayesian evidence revision
//! - `scenario_synthesize` — Dragonfly-eye multi-perspective aggregation
//! - `scenario_declare` — Freeze prospective event/probability/horizon/resolver identity
//! - `scenario_score` — Record outcomes against frozen declarations; legacy history stays unscored
//! - `scenario_calibration` — Frequencies and descriptive signed probability error
//! - `scenario_cross_validate` — LLM vs computation cross-validation
//! - `scenario_assess` — Chermack five-phase project evaluation, anchored on the project record
//! - `scenario_from_markets_set` — Bridge from prediction-markets (multi-record EventTree)
//! - `forecast_persist` — Persist a pre-computed equity price target for later Brier scoring (ported from the companies server)
//! - `forecast_record` — Record a forecast outcome and close the Brier loop (ported from the companies server)
//! - `forecast_list` — List durable equity forecasts for a symbol (ported from the companies server)
//! - `scenario_risk_measure` — Probability-weighted risk measure over caller-supplied scenario branches

use hkask_mcp_server::server::{McpToolError, execute_tool};
use rmcp::{handler::server::wrapper::Parameters, tool, tool_router};

pub mod equity_forecast;
// `#[tool_router(..., vis = "pub")]` in equity_forecast.rs makes
// `equity_forecast_router()` an inherent method on ScenariosServer — no
// re-export needed; `combined_router()` below merges it.
pub(crate) mod ontology;
#[cfg(test)]
mod property_tests;
pub(crate) mod superforecast;
pub(crate) mod templates;
pub mod types;

// Re-export the forecast store so external tool-behavior contract tests (tests/)
// can construct a server over an in-memory store and drive the real
// `Parameters<T>` tool seam — the testing standard
// (docs/reference/mcp-servers/README.md §Testing standard) requires this.
pub use superforecast::ForecastStore;
pub use superforecast::JournalStore;
pub use superforecast::ProjectStore;

use types::*;

// ── Error classification ───────────────────────────────────────────────────

/// Classify a `ScenarioError` into the appropriate `McpToolError` kind.
///
/// Caller-input defects (bad probabilities, cycles, unknown parents, empty
/// input) map to `InvalidArgument`; missing entities map to `NotFound`;
/// missing-state conditions the caller can remedy (no resolved forecast)
/// map to `FailedPrecondition`. `Forecast`
/// computation errors are classified per variant (all current
/// `ForecastError` variants are caller-input defects).
fn map_scenario_error(error: ScenarioError) -> McpToolError {
    match &error {
        ScenarioError::EventNotFound(id) => {
            McpToolError::not_found(format!("event '{id}' not found"))
        }
        ScenarioError::NoForecastData => McpToolError::failed_precondition(error.to_string()),
        ScenarioError::Forecast(forecast_error) => match forecast_error {
            hkask_forecast::ForecastError::InvalidProbability(..)
            | hkask_forecast::ForecastError::BrierLengthMismatch(..)
            | hkask_forecast::ForecastError::BrierNoData => McpToolError::invalid_argument(
                format!("forecast computation failed: {forecast_error}",),
            ),
        },
        // Caller-side input defects
        ScenarioError::NoEvents
        | ScenarioError::InvalidProbability(..)
        | ScenarioError::InvalidDependencyProbability(..)
        | ScenarioError::InvalidDependency(..)
        | ScenarioError::UnknownParent(..)
        | ScenarioError::CycleDetected
        | ScenarioError::DuplicateEvent(..)
        | ScenarioError::InsufficientPerspectives
        | ScenarioError::InvalidInput(..) => McpToolError::invalid_argument(error.to_string()),
    }
}

/// Refuse a call that depends on a store whose files did not load. The
/// reason names the file and the remedy.
pub(crate) fn require_loaded(load_error: Option<&str>) -> Result<(), McpToolError> {
    match load_error {
        Some(reason) => Err(McpToolError::failed_precondition(reason.to_owned())),
        None => Ok(()),
    }
}

/// Refuse a number that is not a probability.
pub(crate) fn require_unit(name: &str, value: f64) -> Result<(), McpToolError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(McpToolError::invalid_argument(format!(
            "{name} must be in [0, 1], got {value}"
        )))
    }
}

pub mod requests;
pub(crate) use requests::*;

// ── Server struct ──────────────────────────────────────────────────────────

hkask_mcp_server::mcp_server!(
    pub struct ScenariosServer {
        pub forecast_store: std::sync::Arc<std::sync::Mutex<superforecast::ForecastStore>>,
        /// Equity forecast persistence (ported from the companies server,
        /// plan Phase A P1) — the same journal+snapshot machinery, typed to
        /// the pre-computed price-target record. Separate snapshot file so
        /// the two stores' histories never mix.
        pub equity_forecast_store: std::sync::Arc<
            std::sync::Mutex<superforecast::JournalStore<equity_forecast::StoredEquityForecast>>,
        >,
        pub project_store: std::sync::Mutex<superforecast::ProjectStore>,
    } = crate::ontology::annotate;
);

impl ScenariosServer {
    /// Persist a tree into the project record (PR-07): upsert the project
    /// (created when absent, keyed by the tree's subject) with the tree as
    /// its last quantified state. `scenario_status` reads the most recently
    /// stored tree from here, so there is no separate in-memory copy to
    /// lose at a restart. Persistence failures surface as tool errors.
    fn persist_tree(&self, tree: &types::EventTree) -> Result<(), McpToolError> {
        let now = chrono::Utc::now();
        let today = now.date_naive();
        let mut store = self.project_store.lock().unwrap_or_else(|e| e.into_inner());
        require_loaded(store.load_error())?;
        let mut project =
            store
                .get(&tree.subject)
                .cloned()
                .unwrap_or_else(|| types::ScenarioProject {
                    project_id: tree.subject.clone(),
                    subject: tree.subject.clone(),
                    framing_document: None,
                    last_tree: None,
                    last_tree_at: None,
                    created_at: today,
                    updated_at: today,
                });
        project.last_tree = Some(tree.clone());
        project.last_tree_at = Some(now);
        project.updated_at = today;
        store
            .upsert(project)
            .map_err(|error| McpToolError::internal(format!("project persistence failed: {error}")))
    }
}

// ── MCP Tools ──────────────────────────────────────────────────────────────

/// The note a tree response carries when some node's parents share an
/// ancestor: marginalization treats parents as independent, so that node's
/// marginal is approximate. `None` when every node's parents are independent.
fn dependent_parents_note(tree: &EventTree) -> Option<&'static str> {
    let events: Vec<ScenarioEvent> = tree.nodes.iter().map(|n| n.event.clone()).collect();
    (!superforecast::parents_are_independent(&events)).then_some(
        "some event's parents share an ancestor: marginals treat parents as independent, so that event's marginal is approximate. The joint probability is unaffected.",
    )
}

/// Build the standard provenance object for a tool output. The common core
/// (`tool`, `server`, `version`) is merged with per-tool `extra` fields.
fn provenance(tool: &str, extra: serde_json::Map<String, serde_json::Value>) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("tool".into(), serde_json::Value::String(tool.to_string()));
    map.insert(
        "server".into(),
        serde_json::Value::String("hkask-mcp-scenarios".to_string()),
    );
    map.insert(
        "version".into(),
        serde_json::Value::String(SERVER_VERSION.to_string()),
    );
    for (key, value) in extra {
        map.insert(key, value);
    }
    serde_json::Value::Object(map)
}

/// Serialize an EventTree's nodes to the standard JSON shape.
fn tree_nodes_json(tree: &EventTree) -> Vec<serde_json::Value> {
    tree.nodes
        .iter()
        .map(|n| {
            let node = serde_json::json!({
                "id": n.event.id,
                "question": n.event.question,
                "marginal_probability": n.marginal_probability,
                "depends_on": n.event.depends_on.iter().map(|d| serde_json::json!({
                    "parent_event_ids": d.parent_event_ids,
                    "conditionals": d.conditionals,
                })).collect::<Vec<_>>(),
                "base_rate": n.event.base_rate,
                "certainty_distance": n.certainty_distance,
            });
            node
        })
        .collect()
}

#[tool_router(router = scenario_router, vis = "pub")]
impl ScenariosServer {
    /// Return the current state snapshot for TUI display.
    #[tool(
        description = "Return current scenario server state: forecast counts, the calibration summary, the 20 most recently declared forecasts, and the most recently quantified or propagated event tree (read from the project store, so it survives a restart). store_errors names any store file that did not load; while it is non-empty the counts are incomplete."
    )]
    pub async fn scenario_status(
        &self,
        _parameters: Parameters<StatusRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_status", async {
            let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
            let total = store.len();
            let resolved: Vec<_> = store.resolved();
            let resolved_count = resolved.len();
            let pending_count = total - resolved_count;

            let curve = match superforecast::compute_calibration_curve(&store) {
                Ok(curve) => Some(curve),
                Err(ScenarioError::NoForecastData) => None,
                Err(error) => return Err(map_scenario_error(error)),
            };
            let overall_brier = curve.as_ref().map(|c| c.overall_brier);
            let eligible_resolved_count = store.eligible_resolved().len();
            let calibration = curve.map(|c| serde_json::json!({
                "total_forecasts": c.total_forecasts,
                "resolved_forecasts": c.resolved_forecasts,
                "unscored_resolved_forecasts": c.unscored_resolved_forecasts,
                "overall_brier": c.overall_brier,
                "signed_probability_error": c.signed_probability_error,
                "signed_probability_error_sample_count": c.signed_probability_error_sample_count,
                "signed_probability_error_qualifying_bins": c.signed_probability_error_qualifying_bins,
                "signed_probability_error_bin_min_count": types::SIGNED_PROBABILITY_ERROR_MIN_BIN_COUNT,
                "evidence_status": if c.signed_probability_error.is_some() { "descriptive_only" } else { "insufficient" },
                "score_convention": "binary_mean_squared_error",
                "baseline": null,
            }));

            // Newest declaration first; undeclared history sorts last.
            let mut by_recency: Vec<_> = store.values().collect();
            by_recency.sort_by(|a, b| {
                let key = |r: &types::StoredForecastRecord| {
                    (r.declaration.as_ref().map(|d| d.declared_at.clone()), r.created_at)
                };
                key(b)
                    .cmp(&key(a))
                    .then_with(|| (&a.forecast_id, &a.event_id).cmp(&(&b.forecast_id, &b.event_id)))
            });
            let recent: Vec<_> = by_recency.into_iter().take(20).map(|r| serde_json::json!({
                "forecast_id": r.forecast_id,
                "event_id": r.event_id,
                "event_name": r.event_name,
                "subject": r.subject,
                "probability": r.probability,
                "created_at": r.created_at.to_string(),
                "scoring_eligible": r.declaration.as_ref().is_some_and(|d| d.is_prospective()),
                "eligibility_note": if r.declaration.as_ref().is_some_and(|d| d.is_prospective()) { "precise prospective declaration" } else { "legacy: no precise prospective declaration" },
                "outcome": r.outcome,
            })).collect();

            let (last_tree, project_error) = {
                let projects = self.project_store.lock().unwrap_or_else(|e| e.into_inner());
                (projects.last_tree(), projects.load_error().map(str::to_owned))
            };
            let store_errors: Vec<String> = [
                store.load_error().map(str::to_owned),
                self.equity_forecast_store.lock().unwrap_or_else(|e| e.into_inner()).load_error().map(str::to_owned),
                project_error,
            ]
            .into_iter()
            .flatten()
            .collect();
            let tree = last_tree.map(|t| serde_json::json!({
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
                    "sub_question_count": n.event.sub_questions.len(),
                    "has_base_rate": n.event.base_rate.is_some(),
                })).collect::<Vec<_>>()
            }));

            Ok(serde_json::json!({
                "pipeline": {
                    "forecast_count": total,
                    "resolved_count": resolved_count,
                    "pending_count": pending_count,
                    "eligible_resolved_count": eligible_resolved_count,
                    "unscored_resolved_count": resolved_count - eligible_resolved_count,
                    "overall_brier": overall_brier,
                    "evidence_status": if eligible_resolved_count == 0 { "insufficient" } else { "descriptive_only" },
                    "recent_forecasts": recent
                },
                "calibration": calibration,
                "event_tree": tree,
                "store_errors": store_errors,
                "provenance": provenance("scenario_status", serde_json::Map::new()),
            }))
        }).await
    }

    /// Bridge: compose a SET of prediction-market records into a dependent
    /// event tree (T4a). The per-record market-bridge gates apply;
    /// dependency edges are caller-authored (the server computes marginals
    /// but never invents conditional probabilities).
    ///
    /// MATURITY NOTE: this is the detailed end of the scenario-modeling
    /// ladder. The simple path — `scenario_quantify` on this server over a
    /// small independent event set → `scenario_risk_measure` over
    /// caller-supplied branch returns — is the intended on-ramp; an analyst
    /// typically arrives at a full tree only after research (company,
    /// industry, economy, technology, management, domain experts) reveals
    /// which events actually condition each other. The flat-tree mode is
    /// retained as a first-class citizen, not a legacy path.
    #[tool(
        description = "Compose a set of prediction-market records (from hkask-mcp-prediction-markets) into a validated EventTree with caller-authored dependency edges. Each record passes the per-record market-bridge gates: a closed or resolved market is refused; a low reliability tier or a match confidence of low withholds that record's base rate and leaves a 0.5 scaffold. Question-overlap duplicates are flagged; cycles, more than 4 parents per edge and wrong-length conditional tables are rejected. Returns the resolved tree (marginals, joint probability) plus composition warnings; approximation_note is set when an event's parents share an ancestor. Detailed-mode tool: the simpler path (scenario_quantify, then scenario_risk_measure over caller-supplied branch returns) is the intended on-ramp before wiring full trees."
    )]
    pub async fn scenario_from_markets_set(
        &self,
        Parameters(req): Parameters<MarketsSetBridgeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_from_markets_set", async {
            let records: Vec<hkask_mcp_prediction_markets::types::MarketRecord> =
                serde_json::from_value(req.market_records.into())
                    .map_err(|e| McpToolError::invalid_argument(format!("invalid market records JSON array: {e}")))?;

            let confidences = req.match_confidences.unwrap_or_else(|| vec![None; records.len()]);
            if let Some(unknown) = confidences.iter().flatten().find(|c| !MATCH_CONFIDENCES.contains(&c.as_str())) {
                return Err(McpToolError::invalid_argument(format!(
                    "match confidence '{unknown}' must be one of: {} (or null)",
                    MATCH_CONFIDENCES.join(", ")
                )));
            }
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

            let (tree, warnings) = superforecast::compose_market_tree(&records, &confidences, &specs)
            .map_err(map_scenario_error)?;

            let nodes = tree_nodes_json(&tree);

            let output = serde_json::json!({
                "tree": {
                    "subject": tree.subject,
                    "root_ids": tree.root_ids,
                    "topo_order": tree.topo_order,
                    "joint_probability": tree.joint_probability,
                    "nodes": nodes,
                },
                "warnings": warnings,
                "approximation_note": dependent_parents_note(&tree),
                "provenance": provenance("scenario_from_markets_set", {
                    let mut m = serde_json::Map::new();
                    m.insert("market_count".into(), records.len().into());
                    m.insert("dependency_edge_count".into(), specs.len().into());
                    m
                }),
                "bridge_note": "Dependency edges and their conditionals are caller-authored; the server validates structure and computes marginals/joints but never invents conditional probabilities. base_rate is None on records refused by the per-market gates.",
            });

            Ok(output)
        })
        .await
    }

    /// Cross-validate two probability estimates to close the learning loop.
    #[tool(
        description = "Cross-validate two probability estimates for the same event — typically an LLM-generated estimate from the superforecasting skill against a server-computed estimate from scenario_calibrate. Computes per-sub-question divergence to identify where estimates differ most. Flags for review when overall divergence exceeds threshold (default 0.15). Both estimates and the threshold must be in [0, 1]. Returns CrossValidation with divergence, per-sub-question breakdown, and recommendation. Closes the learning loop: if divergence > 0.15, activate grill-me skill to interrogate assumptions."
    )]
    pub async fn scenario_cross_validate(
        &self,
        Parameters(req): Parameters<CrossValidateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_cross_validate", async {
            require_unit("estimate_a", req.estimate_a)?;
            require_unit("estimate_b", req.estimate_b)?;
            if let Some(threshold) = req.review_threshold {
                require_unit("review_threshold", threshold)?;
            }
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
                    "framework": "Cross-validation between LLM reasoning (superforecasting skill) and computational verification (scenarios server)",
                    "threshold_rationale": "0.15 divergence threshold based on Tetlock's incremental belief updating (Commandment 4): superforecasters typically move probabilities in 0.05-0.10 increments. A 0.15 divergence suggests fundamentally different assumptions, not just calibration noise.",
                    "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 5-6"
                }
            });

            Ok(output)
        })
        .await
    }

    /// Generate a conversational framing protocol to scope a scenario project.
    #[tool(
        description = "Start a conversational framing session for a scenario project. Generates a 7-turn conversational protocol with natural openings (not numbered questions) designed using behavioral psychology and improv coaching postures. Turns: (1) What is on your mind? (foot-in-the-door), (2) What decision hangs on this? (curiosity gap), (3) When do you need to act? (temporal anchoring), (4) What is NOT on the table? (loss aversion — exclusions first), (5) Who would say I told you so? (social proof + contrarian), (6) What does good enough look like? (peak-end begins), (7) What assumptions could break everything? (peak-end closes). Each turn has improv mode guidance (Plussing, Yes And, Yes But, Coaching) and behavioral psychology notes. The agent acts as a coach, not an interviewer. Run this FIRST — before scenario_brainstorm."
    )]
    pub async fn scenario_frame(
        &self,
        Parameters(req): Parameters<FrameRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_frame", async {
            require_subject(&req.subject)?;
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

            Ok(output)
        })
        .await
    }

    /// Structure completed framing conversation answers into a FramingDocument.
    #[tool(
        description = "Structure completed framing conversation answers into a typed FramingDocument. Accepts the subject and an object with answers from the 7-turn conversational protocol (from scenario_frame). Produces a validated FramingDocument with: focal_question, decision_at_stake, time_horizon, action_deadline, in_scope, out_of_scope, stakeholders (as personas for brainstorming), use_case, success_criteria, constraints, surfaced_assumptions, and exploration_prompts. answers must be a JSON object; time_horizon (tactical, strategic, long_term; default strategic) and use_case (strategic_decision, investment_thesis, monitoring_dashboard, landscape_exploration, contingency_planning; default landscape_exploration) are closed sets and any other value is refused. The output feeds directly into scenario_brainstorm as the frame. Run AFTER the framing conversation is complete and BEFORE scenario_brainstorm. Persists the document under the scenario project record — the project id defaults to the subject, and a re-run updates the same project in place."
    )]
    pub async fn scenario_frame_document(
        &self,
        Parameters(req): Parameters<FrameDocumentRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_frame_document", async {
            let answers: serde_json::Value = req.answers.into();

            let document = superforecast::structure_framing_document(&req.subject, &answers)
                .map_err(map_scenario_error)?;

            // Persist the framing document under the project record
            // (PR-01): the project id defaults to the subject, and a
            // re-run updates the same project in place.
            let project_id = req
                .project_id
                .clone()
                .unwrap_or_else(|| req.subject.clone());
            let today = chrono::Utc::now().date_naive();
            {
                let mut store = self.project_store.lock().unwrap_or_else(|e| e.into_inner());
                require_loaded(store.load_error())?;
                let mut project = store
                    .get(&project_id)
                    .cloned()
                    .unwrap_or_else(|| types::ScenarioProject {
                        project_id: project_id.clone(),
                        subject: req.subject.clone(),
                        framing_document: None,
                        last_tree: None,
                        last_tree_at: None,
                        created_at: today,
                        updated_at: today,
                    });
                project.subject = req.subject.clone();
                project.framing_document = Some(document.clone());
                project.updated_at = today;
                store
                    .upsert(project)
                    .map_err(|error| McpToolError::internal(format!(
                        "project persistence failed: {error}"
                    )))?;
            }

            let output = serde_json::json!({
                "subject": req.subject,
                "project_id": project_id,
                "persisted": true,
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
                    "scenario_synthesize → equal-weight dragonfly-eye aggregation",
                    "scenario_declare → freeze prospective forecast and resolver",
                    "scenario_score → record outcomes against the frozen declaration",
                    "scenario_assess → Chermack project evaluation"
                ]
            });

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
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_brainstorm", async {
            require_subject(&req.subject)?;
            let horizon = parse_time_horizon(req.time_horizon.as_deref())?;
            let research = req.research_context.as_deref().unwrap_or("No research context provided. Run web searches first and pass the results as research_context, or provide context manually.");

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

            Ok(output)
        })
        .await
    }
    /// Returns a scenario event-tree scaffold/template for LLM completion.
    /// Always returns an extraction template (event schema, dependency format,
    /// certainty tiers, Tetlock commandments); the agent fills it against
    /// the `context` field if provided. Does not itself parse research into final events.
    #[tool(
        description = "Build a scenario event tree scaffold from web research. Returns an extraction template (not final events) with: event schema, dependency format, certainty tier definitions, and Tetlock's 10 commandments as methodology. The agent (LLM) fills in event_extraction_prompt against the provided context to produce ScenarioEvent objects. Without context, returns a structural template. The ultimate pipeline artifact: events with calibrated probabilities, conditional dependency chains, and connections to driver/decision factors from the framing document. Feeds into scenario_quantify for probability resolution."
    )]
    pub async fn scenario_build(
        &self,
        Parameters(req): Parameters<BuildEventsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_build", async {
            require_subject(&req.subject)?;
            let horizon = parse_time_horizon(req.time_horizon.as_deref())?;
            let scenario_type = parse_scenario_type(req.scenario_type.as_deref())?;
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
                     3. Assigned a probability tier: proximate (67% or more), probable (33% to under 67%), or possible (under 33%)\n\
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
                     - base_rate and reference_class: null when unavailable
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
                    "time_horizon": serde_json::to_value(horizon).unwrap_or_default(),
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
                    "proximate": {"range": "67% or more", "description": "Already started to happen, could stop"},
                    "probable": {"range": "33% to under 67%", "description": "All elements exist for it to happen"},
                    "possible": {"range": "under 33%", "description": "Could happen but unlikely"}
                },
                "methodology": {
                    "framework": "MAIA event-based scenario planning (Tetlock Superforecasting + Schwartz imagination)",
                    "research_pipeline": "1. Web search (brave/firecrawl/tavily) → 2. scenario_build (this tool) → 3. scenario_quantify (resolve tree) → 4. scenario_calibrate (Fermi probabilities)",
                    "tetlock_commandments": [
                        "1. Triage: focus on Goldilocks-zone questions",
                        "2. Fermi-ize: break into tractable sub-questions",
                        "3. Balance inside and outside views",
                        "4. Incremental belief updating",
                        "5. Look for the clashing causal forces at work in each problem",
                        "6. Distinguish degrees of doubt (use full 0-100% scale)",
                        "7. Balance under/overconfidence",
                        "8. Postmortem successes and failures",
                        "9. Bring out best in others",
                        "10. Master error-balancing"
                    ],
                    "reference": "Tetlock & Gardner, Superforecasting (2015); Schwartz, The Art of the Long View (1991)"
                }
            });

            Ok(output)
        })
        .await
    }

    /// Quantify an event tree: compute marginal probabilities, joint probability,
    /// and build the full resolved tree with sensitivity rankings.
    #[tool(
        description = "Quantify a scenario event tree. Takes an array of ScenarioEvent objects with conditional dependencies and computes: (1) topological sort of dependency graph, (2) marginal probabilities for each event via conditional probability propagation, treating each event's parents as independent, (3) joint probability of the full event chain, (4) certainty distance per event, (5) sensitivity ranking. Rejects cycles, blank or repeated event ids, missing parent references, wrong-length conditional tables and more than 20 parents in one group. approximation_note is set when an event's parents share an ancestor, where its marginal is approximate. Stores the tree in the project record named by the first event's subject. Returns the full resolved EventTree. To render this tree inline for the user, emit a fenced `graph` code block whose body is a JSON object: {\"viz\":\"event_tree\", \"subject\":..., \"joint_probability\":..., \"nodes\":[{\"id\",\"name\",\"question\",\"marginal_probability\",\"certainty_tier\",\"depends_on\":[{\"parent_event_ids\":[...],\"conditionals\":[...]}]}]} — the conversation view renders it as an interactive event-tree DAG (pan/zoom; click a node to set evidence and re-propagate). Pass through the fields this tool returns; keep `depends_on[].conditionals` intact so the DAG can re-propagate when evidence is set."
    )]
    pub async fn scenario_quantify(
        &self,
        Parameters(req): Parameters<QuantifyRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_quantify", async {
            let tree = superforecast::build_event_tree(&req.events)
                .map_err(map_scenario_error)?;

            // Persist into the project record: scenario_status reads the
            // most recently stored tree from there.
            self.persist_tree(&tree)?;

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
                    "certainty_distance": n.certainty_distance,
                    "depends_on": n.event.depends_on.iter().map(|d| serde_json::json!({
                        "parent_event_ids": d.parent_event_ids,
                        "conditionals": d.conditionals,
                    })).collect::<Vec<_>>(),
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
                "approximation_note": dependent_parents_note(&tree),
                "framework": "Conditional probability tree. Each node's marginal is computed via full joint-table marginalization under parent independence: P(E) = Sum_a P(E|a) * Product_i P(p_i)^{a_i} * (1-P(p_i))^{1-a_i}. Root nodes use their intrinsic probability. Joint = product of all-nodes-occur conditionals."
            });

            Ok(output)
        })
        .await
    }

    /// Tree-level propagation: update one root event's prior and recompute
    /// every descendant marginal and the joint.
    #[tool(
        description = "Update one root event's prior probability and propagate through the event tree: all descendant marginals and the joint probability are recomputed. Only a root event (no depends_on) has a prior; an event with parents is refused, because its probability comes from its conditional table. Returns the updated tree plus a propagation journal (per-node before/after deltas). Conditional tables are untouched. Stores the updated tree in the project record."
    )]
    pub async fn scenario_propagate(
        &self,
        Parameters(req): Parameters<PropagateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_propagate", async {
            if req.events.iter().any(|e| e.id == req.event_id && !e.depends_on.is_empty()) {
                return Err(McpToolError::invalid_argument(format!(
                    "event '{}' has parents: its probability comes from its conditional table, so a new prior would change nothing. Revise a root event, or change the conditionals and call scenario_quantify",
                    req.event_id
                )));
            }
            let result = superforecast::propagate_prior_update(&req.events, &req.event_id, req.new_prior)
                .map_err(map_scenario_error)?;

            // Persist the propagated tree so the status snapshot reads the
            // post-update tree.
            self.persist_tree(&result.tree)?;

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
                    "root_ids": result.tree.root_ids,
                    "topo_order": result.tree.topo_order,
                    "joint_probability": result.tree.joint_probability,
                    "nodes": nodes,
                },
                "journal": result.journal,
                "joint_before": result.joint_before,
                "joint_after": result.joint_after,
                "provenance": provenance("scenario_propagate", {
                    let mut m = serde_json::Map::new();
                    m.insert("updated_event".into(), req.event_id.clone().into());
                    m.insert("changed_nodes".into(), result.journal.len().into());
                    m
                }),
                "approximation_note": dependent_parents_note(&result.tree),
                "framework": "Tree-level propagation: the named root event's prior is revised; every descendant marginal is recomputed via CPT marginalization under parent independence; the journal records each changed node.",
            });

            Ok(output)
        })
        .await
    }

    /// Recompute event-tree posteriors under evidence, in BOTH directions
    /// (PR-08): forward to descendants (causal) and backward to ancestors
    /// (diagnostic) — answering "given the leaf was observed, what is the
    /// posterior on the root?", which forward-only propagation cannot.
    #[tool(
        description = "Recompute event-tree posterior probabilities under evidence, in BOTH directions: forward to descendants (causal) and backward to ancestors (diagnostic) — answering 'given the leaf was observed, what is the posterior on the root?', which forward-only propagation cannot. Takes the same valid event tree scenario_quantify accepts plus evidence entries (exactly one of observed_probability, occurred, or likelihood_ratio per entry, and at most one entry per event). Hard evidence clamps the node's marginal; soft evidence (a likelihood ratio) applies a Bayesian update. Uses edge-excluding tree messages on polytrees (singly-connected DAGs). Fractional observed probabilities are fixed marginal constraints fitted within 1e-12; incompatible or unsatisfied evidence returns an explicit error, not an estimate. On multiply-connected DAGs it degrades to forward-only marginalization with a note naming the limitation — never silently. Returns per-node prior marginal, posterior, certainty tier, and the method used. Shares its engine (hkask_forecast::posterior) with the graph widget's interactive re-propagation — one implementation."
    )]
    pub async fn scenario_recompute_posteriors(
        &self,
        Parameters(req): Parameters<PosteriorsRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_recompute_posteriors", async {
            // Same validation contract as scenario_quantify: the events must
            // form a valid tree (probabilities in [0,1], conditional lengths
            // 2^parents, no cycles, no unknown parents).
            let tree = superforecast::build_event_tree(&req.events)
                .map_err(map_scenario_error)?;

            // Convert to the shared posterior engine's neutral nodes — the
            // same engine the graph widget delegates to.
            let nodes: Vec<hkask_forecast::posterior::PosteriorNode> = tree
                .nodes
                .iter()
                .map(|n| hkask_forecast::posterior::PosteriorNode {
                    id: n.event.id.clone(),
                    base_probability: n.event.probability,
                    parents: {
                        let mut seen = std::collections::HashSet::new();
                        n.event
                            .depends_on
                            .iter()
                            .flat_map(|d| d.parent_event_ids.iter())
                            .filter(|p| seen.insert((*p).clone()))
                            .cloned()
                            .collect()
                    },
                    dependencies: n
                        .event
                        .depends_on
                        .iter()
                        .map(|d| hkask_forecast::posterior::PosteriorDependency {
                            parent_ids: d.parent_event_ids.clone(),
                            conditionals: d.conditionals.clone(),
                        })
                        .collect(),
                })
                .collect();
            let id_index: std::collections::HashMap<&str, usize> = nodes
                .iter()
                .enumerate()
                .map(|(i, n)| (n.id.as_str(), i))
                .collect();
            let topo: Vec<usize> = tree
                .topo_order
                .iter()
                .filter_map(|id| id_index.get(id.as_str()).copied())
                .collect();

            // Evidence validation: exactly one of the three fields per entry,
            // and the event must be among the request's events.
            let mut evidence: std::collections::HashMap<usize, hkask_forecast::posterior::Evidence> =
                std::collections::HashMap::new();
            for entry in &req.evidence {
                let idx = *id_index.get(entry.event_id.as_str()).ok_or_else(|| {
                    McpToolError::invalid_argument(format!(
                        "evidence event_id '{}' is not among the request's events",
                        entry.event_id
                    ))
                })?;
                let kind = match (
                    entry.observed_probability,
                    entry.occurred,
                    entry.likelihood_ratio,
                ) {
                    (Some(p), None, None) => {
                        if !(p.is_finite() && (0.0..=1.0).contains(&p)) {
                            return Err(McpToolError::invalid_argument(format!(
                                "observed_probability for event '{}' must be in [0, 1], got {p}",
                                entry.event_id
                            )));
                        }
                        hkask_forecast::posterior::Evidence::Hard(p)
                    }
                    (None, Some(occurred), None) => {
                        hkask_forecast::posterior::Evidence::Hard(if occurred { 1.0 } else { 0.0 })
                    }
                    (None, None, Some(lr)) => {
                        if !(lr.is_finite() && lr >= 0.0) {
                            return Err(McpToolError::invalid_argument(format!(
                                "likelihood_ratio for event '{}' must be finite and >= 0, got {lr}",
                                entry.event_id
                            )));
                        }
                        hkask_forecast::posterior::Evidence::Soft(lr)
                    }
                    _ => {
                        return Err(McpToolError::invalid_argument(format!(
                            "evidence for event '{}' must set exactly one of observed_probability, occurred, likelihood_ratio",
                            entry.event_id
                        )))
                    }
                };
                if evidence.insert(idx, kind).is_some() {
                    return Err(McpToolError::invalid_argument(format!(
                        "event '{}' has more than one evidence entry; give each event at most one",
                        entry.event_id
                    )));
                }
            }

            let polytree = hkask_forecast::posterior::is_polytree(&nodes);
            let (posteriors, method) = if polytree {
                (
                    hkask_forecast::posterior::recompute_posteriors(&nodes, &topo, &evidence)
                        .map_err(|error| {
                            use hkask_forecast::posterior::PosteriorError;
                            match &error {
                                PosteriorError::InvalidModel(_)
                                | PosteriorError::InvalidEvidence(_)
                                | PosteriorError::ImpossibleEvidence =>
                                    McpToolError::invalid_argument(error.to_string()),
                                PosteriorError::UnsatisfiedMarginals =>
                                    McpToolError::failed_precondition(error.to_string()),
                            }
                        })?,
                    "polytree_backward_inference",
                )
            } else {
                (
                    hkask_forecast::posterior::forward_marginals(&nodes, &topo, &evidence),
                    "forward_marginalization_only",
                )
            };

            let output = serde_json::json!({
                "subject": tree.subject,
                "event_count": nodes.len(),
                "evidence_count": evidence.len(),
                "polytree": polytree,
                "method": method,
                "nodes": nodes.iter().enumerate().map(|(i, n)| serde_json::json!({
                    "id": n.id,
                    "question": tree.nodes[i].event.question,
                    "prior_marginal": tree.nodes[i].marginal_probability,
                    "posterior_probability": posteriors[i],
                    "posterior_pct": format!("{:.1}%", posteriors[i] * 100.0),
                    "certainty_tier": hkask_forecast::certainty_tier(posteriors[i]),
                    "evidence_applied": evidence.contains_key(&i),
                })).collect::<Vec<_>>(),
                "note": if polytree { serde_json::Value::Null } else { serde_json::json!(
                    "multiply-connected DAG: backward inference double-counts evidence on non-polytrees; forward marginals only"
                )},
                "provenance": provenance("scenario_recompute_posteriors", {
                    let mut m = serde_json::Map::new();
                    m.insert("engine".into(), "hkask_forecast::posterior (shared with the graph widget)".into());
                    m
                }),
            });

            Ok(output)
        })
        .await
    }

    /// Bayesian update: revise a probability with new evidence.
    #[tool(
        description = "Bayesian update for a scenario event. Apply Bayes' theorem: P(H|E) = P(E|H) × P(H) / P(E). Provide prior probability, evidence likelihood (how likely is the evidence if the hypothesis is true?), and evidence base rate (how common is this evidence in general?). The three must fit one probability model: P(E|H) × P(H) cannot exceed P(E), and such inputs are refused. The posterior is exact and not clamped: a prior of 0 or 1 stays there. Returns the posterior probability and the magnitude of the update. Computes only; nothing is stored, and forecast_id and event_id are echoed back."
    )]
    pub async fn scenario_update(
        &self,
        Parameters(req): Parameters<UpdateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_update", async {
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
            )
            .ok_or_else(|| McpToolError::invalid_argument(format!(
                "evidence_likelihood × prior_probability ({}) exceeds evidence_base_rate ({}): P(E and H) cannot exceed P(E). Check the base rate: P(E) = P(E|H)·P(H) + P(E|not H)·(1 − P(H))",
                req.evidence_likelihood * req.prior_probability,
                req.evidence_base_rate
            )))?;

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

            Ok(output)
        })
        .await
    }

    /// Freeze a prospective scenario forecast in the existing journal.
    #[tool(
        description = "Declare a prospective scenario forecast before resolution. Requires resolution_at mapping exactly every event ID to an RFC 3339 timestamp. Freezes each full event, marginal, precise cutoff and resolver. Admission is strictly before cutoff; the UTC cutoff date must match the descriptive deadline. Complete declarations journal atomically; identical retries preserve original timestamps and p, including after cutoff. Scaffold probabilities with withheld market base rates are ineligible. New probabilities or propositions require a new forecast_id. The resolver is a declared caller-reported source, not authenticated evidence."
    )]
    pub async fn scenario_declare(
        &self,
        Parameters(req): Parameters<DeclareRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_declare", async {
            if req.forecast_id.trim().is_empty() || req.resolver.trim().is_empty() {
                return Err(McpToolError::invalid_argument("forecast_id and resolver must be nonempty"));
            }
            let tree = superforecast::build_event_tree(&req.events).map_err(map_scenario_error)?;
            let mut store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
            require_loaded(store.load_error())?;
            let now = store.now();
            let today = now.date_naive();
            if req.resolution_at.len() != tree.nodes.len() || tree.nodes.iter().any(|n| !req.resolution_at.contains_key(&n.event.id)) {
                return Err(McpToolError::invalid_argument("resolution_at must contain exactly the declared event IDs"));
            }
            let existing: Vec<_> = store.values().filter(|r| r.forecast_id == req.forecast_id).collect();
            if !existing.is_empty() {
                if existing.len() != tree.nodes.len() {
                    return Err(McpToolError::invalid_argument("declaration replay conflicts with frozen membership"));
                }
                for node in &tree.nodes {
                    let record = existing.iter().find(|r| r.event_id == node.event.id)
                        .ok_or_else(|| McpToolError::invalid_argument("declaration replay conflicts with frozen event IDs"))?;
                    let declaration = record.declaration.as_ref()
                        .ok_or_else(|| McpToolError::invalid_argument("legacy identity cannot be redeclared as prospective"))?;
                    if !declaration.is_prospective() || declaration.resolver != req.resolver
                        || declaration.resolution_at != Some(req.resolution_at[&node.event.id])
                        || record.probability != node.marginal_probability
                        || serde_json::to_value(&declaration.event).map_err(|e| McpToolError::internal(e.to_string()))?
                            != serde_json::to_value(&node.event).map_err(|e| McpToolError::internal(e.to_string()))?
                    {
                        return Err(McpToolError::invalid_argument("declaration replay conflicts with frozen proposition, probability, cutoff or resolver"));
                    }
                }
                // The original admission remains authoritative even after cutoff.
                // A previous compaction failure may need publication, not a new intake.
                store.persist().map_err(|e| McpToolError::internal(format!("declaration snapshot retry failed; complete journal remains recoverable: {e}")))?;
                return Ok(serde_json::json!({"forecast_id": req.forecast_id, "status": "declared", "replayed": true, "event_count": tree.nodes.len(), "resolver": req.resolver, "resolution_at": req.resolution_at, "score_convention": "binary_mean_squared_error"}));
            }
            for node in &tree.nodes {
                let event = &node.event;
                let cutoff = req.resolution_at[&event.id];
                if now >= cutoff || event.deadline != cutoff.date_naive() {
                    return Err(McpToolError::invalid_argument("declaration must precede its exact resolution_at cutoff, whose UTC date must match deadline"));
                }
                if event.question.trim().is_empty()
                    || (event.basis.as_deref().is_some_and(|b| b.starts_with("prediction_market:")) && event.base_rate.is_none())
                {
                    return Err(McpToolError::invalid_argument("declaration requires a non-empty question and a declared probability; withheld market scaffolds are ineligible"));
                }
                if store.get(&format!("{}:{}", req.forecast_id, event.id)).is_some() {
                    return Err(McpToolError::invalid_argument("composite forecast identity conflicts with existing history"));
                }
            }
            let records = tree.nodes.iter().map(|node| {
                let event = &node.event;
                (format!("{}:{}", req.forecast_id, event.id), types::StoredForecastRecord {
                    forecast_id: req.forecast_id.clone(),
                    event_id: event.id.clone(),
                    event_name: event.name.clone(),
                    subject: event.subject.clone(),
                    probability: node.marginal_probability,
                    created_at: today,
                    outcome: None,
                    resolved_at: None,
                    declaration: Some(types::ScenarioDeclaration {
                        event: event.clone(),
                        resolver: req.resolver.clone(),
                        declared_at: now.to_rfc3339(),
                        resolution_at: Some(req.resolution_at[&event.id]),
                    }),
                })
            }).collect();
            store.insert_batch(records).map_err(|e| McpToolError::internal(format!("declaration persistence failed; if the journal synced, the complete declaration is recoverable by identical retry: {e}")))?;
            Ok(serde_json::json!({"forecast_id": req.forecast_id, "status": "declared", "replayed": false, "event_count": tree.nodes.len(), "resolver": req.resolver, "resolution_at": req.resolution_at, "score_convention": "binary_mean_squared_error"}))
        }).await
    }

    /// Resolve only a previously declared identity; never create a forecast here.
    #[tool(
        description = "Record outcomes for an existing scenario forecast. Requires exact frozen events and resolver; scores stored marginal probabilities, never resolution-time submissions. Duplicate outcomes and conflicting replays are invalid. Nonmatching outcomes are explicitly listed and ignored. Legacy records lacking declarations remain unscored history. Raw loss does not certify calibration."
    )]
    pub async fn scenario_score(
        &self,
        Parameters(req): Parameters<ScoreRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_score", async {
            superforecast::build_event_tree(&req.events).map_err(map_scenario_error)?;
            let mut seen = std::collections::HashSet::new();
            for outcome in &req.outcomes {
                if !seen.insert(&outcome.event_id) {
                    return Err(McpToolError::invalid_argument(format!("duplicate outcome event_id '{}'", outcome.event_id)));
                }
            }
            let mut store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
            require_loaded(store.load_error())?;
            let records: Vec<_> = store.values().filter(|r| r.forecast_id == req.forecast_id).cloned().collect();
            if records.is_empty() {
                return Err(McpToolError::not_found(format!("forecast '{}' was not declared", req.forecast_id)));
            }
            if records.len() != req.events.len() {
                return Err(McpToolError::invalid_argument("events must exactly match the frozen declaration"));
            }
            let now = store.now();
            let today = now.date_naive();
            // Validate the complete request before any mutation, including replay conflicts.
            for record in &records {
                let event = req.events.iter().find(|e| e.id == record.event_id)
                    .ok_or_else(|| McpToolError::invalid_argument("event identity differs from declaration"))?;
                if let Some(declaration) = &record.declaration {
                    if serde_json::to_value(event).map_err(|e| McpToolError::internal(e.to_string()))?
                        != serde_json::to_value(&declaration.event).map_err(|e| McpToolError::internal(e.to_string()))?
                        || declaration.resolver != req.resolver
                    {
                        return Err(McpToolError::invalid_argument("proposition, horizon, probability or resolver differs from declaration"));
                    }
                    if declaration.resolution_at.is_some() && !declaration.is_prospective() {
                        return Err(McpToolError::invalid_argument("stored declared_at must precede resolution_at; hindsight declarations cannot be scored"));
                    }
                    if req.outcomes.iter().any(|o| o.event_id == record.event_id)
                        && declaration.resolution_at.is_some_and(|cutoff| now < cutoff)
                    {
                        return Err(McpToolError::invalid_argument("outcome cannot resolve before the declared resolution_at deadline"));
                    }
                }
                if let Some(outcome) = req.outcomes.iter().find(|o| o.event_id == record.event_id)
                    && record.outcome.is_some_and(|old| old != outcome.occurred)
                {
                    return Err(McpToolError::invalid_argument("conflicting outcome replay; corrections require a separately sourced version"));
                }
            }
            let unmatched: Vec<_> = req.outcomes.iter().filter(|o| !records.iter().any(|r| r.event_id == o.event_id)).map(|o| &o.event_id).collect();
            let mut per_event = Vec::new();
            let mut scores = Vec::new();
            for mut record in records {
                if let Some(outcome) = req.outcomes.iter().find(|o| o.event_id == record.event_id) {
                    let score = record.declaration.as_ref().filter(|d| d.is_prospective()).map(|_| superforecast::brier_score(record.probability, outcome.occurred));
                    if let Some(score) = score { scores.push(score); }
                    per_event.push(serde_json::json!({"event_id": record.event_id, "forecast_probability": record.probability, "outcome": outcome.occurred, "brier_score": score, "scoring_status": if score.is_some() { "scored" } else { "unscored" }, "unscored_reason": if score.is_none() { Some("legacy record lacks a precise prospective declaration") } else { None }}));
                    if record.outcome.is_none() {
                        record.outcome = Some(outcome.occurred);
                        record.resolved_at = Some(today);
                        store.insert(format!("{}:{}", record.forecast_id, record.event_id), record)
                            .map_err(|e| McpToolError::internal(format!("outcome journal failed; earlier records may be durable: {e}")))?;
                    }
                }
            }
            store.persist().map_err(|e| McpToolError::internal(format!("snapshot failed; journal remains recoverable: {e}")))?;
            let aggregate = (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64);
            Ok(serde_json::json!({"forecast_id": req.forecast_id, "event_count": per_event.len(), "scored_count": scores.len(), "per_event": per_event, "aggregate": {"brier_score": aggregate}, "unmatched_outcome_ids": unmatched, "scored_quantity": "frozen_tree_marginal", "score_convention": "binary_mean_squared_error", "weighting": "one_per_declared_event_version", "baseline": null, "baseline_note": "No matched reference declared; no skill or calibration certification."}))
        }).await
    }

    /// Calibrate a forecast using Fermi decomposition and outside/inside view.
    #[tool(
        description = "Calibrate a forecast probability using Tetlock's methodology. Combines Fermi decomposition (a confidence-weighted mean; at least one sub-question needs confidence above 0) and an optional outside view: base_rate in [0, 1] together with reference_count. Giving only one of the two is refused. Returns a model estimate, not an empirically calibrated probability. Historical correction is withheld until a prospective evaluation policy is approved."
    )]
    pub async fn scenario_calibrate(
        &self,
        Parameters(req): Parameters<CalibrateRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_calibrate", async {
            if req.sub_questions.is_empty() {
                return Err(McpToolError::invalid_argument("at least one sub_question is required"));
            }
            if req.sub_questions.iter().all(|sq| sq.confidence == 0.0) {
                return Err(McpToolError::invalid_argument(
                    "every sub_question has confidence 0, so the weighted mean is undefined: give at least one a confidence above 0",
                ));
            }

            // Stage 1: Fermi decomposition
            let fermi_estimate = superforecast::calibrate_from_fermi(&req.sub_questions)
                .map_err(map_scenario_error)?;

            // Stage 2+3: Outside/inside view blending (if base rate provided)
            let (calibrated, confidence) = match (req.base_rate, req.reference_count) {
                (Some(base_rate), Some(reference_count)) => {
                    require_unit("base_rate", base_rate)?;
                    superforecast::outside_view_adjustment(base_rate, fermi_estimate, reference_count)
                }
                // No base rate: use Fermi estimate directly, lower confidence
                (None, None) => (fermi_estimate, 0.5),
                _ => {
                    return Err(McpToolError::invalid_argument(
                        "base_rate and reference_count go together: supply both for the outside view, or neither",
                    ))
                }
            };

            // No historical correction is deployed without a prospective evaluation policy.
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
                "adjustment_status": "withheld: no validated prospective correction policy",
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
                },
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 4-6"
            });

            Ok(output)
        })
        .await
    }

    /// Synthesize multiple perspectives into one aggregated forecast (dragonfly-eye).
    #[tool(
        description = "Dragonfly-eye synthesis (Tetlock Stage 5). Aggregates multiple independent perspectives on a single event into one calibrated probability. Uses explicit equal weights; historical raw loss is context only. Computes disagreement score (0=consensus, 1=polarized) and identifies the strongest dissenting view. Requires at least 2 perspectives. Returns the aggregated probability, weight distribution, dissent summary, and synthesis quality assessment."
    )]
    pub async fn scenario_synthesize(
        &self,
        Parameters(req): Parameters<SynthesizeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_synthesize", async {
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
                    "weighting": "equal: historical raw losses do not determine weights",
                    "disagreement": "normalized standard deviation of probabilities (0=perfect consensus, 1=maximum polarization)",
                },
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 7 — Dragonfly-Eye"
            });

            Ok(output)
        })
        .await
    }

    /// Compute a calibration curve from stored forecasts.
    #[tool(
        description = "Compute a calibration curve from stored forecasts, optionally filtered by subject. It groups resolved forecasts into 10 probability bins and compares each bin's actual hit rate with its mean forecast probability. signed_probability_error is mean declared p minus observed frequency, not overconfidence; its sign flips under event complementation. Qualifying bins and their sample count are reported. Use after scenario_score to build calibration history."
    )]
    pub async fn scenario_calibration(
        &self,
        Parameters(req): Parameters<CalibrationRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_calibration", async {
            let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
            require_loaded(store.load_error())?;

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
                "pending_forecasts": curve.total_forecasts - curve.resolved_forecasts - curve.unscored_resolved_forecasts,
                "unscored_resolved_forecasts": curve.unscored_resolved_forecasts,
                "overall_brier": curve.overall_brier,
                "signed_probability_error": curve.signed_probability_error,
                "signed_probability_error_sample_count": curve.signed_probability_error_sample_count,
                "signed_probability_error_qualifying_bins": curve.signed_probability_error_qualifying_bins,
                "evidence_status": if curve.signed_probability_error.is_some() { "descriptive_only" } else { "insufficient" },
                "score_convention": "binary_mean_squared_error",
                "weighting": "one_per_declared_event_version",
                "baseline": null,
                "bins": curve.bins.iter().map(|b| {
                    // The hit-rate percentage is withheld below
                    // MIN_N_FOR_HEADLINE and the Wilson interval is served
                    // whenever there is a sample: a bare "67%" from three
                    // forecasts is noise wearing a number, and the interval is
                    // what makes a small bin quotable at all.
                    let hits = b.hit_rate.map(|rate| (rate * b.forecast_count as f64).round() as usize);
                    let hit_rate_ci = hits
                        .and_then(|h| hkask_forecast::wilson_bounds(h, b.forecast_count as usize))
                        .map(|(lo, hi)| serde_json::json!({
                            "low": lo,
                            "high": hi,
                            "low_pct": format!("{:.0}%", lo * 100.0),
                            "high_pct": format!("{:.0}%", hi * 100.0),
                        }));
                    serde_json::json!({
                        "range": b.probability_range,
                        "count": b.forecast_count,
                        "hit_rate": b.hit_rate,
                        "hit_rate_pct": b.hit_rate.filter(|_| b.forecast_count >= hkask_forecast::MIN_N_FOR_HEADLINE as u64).map(|rate| format!("{:.0}%", rate * 100.0)),
                        "hit_rate_ci": hit_rate_ci,
                        "expected_rate": b.expected_rate,
                        "signed_probability_error": b.signed_probability_error,
                        "evidence_status": if b.forecast_count == 0 { "insufficient" } else { "descriptive_only" },
                    })
                }).collect::<Vec<_>>(),
                "guidance": "Descriptive frequencies and raw loss only. Signed errors can cancel; neither zero signed bias nor a sample-count threshold certifies calibration. Compare methods on matched events, lead times and weights against a declared reference. No automatic correction is applied.",
                "sample_unit": "declared_event_version; independent outcome count is not established",
                "binning": "ten equal-width probability bins; expected_rate is the observed mean declared p",
                "signed_probability_error_bin_min_count": types::SIGNED_PROBABILITY_ERROR_MIN_BIN_COUNT,
                "headline_min_bin_count": hkask_forecast::MIN_N_FOR_HEADLINE,
                "interval_note": "Wilson binomial intervals assume independent trials; these intervals do not adjust for dependent events or repeated forecast versions.",
                "reference": "Brier (1950)"
            });

            Ok(output)
        })
        .await
    }

    /// Triage a forecasting question for the Goldilocks zone.
    #[tool(
        description = "Triage a forecasting question (Tetlock Commandment 1). Evaluates whether a question is worth the full superforecasting pipeline. Scores three dimensions: clarity (specificity + deadline), data availability (reference class exists?), and resolution criteria (will we know the answer?). Classifies specification quality into: well_specified (clear question and resolution — a base-rate estimate may suffice), goldilocks (worth the full pipeline), needs_refinement (too vague — sharpen the question)."
    )]
    pub async fn scenario_triage(
        &self,
        Parameters(req): Parameters<TriageRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_triage", async {
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
                    "well_specified" => "Use simple base-rate extrapolation or scenario_calibrate with a well-known reference class. The full pipeline may be overkill.",
                    "goldilocks" => "Run the full pipeline: scenario_build → scenario_calibrate → scenario_quantify. Use Fermi decomposition to break into sub-questions. Set up Bayesian updating via scenario_update.",
                    _ => "Refine the question: (1) add a specific deadline, (2) define clear resolution criteria, (3) identify a reference class. Then re-triage.",
                },
                "reference": "Tetlock & Gardner, Superforecasting (2015), Ch. 3 — Triage and the Goldilocks Zone"
            });

            Ok(output)
        })
        .await
    }

    /// Assess a scenario project across Chermack's five performance phases.
    #[tool(
        description = "Assess a scenario project's effectiveness (Chermack Phase 5), anchored on the project record created by scenario_frame_document (the project id defaults to the subject there; an unknown project id is not found). Assesses the project across all five phases: Preparation (scored from the project's framing document), Exploration (perspective diversity), Development (causal structure — event and dependency counts derived from the project's last quantified tree), Implementation (strategies applied), and Project Assessment (learning + calibration, the curve scoped to the project's subject). Caller-supplied metrics override the derived values and are checked: disagreement_score in [0, 1], events_with_dependencies at most event_count, strategies_implemented at most strategies_generated. Omitted metrics that are neither supplied nor derivable are reported as unreported (unreported_metrics) and their phase scores withheld as insufficient data (null) — never defaulted to zero. Answers Chermack's core question: did the scenario project improve decision quality? Returns per-phase scores, gaps, strengths, learning evidence, and actionable recommendations. Reads only; nothing is stored."
    )]
    pub async fn scenario_assess(
        &self,
        Parameters(req): Parameters<AssessRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "scenario_assess", async {
            // The project record is the assessment's anchor (PR-01): an
            // unknown project id is not_found naming it — assessing a
            // phantom recalls nothing.
            let project = {
                let store = self.project_store.lock().unwrap_or_else(|e| e.into_inner());
                require_loaded(store.load_error())?;
                store.get(&req.project_id).cloned()
            };
            let Some(project) = project else {
                return Err(McpToolError::not_found(format!(
                    "scenario project '{}' not found — create it with scenario_frame_document (the project id defaults to the subject)",
                    req.project_id
                )));
            };

            let learning_events: Vec<String> = req
                .learning_events
                .as_deref()
                .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
                .unwrap_or_default();

            // Calibration evidence is scoped to the project's subject
            // (PR-03): a multi-subject store must not contaminate the
            // Phase-5 assessment with other projects' forecasts.
            let curve = {
                let store = self.forecast_store.lock().unwrap_or_else(|e| e.into_inner());
                require_loaded(store.load_error())?;
                superforecast::compute_calibration_curve(
                    &store.filtered_by_subject(&project.subject),
                )
                .ok()
            };

            // Caller-supplied metrics override; the project record derives
            // the rest (PR-01): event and dependency counts come from the
            // stored tree, Phase 1 from the framing document. An omission
            // is not a zero — what is neither supplied nor derivable is
            // reported as unreported and withholds its phase.
            let event_count = req
                .event_count
                .or_else(|| project.last_tree.as_ref().map(|tree| tree.nodes.len()));
            let events_with_deps = req.events_with_dependencies.or_else(|| {
                project
                    .last_tree
                    .as_ref()
                    .map(|tree| {
                        tree.nodes
                            .iter()
                            .filter(|node| !node.event.depends_on.is_empty())
                            .count()
                    })
            });

            if let Some(disagreement) = req.disagreement_score {
                require_unit("disagreement_score", disagreement)?;
            }
            if let (Some(total), Some(with_deps)) = (event_count, events_with_deps)
                && with_deps > total
            {
                return Err(McpToolError::invalid_argument(format!(
                    "events_with_dependencies ({with_deps}) cannot exceed event_count ({total})"
                )));
            }
            if let (Some(generated), Some(implemented)) = (req.strategies_generated, req.strategies_implemented)
                && implemented > generated
            {
                return Err(McpToolError::invalid_argument(format!(
                    "strategies_implemented ({implemented}) cannot exceed strategies_generated ({generated})"
                )));
            }

            let assessment = superforecast::assess_project(&types::AssessInput {
                project_id: &req.project_id,
                subject: &project.subject,
                framing_document: project.framing_document.as_ref(),
                perspective_count: req.perspective_count,
                disagreement_score: req.disagreement_score,
                event_count,
                events_with_deps,
                calibration_curve: curve.as_ref(),
                strategies_generated: req.strategies_generated,
                strategies_implemented: req.strategies_implemented,
                learning_events: learning_events,
                has_early_warning_indicators: req.has_early_warning_indicators,
            });

            let output = serde_json::json!({
                "project_id": assessment.project_id,
                "subject": assessment.subject,
                "overall_score": assessment.overall_score,
                "overall_pct": assessment.overall_score.map(|s| format!("{:.0}%", s * 100.0)),
                "overall_assessment": assessment.overall_assessment,
                "unreported_metrics": assessment.unreported_metrics,
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
                "learning_evidence": assessment.learning_evidence,
                "recommendations": assessment.recommendations,
                "calibration": curve.map(|c| serde_json::json!({
                    "resolved_forecasts": c.resolved_forecasts,
                    "overall_brier": c.overall_brier,
                    "evidence_status": "descriptive_only",
                })),
                "methodology": {
                    "framework": "Chermack's Performance-Based Scenario System (2011)",
                    "five_phases": [
                        "Phase 1: Project Preparation — scope, stakeholders, resources (Chermack, Ch. 5)",
                        "Phase 2: Scenario Exploration — driving forces, trends, uncertainties (Chermack, Ch. 6)",
                        "Phase 3: Scenario Development — narratives, logic, consistency (Chermack, Ch. 7)",
                        "Phase 4: Scenario Implementation — strategies, wind-tunneling, early warning (Chermack, Ch. 8)",
                        "Phase 5: Project Assessment — learning, performance improvement (Chermack, Ch. 9)",
                    ],
                    "integration": "Combines Chermack (project effectiveness) with Tetlock (forecast accuracy via calibration curve); the Schwartz narrative phases (driving forces, axes, quadrant stories, indicators) live in the scenario-planning skill's template set, quantified here as event trees",
                    "reference": "Chermack, T.J. (2011). Scenario Planning in Organizations: How to Create, Use, and Assess Scenarios. Berrett-Koehler."
                }
            });

            Ok(output)
        })
        .await
    }
}
// ── Tool handler registration ──────────────────────────────────────────────

impl ScenariosServer {
    /// The full registered tool surface: the scenario pipeline router plus
    /// the equity forecast router (ported from the companies server,
    /// Phase A P1). Merged with `+` per the rmcp router-composition
    /// pattern (the companies server's `combined_router()`).
    pub fn combined_router() -> rmcp::handler::server::router::tool::ToolRouter<Self> {
        Self::scenario_router() + Self::equity_forecast_router()
    }
}

#[rmcp::tool_handler(router = Self::combined_router())]
impl rmcp::ServerHandler for ScenariosServer {}

// ── Helpers ────────────────────────────────────────────────────────────────

/// An absent horizon takes the default; a value outside the closed set is
/// refused naming the set, never replaced by the default.
fn parse_time_horizon(value: Option<&str>) -> Result<TimeHorizon, McpToolError> {
    let Some(value) = value else {
        return Ok(TimeHorizon::Strategic);
    };
    TimeHorizon::parse(value).ok_or_else(|| {
        McpToolError::invalid_argument(format!(
            "time_horizon '{value}' must be one of: {}",
            TIME_HORIZONS.join(", ")
        ))
    })
}

fn parse_scenario_type(value: Option<&str>) -> Result<ScenarioType, McpToolError> {
    let Some(value) = value else {
        return Ok(ScenarioType::CompanyAnalysis);
    };
    ScenarioType::parse(value).ok_or_else(|| {
        McpToolError::invalid_argument(format!(
            "scenario_type '{value}' must be one of: {}",
            SCENARIO_TYPES.join(", ")
        ))
    })
}

fn require_subject(subject: &str) -> Result<(), McpToolError> {
    if subject.trim().is_empty() {
        return Err(McpToolError::invalid_argument("subject must be non-empty"));
    }
    Ok(())
}

// ── Pin tests ───────────────────────────────────────────────────────────────
// The pin test enforces the advertised invariant: the tool surface count.

#[cfg(test)]
mod tests {
    use super::*;

    // The scenarios server registers exactly 22 tools. Adding, removing,
    // or renaming a tool is an intentional surface change — the pin catches
    // accidental drift. One macro call: the count test plus the
    // build.rs-generated TOOL_NAMES set against the live router.
    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));
    hkask_mcp_server::tool_surface_pin!(
        ScenariosServer::combined_router(),
        "combined_router",
        22,
        tool_surface_is_exactly_22_registered_tools,
    );

    /// Supplied framing answers are independent of process call history.
    #[tokio::test]
    async fn supplied_framing_answers_are_independent_of_call_history() {
        use std::sync::{Arc, Mutex};
        let directory = tempfile::tempdir().expect("project directory");
        let projects_path = directory.path().join("projects.json");
        let server = ScenariosServer::new(
            hkask_types::WebID::new(),
            Arc::new(Mutex::new(ForecastStore::new(None))),
            Arc::new(Mutex::new(JournalStore::new(None))),
            Mutex::new(ProjectStore::new(Some(projects_path.clone()))),
        );
        let parse = |output: &str| {
            hkask_types::tool_response::parse_tool_response(output).expect("valid tool response")
        };
        let answers = serde_json::json!({
            "focal_question": "Will ACME's new product reach scale by 2027?",
            "decision_at_stake": "Whether to expand production",
            "time_horizon": "tactical",
            "in_scope": ["product demand"],
            "out_of_scope": ["acquisitions"]
        });
        let mut expected_document = None;
        for project_id in ["direct", "after-guidance"] {
            let output = server
                .scenario_frame_document(Parameters(FrameDocumentRequest {
                    subject: "ACME".into(),
                    project_id: Some(project_id.into()),
                    answers: answers.clone().into(),
                }))
                .await
                .expect("supplied framing answers");
            let parsed = parse(&output);
            assert!(parsed.get("sequence_note").is_none(), "{parsed}");
            assert_eq!(parsed["project_id"], project_id);
            assert_eq!(parsed["persisted"], true);
            assert_eq!(
                parsed["framing_document"]["focal_question"],
                answers["focal_question"]
            );
            if let Some(expected) = &expected_document {
                assert_eq!(&parsed["framing_document"], expected);
            } else {
                expected_document = Some(parsed["framing_document"].clone());
                let protocol = parse(
                    &server
                        .scenario_frame(Parameters(FrameRequest {
                            subject: "A different project".into(),
                            prior_answers: Some(answers.clone().into()),
                        }))
                        .await
                        .expect("framing guidance"),
                );
                assert_eq!(
                    protocol["conversation_flow"]
                        .as_array()
                        .expect("turns")
                        .len(),
                    7
                );
                assert_eq!(protocol["prior_answers"], answers);
                assert!(protocol.get("sequence_note").is_none(), "{protocol}");
            }
        }
        let error = server
            .scenario_frame_document(Parameters(FrameDocumentRequest {
                subject: " ".into(),
                project_id: Some("invalid".into()),
                answers: answers.clone().into(),
            }))
            .await
            .expect_err("invalid subject must still fail");
        assert_eq!(error.kind, hkask_types::McpErrorKind::InvalidArgument);
        drop(server);
        let restarted = ProjectStore::new(Some(projects_path));
        for project_id in ["direct", "after-guidance"] {
            let document = restarted
                .get(project_id)
                .expect("persisted project")
                .framing_document
                .as_ref()
                .expect("persisted frame");
            assert_eq!(document.focal_question, answers["focal_question"]);
        }
        assert!(restarted.get("invalid").is_none());
    }

    /// S-02 (mcp-tool-review): a missing-state condition the caller can
    /// remedy surfaces as `failed_precondition`, never `internal`. An empty
    /// forecast store is exactly that condition — the caller records and
    /// resolves forecasts, then retries — so `scenario_calibration` must
    /// classify it accordingly and name the missing state in the message.
    #[tokio::test]
    async fn scenario_calibration_without_forecasts_is_failed_precondition() {
        let server = ScenariosServer::new(
            hkask_types::WebID::new(),
            std::sync::Arc::new(std::sync::Mutex::new(superforecast::ForecastStore::new(
                None,
            ))),
            std::sync::Arc::new(std::sync::Mutex::new(superforecast::JournalStore::new(
                None,
            ))),
            std::sync::Mutex::new(superforecast::ProjectStore::new(None)),
        );
        let error = server
            .scenario_calibration(rmcp::handler::server::wrapper::Parameters(
                crate::requests::CalibrationRequest { subject: None },
            ))
            .await
            .expect_err("calibrating an empty store must fail");
        assert!(
            matches!(error.kind, hkask_types::McpErrorKind::FailedPrecondition),
            "a missing-state condition the caller can remedy is failed_precondition, \
             never internal, got: {error:?}"
        );
        assert!(
            error.message.contains("scenario_declare") && error.message.contains("scenario_score"),
            "the error must name the missing state and its remedy, got: {error:?}"
        );
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
                std::sync::Arc::new(std::sync::Mutex::new(superforecast::ForecastStore::new(
                    // The store wants the snapshot FILE path — passing the
                    // dir alone made production compaction rename onto a
                    // directory (failing past the journal threshold) and
                    // dropped the journal outside the D28 scenarios dir.
                    Some(scenarios_data_dir.join("forecasts.json")),
                ))),
                std::sync::Arc::new(std::sync::Mutex::new(superforecast::JournalStore::new(
                    // Equity forecasts (ported from the companies server,
                    // Phase A P1): separate snapshot file, same D28 data dir
                    // and journal discipline. Never share the snapshot file
                    // with the scenario forecast store — the two record
                    // histories never mix.
                    Some(scenarios_data_dir.join("equity_forecasts.json")),
                ))),
                std::sync::Mutex::new(superforecast::ProjectStore::new(Some(
                    scenarios_data_dir.join("projects.json"),
                ))),
            ))
        },
        vec![],
    )
    .await
}
