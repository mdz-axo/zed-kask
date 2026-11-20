use super::*;
use hkask_lisp::eval_sandboxed;
use serde_json::json;

fn skill_candidate(skill: &str, needle: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../.agents/skills")
        .join(skill)
        .join("SKILL.md");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let matches = extract_candidates(&content)
        .into_iter()
        .filter(|candidate| candidate.source.contains(needle))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "expected one {skill} form containing {needle:?}, got {}",
        matches.len()
    );
    matches
        .into_iter()
        .next()
        .expect("one match checked")
        .source
}

#[test]
fn extracts_inline_fenced_and_quoted_forms() {
    let content = r#"Use `lisp_eval` with `(and ok (= count 1))`.

```lisp
(begin
  (list "(")
  (list ")"))
```
form: "(= (+ a b) 3)"
"#;
    let candidates = extract_candidates(content);
    assert_eq!(candidates.len(), 3);
    for candidate in candidates {
        parse(&candidate.source).expect("extracted candidate should parse");
    }
}

#[test]
fn semantic_lints_catch_only_certain_failures() {
    let forms = parse(
        "(begin (not (is_null (member x xs))) (= verdict \"pass\") (or (string= min_flip \"none\") (> min_flip 0.05)))",
    )
    .expect("fixture should parse");
    let mut messages = Vec::new();
    for form in &forms {
        lint_form(form, &mut |message| messages.push(message));
    }
    assert_eq!(messages.len(), 3);
}

#[test]
fn alist_key_lint_flags_mixed_keys_only() {
    let forms = parse(
        "(begin (list (list \"k\" 1) (list \"j\" 2)) (list (list \"k\" 1) (list 2 \"two\")) (list (list 1 \"one\") (list 2 \"two\")) (list (list k 1) (list \"j\" 2)))",
    )
    .expect("fixture should parse");
    let mut messages = Vec::new();
    for form in &forms {
        lint_form(form, &mut |message| messages.push(message));
    }
    assert_eq!(
        messages.len(),
        1,
        "only the mixed-key form should be flagged: {messages:?}"
    );
    assert!(
        messages[0].contains("mixed alist keys"),
        "flag should name the mixed-key failure: {messages:?}"
    );
}

/// The `PS-1` modifier values are stated in prose in the skill body and in
/// three companion templates. That prose drifted once already (domain +0.10 /
/// SUMO +0.05 / unanchored -0.15 survived a gate edit to +0.05 / +0.03 /
/// -0.10), so the documented values are pinned to the *gate's own* values
/// rather than to literals: each modifier is derived by evaluating the live
/// form with exactly one flag set, and the prose must contain the derived
/// spelling. A gate edit that is not carried into the prose fails here.
#[test]
fn pragmatic_semantics_modifier_prose_matches_the_ps1_gate() {
    let gate = skill_candidate("pragmatic-semantics", "domain_anchored");

    let derive = |flag: &str| -> f64 {
        // A base of 0.5 keeps every branch clear of both clamps, so the
        // derived value is the modifier itself.
        let mut env = json!({
            "base_confidence": 0.5,
            "provenance": "runtime",
            "domain_anchored": false,
            "sumo_anchored": false,
            "spec_checked": false,
            "unanchored": false
        });
        env[flag] = json!(true);
        eval_sandboxed(&gate, &env)
            .expect("PS-1 derives a single-flag modifier")
            .as_f64()
            .expect("PS-1 returns a number")
            - 0.5
    };

    // Non-vacuity control: a mutated gate yields a different derived value,
    // so the prose assertions below are bound to the real gate and cannot
    // pass against a stale one.
    let mutated = gate.replace("domain_anchored 0.05", "domain_anchored 0.10");
    assert_ne!(mutated, gate, "the control must actually mutate the gate");
    let mutated_domain = eval_sandboxed(
        &mutated,
        &json!({
            "base_confidence": 0.5,
            "provenance": "runtime",
            "domain_anchored": true,
            "sumo_anchored": false,
            "spec_checked": false,
            "unanchored": false
        }),
    )
    .expect("mutated PS-1 evaluates")
    .as_f64()
    .expect("mutated PS-1 returns a number")
        - 0.5;

    let domain = derive("domain_anchored");
    let sumo = derive("sumo_anchored");
    let spec = derive("spec_checked");
    let unanchored = derive("unanchored");
    assert_ne!(
        mutated_domain, domain,
        "the control must be discriminating: a mutated gate must derive a different modifier"
    );

    // The positive modifiers are the ones that drifted; the unanchored
    // penalty is checked on its magnitude because the templates use the
    // typographic minus (U+2212) and the skill body the ASCII hyphen.
    let positive = |value: f64| format!("+{value:.2}");
    let domain_text = positive(domain);
    let sumo_text = positive(sumo);
    let spec_text = positive(spec);
    assert!(unanchored < 0.0, "the unanchored modifier is a penalty");
    let unanchored_magnitude = format!("{:.2}", unanchored.abs());

    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let read = |relative: &str| {
        let path = repo.join(relative);
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
    };

    let body = read(".agents/skills/pragmatic-semantics/SKILL.md");
    // The carrier names the flag as the gate's env key does (`domain_anchored`),
    // so a flag rename must be carried into both the gate and this spelling.
    let expected_constraint_line = format!(
        "domain_anchored {domain_text}, SUMO {sumo_text}, spec_checked {spec_text}, unanchored -{unanchored_magnitude}"
    );
    assert!(
        body.contains(&expected_constraint_line),
        "SKILL.md must state the gate's own modifiers ({expected_constraint_line}); \
         the prose has drifted from the gate"
    );

    let conflict =
        read("kask/registry/templates/pragmatic-semantics/semantics-conflict-resolve.j2");
    assert!(
        conflict.contains(&format!("the `PS-1` modifier is {domain_text}")),
        "semantics-conflict-resolve.j2 must carry the gate's domain modifier ({domain_text})"
    );
    assert!(
        conflict.contains(&sumo_text),
        "semantics-conflict-resolve.j2 must carry the gate's SUMO modifier ({sumo_text})"
    );

    let provenance =
        read("kask/registry/templates/pragmatic-semantics/semantics-provenance-trace.j2");
    assert!(
        provenance.contains(&format!(
            "domain supplement ({domain_text}), SUMO ({sumo_text})"
        )),
        "semantics-provenance-trace.j2 must carry the gate's modifiers \
         (domain supplement {domain_text}, SUMO {sumo_text})"
    );

    let classify =
        read("kask/registry/templates/pragmatic-semantics/semantics-classify-statement.j2");
    // The domain-supplement modifier is stated at the rung level (the
    // template's Constraints prose), not as a per-namespace table row: the
    // rung carries the +0.05, so no single supplement owns it.
    assert!(
        classify.contains(&format!("domain-supplement ({domain_text})")),
        "semantics-classify-statement.j2 must carry the gate's domain modifier ({domain_text})"
    );
    assert!(
        classify.contains(&format!("| **SUMO** (upper ontology) | {sumo_text} |")),
        "semantics-classify-statement.j2 must carry the gate's SUMO modifier ({sumo_text})"
    );
}

/// Split a pinned `env:` schema block into (key, type-token) pairs.
///
/// The blocks are schema notation, not JSON (`{"x": <int>, "y": "<string>"}`),
/// so keys are read by scanning at brace depth 1 for a quoted token
/// followed by `:`.
fn env_schema_entries(env: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = env.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut depth = 0i32;
    while i < chars.len() {
        match chars[i] {
            '{' | '[' => {
                depth += 1;
                i += 1;
            }
            '}' | ']' => {
                depth -= 1;
                i += 1;
            }
            '"' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != '"' {
                    j += 1;
                }
                let key: String = chars[start..j.min(chars.len())].iter().collect();
                let after_quote = j + 1;
                let mut k = after_quote;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                if depth == 1 && k < chars.len() && chars[k] == ':' {
                    let mut t = k + 1;
                    let mut nested = 0i32;
                    while t < chars.len() {
                        let c = chars[t];
                        if c == '{' || c == '[' {
                            nested += 1;
                        }
                        if c == '}' || c == ']' {
                            if nested == 0 {
                                break;
                            }
                            nested -= 1;
                        }
                        if c == ',' && nested == 0 {
                            break;
                        }
                        t += 1;
                    }
                    let token: String = chars[k + 1..t].iter().collect();
                    out.push((key, token.trim().to_string()));
                }
                i = after_quote;
            }
            _ => i += 1,
        }
    }
    out
}

/// The env extractor is the instrument's own weak point: a mis-parsed
/// block reports declared keys as unbound (a false positive) or reads a
/// neighbouring block's keys (a false negative). Pin it against the three
/// marker spellings and a multi-line block that the corpus actually uses.
#[test]
fn env_extraction_handles_the_corpus_spellings() {
    let keys = |env: &str| {
        env_schema_entries(env)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        keys(r#"{ "corrected_count": <int>, "min_why_len": <int> }"#),
        vec!["corrected_count", "min_why_len"]
    );
    // A type token that is itself a bracketed structure must not leak its
    // inner keys into the top-level set.
    assert_eq!(
        keys(r#"{ "findings": [{"id": "x", "severity": "y"}], "total": <int> }"#),
        vec!["findings", "total"]
    );

    // The marker spellings: `env:`, `envs:`, `env (note):`.
    for marker in [
        "  - env: `{ \"a\": <int> }`\n",
        "  - envs: `{ \"a\": <int> }`\n",
        "  - env (all counts are this cycle's): `{ \"a\": <int> }`\n",
    ] {
        let body = format!("- form: `(and (<= a 1))`\n{marker}");
        let pinned = pinned_gate_forms(&body);
        assert_eq!(pinned.len(), 1, "one form expected for marker {marker:?}");
        assert_eq!(keys(&pinned[0].1), vec!["a"], "marker {marker:?}");
    }

    // A wrapped block keeps every key.
    let wrapped =
        "- form: `(and (<= a 1))`\n  - env: `{ \"a\": <int>,\n            \"b\": <int> }`\n";
    let pinned = pinned_gate_forms(wrapped);
    assert_eq!(keys(&pinned[0].1), vec!["a", "b"]);

    // A form with no env block yields an empty set, not a neighbour's.
    let no_env = "- form: `(and (<= a 1))`\n\nSome prose.\n";
    let pinned = pinned_gate_forms(no_env);
    assert_eq!(pinned.len(), 1);
    assert!(keys(&pinned[0].1).is_empty());
}

/// A stub value of the declared shape, so evaluation reaches every symbol
/// in the form instead of aborting on the first type error.
fn env_stub(token: &str) -> serde_json::Value {
    let lowered = token.to_ascii_lowercase();
    if token.starts_with('[') {
        json!([])
    } else if token.starts_with('{') {
        json!({})
    } else if lowered.contains("bool") {
        json!(true)
    } else if lowered.contains("int")
        || lowered.contains("count")
        || lowered.contains("number")
        || lowered.contains("float")
        || lowered.contains("ratio")
        || lowered.contains("confidence")
        || token.chars().any(|c| c.is_ascii_digit())
    {
        json!(1)
    } else if lowered.contains("array") || lowered.contains("list") {
        json!([])
    } else if lowered.contains("object") {
        json!({})
    } else {
        json!("")
    }
}

/// Every (form, env) pair pinned in a SKILL.md body, in file order.
fn pinned_gate_forms(content: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.split("- form: `").nth(1) else {
            continue;
        };
        let Some(form) = rest.split('`').next() else {
            continue;
        };
        // The env schema block may wrap across lines, and the marker
        // varies (`env: \``, `envs: \``, `env (note): \``), so locate an
        // `env` token followed within a short span by `:` then a
        // backtick, then gather until the closing backtick. A truncated
        // block reports declared keys as unbound — the extractor's own
        // false positive.
        let mut env = String::new();
        let mut joined = String::new();
        let mut collecting = false;
        for candidate in lines.iter().skip(i + 1).take(8) {
            if !collecting {
                let Some(at) = candidate.find("env") else {
                    continue;
                };
                let tail = &candidate[at..];
                let Some(colon) = tail.find(':') else {
                    continue;
                };
                let after_colon = &tail[colon + 1..];
                let Some(tick) = after_colon.find('`') else {
                    continue;
                };
                // A backtick before the colon is a different construction.
                if tail[..colon].contains('`') {
                    continue;
                }
                let rest = &after_colon[tick + 1..];
                joined.push_str(rest);
                if rest.contains('`') {
                    env = rest.split('`').next().unwrap_or_default().to_string();
                    break;
                }
                collecting = true;
                continue;
            }
            joined.push(' ');
            joined.push_str(candidate);
            if candidate.contains('`') {
                env = joined.split('`').next().unwrap_or_default().to_string();
                break;
            }
        }
        if env.is_empty() && collecting {
            env = joined;
        }
        out.push((form.to_string(), env));
    }
    out
}

/// Walk a skill corpus and return (forms checked, symbol findings).
fn pinned_symbol_findings(skills_dir: &Path) -> (usize, Vec<String>) {
    let mut files = Vec::new();
    collect_files(skills_dir, CorpusKind::Skills, &mut files).expect("walk skill corpus");
    files.sort();
    let mut findings: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for path in &files {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (form, env) in pinned_gate_forms(&content) {
            checked += 1;
            let mut env_object = serde_json::Map::new();
            for (key, token) in env_schema_entries(&env) {
                env_object.insert(key, env_stub(&token));
            }
            if let Err(error) = eval_sandboxed(&form, &serde_json::Value::Object(env_object)) {
                let message = error.to_string();
                if message.contains("unbound symbol") {
                    findings.push(format!("{}: {message} in {form}", display_path(path)));
                }
            }
        }
    }
    (checked, findings)
}

/// S12: a pinned gate form must resolve every symbol it references —
/// against the interpreter's builtins, its own definitions, or its
/// declared env keys. A form naming a builtin that does not exist (a typo,
/// or a builtin removed from the surface) raises `unbound symbol` and is
/// dead at use time; the parser-only gate cannot see it.
///
/// Non-vacuity control: the same walk over a fixture skill whose form
/// references a name no env declares must produce a finding, so a clean
/// corpus result is not simply a walk that fails to evaluate.
#[test]
fn pinned_gate_forms_resolve_every_symbol() {
    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.agents/skills");
    let (checked, findings) = pinned_symbol_findings(&skills_dir);
    assert!(
        checked > 40,
        "expected the corpus to pin many forms, saw {checked}"
    );
    assert!(
        findings.is_empty(),
        "pinned gate forms reference symbols nothing binds:\n{}",
        findings.join("\n")
    );

    // Non-vacuity control: a fixture skill with one undeclared symbol and
    // one resolvable form must report exactly the undeclared one.
    let fixture =
        std::env::temp_dir().join(format!("hkask-lisp-s12-control-{}", std::process::id()));
    let fixture_skill = fixture.join("control-skill");
    fs::create_dir_all(&fixture_skill).expect("create fixture skill dir");
    fs::write(
        fixture_skill.join("SKILL.md"),
        "# Control\n\n- form: `(and (<= rounds_executed 3) (<= no_such_symbol 1))`\n  - env: `{ \"rounds_executed\": <int> }`\n\n- form: `(and (<= rounds_executed 3) (>= rounds_executed 0))`\n  - env: `{ \"rounds_executed\": <int> }`\n",
    )
    .expect("write fixture skill");
    let (control_checked, control_findings) = pinned_symbol_findings(&fixture);
    fs::remove_dir_all(&fixture).ok();
    assert_eq!(
        control_checked, 2,
        "the control must evaluate both fixture forms"
    );
    assert_eq!(
        control_findings.len(),
        1,
        "the control must report exactly the undeclared symbol, got: {control_findings:?}"
    );
    assert!(
        control_findings[0].contains("no_such_symbol"),
        "the control must name the undeclared symbol, got: {control_findings:?}"
    );
}

#[test]
fn prompt_enhance_state_contract_uses_authored_form() {
    smol::block_on(async {
        let form = skill_candidate("prompt-enhance", "define pe-get");
        let skill = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../.agents/skills/prompt-enhance/SKILL.md"),
        )
        .expect("prompt-enhance SKILL.md");
        let raw_filter = skill
            .split_once("### Raw JSON preflight")
            .and_then(|(_, tail)| tail.split_once("```jq\n"))
            .and_then(|(_, tail)| tail.split_once("\n```"))
            .map(|(filter, _)| filter.to_string())
            .expect("the preflight section carries the raw-shape filter");
        assert!(
            !PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../.agents/skills/prompt-enhance/raw-shape.jq")
                .exists(),
            "the filter has one copy, in SKILL.md"
        );
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/prompt_enhance_state.json"
        ))
        .expect("NEW-data PE fixtures are valid JSON");
        let cases = cases.as_array().expect("PE fixture array");
        assert!(!cases.is_empty(), "PE gate must not run zero cases");
        let mut refusing = 0;
        for case in cases {
            let id = case["id"].as_str().expect("fixture identity");
            let env: serde_json::Value = serde_json::from_str(
                case["env"]
                    .as_str()
                    .expect("tool-boundary env is JSON text"),
            )
            .expect("fixture environment JSON");
            assert!(env.is_object(), "{id}: environment must be an object");
            // The Lisp bridge loses null/empty-array/empty-object distinctions.
            // Exercise the SAME authored raw filter before the authored Lisp form.
            // The filter's only copy is the jq fence in the SKILL.md preflight
            // section: installed builds receive SKILL.md alone, so a companion
            // file in the skill folder would not reach them.
            let mut child = smol::process::Command::new("jq")
                .arg("-e")
                .arg(&raw_filter)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("jq is required for the authored raw JSON contract test");
            use smol::io::AsyncWriteExt;
            child
                .stdin
                .take()
                .expect("jq input pipe")
                .write_all(case["env"].as_str().expect("JSON text").as_bytes())
                .await
                .expect("write raw fixture JSON to jq");
            let shape = child.output().await.expect("raw-shape gate exits");
            if !shape.status.success() {
                assert_eq!(
                    shape.status.code(),
                    Some(1),
                    "{id}: jq execution error: {}",
                    String::from_utf8_lossy(&shape.stderr)
                );
                assert_eq!(
                    case["expect_error"],
                    json!(true),
                    "{id}: unexpected raw-shape refusal"
                );
                if let Some(stage) = case.get("expect_stage") {
                    assert_eq!(stage, "raw", "{id}: wrong refusal boundary");
                }
                refusing += 1;
                continue;
            }
            // The 89 raw-valid, lisp-evaluable cases are pinned by the
            // lisp-receipts fence beside the form, over the sibling
            // pe-state-receipts.jsonl: the
            // walker evaluates each case's env over the authored form and
            // compares the full result (both admission directions). What
            // the receipts format cannot express stays here — the raw-shape
            // filter itself (the authored jq seam) and the interpreter-error
            // refusals: an eval failure is a walker defect by design, so a
            // refusing case can be no receipt.
            if case["expect_error"] == json!(true) {
                let result = eval_sandboxed(&form, &env);
                assert!(result.is_err(), "{id}: malformed required data must refuse");
                if let Some(stage) = case.get("expect_stage") {
                    assert_eq!(stage, "lisp", "{id}: raw refusal was not enforced");
                }

                refusing += 1;
            }
        }
        assert!(refusing > 0, "the raw-shape refusals are pinned");
    });
}

#[test]
fn prompt_enhance_state_contract_boundary_probes() {
    let form = skill_candidate("prompt-enhance", "define pe-get");
    let (prefix, _) = form
        .split_once("(define pe-input")
        .expect("authored PE helper definitions precede state evaluation");
    let threshold = r#"(list
        (list "ratings_valid" (pe-ratings ratings 3))
        (list "escalation_threshold" (and (pe-ratings ratings 3) (>= (* 5 (pe-solid ratings)) (* 4 (length ratings)))))
        (list "reprobe_threshold" (and (pe-ratings ratings 3) (< (* 5 (pe-solid ratings)) (* 2 (length ratings)))))))"#;
    let transition = r#"(list (list "admit" (pe-transition critique_runs request))))"#;
    // These expressions are extracted from the same authored helper scope;
    // exact80/exact40 are helper tests, not fabricated reachable round histories.
    for needle in [
        "(>= (* 5 (pe-solid rs)) (* 4 (length rs)))",
        "(< (* 5 (pe-solid rs)) (* 2 (length rs)))",
    ] {
        assert!(form.contains(needle), "threshold expression drift");
    }
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/prompt_enhance_probes.json"
    ))
    .expect("NEW boundary probe fixtures");
    let cases = cases.as_array().expect("probe array");
    assert!(!cases.is_empty(), "nonzero boundary probes");
    for case in cases {
        let id = case["id"].as_str().expect("probe identity");
        let env: serde_json::Value =
            serde_json::from_str(case["env"].as_str().expect("JSON-text env"))
                .expect("probe environment");
        let suffix = match case["probe"].as_str().expect("probe kind") {
            "threshold" => threshold,
            "transition" => transition,
            other => panic!("unsupported probe kind {other}"),
        };
        let actual = eval_sandboxed(&format!("{prefix}{suffix}"), &env)
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        for (key, expected) in case["expect"].as_object().expect("probe expectations") {
            assert_eq!(&actual[key], expected, "{id}: {key}");
        }
    }
}

#[test]
fn guarded_union_and_assoc_null_check_are_accepted() {
    let forms = parse(
        "(and (is_null (assoc \"optional\" row)) (or (and (stringp x) (string= x \"none\")) (and (numberp x) (> x 0.05))))",
    )
    .expect("fixture should parse");
    let mut messages = Vec::new();
    for form in &forms {
        lint_form(form, &mut |message| messages.push(message));
    }
    assert!(messages.is_empty(), "unexpected messages: {messages:?}");
}

fn temp_form(name: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "check-skill-lisp-forms-eval-{name}-{}.lisp",
        std::process::id()
    ));
    fs::write(&path, content)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
    path
}

#[test]
fn eval_form_file_prints_the_env_bound_result() {
    let form = temp_form("ok", "(+ a 2)");
    assert_eq!(
        eval_form_file(&form, OsStr::new(r#"{"a":1}"#)),
        EvalOutcome::Ok(json!(3))
    );
}

#[test]
fn eval_form_file_rejects_malformed_env_as_env_parse() {
    let form = temp_form("env-parse", "(+ a 2)");
    for env in ["{bad", "{\"a\":", "3", "\"text\"", "[1]"] {
        assert!(
            matches!(
                eval_form_file(&form, OsStr::new(env)),
                EvalOutcome::EnvParse(_)
            ),
            "expected ENV_PARSE_ERROR for env {env:?}"
        );
    }
}

#[test]
fn eval_form_file_carries_engine_errors_for_parse_and_runtime_failures() {
    let runtime = temp_form("runtime", "(car 1)");
    match eval_form_file(&runtime, OsStr::new("{}")) {
        EvalOutcome::Failed(message) => assert!(
            message.contains("expected list"),
            "the engine's type error is carried: {message}"
        ),
        outcome => panic!("expected a runtime failure for (car 1), got {outcome:?}"),
    }
    let unparseable = temp_form("unparseable", "(unclosed");
    match eval_form_file(&unparseable, OsStr::new("{}")) {
        EvalOutcome::Failed(message) => {
            assert!(!message.is_empty(), "the engine's parse error is carried")
        }
        outcome => panic!("expected a parse failure for (unclosed, got {outcome:?}"),
    }
}

#[test]
fn eval_form_file_names_a_missing_form_file() {
    let missing = std::env::temp_dir().join("check-skill-lisp-forms-eval-missing.lisp");
    match eval_form_file(&missing, OsStr::new("{}")) {
        EvalOutcome::Failed(message) => {
            assert!(message.contains("cannot read form file"), "{message}")
        }
        outcome => panic!("expected a read failure for a missing form file, got {outcome:?}"),
    }
}

/// The receipts walker's fixture body: one pinned form followed by its
/// `lisp-receipts` fence, the shape every SKILL.md receipts block uses.
fn receipts_fixture() -> String {
    [
        "# Demo",
        "",
        "Gate (the checker evaluates the receipts beside it):",
        "",
        "- form: `(if (>= n 3) (list (list \"verdict\" \"ok\")) (list (list \"verdict\" \"low\")))`",
        "- env: `{ \"n\": <int> }`",
        "",
        "  ```lisp-receipts",
        "  {\"gate\": \"DEMO-1\", \"case\": \"at-floor\", \"env\": {\"n\": 3}, \"expected\": {\"verdict\": \"ok\"}}",
        "  {\"gate\": \"DEMO-1\", \"case\": \"below-floor\", \"env\": {\"n\": 2}, \"expected\": {\"verdict\": \"low\"}}",
        "  ```",
        "",
    ]
    .join("\n")
}

#[test]
fn receipts_extract_gate_case_env_and_expected() {
    let (receipts, malformed) = extract_receipts(&receipts_fixture(), Path::new("."));
    assert!(
        malformed.is_empty(),
        "unexpected malformed receipts: {malformed:?}"
    );
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].gate, "DEMO-1");
    assert_eq!(receipts[0].case, "at-floor");
    assert_eq!(receipts[0].env, json!({"n": 3}));
    assert_eq!(receipts[0].expected, json!({"verdict": "ok"}));
    // The fence's own line, so a defect message names the receipt's line.
    assert_eq!(receipts[0].fence_line, 8);
    assert_eq!(receipts[0].line, 9);
    assert_eq!(receipts[1].line, 10);
}

#[test]
fn receipts_flag_malformed_lines_with_their_line_numbers() {
    let body = [
        "- form: `(>= n 3)`",
        "",
        "  ```lisp-receipts",
        "  not json at all",
        "  {\"gate\": \"DEMO-1\", \"case\": \"x\", \"env\": 3, \"expected\": null}",
        "  {\"case\": \"x\", \"env\": {}, \"expected\": null}",
        "  {\"gate\": \"DEMO-1\", \"env\": {}, \"expected\": null}",
        "  {\"gate\": \"DEMO-1\", \"case\": \"x\", \"env\": {}}",
        "  ```",
    ]
    .join("\n");
    let (receipts, malformed) = extract_receipts(&body, Path::new("."));
    assert!(receipts.is_empty(), "no receipt should parse: {receipts:?}");
    assert_eq!(
        malformed.len(),
        5,
        "every malformed line is reported: {malformed:?}"
    );
    for (line, _source, _message) in &malformed {
        assert!(
            *line >= 4 && *line <= 8,
            "line {line} must name the offending receipt"
        );
    }
    assert!(
        malformed[0].2.contains("not valid JSON"),
        "{}",
        malformed[0].2
    );
    assert!(malformed[1].2.contains("env"), "{}", malformed[1].2);
    assert!(malformed[2].2.contains("gate"), "{}", malformed[2].2);
    assert!(malformed[3].2.contains("case"), "{}", malformed[3].2);
    assert!(malformed[4].2.contains("expected"), "{}", malformed[4].2);
}

#[test]
fn receipts_evaluate_against_the_nearest_preceding_form() {
    let content = receipts_fixture();
    let candidates = extract_candidates(&content);
    let (receipts, malformed) = extract_receipts(&content, Path::new("."));
    assert!(malformed.is_empty());

    // Clean: both receipts match the authored form exactly.
    let mut findings: Vec<(usize, String)> = Vec::new();
    evaluate_receipts(&candidates, &receipts, &mut |receipt, message| {
        findings.push((receipt.line, message))
    });
    assert!(
        findings.is_empty(),
        "clean receipts report nothing: {findings:?}"
    );

    // A SKILL.md-only edit to the form's vocabulary — the floor value and
    // the verdict word — must break the receipts, not the checker.
    let mutated = content.replacen(
        "(if (>= n 3) (list (list \"verdict\" \"ok\")) (list (list \"verdict\" \"low\")))",
        "(if (>= n 2) (list (list \"verdict\" \"fine\")) (list (list \"verdict\" \"low\")))",
        1,
    );
    let mutated_candidates = extract_candidates(&mutated);
    let mut findings: Vec<(usize, String)> = Vec::new();
    evaluate_receipts(&mutated_candidates, &receipts, &mut |receipt, message| {
        findings.push((receipt.line, message))
    });
    assert_eq!(
        findings.len(),
        2,
        "both receipts must disagree with the mutant"
    );
    assert!(
        findings[0].1.contains("DEMO-1 / at-floor"),
        "{}",
        findings[0].1
    );
    assert!(
        findings[0].1.contains("got {\"verdict\":\"fine\"}"),
        "{}",
        findings[0].1
    );
    assert!(
        findings[1].1.contains("DEMO-1 / below-floor"),
        "{}",
        findings[1].1
    );
}

#[test]
fn receipts_report_evaluation_failures_and_missing_forms() {
    // An env that binds nothing the form names fails visibly, the same
    // as an unbound symbol at use time.
    let unbound = [
        "- form: `(>= n 3)`",
        "",
        "  ```lisp-receipts",
        "  {\"gate\": \"DEMO-1\", \"case\": \"unbound\", \"env\": {\"m\": 1}, \"expected\": true}",
        "  ```",
    ]
    .join("\n");
    let candidates = extract_candidates(&unbound);
    let (receipts, malformed) = extract_receipts(&unbound, Path::new("."));
    assert!(malformed.is_empty());
    let mut findings: Vec<(usize, String)> = Vec::new();
    evaluate_receipts(&candidates, &receipts, &mut |receipt, message| {
        findings.push((receipt.line, message))
    });
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].1.contains("evaluation failed"),
        "{}",
        findings[0].1
    );

    // A receipts fence with no form above it is a defect, not a skip.
    let orphan = [
        "  ```lisp-receipts",
        "  {\"gate\": \"DEMO-1\", \"case\": \"orphan\", \"env\": {}, \"expected\": null}",
        "  ```",
    ]
    .join("\n");
    let candidates = extract_candidates(&orphan);
    let (receipts, malformed) = extract_receipts(&orphan, Path::new("."));
    assert!(malformed.is_empty());
    let mut findings: Vec<(usize, String)> = Vec::new();
    evaluate_receipts(&candidates, &receipts, &mut |receipt, message| {
        findings.push((receipt.line, message))
    });
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].1.contains("no form precedes"),
        "{}",
        findings[0].1
    );
}

/// The wiring itself: a corpus scan over a skill whose receipts match is
/// clean, and the same corpus with one receipt's expected flipped fails
/// with file:line. This is the seam the deciding criterion runs through —
/// if the scan stopped evaluating receipts, a broken form would pass.
#[test]
fn receipts_are_evaluated_by_the_corpus_scan() {
    let root = std::env::temp_dir().join(format!("hkask-lisp-receipts-{}", std::process::id()));
    let skills = root.join("skills");
    let templates = root.join("templates");
    let demo = skills.join("demo");
    fs::create_dir_all(&demo).expect("fixture skill dir");
    fs::create_dir_all(&templates).expect("fixture templates dir");
    fs::write(templates.join("demo.j2"), "{# clean #}\n").expect("fixture template");
    let clean = receipts_fixture();
    fs::write(demo.join("SKILL.md"), &clean).expect("fixture skill");

    let stats = scan_corpus(&skills, &templates).expect("scan fixture corpus");
    assert!(stats.findings.is_empty(), "clean receipts: {stats:#?}");

    let broken = clean.replacen(
        "\"expected\": {\"verdict\": \"ok\"}",
        "\"expected\": {\"verdict\": \"no\"}",
        1,
    );
    fs::write(demo.join("SKILL.md"), &broken).expect("overwrite fixture skill");
    let stats = scan_corpus(&skills, &templates).expect("scan mutated fixture corpus");
    assert_eq!(
        stats.findings.len(),
        1,
        "one receipt defect expected: {stats:#?}"
    );
    assert_eq!(
        stats.findings[0].line, 9,
        "the defect names the receipt's line"
    );
    assert!(stats.findings[0].message.contains("DEMO-1 / at-floor"));
    fs::remove_dir_all(&root).ok();
}

/// A fence that names a sibling receipts file: rows are read one JSON
/// object per line from that file, keep the fence's form association,
/// and report defects against the file they live in. This is how a
/// large fixture table stays out of the load-time SKILL.md without
/// leaving the skill directory.
#[test]
fn receipts_fences_can_name_a_sibling_receipts_file() {
    let root =
        std::env::temp_dir().join(format!("hkask-lisp-receipts-file-{}", std::process::id()));
    let skill_dir = root.join("skill");
    fs::create_dir_all(&skill_dir).expect("fixture dir");
    let skill = [
        "# Demo",
        "",
        "- form: `(if (>= n 3) (list (list \"verdict\" \"ok\")) (list (list \"verdict\" \"low\")))`",
        "",
        "  ```lisp-receipts demo-receipts.jsonl",
        "  ```",
        "",
    ]
    .join("\n");
    fs::write(
        skill_dir.join("demo-receipts.jsonl"),
        [
            "{\"gate\": \"DEMO-1\", \"case\": \"at-floor\", \"env\": {\"n\": 3}, \"expected\": {\"verdict\": \"ok\"}}",
            "",
            "{\"gate\": \"DEMO-1\", \"case\": \"below-floor\", \"env\": {\"n\": 2}, \"expected\": {\"verdict\": \"low\"}}",
        ]
        .join("\n"),
    )
    .expect("receipts file");

    let (receipts, malformed) = extract_receipts(&skill, &skill_dir);
    assert!(malformed.is_empty(), "{malformed:?}");
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].line, 1, "row lines number the receipts file");
    assert_eq!(receipts[1].line, 3, "blank lines still number");
    assert_eq!(
        receipts[0].source_path,
        Some(skill_dir.join("demo-receipts.jsonl"))
    );
    let candidates = extract_candidates(&skill);
    let mut findings: Vec<(usize, String)> = Vec::new();
    evaluate_receipts(&candidates, &receipts, &mut |receipt, message| {
        findings.push((receipt.line, message))
    });
    assert!(
        findings.is_empty(),
        "file rows evaluate like inline rows: {findings:?}"
    );
    fs::remove_dir_all(&root).ok();
}

/// The corpus seam for named receipts files: a flipped expectation in
/// the file fails with the file's path and the row's line; a fence that
/// names a missing file, or carries inline rows beside the name, is a
/// defect at the fence.
#[test]
fn receipts_file_defects_surface_through_the_corpus_scan() {
    let root =
        std::env::temp_dir().join(format!("hkask-lisp-receipts-scan-{}", std::process::id()));
    let skills = root.join("skills");
    let templates = root.join("templates");
    let demo = skills.join("demo");
    fs::create_dir_all(&demo).expect("fixture skill dir");
    fs::create_dir_all(&templates).expect("fixture templates dir");
    fs::write(templates.join("demo.j2"), "{# clean #}\n").expect("fixture template");
    let skill = [
        "# Demo",
        "",
        "- form: `(if (>= n 3) (list (list \"verdict\" \"ok\")) (list (list \"verdict\" \"low\")))`",
        "",
        "  ```lisp-receipts demo-receipts.jsonl",
        "  ```",
        "",
    ]
    .join("\n");
    fs::write(demo.join("SKILL.md"), &skill).expect("fixture skill");
    fs::write(
        demo.join("demo-receipts.jsonl"),
        "{\"gate\": \"DEMO-1\", \"case\": \"at-floor\", \"env\": {\"n\": 3}, \"expected\": {\"verdict\": \"no\"}}\n",
    )
    .expect("receipts file");

    let stats = scan_corpus(&skills, &templates).expect("scan named-file corpus");
    assert_eq!(stats.findings.len(), 1, "{stats:#?}");
    assert_eq!(stats.findings[0].path, demo.join("demo-receipts.jsonl"));
    assert_eq!(stats.findings[0].line, 1, "the defect names the row");
    assert!(
        stats.findings[0].message.contains("DEMO-1 / at-floor"),
        "{}",
        stats.findings[0].message
    );

    // A fence naming a missing file is a defect at the fence line.
    fs::remove_file(demo.join("demo-receipts.jsonl")).expect("remove receipts file");
    let stats = scan_corpus(&skills, &templates).expect("scan missing-file corpus");
    assert_eq!(stats.findings.len(), 1, "{stats:#?}");
    assert_eq!(stats.findings[0].path, demo.join("SKILL.md"));
    assert_eq!(stats.findings[0].line, 5, "the defect names the fence");
    assert!(
        stats.findings[0].message.contains("demo-receipts.jsonl"),
        "{}",
        stats.findings[0].message
    );

    // Inline rows beside a named file are ambiguous, so they are a defect.
    fs::write(
        demo.join("SKILL.md"),
        skill.replacen(
            "  ```lisp-receipts demo-receipts.jsonl\n  ```",
            concat!(
                "  ```lisp-receipts demo-receipts.jsonl\n",
                "  {\"gate\": \"DEMO-1\", \"case\": \"inline\", \"env\": {}, \"expected\": null}\n",
                "  ```",
            ),
            1,
        ),
    )
    .expect("inline-beside-name fixture");
    let stats = scan_corpus(&skills, &templates).expect("scan inline-beside-name corpus");
    assert_eq!(stats.findings.len(), 1, "{stats:#?}");
    assert!(
        stats.findings[0]
            .message
            .contains("cannot also carry inline rows"),
        "{}",
        stats.findings[0].message
    );
    fs::remove_dir_all(&root).ok();
}

/// The loader cap: crates/agent_skills rejects an oversized SKILL.md at
/// index time (the skill vanishes from the catalog and the `skill`
/// tool). The scan pins the same bound so the composition error is a
/// corpus defect, not a silent disappearance.
#[test]
fn an_oversized_skill_md_is_a_scan_defect() {
    let root = std::env::temp_dir().join(format!("hkask-lisp-oversize-{}", std::process::id()));
    let skills = root.join("skills");
    let templates = root.join("templates");
    let demo = skills.join("demo");
    let big = skills.join("big");
    fs::create_dir_all(&demo).expect("fixture skill dir");
    fs::create_dir_all(&big).expect("fixture big dir");
    fs::create_dir_all(&templates).expect("fixture templates dir");
    fs::write(templates.join("demo.j2"), "{# clean #}\n").expect("fixture template");
    fs::write(demo.join("SKILL.md"), receipts_fixture()).expect("clean skill");
    fs::write(
        big.join("SKILL.md"),
        format!(
            "{}\n{}\n",
            receipts_fixture(),
            "x".repeat(MAX_SKILL_FILE_SIZE)
        ),
    )
    .expect("oversized skill");

    let stats = scan_corpus(&skills, &templates).expect("scan oversized corpus");
    assert_eq!(stats.findings.len(), 1, "{stats:#?}");
    assert_eq!(stats.findings[0].path, big.join("SKILL.md"));
    assert!(
        stats.findings[0].message.contains("loader cap"),
        "{}",
        stats.findings[0].message
    );
    fs::remove_dir_all(&root).ok();
}
