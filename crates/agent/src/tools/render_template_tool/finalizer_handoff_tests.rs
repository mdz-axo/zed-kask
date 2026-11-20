use super::tests::render_template_for_test;
use serde_json::{Value, json};

fn critic_context(findings: Value) -> Value {
    json!({
        "enhanced_prompt": "Preserve the supplied evidence.",
        "prompt_type": "agent-task",
        "effort_tier": "medium",
        "output_format_resolved": "inline",
        "output_path_resolved": "handoff-fixture.md",
        "audit_findings": [],
        "grill_ratings": [],
        "grill_findings": findings,
        "grill_verdict": "rewrite_needed",
        "mutations_applied": [],
        "mutations_deferred": [],
        "unfold_mode_resolved": "rewrite_only",
        "reference_models": [],
        "unfolded_plan": []
    })
}

fn section_json(rendered: &str, heading: &str) -> Value {
    let section = rendered.split_once(heading).expect("handoff section").1;
    let block = section.split_once("```json\n").expect("JSON block").1;
    let payload = block.split_once("\n```").expect("closed JSON block").0;
    serde_json::from_str(payload).expect("handoff data remains valid JSON")
}

#[test]
fn prompt_enhance_preserves_critic_findings_at_delivery() {
    let findings = json!([{
        "area": "Mechanism",
        "gap": "Keep this exact critic finding, including \"quotes\".\nSecond line.",
        "severity": "Guardrail",
        "suggested_fix_direction": "Do not reconstruct the finding from its rating.",
        "evidence": {"source": "independent fixture"}
    }]);
    for verdict in ["rewrite_needed", "fail", "pass"] {
        let mut context = critic_context(findings.clone());
        context["grill_verdict"] = json!(verdict);
        let rendered = render_template_for_test("prompt-enhance/enhance-output-render", context)
            .expect("complete finalizer context renders");
        let result: Value = serde_json::from_str(&rendered).expect("structured delivery");
        assert_eq!(result["change_log"]["grill_findings"], findings);
        assert_eq!(result["change_log"]["grill_verdict"], verdict);
        assert_eq!(
            section_json(
                result["delivered_output"].as_str().expect("human delivery"),
                "### Specific critic findings",
            ),
            findings,
        );
    }

    for mode in ["inline", "file", "both"] {
        let mut context = critic_context(json!([]));
        context["effort_tier"] = json!("low");
        context["grill_verdict"] = json!("skipped");
        context["output_format_resolved"] = json!(mode);
        let rendered = render_template_for_test("prompt-enhance/enhance-output-render", context)
            .expect("explicit empty findings render in every delivery mode");
        let result: Value = serde_json::from_str(&rendered).expect("structured empty delivery");
        assert_eq!(result["change_log"]["grill_findings"], json!([]));
        assert_eq!(result["change_log"]["grill_verdict"], "skipped");
        assert_eq!(result["change_log"]["input_errors"], json!([]));
        assert_eq!(result["output_path_written"], "");
    }

    let mut missing = critic_context(json!([]));
    missing
        .as_object_mut()
        .expect("context object")
        .remove("grill_findings");
    let error = render_template_for_test("prompt-enhance/enhance-output-render", missing)
        .expect_err("omitted critic handoff must not look like an empty review");
    assert!(error.contains("grill_findings"), "{error}");

    for malformed in [json!("[]"), json!({}), json!(7), Value::Null] {
        let rendered = render_template_for_test(
            "prompt-enhance/enhance-output-render",
            critic_context(malformed),
        )
        .expect("invalid handoff shape is surfaced in the delivery");
        let result: Value = serde_json::from_str(&rendered).expect("error delivery is valid JSON");
        assert_eq!(result["change_log"]["grill_findings"], json!([]));
        assert!(
            result["change_log"]["input_errors"]
                .as_array()
                .expect("errors")
                .iter()
                .any(|error| error
                    .as_str()
                    .is_some_and(|text| text.contains("grill_findings")))
        );
        assert!(
            result["delivered_output"]
                .as_str()
                .expect("delivery")
                .contains("INPUT ERROR")
        );
    }
}

/// Every key the classify and verify templates ask the model to emit has
/// a reader: a later stage's contract input, the PE-State gate, or the
/// SKILL.md step that consumes it. An output with no reader is a model
/// call's worth of tokens nothing uses.
#[test]
fn prompt_enhance_stage_outputs_all_have_a_reader() {
    fn output_keys(rendered: &str) -> Vec<String> {
        let section = rendered.rsplit_once("## Output").expect("output section").1;
        let block = section.split_once("```json\n").expect("JSON block").1;
        let payload = block.split_once("\n```").expect("closed JSON block").0;
        payload
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .filter_map(|rest| rest.split_once('"'))
            .map(|(key, _)| key.to_string())
            .collect()
    }
    let classify = render_template_for_test(
        "prompt-enhance/enhance-classify",
        json!({"prompt": "p", "task": "t", "effort": "medium", "output_format": "inline"}),
    )
    .expect("classify renders");
    assert_eq!(
        output_keys(&classify),
        [
            "prompt_type",
            "effort_tier",
            "output_format_resolved",
            "output_path_resolved",
            "unfold_mode_resolved",
            "anchor_terms",
            "finer_pico_framing",
            "proxy_eval_set",
            "checkability_map",
            "risks"
        ]
    );
    let verify = render_template_for_test(
        "prompt-enhance/enhance-verify",
        json!({
            "enhanced_prompt": "e", "original_prompt": "p", "prompt_type": "coding",
            "effort_tier": "high", "proxy_eval_set": [], "acceptance_criteria": [],
            "checkability_map": [], "attempt_id": 1, "reviewed_revision": 0, "round": 3
        }),
    )
    .expect("verify renders");
    // The three keys PE-State reads from a completed attempt's outcome.
    assert_eq!(output_keys(&verify), ["ratings", "findings", "verdict"]);
    for area in [
        "Recall",
        "Mechanism",
        "Rationale",
        "Edge Cases",
        "Synthesis",
    ] {
        assert!(verify.contains(&format!("\"area\": \"{area}\"")), "{area}");
    }
}

/// The conflict template asks for classifications and a strategy; the
/// ranking, the winner and the escalation cases are PS-2's. A template
/// that asked the model for a rank or a winner would duplicate the gate
/// and let the two disagree.
#[test]
fn pragmatic_semantics_conflict_template_classifies_and_does_not_rank() {
    let render = |conflicts: serde_json::Value, statements: serde_json::Value| {
        render_template_for_test(
            "pragmatic-semantics/semantics-conflict-resolve",
            json!({
                "provenance_result": {"conflicts_detected": conflicts},
                "classification_result": {"provenance": "runtime"},
                "statements": statements
            }),
        )
        .expect("conflict-resolve renders")
    };
    let detected = render(
        json!(true),
        json!([{"id": "S1", "statement": "A"}, {"id": "S2", "statement": "B"}]),
    );
    assert!(detected.contains("## Conflict Detected"));
    assert!(detected.contains("## Statements (the multi-statement case)"));
    let output = detected
        .rsplit_once("## Output Requirements")
        .expect("output section")
        .1;
    let block = output.split_once("```json\n").expect("JSON block").1;
    let shape = block.split_once("\n```").expect("closed block").0;
    let top_level: Vec<&str> = shape
        .lines()
        .filter_map(|line| line.strip_prefix("  \""))
        .filter_map(|rest| rest.split_once('"'))
        .map(|(key, _)| key)
        .collect();
    assert_eq!(
        top_level,
        ["statements", "resolution_strategy", "reasoning"]
    );
    for computed in [
        "\"rank\"",
        "\"winner\"",
        "\"ranked_statements\"",
        "escalate\"",
    ] {
        assert!(
            !shape.contains(computed),
            "{computed} is PS-2's, not the model's"
        );
    }
    // Every tier PS-2 reads is asked for, under the name PS-2 reads.
    for tier in [
        "\"id\"",
        "\"ontological_mode\"",
        "\"epistemic_mode\"",
        "\"constraint_force\"",
        "\"provenance\"",
        "\"ontology_anchor\"",
    ] {
        assert!(shape.contains(tier), "{tier} missing from the output shape");
    }

    let none = render(json!(false), serde_json::Value::Null);
    assert!(none.contains("## No Conflicts Detected"));
    assert!(!none.contains("## Statements (the multi-statement case)"));
}

fn planner_context(questions: Value) -> Value {
    json!({
        "context_summary": "Question handoff fixture, not an approved implementation plan.",
        "target_condition": "Retain the operator's supplied questions at finalization.",
        "dependency_graph": [],
        "tasks": [],
        "slices": [],
        "risks": [],
        "open_questions": questions,
        "gate_result": {"fixture_only": true},
        "refinement_directives": [],
        "phases": [],
        "checkpoints": []
    })
}

#[test]
fn task_breakdown_preserves_open_questions_at_finalization() {
    let questions = json!([
        "Which source license permits redistribution?",
        "Retain this \"quoted\" question.\nAnd its second line.",
        {"question": "Does the operator accept the source limitation?", "source": "operator"}
    ]);
    for supplied in [questions, json!([])] {
        let rendered = render_template_for_test(
            "task-breakdown/task-breakdown-write-plan",
            planner_context(supplied.clone()),
        )
        .expect("an explicit question array renders");
        assert_eq!(
            section_json(&rendered, "## Open Questions from Plan"),
            supplied
        );
    }

    let mut missing = planner_context(json!([]));
    missing
        .as_object_mut()
        .expect("context object")
        .remove("open_questions");
    let error = render_template_for_test("task-breakdown/task-breakdown-write-plan", missing)
        .expect_err("omitted questions must not look like an empty plan stage");
    assert!(error.contains("open_questions"), "{error}");

    for malformed in [json!("[]"), json!({}), json!(7), Value::Null] {
        let rendered = render_template_for_test(
            "task-breakdown/task-breakdown-write-plan",
            planner_context(malformed),
        )
        .expect("invalid question shape is surfaced");
        assert!(rendered.contains("INPUT ERROR") && rendered.contains("open_questions"));
        assert!(!rendered.contains("You are a plan-finalization agent"));
        assert!(!rendered.contains("Full markdown content of plan.md"));
    }
}

#[test]
fn task_count_facts_drive_the_critic_directive_mandate() {
    let context = |count: usize, penalty: f64| {
        json!({
            "tasks": vec![json!({"id": "fixture"}); count],
            "slices": [], "dependency_graph": [],
            "context_summary": "Count-mandate rendering fixture, not a graded plan.",
            "target_condition": "Preserve one source of truth for task-count adjustment.",
            "phases": [], "checkpoints": [],
            "task_count": count, "task_count_penalty": penalty
        })
    };
    for (count, penalty, mandated) in [(2, 0.1, true), (3, 0.0, false)] {
        let rendered = render_template_for_test(
            "task-breakdown/task-breakdown-quality-gate",
            context(count, penalty),
        )
        .expect("precomputed facts reach the real critic template");
        assert_eq!(
            section_json(&rendered, "### Task-count awareness"),
            json!({"task_count": count, "task_count_penalty": penalty}),
        );
        assert_eq!(
            rendered.contains("Required count-induced refinement"),
            mandated
        );
        assert!(rendered.contains("\"base_gate_scores\""));
        assert!(!rendered.contains("\"gate_scores\": {"));
    }
    for missing in ["task_count", "task_count_penalty"] {
        let mut incomplete = context(2, 0.1);
        incomplete
            .as_object_mut()
            .expect("context object")
            .remove(missing);
        let error =
            render_template_for_test("task-breakdown/task-breakdown-quality-gate", incomplete)
                .expect_err("missing deterministic facts must not be fabricated by the critic");
        assert!(error.contains(missing), "{error}");
    }
}

const EQM_HELPS: [&str; 6] = [
    "statistical_reasoning",
    "fact_based",
    "concrete_reasoning",
    "forecast_rationale_align",
    "best_practices",
    "statistical_causal_blend",
];
const EQM_HURTS: [&str; 6] = [
    "gut_based",
    "simplification_bias",
    "confirmation_bias",
    "extreme_confidence",
    "forecast_rationale_misalign",
    "speculative_terms",
];

/// One `per_rationale_scores` record: every helps marker at `helps`,
/// every hurts marker at 0, then the named overrides. The composite is
/// derived, so a fixture cannot disagree with its own scores.
fn eqm_record(
    index: usize,
    question: &str,
    forecaster: Option<&str>,
    helps: u8,
    overrides: &[(&str, u8)],
) -> serde_json::Value {
    let score = |id: &str, base: u8| {
        overrides
            .iter()
            .find(|(name, _)| *name == id)
            .map_or(base, |(_, value)| *value)
    };
    let mut composite = 0i64;
    let mut scores = Vec::new();
    for id in EQM_HELPS {
        let value = score(id, helps);
        composite += i64::from(value);
        scores.push(json!({"id": id, "score": value}));
    }
    for id in EQM_HURTS {
        let value = score(id, 0);
        composite -= i64::from(value);
        scores.push(json!({"id": id, "score": value}));
    }
    let mut record = json!({
        "rationale_index": index, "question": question,
        "composite_score": composite, "scores": scores
    });
    if let Some(forecaster) = forecaster {
        record["forecaster_id"] = json!(forecaster);
    }
    record
}

fn eqm_aggregate(records: Vec<serde_json::Value>) -> serde_json::Value {
    let rendered = render_template_for_test(
        "eqm/eqm-aggregate",
        json!({"per_rationale_scores": records}),
    )
    .expect("the aggregate renders");
    serde_json::from_str(&rendered).expect("the aggregate render is a JSON document")
}

fn eqm_ids(markers: &serde_json::Value) -> Vec<&str> {
    markers
        .as_array()
        .expect("marker list")
        .iter()
        .map(|marker| marker["id"].as_str().expect("marker id"))
        .collect()
}

#[test]
fn eqm_aggregate_computes_composites_screens_and_bias() {
    // r0: helps 9, hurts 4, composite 5; two hurts at 2.
    // r1: all on target, composite 12. r2: helps 6, one hurts at 1.
    let out = eqm_aggregate(vec![
        eqm_record(
            0,
            "Q1",
            Some("f1"),
            2,
            &[
                ("statistical_reasoning", 0),
                ("best_practices", 1),
                ("gut_based", 2),
                ("extreme_confidence", 2),
            ],
        ),
        eqm_record(1, "Q1", Some("f2"), 2, &[]),
        eqm_record(2, "Q2", Some("f1"), 1, &[("speculative_terms", 1)]),
    ]);

    assert_eq!(out["status"], "complete");
    assert_eq!(out["incomplete_rationales"], json!([]));
    assert_eq!(
        out["forecast_level_composite"],
        json!([
            {"question": "Q1", "mean_composite": 8.5, "red_flag_count": 2, "green_flag_count": 10, "rationale_count": 2},
            {"question": "Q2", "mean_composite": 5.0, "red_flag_count": 0, "green_flag_count": 0, "rationale_count": 1}
        ])
    );
    assert_eq!(
        out["forecaster_level_composite"],
        json!([
            {"forecaster_id": "f1", "mean_composite": 5.0, "red_flag_frequency": 1.0, "green_flag_frequency": 2.0, "rationale_count": 2},
            {"forecaster_id": "f2", "mean_composite": 12.0, "red_flag_frequency": 0.0, "green_flag_frequency": 6.0, "rationale_count": 1}
        ])
    );
    // (1 - 4 - 2) / (2 * 3)
    assert_eq!(out["overconfidence_bias"], json!(-0.8333));

    // EQM-2: two hurts at 2 screens red; composite >= 4 endorses only
    // with no hurts above 0 (r0 and r2 both have composite 5).
    let red = out["red_flag_screen"].as_array().expect("red screen");
    assert_eq!(red.len(), 1);
    assert_eq!(red[0]["rationale_index"], 0);
    assert_eq!(
        red[0]["red_flags"],
        json!(["Extreme Confidence", "Gut Based"])
    );
    let green = out["green_flag_endorsement"]
        .as_array()
        .expect("green endorsement");
    assert_eq!(green.len(), 1);
    assert_eq!(green[0]["rationale_index"], 1);
    assert_eq!(green[0]["confidence"], "weak");

    // Priority order: hurts at 2, hurts at 1, helps at 0, helps at 1.
    let summary = &out["failing_marker_summary"];
    assert_eq!(
        eqm_ids(&summary[0]["failing_markers"]),
        [
            "extreme_confidence",
            "gut_based",
            "statistical_reasoning",
            "best_practices"
        ]
    );
    assert_eq!(summary[1]["failing_markers"], json!([]));
    assert_eq!(
        eqm_ids(&summary[2]["failing_markers"])[0],
        "speculative_terms"
    );
    let first = &summary[0]["failing_markers"][0];
    assert_eq!(first["target_score"], 0);
    assert_eq!(first["gap"], 2);
    assert!(
        first["description"]
            .as_str()
            .is_some_and(|text| text.starts_with("The forecaster expresses a high degree"))
    );
}

#[test]
fn eqm_aggregate_keeps_case_distinct_groups_apart() {
    let out = eqm_aggregate(vec![
        eqm_record(0, "q", Some("F1"), 2, &[]),
        eqm_record(1, "Q", Some("f1"), 2, &[]),
        eqm_record(2, "Q", None, 2, &[]),
    ]);
    let counts = |field: &str, key: &str| -> Vec<(String, u64)> {
        out[field]
            .as_array()
            .expect("group list")
            .iter()
            .map(|group| {
                (
                    group[key].as_str().expect("group key").to_string(),
                    group["rationale_count"].as_u64().expect("count"),
                )
            })
            .collect()
    };
    assert_eq!(
        counts("forecast_level_composite", "question"),
        [("Q".to_string(), 2), ("q".to_string(), 1)]
    );
    // The rationale with no forecaster_id is in no forecaster group.
    assert_eq!(
        counts("forecaster_level_composite", "forecaster_id"),
        [("F1".to_string(), 1), ("f1".to_string(), 1)]
    );
    assert_eq!(out["overconfidence_bias"], json!(-2.0));
}

#[test]
fn eqm_aggregate_refuses_incomplete_batches() {
    let good = || eqm_record(0, "Q1", Some("f1"), 2, &[]);
    let empty_aggregates = |out: &serde_json::Value| {
        for field in [
            "forecast_level_composite",
            "forecaster_level_composite",
            "red_flag_screen",
            "green_flag_endorsement",
            "failing_marker_summary",
        ] {
            assert_eq!(out[field], json!([]), "{field}");
        }
        assert!(out["overconfidence_bias"].is_null());
    };

    let mut absent = eqm_record(1, "Q1", Some("f2"), 2, &[]);
    absent["scores"]
        .as_array_mut()
        .expect("scores")
        .retain(|score| score["id"] != "fact_based");
    let mut tool_missing = eqm_record(2, "Q2", Some("f1"), 2, &[]);
    tool_missing["missing_eqms"] = json!(["gut_based"]);
    let mut duplicated = eqm_record(3, "Q2", Some("f1"), 2, &[]);
    let repeat = duplicated["scores"][0].clone();
    duplicated["scores"]
        .as_array_mut()
        .expect("scores")
        .push(repeat);
    let mut out_of_range = eqm_record(4, "Q2", Some("f1"), 2, &[]);
    out_of_range["scores"][6]["score"] = json!(3);
    let mut wrong_composite = eqm_record(5, "Q2", Some("f1"), 2, &[]);
    wrong_composite["composite_score"] = json!(9);

    let out = eqm_aggregate(vec![
        good(),
        absent,
        tool_missing,
        duplicated,
        out_of_range,
        wrong_composite,
    ]);
    assert_eq!(out["status"], "incomplete");
    empty_aggregates(&out);
    let reasons = out["incomplete_rationales"]
        .as_array()
        .expect("incomplete list");
    assert_eq!(
        reasons
            .iter()
            .map(|reason| reason["rationale_index"].as_u64().expect("index"))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5],
        "the complete rationale is not listed"
    );
    assert_eq!(reasons[0]["absent_ids"], json!(["fact_based"]));
    assert_eq!(reasons[1]["missing_eqms"], json!(["gut_based"]));
    assert_eq!(reasons[2]["duplicate_ids"], true);
    assert_eq!(reasons[3]["out_of_range_ids"], json!(["gut_based"]));
    assert_eq!(reasons[4]["composite_mismatch"], true);

    let out = eqm_aggregate(Vec::new());
    assert_eq!(out["status"], "unassessed");
    assert_eq!(out["incomplete_rationales"], json!([]));
    empty_aggregates(&out);
}

#[test]
fn eqm_improve_templates_compute_the_failing_markers() {
    let record = eqm_record(
        0,
        "Q1",
        None,
        2,
        &[
            ("statistical_reasoning", 0),
            ("best_practices", 1),
            ("gut_based", 2),
            ("extreme_confidence", 2),
        ],
    );
    let scores = record["scores"].clone();

    let predict = render_template_for_test(
        "eqm/eqm-imp-predict",
        json!({"scores": scores, "current_composite": 5}),
    )
    .expect("predict renders");
    let selected = section_json(&predict, "## The Marker This Iteration Addresses");
    assert_eq!(eqm_ids(&selected), ["extreme_confidence"]);
    assert_eq!(selected[0]["current_score"], 2);
    assert_eq!(selected[0]["target_score"], 0);

    let on_target = eqm_record(0, "Q1", None, 2, &[]);
    let predict = render_template_for_test(
        "eqm/eqm-imp-predict",
        json!({"scores": on_target["scores"], "current_composite": 12}),
    )
    .expect("predict renders with nothing off target");
    assert_eq!(
        section_json(&predict, "## The Marker This Iteration Addresses"),
        json!([])
    );

    let experiment = render_template_for_test(
        "eqm/eqm-imp-experiment",
        json!({
            "rationale": "RATIONALE-TEXT", "forecast_probability": 0.35,
            "question": "Q1", "prediction": {"hypothesis": "H"}, "scores": scores
        }),
    )
    .expect("experiment renders");
    let failing = section_json(&experiment, "## Failing Markers");
    assert_eq!(
        eqm_ids(&failing),
        [
            "extreme_confidence",
            "gut_based",
            "statistical_reasoning",
            "best_practices"
        ]
    );
    assert!(experiment.contains("RATIONALE-TEXT"));
    assert!(experiment.contains("0.35"));
}

#[test]
fn eqm_validate_returns_the_constant_result_without_outcomes() {
    let rendered = render_template_for_test(
        "eqm/eqm-validate",
        json!({"forecaster_level_composite": [], "realized_outcomes": [], "outcome_statistics": {}}),
    )
    .expect("validate renders without outcomes");
    let out: serde_json::Value =
        serde_json::from_str(&rendered).expect("the no-outcomes render is a JSON document");
    assert_eq!(out["validation_verdict"], "undetermined");
    assert!(out["gaming_suspected"].is_null());
    assert_eq!(out["validation_findings"][0]["status"], "skipped");

    let rendered = render_template_for_test(
        "eqm/eqm-validate",
        json!({
            "forecaster_level_composite": [{"forecaster_id": "f1", "mean_composite": 5.0}],
            "realized_outcomes": [{"question": "Q1", "forecast_probability": 0.35, "outcome": false, "forecaster_id": "f1"}],
            "outcome_statistics": {"forecasters_resolved": 1, "brier_by_forecaster": {"f1": 0.1225}, "composite_accuracy_r": "Undetermined"}
        }),
    )
    .expect("validate renders with outcomes");
    assert!(rendered.contains("You are an EQM validation analyst"));
    assert!(rendered.contains("0.1225"));
    assert!(rendered.contains("\"composite_accuracy_r\":\"Undetermined\""));

    // A forecaster with resolved outcomes and no Brier score: the fixed
    // result names them, and no prompt is rendered.
    let rendered = render_template_for_test(
        "eqm/eqm-validate",
        json!({
            "forecaster_level_composite": [],
            "realized_outcomes": [
                {"question": "Q1", "forecast_probability": 0.35, "outcome": false, "forecaster_id": "f1"},
                {"question": "Q1", "forecast_probability": 0.6, "outcome": true, "forecaster_id": "f2"}
            ],
            "outcome_statistics": {"forecasters_resolved": 1, "brier_by_forecaster": {"f1": 0.1225}, "composite_accuracy_r": "Undetermined"}
        }),
    )
    .expect("validate renders with an uncovered forecaster");
    let out: serde_json::Value =
        serde_json::from_str(&rendered).expect("the uncovered render is a JSON document");
    assert_eq!(out["validation_verdict"], "undetermined");
    assert_eq!(out["validation_findings"][0]["forecasters"], json!(["f2"]));
}

/// `predictive-markers.j2` is generated from `eqm-catalog.yaml`. A catalog
/// edit that is not regenerated into the fragment fails here: ids, names
/// and directions must be equal, and each description must be the
/// catalog's folded text.
#[test]
fn eqm_marker_specs_match_the_catalog() {
    let base =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../kask/registry/templates");
    let catalog: serde_yaml::Value = serde_yaml::from_str(
        &std::fs::read_to_string(base.join("eqm/eqm-catalog.yaml")).expect("catalog"),
    )
    .expect("catalog parses as YAML");
    let text = |value: &serde_yaml::Value, key: &str| {
        value[key]
            .as_str()
            .unwrap_or_else(|| panic!("catalog marker lacks {key}"))
            .trim()
            .to_string()
    };
    let mut expected = Vec::new();
    for family in catalog["categories"].as_sequence().expect("families") {
        for marker in family["markers"].as_sequence().expect("markers") {
            if marker["predictive"].as_bool() == Some(true) {
                expected.push(json!({
                    "id": text(marker, "id"), "name": text(marker, "name"),
                    "direction": text(marker, "direction"),
                    "description": text(marker, "description")
                }));
            }
        }
    }
    assert_eq!(expected.len(), 12, "the catalog marks twelve predictive");

    let mut environment = minijinja::Environment::new();
    super::bind_registry_loader(&mut environment, &base);
    let rendered = environment
        .render_str(
            "{% from \"eqm/predictive-markers.j2\" import marker_specs %}{{ marker_specs | tojson }}",
            json!({}),
        )
        .expect("the fragment exports marker_specs");
    let specs: serde_json::Value = serde_json::from_str(&rendered).expect("marker_specs JSON");
    assert_eq!(specs, serde_json::Value::Array(expected));
}

#[test]
fn seam_audit_final_report_computes_its_facts() {
    let context = json!({
        "task": "fixture",
        "security_track": {"findings": [], "defense_layer_coverage": {"layers": [{"id": 1}, {"id": 2}], "verdict": "Conditional"}},
        "arch_track": {"findings": [], "dead_surface_summary": {}},
        "ui_track": {"findings": [], "pattern_gap_summary": {}},
        "annotated_findings": [
            {"id": "KS-01", "constraint_force": "blocking"},
            {"id": "KS-02", "constraint_force": "blocking"},
            {"id": "RA-01", "constraint_force": "directing"}
        ],
        "gate_b": {"per_track": [2, 1, 0], "per_severity": [0, 1, 1, 1, 0]},
        "ksa_2": {"top3": ["KS-01", "RA-01"], "stable_under_perturbation": false, "excluded": ["UI-07"]},
        "applied_remediations": [{"id": "KS-01", "test_description": "pins it"}],
        "hard_stop": {"triggered": false},
        "convergence_score": 0
    });
    let rendered = render_template_for_test("kask-seam-audit/final-report", context)
        .expect("final-report renders");
    assert_eq!(
        section_json(&rendered, "## Computed facts"),
        json!({
            "findings_per_track": {"security": 2, "architecture": 1, "ui": 0},
            "findings_per_severity": {"critical": 0, "high": 1, "medium": 1, "low": 1, "info": 0},
            "defense_layers_declared": 2,
            "defense_layer_verdict": "Conditional",
            "mcda_top3": ["KS-01", "RA-01"],
            "mcda_stable_under_perturbation": false,
            "mcda_excluded": ["UI-07"],
            "applied_remediation_ids": ["KS-01"],
            // KS-02 is blocking and was not applied; KS-01 was.
            "blocking_findings_unremediated": ["KS-02"],
            "hard_stop_triggered": false,
            "convergence_score": 0
        })
    );
    // The model's output shape no longer carries a computed value.
    let output = rendered
        .split_once("## Output (JSON only)")
        .expect("output section")
        .1;
    for retyped in [
        "convergence_score",
        "mcda_top3",
        "hard_stop_triggered",
        "iterations",
    ] {
        assert!(
            !output.contains(&format!("\"{retyped}\"")),
            "{retyped} is retyped"
        );
    }
}

#[test]
fn seam_audit_remediate_shows_only_the_top_ranked_candidates() {
    let context = |defects: Option<serde_json::Value>| {
        let mut context = json!({
            "task": "fixture", "top3": ["KS-01", "RA-01"],
            "candidates": [
                {"id": "KS-01", "remediation": "tighten the gate"},
                {"id": "UI-07", "remediation": "not ranked"},
                {"id": "RA-01", "remediation": "delete the helper"}
            ],
            "divergence_surface": ["crates/zed/src/main.rs", "crates/hkask-steer/"],
            "seam_scope": "kask/"
        });
        if let Some(defects) = defects {
            context["gate_defects"] = defects;
        }
        context
    };
    let rendered = render_template_for_test("kask-seam-audit/remediate", context(None))
        .expect("remediate renders");
    assert_eq!(
        section_json(&rendered, "Their descriptions:"),
        json!([
            {"id": "KS-01", "remediation": "tighten the gate"},
            {"id": "RA-01", "remediation": "delete the helper"}
        ])
    );
    assert!(!rendered.contains("not ranked"));
    assert_eq!(
        section_json(&rendered, "## Which files may be touched"),
        json!(["crates/zed/src/main.rs", "crates/hkask-steer/"])
    );
    assert!(
        !rendered.contains("within_kask"),
        "the seam test is Gate C's"
    );
    assert!(!rendered.contains("Gate C defects from the previous attempt"));

    let repair = render_template_for_test(
        "kask-seam-audit/remediate",
        context(Some(json!([["KS-01", "remediation_missing_test"]]))),
    )
    .expect("remediate renders a repair");
    assert_eq!(
        section_json(&repair, "## Gate C defects from the previous attempt"),
        json!([["KS-01", "remediation_missing_test"]])
    );
}

#[test]
fn seam_audit_templates_carry_gate_defects_only_on_a_repair() {
    let adjudicate = |defects: Option<serde_json::Value>| {
        let mut context = json!({
            "task": "fixture",
            "security_findings": [{"id": "KS-01", "severity": "high", "file_line": "kask/a.rs:1"}],
            "arch_findings": [], "ui_findings": [],
            "defense_layer_coverage": {"layers": [], "verdict": "Pass"}
        });
        if let Some(defects) = defects {
            context["gate_defects"] = defects;
        }
        render_template_for_test("kask-seam-audit/adjudicate", context).expect("adjudicate renders")
    };
    let first = adjudicate(None);
    assert!(!first.contains("Gate FA defects from the previous attempt"));
    assert_eq!(
        section_json(&first, "security:"),
        json!([{"id": "KS-01", "severity": "high", "file_line": "kask/a.rs:1"}])
    );
    let repair = adjudicate(Some(json!([["KS-01", "revoked_function_not_retired"]])));
    assert_eq!(
        section_json(&repair, "## Gate FA defects from the previous attempt"),
        json!([["KS-01", "revoked_function_not_retired"]])
    );

    for (template, context) in [
        (
            "kask-seam-audit/audit-security",
            json!({"task": "t", "seam_scope": "kask/", "prior_verdicts": []}),
        ),
        (
            "kask-seam-audit/audit-architecture",
            json!({"task": "t", "seam_scope": "kask/"}),
        ),
        ("kask-seam-audit/audit-ui", json!({"task": "t"})),
    ] {
        let first = render_template_for_test(template, context.clone()).expect("track renders");
        assert!(
            !first.contains("Gate B defects from the previous attempt"),
            "{template}"
        );
        let mut with_defects = context;
        with_defects["gate_defects"] = json!([["X-1", "invalid_severity"]]);
        let repair = render_template_for_test(template, with_defects).expect("track repair");
        assert_eq!(
            section_json(&repair, "## Gate B defects from the previous attempt"),
            json!([["X-1", "invalid_severity"]]),
            "{template}"
        );
    }
}
