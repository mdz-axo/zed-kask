//! Local grounding gate — the port of fermi's `grounding_trust` +
//! `completeness` + `reliance` over local agent output contracts.
//!
//! fermi's 56-commit trust wave (62c434f6..1468f18b) established the
//! verification model this module implements locally:
//!
//! - **Grounding** (`grounding_trust::enforce`): a contracted field declared
//!   `sourced` must have come from a tool; a field declared `unavailable`
//!   must be null. A value that could not have come from anywhere is
//!   **stripped** (nulled) before the answer is relied on — the run is not
//!   refused, the fabricated part just does not travel.
//! - **Completeness** (`completeness.rs`): an empty contracted field is
//!   three different situations with three different owners — nobody's
//!   (the contract requires null), the world's (a tool was asked and had
//!   nothing), or the agent's (a named tool never called, or commissioned
//!   work absent). Only the agent's faults are `owed`.
//! - **Reliance** (`reliance.rs`): one token, worst-first, derived from the
//!   verdicts above — never recomputed — so a caller answers "can I use
//!   this answer?" by reading one field instead of re-deriving the
//!   platform's judgement from four sub-blocks.
//!
//! # The local adaptation
//!
//! fermi checks `sourced` fields against `tool_invocations` on the episode.
//! The local analog is the delegation's `tool_calls` summary (stamped by
//! `AgentExecutor`): a `sourced` block with a value and zero tool calls in
//! the run is a value with no possible source — stripped. The raw
//! `response` string is kept verbatim on the result (fermi keeps
//! `episodes.response_text` raw on purpose: it is the evidence of what the
//! model claimed); the enforced document travels beside it in the
//! `grounding` block, so a caller never has to scrape JSON out of prose to
//! get the trustworthy half.
//!
//! # What this deliberately does not judge
//!
//! Same boundary as fermi's completeness gate: *the tool answered with
//! substance and the field is still empty* is not assessed — telling it
//! apart from an honest miss needs a judgement about whether the answer was
//! inside that response. A gate must not accuse on a judgement. `owed`
//! counts only the unambiguous.
//!
//! Provenance stamps use a closed vocabulary (the grounding-verify skill's
//! lattice): `tool_verified`, `model_inference`, `derived`, `stripped`,
//! `no_data`, `owed`, `excused_null`. A stamp outside the set is a bug.

use serde_json::{Map, Value};

/// The worst-first reliance ladder, in evaluation order. Ported verbatim
/// from fermi's `reliance::reliance` — the ordering is argued there and the
/// arguments hold locally: `amended` outranks `incomplete` (an invented
/// value is evidence about the model; an absent one is a gap), `malformed`
/// outranks `amended` (a caller that planned its parse around the declared
/// type is already wrong), and `unverified_*` never lowers reliance (absent
/// must look different from bad).
pub const RELIANCE_TOKENS: [&str; 6] = [
    "unusable",
    "malformed",
    "amended",
    "incomplete",
    "unchecked",
    "clean",
];

/// One sentence per token, said once here so every consumer reads the same
/// gloss (fermi's `reliance::why` — the vocabulary and the precedence
/// cannot drift from the explanation).
pub fn reliance_why(token: &str) -> &'static str {
    match token {
        "unusable" => "no structured document at all — prose, or nothing",
        "malformed" => "the document contradicts the type the agent declared",
        "amended" => "the platform removed values no tool could have supplied",
        "incomplete" => "the agent did not fill fields it was commissioned for",
        "unchecked" => "a document, and no grounding contract was applied — not a pass",
        "clean" => "contract applied, nothing stripped, nothing owed",
        _ => "unknown reliance token",
    }
}

/// The grounding status vocabulary, as it appears in a compiled contract's
/// `grounding` map (fermi's `output_contract.sketch.json` → card compiler).
/// A status outside this set is treated as `narrative` (the permissive
/// reading) and named in the report — never silently dropped.
const KNOWN_STATUSES: [&str; 5] = ["sourced", "inferred", "narrative", "derived", "unavailable"];

/// The outcome of grading one delegation against its output contract.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundingOutcome {
    /// The grounding report: violations (kind + path), stripped paths, and
    /// the enforced document with provenance stamps.
    pub report: Value,
    /// The completeness report: asked_for / filled / owed / no_data /
    /// excused, with the floor-not-total honesty note.
    pub completeness: Value,
    /// The one-token reliance verdict: `{status, why}`.
    pub reliance: Value,
}

/// Grade one delegation's response against the agent's output contract.
///
/// `output_contract` is the card's compiled contract (the same value
/// `contract_checks` reads). `tool_calls` is the delegation's tool-call
/// summary — empty means no tool ran. `schema_check` is the already-computed
/// `output_contract_check` (the schema half); reliance derives from it, it
/// never re-validates (one producer per verdict).
pub fn grade(
    output_contract: Option<&Value>,
    response: &str,
    tool_calls: &[Value],
    schema_check: Option<&Value>,
) -> GroundingOutcome {
    let grounding_map = output_contract
        .and_then(|contract| contract.get("grounding"))
        .and_then(Value::as_object);
    let contract_applied = grounding_map.is_some_and(|map| !map.is_empty());

    // The document: the response parsed as JSON, or the first balanced
    // JSON object embedded in prose. Without a document there is nothing
    // to grade — reliance is `unusable` and the reports say why.
    let document = extract_document(response);
    let Some(mut document) = document else {
        return GroundingOutcome {
            report: Value::Null,
            completeness: Value::Null,
            reliance: reliance_json("unusable"),
        };
    };

    // Malformed outranks everything after `unusable`: the document
    // contradicts the type the agent declared. Only a contradiction
    // counts — `unverified_*` is the common case (most cards declare no
    // schema) and reporting it as malformed would make the token useless
    // on the majority of the corpus.
    let schema_status = schema_check
        .and_then(|check| check.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("unverified_no_schema");
    if schema_status == "invalid" {
        return GroundingOutcome {
            report: Value::Null,
            completeness: Value::Null,
            reliance: reliance_json("malformed"),
        };
    }

    let tool_ran = !tool_calls.is_empty();
    let mut violations: Vec<Value> = Vec::new();
    let mut stripped_paths: Vec<String> = Vec::new();
    let mut owed: Vec<Value> = Vec::new();
    let mut no_data = 0usize;
    let mut excused = 0usize;
    let mut filled = 0usize;
    let mut unknown_statuses: Vec<String> = Vec::new();

    if let Some(map) = grounding_map {
        for (block, declaration) in map {
            let status = declaration_status(declaration);
            if !KNOWN_STATUSES.contains(&status.as_str()) {
                unknown_statuses.push(format!("{block}:{status}"));
            }
            let present = document.get(block).is_some_and(|value| !value.is_null());
            match status.as_str() {
                // The contract requires null: the source does not exist.
                // A value here is a fabrication with no possible source —
                // the one case fermi's strip exists for.
                "unavailable" => {
                    if present {
                        violations.push(violation("unavailable_field_written", block));
                        strip(&mut document, block);
                        stripped_paths.push(block.clone());
                    } else {
                        stamp(&mut document, block, "excused_null");
                        excused += 1;
                    }
                }
                // A tool must have supplied it. A value with no tool call
                // in the run is ungrounded — stripped. An empty field
                // splits by whether the tool was ever asked.
                "sourced" => {
                    if present {
                        if tool_ran {
                            stamp(&mut document, block, "tool_verified");
                            filled += 1;
                        } else {
                            violations.push(violation("ungrounded_field", block));
                            strip(&mut document, block);
                            stripped_paths.push(block.clone());
                        }
                    } else if tool_ran {
                        // Asked and empty — the world's fault, not the
                        // agent's (fermi's Lucanus cervus case).
                        stamp(&mut document, block, "no_data");
                        no_data += 1;
                    } else {
                        // A named tool never called — the agent's fault.
                        stamp(&mut document, block, "owed");
                        owed.push(owed_entry(
                            block,
                            "declared sourced but no tool was called this run",
                        ));
                    }
                }
                // Commissioned judgement: the agent was asked for this and
                // delivered nothing — owed. A value is kept and stamped as
                // the model's own (never tool_verified — the extraction
                // ceiling).
                "inferred" | "narrative" => {
                    if present {
                        stamp(&mut document, block, "model_inference");
                        filled += 1;
                    } else {
                        stamp(&mut document, block, "owed");
                        owed.push(owed_entry(block, "commissioned work absent"));
                    }
                }
                // Declared computed: empty is excused (the contract's
                // table puts derived-empty with unsourced-empty — nobody's).
                "derived" => {
                    if present {
                        stamp(&mut document, block, "derived");
                        filled += 1;
                    } else {
                        stamp(&mut document, block, "excused_null");
                        excused += 1;
                    }
                }
                // Unknown status: the permissive reading, named in the
                // report so the card author sees the typo.
                _ => {
                    if present {
                        stamp(&mut document, block, "model_inference");
                        filled += 1;
                    } else {
                        stamp(&mut document, block, "excused_null");
                        excused += 1;
                    }
                }
            }
        }
    }

    let asked_for = grounding_map.map_or(0, Map::len);
    let amended = !violations.is_empty();
    let token = if amended {
        "amended"
    } else if !owed.is_empty() {
        "incomplete"
    } else if !contract_applied {
        "unchecked"
    } else {
        "clean"
    };

    let mut report = Map::new();
    report.insert("amended".into(), Value::Bool(amended));
    report.insert(
        "stripped".into(),
        Value::Array(
            stripped_paths
                .iter()
                .map(|path| Value::String(path.clone()))
                .collect(),
        ),
    );
    report.insert("violations".into(), Value::Array(violations.clone()));
    report.insert(
        "note".into(),
        Value::String(
            "Fields with no possible source are nulled before the answer is relied on. \
             The raw response is retained verbatim on the result — it is the evidence \
             of what the model claimed."
                .to_string(),
        ),
    );
    if !unknown_statuses.is_empty() {
        report.insert(
            "unknown_statuses".into(),
            Value::Array(
                unknown_statuses
                    .iter()
                    .map(|entry| Value::String(entry.clone()))
                    .collect(),
            ),
        );
    }
    report.insert("document".into(), Value::Object(document));

    let mut completeness = Map::new();
    completeness.insert("asked_for".into(), Value::from(asked_for));
    completeness.insert("filled".into(), Value::from(filled));
    completeness.insert("owed".into(), Value::Array(owed));
    completeness.insert("no_data".into(), Value::from(no_data));
    completeness.insert("excused".into(), Value::from(excused));
    completeness.insert(
        "note".into(),
        Value::String(
            "`owed` counts only unambiguous gaps — a named tool never called, or \
             commissioned work absent. A tool that was asked and had nothing is in \
             `no_data` and is nobody's fault. The count is a floor, not a total."
                .to_string(),
        ),
    );

    GroundingOutcome {
        report: Value::Object(report),
        completeness: Value::Object(completeness),
        reliance: reliance_json(token),
    }
}

/// `{status, why}` — the one-token verdict with its gloss attached, so a
/// caller never has to look the token up.
fn reliance_json(token: &str) -> Value {
    serde_json::json!({ "status": token, "why": reliance_why(token) })
}

/// Read one block's declared status. The compiled form is
/// `{status: "sourced", from: ..., why: ...}`; a bare string is accepted
/// (hand-authored cards) and normalized to lowercase.
fn declaration_status(declaration: &Value) -> String {
    match declaration {
        Value::String(status) => status.to_lowercase(),
        Value::Object(map) => map
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_lowercase)
            .unwrap_or_else(|| "narrative".to_string()),
        _ => "narrative".to_string(),
    }
}

/// One violation: the machine-stable kind plus the path. The value the
/// model wrote is deliberately NOT carried here (fermi's rule: it is the
/// most useful thing for judging a refusal and does not belong in a
/// compact report — the raw response retains it).
fn violation(kind: &str, path: &str) -> Value {
    serde_json::json!({ "kind": kind, "path": path })
}

/// One owed entry: the path plus why it is owed.
fn owed_entry(path: &str, why: &str) -> Value {
    serde_json::json!({ "path": path, "why": why })
}

/// Null one block of the document (the strip) and stamp its provenance.
fn strip(document: &mut Map<String, Value>, block: &str) {
    document.insert(block.to_string(), Value::Null);
    stamp(document, block, "stripped");
}

/// Stamp `<block>_provenance` with the verdict for that block. The stamp
/// vocabulary is closed; a consumer can branch on it without parsing prose.
fn stamp(document: &mut Map<String, Value>, block: &str, provenance: &str) {
    document.insert(
        format!("{block}_provenance"),
        Value::String(provenance.to_string()),
    );
}

/// Extract the JSON document from a response: the whole response parsed as
/// JSON, else the first balanced JSON object embedded in prose. Returns
/// `None` when there is no document (prose only) — reliance `unusable`.
fn extract_document(response: &str) -> Option<Map<String, Value>> {
    let trimmed = response.trim();
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
        return Some(map);
    }
    // Scan for the first '{' whose matching '}' closes a parseable object.
    // Byte-level brace matching over JSON is safe for our purpose: strings
    // containing braces are the one hazard, and a failed parse simply moves
    // the scan to the next candidate — no fabricated documents.
    let bytes = trimmed.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\\' if in_string => {
                escaped = !escaped;
                continue;
            }
            b'"' if !escaped => in_string = !in_string,
            _ => escaped = false,
        }
        if in_string {
            continue;
        }
        match byte {
            b'{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(begin) = start {
                        if let Ok(Value::Object(map)) =
                            serde_json::from_str::<Value>(&trimmed[begin..=index])
                        {
                            return Some(map);
                        }
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract(grounding: Value) -> Value {
        serde_json::json!({ "grounding": grounding })
    }

    fn tool_call() -> Value {
        serde_json::json!({ "tool": "research/web_search", "ok": true })
    }

    #[test]
    fn prose_only_is_unusable() {
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"summary": {"status": "narrative"}}),
            )),
            "The market looks strong this quarter.",
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("unusable")
        );
    }

    #[test]
    fn a_document_with_no_contract_is_unchecked_not_clean() {
        // Absent must look different from bad: `unchecked` is not a pass.
        let outcome = grade(None, r#"{"summary": "hello"}"#, &[], None);
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("unchecked")
        );
    }

    #[test]
    fn a_schema_contradiction_is_malformed() {
        let schema_check = serde_json::json!({ "status": "invalid", "violations": [] });
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"summary": {"status": "narrative"}}),
            )),
            r#"{"summary": 42}"#,
            &[],
            Some(&schema_check),
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("malformed")
        );
    }

    #[test]
    fn unverified_schema_does_not_lower_reliance() {
        // Most cards declare no schema — `unverified_no_schema` must not
        // read as a fault (fermi's absent-≠-bad rule).
        let schema_check = serde_json::json!({ "status": "unverified_no_schema" });
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"summary": {"status": "narrative"}}),
            )),
            r#"{"summary": "hello"}"#,
            &[],
            Some(&schema_check),
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("clean")
        );
    }

    #[test]
    fn a_sourced_value_with_no_tool_call_is_stripped_and_amended() {
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"price": {"status": "sourced"}}),
            )),
            r#"{"price": 42.50}"#,
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("amended")
        );
        let stripped = outcome
            .report
            .get("stripped")
            .and_then(Value::as_array)
            .expect("stripped list");
        assert_eq!(stripped.len(), 1);
        // The enforced document nulls the fabricated value and stamps it.
        let document = outcome.report.get("document").unwrap();
        assert!(document.get("price").is_some_and(Value::is_null));
        assert_eq!(
            document.get("price_provenance").and_then(Value::as_str),
            Some("stripped")
        );
    }

    #[test]
    fn a_sourced_value_with_a_tool_call_is_kept_and_tool_verified() {
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"price": {"status": "sourced"}}),
            )),
            r#"{"price": 42.50}"#,
            &[tool_call()],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("clean")
        );
        let document = outcome.report.get("document").unwrap();
        assert_eq!(
            document.get("price").and_then(Value::as_f64),
            Some(42.5),
            "the sourced value travels"
        );
        assert_eq!(
            document.get("price_provenance").and_then(Value::as_str),
            Some("tool_verified")
        );
    }

    #[test]
    fn a_tool_asked_and_empty_is_nobodys_fault() {
        // fermi's Lucanus cervus case: the tool ran, had nothing, the agent
        // correctly nulled the field — no_data, never owed.
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"genome": {"status": "sourced"}}),
            )),
            r#"{"genome": null}"#,
            &[tool_call()],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("clean"),
            "no_data is excused — the run is clean"
        );
        assert_eq!(
            outcome.completeness.get("no_data").and_then(Value::as_u64),
            Some(1)
        );
        assert_eq!(
            outcome
                .completeness
                .get("owed")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn a_named_tool_never_called_is_owed_and_incomplete() {
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"genome": {"status": "sourced"}}),
            )),
            r#"{"genome": null}"#,
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("incomplete")
        );
        let owed = outcome
            .completeness
            .get("owed")
            .and_then(Value::as_array)
            .expect("owed list");
        assert_eq!(owed.len(), 1);
        assert!(owed[0].get("path").and_then(Value::as_str).unwrap() == "genome");
    }

    #[test]
    fn commissioned_work_absent_is_owed() {
        // inferred/narrative empty — the agent was commissioned for a
        // judgement and delivered nothing.
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"outlook": {"status": "inferred"}}),
            )),
            r#"{"outlook": null}"#,
            &[tool_call()],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("incomplete")
        );
    }

    #[test]
    fn derived_and_unavailable_empty_are_excused() {
        let outcome = grade(
            Some(&contract(serde_json::json!({
                "ratio": {"status": "derived"},
                "wingspan": {"status": "unavailable"}
            }))),
            r#"{"ratio": null, "wingspan": null}"#,
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("clean")
        );
        assert_eq!(
            outcome.completeness.get("excused").and_then(Value::as_u64),
            Some(2)
        );
    }

    #[test]
    fn a_value_written_where_the_contract_says_unavailable_is_stripped() {
        // The genome_profiler shape: a plausible number in a field no
        // source can supply.
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"wingspan": {"status": "unavailable"}}),
            )),
            r#"{"wingspan": 45}"#,
            &[tool_call()],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("amended")
        );
        let document = outcome.report.get("document").unwrap();
        assert!(document.get("wingspan").is_some_and(Value::is_null));
    }

    #[test]
    fn amended_outranks_incomplete() {
        // One fabricated value AND one owed field — the fabrication is the
        // worse fact about the document (fermi's argued ordering).
        let outcome = grade(
            Some(&contract(serde_json::json!({
                "price": {"status": "sourced"},
                "outlook": {"status": "inferred"}
            }))),
            r#"{"price": 42.50, "outlook": null}"#,
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("amended")
        );
    }

    #[test]
    fn a_document_embedded_in_prose_is_extracted() {
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"price": {"status": "sourced"}}),
            )),
            "Here is my analysis:\n{\"price\": 42.50}\nHope that helps!",
            &[tool_call()],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("clean"),
            "the embedded document was found and graded"
        );
    }

    #[test]
    fn braces_inside_strings_do_not_fabricate_a_document() {
        let outcome = grade(
            None,
            "The config uses {braces} in prose but is not JSON.",
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("unusable")
        );
    }

    #[test]
    fn a_bare_string_status_is_accepted() {
        // Hand-authored cards may write "price": "sourced" instead of the
        // compiled object form.
        let outcome = grade(
            Some(&contract(serde_json::json!({"price": "sourced"}))),
            r#"{"price": 42.50}"#,
            &[],
            None,
        );
        assert_eq!(
            outcome.reliance.get("status").and_then(Value::as_str),
            Some("amended"),
            "the bare-string form was read as a sourced declaration"
        );
    }

    #[test]
    fn an_unknown_status_is_named_not_dropped() {
        let outcome = grade(
            Some(&contract(
                serde_json::json!({"price": {"status": "obscure"}}),
            )),
            r#"{"price": 42.50}"#,
            &[tool_call()],
            None,
        );
        let unknown = outcome
            .report
            .get("unknown_statuses")
            .and_then(Value::as_array)
            .expect("unknown statuses named");
        assert_eq!(unknown.len(), 1);
        assert!(unknown[0].as_str().unwrap().contains("obscure"));
    }

    #[test]
    fn the_reliance_vocabulary_is_closed_and_glossed() {
        for token in RELIANCE_TOKENS {
            let why = reliance_why(token);
            assert!(!why.is_empty(), "every token carries a gloss");
        }
        assert_eq!(reliance_why("nonexistent"), "unknown reliance token");
    }

    // ── the live contract corpus ─────────────────────────────────────────
    //
    // fermi pins its cards to their sketches with a corpus test
    // (`tests/contract_sketch_corpus.rs`); the local analog pins the LIVE
    // cards to the gate. This is the evidence that "delegate to any local
    // agent and get a meaningful verdict" is true of the fleet that
    // actually exists — not just of fixtures: every card in
    // agents/local/curated carries a grounding map the gate can read, and
    // grading a compliant response yields `clean` while grading one with
    // commissioned work absent yields `incomplete`.

    /// Load the live local cards. Skips (returns empty) when the agents dir
    /// is absent — the corpus test then no-ops rather than fail in
    /// checkouts that ship without the registry.
    fn live_cards() -> Vec<crate::local_registry::LocalAgentCard> {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let dir = std::path::Path::new(manifest).join("../../../agents/local/curated");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| {
                let path = entry.ok()?.path().join("agent_card.json");
                let text = std::fs::read_to_string(path).ok()?;
                serde_json::from_str(&text).ok()
            })
            .collect()
    }

    #[test]
    fn every_live_card_carries_a_grounding_map_the_gate_can_read() {
        let cards = live_cards();
        if cards.is_empty() {
            return; // registry not shipped in this checkout
        }
        assert!(!cards.is_empty(), "the local registry ships cards");
        for card in &cards {
            let contract = card
                .capabilities
                .output_contract
                .as_ref()
                .unwrap_or_else(|| {
                    panic!(
                        "live card '{}' declares no output_contract — its delegations \
                         can only ever return reliance 'unchecked'",
                        card.agent_id
                    )
                });
            let grounding = contract
                .get("grounding")
                .and_then(Value::as_object)
                .unwrap_or_else(|| {
                    panic!(
                        "live card '{}' has an output_contract with no grounding map",
                        card.agent_id
                    )
                });
            assert!(
                !grounding.is_empty(),
                "{}: empty grounding map",
                card.agent_id
            );
            for (block, declaration) in grounding {
                let status = declaration_status(declaration);
                assert!(
                    KNOWN_STATUSES.contains(&status.as_str()),
                    "live card '{}' block '{}' declares unknown status '{status}' — \
                     the gate would treat it as narrative and name it in every report",
                    card.agent_id,
                    block
                );
            }
        }
    }

    #[test]
    fn grading_a_live_card_response_distinguishes_work_done_from_work_owed() {
        let cards = live_cards();
        if cards.is_empty() {
            return; // registry not shipped in this checkout
        }
        for card in &cards {
            let contract = card
                .capabilities
                .output_contract
                .as_ref()
                .expect("checked above");
            let grounding = contract
                .get("grounding")
                .and_then(Value::as_object)
                .expect("checked above");
            // A compliant response: every contracted block present.
            let compliant: Map<String, Value> = grounding
                .keys()
                .map(|block| (block.clone(), Value::String("content".to_string())))
                .collect();
            let compliant_text = Value::Object(compliant).to_string();
            let outcome = grade(Some(contract), &compliant_text, &[], None);
            assert_eq!(
                outcome.reliance.get("status").and_then(Value::as_str),
                Some("clean"),
                "{}: a compliant response must grade clean",
                card.agent_id
            );
            // Commissioned work absent: every contracted block missing. Only
            // asserted for cards that commission work (inferred/narrative
            // blocks) — a derived/unavailable-only contract legitimately
            // grades clean on empty (the contract requires null).
            let commissions_work = grounding.values().any(|declaration| {
                matches!(
                    declaration_status(declaration).as_str(),
                    "inferred" | "narrative"
                )
            });
            if !commissions_work {
                continue;
            }
            let shirked_text = "{}";
            let outcome = grade(Some(contract), shirked_text, &[], None);
            assert_eq!(
                outcome.reliance.get("status").and_then(Value::as_str),
                Some("incomplete"),
                "{}: a response with every commissioned block absent must grade incomplete",
                card.agent_id
            );
        }
    }
}
