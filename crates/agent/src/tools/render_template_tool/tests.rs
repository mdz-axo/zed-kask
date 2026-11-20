use super::*;

fn registry_template_base() -> std::path::PathBuf {
    let base =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../kask/registry/templates");
    assert!(
        base.is_dir(),
        "registry template directory does not exist: {}",
        base.display()
    );
    base
}

pub(super) fn render_template_for_test(
    template_ref: &str,
    context: serde_json::Value,
) -> Result<String, String> {
    let base = registry_template_base();
    let content = read_template_file(&base, template_ref)?;
    let context: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_value(context).map_err(|error| error.to_string())?;
    validate_contract_inputs(&content, &context)?;
    let body = strip_frontmatter(&content);
    let mut environment = minijinja::Environment::new();
    bind_registry_loader(&mut environment, &base);
    environment
        .render_str(&body, &context)
        .map_err(|error| error.to_string())
}

#[test]
fn mcp_review_artifact_serializes_supplied_evidence_without_reconstruction() {
    let context = serde_json::json!({
        "server": "fixture-server", "run_date": "2026-10-20",
        "tools": [{"name": "wire_alias", "status": "pending-rulings", "anchor": "unresolved"}],
        "findings": [{"id": "F1", "tool": "wire_alias", "claim": "Quoted \"evidence\"\nnext line", "status": "pending-rulings"}],
        "reconciliation": {
            "input_valid": true, "source_count": 3, "live_count": 3, "reviewed_count": 1,
            "source_live_match": false, "review_complete": false,
            "source_only_names": ["old"], "live_only_names": ["new"],
            "not_reviewed_names": ["old", "other"], "unknown_reviewed_names": [],
            "result": "source_live_drift"
        },
        "gated_pending": ["The source ruling remains open"],
        "notes": ["Not a complete server review", "Unicode: λ; braces: {x}"],
        "efficiency_metric_space": {"harness": "fixture", "axes": [{"axis": "tokens", "type": "blocked"}]},
        "anchor_registry": {"ruled": [], "pending": ["unresolved"]},
        "citation_check": {"result": "not_run", "checked": null},
        "ask_first": {"batched_ask_presented": false, "defaults_applied": ["flagged technical default"]}
    });
    let output = render_template_for_test("mcp-tool-review/findings-artifact", context.clone())
        .expect("completed-context deterministic formatter renders");
    let artifact: serde_json::Value = serde_json::from_str(&output)
        .expect("the render itself is the JSON artifact, not a generation prompt");
    let template = read_template_file(
        &registry_template_base(),
        "mcp-tool-review/findings-artifact",
    )
    .expect("artifact template exists");
    let metadata: serde_yaml::Value = serde_yaml::from_str(
        template_metadata_header(&template).expect("artifact declares its contract"),
    )
    .expect("artifact metadata is valid YAML");
    let declared_fields = metadata["contract"]["output"]
        .as_mapping()
        .expect("artifact output contract is a mapping")
        .keys()
        .map(|key| key.as_str().expect("output names are strings"))
        .collect::<std::collections::BTreeSet<_>>();
    let rendered_fields = artifact
        .as_object()
        .expect("artifact is a flat JSON object")
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        declared_fields, rendered_fields,
        "output contract matches the artifact"
    );
    assert_eq!(artifact["capability"], "mcp-tool-review");
    for key in [
        "server",
        "run_date",
        "tools",
        "findings",
        "reconciliation",
        "notes",
        "efficiency_metric_space",
        "anchor_registry",
        "citation_check",
        "ask_first",
    ] {
        assert_eq!(artifact[key], context[key], "lossless {key} handoff");
    }
    assert_eq!(artifact["gated_pending_rulings"], context["gated_pending"]);
    assert_eq!(artifact["reconciliation"]["review_complete"], false);
    assert_eq!(artifact["citation_check"]["result"], "not_run");
    for key in [
        "reconciliation",
        "efficiency_metric_space",
        "anchor_registry",
        "citation_check",
        "ask_first",
    ] {
        let mut missing = context.clone();
        missing.as_object_mut().expect("context object").remove(key);
        assert!(
            render_template_for_test("mcp-tool-review/findings-artifact", missing)
                .expect_err("missing evidence must not be reconstructed")
                .contains(key)
        );
    }
}

// Run the authored filters, not a Rust translation of the decision rules.
fn grill_jq(section: &str, input: serde_json::Value) -> Result<serde_json::Value, String> {
    authored_jq("grill-me", section, input)
}

fn authored_jq(
    skill: &str,
    section: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    authored_jq_with_args(skill, section, &[], input)
}

/// Run the first ```jq fence under `### {section}` of a SKILL.md over
/// `input`, with extra jq arguments (e.g. `--arg since …`) bound the way
/// the skill's documented command binds them. The fence may be indented
/// inside a list item; the filter is read up to its closing fence.
fn authored_jq_with_args(
    skill: &str,
    section: &str,
    extra_args: &[&str],
    input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use smol::{io::AsyncWriteExt, process::Command};
    use std::process::Stdio;
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.agents/skills")
        .join(skill)
        .join("SKILL.md");
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let filter = source
        .split_once(&format!("### {section}\n"))
        .and_then(|(_, tail)| tail.split_once("```jq\n"))
        .and_then(|(_, tail)| tail.split_once("```"))
        .map(|(filter, _)| filter.trim())
        .ok_or_else(|| format!("missing authored {section} filter"))?;
    let mut child = Command::new("jq")
        .arg("-ce")
        .args(extra_args)
        .arg(filter)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("jq required: {e}"))?;
    smol::block_on(async {
        let bytes = serde_json::to_vec(&input).map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().expect("piped input");
        stdin.write_all(&bytes).await.map_err(|e| e.to_string())?;
        drop(stdin);
        let output = child.output().await.map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
    })
}

#[test]
fn superforecasting_record_and_admission_preserve_source_ownership() {
    use serde_json::json;
    let events = json!([
        {"id":"root", "question":"Root", "deadline":"2027-01-01", "probability":0.6, "depends_on":[]},
        {"id":"outcome", "question":"QUESTION_SENTINEL", "deadline":"2027-01-01", "probability":0.5,
         "depends_on":[{"parent_event_ids":["root"],"conditionals":[0.2,0.8]}]}
    ]);
    let mut input = json!({
        "stage0":{"forecasting_question":"QUESTION_SENTINEL", "resolution_criteria":"CRITERIA_SENTINEL", "resolution_deadline":"2027-01-01"},
        "stage6":{"final_probability":0.56,"confidence_level":"medium"},
        "outcome_node_id":"outcome",
        "quantify":{"arguments":{"events":events},"response":{"nodes":[
            {"id":"root","question":"Root","deadline":"2027-01-01","marginal_probability":0.6},
            {"id":"outcome","question":"QUESTION_SENTINEL","deadline":"2027-01-01","marginal_probability":0.56}
        ]}},
        "declaration":{"forecast_id":"caller-owned","events":events,"resolver":"declared-test-source",
            "resolution_at":{"root":"2027-01-01T12:00:00Z","outcome":"2027-01-01T12:00:00Z"}},
        "narrative":{"reasoning_summary":"EVIDENCE_SENTINEL","key_assumptions":["ASSUMPTION_SENTINEL"]},
        "receipt":null
    });
    let binding = authored_jq("superforecasting", "Record assembly", input.clone())
        .expect("dependent marginal, not raw prior, binds");
    assert_eq!(
        binding["declaration"], input["declaration"],
        "pass exact arguments, not a reconstructed tree"
    );
    let record = &binding["record_result"];
    assert_eq!(record["admission_receipt"], json!(null));
    assert_eq!(record["forecast_record"]["probability"], 0.56);
    assert_eq!(
        record["forecast_record"]["resolution_criteria"],
        "CRITERIA_SENTINEL"
    );
    assert_eq!(record["tracking_id"], "caller-owned");
    assert!(record["forecast_record"].get("timestamp").is_none());
    for (path, value) in [
        ("final_probability", json!(0.42)),
        ("final_probability", json!(null)),
    ] {
        let mut bad = input.clone();
        bad["stage6"][path] = value;
        assert!(authored_jq("superforecasting", "Record assembly", bad).is_err());
    }
    let mut stale = input.clone();
    stale["declaration"]["events"][1]["probability"] = json!(0.42);
    assert!(authored_jq("superforecasting", "Record assembly", stale).is_err());
    for key in ["question", "deadline", "id"] {
        let mut bad = input.clone();
        bad["quantify"]["response"]["nodes"][1][key] = json!("changed");
        assert!(
            authored_jq("superforecasting", "Record assembly", bad).is_err(),
            "changed {key}"
        );
    }
    let mut duplicate = input.clone();
    let node = duplicate["quantify"]["response"]["nodes"][1].clone();
    duplicate["quantify"]["response"]["nodes"]
        .as_array_mut()
        .expect("nodes")
        .push(node);
    assert!(authored_jq("superforecasting", "Record assembly", duplicate).is_err());
    for key in ["probability", "timestamp", "tracking_id"] {
        let mut injected = input.clone();
        injected["narrative"][key] = json!("injected");
        assert!(
            authored_jq("superforecasting", "Record assembly", injected).is_err(),
            "reject narrative {key}"
        );
    }
    for malformed in [
        json!(null),
        json!({}),
        json!({"reasoning_summary":"ok","key_assumptions":"not an array"}),
    ] {
        let mut bad = input.clone();
        bad["narrative"] = malformed;
        assert!(authored_jq("superforecasting", "Record assembly", bad).is_err());
    }
    let mut omitted = input.clone();
    omitted.as_object_mut().expect("handoff").remove("receipt");
    assert!(authored_jq("superforecasting", "Record assembly", omitted).is_err());
    let quality_rows = json!({"crit":{"calibration_realism":[1,1,1,1],"confidence_justification":[1,1,1,1],"evidence_trail":[1,1,1,1],"record_completeness":[1,1,1,1]},"gate_findings":[]});
    assert_eq!(
        authored_jq(
            "superforecasting",
            "Quality input shape",
            quality_rows.clone()
        )
        .expect("raw critic object"),
        quality_rows["crit"]
    );
    for malformed in [
        json!([]),
        json!([["calibration_realism", [1, 1, 1, 1]]]),
        json!({"calibration_realism":[1,1,1,1]}),
    ] {
        let mut bad = quality_rows.clone();
        bad["crit"] = malformed;
        assert!(authored_jq("superforecasting", "Quality input shape", bad).is_err());
    }
    input["receipt"] = json!({"forecast_id":"caller-owned","status":"declared","event_count":2,
        "resolver":"declared-test-source","resolution_at":input["declaration"]["resolution_at"]});
    let admitted = authored_jq("superforecasting", "Record assembly", input.clone())
        .expect("actual receipt shape");
    let admitted = &admitted["record_result"];
    assert_eq!(admitted["admission_receipt"], input["receipt"]);
    for key in [
        "forecast_id",
        "status",
        "resolver",
        "resolution_at",
        "event_count",
    ] {
        let mut bad = input.clone();
        bad["receipt"][key] = json!(null);
        assert!(
            authored_jq("superforecasting", "Record assembly", bad).is_err(),
            "receipt {key}"
        );
    }
    let prompt = render_template_for_test("superforecasting/stage_7_record", json!({
        "forecasting_question":"QUESTION_SENTINEL","final_probability":0.56,"confidence_level":"medium",
        "pipeline_summary":{"stage0":input["stage0"],"stage6":input["stage6"],"evidence":["EVIDENCE_SENTINEL"]},
        "resolution_criteria":"CRITERIA_SENTINEL","expiration_date":"2027-01-01"
    })).expect("real narrative renderer");
    assert!(
        prompt.contains("EVIDENCE_SENTINEL"),
        "raw evidence survives rather than only counts"
    );
    assert!(!prompt.contains("<ISO 8601 timestamp>"));
    let quality = render_template_for_test(
        "superforecasting/forecast-quality-gate",
        json!({
            "forecasting_question":"QUESTION_SENTINEL","calibration_result":input["stage6"],
            "record_result":admitted,"synthesis_output":{"evidence":["EVIDENCE_SENTINEL"]}
        }),
    )
    .expect("assembled record feeds real quality renderer");
    assert!(quality.contains("CRITERIA_SENTINEL") && quality.contains("caller-owned"));
    assert!(quality.contains("\"crit\"") && !quality.contains("\"calibration_realism_score\""));
}

fn grill_input() -> serde_json::Value {
    serde_json::json!({
        "question_bank": [
            {"id":11,"question":"QUESTION_SENTINEL","level":2,"area":"math"},
            {"id":12,"question":"When may an answer be graded?","level":2,"area":"evidence"}
        ],
        "answers": [
            {"id":1,"question_id":11,"text":"ANSWER_SENTINEL","evidence":[],"round":1,"rating":null,"failed":false,"note":null},
            {"id":2,"question_id":12,"text":"After it exists","evidence":[],"round":1,"rating":null,"failed":false,"note":null}
        ],
        "new_answers":[],"answer_ids":[],"evaluations":[],"level":2,"round":1,"mode":"interrogate",
        "calibrated_level":null,"respondent":"human","regrades_used":0
    })
}

// Apply one update and carry the returned ledger and regrade count forward,
// as the skill's instructions do.
fn grill_step(input: &mut serde_json::Value) -> Result<serde_json::Value, String> {
    let result = grill_jq("Feedback/update", input.clone())?;
    input["answers"] = result["answers"].clone();
    input["regrades_used"] = result["regrades_used"].clone();
    Ok(result)
}

#[test]
fn grill_feedback_counts_distinct_answers_not_regrades() {
    let mut input = grill_input();
    input["answer_ids"] = serde_json::json!([2, 1]);
    input["evaluations"] = serde_json::json!([
        {"answer_id":2,"rating":"Solid","note":"Correct"},
        {"answer_id":1,"rating":"Gap","note":"Incorrect"}
    ]);
    let first = grill_step(&mut input).expect("first grading");
    assert_eq!(first["regrades_used"], 0, "a first grade is not a regrade");
    assert_eq!(first["direction"], "hold");
    input["answer_ids"] = serde_json::json!([1]);
    for (rating, used) in [("Gap", 1), ("Solid", 2)] {
        input["evaluations"] =
            serde_json::json!([{"answer_id":1,"rating":rating,"note":"Regrade"}]);
        let result = grill_step(&mut input).expect("regrade within the bound");
        assert_eq!(result["regrades_used"], used);
        assert_eq!(result["retired_ids"], serde_json::json!([]));
        assert_eq!(result["answers"].as_array().expect("ledger").len(), 2);
    }
    // The latest grade decides the direction; the earlier Gap stays on record.
    let corrected = grill_jq("Feedback/update", {
        let mut query = input.clone();
        query["answer_ids"] = serde_json::json!([]);
        query["evaluations"] = serde_json::json!([]);
        query
    })
    .expect("query after regrades");
    assert_eq!(corrected["direction"], "escalate");
    assert_eq!(corrected["answers"][0]["rating"], "Solid");
    assert_eq!(corrected["answers"][0]["failed"], true);
    assert_eq!(corrected["regrades_used"], 2, "a query is not a regrade");
    let refused = grill_jq("Feedback/update", input.clone()).expect_err("third regrade");
    assert!(refused.contains("regrade bound exceeded"), "{refused}");
    // Two more distinct submissions to the same question, each graded Gap:
    // the third failed answer retires the question, and neither is a regrade.
    for (id, round) in [(3, 2), (4, 3)] {
        input["round"] = serde_json::json!(round);
        input["answer_ids"] = serde_json::json!([]);
        input["evaluations"] = serde_json::json!([]);
        input["new_answers"] = serde_json::json!([
            {"question_id":11,"text":"same text, distinct submission","evidence":[]}
        ]);
        let appended = grill_step(&mut input).expect("append");
        assert_eq!(appended["pending_ids"], serde_json::json!([id]));
        assert_eq!(appended["direction"], "await_grades");
        assert_eq!(
            appended["next_round"], round,
            "an ungraded batch does not advance"
        );
        input["new_answers"] = serde_json::json!([]);
        input["answer_ids"] = serde_json::json!([id]);
        input["evaluations"] =
            serde_json::json!([{"answer_id":id,"rating":"Gap","note":"New attempt"}]);
        let result = grill_step(&mut input).expect("distinct attempt");
        assert_eq!(result["regrades_used"], 2);
        assert_eq!(
            result["retired_ids"],
            if id == 4 {
                serde_json::json!([11])
            } else {
                serde_json::json!([])
            }
        );
    }
}

#[test]
fn grill_update_appends_answers_and_requires_agent_evidence() {
    let mut input = grill_input();
    input["answers"] = serde_json::json!([
        {"id":7,"question_id":11,"text":"Earlier","evidence":["src/a.rs:3"],"round":1,"rating":"Solid","failed":false,"note":"ok"}
    ]);
    input["round"] = serde_json::json!(2);
    input["respondent"] = serde_json::json!("agent");
    input["new_answers"] = serde_json::json!([
        {"question_id":12,"text":"SECOND","evidence":["src/b.rs:9"]},
        {"question_id":11,"text":"THIRD","evidence":["tool:grep#2","src/c.rs:1"]}
    ]);
    let appended = grill_jq("Feedback/update", input.clone()).expect("append");
    // IDs follow the ledger's maximum, not its length, and rows keep their evidence.
    assert_eq!(
        appended["answers"],
        serde_json::json!([
            {"id":7,"question_id":11,"text":"Earlier","evidence":["src/a.rs:3"],"round":1,"rating":"Solid","failed":false,"note":"ok"},
            {"id":8,"question_id":12,"text":"SECOND","evidence":["src/b.rs:9"],"round":2,"rating":null,"failed":false,"note":null},
            {"id":9,"question_id":11,"text":"THIRD","evidence":["tool:grep#2","src/c.rs:1"],"round":2,"rating":null,"failed":false,"note":null}
        ])
    );
    assert_eq!(appended["pending_ids"], serde_json::json!([8, 9]));

    let mut unsupported = input.clone();
    unsupported["new_answers"][1]["evidence"] = serde_json::json!([]);
    assert!(
        grill_jq("Feedback/update", unsupported.clone()).is_err(),
        "an agent answer without evidence is refused"
    );
    unsupported["respondent"] = serde_json::json!("human");
    assert!(
        grill_jq("Feedback/update", unsupported).is_ok(),
        "a human answer needs no locator"
    );
    for (label, patch) in [
        (
            "blank locator",
            serde_json::json!([{"question_id":12,"text":"x","evidence":[""]}]),
        ),
        (
            "unknown question",
            serde_json::json!([{"question_id":99,"text":"x","evidence":["a:1"]}]),
        ),
        (
            "empty text",
            serde_json::json!([{"question_id":12,"text":"","evidence":["a:1"]}]),
        ),
        (
            "same question twice in one call",
            serde_json::json!([
                {"question_id":12,"text":"x","evidence":["a:1"]},
                {"question_id":12,"text":"y","evidence":["a:2"]}
            ]),
        ),
    ] {
        let mut bad = input.clone();
        bad["new_answers"] = patch;
        assert!(grill_jq("Feedback/update", bad).is_err(), "{label}");
    }
    // A second answer to a question already answered in this round is refused.
    let mut repeat = input.clone();
    repeat["answers"] = appended["answers"].clone();
    repeat["new_answers"] =
        serde_json::json!([{"question_id":12,"text":"again","evidence":["a:1"]}]);
    assert!(
        grill_jq("Feedback/update", repeat).is_err(),
        "second answer in the round"
    );
    // One call appends or grades, never both.
    let mut both = input.clone();
    both["answer_ids"] = serde_json::json!([7]);
    both["evaluations"] = serde_json::json!([{"answer_id":7,"rating":"Gap","note":"n"}]);
    assert!(
        grill_jq("Feedback/update", both).is_err(),
        "append and grade together"
    );
    let mut stranger = input;
    stranger["respondent"] = serde_json::json!("committee");
    assert!(
        grill_jq("Feedback/update", stranger).is_err(),
        "unknown respondent"
    );
}

#[test]
fn grill_feedback_preserves_boundaries_and_rejects_bad_batches() {
    // (solid of five, level, round) -> direction, and the level, round and stop
    // the caller is told to use next.
    for (solid, level, round, direction, next_level, next_round, stop) in [
        (4, 2, 1, "escalate", 3, 2, false),
        (3, 2, 1, "hold", 2, 2, false),
        (2, 2, 1, "hold", 2, 2, false),
        (1, 2, 1, "reprobe", 2, 2, false),
        (4, 5, 1, "complete", 5, 1, true),
        (0, 2, 5, "complete", 2, 5, true),
    ] {
        let mut input = grill_input();
        input["level"] = serde_json::json!(level);
        input["round"] = serde_json::json!(round);
        input["answers"] = serde_json::json!((1..=5).map(|id| serde_json::json!({
            "id":id,"question_id":11,"text":"Fixture","evidence":[],"round":round,"rating":null,"failed":false,"note":null
        })).collect::<Vec<_>>());
        input["answer_ids"] = serde_json::json!([1, 2, 3, 4, 5]);
        input["evaluations"] = serde_json::json!(
            (1..=5)
                .map(|id| serde_json::json!({
                    "answer_id":id,"rating":if id<=solid {"Solid"} else {"Gap"},"note":"Fixture"
                }))
                .collect::<Vec<_>>()
        );
        let result = grill_jq("Feedback/update", input).expect("threshold");
        assert_eq!(result["direction"], direction, "{solid}/5 at {level}");
        assert_eq!(result["next_level"], next_level, "{solid}/5 at {level}");
        assert_eq!(result["next_round"], next_round, "{solid}/5 round {round}");
        assert_eq!(result["next_mode"], "interrogate");
        assert_eq!(result["stop"], stop, "{solid}/5 round {round}");
    }
    // Grading the calibration batch seeds the level and switches the mode once.
    let mut calibration = grill_input();
    calibration["mode"] = serde_json::json!("calibrate");
    calibration["level"] = serde_json::json!(1);
    calibration["calibrated_level"] = serde_json::json!(3);
    calibration["answer_ids"] = serde_json::json!([1, 2]);
    calibration["evaluations"] = serde_json::json!([
        {"answer_id":1,"rating":"Solid","note":"n"},{"answer_id":2,"rating":"Solid","note":"n"}
    ]);
    let seeded = grill_jq("Feedback/update", calibration).expect("calibration");
    assert_eq!(
        (
            &seeded["direction"],
            &seeded["next_level"],
            &seeded["next_round"],
            &seeded["next_mode"]
        ),
        (
            &serde_json::json!("hold"),
            &serde_json::json!(3),
            &serde_json::json!(2),
            &serde_json::json!("interrogate")
        )
    );
    let input = grill_input();
    let pending = grill_jq("Feedback/update", input.clone()).expect("pending");
    assert_eq!(pending["pending_ids"], serde_json::json!([1, 2]));
    assert_eq!(pending["direction"], "await_grades");
    assert_eq!(
        pending["next_round"], 1,
        "an ungraded batch does not advance"
    );
    // The fifth batch is graded before it is assessed: stop waits for the grades.
    let mut last = input.clone();
    last["round"] = serde_json::json!(5);
    last["answers"][0]["round"] = serde_json::json!(5);
    let last = grill_jq("Feedback/update", last).expect("ungraded fifth batch");
    assert_eq!(last["direction"], "await_grades");
    assert_eq!(last["stop"], false);
    for (key, value) in [
        ("level", serde_json::json!(0)),
        ("round", serde_json::json!(6)),
        ("round", serde_json::json!(1.5)),
        ("answer_ids", serde_json::json!([1])),
    ] {
        let mut bad = input.clone();
        bad[key] = value;
        assert!(grill_jq("Feedback/update", bad).is_err(), "bad {key}");
    }
    let mut unknown = input.clone();
    unknown["answer_ids"] = serde_json::json!([99]);
    unknown["evaluations"] = serde_json::json!([{"answer_id":99,"rating":"Gap","note":"Unknown"}]);
    assert!(
        grill_jq("Feedback/update", unknown).is_err(),
        "matching batch IDs must still reference an actual answer"
    );
    let mut empty = input;
    empty["answers"] = serde_json::json!([]);
    empty["round"] = serde_json::json!(5);
    let end = grill_jq("Feedback/update", empty).expect("empty fifth batch");
    assert_eq!(end["direction"], "hold");
    assert_eq!(end["stop"], true);
}

#[test]
fn grill_delivery_preserves_retries_explanations_and_final_evidence() {
    let session = grill_input();
    let mut delivery = serde_json::json!({"question_bank":session["question_bank"],"retired_ids":[11],"level":2,
        "drafts":[{"retry_id":12,"question":null,"area":null},{"retry_id":null,"question":"New question","area":"math"}],
        "explanations":[{"question_id":11,"explanation":"Correct answer"}]});
    let questions = grill_jq("Question delivery", delivery.clone()).expect("legal retry");
    assert_eq!(questions[0], session["question_bank"][1]);
    assert_eq!(questions[1]["id"], 14);
    delivery["drafts"][0]["retry_id"] = serde_json::json!(11);
    assert!(
        grill_jq("Question delivery", delivery.clone()).is_err(),
        "retired retry"
    );
    delivery["drafts"][0]["retry_id"] = serde_json::json!(12);
    delivery["explanations"] = serde_json::json!([]);
    assert!(
        grill_jq("Question delivery", delivery).is_err(),
        "missing retired explanation"
    );
    let mut final_input = serde_json::json!({"question_bank":session["question_bank"],"answers":session["answers"],
        "summary":[{"area":"math","rating":"Unassessed","answer_ids":[]},
                   {"area":"evidence","rating":"Unassessed","answer_ids":[]}]});
    assert_eq!(
        grill_jq("Final evidence", final_input.clone()).expect("unanswered areas"),
        true
    );
    final_input["summary"][0] = serde_json::json!({"area":"math","rating":"Gap","answer_ids":[1]});
    assert!(
        grill_jq("Final evidence", final_input.clone()).is_err(),
        "ungraded cannot support Gap"
    );
    final_input["answers"][0]["rating"] = serde_json::json!("Gap");
    assert_eq!(
        grill_jq("Final evidence", final_input.clone()).expect("actual evidence"),
        true
    );
    final_input["summary"][0]["answer_ids"] = serde_json::json!([99]);
    assert!(
        grill_jq("Final evidence", final_input).is_err(),
        "invented evidence"
    );
}

#[test]
fn grill_round_separates_identified_grading_from_next_questions() {
    let session = grill_input();
    let mut context = serde_json::json!({"topic":"fixture","respondent":"human","phase":"grade","mode":"interrogate",
        "current_level":2,"round_number":1,"question_bank":session["question_bank"],
        "answer_ids":[1,2],"feedback":{"answers":session["answers"],"direction":"await_grades","retired_ids":[],"stop":false}});
    let grade =
        render_template_for_test("grill-me/grill-me-round", context.clone()).expect("grade render");
    assert!(grade.contains("QUESTION_SENTINEL") && grade.contains("ANSWER_SENTINEL"));
    assert!(
        grade.contains("## Grade Identified Answers Only")
            && !grade.contains("## Draft Next Questions Only")
    );
    context["phase"] = serde_json::json!("ask");
    context["answer_ids"] = serde_json::json!([]);
    for (direction, stop, allowed) in [
        ("escalate", false, true),
        ("await_grades", false, false),
        ("complete", true, false),
    ] {
        context["feedback"] =
            serde_json::json!({"answers":[],"direction":direction,"retired_ids":[11],"stop":stop});
        let output = render_template_for_test("grill-me/grill-me-round", context.clone())
            .expect("ask render");
        assert_eq!(output.contains("## Draft Next Questions Only"), allowed);
        if allowed {
            assert!(output.contains("retired_explanations") && !output.contains("\"assessment\":"));
        }
    }
}

#[test]
fn grill_round_tells_an_examiner_of_an_agent_to_grade_the_evidence() {
    let session = grill_input();
    let mut answers = session["answers"].clone();
    answers[0]["evidence"] = serde_json::json!(["LOCATOR_SENTINEL:12"]);
    let mut context = serde_json::json!({"topic":"fixture","respondent":"agent","phase":"grade","mode":"interrogate",
        "current_level":2,"round_number":1,"question_bank":session["question_bank"],
        "answer_ids":[1],"feedback":{"answers":answers,"direction":"await_grades","retired_ids":[],"stop":false}});
    let agent_grade = render_template_for_test("grill-me/grill-me-round", context.clone())
        .expect("agent grade render");
    // The selected answer reaches the grader with its locator; the unselected one does not.
    assert!(
        agent_grade.contains("LOCATOR_SENTINEL:12") && !agent_grade.contains("After it exists")
    );
    assert!(agent_grade.contains("You are the\nexaminer"));
    assert!(agent_grade.contains("does not show what it claims is Gap"));
    context["phase"] = serde_json::json!("ask");
    context["answer_ids"] = serde_json::json!([]);
    context["feedback"]["direction"] = serde_json::json!("hold");
    let agent_ask = render_template_for_test("grill-me/grill-me-round", context.clone())
        .expect("agent ask render");
    assert!(agent_ask.contains("answerable with\na locator"));
    context["respondent"] = serde_json::json!("human");
    for phase in ["ask", "grade"] {
        context["phase"] = serde_json::json!(phase);
        let human = render_template_for_test("grill-me/grill-me-round", context.clone())
            .expect("human render");
        assert!(
            !human.contains("examiner") && !human.contains("locator"),
            "a human respondent gets no agent instructions in {phase}"
        );
    }
    context["respondent"] = serde_json::json!("committee");
    let unknown = render_template_for_test("grill-me/grill-me-round", context)
        .expect("unknown respondent render");
    assert!(
        unknown.contains("invalid_phase_mode_or_respondent")
            && !unknown.contains("## Draft Next Questions Only")
            && !unknown.contains("## Grade Identified Answers Only")
    );
}

#[test]
fn grill_assessment_skeleton_groups_graded_answers_by_area() {
    let mut session = grill_input();
    session["question_bank"]
        .as_array_mut()
        .expect("bank")
        .push(serde_json::json!({"id":13,"question":"Third","level":3,"area":"math"}));
    session["answers"] = serde_json::json!([
        {"id":1,"question_id":11,"text":"a","evidence":[],"round":1,"rating":"Gap","failed":true,"note":"n"},
        {"id":2,"question_id":12,"text":"b","evidence":[],"round":1,"rating":null,"failed":false,"note":null},
        {"id":3,"question_id":13,"text":"c","evidence":[],"round":2,"rating":"Solid","failed":false,"note":"n"}
    ]);
    let skeleton = grill_jq(
        "Assessment skeleton",
        serde_json::json!({"question_bank":session["question_bank"],"answers":session["answers"]}),
    )
    .expect("skeleton");
    // An answered-but-ungraded area is not assessable; areas come out sorted.
    assert_eq!(
        skeleton,
        serde_json::json!([
            {"area":"evidence","question_ids":[12],"graded_answer_ids":[],"latest_ratings":[],"assessable":false},
            {"area":"math","question_ids":[11,13],"graded_answer_ids":[1,3],"latest_ratings":["Gap","Solid"],"assessable":true}
        ])
    );
    // A summary that follows the skeleton passes the delivery check; one that
    // rates the unassessable area does not.
    let mut final_input = serde_json::json!({"question_bank":session["question_bank"],"answers":session["answers"],
        "summary":[{"area":"evidence","rating":"Unassessed","answer_ids":[]},
                   {"area":"math","rating":"Partial","answer_ids":[1,3]}]});
    assert_eq!(
        grill_jq("Final evidence", final_input.clone()).expect("skeleton-shaped summary"),
        true
    );
    final_input["summary"][0] =
        serde_json::json!({"area":"evidence","rating":"Gap","answer_ids":[2]});
    assert!(grill_jq("Final evidence", final_input).is_err());
}

#[test]
fn grill_assessment_preserves_answer_evidence_and_absent_calibration() {
    let session = grill_input();
    let areas = serde_json::json!([
        {"area":"AREA_SENTINEL","question_ids":[11],"graded_answer_ids":[],"latest_ratings":[],"assessable":false}
    ]);
    let mut context = serde_json::json!({
        "topic":"fixture","respondent":"human","areas":areas,"question_bank":session["question_bank"],
        "answers":session["answers"],"calibrated_level":null
    });
    let output = render_template_for_test("grill-me/grill-me-assess", context.clone())
        .expect("final render");
    for text in [
        "QUESTION_SENTINEL",
        "ANSWER_SENTINEL",
        "AREA_SENTINEL",
        "Unassessed",
        "answer_ids",
    ] {
        assert!(output.contains(text), "{text}");
    }
    assert!(!output.contains("solid|partial|gap"));
    assert!(!output.contains("not study advice"));
    context["respondent"] = serde_json::json!("agent");
    let agent = render_template_for_test("grill-me/grill-me-assess", context.clone())
        .expect("agent final render");
    assert!(agent.contains("not study advice"));
    context.as_object_mut().expect("context").remove("areas");
    assert!(
        render_template_for_test("grill-me/grill-me-assess", context)
            .expect_err("the skeleton is required")
            .contains("areas")
    );
}

// One cargo `--message-format=json` record, as clippy prints it.
fn cargo_message(level: &str, code: Option<&str>, message: &str, line: u32) -> String {
    serde_json::json!({
        "reason": "compiler-message",
        "message": {
            "level": level, "message": message,
            "code": code.map(|code| serde_json::json!({"code": code})),
            "spans": [{"file_name": "src/lib.rs", "line_start": line, "is_primary": true}]
        }
    })
    .to_string()
}

fn cargo_capture(messages: &[String], success: bool) -> String {
    let mut lines = vec!["sccache enabled: /not/json".to_string()];
    lines.extend(messages.iter().cloned());
    lines.push(serde_json::json!({"reason": "build-finished", "success": success}).to_string());
    lines.join("\n") + "\n"
}

fn compiler_facts(
    baseline: &str,
    proposed: &str,
    unsafe_grep: &str,
    miri: &str,
    record: &str,
) -> Result<serde_json::Value, String> {
    authored_jq(
        "idiomatic-rust",
        "Compiler facts",
        serde_json::json!({"baseline": baseline, "proposed": proposed,
            "unsafe_grep": unsafe_grep, "miri": miri, "record": record}),
    )
}

#[test]
fn idiomatic_rust_compiler_facts_are_measured_from_the_captures() {
    let ptr_arg = cargo_message("error", Some("clippy::ptr_arg"), "writing `&Vec`", 1);
    let ptr_arg_moved = cargo_message("error", Some("clippy::ptr_arg"), "writing `&Vec`", 9);
    let unused = cargo_message("warning", Some("unused_variables"), "unused variable", 2);
    let mismatch = cargo_message("error", Some("E0308"), "mismatched types", 4);
    let syntax = cargo_message("error", None, "mismatched closing delimiter", 3);
    let note = serde_json::json!({"reason": "compiler-message",
        "message": {"level": "failure-note", "message": "For more information", "code": null, "spans": []}})
    .to_string();
    let clean = cargo_capture(&[], true);
    let lints = cargo_capture(&[ptr_arg.clone(), ptr_arg, unused.clone()], false);

    // Before the proposal is applied: the baseline's diagnostics stand, and
    // nothing is reported as introduced or resolved.
    let cold = compiler_facts(&lints, "", "", "", "").expect("baseline only");
    assert_eq!(cold["baseline_status"], "ran");
    assert_eq!(cold["proposed_status"], "not_run");
    assert_eq!(
        cold["open_codes"],
        serde_json::json!(["clippy::ptr_arg", "unused_variables"])
    );
    // The duplicate record cargo prints per target is one diagnostic.
    assert_eq!(
        cold["diagnostics"].as_array().expect("diagnostics").len(),
        2
    );
    assert_eq!(cold["new_lints"], serde_json::json!([]));
    assert_eq!(cold["resolved_lints"], serde_json::json!([]));
    assert_eq!(
        cold["gate"],
        serde_json::json!({"baseline_status":"ran","proposed_status":"not_run",
            "open_codes":["clippy::ptr_arg","unused_variables"],"miri":"not_run",
            "compile_errors":0,"new_lints":0,"unsafe_lines":0,"history":[],"iteration":1})
    );

    // A lint in both captures is not new, even when its line moved.
    let moved = cargo_capture(&[ptr_arg_moved, unused], false);
    let same = compiler_facts(&lints, &moved, "", "", "").expect("same lints");
    assert_eq!(same["gate"]["new_lints"], 0);
    assert_eq!(same["resolved_lints"], serde_json::json!([]));

    // Lints are told from compile errors by code, not level: with warnings
    // denied a lint arrives at level error.
    let introduced = compiler_facts(&clean, &lints, "", "", "").expect("introduced");
    assert_eq!(introduced["gate"]["compile_errors"], 0);
    assert_eq!(introduced["gate"]["new_lints"], 2);
    let resolved = compiler_facts(&lints, &clean, "", "", "").expect("resolved");
    assert_eq!(resolved["gate"]["new_lints"], 0);
    assert_eq!(
        resolved["resolved_lints"]
            .as_array()
            .expect("resolved")
            .len(),
        2
    );
    assert_eq!(resolved["open_codes"], serde_json::json!([]));

    let broken = cargo_capture(&[mismatch, syntax, note], false);
    let errors = compiler_facts(&clean, &broken, "", "", "").expect("compile errors");
    assert_eq!(errors["gate"]["compile_errors"], 2);
    assert_eq!(errors["gate"]["new_lints"], 0);
    assert_eq!(
        errors["open_codes"],
        serde_json::json!(["E0308", "rustc-error"])
    );

    // A build that fails and reports nothing is not a clean proposal.
    let silent =
        compiler_facts(&clean, &cargo_capture(&[], false), "", "", "").expect("silent failure");
    assert_eq!(silent["proposed_status"], "unexplained_failure");
    let absent = compiler_facts("", "", "", "", "").expect("nothing ran");
    assert_eq!(absent["baseline_status"], "not_run");

    // unsafe lines drop comments; Miri's status is read from its output.
    let grep = "src/a.rs:4:    // SAFETY: unsafe is fine\nsrc/a.rs:5:    unsafe { *p }\n9:unsafe fn f() {}\n";
    for (miri, status) in [
        ("", "not_run"),
        ("test result: ok.\nmiri-exit=0\n", "passed"),
        (
            "error: Undefined Behavior: dereference\nmiri-exit=nonzero\n",
            "ub",
        ),
        (
            "error: 'cargo-miri' is not installed\nmiri-exit=nonzero\n",
            "failed",
        ),
    ] {
        let facts = compiler_facts(&clean, &clean, grep, miri, "").expect("miri");
        assert_eq!(facts["gate"]["miri"], status, "{miri}");
        assert_eq!(facts["gate"]["unsafe_lines"], 2);
    }

    // The iteration and the confirmed history come from the record, in order.
    let record = "{\"iteration\":1,\"confirmed\":2,\"outcome\":\"continue\"}\n{\"iteration\":2,\"confirmed\":4,\"outcome\":\"continue\"}\n";
    let third = compiler_facts(&clean, &clean, "", "", record).expect("third pass");
    assert_eq!(third["gate"]["history"], serde_json::json!([2, 4]));
    assert_eq!(third["gate"]["iteration"], 3);
    for bad in [
        "{\"iteration\":2,\"confirmed\":1}\n",
        "{\"iteration\":1,\"outcome\":\"continue\"}\n",
    ] {
        assert!(
            compiler_facts(&clean, &clean, "", "", bad).is_err(),
            "record refused: {bad}"
        );
    }
}

#[test]
fn idiomatic_rust_templates_tell_not_run_from_clean_from_reported() {
    let lint = cargo_message("error", Some("clippy::ptr_arg"), "LINT_SENTINEL", 1);
    let error = cargo_message("error", Some("E0308"), "ERROR_SENTINEL", 4);
    let clean = cargo_capture(&[], true);
    let lints = cargo_capture(&[lint], false);
    let broken = cargo_capture(&[error], false);
    let inquiry = |facts: serde_json::Value, focus: &str| {
        render_template_for_test(
            "idiomatic-rust/idiomatic-rust-inquiry",
            serde_json::json!({"design_problem":"PROBLEM_SENTINEL","existing_code":"fn f() {}",
                "design_constraints":{},"compiler_facts":facts,"iteration_focus":focus}),
        )
        .expect("inquiry render")
    };
    let not_run = inquiry(compiler_facts("", "", "", "", "").expect("facts"), "");
    assert!(not_run.contains("did not run on this code (`not_run`)"));
    assert!(!not_run.contains("reported nothing") && !not_run.contains("ground\ntruth"));
    let clean_inquiry = inquiry(compiler_facts(&clean, "", "", "", "").expect("facts"), "");
    assert!(clean_inquiry.contains("ran on this code and reported nothing"));
    assert!(!clean_inquiry.contains("did not run"));
    let reported = inquiry(
        compiler_facts(&lints, "", "", "", "").expect("facts"),
        "FOCUS_SENTINEL",
    );
    assert!(reported.contains("LINT_SENTINEL") && reported.contains("[\"clippy::ptr_arg\"]"));
    assert!(reported.contains("Principle 2 (ownership is architecture)"));
    assert!(reported.contains("FOCUS_SENTINEL") && reported.contains("PROBLEM_SENTINEL"));
    assert!(!reported.contains("did not run") && !reported.contains("reported nothing"));
    // The eight principles reach the prompt; no typed diagnostic count is asked for.
    assert_eq!(reported.matches("\n8. **Unsafe as contract.**").count(), 1);
    assert!(
        !reported.contains("diagnostics_interpreted") && !reported.contains("compiler_confirmed")
    );

    let design = |actions: serde_json::Value| {
        render_template_for_test(
            "idiomatic-rust/idiomatic-rust-design",
            serde_json::json!({"design_problem":"PROBLEM_SENTINEL","existing_code":"",
                "assessment":{"invariants":["i"]},"code_actions":actions,"iteration_focus":""}),
        )
        .expect("design render")
    };
    let no_actions = design(serde_json::json!([]));
    assert!(no_actions.contains("`get_code_actions` was not run"));
    let actions = design(serde_json::json!([
        "Found 1 code action(s):\n1. ACTION_SENTINEL"
    ]));
    assert!(actions.contains("ACTION_SENTINEL") && !actions.contains("was not run"));
    assert!(actions.contains("\"status\": \"fixed\"") && actions.contains("`open`"));

    let challenge = |facts: serde_json::Value, references: serde_json::Value| {
        render_template_for_test(
            "idiomatic-rust/idiomatic-rust-challenge",
            serde_json::json!({"design_problem":"PROBLEM_SENTINEL","design_proposal":{"proposed_design":"x"},
                "assessment":{"invariants":["i"]},"design_constraints":{},
                "compiler_facts":facts,"reference_results":references}),
        )
        .expect("challenge render")
    };
    let unapplied = challenge(
        compiler_facts(&clean, "", "", "", "").expect("facts"),
        serde_json::json!([]),
    );
    assert!(unapplied.contains("The proposal has not been compiled"));
    assert!(unapplied.contains("`find_references` was not run"));
    assert!(!unapplied.contains("was applied and compiled") && !unapplied.contains("## `unsafe`"));
    let silent = challenge(
        compiler_facts(&clean, &cargo_capture(&[], false), "", "", "").expect("facts"),
        serde_json::json!([]),
    );
    assert!(silent.contains("failed without reporting a diagnostic"));
    let measured = challenge(
        compiler_facts(
            &lints,
            &broken,
            "src/a.rs:5:    unsafe { UNSAFE_SENTINEL }\n",
            "error: Undefined Behavior\nmiri-exit=nonzero\n",
            "",
        )
        .expect("facts"),
        serde_json::json!(["Found 2 references to `Queue`:\n\n## REFERENCE_SENTINEL"]),
    );
    assert!(measured.contains("was applied and compiled"));
    // Each measured list reaches the prompt under its own heading: the text
    // between one heading and the next holds that list and no other.
    let section = |text: &str, from: &str, to: &str| -> String {
        let (_, tail) = text
            .split_once(from)
            .unwrap_or_else(|| panic!("no {from:?}"));
        let (body, _) = tail.split_once(to).unwrap_or_else(|| panic!("no {to:?}"));
        body.to_string()
    };
    let errors = section(&measured, "Compile errors", "Lints the proposal introduced");
    assert!(errors.contains("ERROR_SENTINEL") && !errors.contains("LINT_SENTINEL"));
    let introduced = section(
        &measured,
        "Lints the proposal introduced",
        "Baseline lints the proposal resolved",
    );
    assert!(
        !introduced.contains("_SENTINEL"),
        "this proposal introduced no lint"
    );
    let resolved = section(
        &measured,
        "Baseline lints the proposal resolved",
        "Everything still reported",
    );
    assert!(resolved.contains("LINT_SENTINEL") && !resolved.contains("ERROR_SENTINEL"));
    let still = section(
        &measured,
        "Everything still reported",
        "Clippy is a codified",
    );
    assert!(still.contains("ERROR_SENTINEL") && !still.contains("LINT_SENTINEL"));
    // A lint the proposal introduces is listed as introduced, not as resolved.
    let regressed = challenge(
        compiler_facts(&clean, &lints, "", "", "").expect("facts"),
        serde_json::json!(["Found 1 references to `Queue`"]),
    );
    let introduced = section(
        &regressed,
        "Lints the proposal introduced",
        "Baseline lints the proposal resolved",
    );
    assert!(introduced.contains("LINT_SENTINEL"));
    let resolved = section(
        &regressed,
        "Baseline lints the proposal resolved",
        "Everything still reported",
    );
    assert!(!resolved.contains("LINT_SENTINEL"));
    assert!(measured.contains("REFERENCE_SENTINEL") && measured.contains("UNSAFE_SENTINEL"));
    assert!(measured.contains("Miri on the proposal: `ub`"));
    assert!(!measured.contains("has not been compiled") && !measured.contains("was not run"));
    for typed in [
        "find_references_count",
        "proposed_code_compiles",
        "miri_checked",
        "unsafe_blocks",
    ] {
        assert!(
            !measured.contains(typed),
            "the model is not asked to type {typed}"
        );
    }
}

fn change_scope(
    before: &str,
    after: &str,
    audits: &str,
    plan: serde_json::Value,
) -> Result<serde_json::Value, String> {
    authored_jq(
        "coding-guidelines",
        "Change scope",
        serde_json::json!({"before": before, "after": after, "audits": audits,
            "assess": [{"constrained_plan": plan}]}),
    )
}

#[test]
fn coding_guidelines_change_scope_is_measured_against_the_plan() {
    let plan = serde_json::json!([
        {"step":"a","files_to_touch":["src/a.rs","src/b.rs"],"files_not_to_touch":["src/c.rs"]},
        {"step":"b","files_to_touch":["src/b.rs"],"files_not_to_touch":["src/d.rs"]}
    ]);
    // Another session's uncommitted file is in both captures, unchanged.
    let before = "7\t2\tkask/other/session.rs\n";
    let inside = "7\t2\tkask/other/session.rs\n3\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n";
    let exact = change_scope(before, inside, "", plan.clone()).expect("change inside the plan");
    assert_eq!(
        exact,
        serde_json::json!({"changed":["src/a.rs","src/b.rs"],"unplanned":[],
            "untouched_planned":[],"forbidden_touched":[],"prior_audits":[]})
    );

    // A stray file, a forbidden file, an untracked file, and a planned file
    // left alone are each reported in their own list.
    let stray = "7\t2\tkask/other/session.rs\n3\t1\tsrc/a.rs\n2\t2\tsrc/c.rs\nnew\tnew\tsrc/z.rs\n";
    let audits = "{\"audit\":1,\"verdict\":\"correct\",\"overall\":0.65}\n";
    let strayed = change_scope(before, stray, audits, plan.clone()).expect("stray change");
    assert_eq!(
        strayed,
        serde_json::json!({"changed":["src/a.rs","src/c.rs","src/z.rs"],
            "unplanned":["src/c.rs","src/z.rs"],"untouched_planned":["src/b.rs"],
            "forbidden_touched":["src/c.rs"],"prior_audits":["correct"]})
    );

    // A file whose line moved between the captures counts as changed.
    let grown = "9\t2\tkask/other/session.rs\n3\t1\tsrc/a.rs\n10\t0\tsrc/b.rs\n";
    assert_eq!(
        change_scope(before, grown, "", plan.clone()).expect("other file grew")["unplanned"],
        serde_json::json!(["kask/other/session.rs"])
    );
    let nothing = change_scope(before, before, "", plan.clone()).expect("no change");
    assert_eq!(nothing["changed"], serde_json::json!([]));
    assert_eq!(
        nothing["untouched_planned"],
        serde_json::json!(["src/a.rs", "src/b.rs"])
    );

    // A committed change is audited with no baseline.
    let committed = change_scope(
        "",
        "3\t1\tsrc/a.rs\n-\t-\tassets/logo.png\n",
        "",
        plan.clone(),
    )
    .expect("committed change");
    assert_eq!(
        committed["changed"],
        serde_json::json!(["src/a.rs", "assets/logo.png"])
    );
    assert_eq!(
        committed["unplanned"],
        serde_json::json!(["assets/logo.png"])
    );

    assert!(
        change_scope("", "not numstat output\n", "", plan.clone()).is_err(),
        "a line that is not numstat output is refused"
    );
    for bad in [
        "{\"audit\":2,\"verdict\":\"correct\"}\n",
        "{\"audit\":1}\n",
        "{\"audit\":1,\"verdict\":\"correct\"}\n{\"audit\":1,\"verdict\":\"pass\"}\n",
    ] {
        assert!(
            change_scope(before, inside, bad, plan.clone()).is_err(),
            "audit record refused: {bad}"
        );
    }
}

#[test]
fn coding_guidelines_templates_carry_the_plan_fields_and_the_measured_scope() {
    let assess = render_template_for_test(
        "coding-guidelines/guidelines-assess",
        serde_json::json!({"task_description":"TASK_SENTINEL","code_context":{"note":"CONTEXT_SENTINEL"}}),
    )
    .expect("assess render");
    assert!(assess.contains("TASK_SENTINEL") && assess.contains("CONTEXT_SENTINEL"));
    // The field is asked for (with the rule that it is an exact quote) and shown.
    assert!(assess.contains("- `traces_to`: The words of the task above"));
    assert!(assess.contains("copied exactly") && assess.contains("\"traces_to\":"));
    assert!(assess.contains("7. **Style changes outside task scope**"));
    assert!(!assess.contains("assumption_log"));

    let verify = |scope: serde_json::Value, diff: &str| {
        render_template_for_test(
            "coding-guidelines/guidelines-verify",
            serde_json::json!({"task_description":"TASK_SENTINEL",
                "assumptions":[{"assumption":"ASSUMPTION_SENTINEL"}],
                "success_criteria":[{"goal":"CRITERION_SENTINEL"}],
                "constrained_plan":[{"step":"PLAN_SENTINEL"}],
                "diff_preview":diff,"scope":scope}),
        )
        .expect("verify render")
    };
    let audited = verify(
        serde_json::json!({"changed":["CHANGED_SENTINEL","UNPLANNED_SENTINEL","FORBIDDEN_SENTINEL"],
            "unplanned":["UNPLANNED_SENTINEL"],"untouched_planned":["UNTOUCHED_SENTINEL"],
            "forbidden_touched":["FORBIDDEN_SENTINEL"],"prior_audits":[]}),
        "DIFF_SENTINEL",
    );
    for text in [
        "TASK_SENTINEL",
        "ASSUMPTION_SENTINEL",
        "CRITERION_SENTINEL",
        "PLAN_SENTINEL",
        "DIFF_SENTINEL",
        "CHANGED_SENTINEL",
    ] {
        assert!(audited.contains(text), "{text}");
    }
    // Each measured list sits on its own labelled line.
    for (label, sentinel) in [
        (
            "- Changed but in no step's `files_to_touch`: ",
            "[\"UNPLANNED_SENTINEL\"]",
        ),
        (
            "- Changed although a step listed it under `files_not_to_touch`: ",
            "[\"FORBIDDEN_SENTINEL\"]",
        ),
        ("- Planned but not changed: ", "[\"UNTOUCHED_SENTINEL\"]"),
    ] {
        assert!(audited.contains(&format!("{label}{sentinel}")), "{label}");
    }
    // The audit checks the same seven anti-patterns the plan forbade.
    assert!(audited.contains("6. **Unrequested logging/telemetry**"));
    assert!(audited.contains("7. **Style changes outside task scope**"));
    assert!(!audited.contains("No file was changed"));
    // The model is asked for no overall score, count or recommendations list.
    assert!(!audited.contains("\"recommendations\"") && !audited.contains("overall_score"));

    let unchanged = verify(
        serde_json::json!({"changed":[],"unplanned":[],"untouched_planned":["UNTOUCHED_SENTINEL"],
            "forbidden_touched":[],"prior_audits":[]}),
        "",
    );
    assert!(
        unchanged.contains("No file was changed")
            && unchanged.contains("No diff text was provided")
    );
    assert!(!unchanged.contains("- Changed:"));
    let mut missing = serde_json::json!({"task_description":"t","assumptions":[],"success_criteria":[],
        "constrained_plan":[],"diff_preview":""});
    assert!(
        render_template_for_test("coding-guidelines/guidelines-verify", missing.take())
            .expect_err("the measured scope is required")
            .contains("scope")
    );
}

/// The essentialist template is the interrogator's once-per-run reference:
/// the three gates, the finding vocabulary the `ESS-1` gate consumes, and
/// the reduction actions. The loop (mode, order, retries, rounds, zero-delta,
/// score) lives in the SKILL.md form and must not be restated here as
/// pseudo-code, nor may a `mode: autonomous` claim be printed as verified.
#[test]
fn essentialist_flow_is_a_gate_reference_not_a_loop() {
    let context = serde_json::json!({
        "artifact": {"name": "Wrapper", "source": "pub fn get() {}"},
        "code_context": {"callers": {"a.rs": "get()"}, "dependencies": {}, "structure": ["mod wrapper"]},
        "mode": "autonomous",
        "scope": "auto"
    });
    let rendered = render_template_for_test("essentialist/essentialist-flow", context.clone())
        .expect("essentialist flow renders with the documented context");
    for gate in [
        "Gate 1 — EXIST: Deletion Test",
        "Gate 2 — SURFACE: Interface Count",
        "Gate 3 — CONTRACT: Abstraction Trace",
    ] {
        assert!(rendered.contains(gate), "missing {gate}");
    }
    // The finding vocabulary ESS-1 validates against, verbatim.
    assert!(
        rendered.contains(
            "\"constraint_force\": \"prohibition|guardrail|guideline|evidence|hypothesis\""
        )
    );
    assert!(rendered.contains("\"action\": \"DELETE|MERGE|JUSTIFY|RETAIN\""));
    assert!(rendered.contains("malformed_findings"));
    // G3 is this template's own trace; the former delegation is gone.
    assert!(rendered.contains("Every generic parameter"));
    assert!(!rendered.contains("guidelines-verify"));
    // No loop restatement and no authorization claim from a parameter.
    for absent in [
        "FOR round",
        "WHILE retries",
        "max_retries_per_gate",
        "authorization was verified",
    ] {
        assert!(
            !rendered.contains(absent),
            "template restates the loop: {absent}"
        );
    }
    assert!(rendered.contains("Claimed mode**: autonomous"));
    assert!(rendered.contains("resolves to advisory"));

    let mut missing = context;
    missing
        .as_object_mut()
        .expect("context object")
        .remove("code_context");
    assert!(
        render_template_for_test("essentialist/essentialist-flow", missing)
            .expect_err("the code context is required")
            .contains("code_context")
    );
}

/// The coaching ASSESS renders emit `coach_assessment` in exactly the
/// vocabulary the `KI-2` gate compares against; the gate reads those
/// renders, so the words must stay in the templates.
#[test]
fn kata_improvement_coaching_renders_carry_the_assessment_vocabulary() {
    let context = serde_json::json!({
        "context": {}, "learner_bot": "agent-a", "previous_steps": [],
        "learner_answer": "Reach 95% pass rate within 3 experiments, measured by the test runner."
    });
    for (stem, pass, fail) in [
        ("coaching-q1-target", "clear", "vague"),
        ("coaching-q2-actual", "data-driven", "assumption-based"),
        ("coaching-q3-obstacles", "focused", "scattered"),
        ("coaching-q4-experiment", "testable", "untestable"),
        ("coaching-q5-learn", "tight-loop", "open-loop"),
    ] {
        let rendered =
            render_template_for_test(&format!("kata-improvement/{stem}"), context.clone())
                .unwrap_or_else(|error| panic!("{stem} renders with a learner answer: {error}"));
        assert!(
            rendered.contains("coach_assessment"),
            "{stem} names the field"
        );
        assert!(
            rendered.contains(&format!("\"{pass}\"")),
            "{stem} carries {pass}"
        );
        assert!(
            rendered.contains(&format!("\"{fail}\"")),
            "{stem} carries {fail}"
        );
        assert!(
            rendered.contains("measured by the test runner"),
            "{stem} shows the answer"
        );
    }
    let mut unanswered = context;
    unanswered
        .as_object_mut()
        .expect("context object")
        .remove("learner_answer");
    assert!(
        render_template_for_test("kata-improvement/coaching-q1-target", unanswered)
            .expect_err("ASSESS needs the learner's answer")
            .contains("learner_answer")
    );
}

/// The kanban sweep's stale list is the one date comparison the lisp
/// gate cannot make; the authored jq filter computes it from the re-listed
/// rows and the recorded sweep start.
#[test]
fn kanban_task_management_stale_filter_reads_in_progress_activity_against_the_sweep_start() {
    let rows = serde_json::json!({"tasks": [
        {"task_id": "stale", "status": "in_progress", "activity": {"at": "2026-11-12T10:00:00Z"}},
        {"task_id": "fresh", "status": "in_progress", "activity": {"at": "2026-11-13T09:00:00Z"}},
        {"task_id": "silent", "status": "in_progress", "activity": null},
        {"task_id": "review-old", "status": "review", "activity": {"at": "2026-11-01T00:00:00Z"}}
    ]});
    // The filter reads `$since`; bind it the way the skill's command does.
    let stale = authored_jq_with_args(
        "kanban-task-management",
        "Operate-phase sweep loop",
        &["--arg", "since", "2026-11-13T00:00:00Z"],
        rows,
    )
    .expect("authored stale.jq runs");
    assert_eq!(stale, serde_json::json!(["stale"]));
}

/// The scenario convergence gate's past-deadline list is a date
/// comparison the lisp gate cannot make; the authored jq filter computes
/// it from the declared events and today's date.
#[test]
fn scenario_planning_due_filter_lists_events_whose_deadline_has_passed() {
    let events = serde_json::json!({"events": [
        {"id": "past", "deadline": "2026-10-01"},
        {"id": "today", "deadline": "2026-11-13"},
        {"id": "future", "deadline": "2027-01-01"}
    ]});
    let due = authored_jq_with_args(
        "scenario-planning",
        "Convergence",
        &["--arg", "today", "2026-11-13"],
        events,
    )
    .expect("authored due.jq runs");
    // A deadline equal to today has not passed.
    assert_eq!(due, serde_json::json!(["past"]));
}

/// skill-maintenance's SM-1 measures a goal the lisp cannot slice out of
/// the template text; the authored jq filter extracts it, and it must
/// work on a real registry template's header, collapsing its line wraps.
#[test]
fn skill_maintenance_goal_filter_extracts_the_goal_from_a_real_template_header() {
    let template =
        std::fs::read_to_string(registry_template_base().join("essentialist/essentialist-flow.j2"))
            .expect("read the essentialist template");
    let goal = authored_jq_with_args(
        "skill-maintenance",
        "skill-maintenance template-logic audit (formerly skill-logic-audit)",
        &[],
        serde_json::Value::String(template),
    )
    .expect("authored goal.jq runs");
    let goal = goal.as_str().expect("the goal is one string");
    assert!(
        goal.starts_with("Give the eliminative interrogator its reference"),
        "got: {goal}"
    );
    assert!(!goal.contains('\n'), "line wraps are collapsed");
    assert!(!goal.contains("#}"));
}

#[test]
fn corpus_audit_repairs_are_pinned_at_template_handoffs() {
    let complete_intake = render_template_for_test(
        "product-manager/intake-brief",
        serde_json::json!({
            "requirement": "Prevent broken gates",
            "spec_provenance": "operator provided 2026-10-17",
            "acceptance_criteria": ["Malformed gates fail", "Valid gates pass"],
            "constraints": ["Do not touch concurrent streams"],
            "standing_rules": ["Fix the tool"]
        }),
    )
    .expect("complete intake renders");
    assert!(complete_intake.contains("\"missing_fields\": []"));

    let sparse_intake = render_template_for_test(
        "product-manager/intake-brief",
        serde_json::json!({
            "requirement": "",
            "spec_provenance": "",
            "acceptance_criteria": [],
            "constraints": [],
            "standing_rules": []
        }),
    )
    .expect("sparse intake renders");
    for field in [
        "requirement",
        "spec_provenance",
        "acceptance_criteria",
        "constraints",
        "standing_rules",
    ] {
        assert!(
            sparse_intake.contains(&format!("\"field\":\"{field}\"")),
            "missing question for {field}: {sparse_intake}"
        );
    }

    let excessive_criteria = render_template_for_test(
        "product-manager/intake-brief",
        serde_json::json!({
            "requirement": "Prevent broken gates",
            "spec_provenance": "operator provided 2026-10-17",
            "acceptance_criteria": ["a", "b", "c", "d", "e"],
            "constraints": ["bounded"],
            "standing_rules": ["fix the tool"]
        }),
    )
    .expect("excessive criteria intake renders");
    assert!(excessive_criteria.contains("\"field\":\"acceptance_criteria\""));

    let approved_only = render_template_for_test(
        "therapy/report",
        serde_json::json!({
            "target": "fixture",
            "findings": [
                {"id":"F001","issue_type":"contradiction"},
                {"id":"F002","issue_type":"reification_candidate"}
            ],
            "proposals": [
                {"approved":true,"finding_id":"F001","strategy":"forget"},
                {"approved":true,"finding_id":"F002","strategy":"create_skill"}
            ],
            "executions": [],
            "values_pass": {}
        }),
    )
    .expect("approved-only therapy report renders");
    assert!(approved_only.contains("Contradictions resolved: 0"));
    assert!(approved_only.contains("Skills created: 0"));

    let executed = render_template_for_test(
        "therapy/report",
        serde_json::json!({
            "target": "fixture",
            "findings": [
                {"id":"F001","issue_type":"contradiction"},
                {"id":"F002","issue_type":"reification_candidate"}
            ],
            "proposals": [
                {"approved":true,"finding_id":"F001","strategy":"forget"},
                {"approved":true,"finding_id":"F002","strategy":"create_skill"}
            ],
            "executions": [
                {"finding_id":"F001","strategy":"forget","success":true,"outcome":"deleted"},
                {"finding_id":"F001","strategy":"forget","success":true,"outcome":"duplicate receipt"},
                {"finding_id":"F002","strategy":"create_skill","success":true,"outcome":"created"}
            ],
            "values_pass": {}
        }),
    )
    .expect("executed therapy report renders");
    assert!(executed.contains("Contradictions resolved: 1"));
    assert!(!executed.contains("Contradictions resolved: 2"));
    assert!(executed.contains("Skills created: 1"));
}

#[test]
fn optional_cross_skill_inputs_are_actually_optional() {
    for (template_ref, context) in [
        (
            "falsifiability/falsifiability-admit",
            serde_json::json!({"target": "A treatment reduces p95 latency"}),
        ),
        (
            "task-breakdown/task-breakdown-decompose",
            serde_json::json!({
                "context_summary": "Implement a parser guard",
                "dependency_graph": [],
                "risks": [],
                "refinement_directives": [],
                "parallelization_hint": "sequential"
            }),
        ),
        (
            "skill-discovery/skill-discovery-route",
            serde_json::json!({
                "task_description": "debug a performance regression",
                "task_context": "Rust workspace",
                "skill_catalog": []
            }),
        ),
    ] {
        render_template_for_test(template_ref, context).unwrap_or_else(|error| {
            panic!("{template_ref} should accept omitted optional inputs: {error}")
        });
    }
}

#[test]
fn test_strip_frontmatter_preserves_content_without_header() {
    let input = "Hello {{ name }}!";
    let result = strip_frontmatter(input);
    assert_eq!(result, "Hello {{ name }}!");
}

#[test]
fn contract_validation_rejects_missing_required_render_inputs() {
    let input = "[inference]\ncontract:\n  input:\n    grill_ratings:\n      type: array\n    grill_verdict:\n      type: string\n---\n{{ grill_verdict }}";
    let context = std::collections::HashMap::from([(
        "verification".to_string(),
        serde_json::json!({
            "verdict": "rewrite_needed",
            "ratings": []
        }),
    )]);

    let error = validate_contract_inputs(input, &context).expect_err("missing flat inputs");

    assert!(error.contains("grill_ratings"), "got: {error}");
    assert!(error.contains("grill_verdict"), "got: {error}");

    let present_but_blank = std::collections::HashMap::from([
        ("grill_ratings".to_string(), serde_json::json!([])),
        ("grill_verdict".to_string(), serde_json::json!("")),
    ]);
    validate_contract_inputs(input, &present_but_blank)
        .expect("required strings without non_blank retain presence-only validation");
}

#[test]
fn expectation_inquiry_requires_active_user_goal() {
    let path = registry_template_base().join("gradient-hunter/expectation-inquiry.j2");
    let template = std::fs::read_to_string(path).expect("shipped inquiry template");
    let mut context = std::collections::HashMap::from([
        (
            "expectation".to_string(),
            serde_json::json!({"statement": "expected"}),
        ),
        (
            "observation".to_string(),
            serde_json::json!({"statement": "observed"}),
        ),
        ("probe_results".to_string(), serde_json::json!([])),
    ]);
    let error = validate_contract_inputs(&template, &context)
        .expect_err("an inquiry without the user's goal cannot pass admission");
    assert!(error.contains("active_goal"), "got: {error}");

    for invalid_goal in [
        serde_json::json!(""),
        serde_json::json!(" \t\n "),
        serde_json::json!(null),
        serde_json::json!(42),
    ] {
        context.insert("active_goal".to_string(), invalid_goal);
        let error = validate_contract_inputs(&template, &context)
            .expect_err("an inquiry with a blank or non-string goal cannot render");
        assert!(error.contains("active_goal"), "got: {error}");
        assert!(error.contains("non-blank"), "got: {error}");
    }

    context.insert(
        "active_goal".to_string(),
        serde_json::json!("Investigate meaningful surprises"),
    );
    validate_contract_inputs(&template, &context).expect("goal-carrying inquiry admits");
}

#[test]
fn contract_validation_allows_optional_and_defaulted_inputs() {
    let input = "[inference]\ncontract:\n  input:\n    required_value: string\n    nullable_value: string|null\n    defaulted_value:\n      type: string\n      default: fallback\n    explicitly_optional:\n      type: string\n      required: false\n---\n{{ required_value }}";
    let context = std::collections::HashMap::from([(
        "required_value".to_string(),
        serde_json::json!("present"),
    )]);

    validate_contract_inputs(input, &context).expect("optional inputs may be absent");
}

#[test]
fn test_strip_inference_header_through_first_lone_terminator() {
    // The dominant convention: [inference]-keyed header terminated by a
    // lone `---` — NOT leading frontmatter. The old stripper matched 0
    // templates because it required the file to START with `---`.
    let input = "[inference]\ncontract:\n  input: {}\n---\nYou are a triage agent.";
    let result = strip_frontmatter(input);
    assert_eq!(result, "You are a triage agent.");
    assert!(!result.contains("contract"));
}

#[test]
fn test_inference_mention_in_running_prose_is_preserved() {
    // Only the header is stripped; an `[inference]` mention in prose stays.
    let input = "[inference]\ncontract: {}\n---\nUse the [inference] block to set temperature.";
    let result = strip_frontmatter(input);
    assert!(result.contains("Use the [inference] block"));
}

#[test]
fn test_real_triage_template_strips_clean() {
    // The shape of kask/registry/templates/kanban-task-management/triage.j2.
    let input = "{# goal: Triage. #}\n[inference]\ncontract:\n  input:\n    project_description: string\n  output:\n    phase: string\n---\n{# Ontology: PKO #}\n\nYou are a kanban task management triage agent.";
    let result = strip_frontmatter(input);
    // The {# Ontology #} Jinja comment survives stripping (minijinja
    // removes it at render time); the header must be gone.
    assert!(
        result.contains("You are a kanban task management triage agent."),
        "got: {result:?}"
    );
    assert!(!result.contains("[inference]"));
    assert!(!result.contains("contract"));
    // Render through minijinja to confirm the comment is gone too.
    let env = minijinja::Environment::new();
    let rendered = env.render_str(&result, &()).unwrap();
    assert!(rendered.contains("You are a kanban task management triage agent."));
    assert!(!rendered.contains("{#"));
}

/// The former body `[inference]` parameter stanza keys. No code reads
/// them (removed corpus-wide 2026-09-28); a line matching one is a
/// reintroduced performative stanza that would leak verbatim into the
/// rendered prompt.
fn is_inference_param_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    ["temperature", "work_effort", "verbosity", "thinking_budget"]
        .iter()
        .any(|key| {
            trimmed
                .strip_prefix(key)
                .is_some_and(|rest| rest.starts_with(" =") || rest.starts_with('='))
        })
}

#[test]
fn all_registry_templates_conform() {
    fn collect_templates(
        directory: &std::path::Path,
        templates: &mut Vec<std::path::PathBuf>,
    ) -> Result<(), String> {
        let entries = std::fs::read_dir(directory)
            .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to read an entry in {}: {error}",
                    directory.display()
                )
            })?;
            let path = entry.path();
            if path.is_dir() {
                collect_templates(&path, templates)?;
            } else if path.extension().is_some_and(|extension| extension == "j2") {
                templates.push(path);
            }
        }
        Ok(())
    }

    let base = registry_template_base();
    let mut templates = Vec::new();
    if let Err(error) = collect_templates(&base, &mut templates) {
        panic!("{error}");
    }
    assert!(!templates.is_empty(), "registry template census is empty");

    let mut parse_errors = Vec::new();
    let mut python_templates = Vec::new();
    let mut leaked_param_templates = Vec::new();
    for path in templates {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => panic!("failed to read {}: {error}", path.display()),
        };
        if content.lines().any(|line| {
            line.starts_with("import ") || (line.starts_with("from ") && line.contains(" import "))
        }) {
            python_templates.push(path.display().to_string());
        }

        if strip_frontmatter(&content)
            .lines()
            .any(|line| is_inference_param_line(line))
        {
            leaked_param_templates.push(path.display().to_string());
        }

        let Some(header) = template_metadata_header(&content) else {
            continue;
        };
        match serde_yaml::from_str::<serde_yaml::Value>(header) {
            Ok(metadata) if !metadata.is_mapping() => parse_errors.push(format!(
                "{}: metadata header must be a YAML mapping",
                path.display()
            )),
            // The header carries `contract` only. `visibility` had no
            // reader anywhere (removed corpus-wide 2026-09-28); any other
            // key is decoration nothing consumes.
            Ok(metadata) => {
                let extra: Vec<String> = metadata
                    .as_mapping()
                    .into_iter()
                    .flat_map(|mapping| mapping.keys())
                    .filter_map(|key| key.as_str())
                    .filter(|key| *key != "contract")
                    .map(str::to_owned)
                    .collect();
                if !extra.is_empty() {
                    parse_errors.push(format!(
                        "{}: header carries keys nothing reads: {}",
                        path.display(),
                        extra.join(", ")
                    ));
                }
            }
            Err(error) => parse_errors.push(format!("{}: {error}", path.display())),
        }
    }
    assert!(
        parse_errors.is_empty(),
        "failed to parse registry contract headers:\n{}",
        parse_errors.join("\n")
    );
    assert!(
        python_templates.is_empty(),
        "registry contains Python code templates:\n{}",
        python_templates.join("\n")
    );
    assert!(
        leaked_param_templates.is_empty(),
        "templates carrying a body [inference] parameter stanza \
         (performative: no code reads these keys; removed 2026-09-28):\n{}",
        leaked_param_templates.join("\n")
    );
}

#[test]
fn test_include_traversal_is_blocked() {
    let base = registry_template_base();
    let mut env = minijinja::Environment::new();
    bind_registry_loader(&mut env, &base);
    // A traversal name resolves to not-found → render error, never a
    // file outside the registry.
    let result = env.render_str("{% include \"../../Cargo.toml\" %}", serde_json::json!({}));
    assert!(result.is_err());
}

#[test]
fn test_resolve_template_path_rejects_traversal() {
    let base = registry_template_base();
    let result = resolve_template_path(&base, "../../etc/passwd");
    assert!(result.is_none(), "path traversal must be rejected");
}

#[test]
fn test_resolve_template_path_accepts_valid_ref() {
    let base = registry_template_base();
    let result = resolve_template_path(&base, "essentialist/essentialist-flow.j2");
    assert!(result.is_some(), "valid template ref must resolve");
}

#[test]
fn test_read_template_file_finds_j2() {
    let base = registry_template_base();
    let result = read_template_file(&base, "essentialist/essentialist-flow");
    assert!(
        result.is_ok(),
        "should find .j2 file with extension-less ref"
    );
    let content = result.expect("checked is_ok above");
    assert!(content.contains("---"), "template should have frontmatter");
}

// Regression for the canonicalize-before-existence-check bug: an
// extensionless ref whose exact path doesn't exist (the .j2 file does)
// must resolve via the .j2 retry, not error out as "escapes base path".
#[test]
fn test_read_template_file_finds_j2_with_extensionless_ref() {
    let base = registry_template_base();
    // This is the exact ref that failed during the prompt-enhance run.
    let result = read_template_file(&base, "prompt-enhance/enhance-classify");
    assert!(
        result.is_ok(),
        "extensionless ref should resolve to .j2 file, got: {:?}",
        result.err()
    );
    let content = result.expect("checked is_ok above");
    assert!(
        content.contains("---"),
        "enhance-classify.j2 should have frontmatter"
    );
}

#[test]
fn test_context_decodes_json_text() {
    let input =
        serde_json::json!({"template_ref": "essentialist/essentialist-flow", "context": "{}"});
    let result: RenderTemplateToolInput =
        serde_json::from_value(input).expect("stringified context must be accepted");
    assert!(
        result.context.is_empty(),
        "stringified empty object must parse to empty map"
    );
}

#[test]
fn context_json_text_preserves_nested_values_and_required_input_validation() {
    let variables = serde_json::json!({
        "task": "simplify", "items": [1, 2], "literal": "{\"not\":\"an object\"}"
    });
    let input: RenderTemplateToolInput = serde_json::from_value(serde_json::json!({
        "template_ref": "essentialist/essentialist-flow",
        "context": variables.to_string()
    }))
    .expect("JSON-text context");
    assert_eq!(input.context["items"], serde_json::json!([1, 2]));
    assert!(input.context["literal"].is_string());
    let wire = serde_json::to_value(&input).expect("native input serialization");
    assert!(wire["context"].is_string());
    let required = "[inference]\ncontract:\n  input:\n    missing: string\n---\nbody";
    assert!(validate_contract_inputs(required, &input.context).is_err());
    for invalid in [
        serde_json::json!({}),
        serde_json::json!("{\"cut\":"),
        serde_json::json!("[]"),
    ] {
        let error = serde_json::from_value::<RenderTemplateToolInput>(serde_json::json!({
            "template_ref": "probe", "context": invalid
        }))
        .expect_err("invalid context must be rejected");
        assert!(error.to_string().contains("context"));
    }
}

#[test]
fn context_path_loads_a_json_object_file() {
    let dir = std::env::temp_dir().join(format!("render-template-context-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("context.json");
    std::fs::write(
        &path,
        serde_json::to_string(&serde_json::json!({"task": "from-file", "items": [1, 2]}))
            .expect("serialize context"),
    )
    .expect("write context file");
    let input: RenderTemplateToolInput = serde_json::from_value(serde_json::json!({
        "template_ref": "essentialist/essentialist-flow",
        "context_path": path.to_string_lossy().into_owned(),
    }))
    .expect("context_path input");
    assert!(input.context.is_empty(), "inline context stays empty");
    let loaded = load_context_file(&input.context_path.unwrap()).expect("load");
    assert_eq!(loaded["task"], "from-file");
    assert_eq!(loaded["items"], serde_json::json!([1, 2]));
}

#[test]
fn context_path_rejects_non_object_and_missing_files() {
    let dir = std::env::temp_dir().join(format!(
        "render-template-context-bad-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let array_path = dir.join("array.json");
    std::fs::write(&array_path, "[1,2]").expect("write array file");
    assert!(
        load_context_file(&array_path.to_string_lossy()).is_err(),
        "non-object context file must be rejected"
    );
    assert!(
        load_context_file(&dir.join("missing.json").to_string_lossy()).is_err(),
        "missing context file must be rejected"
    );
}

#[test]
fn context_path_renders_a_real_template_end_to_end() {
    // The original failure: a context payload large enough that the model
    // could not emit it as one tool-call argument. Through context_path the
    // same payload renders without crossing the model boundary inline.
    let dir = std::env::temp_dir().join(format!(
        "render-template-context-e2e-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("context.json");
    let large_value = "x".repeat(64 * 1024);
    let context = serde_json::json!({
        "prompt": large_value,
        "prompt_type": "agent-task",
        "effort_tier": "medium",
        "risks": [],
        "checkability_map": [],
        "task": "large-context render",
        "unfold_mode_resolved": "task_graph",
        "finer_pico_framing": {},
        "reference_models": [],
        "prior_enhanced_prompt": "",
        "critic_feedback": []
    });
    std::fs::write(&path, serde_json::to_string(&context).expect("serialize")).expect("write");
    crate::set_template_base_path(registry_template_base());
    let input: RenderTemplateToolInput = serde_json::from_value(serde_json::json!({
        "template_ref": "prompt-enhance/enhance-rewrite",
        "context_path": path.to_string_lossy().into_owned(),
    }))
    .expect("context_path input");
    let rendered = render_registry_template(&input).expect("large context renders");
    assert!(
        rendered.contains("large-context render"),
        "rendered prompt must carry the file-supplied task"
    );
}

#[test]
fn context_schema_advertises_json_text() {
    let schema =
        serde_json::to_value(schemars::schema_for!(RenderTemplateToolInput)).expect("schema");
    assert_eq!(schema["properties"]["context"]["type"], "string");
    assert!(
        schema["properties"]["context"]
            .get("additionalProperties")
            .is_none()
    );
    assert!(hkask_types::find_boolean_schema_positions(&schema).is_empty());
}

// Exercise the shared normalized wire schema, not only raw schema generation.
#[test]
fn context_and_env_advertise_the_same_json_text_wire_schema() {
    fn normalized_tool_schema<T: crate::AgentTool>() -> serde_json::Value {
        let mut schema = T::input_schema().to_value();
        language_model::tool_schema::normalize_tool_schema(&mut schema);
        schema
    }
    let render = normalized_tool_schema::<super::super::RenderTemplateTool>();
    let lisp = normalized_tool_schema::<super::super::LispEvalTool>();
    for (tool_name, schema, field) in [
        ("render_template", &render, "context"),
        ("lisp_eval", &lisp, "env"),
    ] {
        let field_schema = schema
            .get("properties")
            .and_then(|p| p.get(field))
            .unwrap_or_else(|| panic!("{tool_name}.{field} missing from wire schema"));
        assert_eq!(
            field_schema.get("type"),
            Some(&serde_json::json!("string")),
            "{tool_name}.{field} must carry JSON text on the wire"
        );
        assert!(field_schema.get("properties").is_none());
        assert!(field_schema.get("additionalProperties").is_none());
        if let Some(default) = field_schema.get("default") {
            assert!(
                default.is_string(),
                "{tool_name}.{field} default must match its wire type"
            );
            let decoded: serde_json::Value =
                serde_json::from_str(default.as_str().expect("checked JSON-text default"))
                    .expect("default is valid JSON text");
            assert!(decoded.is_object());
        }
    }
    // The two fields share one declaration helper; their wire schemas must
    // be structurally identical (modulo each field's own doc-comment
    // `description`), not merely individually well-formed.
    let structural = |schema: Option<&serde_json::Value>| {
        let mut s = schema
            .expect("field schema")
            .as_object()
            .expect("object schema")
            .clone();
        s.remove("description");
        serde_json::Value::Object(s)
    };
    assert_eq!(
        structural(render.get("properties").and_then(|p| p.get("context"))),
        structural(lisp.get("properties").and_then(|p| p.get("env")))
    );
}

#[test]
fn header_with_trailing_whitespace_or_crlf_does_not_bypass_contract_validation() {
    for header_line in ["[inference]   \n", "[inference]\r\n"] {
        let template =
            format!("{header_line}contract:\n  input:\n    required_key: string\n---\nbody");
        let stripped = strip_frontmatter(&template);
        assert_eq!(stripped, "body", "strip_frontmatter stripped the header");
        let err = validate_contract_inputs(&template, &std::collections::HashMap::new())
            .expect_err("contract validation must not be bypassed when header is stripped");
        assert!(err.contains("required_key"), "got: {err}");
    }
}

#[test]
fn non_string_template_ref_teaches_field_and_shape() {
    let err = serde_json::from_value::<RenderTemplateToolInput>(
        serde_json::json!({"template_ref": 42, "context": {}}),
    )
    .expect_err("non-string template_ref must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("template_ref: received number 42"),
        "got: {msg}"
    );
}

#[test]
fn context_path_rejects_proc_symlinks_sensitive_and_uncontained_paths() {
    let scratch_dir =
        std::env::temp_dir().join(format!("render-template-s121-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch_dir);
    std::fs::create_dir_all(&scratch_dir).expect("scratch dir");

    let valid_scratch = scratch_dir.join("valid.json");
    std::fs::write(&valid_scratch, r#"{"task":"ok"}"#).expect("write valid scratch");
    let loaded = load_context_file(&valid_scratch.to_string_lossy()).expect("valid scratch loads");
    assert_eq!(loaded["task"], "ok");

    // 1. Virtual / device filesystems must be rejected outright.
    for virtual_path in [
        "/proc/self/environ",
        "/proc/self/status",
        "/sys/kernel/uevent_seqnum",
        "/dev/null",
    ] {
        assert!(
            load_context_file(virtual_path).is_err(),
            "virtual/device path must be rejected: {virtual_path}"
        );
    }

    // 2. Symlink leaf and symlink ancestor inside scratch dir must be rejected.
    #[cfg(unix)]
    {
        let target_dir = std::env::temp_dir().join(format!(
            "render-template-s121-target-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&target_dir);
        std::fs::create_dir_all(&target_dir).expect("target dir");
        let external_json = target_dir.join("secret.json");
        std::fs::write(&external_json, r#"{"leaked":"yes"}"#).expect("write external json");

        let leaf_symlink = scratch_dir.join("leaf-link.json");
        std::os::unix::fs::symlink(&external_json, &leaf_symlink).expect("leaf symlink");
        assert!(
            load_context_file(&leaf_symlink.to_string_lossy()).is_err(),
            "O_NOFOLLOW / symlink check must reject leaf symlink"
        );

        let dir_symlink = scratch_dir.join("dir-link");
        std::os::unix::fs::symlink(&target_dir, &dir_symlink).expect("dir symlink");
        let via_dir_symlink = dir_symlink.join("secret.json");
        assert!(
            load_context_file(&via_dir_symlink.to_string_lossy()).is_err(),
            "ancestor symlink check must reject symlinked directory component"
        );
        let _ = std::fs::remove_dir_all(&target_dir);
    }

    // 3. Oversized file (> MAX_CONTEXT_FILE_BYTES) must be rejected via Read::take.
    let oversized = scratch_dir.join("oversized.json");
    let mut big = String::with_capacity(MAX_CONTEXT_FILE_BYTES + 64);
    big.push_str("{\"blob\":\"");
    big.push_str(&"a".repeat(MAX_CONTEXT_FILE_BYTES + 16));
    big.push_str("\"}");
    std::fs::write(&oversized, big.as_bytes()).expect("write oversized");
    assert!(
        load_context_file(&oversized.to_string_lossy()).is_err(),
        "oversized context_path must be rejected"
    );

    // 4. Sensitive paths & agent output spills must be rejected even when valid JSON.
    let dotenv = scratch_dir.join(".env");
    std::fs::write(&dotenv, r#"{"secret":"1"}"#).expect("write .env");
    assert!(load_context_file(&dotenv.to_string_lossy()).is_err());

    let dotenv_local = scratch_dir.join(".env.local");
    std::fs::write(&dotenv_local, r#"{"secret":"1"}"#).expect("write .env.local");
    assert!(load_context_file(&dotenv_local.to_string_lossy()).is_err());

    let pem = scratch_dir.join("cert.pem");
    std::fs::write(&pem, r#"{"secret":"1"}"#).expect("write .pem");
    assert!(load_context_file(&pem.to_string_lossy()).is_err());

    let key = scratch_dir.join("priv.key");
    std::fs::write(&key, r#"{"secret":"1"}"#).expect("write .key");
    assert!(load_context_file(&key.to_string_lossy()).is_err());

    let local_zed = scratch_dir.join(".zed");
    std::fs::create_dir_all(&local_zed).expect(".zed dir");
    let local_settings = local_zed.join("settings.json");
    std::fs::write(&local_settings, r#"{"private_files":[]}"#).expect("write local settings");
    assert!(
        load_context_file(&local_settings.to_string_lossy()).is_err(),
        ".zed local settings must be rejected"
    );

    let global_settings = paths::config_dir().join("settings.json");
    assert!(
        load_context_file(&global_settings.to_string_lossy()).is_err(),
        "global config_dir settings.json must be rejected"
    );
    assert!(
        load_context_file("~/.config/zed-kask/settings.json").is_err(),
        "tilde global config_dir settings.json must be rejected"
    );
    assert!(
        load_context_file("~/.ssh/config").is_err(),
        "~/.ssh must be rejected"
    );
    assert!(
        load_context_file("~/.aws/credentials").is_err(),
        "~/.aws must be rejected"
    );
    assert!(
        load_context_file("~/.gnupg/gpg.conf").is_err(),
        "~/.gnupg must be rejected"
    );

    let agent_spill =
        std::env::temp_dir().join(format!("zed-agent-output-s121-{}.json", std::process::id()));
    std::fs::write(&agent_spill, r#"{"spilled":"output"}"#).expect("write spill");
    let spill_res = load_context_file(&agent_spill.to_string_lossy());
    let _ = std::fs::remove_file(&agent_spill);
    assert!(
        spill_res.is_err(),
        "/tmp/zed-agent-output-* spill files must be rejected"
    );

    // Non-.json scratch file under temp_dir must be rejected.
    let scratch_txt = scratch_dir.join("notes.txt");
    std::fs::write(&scratch_txt, r#"{"not":"json-ext"}"#).expect("write txt");
    assert!(
        load_context_file(&scratch_txt.to_string_lossy()).is_err(),
        "non-.json scratch file under temp_dir must be rejected"
    );

    // Unbounded virtual reader (st_size == 0 analogue) stops at MAX_CONTEXT_FILE_BYTES + 1.
    let infinite_err = read_bounded_context_json(std::io::repeat(b' '), "/proc/self/environ")
        .expect_err("infinite reader must stop at MAX_CONTEXT_FILE_BYTES + 1");
    assert!(
        infinite_err.contains("exceeds the"),
        "unexpected error: {infinite_err}"
    );

    // Uncontained host paths outside worktrees/skills/artifacts/temp must be rejected.
    let outside_host = paths::home_dir().join("uncontained-host-probe.json");
    assert!(
        load_context_file(&outside_host.to_string_lossy()).is_err(),
        "host file outside permitted roots must be rejected"
    );
    assert!(
        load_context_file("relative/without-project.json").is_err(),
        "bare relative path without Project must be rejected"
    );

    // Valid worktree root explicitly admitted via load_context_file_with_roots succeeds.
    let wt_dir = scratch_dir.join("worktree-root");
    std::fs::create_dir_all(&wt_dir).expect("wt dir");
    let wt_context = wt_dir.join("payload.json");
    std::fs::write(&wt_context, r#"{"worktree":"accepted"}"#).expect("write wt json");
    let wt_loaded =
        load_context_file_with_roots(&wt_context.to_string_lossy(), std::slice::from_ref(&wt_dir))
            .expect("worktree json loads");
    assert_eq!(wt_loaded["worktree"], "accepted");

    let _ = std::fs::remove_dir_all(&scratch_dir);
}

#[gpui::test]
async fn context_path_enforces_worktree_private_files_and_admits_public_worktree_json(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
    });

    let wt_temp = tempfile::tempdir().expect("real worktree tempdir");
    let public_disk_json = wt_temp.path().join("public_context.json");
    std::fs::write(&public_disk_json, r#"{"task": "worktree-public"}"#).expect("write public");

    let fs = fs::FakeFs::new(cx.executor());
    fs.insert_tree(
        wt_temp.path(),
        serde_json::json!({
            ".zed": {
                "settings.json": r#"{"private_files": ["**/private_secret.json"]}"#
            },
            "public_context.json": r#"{"task": "worktree-public"}"#,
            "private_secret.json": r#"{"task": "worktree-private"}"#,
        }),
    )
    .await;
    let public_json = wt_temp.path().join("public_context.json");
    let private_json = wt_temp.path().join("private_secret.json");

    cx.update(|cx| {
        use gpui::UpdateGlobal as _;
        settings::SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.private_files =
                    Some(vec!["**/private_secret.json".to_string()].into());
            });
        });
    });

    let project = project::Project::test(fs, [wt_temp.path()], cx).await;

    let mut async_cx = cx.to_async();
    let private_err =
        resolve_project_context_path(&project, &private_json.to_string_lossy(), &mut async_cx)
            .await;
    assert!(
        private_err.is_err(),
        "WorktreeSettings::is_path_private must reject private_secret.json even under temp_dir"
    );

    let public_ok =
        resolve_project_context_path(&project, &public_json.to_string_lossy(), &mut async_cx)
            .await
            .expect("non-private worktree json must resolve");
    match public_ok {
        ProjectContextPathResolution::WorktreeFile {
            abs_path,
            canonical_roots,
        } => {
            let map = load_context_file_with_roots(&abs_path.to_string_lossy(), &canonical_roots)
                .expect("worktree json loads");
            assert_eq!(map["task"], "worktree-public");
        }
        ProjectContextPathResolution::External => {
            panic!("expected WorktreeFile resolution for in-worktree context_path")
        }
    }
}
