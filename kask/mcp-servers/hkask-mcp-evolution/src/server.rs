//! Evolution MCP server — the experiment registry (repair plan §P8).
//!
//! One server owns the durable record of evolution experiments: declaration
//! with a pre-registered prediction, variant registration with lineage,
//! grounded fitness (recorded report refs only), selection fossils, lineage
//! reads, and population queries. The registry is the single record path —
//! superseded retention mechanisms are deleted in the same change that
//! lands this server, never run alongside it (§P8.7-Q5).

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, execute_tool, resolve_db_passphrase};
use hkask_types::AnyJsonValue;
use hkask_types::McpErrorKind;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::store::EvolutionStore;
use crate::types::{EvolutionError, LAYERS, Prediction, STATUSES};

hkask_mcp_server::mcp_server!(
    pub struct EvolutionServer {
        pub store: Arc<EvolutionStore>,
    }
);

/// Classify [`EvolutionError`] for MCP dispatch: each variant maps to a
/// distinct `McpToolError` kind so callers can distinguish "bad input" from
/// "no such record" from "resolved experiment" from "store failure" — never
/// a blanket internal (project rule).
pub fn map_evolution_error(error: EvolutionError) -> McpToolError {
    match error {
        EvolutionError::ExperimentNotFound(_) | EvolutionError::VariantNotFound(_) => {
            McpToolError::not_found(error.to_string())
        }
        EvolutionError::ExperimentResolved(_) => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        EvolutionError::VariantNotInExperiment(..)
        | EvolutionError::UnknownLayer(_)
        | EvolutionError::ConfidenceOutOfRange(_)
        | EvolutionError::BudgetMissingMaxRuns(_)
        | EvolutionError::RunCountInvalid(_)
        | EvolutionError::UnknownVerdict(_)
        | EvolutionError::UnknownStatus(_)
        | EvolutionError::SelectedWithoutVariant
        | EvolutionError::RejectedWithoutReasons
        | EvolutionError::Empty(_)
        | EvolutionError::BadTimestamp(_) => McpToolError::invalid_argument(error.to_string()),
        EvolutionError::BudgetExhausted(..) => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        EvolutionError::Database(_) | EvolutionError::Serialization(_) => {
            McpToolError::internal(error.to_string())
        }
    }
}

/// Reject empty/whitespace-only required string fields as invalid_argument.
fn require_non_empty(label: &'static str, value: &str) -> Result<(), McpToolError> {
    if value.trim().is_empty() {
        return Err(McpToolError::invalid_argument(
            EvolutionError::Empty(label).to_string(),
        ));
    }
    Ok(())
}

// ── Request types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExperimentProposeRequest {
    /// The falsifiable hypothesis the experiment tests.
    pub hypothesis: String,
    /// The artifact layer: skill, agent_card, prompt_template, tool_schema,
    /// regulation_scalar, or lora_adapter.
    pub layer: String,
    /// References to the genotype artifacts — for the composite skill
    /// genotype, the SKILL.md and its template paths together (§P8.7-Q2).
    pub genotype_refs: Vec<String>,
    /// The eval-set declaration (arbitrary JSON): fixed task set, pin
    /// suites, disjoint feedback/selection splits.
    pub eval_set: AnyJsonValue,
    /// The declared fitness function — its name and how it is computed.
    pub fitness_fn: String,
    /// The noise band within which score differences count as ties (e.g.
    /// {"cost_band_pct": 10}).
    pub noise_band: AnyJsonValue,
    /// The pre-registered prediction — the tiny controller (§P8.1). Its
    /// confidence must be in [0, 1].
    pub prediction: Prediction,
    /// The energy-budget declaration (arbitrary JSON): the spend ceiling
    /// for auto-run (§P8.7-Q1). **Must carry `max_runs` — a positive integer
    /// the server enforces at `fitness_record` (§P8.9 step 2, D-1): the
    /// declared budget is the Layer-A set point, and recording past it is a
    /// typed refusal, never a silent overshoot.
    pub budget: AnyJsonValue,
    /// The linked kanban goal (§P8.9 step 5) — the goal loop Brier-scores the
    /// pre-registered claim against the measured outcome; this field is the
    /// explicit registry ↔ goal-loop join.
    pub linked_goal_id: Option<String>,
    /// Optional replay-convergence key: a retried call returns the existing
    /// experiment instead of duplicating it.
    pub experiment_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VariantRegisterRequest {
    pub experiment_id: String,
    /// The variant's genotype configuration (arbitrary JSON — e.g. the
    /// composite (SKILL.md, templates) pair or the agent-card config).
    pub genotype_config: AnyJsonValue,
    /// The parent variant this one derives from — lineage (§P8.1).
    pub parent_variant_id: Option<String>,
    /// Optional replay-convergence key, unique within the experiment.
    pub variant_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FitnessRecordRequest {
    pub experiment_id: String,
    pub variant_id: String,
    /// Recorded report references only — never simulated (§P8.3). Each
    /// entry names a real harness/evaluator report (e.g. a
    /// swarm_eval_agent_local run).
    pub runs: Vec<String>,
    /// The number of rollouts the referenced reports cover (§P8.9 step 2).
    /// The server sums recorded run counts against the experiment's declared
    /// `budget.max_runs` ceiling and refuses the record with a typed
    /// `budget_exhausted` error when it would exceed it.
    pub run_count: u64,
    /// Per-objective scores computed from those runs (arbitrary JSON).
    pub scores: AnyJsonValue,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelectionRecordRequest {
    pub experiment_id: String,
    /// "selected" or "rejected".
    pub verdict: String,
    /// Required when verdict is selected: the winning variant.
    pub selected_variant_id: Option<String>,
    /// Required when verdict is rejected: why it was rejected — the
    /// fossil's reasons.
    pub reject_reasons: Vec<String>,
    /// The algedonic review record that chaired the selection (§P8.7-Q4).
    pub algedonic_reference: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LineageReadRequest {
    /// A variant id or an experiment id.
    pub artifact_ref: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PopulationQueryRequest {
    /// Filter by artifact layer.
    pub layer: Option<String>,
    /// Filter by status: proposed, running, or resolved.
    pub status: Option<String>,
    /// Only experiments created at or after this RFC 3339 timestamp.
    pub created_since: Option<String>,
    /// Cap on returned experiments (default 50, max 200).
    pub limit: Option<u32>,
}

// ── Tool router ─────────────────────────────────────────────────────

#[tool_router(router = evolution_router, vis = "pub")]
impl EvolutionServer {
    #[tool(
        description = "Register an evolution experiment (protocol step 1, Declare): hypothesis, artifact layer (skill | agent_card | prompt_template | tool_schema | regulation_scalar | lora_adapter), genotype refs, eval set, fitness function, noise band, pre-registered prediction with confidence in [0,1], and an energy budget that MUST carry max_runs (a positive integer the server enforces at fitness_record — recording past the ceiling is a typed budget_exhausted refusal, §P8.9 step 2). Optional linked_goal_id joins the experiment to the kanban goal that Brier-scores its claim. Returns the experiment record with status proposed. Optional experiment_key converges retried calls onto the existing record instead of duplicating it."
    )]
    pub async fn experiment_propose(
        &self,
        Parameters(ExperimentProposeRequest {
            hypothesis,
            layer,
            genotype_refs,
            eval_set,
            fitness_fn,
            noise_band,
            prediction,
            budget,
            linked_goal_id,
            experiment_key,
        }): Parameters<ExperimentProposeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "experiment_propose", async {
            require_non_empty("hypothesis", &hypothesis)?;
            if !LAYERS.contains(&layer.as_str()) {
                return Err(map_evolution_error(EvolutionError::UnknownLayer(layer)));
            }
            if genotype_refs.is_empty() || genotype_refs.iter().any(|r| r.trim().is_empty()) {
                return Err(map_evolution_error(EvolutionError::Empty("genotype_refs")));
            }
            require_non_empty("fitness_fn", &fitness_fn)?;
            require_non_empty("prediction.claim", &prediction.claim)?;
            if !(0.0..=1.0).contains(&prediction.confidence) {
                return Err(map_evolution_error(EvolutionError::ConfidenceOutOfRange(
                    prediction.confidence,
                )));
            }
            // §P8.9 step 2 (D-1: enforce): the declared budget must carry a
            // machine-readable run ceiling — the Layer-A set point.
            let max_runs = budget
                .get("max_runs")
                .and_then(serde_json::Value::as_u64)
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    map_evolution_error(EvolutionError::BudgetMissingMaxRuns(budget.to_string()))
                })?;
            let record = self
                .store
                .propose_experiment(
                    &hypothesis,
                    &layer,
                    &genotype_refs,
                    &eval_set,
                    &fitness_fn,
                    &noise_band,
                    &prediction,
                    &budget,
                    max_runs,
                    linked_goal_id.as_deref(),
                    experiment_key.as_deref(),
                )
                .map_err(map_evolution_error)?;
            Ok(serde_json::json!({ "status": "proposed", "experiment": record }))
        })
        .await
    }

    #[tool(
        description = "Register a variant inside an experiment (protocol step 2, Vary): the genotype configuration (arbitrary JSON — e.g. the composite (SKILL.md, templates) pair or the agent-card config), an optional parent variant for lineage, and an optional variant_key for replay convergence. The first variant moves the experiment to running. A resolved experiment no longer accepts variants."
    )]
    pub async fn variant_register(
        &self,
        Parameters(VariantRegisterRequest {
            experiment_id,
            genotype_config,
            parent_variant_id,
            variant_key,
        }): Parameters<VariantRegisterRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "variant_register", async {
            require_non_empty("experiment_id", &experiment_id)?;
            let record = self
                .store
                .register_variant(
                    &experiment_id,
                    &genotype_config,
                    parent_variant_id.as_deref(),
                    variant_key.as_deref(),
                )
                .map_err(map_evolution_error)?;
            Ok(serde_json::json!({ "status": "registered", "variant": record }))
        })
        .await
    }

    #[tool(
        description = "Record grounded fitness for a variant (protocol step 3, Test): runs are recorded report references ONLY — never simulated; each entry names a real harness or evaluator report (e.g. a swarm_eval_agent_local run) — plus run_count (the number of rollouts those reports cover; the server sums run counts against the experiment's declared budget.max_runs and refuses with a typed budget_exhausted error when the record would exceed the ceiling, §P8.9 step 2) and per-objective scores computed from those runs. Immutable once written. A resolved experiment no longer accepts fitness."
    )]
    pub async fn fitness_record(
        &self,
        Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs,
            run_count,
            scores,
        }): Parameters<FitnessRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fitness_record", async {
            require_non_empty("experiment_id", &experiment_id)?;
            require_non_empty("variant_id", &variant_id)?;
            if runs.is_empty() || runs.iter().any(|r| r.trim().is_empty()) {
                return Err(map_evolution_error(EvolutionError::Empty("runs")));
            }
            if run_count == 0 {
                return Err(map_evolution_error(EvolutionError::RunCountInvalid(
                    run_count,
                )));
            }
            let record = self
                .store
                .record_fitness(&experiment_id, &variant_id, &runs, run_count, &scores)
                .map_err(map_evolution_error)?;
            Ok(serde_json::json!({ "status": "recorded", "fitness": record }))
        })
        .await
    }

    #[tool(
        description = "Record the selection verdict for an experiment (protocol step 4, Select): verdict selected (naming the winning variant) or rejected (with at least one reject reason). Both outcomes persist as fossils — selected and rejected are both retained. The first selection resolves the experiment; a further selection is a failed_precondition conflict. algedonic_reference names the review record that chaired the selection."
    )]
    pub async fn selection_record(
        &self,
        Parameters(SelectionRecordRequest {
            experiment_id,
            verdict,
            selected_variant_id,
            reject_reasons,
            algedonic_reference,
        }): Parameters<SelectionRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "selection_record", async {
            require_non_empty("experiment_id", &experiment_id)?;
            require_non_empty("verdict", &verdict)?;
            let (record, experiment) = self
                .store
                .record_selection(
                    &experiment_id,
                    &verdict,
                    selected_variant_id.as_deref(),
                    &reject_reasons,
                    algedonic_reference.as_deref(),
                )
                .map_err(map_evolution_error)?;
            Ok(serde_json::json!({
                "status": "resolved",
                "selection": record,
                "experiment": experiment,
            }))
        })
        .await
    }

    #[tool(
        description = "Read the lineage of a variant or experiment: for a variant id, the ancestry chain (oldest parent first) with the experiment, its fitness records, and its selection events; for an experiment id, the experiment with all variants, fitness, and selections. The fossil record — rejected variants and their reasons are retained for consultation, so a rejected mutation is never blindly retried."
    )]
    pub async fn lineage_read(
        &self,
        Parameters(LineageReadRequest { artifact_ref }): Parameters<LineageReadRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "lineage_read", async {
            require_non_empty("artifact_ref", &artifact_ref)?;
            match self.store.variant_by_id(&artifact_ref) {
                Ok(variant) => {
                    let ancestry = self
                        .store
                        .variant_ancestry(&artifact_ref)
                        .map_err(map_evolution_error)?;
                    let experiment = self
                        .store
                        .experiment_by_id(&variant.experiment_id)
                        .map_err(map_evolution_error)?;
                    let fitness = self
                        .store
                        .fitness_for_experiment(&variant.experiment_id)
                        .map_err(map_evolution_error)?;
                    let selections = self
                        .store
                        .selections_for_experiment(&variant.experiment_id)
                        .map_err(map_evolution_error)?;
                    Ok(serde_json::json!({
                        "kind": "variant",
                        "ancestry": ancestry,
                        "experiment": experiment,
                        "fitness": fitness,
                        "selections": selections,
                    }))
                }
                Err(EvolutionError::VariantNotFound(_)) => {
                    let experiment = self
                        .store
                        .experiment_by_id(&artifact_ref)
                        .map_err(map_evolution_error)?;
                    let variants = self
                        .store
                        .variants_for_experiment(&artifact_ref)
                        .map_err(map_evolution_error)?;
                    let fitness = self
                        .store
                        .fitness_for_experiment(&artifact_ref)
                        .map_err(map_evolution_error)?;
                    let selections = self
                        .store
                        .selections_for_experiment(&artifact_ref)
                        .map_err(map_evolution_error)?;
                    Ok(serde_json::json!({
                        "kind": "experiment",
                        "experiment": experiment,
                        "variants": variants,
                        "fitness": fitness,
                        "selections": selections,
                    }))
                }
                Err(error) => Err(map_evolution_error(error)),
            }
        })
        .await
    }

    #[tool(
        description = "Query the experiment population: optional layer, status (proposed | running | resolved), created_since (RFC 3339), and a limit (default 50, max 200). Returns experiments newest first — the registry's population view for the algedonic agenda and the curator's ORIENT."
    )]
    pub async fn population_query(
        &self,
        Parameters(PopulationQueryRequest {
            layer,
            status,
            created_since,
            limit,
        }): Parameters<PopulationQueryRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "population_query", async {
            if let Some(value) = &layer {
                if !LAYERS.contains(&value.as_str()) {
                    return Err(map_evolution_error(EvolutionError::UnknownLayer(
                        value.clone(),
                    )));
                }
            }
            if let Some(value) = &status {
                if !STATUSES.contains(&value.as_str()) {
                    return Err(map_evolution_error(EvolutionError::UnknownStatus(
                        value.clone(),
                    )));
                }
            }
            if let Some(value) = &created_since {
                chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
                    map_evolution_error(EvolutionError::BadTimestamp(value.clone()))
                })?;
            }
            let limit = limit.unwrap_or(50).clamp(1, 200);
            let experiments = self
                .store
                .population(
                    layer.as_deref(),
                    status.as_deref(),
                    created_since.as_deref(),
                    limit,
                )
                .map_err(map_evolution_error)?;
            Ok(serde_json::json!({
                "count": experiments.len(),
                "experiments": experiments,
            }))
        })
        .await
    }
}

#[tool_handler(router = Self::evolution_router())]
impl rmcp::ServerHandler for EvolutionServer {}

/// Start the server: the registry lives in the per-agent evolution database
/// under the hKask data dir (`HKASK_EVOLUTION_DB` overrides the path), opened
/// through the canonical passphrase chain — a missing `HKASK_DB_PASSPHRASE`
/// fails startup visibly, never as an empty-key open.
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-evolution",
        env!("CARGO_PKG_VERSION"),
        |ctx: hkask_mcp_server::ServerContext| {
            let registry_path = crate::registry_path();
            // First boot: the registry's parent directory may not exist yet
            // (the default lives under nested per-agent data dirs). Create
            // it up front so the failure mode is a real DB error, not a
            // missing-directory error; either way the open surfaces it
            // visibly.
            if let Some(Err(error)) = registry_path.parent().map(std::fs::create_dir_all) {
                tracing::warn!(
                    target: "hkask.mcp.evolution",
                    path = %registry_path.display(),
                    %error,
                    "Failed to create the evolution DB directory \
                     — the subsequent DB open will surface the failure"
                );
            }
            let db_path = registry_path.to_string_lossy().to_string();
            // Canonical 2-tier passphrase chain (ctx.credentials → env →
            // keychain). See `hkask_mcp_server::server::resolve_db_passphrase`.
            // Startup failures map to Infrastructure — visible and named, never
            // a silent empty-key open.
            let infrastructure = |message: String| {
                hkask_mcp_server::McpError::Infrastructure(hkask_types::InfrastructureError::Io(
                    message,
                ))
            };
            let passphrase = resolve_db_passphrase(&ctx.credentials)
                .map_err(|error| infrastructure(format!("HKASK_DB_PASSPHRASE: {error}")))?;
            let db = hkask_storage::open_or_repair(&db_path, &passphrase)
                .map_err(|error| infrastructure(error.to_string()))?;
            let pool = db
                .sqlite_pool()
                .map_err(|error| infrastructure(format!("sqlite pool: {error}")))?;
            let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> = Arc::new(
                hkask_storage::database::sqlite::SqliteDriver::new_labeled(pool, db_path.as_str()),
            );
            let store = Arc::new(
                EvolutionStore::with_driver(driver)
                    .map_err(|error| infrastructure(format!("evolution store init: {error}")))?,
            );
            Ok(EvolutionServer::new(ctx.webid, store))
        },
        vec![hkask_mcp_server::CredentialRequirement::required(
            "HKASK_DB_PASSPHRASE",
            "SQLCipher encryption passphrase for the evolution experiment registry — the server refuses to start without it (the registry is the record of record from day one; no legacy-import path)",
        )],
    )
    .await
}
