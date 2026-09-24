//! Contract check for the agent-executed company research gates, not a Rust
//! publication interceptor or a substitute for independent source review.

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

const HANDOFF: &str =
    include_str!("../../../registry/templates/company-research/verification-handoff.j2");
const FLASH: &str = include_str!("../../../../.agents/skills/company-research-flash/SKILL.md");
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

fn cases<'a>(fixtures: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    let cases = fixtures[key]
        .as_array()
        .context("missing fixture case array")?;
    ensure!(!cases.is_empty(), "fixture case array must not be empty");
    Ok(cases)
}

/// expect: A high verification score cannot authorize flash publication if a
/// load-bearing claim failed, checks were not performed, or the source packet
/// was not supplied. Passing verification remains subject to ENTER/confidence.
#[test]
fn company_handoff_gate_controls_flash_publication() -> Result<()> {
    let fixtures: Value = serde_json::from_str(FIXTURES)?;
    let verification_form = single_lisp_form(HANDOFF)?;
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
        let gate = hkask_lisp::eval_sandboxed(verification_form, case)
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
