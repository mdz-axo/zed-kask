//! Contract check for the agent-executed company research gates, not a Rust
//! publication interceptor or a substitute for independent source review.

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const HANDOFF: &str =
    include_str!("../../../registry/templates/company-research/verification-handoff.j2");
const FLASH: &str = include_str!("../../../../.agents/skills/company-research-flash/SKILL.md");
const DEEP: &str = include_str!("../../../../.agents/skills/company-research-deep/SKILL.md");
const FIXTURES: &str =
    include_str!("../../../registry/company-research-fixtures/verification.json");

fn single_lisp_form(section: &str) -> Result<&str> {
    ensure!(
        section.matches("```lisp").count() == 1,
        "expected exactly one Lisp gate in the selected skill section"
    );
    let (_, fenced) = section.split_once("```lisp").context("Lisp fence")?;
    let (form, _) = fenced.split_once("```").context("closing Lisp fence")?;
    ensure!(!form.trim().is_empty(), "Lisp gate must not be empty");
    Ok(form.trim())
}

fn source_check_form() -> Result<&'static str> {
    HANDOFF
        .split_once("```source-check-lisp")
        .context("source-check form")?
        .1
        .split_once("```")
        .map(|(form, _)| form.trim())
        .context("closing source-check fence")
}

fn source_status(packet: &Value) -> Result<Value> {
    let mut input = packet.clone();
    input["disclosures"] = packet["disclosure_inventory"].clone();
    hkask_lisp::eval_sandboxed(source_check_form()?, &input)
        .context("check original disclosure and observed search/extraction log")
}

fn cases<'a>(fixtures: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    let cases = fixtures[key]
        .as_array()
        .context("missing fixture case array")?;
    ensure!(!cases.is_empty(), "fixture case array must not be empty");
    Ok(cases)
}

/// expect: Both research consumers supply the same snapshot inputs at first
/// commitment and on changes; these pins only check instructions, not execution.
#[test]
fn company_consumers_keep_shared_handoff_wired_at_both_times() -> Result<()> {
    for (name, skill) in [("flash", FLASH), ("deep", DEEP)] {
        ensure!(
            skill.contains("candidate commitment") || skill.contains("first commitment"),
            "{name}: missing early interruption"
        );
        for field in [
            "issuer_identifier",
            "as_of_date",
            "disclosure_inventory",
            "original_forecast",
            "working_forecast",
            "source_review_status",
        ] {
            ensure!(
                skill.contains(field),
                "{name}: missing shared handoff input {field}"
            );
        }
        ensure!(
            skill.contains("company-research/verification-handoff"),
            "{name}: missing shared handoff call"
        );
    }
    Ok(())
}

/// expect: A high verification score cannot authorize flash publication if a
/// load-bearing claim failed, checks were not performed, or the source packet
/// was not supplied. Passing verification remains subject to ENTER/confidence.
#[test]
fn company_handoff_gate_controls_flash_publication() -> Result<()> {
    let fixtures: Value = serde_json::from_str(FIXTURES)?;
    let verification_form = single_lisp_form(HANDOFF)?;
    let status = source_status(&fixtures["packet"])?;
    ensure!(
        status == "checked",
        "clean fixture did not reconcile: {status}"
    );
    let flash_section = FLASH
        .split_once("### verify-before-publish")
        .context("flash verification section")?
        .1
        .split_once("### persist-report")
        .context("flash publication boundary")?
        .0;
    let publication_form = single_lisp_form(flash_section)?;

    for case in cases(&fixtures, "gate_cases")? {
        let name = case["name"].as_str().context("gate case name")?;
        let expected = case["expected"].as_str().context("gate case expectation")?;
        let mut input = case.clone();
        input["source_review_status"] = status.clone();
        input["original_forecast"] = Value::Null;
        input["working_forecast"] = Value::Null;
        let gate = hkask_lisp::eval_sandboxed(verification_form, &input)
            .with_context(|| format!("evaluate handoff gate for {name}"))?;
        ensure!(gate == expected, "{name}: expected {expected}, got {gate}");

        // Carry the actual gate result into the final decision: a high score,
        // high confidence and ENTER eligibility cannot override a blocked gate.
        let publication = hkask_lisp::eval_sandboxed(
            publication_form,
            &json!({"enter_eligible":true, "unadjusted_confidence":0.9,
                "confidence_adjustment":0.0, "verification_gate":gate}),
        )
        .with_context(|| format!("evaluate flash publication for {name}"))?;
        let publish = publication
            .get(1)
            .and_then(Value::as_bool)
            .context("flash publication Boolean")?;
        ensure!(
            publish == (expected == "passed"),
            "{name}: unexpected publication result {publication}"
        );
    }

    for case in cases(&fixtures, "publication_cases")? {
        let name = case["name"].as_str().context("publication case name")?;
        let expected = case["expected_publish"]
            .as_bool()
            .context("publication case expectation")?;
        let publication = hkask_lisp::eval_sandboxed(publication_form, case)
            .with_context(|| format!("evaluate flash publication for {name}"))?;
        let publish = publication
            .get(1)
            .and_then(Value::as_bool)
            .context("flash publication Boolean")?;
        ensure!(
            publish == expected,
            "{name}: unexpected result {publication}"
        );
    }

    // Negative control: deleting the final verification check must make a
    // blocked case publishable, so the fixtures actually exercise that seam.
    let unchecked = publication_form.replacen(
        "(member verification_gate (list \"passed\"))",
        "(>= adjusted 0.50)",
        1,
    );
    ensure!(
        unchecked != publication_form,
        "publication gate changed shape"
    );
    let unchecked_result = hkask_lisp::eval_sandboxed(
        &unchecked,
        &json!({"enter_eligible":true, "unadjusted_confidence":0.9,
            "confidence_adjustment":0.0, "verification_gate":"needs_work"}),
    )?;
    ensure!(
        unchecked_result.get(1).and_then(Value::as_bool) == Some(true),
        "negative control did not remove the publication guard"
    );
    Ok(())
}

/// expect: A perfect fact score cannot authorize a report when independent
/// disclosure review was not performed or found a material omission, or when
/// EQM silently changed the frozen forecast instead of just its rationale.
#[test]
fn company_handoff_requires_source_and_forecast_integrity() -> Result<()> {
    let gate = single_lisp_form(HANDOFF)?;
    let fixtures: Value = serde_json::from_str(FIXTURES)?;
    let packet = &fixtures["packet"];
    let original = json!({
        "statement": "Will income exceed the threshold?", "horizon": "strategic",
        "probability": 0.5, "resolution_criteria": "Annual survey release",
        "expiration_date": "2027-01-31", "trajectory_basis": "survey baseline",
        "rationale_sources": ["source:survey"]
    });
    let rationale_edit = json!({"rationale":"Rewritten after checking evidence"});
    let mut edited = original.clone();
    edited["rationale"] = rationale_edit["rationale"].clone();
    let mut changed_probability = original.clone();
    changed_probability["probability"] = json!(0.75);
    let mut changed_rule = original.clone();
    changed_rule["resolution_criteria"] = json!("Management estimate");
    let mut changed_deadline = original.clone();
    changed_deadline["expiration_date"] = json!("2028-01-31");

    for (name, packet_variant, revised, expected_source, expected_gate) in [
        (
            "clean",
            packet.clone(),
            original.clone(),
            "checked",
            "passed",
        ),
        (
            "rationale_only",
            packet.clone(),
            edited,
            "checked",
            "passed",
        ),
        (
            "changed_probability",
            packet.clone(),
            changed_probability,
            "checked",
            "needs_work",
        ),
        (
            "changed_resolution",
            packet.clone(),
            changed_rule,
            "checked",
            "needs_work",
        ),
        (
            "changed_deadline",
            packet.clone(),
            changed_deadline,
            "checked",
            "needs_work",
        ),
        (
            "unperformed_discovery",
            {
                let mut p = packet.clone();
                p["pipeline_tool_log"] = json!([]);
                p
            },
            original.clone(),
            "not_checked",
            "incomplete",
        ),
        (
            "omitted_regulatory_disclosure",
            {
                let mut p = packet.clone();
                p["source_outputs"].as_array_mut().context("source outputs")?.push(json!({
                    "tool_name":"web_extract", "description":"Synthetic regulator decision",
                    "output_key":"source:web_extract:regulator",
                    "output":{"content":"The regulator issued a material sanction in July 2026."},
                    "source_kind":"original", "url":"https://regulator.example.invalid/decision",
                    "retrieved_at":"2026-09-24", "period":"2026-07", "unit":null
                }));
                p["pipeline_tool_log"]
                    .as_array_mut()
                    .context("tool log")?
                    .push(json!({
                        "tool_name":"web_extract", "output_key":"source:web_extract:regulator",
                        "status":"ok"
                    }));
                p["disclosure_inventory"]
                    .as_array_mut()
                    .context("inventory")?
                    .push(json!({
                        "output_key":"source:web_extract:regulator", "published_at":"2026-07-15",
                        "quote":"The regulator issued a material sanction in July 2026.",
                        "report_marker":"material sanction in July 2026"
                    }));
                p
            },
            original.clone(),
            "material_omission",
            "needs_work",
        ),
        (
            "uninventoried_original",
            {
                let mut p = packet.clone();
                p["source_outputs"]
                    .as_array_mut()
                    .context("source outputs")?
                    .push(json!({
                        "tool_name":"web_extract", "output_key":"source:web_extract:unlisted",
                        "output":{"content":"Material regulator action."}, "source_kind":"original"
                    }));
                p
            },
            original.clone(),
            "not_checked",
            "incomplete",
        ),
        (
            "irrelevant_original_with_reason",
            {
                let mut p = packet.clone();
                p["source_outputs"]
                    .as_array_mut()
                    .context("source outputs")?
                    .push(json!({
                        "tool_name":"web_extract", "output_key":"source:web_extract:unrelated",
                        "output":{"content":"Public contact details."}, "source_kind":"original"
                    }));
                p["pipeline_tool_log"].as_array_mut().context("tool log")?.push(json!({
                    "tool_name":"web_extract", "output_key":"source:web_extract:unrelated", "status":"ok"
                }));
                p["disclosure_inventory"]
                    .as_array_mut()
                    .context("inventory")?
                    .push(json!({
                        "output_key":"source:web_extract:unrelated", "published_at":"2026-07-15",
                        "quote":"Public contact details.", "disposition":"not_material",
                        "reason":"Contact page contains no business or regulatory disclosure"
                    }));
                p
            },
            original.clone(),
            "checked",
            "passed",
        ),
        (
            "generated_summary_only",
            {
                let mut p = packet.clone();
                p["source_outputs"][0]["source_kind"] = json!("synthesis");
                p["source_outputs"][0]["output"]["content"] = json!("We announced partnerships.");
                p
            },
            original.clone(),
            "not_checked",
            "incomplete",
        ),
        (
            "quote_only_in_generated_answer",
            {
                let mut p = packet.clone();
                p["source_outputs"]
                    .as_array_mut()
                    .context("source outputs")?
                    .push(json!({
                        "tool_name":"web_search", "description":"Generated search answer",
                        "output_key":"source:web_search:answer",
                        "output":{"content":"All partners are paying customers."},
                        "source_kind":"synthesis", "url":null,
                        "retrieved_at":"2026-09-24", "period":null, "unit":null
                    }));
                p["pipeline_tool_log"]
                    .as_array_mut()
                    .context("tool log")?
                    .push(json!({
                        "tool_name":"web_search", "output_key":"source:web_search:answer",
                        "status":"ok"
                    }));
                p["disclosure_inventory"]
                    .as_array_mut()
                    .context("inventory")?
                    .push(json!({
                        "output_key":"source:web_search:answer", "published_at":"2026-07-15",
                        "quote":"All partners are paying customers.",
                        "report_marker":"All partners are paying customers."
                    }));
                p["target_text"] =
                    json!("We announced partnerships. All partners are paying customers.");
                p
            },
            original.clone(),
            "not_checked",
            "incomplete",
        ),
        (
            "missing_as_of",
            {
                let mut p = packet.clone();
                p["as_of_date"] = Value::Null;
                p
            },
            original.clone(),
            "not_checked",
            "incomplete",
        ),
        (
            "missing_original",
            {
                let mut p = packet.clone();
                p["source_outputs"] = json!([]);
                p
            },
            original.clone(),
            "not_checked",
            "incomplete",
        ),
    ] {
        let status = source_status(&packet_variant)?;
        ensure!(
            status == expected_source,
            "{name}: expected source status {expected_source}, got {status}"
        );
        let input = json!({"fact_score":1.0,"claims_checked":3,"decoupling":"spawn_agent",
            "checks_complete":true,"material_failure":false,"source_review_status":status,
            "original_forecast":original,"working_forecast":revised});
        let result = hkask_lisp::eval_sandboxed(gate, &input)
            .with_context(|| format!("evaluate source/forecast integrity: {name}"))?;
        ensure!(
            result == expected_gate,
            "{name}: expected {expected_gate}, got {result}"
        );
        eprintln!("{name}: source={status}, gate={result}");
    }
    Ok(())
}
