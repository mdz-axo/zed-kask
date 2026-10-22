//! Exercise the authored output contract and gates, including retained live render receipts.
use hkask_lisp::eval_sandboxed;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn template() -> String {
    fs::read_to_string(
        root().join("kask/registry/templates/experimental-design/normalize-hypothesis.j2"),
    )
    .expect("read real normalization template")
}

fn schema(text: &str) -> Value {
    let section = text
        .split_once("## Output schema")
        .expect("authored output schema")
        .1;
    let block = section
        .split_once("```json\n")
        .expect("JSON schema fence")
        .1;
    serde_json::from_str(block.split_once("\n```").expect("schema fence end").0)
        .expect("valid authored JSON schema")
}

fn form(marker: &str) -> String {
    let skill = fs::read_to_string(root().join(".agents/skills/experimental-design/SKILL.md"))
        .expect("read canonical skill");
    skill
        .split("```lisp\n")
        .filter_map(|block| block.split_once("\n```"))
        .map(|(body, _)| body)
        .find(|body| body.contains(marker))
        .expect("authored gate")
        .to_owned()
}

fn validate(schema: &Value, value: &Value) -> bool {
    let kind = &schema["type"];
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.as_str().is_some_and(|s| !s.is_empty()),
        "null" => value.is_null(),
        _ => panic!("unsupported schema type: {kind}"),
    };
    let typed = match kind {
        Value::String(kind) => matches(kind),
        Value::Array(kinds) => kinds
            .iter()
            .any(|kind| matches(kind.as_str().expect("type name"))),
        _ => panic!("schema type required"),
    };
    if !typed {
        return false;
    }
    if let Some(choices) = schema["enum"].as_array() {
        return choices.contains(value);
    }
    if let Some(object) = value.as_object() {
        let properties = schema["properties"].as_object().expect("object properties");
        let required = schema["required"].as_array().expect("required keys");
        return required
            .iter()
            .all(|key| object.contains_key(key.as_str().expect("required key")))
            && (schema["additionalProperties"] != false
                || object.keys().all(|key| properties.contains_key(key)))
            && object.iter().all(|(key, value)| {
                properties
                    .get(key)
                    .is_none_or(|schema| validate(schema, value))
            });
    }
    if let Some(items) = value.as_array() {
        return items.iter().all(|item| validate(&schema["items"], item));
    }
    true
}

fn reconciles(output: &Value, contract: &Value) -> bool {
    if !validate(contract, output) {
        return false;
    }
    let owners_match = output["owner_returns"]
        .as_array()
        .expect("owner rows")
        .iter()
        .all(|row| {
            let field = &row["field"];
            let owner = row["owner"].as_str().expect("owner string");
            let partition = if output["framer_missing"]
                .as_array()
                .expect("partition")
                .contains(field)
            {
                "hypothesis-framer"
            } else if output["falsifiability_missing"]
                .as_array()
                .expect("partition")
                .contains(field)
            {
                "falsifiability"
            } else {
                "operator_or_source"
            };
            owner == partition
        });
    owners_match && eval_sandboxed(&form("define unique-strings"), output).expect("ED-0R") == true
}

#[test]
fn authored_contract_feeds_gates_and_retains_positive_missing_fields() {
    let authored = template();
    let contract = schema(&authored);
    let required = contract["required"].as_array().expect("required keys");
    assert_eq!(required.len(), 7, "all seven handoff fields are required");
    let header = authored
        .split_once("---")
        .expect("contract header terminator")
        .0;
    for key in required {
        assert!(
            header.contains(&format!("    {}:", key.as_str().expect("key"))),
            "header and rendered schema must agree"
        );
    }
    let mut output = json!({
        "hypothesis_spec": null,
        "missing_fields": ["research_question", "falsifiability_inputs.admitted_target_artifact"],
        "owner_returns": [
            {"field": "research_question", "owner": "hypothesis-framer", "reason": "fixture omits question"},
            {"field": "falsifiability_inputs.admitted_target_artifact", "owner": "falsifiability", "reason": "fixture omits admission"}
        ],
        "framer_missing": ["research_question"],
        "falsifiability_missing": ["falsifiability_inputs.admitted_target_artifact"],
        "provenance_missing": [], "constraints_missing": []
    });
    assert!(
        reconciles(&output, &contract),
        "positive missing fixture must reconcile"
    );
    assert_eq!(
        eval_sandboxed(&form("blocked: malformed_normalization"), &output).expect("ED-0"),
        "return_to_framer"
    );
    output["owner_returns"][0]["owner"] = json!("falsifiability");
    assert!(!reconciles(&output, &contract), "wrong owner must fail");
    output["owner_returns"][0]["owner"] = json!("hypothesis-framer");
    output["framer_missing"] = json!([]);
    assert!(
        !reconciles(&output, &contract),
        "dropped partition must fail"
    );
    for key in [
        "framer_missing",
        "falsifiability_missing",
        "provenance_missing",
        "constraints_missing",
    ] {
        output.as_object_mut().expect("output").remove(key);
    }
    assert!(
        !reconciles(&output, &contract),
        "old three-key output cannot feed gates"
    );
    let ready = json!({
        "hypothesis_spec": {"spec_version": "hypothesis-spec-v1"},
        "missing_fields": [], "owner_returns": [], "framer_missing": [],
        "falsifiability_missing": [], "provenance_missing": [], "constraints_missing": []
    });
    assert!(
        reconciles(&ready, &contract),
        "complete fixture must remain legitimate"
    );
    assert_eq!(
        eval_sandboxed(&form("blocked: malformed_normalization"), &ready).expect("ED-0"),
        "ready"
    );
}

#[test]
#[ignore = "requires NORMALIZATION_RUN_DIR containing actual render_template and output receipts"]
fn retained_live_render_and_output_feed_authored_gates() {
    let run =
        PathBuf::from(std::env::var("NORMALIZATION_RUN_DIR").expect("retained normalization run"));
    let rendered = fs::read_to_string(run.join("rendered-normalize-hypothesis.md"))
        .expect("live render receipt");
    let output: Value = serde_json::from_str(
        &fs::read_to_string(run.join("normalization-output.json")).expect("actual output"),
    )
    .expect("output JSON");
    let context: Value = serde_json::from_str(
        &fs::read_to_string(run.join("render-context.json")).expect("actual render context"),
    )
    .expect("context JSON");
    assert_eq!(
        schema(&rendered),
        schema(&template()),
        "live renderer must expose authored output contract"
    );
    for (key, marker) in [
        ("framing_artifact", "## Framing artifact\n\n"),
        ("falsifiability_artifact", "## Falsifiability artifact\n\n"),
        ("source_provenance", "- source_provenance: "),
        ("constraints", "- constraints: "),
        ("unresolved_assumptions", "- unresolved_assumptions: "),
    ] {
        let line = rendered
            .split_once(marker)
            .expect("rendered input")
            .1
            .lines()
            .next()
            .expect("JSON input line");
        let actual: Value = serde_json::from_str(line).expect("rendered JSON input");
        assert_eq!(actual, context[key], "live prompt must retain exact {key}");
    }
    assert!(
        reconciles(&output, &schema(&rendered)),
        "actual output must feed ED-0R without reconstructed arrays"
    );
    let calls: Value = serde_json::from_str(
        &fs::read_to_string(run.join("gate-inputs.json")).expect("retained actual gate inputs"),
    )
    .expect("gate inputs JSON");
    for (key, marker) in [
        ("ED_0R", "define unique-strings"),
        ("ED_0", "blocked: malformed_normalization"),
    ] {
        assert_eq!(
            calls[key]["env"], output,
            "gate must consume unchanged output"
        );
        assert_eq!(
            calls[key]["form"],
            form(marker),
            "retained call must use authored form"
        );
    }
    let receipt: Value = serde_json::from_str(
        &fs::read_to_string(run.join("gate-receipts.json")).expect("gate receipt"),
    )
    .expect("receipt JSON");
    assert_eq!(
        receipt["ED_0R"],
        eval_sandboxed(&form("define unique-strings"), &output).expect("ED-0R")
    );
    assert_eq!(
        receipt["ED_0"],
        eval_sandboxed(&form("blocked: malformed_normalization"), &output).expect("ED-0")
    );
    let mut dropped = output;
    dropped
        .as_object_mut()
        .expect("output")
        .remove("falsifiability_missing");
    assert!(
        !reconciles(&dropped, &schema(&rendered)),
        "live handoff must reject a dropped partition"
    );
}
