//! Tool-behavior contract tests — every tool invoked through its public
//! `Parameters<T>` seam (the registry README's testing standard; the CI gate
//! is `kask/scripts/check-mcp-tool-tests.sh`).
//!
//! The server is constructed directly over an in-memory SQLCipher driver, so
//! these tests exercise the real tool surface end-to-end: request shapes,
//! validation, per-variant error classification, and the full protocol
//! lifecycle — the in-process rehearsal of the §P8.5 run-one-through.

use hkask_mcp_experimentation::server::{
    ExperimentProposeRequest, ExperimentSealRequest, ExperimentationServer,
    FinalEvaluationRecordRequest, FitnessRecordRequest, LineageReadRequest, PopulationQueryRequest,
    SelectionRecordRequest, VariantRegisterRequest,
};
use hkask_mcp_experimentation::store::ExperimentationStore;
use hkask_mcp_experimentation::types::{
    ExperimentStatus, Prediction, RUBRIC_KIND_IMPLEMENTATION, RUBRIC_KIND_METRIC, Rubric,
    RubricVerification,
};
use hkask_storage::database::sqlite::SqliteDriver;
use hkask_types::AnyJsonValue;
use hkask_types::WebID;
use hkask_types::tool_response::unwrap_tool_envelope;
use rmcp::handler::server::wrapper::Parameters;
use std::sync::Arc;

fn make_server() -> ExperimentationServer {
    let store = ExperimentationStore::with_driver(SqliteDriver::in_memory_driver())
        .expect("in-memory experimentation store");
    ExperimentationServer::new(WebID::new(), Arc::new(store))
}

/// Parse a tool response and unwrap the `{"content": <value>}` envelope —
/// the canonical response shape (never re-implemented here).
fn envelope(response: &str) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_str(response).expect("tool response is JSON");
    unwrap_tool_envelope(value)
}

fn proposal(layer: &str) -> ExperimentProposeRequest {
    ExperimentProposeRequest {
        hypothesis: "Variant B beats the baseline on the fixed task set".to_string(),
        layer: layer.to_string(),
        genotype_refs: vec![".agents/skills/self-improvement/SKILL.md".to_string()],
        eval_set: AnyJsonValue(serde_json::json!({"task_set": "fixed-5"})),
        fitness_fn: "evaluator pass rate on the fixed set".to_string(),
        noise_band: AnyJsonValue(serde_json::json!({"cost_band_pct": 10})),
        rubrics: vec![
            Rubric {
                id: "pass_rate_threshold".to_string(),
                description: "evaluator pass rate on the fixed set reaches 0.8".to_string(),
                kind: RUBRIC_KIND_METRIC.to_string(),
                verification: RubricVerification::ReportRef {
                    harness: "eval_harness".to_string(),
                },
            },
            Rubric {
                id: "uses_lisp_gate".to_string(),
                description: "the skill routes its gate through lisp_eval".to_string(),
                kind: RUBRIC_KIND_IMPLEMENTATION.to_string(),
                verification: RubricVerification::Contains {
                    spec: "lisp_eval".to_string(),
                },
            },
        ],
        prediction: Prediction {
            claim: "variant B wins".to_string(),
            confidence: 0.7,
        },
        budget: AnyJsonValue(serde_json::json!({"energy_budget": "one eval run", "max_runs": 18})),
        design_spec: None,
        linked_goal_id: None,
        experiment_key: None,
    }
}

async fn propose(server: &ExperimentationServer, layer: &str) -> serde_json::Value {
    let response = server
        .experiment_propose(Parameters(proposal(layer)))
        .await
        .expect("experiment_propose succeeds");
    envelope(&response)
}

async fn register_variant(
    server: &ExperimentationServer,
    experiment_id: &str,
    name: &str,
    parent: Option<&str>,
) -> serde_json::Value {
    register_variant_with_role(server, experiment_id, name, parent, None).await
}

/// Register a variant with an explicit role (P4) — the default helper
/// above registers challengers, matching the server's default.
async fn register_variant_with_role(
    server: &ExperimentationServer,
    experiment_id: &str,
    name: &str,
    parent: Option<&str>,
    role: Option<&str>,
) -> serde_json::Value {
    let response = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.to_string(),
            genotype_config: AnyJsonValue(serde_json::json!({"name": name})),
            parent_variant_id: parent.map(str::to_string),
            role: role.map(str::to_string),
            variant_key: None,
        }))
        .await
        .expect("variant_register succeeds");
    envelope(&response)
}

/// Propose an experiment and register its first variant — the preamble the
/// fitness-record and selection tests need. Returns (experiment_id, variant_id).
async fn propose_with_variant(
    server: &ExperimentationServer,
    layer: &str,
    variant_name: &str,
) -> (String, String) {
    let proposed = propose(server, layer).await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let variant = register_variant(server, &experiment_id, variant_name, None).await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    (experiment_id, variant_id)
}

/// The full §P8.3 protocol through the tool seam: Declare → Vary → Test →
/// Select → Retain, then lineage and population read back the whole record.
#[tokio::test]
async fn full_protocol_runs_through_the_tool_seam() {
    let server = make_server();

    // Declare — the pre-registered prediction round-trips.
    let proposed = propose(&server, "skill").await;
    assert_eq!(proposed["status"], "proposed");
    assert_eq!(proposed["experiment"]["status"], "proposed");
    assert_eq!(proposed["experiment"]["prediction"]["confidence"], 0.7);
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("experiment id")
        .to_string();

    // Vary — a baseline, then a challenger with lineage.
    let baseline =
        register_variant_with_role(&server, &experiment_id, "baseline", None, Some("baseline"))
            .await;
    assert_eq!(baseline["status"], "registered");
    let baseline_id = baseline["variant"]["id"].as_str().expect("id").to_string();
    let challenger =
        register_variant(&server, &experiment_id, "challenger", Some(&baseline_id)).await;
    let challenger_id = challenger["variant"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    // Test — the baseline measures, the seal freezes the contract (P2),
    // then the challenger's recorded report refs cite the seal.
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: baseline_id.clone(),
            runs: vec!["baseline harness report".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.6})),
            seal_id: None,
        }))
        .await
        .expect("baseline fitness_record succeeds");
    let sealed = server
        .experiment_seal(Parameters(ExperimentSealRequest {
            experiment_id: experiment_id.clone(),
            baseline_variant_id: baseline_id.clone(),
            baseline_pass_rate: Some(0.6),
            scientific_headroom: None,
        }))
        .await
        .expect("experiment_seal succeeds");
    let sealed = envelope(&sealed);
    assert_eq!(sealed["status"], "sealed");
    let seal_id = sealed["seal"]["id"].as_str().expect("seal id").to_string();
    assert_eq!(
        sealed["seal"]["fingerprint"]
            .as_str()
            .expect("fingerprint")
            .len(),
        64
    );

    let fitness = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id.clone(),
            runs: vec!["eval_harness run 42".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.8, "total_tokens": 1200})),
            seal_id: Some(seal_id.clone()),
        }))
        .await
        .expect("fitness_record succeeds");
    let fitness = envelope(&fitness);
    assert_eq!(fitness["fitness"]["variant_id"], challenger_id.as_str());
    assert_eq!(fitness["fitness"]["run_count"], 3);
    assert_eq!(fitness["fitness"]["seal_id"], seal_id.as_str());
    assert_eq!(fitness["fitness"]["runs"][0], "eval_harness run 42");

    // Select — the fossil resolves the experiment.
    let selection = server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: experiment_id.clone(),
            verdict: Some("selected".to_string()),
            scientific_outcome: None,
            selected_variant_id: Some(challenger_id.clone()),
            reject_reasons: Vec::new(),
            algedonic_reference: Some("algedonic 2026-09-30#1".to_string()),
        }))
        .await
        .expect("selection_record succeeds");
    let selection = envelope(&selection);
    assert_eq!(selection["status"], "resolved");
    assert_eq!(selection["selection"]["verdict"], "selected");
    assert_eq!(selection["experiment"]["status"], "resolved");

    // Retain — lineage and population read the full record back.
    let lineage = server
        .lineage_read(Parameters(LineageReadRequest {
            artifact_ref: experiment_id.clone(),
        }))
        .await
        .expect("lineage_read succeeds");
    let lineage = envelope(&lineage);
    assert_eq!(lineage["kind"], "experiment");
    assert_eq!(lineage["variants"].as_array().expect("variants").len(), 2);
    assert_eq!(lineage["selections"][0]["verdict"], "selected");

    let population = server
        .population_query(Parameters(PopulationQueryRequest {
            layer: Some("skill".to_string()),
            status: Some(ExperimentStatus::Resolved),
            created_since: None,
            limit: None,
        }))
        .await
        .expect("population_query succeeds");
    let population = envelope(&population);
    assert_eq!(population["count"], 1);
}

/// P1: the declaration requires a valid typed rubric bundle — the
/// machine-checkable eval contract. Empty bundles and bundles missing a
/// required kind are typed invalid_argument refusals; a valid bundle
/// round-trips on the experiment record.
#[tokio::test]
async fn declarations_require_a_valid_rubric_bundle() {
    let server = make_server();

    let mut request = proposal("skill");
    request.rubrics = Vec::new();
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("empty rubric bundle refused");
    assert!(error.to_string().contains("rubric"), "{error}");

    let mut request = proposal("skill");
    request.rubrics.truncate(1); // only the metric rubric remains
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("missing implementation rubric refused");
    assert!(
        error
            .to_string()
            .contains("implementation or grounded_report"),
        "{error}"
    );

    let proposed = propose(&server, "skill").await;
    assert_eq!(
        proposed["experiment"]["rubrics"][0]["id"],
        "pass_rate_threshold"
    );
    assert_eq!(
        proposed["experiment"]["rubrics"][1]["verification"]["type"],
        "contains"
    );
}

/// P4: variant roles are enforced through the tool seam — exactly one
/// baseline per experiment; an ablation names its parent; unknown roles
/// are refused; the default role is challenger.
#[tokio::test]
async fn variant_roles_are_enforced_through_the_tool_seam() {
    let server = make_server();
    let proposed = propose(&server, "skill").await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    let baseline = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.clone(),
            genotype_config: AnyJsonValue(serde_json::json!({"name": "baseline"})),
            parent_variant_id: None,
            role: Some("baseline".to_string()),
            variant_key: None,
        }))
        .await
        .expect("baseline registers");
    let baseline = envelope(&baseline);
    assert_eq!(baseline["variant"]["role"], "baseline");

    // A second baseline is a typed refusal naming the existing one.
    let error = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.clone(),
            genotype_config: AnyJsonValue(serde_json::json!({"name": "second baseline"})),
            parent_variant_id: None,
            role: Some("baseline".to_string()),
            variant_key: None,
        }))
        .await
        .expect_err("second baseline refused");
    assert!(
        error.to_string().contains("exactly one baseline"),
        "{error}"
    );

    // An ablation without a parent is refused.
    let error = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.clone(),
            genotype_config: AnyJsonValue(serde_json::json!({"name": "orphan"})),
            parent_variant_id: None,
            role: Some("ablation".to_string()),
            variant_key: None,
        }))
        .await
        .expect_err("unparented ablation refused");
    assert!(error.to_string().contains("parent_variant_id"), "{error}");

    // Unknown roles are refused with the valid values.
    let error = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.clone(),
            genotype_config: AnyJsonValue(serde_json::json!({"name": "vibe"})),
            parent_variant_id: None,
            role: Some("vibe".to_string()),
            variant_key: None,
        }))
        .await
        .expect_err("unknown role refused");
    assert!(
        error.to_string().contains("baseline, challenger, ablation"),
        "{error}"
    );

    // The default role is challenger.
    let challenger = register_variant(&server, &experiment_id, "challenger", None).await;
    assert_eq!(challenger["variant"]["role"], "challenger");
}

/// P2: the seal through the tool seam — the baseline measures, the seal
/// freezes the contract (fingerprint + headroom attestation), post-seal
/// challenger fitness must cite the seal, and a saturated baseline is
/// the no-headroom void condition.
#[tokio::test]
async fn experiment_seal_freezes_the_contract_through_the_tool_seam() {
    let server = make_server();
    let proposed = propose(&server, "skill").await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    // Baseline with role + measured fitness.
    let baseline =
        register_variant_with_role(&server, &experiment_id, "baseline", None, Some("baseline"))
            .await;
    let baseline_id = baseline["variant"]["id"].as_str().expect("id").to_string();
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: baseline_id.clone(),
            runs: vec!["baseline harness report".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.6})),
            seal_id: None,
        }))
        .await
        .expect("baseline fitness");

    // Seal.
    let sealed = server
        .experiment_seal(Parameters(ExperimentSealRequest {
            experiment_id: experiment_id.clone(),
            baseline_variant_id: baseline_id.clone(),
            baseline_pass_rate: Some(0.6),
            scientific_headroom: None,
        }))
        .await
        .expect("seal succeeds");
    let sealed = envelope(&sealed);
    assert_eq!(sealed["status"], "sealed");
    assert_eq!(sealed["seal"]["baseline_pass_rate"], 0.6);
    let seal_id = sealed["seal"]["id"].as_str().expect("seal id").to_string();

    // Post-seal challenger fitness cites the seal; without the citation,
    // a typed refusal.
    let challenger = register_variant(&server, &experiment_id, "challenger", None).await;
    let challenger_id = challenger["variant"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id.clone(),
            runs: vec!["r".to_string()],
            run_count: 1,
            scores: AnyJsonValue(serde_json::json!({})),
            seal_id: None,
        }))
        .await
        .expect_err("missing citation refused");
    assert!(error.to_string().contains("seal_id"), "{error}");
    let fitness = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id,
            runs: vec!["challenger harness report".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.8})),
            seal_id: Some(seal_id.clone()),
        }))
        .await
        .expect("cited fitness succeeds");
    let fitness = envelope(&fitness);
    assert_eq!(fitness["fitness"]["seal_id"], seal_id.as_str());

    // A second, different seal is refused.
    let error = server
        .experiment_seal(Parameters(ExperimentSealRequest {
            experiment_id: experiment_id.clone(),
            baseline_variant_id: baseline_id.clone(),
            baseline_pass_rate: Some(0.7),
            scientific_headroom: None,
        }))
        .await
        .expect_err("different seal refused");
    assert!(error.to_string().contains("already sealed"), "{error}");

    // A saturated baseline is the no-headroom void condition.
    let fresh = propose(&server, "skill").await;
    let fresh_id = fresh["experiment"]["id"].as_str().expect("id").to_string();
    let fresh_baseline =
        register_variant_with_role(&server, &fresh_id, "baseline", None, Some("baseline")).await;
    let fresh_baseline_id = fresh_baseline["variant"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: fresh_id.clone(),
            variant_id: fresh_baseline_id.clone(),
            runs: vec!["saturated baseline".to_string()],
            run_count: 1,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 1.0})),
            seal_id: None,
        }))
        .await
        .expect("saturated baseline fitness");
    let error = server
        .experiment_seal(Parameters(ExperimentSealRequest {
            experiment_id: fresh_id,
            baseline_variant_id: fresh_baseline_id,
            baseline_pass_rate: Some(1.0),
            scientific_headroom: None,
        }))
        .await
        .expect_err("saturation refused");
    assert!(error.to_string().contains("headroom"), "{error}");
}

/// P3: the final evaluation through the tool seam — requires the seal,
/// enforces run-ref disjointness, and counts against the budget.
#[tokio::test]
async fn final_evaluation_record_enforces_disjointness_through_the_tool_seam() {
    let server = make_server();
    let proposed = propose(&server, "skill").await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    // The sealed flow: baseline + fitness + seal + challenger fitness.
    let baseline =
        register_variant_with_role(&server, &experiment_id, "baseline", None, Some("baseline"))
            .await;
    let baseline_id = baseline["variant"]["id"].as_str().expect("id").to_string();
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: baseline_id.clone(),
            runs: vec!["b1".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.6})),
            seal_id: None,
        }))
        .await
        .expect("baseline fitness");
    let sealed = server
        .experiment_seal(Parameters(ExperimentSealRequest {
            experiment_id: experiment_id.clone(),
            baseline_variant_id: baseline_id,
            baseline_pass_rate: Some(0.6),
            scientific_headroom: None,
        }))
        .await
        .expect("seal");
    let seal_id = envelope(&sealed)["seal"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let challenger = register_variant(&server, &experiment_id, "challenger", None).await;
    let challenger_id = challenger["variant"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id.clone(),
            runs: vec!["c1".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.8})),
            seal_id: Some(seal_id.clone()),
        }))
        .await
        .expect("challenger fitness");

    // A reused run ref is refused naming the overlap.
    let error = server
        .final_evaluation_record(Parameters(FinalEvaluationRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id.clone(),
            runs: vec!["c1".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({})),
        }))
        .await
        .expect_err("overlap refused");
    assert!(error.to_string().contains("c1"), "{error}");

    // A disjoint selection set records; the lineage reads it back.
    let final_eval = server
        .final_evaluation_record(Parameters(FinalEvaluationRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id.clone(),
            runs: vec!["f1".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.85})),
        }))
        .await
        .expect("final evaluation succeeds");
    let final_eval = envelope(&final_eval);
    assert_eq!(final_eval["final_evaluation"]["seal_id"], seal_id.as_str());

    let lineage = server
        .lineage_read(Parameters(LineageReadRequest {
            artifact_ref: experiment_id.clone(),
        }))
        .await
        .expect("lineage_read succeeds");
    let lineage = envelope(&lineage);
    assert_eq!(
        lineage["final_evaluations"].as_array().expect("list").len(),
        1
    );
    assert!(
        lineage["seal"]["fingerprint"].is_string(),
        "the seal rides the experiment lineage"
    );
}

/// Invalid input is rejected with errors that name the offending field —
/// the error-specificity half of the contract (an error that does not teach
/// produces identical retries).
#[tokio::test]
async fn invalid_inputs_are_rejected_with_named_errors() {
    let server = make_server();

    let mut request = proposal("skill");
    request.hypothesis = "   ".to_string();
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("empty hypothesis rejected");
    assert!(error.to_string().contains("hypothesis"), "{error}");

    let error = server
        .experiment_propose(Parameters(proposal("genome")))
        .await
        .expect_err("unknown layer rejected");
    assert!(
        error.to_string().contains("skill, prompt_template"),
        "{error}"
    );

    let mut request = proposal("skill");
    request.prediction.confidence = 1.5;
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("confidence out of range rejected");
    assert!(error.to_string().contains("confidence"), "{error}");

    let mut request = proposal("skill");
    request.genotype_refs = Vec::new();
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("empty genotype refs rejected");
    assert!(error.to_string().contains("genotype_refs"), "{error}");

    // Fitness with no recorded runs is refused (§P8.3).
    let (experiment_id, variant_id) = propose_with_variant(&server, "skill", "v1").await;
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs: Vec::new(),
            run_count: 1,
            scores: AnyJsonValue(serde_json::json!({})),
            seal_id: None,
        }))
        .await
        .expect_err("empty runs rejected");
    assert!(error.to_string().contains("runs"), "{error}");

    // A zero run_count is refused — a fitness record must state how many
    // rollouts its reports cover (§P8.9 step 2).
    let (experiment_id, variant_id) = propose_with_variant(&server, "skill", "v2").await;
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs: vec!["r".to_string()],
            run_count: 0,
            scores: AnyJsonValue(serde_json::json!({})),
            seal_id: None,
        }))
        .await
        .expect_err("zero run_count rejected");
    assert!(error.to_string().contains("run_count"), "{error}");
}

/// Unknown records are not_found; a resolved experiment refuses further
/// work; verdict validation is enforced at the tool boundary.
#[tokio::test]
async fn unknown_records_are_not_found_and_resolved_experiments_refuse_work() {
    let server = make_server();

    let error = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: "exp_missing".to_string(),
            genotype_config: AnyJsonValue(serde_json::json!({})),
            parent_variant_id: None,
            role: None,
            variant_key: None,
        }))
        .await
        .expect_err("unknown experiment is not found");
    assert!(error.to_string().contains("not found"), "{error}");

    // Drive one experiment to resolved, then verify the refusal.
    let (experiment_id, variant_id) = propose_with_variant(&server, "skill", "v1").await;
    server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: experiment_id.clone(),
            verdict: Some("selected".to_string()),
            scientific_outcome: None,
            selected_variant_id: Some(variant_id),
            reject_reasons: Vec::new(),
            algedonic_reference: None,
        }))
        .await
        .expect("selection succeeds");
    let error = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.clone(),
            genotype_config: AnyJsonValue(serde_json::json!({})),
            parent_variant_id: None,
            role: None,
            variant_key: None,
        }))
        .await
        .expect_err("resolved experiment refuses new variants");
    assert!(error.to_string().contains("already resolved"), "{error}");

    // Verdict validation on a fresh experiment.
    let proposed = propose(&server, "skill").await;
    let fresh = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let error = server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: fresh.clone(),
            verdict: Some("selected".to_string()),
            scientific_outcome: None,
            selected_variant_id: None,
            reject_reasons: Vec::new(),
            algedonic_reference: None,
        }))
        .await
        .expect_err("selected without a variant rejected");
    assert!(error.to_string().contains("selected_variant_id"), "{error}");
    let error = server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: fresh,
            verdict: Some("rejected".to_string()),
            scientific_outcome: None,
            selected_variant_id: None,
            reject_reasons: Vec::new(),
            algedonic_reference: None,
        }))
        .await
        .expect_err("rejected without reasons rejected");
    assert!(error.to_string().contains("reject_reason"), "{error}");
}

/// Replay convergence through the tool seam: a retried propose with the same
/// experiment_key returns the existing record instead of duplicating it.
#[tokio::test]
async fn replay_keys_converge_retried_calls_through_the_tool_seam() {
    let server = make_server();

    let mut first = proposal("skill");
    first.experiment_key = Some("propose-key-1".to_string());
    let first = server
        .experiment_propose(Parameters(first))
        .await
        .expect("first propose succeeds");
    let first = envelope(&first);

    let mut second = proposal("skill");
    second.experiment_key = Some("propose-key-1".to_string());
    let second = server
        .experiment_propose(Parameters(second))
        .await
        .expect("retried propose succeeds");
    let second = envelope(&second);

    assert_eq!(
        first["experiment"]["id"], second["experiment"]["id"],
        "a retried propose with the same key must converge, not duplicate"
    );
}

/// §P8.9 step 2 (D-1: enforce) through the tool seam: a declaration without
/// `budget.max_runs` is refused; recording past the declared ceiling is a
/// typed `budget_exhausted` refusal naming the experiment and the ceiling —
/// never a silent overshoot.
#[tokio::test]
async fn budget_max_runs_is_enforced_through_the_tool_seam() {
    let server = make_server();

    // A budget without max_runs is refused at declaration.
    let mut request = proposal("skill");
    request.budget = AnyJsonValue(serde_json::json!({"energy_budget": "one eval run"}));
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("budget without max_runs rejected");
    assert!(error.to_string().contains("budget.max_runs"), "{error}");

    // A non-positive max_runs is refused too.
    let mut request = proposal("skill");
    request.budget = AnyJsonValue(serde_json::json!({"max_runs": 0}));
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("zero max_runs rejected");
    assert!(error.to_string().contains("budget.max_runs"), "{error}");

    // The ceiling is enforced at fitness_record: 9 + 9 = 18 is allowed at
    // the boundary; the 19th run is a typed refusal.
    let proposed = propose(&server, "skill").await; // max_runs = 18
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    assert_eq!(proposed["experiment"]["max_runs"], 18);
    let variant =
        register_variant_with_role(&server, &experiment_id, "baseline", None, Some("baseline"))
            .await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    for _ in 0..2 {
        server
            .fitness_record(Parameters(FitnessRecordRequest {
                experiment_id: experiment_id.clone(),
                variant_id: variant_id.clone(),
                runs: vec!["harness report".to_string()],
                run_count: 9,
                scores: AnyJsonValue(serde_json::json!({})),
                seal_id: None,
            }))
            .await
            .expect("records up to the ceiling are allowed");
    }
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id,
            runs: vec!["one run too many".to_string()],
            run_count: 1,
            scores: AnyJsonValue(serde_json::json!({})),
            seal_id: None,
        }))
        .await
        .expect_err("over-ceiling record refused");
    let message = error.to_string();
    assert!(message.contains("budget exhausted"), "{message}");
    assert!(message.contains(&experiment_id), "{message}");
    assert!(message.contains("19"), "{message}");
    assert!(message.contains("18"), "{message}");
}

/// §P8.9 step 5: the linked goal id round-trips through the declaration and
/// the population query — the explicit registry ↔ goal-loop join.
#[tokio::test]
async fn linked_goal_id_round_trips_through_the_declaration() {
    let server = make_server();
    let mut request = proposal("skill");
    request.linked_goal_id = Some("04998f4f-2ae2-4d53-a67d-f178e77072f5".to_string());
    let response = server
        .experiment_propose(Parameters(request))
        .await
        .expect("propose with linked goal");
    let response = envelope(&response);
    assert_eq!(
        response["experiment"]["linked_goal_id"],
        "04998f4f-2ae2-4d53-a67d-f178e77072f5"
    );
    let experiment_id = response["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let population = server
        .population_query(Parameters(PopulationQueryRequest {
            layer: None,
            status: None,
            created_since: None,
            limit: Some(10),
        }))
        .await
        .expect("population query");
    let population = envelope(&population);
    assert_eq!(
        population["experiments"][0]["linked_goal_id"],
        "04998f4f-2ae2-4d53-a67d-f178e77072f5"
    );
    assert_eq!(population["experiments"][0]["id"], experiment_id.as_str());
}

#[test]
fn experimentation_declares_its_db_passphrase_as_required() {
    let source = include_str!("../src/server.rs");
    let declaration = [
        "CredentialRequirement::required(",
        "\n            \"HKASK_DB_PASSPHRASE\"",
    ]
    .concat();
    assert!(
        source.contains(&declaration),
        "the experimentation server must declare HKASK_DB_PASSPHRASE required — its run() \n         refuses to start without it, so an undeclared (vec![]) or optional form \n         misdescribes the startup contract"
    );
}

/// A-1: an oversized max_runs (> i64::MAX) is rejected at the tool boundary
/// with the typed BudgetMissingMaxRuns error — an unchecked cast wraps it
/// negative, and a negative ceiling reads as an unspent budget forever (the
/// budget refusal and the stuck-running sensor fail together).
#[tokio::test]
async fn oversized_max_runs_is_rejected_through_the_tool_seam() {
    let server = make_server();

    let mut request = proposal("skill");
    request.budget = AnyJsonValue(serde_json::json!({
        "max_runs": 9223372036854775808u64
    }));
    let error = server
        .experiment_propose(Parameters(request))
        .await
        .expect_err("oversized max_runs rejected");
    assert!(error.to_string().contains("budget.max_runs"), "{error}");
}

/// A-1: an oversized run_count (> i64::MAX) is rejected at the tool boundary
/// with the typed RunCountInvalid error — the defensive db_int floor under
/// it would refuse the write even if this boundary check were removed.
#[tokio::test]
async fn oversized_run_count_is_rejected_through_the_tool_seam() {
    let server = make_server();

    let proposed = propose(&server, "skill").await; // max_runs = 18
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let variant =
        register_variant_with_role(&server, &experiment_id, "baseline", None, Some("baseline"))
            .await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs: vec!["harness report".to_string()],
            run_count: 9223372036854775808u64,
            scores: AnyJsonValue(serde_json::json!({})),
            seal_id: None,
        }))
        .await
        .expect_err("oversized run_count rejected");
    assert!(error.to_string().contains("run_count"), "{error}");
}

// ── Scientific-design evolution through the tool seam (task `f1a90cd2`) ──

/// A proposal carrying a typed `DesignSpec` — the scientific declaration the
/// artifact `proposal` helper cannot express.
fn design_proposal(design_spec: serde_json::Value) -> ExperimentProposeRequest {
    ExperimentProposeRequest {
        hypothesis: "Carrier exposure versus targeting dominates ADC efficacy".to_string(),
        layer: "scientific_hypothesis".to_string(),
        genotype_refs: vec!["experimental-design/design-spec-v1".to_string()],
        eval_set: AnyJsonValue(
            serde_json::json!({"schema": "experimental-design-registry-adapter-v1"}),
        ),
        fitness_fn: "primary interaction confidence width relative to the ratified margin"
            .to_string(),
        noise_band: AnyJsonValue(serde_json::json!({"tie_rule": "within the practical margin"})),
        rubrics: vec![
            Rubric {
                id: "interaction_width".to_string(),
                description: "the primary interaction interval is narrower than the margin"
                    .to_string(),
                kind: RUBRIC_KIND_METRIC.to_string(),
                verification: RubricVerification::ReportRef {
                    harness: "eval_harness".to_string(),
                },
            },
            Rubric {
                id: "design_spec_present".to_string(),
                description: "the declaration carries the versioned design_spec".to_string(),
                kind: RUBRIC_KIND_IMPLEMENTATION.to_string(),
                verification: RubricVerification::Contains {
                    spec: "design_spec_version".to_string(),
                },
            },
        ],
        prediction: Prediction {
            claim: "targeting contributes a practically meaningful advantage".to_string(),
            confidence: 0.6,
        },
        budget: AnyJsonValue(serde_json::json!({"max_runs": 24})),
        linked_goal_id: None,
        design_spec: Some(AnyJsonValue(design_spec)),
        experiment_key: None,
    }
}

/// Criterion 1: the tool boundary accepts a versioned design, exposes it on
/// the record, and refuses an unversioned one with a typed error.
#[tokio::test]
async fn design_spec_is_versioned_at_the_tool_boundary() {
    let server = make_server();
    let response = server
        .experiment_propose(Parameters(design_proposal(serde_json::json!({
            "spec_version": "design-spec-v1",
            "spec_id": "adc-factorial-v1",
        }))))
        .await
        .expect("versioned design accepted");
    let proposed = envelope(&response);
    assert_eq!(
        proposed["experiment"]["design_spec"]["spec_id"],
        "adc-factorial-v1"
    );

    // Unversioned → typed invalid_argument.
    let error = server
        .experiment_propose(Parameters(design_proposal(serde_json::json!({
            "spec_id": "no-version",
        }))))
        .await
        .expect_err("unversioned design refused");
    assert!(error.to_string().contains("spec_version"), "{error}");
}

/// Criterion 2: `experiment_seal` accepts a tagged scientific headroom
/// criterion through the tool seam, with no baseline pass rate.
#[tokio::test]
async fn scientific_headroom_seals_through_the_tool_seam() {
    let server = make_server();
    let proposed = envelope(
        &server
            .experiment_propose(Parameters(design_proposal(serde_json::json!({
                "spec_version": "design-spec-v1",
            }))))
            .await
            .expect("propose"),
    );
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let baseline =
        register_variant_with_role(&server, &experiment_id, "baseline", None, Some("baseline"))
            .await;
    let baseline_id = baseline["variant"]["id"].as_str().expect("id").to_string();
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: baseline_id.clone(),
            runs: vec!["baseline report".to_string()],
            run_count: 2,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.6})),
            seal_id: None,
        }))
        .await
        .expect("baseline fitness");

    let sealed = envelope(
        &server
            .experiment_seal(Parameters(ExperimentSealRequest {
                experiment_id: experiment_id.clone(),
                baseline_variant_id: baseline_id.clone(),
                baseline_pass_rate: None,
                scientific_headroom: Some(hkask_mcp_experimentation::types::ScientificHeadroom {
                    design_family: "single_arm".to_string(),
                    criterion: "the single-arm interval width is below the ratified margin"
                        .to_string(),
                    evidence: vec!["power simulation report".to_string()],
                }),
            }))
            .await
            .expect("headroom seal"),
    );
    assert_eq!(
        sealed["seal"]["scientific_headroom"]["design_family"],
        "single_arm"
    );
    assert!(sealed["seal"]["baseline_pass_rate"].is_null());

    // Neither attestation → typed failed_precondition.
    let fresh = envelope(
        &server
            .experiment_propose(Parameters(design_proposal(serde_json::json!({
                "spec_version": "design-spec-v1",
            }))))
            .await
            .expect("propose"),
    );
    let fresh_id = fresh["experiment"]["id"].as_str().expect("id").to_string();
    let fresh_baseline =
        register_variant_with_role(&server, &fresh_id, "baseline", None, Some("baseline")).await;
    let fresh_baseline_id = fresh_baseline["variant"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: fresh_id.clone(),
            variant_id: fresh_baseline_id.clone(),
            runs: vec!["baseline report".to_string()],
            run_count: 2,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.6})),
            seal_id: None,
        }))
        .await
        .expect("baseline fitness");
    let error = server
        .experiment_seal(Parameters(ExperimentSealRequest {
            experiment_id: fresh_id,
            baseline_variant_id: fresh_baseline_id,
            baseline_pass_rate: None,
            scientific_headroom: None,
        }))
        .await
        .expect_err("no attestation refused");
    assert!(error.to_string().contains("exactly one"), "{error}");
}

/// Criterion 1 (read paths): a typed `design_spec` is exposed through
/// both the population query and the lineage read.
#[tokio::test]
async fn design_spec_is_exposed_through_lineage_and_population() {
    let server = make_server();
    let proposed = envelope(
        &server
            .experiment_propose(Parameters(design_proposal(serde_json::json!({
                "spec_version": "design-spec-v1",
                "spec_id": "adc-factorial-v1",
            }))))
            .await
            .expect("propose"),
    );
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    let lineage = envelope(
        &server
            .lineage_read(Parameters(LineageReadRequest {
                artifact_ref: experiment_id.clone(),
            }))
            .await
            .expect("lineage"),
    );
    assert_eq!(
        lineage["experiment"]["design_spec"]["spec_id"],
        "adc-factorial-v1"
    );

    let population = envelope(
        &server
            .population_query(Parameters(PopulationQueryRequest {
                layer: Some("scientific_hypothesis".to_string()),
                status: None,
                created_since: None,
                limit: None,
            }))
            .await
            .expect("population"),
    );
    let row = population["experiments"]
        .as_array()
        .expect("array")
        .iter()
        .find(|row| row["id"] == experiment_id.as_str())
        .expect("row present");
    assert_eq!(row["design_spec"]["spec_id"], "adc-factorial-v1");
}

/// Criterion 3: a typed scientific outcome resolves through the tool seam and
/// is never renamed to selected/rejected.
#[tokio::test]
async fn scientific_outcome_resolves_through_the_tool_seam() {
    let server = make_server();
    let proposed = envelope(
        &server
            .experiment_propose(Parameters(design_proposal(serde_json::json!({
                "spec_version": "design-spec-v1",
            }))))
            .await
            .expect("propose"),
    );
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    register_variant(&server, &experiment_id, "arm", None).await;

    let resolved = envelope(
        &server
            .selection_record(Parameters(SelectionRecordRequest {
                experiment_id: experiment_id.clone(),
                verdict: None,
                selected_variant_id: None,
                reject_reasons: Vec::new(),
                algedonic_reference: None,
                scientific_outcome: Some("evidence_of_practical_equivalence".to_string()),
            }))
            .await
            .expect("typed outcome resolves"),
    );
    assert_eq!(
        resolved["selection"]["scientific_outcome"],
        "evidence_of_practical_equivalence"
    );
    assert_eq!(
        resolved["selection"]["verdict"],
        "evidence_of_practical_equivalence"
    );

    // Both verdict and outcome → typed invalid_argument.
    let fresh = envelope(
        &server
            .experiment_propose(Parameters(design_proposal(serde_json::json!({
                "spec_version": "design-spec-v1",
            }))))
            .await
            .expect("propose"),
    );
    let fresh_id = fresh["experiment"]["id"].as_str().expect("id").to_string();
    register_variant(&server, &fresh_id, "arm", None).await;
    let error = server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: fresh_id,
            verdict: Some("selected".to_string()),
            selected_variant_id: None,
            reject_reasons: Vec::new(),
            algedonic_reference: None,
            scientific_outcome: Some("inconclusive".to_string()),
        }))
        .await
        .expect_err("both resolutions refused");
    assert!(error.to_string().contains("exactly one"), "{error}");
}
