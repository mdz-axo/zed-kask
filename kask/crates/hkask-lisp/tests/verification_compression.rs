use serde_json::{Value, json};

const SKILL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../.agents/skills/verification-compression/SKILL.md"
));

fn form_after(marker: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let start = SKILL
        .find(marker)
        .ok_or_else(|| std::io::Error::other(format!("skill is missing form marker {marker}")))?;
    let remainder = &SKILL[start + marker.len()..];
    let end = remainder
        .find('`')
        .ok_or_else(|| std::io::Error::other("skill form has no closing backtick"))?;
    Ok(&remainder[..end])
}

fn delta_form() -> Result<&'static str, Box<dyn std::error::Error>> {
    let marker = "- form: `";
    let start = SKILL
        .rfind(marker)
        .ok_or_else(|| std::io::Error::other("skill has no delta form"))?;
    let remainder = &SKILL[start + marker.len()..];
    let end = remainder
        .find('`')
        .ok_or_else(|| std::io::Error::other("delta form has no closing backtick"))?;
    Ok(&remainder[..end])
}

fn evaluate(form: &str, environment: Value) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(hkask_lisp::eval_sandboxed_with_budget(
        form,
        &environment,
        100_000,
        1024,
    )?)
}

fn context(mode: &str, changed_source: bool, approved: bool) -> Value {
    json!({
        "mode": mode,
        "timing_scope": "warm",
        "toolchain_before": "rustc-fixture",
        "toolchain_after": "rustc-fixture",
        "environment_before": "same-environment",
        "environment_after": "same-environment",
        "oracle_hash_before": "fixed-oracle",
        "oracle_hash_after": "fixed-oracle",
        "contract_hash_before": "fixed-contract",
        "contract_hash_after": "fixed-contract",
        "source_hash_before": "source-before",
        "source_hash_after": if changed_source { "source-after" } else { "source-before" },
        "source_changed": changed_source,
        "authorized_diff_verified": approved,
        "hashes_verified": true
    })
}

fn metrics(code_nodes_before: i64, code_nodes_after: i64, warm_after: [i64; 2]) -> Value {
    json!({
        "nodes_before": 9, "edges_before": 8,
        "nodes_after": 5, "edges_after": 4,
        "code_graph_status": "measured",
        "code_nodes_before": code_nodes_before,
        "code_edges_before": 2,
        "code_nodes_after": code_nodes_after,
        "code_edges_after": 2,
        "cold": {"status":"not_run", "timing_samples_before_ms":[], "timing_samples_after_ms":[], "time_before_ms":0, "time_after_ms":0},
        "warm": {"status":"measured", "timing_samples_before_ms":[200, 210], "timing_samples_after_ms":warm_after, "time_before_ms":205, "time_after_ms":190}
    })
}

/// expect: "No-edit analysis cannot certify production code-graph compression." [P8]
#[test]
fn pinned_forms_reject_no_edit_code_reduction() -> Result<(), Box<dyn std::error::Error>> {
    let check = form_after("- Context and samples: `")?;
    let delta = delta_form()?;
    let environment =
        json!({"context":context("analyze", false, false),"metrics":metrics(10, 1, [180, 200])});
    assert_eq!(evaluate(check, environment.clone())?, json!(false));
    assert_eq!(
        evaluate(delta, environment)?["code_graph_compression"],
        Value::Null
    );
    Ok(())
}

/// expect: "An approved source refactor is not falsely rejected as changed evidence." [P8]
#[test]
fn pinned_forms_admit_approved_edit_with_fixed_oracle() -> Result<(), Box<dyn std::error::Error>> {
    let check = form_after("- Context and samples: `")?;
    let delta = delta_form()?;
    let environment =
        json!({"context":context("execute", true, true),"metrics":metrics(10, 7, [180, 200])});
    assert_eq!(evaluate(check, environment.clone())?, json!(true));
    assert_eq!(
        evaluate(delta, environment)?["code_graph_compression"],
        json!(0.25)
    );
    Ok(())
}

/// expect: "A verified empty diff cannot earn code-graph compression." [P8]
#[test]
fn pinned_forms_reject_approved_but_unchanged_source() -> Result<(), Box<dyn std::error::Error>> {
    let check = form_after("- Context and samples: `")?;
    let delta = delta_form()?;
    let mut untrusted_report = context("execute", false, true);
    untrusted_report["source_hash_after"] = json!("invented-different-hash");
    let environment = json!({"context":untrusted_report,"metrics":metrics(10, 5, [180, 200])});
    assert_eq!(evaluate(check, environment.clone())?, json!(false));
    assert_eq!(
        evaluate(delta, environment)?["code_graph_compression"],
        Value::Null
    );
    Ok(())
}

/// expect: "An impossible measured duration never opens the acceleration gate." [P8]
#[test]
fn pinned_context_form_rejects_nonpositive_sample() -> Result<(), Box<dyn std::error::Error>> {
    let check = form_after("- Context and samples: `")?;
    let environment =
        json!({"context":context("execute", true, true),"metrics":metrics(10, 7, [-10, 20])});
    assert_eq!(evaluate(check, environment)?, json!(false));
    Ok(())
}
