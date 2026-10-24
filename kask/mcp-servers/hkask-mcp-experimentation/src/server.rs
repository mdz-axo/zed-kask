//! Experimentation MCP server — the experiment registry (repair plan §P8).
//!
//! One server owns the durable record of experiments: declaration
//! with a pre-registered prediction, variant registration with lineage,
//! fitness records (caller-asserted report refs, stored not verified),
//! selection fossils, lineage reads, and population queries. The registry is the single record path —
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

use crate::store::ExperimentationStore;
use crate::types::{
    ExperimentStatus, ExperimentationError, LAYERS, Prediction, Rubric, ScientificHeadroom,
    VARIANT_ROLE_CHALLENGER, validate_design_spec, validate_rubrics, validate_scientific_headroom,
};

hkask_mcp_server::mcp_server!(
    pub struct ExperimentationServer {
        pub store: Arc<ExperimentationStore>,
    }
);

/// Classify [`ExperimentationError`] for MCP dispatch: each variant maps to a
/// distinct `McpToolError` kind so callers can distinguish "bad input" from
/// "no such record" from "resolved experiment" from "store failure" — never
/// a blanket internal (project rule).
pub fn map_experimentation_error(error: ExperimentationError) -> McpToolError {
    match error {
        ExperimentationError::ExperimentNotFound(_) | ExperimentationError::VariantNotFound(_) => {
            McpToolError::not_found(error.to_string())
        }
        ExperimentationError::ExperimentResolved(_) => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        ExperimentationError::VariantNotInExperiment(..)
        | ExperimentationError::UnknownLayer(_)
        | ExperimentationError::ConfidenceOutOfRange(_)
        | ExperimentationError::BudgetMissingMaxRuns(_)
        | ExperimentationError::RunCountInvalid(_)
        | ExperimentationError::UnknownVerdict(_)
        | ExperimentationError::SelectedWithoutVariant
        | ExperimentationError::RejectedWithoutReasons
        | ExperimentationError::Empty(_)
        | ExperimentationError::BadTimestamp(_)
        | ExperimentationError::RubricsRequired
        | ExperimentationError::RubricsMissingKind(_)
        | ExperimentationError::UnknownRubricKind(_)
        | ExperimentationError::DuplicateRubricId(_)
        | ExperimentationError::RubricIdInvalid(_)
        | ExperimentationError::UnknownVariantRole(_)
        | ExperimentationError::AblationWithoutParent
        | ExperimentationError::UnknownScientificOutcome(_)
        | ExperimentationError::SelectionResolutionConflict(..)
        | ExperimentationError::DesignSpecInvalid(_) => {
            McpToolError::invalid_argument(error.to_string())
        }
        ExperimentationError::DuplicateBaseline(..) => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        ExperimentationError::BudgetExhausted(..) => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        ExperimentationError::AlreadySealed(..)
        | ExperimentationError::ExperimentNotSealed(_)
        | ExperimentationError::SealCitationMissing(..)
        | ExperimentationError::NoHeadroom(_)
        | ExperimentationError::SealAttestationConflict
        | ExperimentationError::ScientificHeadroomIncomplete
        | ExperimentationError::SealFitnessMissing => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        ExperimentationError::SealBaselineMismatch(..)
        | ExperimentationError::SealMismatch(..)
        | ExperimentationError::SealPassRateInvalid(_)
        | ExperimentationError::RunRefsNotDisjoint(_) => {
            McpToolError::invalid_argument(error.to_string())
        }
        ExperimentationError::Database(_) | ExperimentationError::Serialization(_) => {
            McpToolError::internal(error.to_string())
        }
    }
}

/// Reject empty/whitespace-only required string fields as invalid_argument.
fn require_non_empty(label: &'static str, value: &str) -> Result<(), McpToolError> {
    if value.trim().is_empty() {
        return Err(McpToolError::invalid_argument(
            ExperimentationError::Empty(label).to_string(),
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
    /// regulation_scalar, or scientific_hypothesis (the
    /// general lab notebook — D2, operator ruling 2026-10-15).
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
    /// The typed rubric bundle — the machine-checkable eval contract
    /// (P1). Non-empty; at least one `metric` rubric and at least one
    /// `implementation` or `grounded_report` rubric; stable snake_case
    /// ids; verification uses the same vocabulary as
    /// `swarm_evaluate_local`'s evaluators (contains / not_contains /
    /// regex) plus `report_ref`.
    pub rubrics: Vec<Rubric>,
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
    /// The optional typed, versioned scientific `DesignSpec` (task
    /// `f1a90cd2`) — a JSON object carrying a non-empty `spec_version`. When
    /// present it is stored, exposed through lineage/population reads, and
    /// joins the seal fingerprint. Omit for legacy artifact experiments.
    pub design_spec: Option<AnyJsonValue>,
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
    /// baseline | challenger | ablation (P4). Defaults to challenger.
    /// Exactly one baseline is allowed per experiment; an ablation must
    /// name its parent_variant_id — the variant whose component it ablates.
    pub role: Option<String>,
    /// Optional replay-convergence key, unique within the experiment.
    pub variant_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExperimentSealRequest {
    pub experiment_id: String,
    /// The baseline variant the seal attests (must carry the baseline
    /// role, P4).
    pub baseline_variant_id: String,
    /// The measured baseline pass rate from the recorded baseline
    /// fitness — must be < 1.0 (the headroom pre-check, §5 Q6). A
    /// saturated baseline (1.0) is a typed no-headroom refusal: void the
    /// experiment via selection_record with a no-headroom reject reason,
    /// redesign the eval set, and re-declare. Supply exactly one of this
    /// and `scientific_headroom`.
    pub baseline_pass_rate: Option<f64>,
    /// The tagged scientific headroom criterion (task `f1a90cd2`) for a
    /// design family with no meaningful binary baseline pass rate —
    /// single-arm calibration, observational estimation, equivalence. The
    /// honest alternative to fabricating a pass rate. Supply exactly one of
    /// this and `baseline_pass_rate`.
    pub scientific_headroom: Option<ScientificHeadroom>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FitnessRecordRequest {
    pub experiment_id: String,
    pub variant_id: String,
    /// Recorded report references (§P8.3). Each entry is expected to name a
    /// real harness/evaluator report (e.g. a swarm_eval_agent_local run) —
    /// the caller's assertion; this server stores refs, it does not verify them.
    pub runs: Vec<String>,
    /// The number of rollouts the referenced reports cover (§P8.9 step 2).
    /// The server sums recorded run counts against the experiment's declared
    /// `budget.max_runs` ceiling and refuses the record with a typed
    /// `budget_exhausted` error when it would exceed it.
    pub run_count: u64,
    /// Per-objective scores computed from those runs (arbitrary JSON).
    pub scores: AnyJsonValue,
    /// The seal this record cites (P2) — required once the experiment is
    /// sealed; NULL only for the baseline's own pre-seal measurement.
    pub seal_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FinalEvaluationRecordRequest {
    pub experiment_id: String,
    pub variant_id: String,
    /// Recorded report references for the selection set — must be disjoint
    /// from every run ref the experiment has already recorded (the server
    /// enforces this; overlaps are a typed refusal naming them).
    pub runs: Vec<String>,
    /// The number of rollouts the referenced reports cover — counts
    /// against the declared budget.max_runs together with all fitness
    /// records.
    pub run_count: u64,
    /// Per-objective scores computed from those runs (arbitrary JSON).
    pub scores: AnyJsonValue,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SelectionRecordRequest {
    pub experiment_id: String,
    /// "selected" or "rejected" — the legacy binary verdict for an artifact
    /// experiment. Supply exactly one of this and `scientific_outcome`.
    pub verdict: Option<String>,
    /// Required when verdict is selected: the winning variant.
    pub selected_variant_id: Option<String>,
    /// Required when verdict is rejected: why it was rejected — the
    /// fossil's reasons.
    pub reject_reasons: Vec<String>,
    /// The algedonic review record that chaired the selection (§P8.7-Q4).
    pub algedonic_reference: Option<String>,
    /// The typed scientific outcome (task `f1a90cd2`) when this selection
    /// resolves a scientific design rather than an artifact experiment:
    /// rejected | corroborated | evidence_of_practical_equivalence |
    /// inconclusive | measurement_void | design_void. Only `rejected` and
    /// `corroborated` enter Brier calibration; the rest are excluded on this
    /// typed field. Supply exactly one of this and `verdict`.
    pub scientific_outcome: Option<String>,
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
    pub status: Option<ExperimentStatus>,
    /// Only experiments created at or after this RFC 3339 timestamp.
    pub created_since: Option<String>,
    /// Cap on returned experiments (default 50, max 200).
    pub limit: Option<u32>,
}

// ── Tool router ─────────────────────────────────────────────────────

#[tool_router(router = experimentation_router, vis = "pub")]
impl ExperimentationServer {
    #[tool(
        description = "Register an experiment (protocol step 1, Declare): hypothesis, artifact layer (skill | agent_card | prompt_template | tool_schema | regulation_scalar | scientific_hypothesis), genotype refs, eval set, fitness function, noise band, a typed rubric bundle (the machine-checkable eval contract: non-empty, at least one metric rubric and one implementation or grounded_report rubric, stable snake_case ids, verification in the swarm evaluator vocabulary — contains / not_contains / regex / report_ref), pre-registered prediction with confidence in [0,1], and an energy budget that MUST carry max_runs (a positive integer the server enforces at fitness_record — recording past the ceiling is a typed budget_exhausted refusal, §P8.9 step 2). Optional design_spec carries a typed, versioned scientific DesignSpec (a JSON object with a non-empty spec_version) for a scientific-hypothesis declaration — it is stored, exposed through lineage/population reads, and joins the seal fingerprint, so a post-seal design edit yields a different fingerprint rather than silently reusing a stale design; omit it for a legacy artifact experiment. Optional linked_goal_id joins the experiment to the kanban goal that Brier-scores its claim. Returns the experiment record with status proposed. Optional experiment_key converges retried calls onto the existing record instead of duplicating it."
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
            rubrics,
            prediction,
            budget,
            linked_goal_id,
            design_spec,
            experiment_key,
        }): Parameters<ExperimentProposeRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "experiment_propose", async {
            require_non_empty("hypothesis", &hypothesis)?;
            if !LAYERS.contains(&layer.as_str()) {
                return Err(map_experimentation_error(
                    ExperimentationError::UnknownLayer(layer),
                ));
            }
            if genotype_refs.is_empty() || genotype_refs.iter().any(|r| r.trim().is_empty()) {
                return Err(map_experimentation_error(ExperimentationError::Empty(
                    "genotype_refs",
                )));
            }
            require_non_empty("fitness_fn", &fitness_fn)?;
            require_non_empty("prediction.claim", &prediction.claim)?;
            if !(0.0..=1.0).contains(&prediction.confidence) {
                return Err(map_experimentation_error(
                    ExperimentationError::ConfidenceOutOfRange(prediction.confidence),
                ));
            }
            // P1: the declaration carries a typed, machine-checkable eval
            // contract — the same grammar the execution harnesses verify
            // against (the swarm evaluator vocabulary).
            validate_rubrics(&rubrics).map_err(map_experimentation_error)?;
            // The optional typed DesignSpec (task `f1a90cd2`) must be a
            // versioned JSON object when supplied — a design is a versioned
            // artifact, never an unversioned blob.
            let design_spec_value = design_spec.map(|value| value.0);
            validate_design_spec(design_spec_value.as_ref()).map_err(map_experimentation_error)?;
            // §P8.9 step 2 (D-1: enforce): the declared budget must carry a
            // machine-readable run ceiling — the Layer-A set point.
            let max_runs = budget
                .get("max_runs")
                .and_then(serde_json::Value::as_u64)
                .filter(|value| *value > 0 && *value <= i64::MAX as u64)
                .ok_or_else(|| {
                    map_experimentation_error(ExperimentationError::BudgetMissingMaxRuns(
                        budget.to_string(),
                    ))
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
                    &rubrics,
                    &prediction,
                    &budget,
                    max_runs,
                    linked_goal_id.as_deref(),
                    experiment_key.as_deref(),
                    design_spec_value.as_ref(),
                )
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "experiment_propose",
                serde_json::json!({ "status": "proposed", "experiment": record }),
            ))
        })
        .await
    }

    #[tool(
        description = "Register a variant inside an experiment (protocol step 2, Vary): the genotype configuration (arbitrary JSON — e.g. the composite (SKILL.md, templates) pair or the agent-card config), an optional role (baseline | challenger | ablation, default challenger — exactly one baseline per experiment; an ablation names its parent variant), an optional parent variant for lineage, and an optional variant_key for replay convergence. The first variant moves the experiment to running. A resolved experiment no longer accepts variants."
    )]
    pub async fn variant_register(
        &self,
        Parameters(VariantRegisterRequest {
            experiment_id,
            genotype_config,
            parent_variant_id,
            role,
            variant_key,
        }): Parameters<VariantRegisterRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "variant_register", async {
            require_non_empty("experiment_id", &experiment_id)?;
            let role = role.unwrap_or_else(|| VARIANT_ROLE_CHALLENGER.to_string());
            let record = self
                .store
                .register_variant(
                    &experiment_id,
                    &genotype_config,
                    parent_variant_id.as_deref(),
                    &role,
                    variant_key.as_deref(),
                )
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "variant_register",
                serde_json::json!({ "status": "registered", "variant": record }),
            ))
        })
        .await
    }

    #[tool(
        description = "Seal the experiment's eval contract (P2 — the headroom pre-check made mechanical): freezes the declaration's (eval_set, rubrics, fitness_fn, noise_band, design_spec) under a SHA-256 fingerprint and records the headroom attestation — the baseline variant, its recorded fitness evidence, and exactly one of a measured pass rate (baseline_pass_rate, must be < 1.0; saturation is a typed no-headroom refusal that voids via selection_record, §5 Q6) or a tagged scientific_headroom criterion {design_family, criterion, evidence} for a design family with no meaningful binary baseline pass rate (single-arm calibration, observational estimation, equivalence). Supplying both or neither is a typed refusal. Pre-seal, only the baseline records fitness; post-seal, every fitness record cites the seal. An identical retried seal converges on the existing record; a different attempt is a typed refusal."
    )]
    pub async fn experiment_seal(
        &self,
        Parameters(ExperimentSealRequest {
            experiment_id,
            baseline_variant_id,
            baseline_pass_rate,
            scientific_headroom,
        }): Parameters<ExperimentSealRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "experiment_seal", async {
            require_non_empty("experiment_id", &experiment_id)?;
            require_non_empty("baseline_variant_id", &baseline_variant_id)?;
            if let Some(headroom) = &scientific_headroom {
                validate_scientific_headroom(headroom).map_err(map_experimentation_error)?;
            }
            let record = self
                .store
                .seal_experiment(
                    &experiment_id,
                    &baseline_variant_id,
                    baseline_pass_rate,
                    scientific_headroom.as_ref(),
                )
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "experiment_seal",
                serde_json::json!({ "status": "sealed", "seal": record }),
            ))
        })
        .await
    }

    #[tool(
        description = "Record fitness for a variant (protocol step 3, Test): runs are caller-asserted report references — each entry is expected to name a real harness or evaluator report (e.g. a swarm_eval_agent_local run); this server stores the declaration and enforces bookkeeping (ids, seal relations, budget sums), it does not authenticate the referenced reports — plus run_count (the number of rollouts those reports cover; the server sums run counts against the experiment's declared budget.max_runs ceiling and refuses the record with a typed budget_exhausted error when it would exceed it, §P8.9 step 2) and per-objective scores computed from those runs. P2: pre-seal, only the baseline variant records fitness; post-seal, every record cites the active seal via seal_id. Immutable once written. A resolved experiment no longer accepts fitness."
    )]
    pub async fn fitness_record(
        &self,
        Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs,
            run_count,
            scores,
            seal_id,
        }): Parameters<FitnessRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "fitness_record", async {
            require_non_empty("experiment_id", &experiment_id)?;
            require_non_empty("variant_id", &variant_id)?;
            if runs.is_empty() || runs.iter().any(|r| r.trim().is_empty()) {
                return Err(map_experimentation_error(ExperimentationError::Empty(
                    "runs",
                )));
            }
            if run_count == 0 || run_count > i64::MAX as u64 {
                return Err(map_experimentation_error(
                    ExperimentationError::RunCountInvalid(run_count),
                ));
            }
            let record = self
                .store
                .record_fitness(
                    &experiment_id,
                    &variant_id,
                    &runs,
                    run_count,
                    &scores,
                    seal_id.as_deref(),
                )
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "fitness_record",
                serde_json::json!({ "status": "recorded", "fitness": record }),
            ))
        })
        .await
    }

    #[tool(
        description = "Record the sealed final evaluation (P3): the leading variant re-scored on the selection set. Requires the experiment sealed (P2); run refs must be DISJOINT from every prior fitness record and final evaluation for the experiment — the server enforces the disjointness (the GEPA feedback/selection split made mechanical) and refuses overlaps naming them; run_count counts against budget.max_runs together with all fitness records. Selection-grade evidence for the algedonic review."
    )]
    pub async fn final_evaluation_record(
        &self,
        Parameters(FinalEvaluationRecordRequest {
            experiment_id,
            variant_id,
            runs,
            run_count,
            scores,
        }): Parameters<FinalEvaluationRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "final_evaluation_record", async {
            require_non_empty("experiment_id", &experiment_id)?;
            require_non_empty("variant_id", &variant_id)?;
            if runs.is_empty() || runs.iter().any(|r| r.trim().is_empty()) {
                return Err(map_experimentation_error(ExperimentationError::Empty(
                    "runs",
                )));
            }
            if run_count == 0 || run_count > i64::MAX as u64 {
                return Err(map_experimentation_error(
                    ExperimentationError::RunCountInvalid(run_count),
                ));
            }
            let record = self
                .store
                .record_final_evaluation(&experiment_id, &variant_id, &runs, run_count, &scores)
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "final_evaluation_record",
                serde_json::json!({ "status": "recorded", "final_evaluation": record }),
            ))
        })
        .await
    }

    #[tool(
        description = "Record the selection resolution for an experiment (protocol step 4, Select): exactly one of a legacy verdict (selected, naming the winning variant, or rejected, with at least one reject reason) or a typed scientific_outcome for a scientific design — rejected | corroborated | evidence_of_practical_equivalence | inconclusive | measurement_void | design_void. Supplying both or neither is a typed refusal; a scientific outcome is stored as itself and is never renamed selected/rejected. Both outcomes persist as fossils. Only the two genuinely binary outcomes (rejected, corroborated) enter Brier calibration; the bounded and void outcomes are excluded on the typed field. The first selection resolves the experiment; a further selection is a failed_precondition conflict. algedonic_reference names the review record that chaired the selection."
    )]
    pub async fn selection_record(
        &self,
        Parameters(SelectionRecordRequest {
            experiment_id,
            verdict,
            selected_variant_id,
            reject_reasons,
            algedonic_reference,
            scientific_outcome,
        }): Parameters<SelectionRecordRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "selection_record", async {
            require_non_empty("experiment_id", &experiment_id)?;
            // Exactly one resolution: the legacy binary verdict or the typed
            // scientific outcome (task `f1a90cd2`). Never both, never
            // neither — a silent default would fabricate a resolution. A
            // scientific outcome is passed through as itself; the store
            // validates it against the closed vocabulary.
            let verdict = match (&verdict, &scientific_outcome) {
                (Some(verdict), None) => verdict.clone(),
                (None, Some(outcome)) => outcome.clone(),
                _ => {
                    return Err(map_experimentation_error(
                        ExperimentationError::SelectionResolutionConflict(
                            verdict,
                            scientific_outcome,
                        ),
                    ));
                }
            };
            let (record, experiment) = self
                .store
                .record_selection(
                    &experiment_id,
                    &verdict,
                    selected_variant_id.as_deref(),
                    &reject_reasons,
                    algedonic_reference.as_deref(),
                    scientific_outcome.as_deref(),
                )
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "selection_record",
                serde_json::json!({
                    "status": "resolved",
                    "selection": record,
                    "experiment": experiment,
                }),
            ))
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
                        .map_err(map_experimentation_error)?;
                    let experiment = self
                        .store
                        .experiment_by_id(&variant.experiment_id)
                        .map_err(map_experimentation_error)?;
                    let fitness = self
                        .store
                        .fitness_for_experiment(&variant.experiment_id)
                        .map_err(map_experimentation_error)?;
                    let selections = self
                        .store
                        .selections_for_experiment(&variant.experiment_id)
                        .map_err(map_experimentation_error)?;
                    let final_evaluations = self
                        .store
                        .final_evaluations_for_experiment(&variant.experiment_id)
                        .map_err(map_experimentation_error)?;
                    Ok(crate::ontology::annotate(
                        "lineage_read",
                        serde_json::json!({
                            "kind": "variant",
                            "ancestry": ancestry,
                            "experiment": experiment,
                            "fitness": fitness,
                            "selections": selections,
                            "final_evaluations": final_evaluations,
                        }),
                    ))
                }
                Err(ExperimentationError::VariantNotFound(_)) => {
                    let experiment = self
                        .store
                        .experiment_by_id(&artifact_ref)
                        .map_err(map_experimentation_error)?;
                    let variants = self
                        .store
                        .variants_for_experiment(&artifact_ref)
                        .map_err(map_experimentation_error)?;
                    let fitness = self
                        .store
                        .fitness_for_experiment(&artifact_ref)
                        .map_err(map_experimentation_error)?;
                    let selections = self
                        .store
                        .selections_for_experiment(&artifact_ref)
                        .map_err(map_experimentation_error)?;
                    let seal = self
                        .store
                        .seal_for_experiment(&artifact_ref)
                        .map_err(map_experimentation_error)?;
                    let final_evaluations = self
                        .store
                        .final_evaluations_for_experiment(&artifact_ref)
                        .map_err(map_experimentation_error)?;
                    Ok(crate::ontology::annotate(
                        "lineage_read",
                        serde_json::json!({
                            "kind": "experiment",
                            "experiment": experiment,
                            "variants": variants,
                            "fitness": fitness,
                            "selections": selections,
                            "seal": seal,
                            "final_evaluations": final_evaluations,
                        }),
                    ))
                }
                Err(error) => Err(map_experimentation_error(error)),
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
                    return Err(map_experimentation_error(
                        ExperimentationError::UnknownLayer(value.clone()),
                    ));
                }
            }

            if let Some(value) = &created_since {
                chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
                    map_experimentation_error(ExperimentationError::BadTimestamp(value.clone()))
                })?;
            }
            let limit = limit.unwrap_or(50).clamp(1, 200);
            let experiments = self
                .store
                .population(
                    layer.as_deref(),
                    status.map(ExperimentStatus::as_str),
                    created_since.as_deref(),
                    limit,
                )
                .map_err(map_experimentation_error)?;
            Ok(crate::ontology::annotate(
                "population_query",
                serde_json::json!({
                    "count": experiments.len(),
                    "experiments": experiments,
                }),
            ))
        })
        .await
    }
}

#[tool_handler(router = Self::experimentation_router())]
impl rmcp::ServerHandler for ExperimentationServer {}

/// Start the server: the registry lives in the per-agent experimentation database
/// under the hKask data dir (`HKASK_EXPERIMENTATION_DB` overrides the path), opened
/// through the canonical passphrase chain — a missing `HKASK_DB_PASSPHRASE`
/// fails startup visibly, never as an empty-key open.
pub async fn run() -> Result<(), hkask_mcp_server::McpError> {
    hkask_mcp_server::run_server(
        "hkask-mcp-experimentation",
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
                    target: "hkask.mcp.experimentation",
                    path = %registry_path.display(),
                    %error,
                    "Failed to create the experimentation DB directory \
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
                ExperimentationStore::with_driver(driver)
                    .map_err(|error| infrastructure(format!("experimentation store init: {error}")))?,
            );
            Ok(ExperimentationServer::new(ctx.webid, store))
        },
        vec![hkask_mcp_server::CredentialRequirement::required(
            "HKASK_DB_PASSPHRASE",
            "SQLCipher encryption passphrase for the experiment registry — the server refuses to start without it (the registry is the record of record from day one; no legacy-import path)",
        )],
    )
    .await
}
