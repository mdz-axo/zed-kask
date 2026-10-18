#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hkask_lisp::{LispValue, parse};

#[derive(Debug, Clone)]
struct Candidate {
    line: usize,
    source: String,
}

#[derive(Debug)]
struct Finding {
    path: PathBuf,
    line: usize,
    message: String,
}

#[derive(Debug, Default)]
struct ScanStats {
    files: usize,
    skill_files: usize,
    candidates: usize,
    findings: Vec<Finding>,
}

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(skills_dir) = args.next() else {
        eprintln!("usage: check-skill-lisp-forms SKILLS_DIR TEMPLATES_DIR");
        return ExitCode::from(2);
    };
    let Some(templates_dir) = args.next() else {
        eprintln!("usage: check-skill-lisp-forms SKILLS_DIR TEMPLATES_DIR");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: check-skill-lisp-forms SKILLS_DIR TEMPLATES_DIR");
        return ExitCode::from(2);
    }

    let skills_dir = PathBuf::from(skills_dir);
    let templates_dir = PathBuf::from(templates_dir);
    match scan_corpus(&skills_dir, &templates_dir) {
        Ok(stats) if stats.findings.is_empty() => {
            println!(
                "OK: scanned {} files ({} skills), parsed {} Lisp candidates; 0 defects.",
                stats.files, stats.skill_files, stats.candidates
            );
            ExitCode::SUCCESS
        }
        Ok(stats) => {
            for finding in &stats.findings {
                eprintln!(
                    "SKILL LISP DEFECT: {}:{}: {}",
                    display_path(&finding.path),
                    finding.line,
                    finding.message
                );
            }
            eprintln!(
                "FAIL: scanned {} files ({} skills), parsed {} Lisp candidates; {} defect(s).",
                stats.files,
                stats.skill_files,
                stats.candidates,
                stats.findings.len()
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("SKILL LISP CHECK FAILED: {error}");
            ExitCode::FAILURE
        }
    }
}

fn scan_corpus(skills_dir: &Path, templates_dir: &Path) -> Result<ScanStats, String> {
    if !skills_dir.is_dir() {
        return Err(format!(
            "skill corpus directory not found: {}",
            display_path(skills_dir)
        ));
    }
    if !templates_dir.is_dir() {
        return Err(format!(
            "template corpus directory not found: {}",
            display_path(templates_dir)
        ));
    }

    let mut files = Vec::new();
    collect_files(skills_dir, CorpusKind::Skills, &mut files)?;
    collect_files(templates_dir, CorpusKind::Templates, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err("skill/template corpus contains no scannable files".into());
    }

    let mut stats = ScanStats::default();
    for path in files {
        stats.files += 1;
        let is_skill = path.file_name() == Some(OsStr::new("SKILL.md"));
        if is_skill {
            stats.skill_files += 1;
        }

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                stats.findings.push(Finding {
                    path,
                    line: 1,
                    message: format!("file is not readable UTF-8: {error}"),
                });
                continue;
            }
        };
        let candidates = extract_candidates(&content);
        if is_skill && content.contains("lisp_eval") && candidates.is_empty() {
            stats.findings.push(Finding {
                path: path.clone(),
                line: 1,
                message: "SKILL.md names lisp_eval but exposes no parseable Lisp candidate to the checker"
                    .into(),
            });
        }

        for candidate in candidates {
            stats.candidates += 1;
            match parse(&candidate.source) {
                Ok(forms) => {
                    for form in &forms {
                        lint_form(form, &mut |message| {
                            stats.findings.push(Finding {
                                path: path.clone(),
                                line: candidate.line,
                                message,
                            });
                        });
                    }
                }
                Err(error) => stats.findings.push(Finding {
                    path: path.clone(),
                    line: candidate.line,
                    message: error.to_string(),
                }),
            }
        }
    }

    if stats.skill_files == 0 {
        return Err("skill corpus contains no SKILL.md files".into());
    }
    if stats.candidates == 0 {
        return Err("skill/template corpus exposes zero Lisp candidates".into());
    }
    Ok(stats)
}

#[derive(Clone, Copy)]
enum CorpusKind {
    Skills,
    Templates,
}

fn collect_files(dir: &Path, kind: CorpusKind, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("cannot read {}: {error}", display_path(dir)))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("cannot read entry under {}: {error}", display_path(dir)))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", display_path(&path)))?;
        if file_type.is_dir() {
            collect_files(&path, kind, files)?;
        } else if file_type.is_file() && should_scan(&path, kind) {
            files.push(path);
        }
    }
    Ok(())
}

fn should_scan(path: &Path, kind: CorpusKind) -> bool {
    match kind {
        CorpusKind::Skills => path.file_name() == Some(OsStr::new("SKILL.md")),
        CorpusKind::Templates => {
            matches!(path.extension().and_then(OsStr::to_str), Some("j2" | "md"))
        }
    }
}

fn extract_candidates(content: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if let Some((marker, language)) = fence_start(trimmed) {
            let start_line = index + 2;
            let mut body = String::new();
            index += 1;
            while index < lines.len() && !lines[index].trim_start().starts_with(marker) {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(lines[index]);
                index += 1;
            }
            let candidate = body.trim();
            if !candidate.is_empty()
                && (matches!(language, "lisp" | "scheme") || looks_like_lisp(candidate))
            {
                push_candidate(&mut candidates, &mut seen, start_line, candidate);
            } else {
                for (offset, body_line) in body.lines().enumerate() {
                    let body_line_number = start_line + offset;
                    extract_inline_spans(body_line, body_line_number, &mut candidates, &mut seen);
                    extract_form_value(body_line, body_line_number, &mut candidates, &mut seen);
                }
            }
        } else {
            let prose_start = index;
            let mut prose = String::new();
            while index < lines.len() && fence_start(lines[index].trim_start()).is_none() {
                if !prose.is_empty() {
                    prose.push('\n');
                }
                prose.push_str(lines[index]);
                extract_form_value(lines[index], index + 1, &mut candidates, &mut seen);
                index += 1;
            }
            extract_inline_spans(&prose, prose_start + 1, &mut candidates, &mut seen);
            continue;
        }
        index += 1;
    }

    candidates
}

fn fence_start(line: &str) -> Option<(&str, &str)> {
    for marker in ["```", "~~~"] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some((marker, rest.trim()));
        }
    }
    None
}

fn extract_inline_spans(
    text: &str,
    first_line: usize,
    candidates: &mut Vec<Candidate>,
    seen: &mut BTreeSet<(usize, String)>,
) {
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(open_offset) = text[cursor..].find('`') else {
            break;
        };
        let open = cursor + open_offset;
        let delimiter_len = text[open..]
            .bytes()
            .take_while(|byte| *byte == b'`')
            .count();
        if delimiter_len >= 3 {
            cursor = open + delimiter_len;
            continue;
        }
        let body_start = open + delimiter_len;
        let Some(close) = find_matching_backticks(text, body_start, delimiter_len) else {
            break;
        };
        let candidate = text[body_start..close].trim();
        if looks_like_lisp(candidate) {
            let line = first_line + text[..open].bytes().filter(|byte| *byte == b'\n').count();
            push_candidate(candidates, seen, line, candidate);
        }
        cursor = close + delimiter_len;
    }
}

fn find_matching_backticks(text: &str, mut cursor: usize, delimiter_len: usize) -> Option<usize> {
    while cursor < text.len() {
        let offset = text[cursor..].find('`')?;
        let start = cursor + offset;
        let run_len = text[start..]
            .bytes()
            .take_while(|byte| *byte == b'`')
            .count();
        if run_len == delimiter_len {
            return Some(start);
        }
        cursor = start + run_len;
    }
    None
}

fn extract_form_value(
    line: &str,
    line_number: usize,
    candidates: &mut Vec<Candidate>,
    seen: &mut BTreeSet<(usize, String)>,
) {
    let Some(form_offset) = line.find("form:") else {
        return;
    };
    let rest = line[form_offset + "form:".len()..].trim_start();
    if rest.starts_with('`') || rest.is_empty() {
        return;
    }
    if rest.starts_with('"') {
        if let Some(encoded) = quoted_json_string(rest)
            && let Ok(decoded) = serde_json::from_str::<String>(encoded)
            && looks_like_lisp(&decoded)
        {
            push_candidate(candidates, seen, line_number, decoded.trim());
        }
    } else if looks_like_lisp(rest) {
        push_candidate(candidates, seen, line_number, rest);
    }
}

fn quoted_json_string(input: &str) -> Option<&str> {
    let mut escaped = false;
    for (index, character) in input.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return input.get(..=index);
        }
    }
    None
}

fn push_candidate(
    candidates: &mut Vec<Candidate>,
    seen: &mut BTreeSet<(usize, String)>,
    line: usize,
    source: &str,
) {
    let source = source.trim().to_string();
    if seen.insert((line, source.clone())) {
        candidates.push(Candidate { line, source });
    }
}

fn looks_like_lisp(candidate: &str) -> bool {
    let trimmed = candidate.trim_start();
    trimmed.starts_with('(') || trimmed.starts_with("'(")
}

fn lint_form(form: &LispValue, report: &mut impl FnMut(String)) {
    let LispValue::List(list) = form else {
        return;
    };
    let items = list.to_vec();
    let head = items.first().and_then(symbol_name);
    if head == Some("quote") {
        return;
    }

    if head == Some("is_null") && items.get(1).and_then(call_head) == Some("member") {
        report(
            "is_null(member ...) is permanently false because member returns a boolean; use member directly"
                .into(),
        );
    }
    if head == Some("=")
        && items
            .iter()
            .skip(1)
            .any(|item| matches!(item, LispValue::String(_)))
    {
        report("numeric = receives a string literal; use string= for string comparisons".into());
    }
    if head == Some("string=")
        && items.iter().skip(1).any(|item| {
            matches!(
                item,
                LispValue::Int(_) | LispValue::Float(_) | LispValue::Bool(_)
            )
        })
    {
        report("string= receives a numeric or boolean literal".into());
    }
    if head == Some("or") {
        lint_unguarded_union_comparison(&items[1..], report);
    }

    for item in items.iter().skip(1) {
        lint_form(item, report);
    }
}

fn lint_unguarded_union_comparison(branches: &[LispValue], report: &mut impl FnMut(String)) {
    let mut string_symbols = BTreeSet::new();
    let mut numeric_symbols = BTreeSet::new();
    for branch in branches {
        let LispValue::List(list) = branch else {
            continue;
        };
        let items = list.to_vec();
        match items.first().and_then(symbol_name) {
            Some("string=") => {
                for item in items.iter().skip(1) {
                    if let Some(symbol) = symbol_name(item) {
                        string_symbols.insert(symbol.to_string());
                    }
                }
            }
            Some("<" | "<=" | ">" | ">=" | "=" | "!=") => {
                for item in items.iter().skip(1) {
                    if let Some(symbol) = symbol_name(item) {
                        numeric_symbols.insert(symbol.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(symbol) = string_symbols.intersection(&numeric_symbols).next() {
        report(format!(
            "or compares `{symbol}` as both string and number without stringp/numberp guards"
        ));
    }
}

fn call_head(value: &LispValue) -> Option<&str> {
    let LispValue::List(list) = value else {
        return None;
    };
    symbol_name(&list.head)
}

fn symbol_name(value: &LispValue) -> Option<&str> {
    match value {
        LispValue::Symbol(symbol) => Some(symbol),
        _ => None,
    }
}

fn display_path(path: &Path) -> String {
    if let Ok(current_dir) = env::current_dir()
        && let Ok(relative) = path.strip_prefix(current_dir)
    {
        return relative.display().to_string();
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
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
    fn repaired_corpus_gates_discriminate_both_directions() {
        let essentialist = skill_candidate("essentialist", "current_surviving_items");
        let stable = json!({
            "current_surviving_items": ["A", "B"],
            "prior_surviving_items": ["A", "B"],
            "current_structure": [["module", "m"]],
            "prior_structure": [["module", "m"]],
            "current_interfaces": ["f"],
            "prior_interfaces": ["f"]
        });
        assert_eq!(
            eval_sandboxed(&essentialist, &stable).expect("stable essentialist form"),
            json!(true)
        );
        let mut changed = stable;
        changed["current_structure"] = json!([["module", "changed"]]);
        assert_eq!(
            eval_sandboxed(&essentialist, &changed).expect("changed essentialist form"),
            json!(false)
        );

        let wardley = skill_candidate("wardley-mapper", "all-names-present");
        let valid_map = json!({
            "components": [
                {"name":"API","evolution":"Product","x":0.6,"y":0.9,"links":["DB"]},
                {"name":"DB","evolution":"Commodity","x":0.8,"y":0.2,"links":[]}
            ],
            "component_names": ["API", "DB"],
            "map_diagram": "quadrantChart\nAPI: [0.6, 0.9]\nDB: [0.8, 0.2]"
        });
        assert_eq!(
            eval_sandboxed(&wardley, &valid_map).expect("valid Wardley form"),
            json!(true)
        );
        let invalid_map = json!({
            "components": [
                {"name":"API","evolution":"Product","x":0.6,"y":0.9,"links":[]}
            ],
            "component_names": ["API", "DB"],
            "map_diagram": "not a quadrant chart\nAPI"
        });
        assert_eq!(
            eval_sandboxed(&wardley, &invalid_map).expect("invalid Wardley form"),
            json!(false)
        );

        let sankey = skill_candidate("sankey-flow", "all-edges-ok");
        let valid_sankey = json!({
            "nodes": [{"id":"Revenue"},{"id":"Profit"}],
            "edges": [{"source":"Revenue","target":"Profit","weight":5000,"weight_source":"statement"}]
        });
        assert_eq!(
            eval_sandboxed(&sankey, &valid_sankey).expect("valid Sankey form"),
            json!(true)
        );
        let invalid_sankey = json!({
            "nodes": [{"id":"Revenue"},{"id":"Revenue"}],
            "edges": [{"source":"Revenue","target":"Ghost","weight":"5000"}]
        });
        assert_eq!(
            eval_sandboxed(&sankey, &invalid_sankey).expect("invalid Sankey form"),
            json!(false)
        );

        // PE-2 counts Prohibition-tier findings from the critic's structured
        // output. Before the repair the count was a model-typed env value, so
        // a `pass` verdict could be self-certified over a blocking defect.
        let prompt_enhance = skill_candidate("prompt-enhance", "count-prohibitions");
        let pass_over_blocking = json!({
            "rounds_executed": 1,
            "pdca_corrections": 0,
            "verdict": "pass",
            "findings": [
                {"area":"Recall","gap":"g1","severity":"Prohibition"},
                {"area":"Mechanism","gap":"g2","severity":"Guideline"}
            ]
        });
        assert_eq!(
            eval_sandboxed(&prompt_enhance, &pass_over_blocking).expect("PE-2 blocking case"),
            json!([1, false]),
            "a pass verdict carrying a Prohibition finding must fail PE-2"
        );
        let clean_pass = json!({
            "rounds_executed": 1,
            "pdca_corrections": 0,
            "verdict": "pass",
            "findings": [{"area":"Mechanism","gap":"g2","severity":"Guardrail"}]
        });
        assert_eq!(
            eval_sandboxed(&prompt_enhance, &clean_pass).expect("PE-2 clean case"),
            json!([0, true])
        );
        let no_findings = json!({
            "rounds_executed": 1,
            "pdca_corrections": 0,
            "verdict": "pass",
            "findings": []
        });
        assert_eq!(
            eval_sandboxed(&prompt_enhance, &no_findings).expect("PE-2 empty findings"),
            json!([0, true])
        );
        let non_pass_with_defect = json!({
            "rounds_executed": 3,
            "pdca_corrections": 1,
            "verdict": "rewrite_needed",
            "findings": [{"area":"Recall","gap":"g1","severity":"Prohibition"}]
        });
        assert_eq!(
            eval_sandboxed(&prompt_enhance, &non_pass_with_defect).expect("PE-2 non-pass case"),
            json!([1, true]),
            "a non-pass verdict carries its defect without breaching the escalation bound"
        );

        // PS-1 applies the ontology/spec modifiers exactly once. The prose
        // receipt (base 0.7, FIBO + spec-checked -> 0.8) is pinned here so a
        // modifier edit cannot silently diverge from the documented value.
        let pragmatic_semantics = skill_candidate("pragmatic-semantics", "fibo_anchored");
        let fibo_spec_checked = json!({
            "base_confidence": 0.7,
            "fibo_anchored": true,
            "sumo_anchored": false,
            "spec_checked": true,
            "unanchored": false
        });
        assert_eq!(
            eval_sandboxed(&pragmatic_semantics, &fibo_spec_checked).expect("PS-1 anchored case"),
            json!(0.8)
        );
        let unanchored = json!({
            "base_confidence": 0.7,
            "fibo_anchored": false,
            "sumo_anchored": false,
            "spec_checked": false,
            "unanchored": true
        });
        assert_eq!(
            eval_sandboxed(&pragmatic_semantics, &unanchored).expect("PS-1 unanchored case"),
            json!(0.6)
        );
    }

    /// The `PS-1` modifier values are stated in prose in the skill body and in
    /// three companion templates. That prose drifted once already (FIBO +0.10 /
    /// SUMO +0.05 / unanchored -0.15 survived a gate edit to +0.05 / +0.03 /
    /// -0.10), so the documented values are pinned to the *gate's own* values
    /// rather than to literals: each modifier is derived by evaluating the live
    /// form with exactly one flag set, and the prose must contain the derived
    /// spelling. A gate edit that is not carried into the prose fails here.
    #[test]
    fn pragmatic_semantics_modifier_prose_matches_the_ps1_gate() {
        let gate = skill_candidate("pragmatic-semantics", "fibo_anchored");

        let derive = |flag: &str| -> f64 {
            // A base of 0.5 keeps every branch clear of both clamps, so the
            // derived value is the modifier itself.
            let mut env = json!({
                "base_confidence": 0.5,
                "fibo_anchored": false,
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
        let mutated = gate.replace("fibo_anchored 0.05", "fibo_anchored 0.10");
        assert_ne!(mutated, gate, "the control must actually mutate the gate");
        let mutated_fibo = eval_sandboxed(
            &mutated,
            &json!({
                "base_confidence": 0.5,
                "fibo_anchored": true,
                "sumo_anchored": false,
                "spec_checked": false,
                "unanchored": false
            }),
        )
        .expect("mutated PS-1 evaluates")
        .as_f64()
        .expect("mutated PS-1 returns a number")
            - 0.5;

        let fibo = derive("fibo_anchored");
        let sumo = derive("sumo_anchored");
        let spec = derive("spec_checked");
        let unanchored = derive("unanchored");
        assert_ne!(
            mutated_fibo, fibo,
            "the control must be discriminating: a mutated gate must derive a different modifier"
        );

        // The positive modifiers are the ones that drifted; the unanchored
        // penalty is checked on its magnitude because the templates use the
        // typographic minus (U+2212) and the skill body the ASCII hyphen.
        let positive = |value: f64| format!("+{value:.2}");
        let fibo_text = positive(fibo);
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
        let expected_constraint_line = format!(
            "FIBO {fibo_text}, SUMO {sumo_text}, spec_checked {spec_text}, unanchored -{unanchored_magnitude}"
        );
        assert!(
            body.contains(&expected_constraint_line),
            "SKILL.md must state the gate's own modifiers ({expected_constraint_line}); \
             the prose has drifted from the gate"
        );

        let conflict =
            read("kask/registry/templates/pragmatic-semantics/semantics-conflict-resolve.j2");
        assert!(
            conflict.contains(&format!("the `PS-1` modifier is {fibo_text}")),
            "semantics-conflict-resolve.j2 must carry the gate's FIBO modifier ({fibo_text})"
        );
        assert!(
            conflict.contains(&sumo_text),
            "semantics-conflict-resolve.j2 must carry the gate's SUMO modifier ({sumo_text})"
        );

        let provenance =
            read("kask/registry/templates/pragmatic-semantics/semantics-provenance-trace.j2");
        assert!(
            provenance.contains(&format!("FIBO ({fibo_text}), SUMO ({sumo_text})")),
            "semantics-provenance-trace.j2 must carry the gate's modifiers \
             (FIBO {fibo_text}, SUMO {sumo_text})"
        );

        let classify =
            read("kask/registry/templates/pragmatic-semantics/semantics-classify-statement.j2");
        assert!(
            classify.contains(&format!("| **FIBO** (financial) | {fibo_text} |")),
            "semantics-classify-statement.j2 must carry the gate's FIBO modifier ({fibo_text})"
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
}
