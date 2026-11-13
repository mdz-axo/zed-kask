//! Experimentation MCP server — the experiment registry (repair plan §P8).
//!
//! One server owns the durable record of experiments: declaration
//! with a pre-registered prediction, variant registration with lineage,
//! fitness records (caller-asserted report refs, stored not verified),
//! selection fossils, lineage reads, and population queries. The registry is the single record path —
//! superseded retention mechanisms are deleted in the same change that
//! lands this server, never run alongside it (§P8.7-Q5).
//!
//! What reaches a model: the editor forwards a tool description only up to
//! its first sentence within 200 bytes (`build_request_tools`), and forwards
//! the input schema whole. So each description leads with one complete
//! sentence, and every rule a caller must follow is stated on the parameter
//! it constrains. `descriptions_lead_with_a_complete_sentence` pins the first.

use std::sync::Arc;

use hkask_mcp_server::server::{McpToolError, execute_tool};
use hkask_types::AnyJsonValue;
use hkask_types::McpErrorKind;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_handler, tool_router};
use schemars::{JsonSchema, json_schema};
use serde::Deserialize;

use crate::store::ExperimentationStore;
use crate::types::{
    ExperimentStatus, ExperimentationError, LAYERS, Prediction, Rubric, SCIENTIFIC_OUTCOMES,
    ScientificHeadroom, VARIANT_ROLE_CHALLENGER, VARIANT_ROLES, VERDICTS, validate_design_spec,
    validate_rubrics,
};

hkask_mcp_server::mcp_server!(
    pub struct ExperimentationServer {
        pub store: Arc<ExperimentationStore>,
    }
);

/// Classify [`ExperimentationError`] for MCP dispatch under the canonical
/// contract: a caller-named record that does not exist is `not_found`; input
/// that is malformed or outside a closed set is `invalid_argument`; a request
/// the registry's current state refuses is `failed_precondition`; a store
/// fault is `internal`.
pub fn map_experimentation_error(error: ExperimentationError) -> McpToolError {
    match error {
        ExperimentationError::ExperimentNotFound(_)
        | ExperimentationError::VariantNotFound(_)
        | ExperimentationError::ArtifactNotFound(_) => McpToolError::not_found(error.to_string()),
        ExperimentationError::VariantNotInExperiment(..)
        | ExperimentationError::UnknownLayer(_)
        | ExperimentationError::BlankOptional(_)
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
        | ExperimentationError::SelectionResolutionConflict(_)
        | ExperimentationError::DesignSpecInvalid(_)
        | ExperimentationError::ExperimentKeyDiffers(_)
        | ExperimentationError::VariantKeyDiffers(_)
        | ExperimentationError::SealAttestationConflict
        | ExperimentationError::ScientificHeadroomIncomplete
        | ExperimentationError::SealBaselineMismatch(..)
        | ExperimentationError::SealMismatch(..)
        | ExperimentationError::SealPassRateInvalid(_)
        | ExperimentationError::RunRefsNotDisjoint(_) => {
            McpToolError::invalid_argument(error.to_string())
        }
        ExperimentationError::ExperimentResolved(_)
        | ExperimentationError::DuplicateBaseline(..)
        | ExperimentationError::BudgetExhausted(..)
        | ExperimentationError::AlreadySealed(..)
        | ExperimentationError::ExperimentNotSealed(_)
        | ExperimentationError::SealCitationMissing(..)
        | ExperimentationError::NoHeadroom(_)
        | ExperimentationError::SealFitnessMissing => {
            McpToolError::new(McpErrorKind::FailedPrecondition, error.to_string())
        }
        ExperimentationError::Database(_) | ExperimentationError::Serialization(_) => {
            McpToolError::internal(error.to_string())
        }
    }
}

/// Reject empty/whitespace-only required string fields as invalid_argument.
fn require_non_empty(label: &'static str, value: &str) -> Result<(), McpToolError> {
    if value.trim().is_empty() {
        return Err(map_experimentation_error(ExperimentationError::Empty(
            label,
        )));
    }
    Ok(())
}

/// An optional id or key is omitted or meaningful, never blank: a blank
/// replay key would bind every later blank-keyed call to one record, and a
/// blank id would be stored as a reference to nothing.
fn reject_blank(label: &'static str, value: Option<&str>) -> Result<(), McpToolError> {
    if value.is_some_and(|text| text.trim().is_empty()) {
        return Err(map_experimentation_error(
            ExperimentationError::BlankOptional(label),
        ));
    }
    Ok(())
}

fn require_runs(runs: &[String], run_count: u64) -> Result<(), McpToolError> {
    if runs.is_empty() || runs.iter().any(|run| run.trim().is_empty()) {
        return Err(map_experimentation_error(ExperimentationError::Empty(
            "runs",
        )));
    }
    if run_count == 0 || run_count > i64::MAX as u64 {
        return Err(map_experimentation_error(
            ExperimentationError::RunCountInvalid(run_count),
        ));
    }
    Ok(())
}

// ── Closed vocabularies in the input schema ─────────────────────────
//
// The fields stay strings (the handler names the valid values when it
// refuses one); the schema enumerates the same constants so a caller reads
// the set before calling.

fn optional_enum(values: &[&str]) -> schemars::Schema {
    json_schema!({"anyOf": [{"type": "string", "enum": values}, {"type": "null"}]})
}

fn layer_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    json_schema!({"type": "string", "enum": LAYERS})
}

fn optional_layer_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    optional_enum(LAYERS)
}

fn optional_role_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    optional_enum(VARIANT_ROLES)
}

fn optional_verdict_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    optional_enum(VERDICTS)
}

fn optional_outcome_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    optional_enum(SCIENTIFIC_OUTCOMES)
}

// ── Request types ───────────────────────────────────────────────────
//
// Every request refuses unknown fields: a misspelled optional parameter is
// an error the caller sees, not a silently applied default.

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExperimentProposeRequest {
    /// The falsifiable hypothesis the experiment tests.
    pub hypothesis: String,
    /// The artifact layer under experiment.
    #[schemars(schema_with = "layer_schema")]
    pub layer: String,
    /// References to the artifacts under experiment. At least one, none blank.
    pub genotype_refs: Vec<String>,
    /// The eval-set declaration: the fixed tasks and any feedback/selection
    /// split. Sealed.
    pub eval_set: AnyJsonValue,
    /// The fitness function: its name and how it is computed. Sealed.
    pub fitness_fn: String,
    /// The band within which score differences count as ties, for example
    /// {"cost_band_pct": 10}. Sealed.
    pub noise_band: AnyJsonValue,
    /// The rubric bundle: at least one `metric` rubric and at least one
    /// `implementation` or `grounded_report` rubric, with unique ids. Sealed.
    pub rubrics: Vec<Rubric>,
    /// The prediction, declared before any measurement.
    pub prediction: Prediction,
    /// The budget declaration, a JSON object. It must carry `max_runs`, a
    /// positive integer: the ceiling on the summed run_count of all fitness
    /// records and final evaluations. A record past it is refused.
    pub budget: AnyJsonValue,
    /// The id of a kanban goal linked to this experiment. Stored and
    /// returned; the server does not read the goal.
    pub linked_goal_id: Option<String>,
    /// A scientific design: a JSON object with a non-empty `spec_version`
    /// string. Sealed. Omit when the experiment has no separate design.
    pub design_spec: Option<AnyJsonValue>,
    /// Replay key: a retry with the same key and the same declaration
    /// returns the existing experiment; a different declaration under the
    /// same key is refused.
    pub experiment_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VariantRegisterRequest {
    pub experiment_id: String,
    /// The variant's configuration: its content or a descriptor of it.
    pub genotype_config: AnyJsonValue,
    /// The variant this one derives from, in the same experiment. Required
    /// for an ablation.
    pub parent_variant_id: Option<String>,
    /// The variant's role; challenger when omitted. One baseline per
    /// experiment.
    #[schemars(schema_with = "optional_role_schema")]
    pub role: Option<String>,
    /// Replay key, unique within the experiment: a retry with the same key
    /// and the same variant returns the existing one; a different variant
    /// under the same key is refused.
    pub variant_key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExperimentSealRequest {
    pub experiment_id: String,
    /// The baseline-role variant, with at least one fitness record.
    pub baseline_variant_id: String,
    /// The baseline's measured pass rate, in [0, 1). A rate of 1.0 is
    /// refused: the eval set cannot discriminate. Supply exactly one of this
    /// and `scientific_headroom`.
    pub baseline_pass_rate: Option<f64>,
    /// The attestation for a design with no meaningful binary pass rate.
    /// Supply exactly one of this and `baseline_pass_rate`.
    pub scientific_headroom: Option<ScientificHeadroom>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FitnessRecordRequest {
    pub experiment_id: String,
    pub variant_id: String,
    /// Report references, at least one, none blank. The caller asserts they
    /// name real reports; the server stores them and does not open them. A
    /// reference a final evaluation already used is refused.
    pub runs: Vec<String>,
    /// The number of runs those reports cover, at least 1. Counted against
    /// `budget.max_runs`; a record that would pass the ceiling is refused.
    pub run_count: u64,
    /// Per-objective scores computed from those runs.
    pub scores: AnyJsonValue,
    /// The experiment's seal id. Required once the experiment is sealed;
    /// omitted before, when only the baseline variant may record.
    pub seal_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalEvaluationRecordRequest {
    pub experiment_id: String,
    pub variant_id: String,
    /// Report references for the held-out runs: at least one, none blank,
    /// none repeated, and none present in any earlier fitness record or
    /// final evaluation of the experiment. An overlap is refused, naming it.
    pub runs: Vec<String>,
    /// The number of runs those reports cover, at least 1. Counted against
    /// `budget.max_runs` together with every fitness record.
    pub run_count: u64,
    /// Per-objective scores computed from those runs.
    pub scores: AnyJsonValue,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionRecordRequest {
    pub experiment_id: String,
    /// The verdict. Supply exactly one of this and `scientific_outcome`.
    #[schemars(schema_with = "optional_verdict_schema")]
    pub verdict: Option<String>,
    /// The chosen variant, in this experiment. Required when verdict is
    /// selected.
    pub selected_variant_id: Option<String>,
    /// Why the experiment was rejected, no entry blank. At least one when
    /// verdict or scientific_outcome is rejected.
    #[serde(default)]
    pub reject_reasons: Vec<String>,
    /// The review record that decided the selection.
    pub algedonic_reference: Option<String>,
    /// The outcome of a scientific design. Supply exactly one of this and
    /// `verdict`.
    #[schemars(schema_with = "optional_outcome_schema")]
    pub scientific_outcome: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineageReadRequest {
    /// An experiment id or a variant id.
    pub artifact_ref: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PopulationQueryRequest {
    /// Only experiments on this layer.
    #[schemars(schema_with = "optional_layer_schema")]
    pub layer: Option<String>,
    /// Only experiments in this status.
    pub status: Option<ExperimentStatus>,
    /// Only experiments created at or after this RFC 3339 instant, in any
    /// UTC offset.
    pub created_since: Option<String>,
    /// How many experiments to return: 50 when omitted; a value outside
    /// 1-200 is moved to the nearer bound.
    pub limit: Option<u32>,
}

// ── Tool router ─────────────────────────────────────────────────────

#[tool_router(router = experimentation_router, vis = "pub")]
impl ExperimentationServer {
    #[tool(
        description = "Declare an experiment before any measurement: hypothesis, layer, eval contract, prediction and run budget. Returns the record with status proposed. The declaration is immutable: no tool edits it. A retry under the same experiment_key returns the existing record; a different declaration under that key is refused."
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
            reject_blank("linked_goal_id", linked_goal_id.as_deref())?;
            reject_blank("experiment_key", experiment_key.as_deref())?;
            validate_rubrics(&rubrics).map_err(map_experimentation_error)?;
            let design_spec_value = design_spec.map(|value| value.0);
            validate_design_spec(design_spec_value.as_ref()).map_err(map_experimentation_error)?;
            // The declared budget must carry a machine-readable run ceiling.
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
        description = "Register one variant of an experiment, with its role and optional parent. The first variant moves the experiment from proposed to running. One baseline per experiment; an ablation names its parent. A retry under the same variant_key returns the existing variant; a different variant under that key is refused. A resolved experiment accepts no new variant."
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
            reject_blank("parent_variant_id", parent_variant_id.as_deref())?;
            reject_blank("variant_key", variant_key.as_deref())?;
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
        description = "Seal an experiment after its baseline is measured: store a SHA-256 fingerprint of the declared eval contract and one headroom attestation. Requires a baseline-role variant with recorded fitness. Before the seal only the baseline records fitness; after it every fitness record cites the seal. One seal per experiment, permanent: an identical retry returns it, a different one is refused. The fingerprint covers eval_set, fitness_fn, noise_band, rubrics and design_spec exactly as declared, object key order included."
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
        description = "Record measured fitness for one variant: report references, the number of runs they cover, and scores. The references are the caller's assertion: the server stores them and does not open or verify the reports. A record that would take the experiment past budget.max_runs is refused. Records are immutable. A resolved experiment accepts none."
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
            require_runs(&runs, run_count)?;
            reject_blank("seal_id", seal_id.as_deref())?;
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
        description = "Record the final evaluation of one variant on runs held out from every earlier record of the experiment. Requires a sealed experiment. The server compares run references only: a reference that appears in an earlier fitness record or final evaluation is refused, and a later fitness record may not reuse one. The references are the caller's assertion, as in fitness_record."
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
            require_runs(&runs, run_count)?;
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
        description = "Resolve an experiment with exactly one of verdict (selected or rejected) or scientific_outcome. The first selection resolves the experiment permanently; a second is refused. A scientific outcome is stored in scientific_outcome, never as a verdict value. The server stores the resolution and computes no score from it."
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
            reject_blank("selected_variant_id", selected_variant_id.as_deref())?;
            reject_blank("algedonic_reference", algedonic_reference.as_deref())?;
            // Exactly one resolution, never a default. The store checks the
            // token against the vocabulary of the field it arrived in.
            let token = match (&verdict, &scientific_outcome) {
                (Some(verdict), None) => verdict.clone(),
                (None, Some(outcome)) => outcome.clone(),
                (Some(_), Some(_)) => {
                    return Err(map_experimentation_error(
                        ExperimentationError::SelectionResolutionConflict("both"),
                    ));
                }
                (None, None) => {
                    return Err(map_experimentation_error(
                        ExperimentationError::SelectionResolutionConflict("neither"),
                    ));
                }
            };
            let (record, experiment) = self
                .store
                .record_selection(
                    &experiment_id,
                    &token,
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
        description = "Read one experiment's whole record by experiment id or variant id. Returns the experiment, its seal, and every fitness record, final evaluation and selection; with an experiment id also all variants, with a variant id that variant's ancestry, oldest parent first. Resolved and rejected experiments stay readable."
    )]
    pub async fn lineage_read(
        &self,
        Parameters(LineageReadRequest { artifact_ref }): Parameters<LineageReadRequest>,
    ) -> Result<String, McpToolError> {
        execute_tool(self, "lineage_read", async {
            require_non_empty("artifact_ref", &artifact_ref)?;
            let store = &self.store;
            // A variant id is tried first; the two id spaces are disjoint.
            let (kind, experiment_id, members) = match store.variant_by_id(&artifact_ref) {
                Ok(variant) => (
                    "variant",
                    variant.experiment_id,
                    store.variant_ancestry(&artifact_ref),
                ),
                Err(ExperimentationError::VariantNotFound(_)) => (
                    "experiment",
                    artifact_ref.clone(),
                    store.variants_for_experiment(&artifact_ref),
                ),
                Err(error) => return Err(map_experimentation_error(error)),
            };
            let experiment = store.experiment_by_id(&experiment_id).map_err(|error| {
                map_experimentation_error(match error {
                    ExperimentationError::ExperimentNotFound(_) => {
                        ExperimentationError::ArtifactNotFound(artifact_ref.clone())
                    }
                    other => other,
                })
            })?;
            let members = members.map_err(map_experimentation_error)?;
            let mut body = serde_json::json!({
                "kind": kind,
                "experiment": experiment,
                "seal": store
                    .seal_for_experiment(&experiment_id)
                    .map_err(map_experimentation_error)?,
                "fitness": store
                    .fitness_for_experiment(&experiment_id)
                    .map_err(map_experimentation_error)?,
                "final_evaluations": store
                    .final_evaluations_for_experiment(&experiment_id)
                    .map_err(map_experimentation_error)?,
                "selections": store
                    .selections_for_experiment(&experiment_id)
                    .map_err(map_experimentation_error)?,
            });
            let members_key = if kind == "variant" {
                "ancestry"
            } else {
                "variants"
            };
            body[members_key] = serde_json::json!(members);
            Ok(crate::ontology::annotate("lineage_read", body))
        })
        .await
    }

    #[tool(
        description = "List experiments, newest first, with optional layer, status and created_since filters. Returns each experiment's full declaration, 50 by default and at most 200; use lineage_read for one experiment's variants, records and resolution."
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
            // Stored timestamps are UTC text and the filter compares text, so
            // the instant is rewritten in UTC first: the same instant written
            // at another offset must select the same rows.
            let created_since = created_since
                .map(|value| {
                    chrono::DateTime::parse_from_rfc3339(&value)
                        .map(|instant| instant.with_timezone(&chrono::Utc).to_rfc3339())
                        .map_err(|_| {
                            map_experimentation_error(ExperimentationError::BadTimestamp(value))
                        })
                })
                .transpose()?;
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
            let db_path = registry_path.to_string_lossy().to_string();
            // The shared server-database bootstrap (dual-axis review Batch
            // C, migration 3 of 3): the parent self-heal, the 2-tier
            // passphrase chain, open-or-repair, the pool, and the labeled
            // driver — absorbed into hkask-mcp-server. Startup failures
            // map to Infrastructure — visible and named, never a silent
            // empty-key open.
            let infrastructure = |message: String| {
                hkask_mcp_server::McpError::Infrastructure(hkask_types::InfrastructureError::Io(
                    message,
                ))
            };
            let server_db = hkask_mcp_server::open_server_db(&db_path, &ctx.credentials)
                .map_err(|error| infrastructure(error.to_string()))?;
            let driver: Arc<dyn hkask_storage::database::driver::DatabaseDriver> =
                Arc::clone(&server_db.driver);
            let store = Arc::new(
                ExperimentationStore::with_driver(driver)
                    .map_err(|error| infrastructure(format!("experimentation store init: {error}")))?,
            );
            Ok(ExperimentationServer::new(ctx.webid, store))
        },
        vec![hkask_mcp_server::db_passphrase_requirement_with(
            "SQLCipher encryption passphrase for the experiment registry — the server refuses to start without it (the registry is the record of record from day one; no legacy-import path)",
        )],
    )
    .await
}
