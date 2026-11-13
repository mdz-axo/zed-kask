//! Regression pin for the direct-model-edit gates.
//!
//! Two halves are pinned together, because a gate's verdict depends on both:
//! `dme-measure.jq` (measures harness files into an env) and the `dme-*.lisp`
//! forms (decide over that env). Every declared case is built here, at test
//! time, by running `jq` over `dme-gate-cases.gen.jq`, which calls the same
//! builders the skill names; the result is evaluated through the interpreter
//! entry point and budget the skill tells callers to use. `jq` is required:
//! a missing `jq` fails the test, it does not skip it.

use serde_json::Value;
use std::path::PathBuf;

const MAX_STEPS: u64 = 2_000_000;
const MAX_DEPTH: u64 = 1024;
const GATES: [&str; 5] = ["dme-0", "dme-2b", "dme-2c", "dme-2d", "dme-3"];

fn gates_dir() -> PathBuf {
    match std::env::var_os("DME_GATES_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dme-harness/gates"),
    }
}

fn skill_md() -> PathBuf {
    match std::env::var_os("DME_SKILL_MD") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../.agents/skills/direct-model-edit/SKILL.md"),
    }
}

fn read(name: &str) -> String {
    let path = gates_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The inline route form in SKILL.md whose text contains `needle`.
fn inline_form(needle: &str) -> String {
    let text = std::fs::read_to_string(skill_md()).expect("read SKILL.md");
    let forms: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("- form: `"))
        .filter_map(|rest| rest.trim_end().strip_suffix('`'))
        .filter(|form| form.contains(needle))
        .collect();
    assert_eq!(
        forms.len(),
        1,
        "expected one inline form containing {needle:?}"
    );
    forms[0].to_string()
}

fn form_for(gate: &str) -> String {
    match gate {
        "dme-1" => inline_form("blocker-removal-route"),
        "dme-1s" => inline_form("skill-unlock-route"),
        _ => read(&format!("{gate}.lisp")),
    }
}

fn declared_cases() -> Vec<Value> {
    let dir = gates_dir();
    let output = smol::block_on(
        smol::process::Command::new("jq")
            .args(["-c", "-n", "-L"])
            .arg(&dir)
            .arg("--slurpfile")
            .arg("fx")
            .arg(dir.join("dme-gate-fixture.json"))
            .arg("-f")
            .arg(dir.join("dme-gate-cases.gen.jq"))
            .output(),
    )
    .expect("jq must be installed: the gates' measuring half is a jq module");
    assert!(
        output.status.success(),
        "jq failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("the generator prints one JSON array")
}

fn eval(form: &str, env: &Value) -> Value {
    hkask_lisp::eval_sandboxed_with_budget(form, env, MAX_STEPS, MAX_DEPTH)
        .unwrap_or_else(|error| Value::String(format!("EVALUATOR-ERROR: {error}")))
}

fn head(value: &Value) -> &Value {
    match value {
        Value::Array(items) if !items.is_empty() => &items[0],
        other => other,
    }
}

/// `head`: the verdict symbol; `equals`: the whole result; `contains`: every
/// string occurs in the rendered result; `at`: `[index, value]` pairs.
fn meets(got: &Value, expect: &Value) -> bool {
    let text = got.to_string();
    expect.get("head").is_none_or(|want| head(got) == want)
        && expect.get("equals").is_none_or(|want| got == want)
        && expect
            .get("contains")
            .and_then(Value::as_array)
            .is_none_or(|needles| {
                needles
                    .iter()
                    .all(|n| n.as_str().is_some_and(|n| text.contains(n)))
            })
        && expect
            .get("at")
            .and_then(Value::as_array)
            .is_none_or(|pairs| {
                pairs.iter().all(|pair| {
                    pair[0].as_u64().and_then(|i| got.get(i as usize)) == Some(&pair[1])
                })
            })
}

#[test]
fn every_declared_case_returns_its_declared_verdict() {
    let cases = declared_cases();
    let mut failures = Vec::new();
    for case in &cases {
        let id = case["id"].as_str().expect("case id");
        let gate = case["gate"].as_str().expect("case gate");
        let got = eval(&form_for(gate), &case["env"]);
        if !meets(&got, &case["expect"]) {
            failures.push(format!(
                "{id} ({gate}): got {got}, declared {}",
                case["expect"]
            ));
        }
    }
    assert!(
        cases.len() >= 200,
        "expected at least 200 declared cases, found {}",
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// The gates' sign test in exact integer arithmetic: two-sided p < 0.05 over
/// `n = b + c` answers that changed class, `k = min(b, c)`:
/// `40 * sum_{j <= k} C(n, j) < 2^n`.
fn exact_sign_test(b: u32, c: u32) -> bool {
    let (n, k) = (b + c, b.min(c));
    if n == 0 {
        return false;
    }
    let mut term: u128 = 1;
    let mut tail: u128 = 0;
    for j in 0..=k {
        tail += term;
        term = term * u128::from(n - j) / u128::from(j + 1);
    }
    40 * tail < 1u128 << n
}

#[test]
fn the_float_sign_test_agrees_with_exact_arithmetic_up_to_100_changed_answers() {
    let gate = read("dme-3.lisp");
    let defs: Vec<&str> = gate
        .lines()
        .filter(|line| {
            [" (define hp ", " (define tl ", " (define sig "]
                .iter()
                .any(|p| line.starts_with(p))
        })
        .collect();
    assert_eq!(
        defs.len(),
        3,
        "the three sign-test definitions are one line each"
    );
    let form = format!("(begin {} (sig b c))", defs.join("\n"));
    for b in 0..=100u32 {
        for c in 0..=(100 - b) {
            let got = eval(&form, &serde_json::json!({ "b": b, "c": c }));
            assert_eq!(got, Value::Bool(exact_sign_test(b, c)), "b={b} c={c}");
        }
    }
}

/// The verdict table proved in `kask/lean/dme_gates.lean` (`attributeGain`):
/// signs -1 (established loss), 0 (nothing established), 1 (established
/// gain); `band` = the finished-answer net change is within 1.0 point.
fn lean_attribute_gain(net: i64, fin: i64, unb: i64, band: bool) -> &'static str {
    match (net, fin, unb, band) {
        (0, _, _, _) => "net-change-not-established",
        (-1, _, _, _) => "net-loss-established",
        (1, 1, _, _) => "gain-includes-finished-answers",
        (1, -1, 1, _) => "unblocking-with-loss-on-finished-answers",
        (1, 0, 1, true) => "mostly-unblocking",
        (1, 0, 1, false) => "unblocking-with-unresolved-change-on-finished-answers",
        _ => "net-gain-source-unresolved",
    }
}

#[test]
fn the_attribution_rule_in_the_gate_file_is_the_table_the_lean_pin_proves() {
    let gate = read("dme-3.lisp");
    let start = gate
        .find("(cond ((= net 0)")
        .expect("the attribution cond starts as pinned");
    let last = "(t (quote unblocking-with-unresolved-change-on-finished-answers)))";
    let end = start
        + gate[start..]
            .find(last)
            .expect("the attribution cond ends as pinned")
        + last.len();
    let form = format!(
        "(let ((net n) (fin f) (unb u) (band k)) {})",
        &gate[start..end]
    );
    for net in -1..=1i64 {
        for fin in -1..=1i64 {
            for unb in -1..=1i64 {
                for band in [false, true] {
                    let got = eval(
                        &form,
                        &serde_json::json!({ "n": net, "f": fin, "u": unb, "k": band }),
                    );
                    let want = lean_attribute_gain(net, fin, unb, band);
                    assert_eq!(
                        got,
                        Value::String(want.into()),
                        "net={net} fin={fin} unb={unb} band={band}"
                    );
                }
            }
        }
    }
}

/// The gate files repeat their shared helper definitions so that each is one
/// self-contained `lisp_eval` form. A helper defined in more than one file
/// must be the same text everywhere. A definition runs from its ` (define `
/// line to the next one, or to the gate's closing `cond`.
#[test]
fn shared_definitions_are_identical_across_gate_files() {
    let mut seen: std::collections::BTreeMap<String, (String, String)> = Default::default();
    let mut shared = 0usize;
    for gate in GATES {
        let text = read(&format!("{gate}.lisp"));
        let mut blocks: Vec<String> = Vec::new();
        for line in text.lines() {
            if line.starts_with(" (define ") {
                blocks.push(line.to_string());
            } else if line.starts_with(" (cond ((not (objp in))") {
                break;
            } else if let Some(block) = blocks.last_mut() {
                block.push('\n');
                block.push_str(line);
            }
        }
        for block in blocks {
            let name = block[" (define ".len()..]
                .split_whitespace()
                .next()
                .expect("definition name")
                .to_string();
            match seen.get(&name) {
                Some((first_gate, first_block)) => {
                    assert_eq!(
                        first_block, &block,
                        "`{name}` differs between {first_gate}.lisp and {gate}.lisp"
                    );
                    shared += 1;
                }
                None => {
                    seen.insert(name, (gate.to_string(), block));
                }
            }
        }
    }
    assert!(
        shared >= 40,
        "expected the shared helpers to repeat across files, found {shared} repeats"
    );
}

fn lean_dir() -> PathBuf {
    match std::env::var_os("DME_LEAN_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lean"),
    }
}

/// `lean_attribute_gain` above is a copy of the Lean definition's match arms.
/// This keeps the copy honest: the pin must contain exactly these arms.
#[test]
fn the_rust_copy_of_the_lean_table_is_the_lean_table() {
    let pin =
        std::fs::read_to_string(lean_dir().join("dme_gates.lean")).expect("read dme_gates.lean");
    let arms = "  match net, fin, unb, band with
  | .flat, _, _, _ => .netChangeNotEstablished
  | .down, _, _, _ => .netLossEstablished
  | .up, .up, _, _ => .gainIncludesFinishedAnswers
  | .up, .down, .up, _ => .unblockingWithLossOnFinishedAnswers
  | .up, .flat, .up, true => .mostlyUnblocking
  | .up, .flat, .up, false => .unblockingWithUnresolvedChangeOnFinishedAnswers
  | .up, _, _, _ => .netGainSourceUnresolved
";
    assert!(
        pin.contains(arms),
        "attributeGain's match arms changed; update lean_attribute_gain with them"
    );
}

/// The negative control is checked as a file on its own, so it repeats the
/// definitions it makes false claims about. Each repeated definition must be
/// the pin's text, or the control stops being about the pin.
#[test]
fn the_negative_control_repeats_the_pin_verbatim() {
    let dir = lean_dir();
    let pin = std::fs::read_to_string(dir.join("dme_gates.lean")).expect("read dme_gates.lean");
    let control = std::fs::read_to_string(dir.join("dme_gates_negative_control.lean"))
        .expect("read the negative control");
    let body = control
        .split_once("-/\n")
        .expect("the control opens with a doc comment")
        .1;
    let definitions: Vec<&str> = body
        .split("\n\n")
        .map(str::trim)
        .filter(|block| block.starts_with("def ") || block.starts_with("inductive "))
        .collect();
    assert_eq!(definitions.len(), 4, "the control repeats four definitions");
    for definition in definitions {
        assert!(
            pin.contains(definition),
            "not in the pin verbatim:\n{definition}"
        );
    }
    assert_eq!(
        body.matches("\nexample : ").count(),
        4,
        "the control makes four false claims"
    );
}
