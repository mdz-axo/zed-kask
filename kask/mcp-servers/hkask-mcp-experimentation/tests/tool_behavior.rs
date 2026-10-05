//! Tool-behavior contract tests — every tool invoked through its public
//! `Parameters<T>` seam (the registry README's testing standard; the CI gate
//! is `kask/scripts/check-mcp-tool-tests.sh`).
//!
//! The server is constructed directly over an in-memory SQLCipher driver, so
//! these tests exercise the real tool surface end-to-end: request shapes,
//! validation, per-variant error classification, and the full protocol
//! lifecycle — the in-process rehearsal of the §P8.5 run-one-through.

use hkask_mcp_experimentation::server::{
    ExperimentProposeRequest, ExperimentationServer, FitnessRecordRequest, LineageReadRequest,
    PopulationQueryRequest, SelectionRecordRequest, VariantRegisterRequest,
};
use hkask_mcp_experimentation::store::ExperimentationStore;
use hkask_mcp_experimentation::types::Prediction;
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
        prediction: Prediction {
            claim: "variant B wins".to_string(),
            confidence: 0.7,
        },
        budget: AnyJsonValue(serde_json::json!({"energy_budget": "one eval run", "max_runs": 18})),
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
    let response = server
        .variant_register(Parameters(VariantRegisterRequest {
            experiment_id: experiment_id.to_string(),
            genotype_config: AnyJsonValue(serde_json::json!({"name": name})),
            parent_variant_id: parent.map(str::to_string),
            variant_key: None,
        }))
        .await
        .expect("variant_register succeeds");
    envelope(&response)
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
    let baseline = register_variant(&server, &experiment_id, "baseline", None).await;
    assert_eq!(baseline["status"], "registered");
    let baseline_id = baseline["variant"]["id"].as_str().expect("id").to_string();
    let challenger =
        register_variant(&server, &experiment_id, "challenger", Some(&baseline_id)).await;
    let challenger_id = challenger["variant"]["id"]
        .as_str()
        .expect("id")
        .to_string();

    // Test — recorded report refs only.
    let fitness = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id: experiment_id.clone(),
            variant_id: challenger_id.clone(),
            runs: vec!["swarm_eval_agent_local run 42".to_string()],
            run_count: 3,
            scores: AnyJsonValue(serde_json::json!({"pass_rate": 0.8, "total_tokens": 1200})),
        }))
        .await
        .expect("fitness_record succeeds");
    let fitness = envelope(&fitness);
    assert_eq!(fitness["fitness"]["variant_id"], challenger_id.as_str());
    assert_eq!(fitness["fitness"]["run_count"], 3);
    assert_eq!(
        fitness["fitness"]["runs"][0],
        "swarm_eval_agent_local run 42"
    );

    // Select — the fossil resolves the experiment.
    let selection = server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: experiment_id.clone(),
            verdict: "selected".to_string(),
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
            status: Some("resolved".to_string()),
            created_since: None,
            limit: None,
        }))
        .await
        .expect("population_query succeeds");
    let population = envelope(&population);
    assert_eq!(population["count"], 1);
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
    assert!(error.to_string().contains("skill, agent_card"), "{error}");

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

    // Fitness with no recorded runs is refused — never simulated (§P8.3).
    let proposed = propose(&server, "skill").await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let variant = register_variant(&server, &experiment_id, "v1", None).await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs: Vec::new(),
            run_count: 1,
            scores: AnyJsonValue(serde_json::json!({})),
        }))
        .await
        .expect_err("empty runs rejected");
    assert!(error.to_string().contains("runs"), "{error}");

    // A zero run_count is refused — a fitness record must state how many
    // rollouts its reports cover (§P8.9 step 2).
    let proposed = propose(&server, "skill").await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let variant = register_variant(&server, &experiment_id, "v2", None).await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    let error = server
        .fitness_record(Parameters(FitnessRecordRequest {
            experiment_id,
            variant_id,
            runs: vec!["r".to_string()],
            run_count: 0,
            scores: AnyJsonValue(serde_json::json!({})),
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
            variant_key: None,
        }))
        .await
        .expect_err("unknown experiment is not found");
    assert!(error.to_string().contains("not found"), "{error}");

    // Drive one experiment to resolved, then verify the refusal.
    let proposed = propose(&server, "skill").await;
    let experiment_id = proposed["experiment"]["id"]
        .as_str()
        .expect("id")
        .to_string();
    let variant = register_variant(&server, &experiment_id, "v1", None).await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    server
        .selection_record(Parameters(SelectionRecordRequest {
            experiment_id: experiment_id.clone(),
            verdict: "selected".to_string(),
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
            verdict: "selected".to_string(),
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
            verdict: "rejected".to_string(),
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
    let variant = register_variant(&server, &experiment_id, "baseline", None).await;
    let variant_id = variant["variant"]["id"].as_str().expect("id").to_string();
    for _ in 0..2 {
        server
            .fitness_record(Parameters(FitnessRecordRequest {
                experiment_id: experiment_id.clone(),
                variant_id: variant_id.clone(),
                runs: vec!["harness report".to_string()],
                run_count: 9,
                scores: AnyJsonValue(serde_json::json!({})),
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
