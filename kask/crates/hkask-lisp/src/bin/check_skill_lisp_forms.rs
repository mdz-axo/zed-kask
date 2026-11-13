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
    lint_alist_keys(form, report);

    for item in items.iter().skip(1) {
        lint_form(item, report);
    }
}

/// Flag alist-like forms whose literal keys are not all strings: the result
/// will not project to a JSON object, and the silent array fallback is the
/// failure mode the projection precondition documents. Only literal keys are
/// decidable statically — symbols may be strings at runtime, so forms with
/// variable keys are skipped rather than false-positived.
fn lint_alist_keys(form: &LispValue, report: &mut impl FnMut(String)) {
    let LispValue::List(list) = form else {
        return;
    };
    let items = list.to_vec();
    let Some("list") = items.first().and_then(symbol_name) else {
        return;
    };
    // Direct pair-like arguments: (list <key> <value> ...).
    let mut keys: Vec<LispValue> = Vec::new();
    for item in items.iter().skip(1) {
        let LispValue::List(pair) = item else {
            continue;
        };
        let pair_items = pair.to_vec();
        if pair_items.len() >= 3 {
            keys.push(pair_items[1].clone());
        }
    }
    if keys.len() < 2 {
        return;
    }
    if keys.iter().any(|key| {
        !matches!(
            key,
            LispValue::String(_)
                | LispValue::Int(_)
                | LispValue::Float(_)
                | LispValue::Bool(_)
                | LispValue::Nil
        )
    }) {
        return;
    }
    let string_keys = keys
        .iter()
        .filter(|key| matches!(key, LispValue::String(_)))
        .count();
    if string_keys == 0 || string_keys == keys.len() {
        return;
    }
    report(
        "mixed alist keys: this form will not project to a JSON object — use string keys for every pair"
            .into(),
    );
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
    fn superforecasting_quality_uses_raw_bounded_criteria() {
        let form = skill_candidate("superforecasting", "define wsum");
        let dimensions = [
            "calibration_realism",
            "confidence_justification",
            "evidence_trail",
            "record_completeness",
        ];
        let criteria = |score: f64| {
            let rows = dimensions
                .iter()
                .map(|name| ((*name).to_string(), json!(vec![score; 4])))
                .collect::<serde_json::Map<_, _>>();
            json!({"crit": rows})
        };
        for (score, pass) in [(1.0, true), (0.6, true), (0.599999, false), (0.0, false)] {
            let result = eval_sandboxed(&form, &criteria(score)).expect("authored SF-3 evaluates");
            assert_eq!(result[4], pass, "inclusive threshold for {score}: {result}");
            for total in result
                .as_array()
                .expect("totals and verdict")
                .iter()
                .take(4)
            {
                assert!((total.as_f64().expect("numeric total") - score).abs() < 1e-12);
            }
        }
        for (index, weights) in [
            ([1.0, 0.0, 0.0, 0.0], [0.30, 0.35, 0.35, 0.35]),
            ([0.0, 1.0, 0.0, 0.0], [0.25, 0.30, 0.30, 0.25]),
            ([0.0, 0.0, 1.0, 0.0], [0.20; 4]),
            ([0.0, 0.0, 0.0, 1.0], [0.25, 0.15, 0.15, 0.20]),
        ] {
            let mut env = criteria(0.0);
            for name in dimensions {
                env["crit"][name] = json!(index);
            }
            let result = eval_sandboxed(&form, &env).expect("one-hot criterion weights");
            for (i, expected) in weights.iter().enumerate() {
                assert!((result[i].as_f64().expect("weighted total") - expected).abs() < 1e-12);
            }
        }
        for value in [
            json!(null),
            json!([]),
            json!([1, 1, 1]),
            json!([1, 1, 1, 1, 1]),
            json!([2, 1, 1, 1]),
            json!([-0.1, 1, 1, 1]),
            json!(["1", 1, 1, 1]),
            json!([true, 1, 1, 1]),
        ] {
            let mut env = criteria(1.0);
            env["crit"]["evidence_trail"] = value;
            assert_eq!(
                eval_sandboxed(&form, &env).expect("invalid scores are explicit"),
                json!([null, "invalid_criteria"])
            );
        }
        let mut extra = criteria(1.0);
        extra["crit"]["invented_dimension"] = json!([1, 1, 1, 1]);
        assert_eq!(
            eval_sandboxed(&form, &extra).expect("extra dimension refused"),
            json!([null, "invalid_criteria"])
        );
    }

    #[test]
    fn mcp_inventory_reconciliation_uses_exact_raw_names() {
        let form = skill_candidate("mcp-tool-review", "define mtr-reconcile");
        let base = json!({
            "source_names": ["direct", "wire_alias", "wrapped"],
            "live_names": ["wrapped", "direct", "wire_alias"],
            "reviewed_names": ["wire_alias", "wrapped", "direct"]
        });
        let complete = eval_sandboxed(&form, &base).expect("authored name reconciliation");
        assert_eq!(complete["input_valid"], true);
        assert_eq!(complete["source_live_match"], true);
        assert_eq!(complete["review_complete"], true);
        assert_eq!(complete["source_count"], 3);
        assert_eq!(complete["live_count"], 3);
        assert_eq!(complete["reviewed_count"], 3);
        assert_eq!(complete["result"], "pass");

        let drift = eval_sandboxed(
            &form,
            &scientific_env(
                &base,
                json!({
                    "live_names": ["direct", "renamed", "wrapped"],
                    "source_count": 3, "live_count": 3, "reviewed_count": 3, "missing_in_source": 0
                }),
            ),
        )
        .expect("same-count rename cannot hide behind supplied counts");
        assert_eq!(drift["source_live_match"], false);
        assert_eq!(drift["review_complete"], false);
        assert_eq!(drift["source_only_names"], json!(["wire_alias"]));
        assert_eq!(drift["live_only_names"], json!(["renamed"]));
        assert_eq!(drift["result"], "source_live_drift");

        let partial = eval_sandboxed(
            &form,
            &scientific_env(
                &base,
                json!({
                    "reviewed_names": ["direct"]
                }),
            ),
        )
        .expect("bounded review is incomplete, not inventory drift");
        assert_eq!(partial["source_live_match"], true);
        assert_eq!(partial["review_complete"], false);
        assert_eq!(partial["reviewed_count"], 1);
        assert_eq!(
            partial["not_reviewed_names"],
            json!(["wire_alias", "wrapped"])
        );
        assert_eq!(partial["result"], "review_incomplete");
        let foreign_review = eval_sandboxed(
            &form,
            &scientific_env(
                &base,
                json!({
                    "reviewed_names": ["direct", "wire_alias", "unknown"]
                }),
            ),
        )
        .expect("unknown reviewed identity");
        assert_eq!(foreign_review["review_complete"], false);
        assert_eq!(foreign_review["unknown_reviewed_names"], json!(["unknown"]));

        for changes in [
            json!({"source_names": []}),
            json!({"source_names": "direct"}),
            json!({"live_names": ["direct", "direct", "wrapped"]}),
            json!({"reviewed_names": ["direct", "direct", "wrapped"]}),
            json!({"source_names": ["direct", "", "wrapped"]}),
            json!({"live_names": ["direct", 7, "wrapped"]}),
        ] {
            let invalid = eval_sandboxed(&form, &scientific_env(&base, changes))
                .expect("invalid names have a fail-visible result");
            assert_eq!(invalid["input_valid"], false);
            assert_eq!(invalid["review_complete"], false);
            assert_eq!(invalid["result"], "invalid_names");
        }
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

        // Prompt-enhance attempt, rating and blocking-defect cases are consolidated
        // in prompt_enhance_state_contract_uses_authored_form below.

        // PS-1 applies the ontology/spec modifiers exactly once. The prose
        // receipt (base 0.7, domain anchor + spec-checked -> 0.8) is pinned
        // here so a modifier edit cannot silently diverge from the documented value.
        let pragmatic_semantics = skill_candidate("pragmatic-semantics", "domain_anchored");
        let domain_spec_checked = json!({
            "base_confidence": 0.7,
            "provenance": "runtime",
            "domain_anchored": true,
            "sumo_anchored": false,
            "spec_checked": true,
            "unanchored": false
        });
        assert_eq!(
            eval_sandboxed(&pragmatic_semantics, &domain_spec_checked).expect("PS-1 anchored case"),
            json!(0.8)
        );
        let unanchored = json!({
            "base_confidence": 0.7,
            "provenance": "runtime",
            "domain_anchored": false,
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

    fn scientific_env(base: &serde_json::Value, changes: serde_json::Value) -> serde_json::Value {
        let mut env = base.clone();
        env.as_object_mut()
            .expect("gate fixture is an object")
            .extend(
                changes
                    .as_object()
                    .expect("fixture changes are an object")
                    .clone(),
            );
        env
    }

    fn assert_scientific_case(
        form: &str,
        case: &str,
        env: &serde_json::Value,
        expected: serde_json::Value,
    ) {
        let actual = eval_sandboxed(form, env)
            .unwrap_or_else(|error| panic!("{case}: authored gate failed to evaluate: {error}"));
        assert_eq!(actual, expected, "{case}");
    }

    fn scientific_normalization_env() -> serde_json::Value {
        json!({
            "hypothesis_spec": {"spec_version": "hypothesis-spec-v1"},
            "missing_fields": [], "owner_returns": [], "framer_missing": [],
            "falsifiability_missing": [], "provenance_missing": [], "constraints_missing": []
        })
    }

    fn scientific_findings_env() -> serde_json::Value {
        json!({
            "design_valid": true, "measurement_valid": true,
            "prediction_contradicted": false, "precision_adequate": true,
            "equivalence_prespecified": false, "equivalence_met": false,
            "severe": false, "run_reports": ["fixture:report"]
        })
    }

    fn scientific_mapping_env(arm: &str) -> serde_json::Value {
        json!({
            "layer": "scientific_hypothesis", "design_spec_toplevel": true,
            "design_spec_in_eval_set": false, "spec_version_nonempty": true,
            "max_runs": 1, "rubric_kinds": ["metric", "implementation"],
            "feedback_partition_present": true, "selection_partition_present": true,
            "expected_headroom_arm": arm, "mapped_headroom_arm": arm,
            "meaningful_pass_rate_definition": true, "binary_resolution_present": true,
            "design_family_nonempty": true, "criterion_nonempty": true
        })
    }

    #[test]
    fn scientific_gate_ed0r_reconciles_owner_and_partition_sets() {
        let form = skill_candidate("experimental-design", "define unique-strings");
        let complete = scientific_normalization_env();
        assert_scientific_case(&form, "complete", &complete, json!(true));
        let missing = scientific_env(
            &complete,
            json!({
                "hypothesis_spec": null,
                "missing_fields": ["question", "rivals"],
                "owner_returns": [{"field": "question"}, {"field": "rivals"}],
                "framer_missing": ["question"], "falsifiability_missing": ["rivals"]
            }),
        );
        assert_scientific_case(&form, "all missing owners retained", &missing, json!(true));
        for (case, changes) in [
            (
                "missing fields with a nonnull spec",
                json!({"hypothesis_spec": {"spec_version": "hypothesis-spec-v1"}}),
            ),
            (
                "dropped owner",
                json!({"owner_returns": [{"field": "question"}]}),
            ),
            (
                "duplicate owner",
                json!({"owner_returns": [{"field": "question"}, {"field": "question"}]}),
            ),
            (
                "unknown owner field",
                json!({"owner_returns": [{"field": "question"}, {"field": "other"}]}),
            ),
            (
                "owner without a field",
                json!({"owner_returns": [{"field": "question"}, {}]}),
            ),
            (
                "duplicate missing field",
                json!({"missing_fields": ["question", "question"]}),
            ),
            (
                "overlapping partitions",
                json!({"falsifiability_missing": ["question"]}),
            ),
            (
                "unknown partition field",
                json!({"falsifiability_missing": ["other"]}),
            ),
            ("missing partition", json!({"falsifiability_missing": []})),
        ] {
            assert_scientific_case(
                &form,
                case,
                &scientific_env(&missing, changes),
                json!(false),
            );
        }
        assert_scientific_case(
            &form,
            "complete fields require a spec",
            &scientific_env(&complete, json!({"hypothesis_spec": null})),
            json!(false),
        );
    }

    #[test]
    fn scientific_gate_ed0_routes_to_the_earliest_missing_owner() {
        let form = skill_candidate("experimental-design", "blocked: malformed_normalization");
        let base = scientific_normalization_env();
        for (case, changes, expected) in [
            ("ready", json!({}), "ready"),
            (
                "framing",
                json!({"framer_missing": ["question"]}),
                "return_to_framer",
            ),
            (
                "falsifiability",
                json!({"falsifiability_missing": ["rivals"]}),
                "return_to_falsifiability",
            ),
            (
                "provenance",
                json!({"provenance_missing": ["source"]}),
                "blocked: provenance",
            ),
            (
                "constraints",
                json!({"constraints_missing": ["ethics"]}),
                "blocked: constraints",
            ),
            (
                "null spec",
                json!({"hypothesis_spec": null}),
                "blocked: malformed_normalization",
            ),
            (
                "framing precedes later owners",
                json!({
                    "framer_missing": ["question"], "falsifiability_missing": ["rivals"],
                    "provenance_missing": ["source"], "constraints_missing": ["ethics"]
                }),
                "return_to_framer",
            ),
        ] {
            assert_scientific_case(
                &form,
                case,
                &scientific_env(&base, changes),
                json!(expected),
            );
        }
    }

    #[test]
    fn scientific_gate_edc_bounds_candidates_and_requires_unique_ids() {
        let form = skill_candidate("experimental-design", "define unique-candidate-ids");
        for count in 0..=6 {
            let drafts = (0..count)
                .map(|id| json!({"candidate_id": format!("c{id}")}))
                .collect::<Vec<_>>();
            assert_scientific_case(
                &form,
                &format!("candidate count {count}"),
                &json!({
                    "candidate_drafts": drafts, "only_credible_explanation": null
                }),
                json!((2..=5).contains(&count)),
            );
        }
        for (case, drafts, explanation, expected) in [
            (
                "justified sole candidate",
                json!([{"candidate_id": "a"}]),
                json!("Other designs cannot identify this contrast"),
                true,
            ),
            (
                "empty sole justification",
                json!([{"candidate_id": "a"}]),
                json!(""),
                false,
            ),
            (
                "duplicate ids",
                json!([{"candidate_id": "a"}, {"candidate_id": "a"}]),
                json!(null),
                false,
            ),
            (
                "empty id",
                json!([{"candidate_id": "a"}, {"candidate_id": ""}]),
                json!(null),
                false,
            ),
            (
                "missing id",
                json!([{"candidate_id": "a"}, {}]),
                json!(null),
                false,
            ),
        ] {
            assert_scientific_case(
                &form,
                case,
                &json!({
                    "candidate_drafts": drafts, "only_credible_explanation": explanation
                }),
                json!(expected),
            );
        }
    }

    #[test]
    fn scientific_gate_ed2i_reconciles_findings_with_report_presence() {
        let form = skill_candidate("experimental-design", "(length run_reports)");
        let base = scientific_findings_env();
        for (case, changes, expected) in [
            ("reported observation", json!({}), true),
            (
                "contradiction",
                json!({"prediction_contradicted": true}),
                true,
            ),
            (
                "prespecified equivalence",
                json!({"equivalence_met": true, "equivalence_prespecified": true}),
                true,
            ),
            (
                "contradiction and equivalence",
                json!({"prediction_contradicted": true, "equivalence_met": true, "equivalence_prespecified": true}),
                false,
            ),
            (
                "post hoc equivalence",
                json!({"equivalence_met": true}),
                false,
            ),
            (
                "missing reports claimed valid",
                json!({"run_reports": []}),
                false,
            ),
            (
                "missing reports surfaced as invalid",
                json!({"run_reports": [], "measurement_valid": false}),
                true,
            ),
        ] {
            assert_scientific_case(
                &form,
                case,
                &scientific_env(&base, changes),
                json!(expected),
            );
        }
    }

    #[test]
    fn scientific_gate_ed2_distinguishes_outcomes_and_respects_precision() {
        let form = skill_candidate("experimental-design", "\"corroborated\"");
        let base = scientific_findings_env();
        for (case, changes, expected) in [
            ("neutral", json!({}), "inconclusive"),
            (
                "invalid design",
                json!({"design_valid": false, "prediction_contradicted": true}),
                "design_void",
            ),
            (
                "invalid measurement cannot refute",
                json!({"measurement_valid": false, "prediction_contradicted": true}),
                "measurement_void",
            ),
            (
                "precise contradiction",
                json!({"prediction_contradicted": true}),
                "rejected",
            ),
            (
                "imprecise contradiction",
                json!({"prediction_contradicted": true, "precision_adequate": false}),
                "inconclusive",
            ),
            (
                "equivalence",
                json!({"equivalence_prespecified": true, "equivalence_met": true}),
                "evidence_of_practical_equivalence",
            ),
            (
                "imprecise equivalence",
                json!({"equivalence_prespecified": true, "equivalence_met": true, "precision_adequate": false}),
                "inconclusive",
            ),
            (
                "severe precise test",
                json!({"severe": true}),
                "corroborated",
            ),
            (
                "imprecision cannot corroborate",
                json!({"severe": true, "precision_adequate": false}),
                "inconclusive",
            ),
        ] {
            assert_scientific_case(
                &form,
                case,
                &scientific_env(&base, changes),
                json!(expected),
            );
        }
    }

    #[test]
    fn scientific_gate_htr_prefers_binary_headroom_without_excluding_scientific_designs() {
        let form = skill_candidate("hypothesis-testing", "(let ((common");
        let base = json!({
            "meaningful_baseline": true, "rubric_expressible": true, "positive_max_runs": true,
            "partitions_valid": true, "design_spec_versioned": true, "executor_bound": true,
            "binary_headroom": true, "genuinely_binary_claim": true, "scientific_headroom_criterion": false
        });
        for (case, changes, expected) in [
            ("binary", json!({}), "compatible_binary"),
            (
                "both available prefers binary",
                json!({"scientific_headroom_criterion": true}),
                "compatible_binary",
            ),
            (
                "scientific without binary pass rate",
                json!({"binary_headroom": false, "scientific_headroom_criterion": true}),
                "compatible_scientific",
            ),
            (
                "nonbinary scientific claim",
                json!({"genuinely_binary_claim": false, "scientific_headroom_criterion": true}),
                "compatible_scientific",
            ),
            (
                "neither headroom",
                json!({"binary_headroom": false}),
                "registry_schema_mismatch",
            ),
            (
                "nonbinary pass-rate claim",
                json!({"genuinely_binary_claim": false}),
                "registry_schema_mismatch",
            ),
        ] {
            assert_scientific_case(
                &form,
                case,
                &scientific_env(&base, changes),
                json!(expected),
            );
        }
        for field in [
            "meaningful_baseline",
            "rubric_expressible",
            "positive_max_runs",
            "partitions_valid",
            "design_spec_versioned",
            "executor_bound",
        ] {
            let mut env = base.clone();
            env["scientific_headroom_criterion"] = json!(true);
            env[field] = json!(false);
            assert_scientific_case(&form, field, &env, json!("registry_schema_mismatch"));
        }
    }

    #[test]
    fn scientific_gate_htrm_validates_exact_arm_tokens_and_rendered_rubric_kinds() {
        let form = skill_candidate("hypothesis-testing", "define valid-rubric-kinds");
        for arm in ["compatible_binary", "compatible_scientific"] {
            let base = scientific_mapping_env(arm);
            assert_scientific_case(&form, arm, &base, json!(true));
            for (case, changes) in [
                ("wrong layer", json!({"layer": "skill"})),
                ("spec not top-level", json!({"design_spec_toplevel": false})),
                (
                    "dual-written spec",
                    json!({"design_spec_in_eval_set": true}),
                ),
                ("unversioned spec", json!({"spec_version_nonempty": false})),
                ("zero budget", json!({"max_runs": 0})),
                ("negative budget", json!({"max_runs": -1})),
                (
                    "missing feedback",
                    json!({"feedback_partition_present": false}),
                ),
                (
                    "missing selection",
                    json!({"selection_partition_present": false}),
                ),
                ("no rubrics", json!({"rubric_kinds": []})),
                ("no metric", json!({"rubric_kinds": ["implementation"]})),
                (
                    "no implementation evidence",
                    json!({"rubric_kinds": ["metric"]}),
                ),
                (
                    "unknown kind",
                    json!({"rubric_kinds": ["metric", "implementation", "other"]}),
                ),
                (
                    "cased kind",
                    json!({"rubric_kinds": ["metric", "Implementation"]}),
                ),
            ] {
                assert_scientific_case(&form, case, &scientific_env(&base, changes), json!(false));
            }
            assert_scientific_case(
                &form,
                "grounded report is valid evidence",
                &scientific_env(
                    &base,
                    json!({"rubric_kinds": ["metric", "grounded_report"]}),
                ),
                json!(true),
            );
            let (required, irrelevant) = if arm == "compatible_binary" {
                (
                    [
                        "meaningful_pass_rate_definition",
                        "binary_resolution_present",
                    ],
                    ["design_family_nonempty", "criterion_nonempty"],
                )
            } else {
                (
                    ["design_family_nonempty", "criterion_nonempty"],
                    [
                        "meaningful_pass_rate_definition",
                        "binary_resolution_present",
                    ],
                )
            };
            for field in required {
                let mut env = base.clone();
                env[field] = json!(false);
                assert_scientific_case(&form, field, &env, json!(false));
            }
            let mut env = base.clone();
            for field in irrelevant {
                env[field] = json!(false);
            }
            assert_scientific_case(
                &form,
                "other arm fields are not required",
                &env,
                json!(true),
            );
            let other = if arm == "compatible_binary" {
                "compatible_scientific"
            } else {
                "compatible_binary"
            };
            assert_scientific_case(
                &form,
                "mapper must not switch arms",
                &scientific_env(&base, json!({"mapped_headroom_arm": other})),
                json!(false),
            );
        }
        for token in [
            "registry_schema_mismatch",
            "binary",
            "scientific",
            "",
            "compatible_scientfic",
        ] {
            let env = scientific_mapping_env(token);
            assert_scientific_case(
                &form,
                "matching unknown tokens still refuse",
                &env,
                json!(false),
            );
        }
    }

    /// The one form pinned under the `### {heading}` entry of the
    /// kask-seam-audit SKILL.md Gates section.
    fn ksa_gate(heading: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../.agents/skills/kask-seam-audit/SKILL.md");
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let section = content
            .split("\n### ")
            .find(|section| section.starts_with(heading))
            .unwrap_or_else(|| panic!("SKILL.md has no gate entry starting {heading:?}"));
        let forms = extract_candidates(section);
        assert_eq!(forms.len(), 1, "{heading}: expected one pinned form");
        forms.into_iter().next().expect("one form checked").source
    }

    fn ksa_eval(heading: &str, case: &str, env: serde_json::Value) -> serde_json::Value {
        eval_sandboxed(&ksa_gate(heading), &env)
            .unwrap_or_else(|error| panic!("{heading} / {case}: gate failed to evaluate: {error}"))
    }

    #[test]
    fn scientific_gate_ksa_gate_b_requires_citation_or_reasoned_deferral() {
        let clean = json!({
            "sec_findings": [
                {"id": "KS-01", "severity": "high", "file_line": "kask/a.rs:10"},
                {"id": "KS-02", "severity": "low", "deferred": true, "deferred_reason": "upstream non-D-seam"}
            ],
            "arch_findings": [{"id": "RA-01", "severity": "info", "file_line": "kask/b.rs:3"}],
            "ui_findings": [{"id": "UI-01", "severity": "medium", "file_line": "crates/x.rs:9"}]
        });
        let out = ksa_eval("Gate B", "clean", clean.clone());
        assert_eq!(out["passed"], json!(true));
        assert_eq!(out["per_track"], json!([2, 1, 1]));
        assert_eq!(out["per_severity"], json!([0, 1, 1, 1, 1]));

        let empty = json!({"sec_findings": [], "arch_findings": [], "ui_findings": []});
        assert_eq!(
            ksa_eval("Gate B", "empty tracks", empty)["passed"],
            json!(true)
        );

        for (case, track, finding, code) in [
            (
                "deferred without a reason",
                "sec_findings",
                json!({"id": "X-1", "severity": "high", "deferred": true}),
                "uncited_and_not_deferred",
            ),
            (
                "empty citation",
                "sec_findings",
                json!({"id": "X-1", "severity": "high", "file_line": ""}),
                "uncited_and_not_deferred",
            ),
            (
                "unknown severity",
                "arch_findings",
                json!({"id": "X-1", "severity": "severe", "file_line": "kask/a.rs:1"}),
                "invalid_severity",
            ),
            (
                "critical on the UI track",
                "ui_findings",
                json!({"id": "X-1", "severity": "critical", "file_line": "crates/x.rs:1"}),
                "invalid_severity",
            ),
            (
                "id reused across tracks",
                "arch_findings",
                json!({"id": "KS-01", "severity": "low", "file_line": "kask/c.rs:1"}),
                "duplicate_id",
            ),
        ] {
            let mut env = clean.clone();
            env[track]
                .as_array_mut()
                .expect("track list")
                .push(finding.clone());
            let out = ksa_eval("Gate B", case, env);
            assert_eq!(out["passed"], json!(false), "{case}");
            let id = finding["id"].as_str().expect("fixture id");
            assert_eq!(out["defects"][id], json!(code), "{case}");
        }
    }

    #[test]
    fn scientific_gate_ksa_gate_fa_enforces_retirement_and_coverage() {
        let findings = json!([{"id": "KS-01"}, {"id": "RA-01"}]);
        let annotated = |status: &str, retire: bool| {
            json!([
                {"id": "KS-01", "current_function": "governed MCP runtime", "authority_status": status, "retire": retire},
                {"id": "RA-01", "current_function": "dead helper", "authority_status": "supported", "retire": false}
            ])
        };
        let clean = ksa_eval(
            "Gate FA",
            "supported",
            json!({"findings": findings, "annotated_findings": annotated("supported", false)}),
        );
        assert_eq!(clean["passed"], json!(true));
        assert_eq!(clean["convergence_score"], json!(0));

        let retired = ksa_eval(
            "Gate FA",
            "revoked and retired",
            json!({"findings": findings, "annotated_findings": annotated("revoked", true)}),
        );
        assert_eq!(retired["passed"], json!(true));

        for status in ["revoked", "deprecated"] {
            let kept = ksa_eval(
                "Gate FA",
                "revoked but kept",
                json!({"findings": findings, "annotated_findings": annotated(status, false)}),
            );
            assert_eq!(kept["passed"], json!(false), "{status}");
            assert_eq!(
                kept["defects"]["KS-01"],
                json!("revoked_function_not_retired")
            );
            assert_eq!(kept["convergence_score"], json!(1));
        }

        let bad_status = ksa_eval(
            "Gate FA",
            "unknown status",
            json!({"findings": findings, "annotated_findings": annotated("maybe", false)}),
        );
        assert_eq!(
            bad_status["defects"]["KS-01"],
            json!("invalid_authority_status")
        );

        let uncovered = ksa_eval(
            "Gate FA",
            "finding with no annotation",
            json!({
                "findings": [{"id": "KS-01"}, {"id": "RA-01"}, {"id": "UI-01"}],
                "annotated_findings": annotated("supported", false)
            }),
        );
        assert_eq!(uncovered["passed"], json!(false));
        assert_eq!(uncovered["unadjudicated"], json!(["UI-01"]));
        assert_eq!(uncovered["convergence_score"], json!(1));
    }

    #[test]
    fn scientific_gate_ksa_2_weights_eligibility_and_ranking() {
        let candidate = |id: &str, scores: [f64; 4], deepens: bool, deletion: &str| json!({"id": id, "scores": scores, "deepens_module": deepens, "deletion_test": deletion});
        let candidates = json!([
            candidate("A", [1.0, 0.0, 0.0, 0.0], false, "n/a"),
            candidate("B", [0.0, 1.0, 0.0, 0.0], false, "complexity_vanishes"),
            candidate("C", [0.0, 0.0, 1.0, 0.0], true, "complexity_reappears"),
            candidate("D", [1.0, 1.0, 1.0, 1.0], false, "unclear"),
            candidate("E", [1.0, 1.0, 1.0, 1.0], false, "complexity_reappears"),
            candidate("F", [0.0, 0.0, 0.0, 1.0], false, "n/a")
        ]);
        for (priority, weights, top) in [
            ("security", [0.35, 0.25, 0.25, 0.15], ["A", "B", "C"]),
            ("codebase", [0.25, 0.4, 0.2, 0.15], ["B", "A", "C"]),
            ("ui", [0.25, 0.2, 0.4, 0.15], ["C", "A", "B"]),
        ] {
            let out = ksa_eval(
                "KSA-2",
                priority,
                json!({"operator_priority": priority, "candidates": candidates}),
            );
            assert_eq!(out["weights"], json!(weights), "{priority}");
            let sum: f64 = weights.iter().sum();
            assert!((sum - 1.0).abs() < 1e-9, "{priority} weights sum to {sum}");
            // D (unclear) and E (reappears, not deepening) outscore everything
            // and are still never ranked.
            assert_eq!(out["excluded"], json!(["D", "E"]), "{priority}");
            assert_eq!(out["top3"], json!(top), "{priority}");
        }
        let unknown = ksa_eval(
            "KSA-2",
            "unknown priority",
            json!({"operator_priority": "speed", "candidates": candidates}),
        );
        assert_eq!(unknown["error"], json!("unknown_operator_priority"));

        // A +20% codebase weight (0.25 -> 0.30) lifts Y (0.30) past X (0.28).
        let fragile = ksa_eval(
            "KSA-2",
            "fragile ranking",
            json!({"operator_priority": "security", "candidates": [
                candidate("P", [1.0, 1.0, 1.0, 1.0], false, "n/a"),
                candidate("Q", [1.0, 1.0, 1.0, 0.0], false, "n/a"),
                candidate("X", [0.8, 0.0, 0.0, 0.0], false, "n/a"),
                candidate("Y", [0.0, 1.0, 0.0, 0.0], false, "n/a")
            ]}),
        );
        assert_eq!(fragile["top3"], json!(["P", "Q", "X"]));
        assert_eq!(fragile["stable_under_perturbation"], json!(false));
        let stable = ksa_eval(
            "KSA-2",
            "three candidates",
            json!({"operator_priority": "security", "candidates": [
                candidate("P", [1.0, 0.0, 0.0, 0.0], false, "n/a"),
                candidate("Q", [0.0, 1.0, 0.0, 0.0], false, "n/a")
            ]}),
        );
        assert_eq!(stable["stable_under_perturbation"], json!(true));
    }

    #[test]
    fn scientific_gate_ksa_gate_c_computes_the_seam_boundary() {
        let env = |remediations: serde_json::Value, hard_stop: bool| {
            json!({
                "applied_remediations": remediations,
                "top3": ["KS-01", "RA-01"],
                "divergence_surface": ["crates/zed/src/main.rs", "crates/hkask-steer/"],
                "seam_scope": "kask/",
                "hard_stop_triggered": hard_stop
            })
        };
        let applied = |id: &str, files: serde_json::Value, test_added: bool| json!({"id": id, "touched_files": files, "test_added": test_added});
        let allowed = json!([
            "kask/crates/hkask-types/src/voice.rs",
            "crates/zed/src/main.rs",
            "crates/hkask-steer/src/lib.rs"
        ]);
        let clean = ksa_eval(
            "Gate C",
            "clean",
            env(json!([applied("KS-01", allowed.clone(), true)]), false),
        );
        assert_eq!(clean["passed"], json!(true));
        assert_eq!(clean["hard_stop_required"], json!(false));
        assert_eq!(
            ksa_eval("Gate C", "nothing applied", env(json!([]), false))["passed"],
            json!(true)
        );

        // An upstream file is outside the seam whatever the model declared;
        // a path that merely shares a prefix with a surface file is not on it.
        for path in [
            "crates/editor/src/editor.rs",
            "crates/zed/src/main.rs.bak",
            "crates/hkask-steerage/x.rs",
        ] {
            let out = ksa_eval(
                "Gate C",
                path,
                env(json!([applied("KS-01", json!([path]), true)]), false),
            );
            assert_eq!(out["passed"], json!(false), "{path}");
            assert_eq!(
                out["defects"]["KS-01"],
                json!("touched_file_outside_seam"),
                "{path}"
            );
            assert_eq!(out["outside_seam"], json!([path]), "{path}");
            assert_eq!(out["hard_stop_required"], json!(true), "{path}");
        }
        let untested = ksa_eval(
            "Gate C",
            "no test",
            env(json!([applied("KS-01", allowed.clone(), false)]), false),
        );
        assert_eq!(
            untested["defects"]["KS-01"],
            json!("remediation_missing_test")
        );
        let unranked = ksa_eval(
            "Gate C",
            "not top ranked",
            env(json!([applied("UI-09", allowed, true)]), false),
        );
        assert_eq!(unranked["defects"]["UI-09"], json!("not_top_ranked"));
        let stopped = ksa_eval("Gate C", "declared hard stop", env(json!([]), true));
        assert_eq!(stopped["passed"], json!(false));
    }

    #[test]
    fn scientific_gate_ksa_1_derives_layout_gates_and_probes() {
        let child = |element: &str, width: u32, kind: &str, primary: bool, label: bool| {
            json!({"element": element, "min_width_px": width, "kind": kind,
                   "primary_action": primary, "is_label": label})
        };
        let base = json!({
            "container_outer_width": 400, "padding_px": [12, 12], "border_px": 2,
            "gap_px": [4, 4],
            "child_min_widths": [
                child("name", 0, "text_column", false, false),
                child("Details", 60, "fixed", true, true),
                child("menu", 32, "fixed", true, false)
            ],
            "fitts_min_target": 32, "primary_action_visible": true
        });
        // available 400 - 24 - 2 = 374; children 92 + gaps 8 = 100.
        let out = ksa_eval("KSA-1", "fits", base.clone());
        assert_eq!(out["all_gates_passed"], json!(true));
        assert_eq!(out["failing_gates"], json!([]));
        assert_eq!(out["container_available_width"], json!(374));
        assert_eq!(out["sum_min_widths"], json!(100));
        assert_eq!(out["overflow_delta"], json!(-274));
        assert_eq!(out["text_column_residual"], json!(274));
        assert_eq!(out["action_count"], json!(2));
        assert_eq!(out["fitts_violations"], json!([]));
        assert_eq!(out["probe_narrow_300"], json!([]));
        assert_eq!(out["probe_labels_plus_30pct"], json!([]));

        let with = |changes: serde_json::Value| scientific_env(&base, changes);
        // Four fixed 60px labels + a 0px text column: 240 + gaps 16 = 256 of
        // 374 leaves a 118px text column; at 300px outer (274) it still fits
        // without overflow; 30% longer labels (312 + 16) still fit 374.
        let crowded = ksa_eval(
            "KSA-1",
            "text column squeezed",
            with(json!({"gap_px": [4, 4, 4, 4], "child_min_widths": [
                child("name", 0, "text_column", false, false),
                child("a", 60, "fixed", true, true), child("b", 60, "fixed", true, true),
                child("c", 60, "fixed", true, true), child("d", 60, "fixed", true, true)
            ]})),
        );
        assert_eq!(crowded["failing_gates"], json!(["text_column_min_width"]));
        assert_eq!(crowded["text_column_residual"], json!(118));

        // 374 available; six fixed 60px labels = 360 + gaps 20 = 380.
        let overflow = ksa_eval(
            "KSA-1",
            "overflow and over budget",
            with(json!({"gap_px": [4, 4, 4, 4, 4], "child_min_widths": [
                child("a", 60, "fixed", true, true), child("b", 60, "fixed", true, true),
                child("c", 60, "fixed", true, true), child("d", 60, "fixed", true, true),
                child("e", 60, "fixed", true, true), child("f", 60, "fixed", true, true)
            ]})),
        );
        assert_eq!(overflow["overflow_delta"], json!(6));
        assert_eq!(overflow["action_count"], json!(6));
        assert_eq!(
            overflow["failing_gates"],
            json!([
                "no_overflow",
                "text_column_min_width",
                "action_count_budget"
            ])
        );

        let off_grid = ksa_eval("KSA-1", "off-grid gap", with(json!({"gap_px": [4, 5]})));
        assert_eq!(off_grid["failing_gates"], json!(["on_grid_spacing"]));
        let no_primary = ksa_eval(
            "KSA-1",
            "no primary",
            with(json!({"primary_action_visible": false})),
        );
        assert_eq!(
            no_primary["failing_gates"],
            json!(["primary_action_visible"])
        );
        let small = ksa_eval(
            "KSA-1",
            "small target",
            with(json!({"child_min_widths": [
                child("name", 0, "text_column", false, false),
                child("icon", 24, "fixed", true, false)
            ]})),
        );
        assert_eq!(small["fitts_violations"], json!(["icon"]));
        assert_eq!(
            small["all_gates_passed"],
            json!(true),
            "Fitts is reported, not one of the five gates"
        );

        // Fits at 400 (374 available): 100 + 100 fixed + gap 4 = 204, text
        // column 170. At 300 outer (274 available) the text column is 70.
        // With labels 30% wider (260 + 4) the text column is 110.
        let probes = ksa_eval(
            "KSA-1",
            "passes now, fails both probes",
            with(json!({"gap_px": [4], "child_min_widths": [
                child("name", 0, "text_column", false, false),
                child("Publish", 100, "fixed", true, true),
                child("Clone", 100, "fixed", true, true)
            ]})),
        );
        assert_eq!(probes["all_gates_passed"], json!(true));
        assert_eq!(probes["probe_narrow_300"], json!(["text_column_min_width"]));
        assert_eq!(
            probes["probe_labels_plus_30pct"],
            json!(["text_column_min_width"])
        );
    }

    #[test]
    fn scientific_gate_ksa_4_accepts_only_a_fully_probed_layout() {
        let probe = |name: &str, passed: bool, broken: serde_json::Value| json!({"probe_name": name, "input": "fixture", "passed": passed, "broken_gates": broken});
        let all_passing = json!([
            probe("long_label", true, json!([])),
            probe("high_action_count", true, json!([])),
            probe("empty_description", true, json!([]))
        ]);
        let base = json!({
            "failing_gates": [], "probe_narrow_300": [], "probe_labels_plus_30pct": [],
            "probes": all_passing, "hierarchy_present": true
        });
        // The case the former gate could never pass: every probe passing.
        let out = ksa_eval("KSA-4", "all passing", base.clone());
        assert_eq!(out["accepted"], json!(true));
        assert_eq!(out["failing_gates"], json!([]));

        let with = |changes: serde_json::Value| scientific_env(&base, changes);
        let failed_probe = ksa_eval(
            "KSA-4",
            "a judgment probe fails",
            with(json!({"probes": [
                probe("long_label", false, json!(["no_overflow", "text_column_min_width"])),
                probe("high_action_count", false, json!(["no_overflow"])),
                probe("empty_description", true, json!([]))
            ]})),
        );
        assert_eq!(failed_probe["accepted"], json!(false));
        assert_eq!(
            failed_probe["failing_probes"],
            json!(["long_label", "high_action_count"])
        );
        assert_eq!(
            failed_probe["failing_gates"],
            json!(["text_column_min_width", "no_overflow"]),
            "each broken gate once"
        );

        let missing = ksa_eval(
            "KSA-4",
            "a probe was not run",
            with(json!({"probes": [probe("long_label", true, json!([]))]})),
        );
        assert_eq!(missing["accepted"], json!(false));
        assert_eq!(
            missing["missing_probes"],
            json!(["high_action_count", "empty_description"])
        );

        for (case, changes) in [
            (
                "a gate still fails",
                json!({"failing_gates": ["no_overflow"]}),
            ),
            (
                "the 300px probe fails",
                json!({"probe_narrow_300": ["text_column_min_width"]}),
            ),
            (
                "the longer-label probe fails",
                json!({"probe_labels_plus_30pct": ["no_overflow"]}),
            ),
            ("no visual hierarchy", json!({"hierarchy_present": false})),
        ] {
            assert_eq!(
                ksa_eval("KSA-4", case, with(changes))["accepted"],
                json!(false),
                "{case}"
            );
        }
    }

    #[test]
    fn scientific_gate_pc_1_decides_viability_from_the_analyses_as_returned() {
        let form = skill_candidate("pragmatic-cybernetics", "define broken-value ");
        let base = json!({
            "loop_analysis": {
                "polarity": "balancing", "delay": "short", "gain": "proportional",
                "closure": "closed", "fidelity": "high",
                "broken_loop_diagnosis": "", "remediation": ""
            },
            "vsm_mapping": {
                "S1_operations": ["agent execution"], "S2_coordination": ["MCP dispatch"],
                "S3_control": ["call caps"], "S4_intelligence": ["spec drift"],
                "S5_policy": ["Magna Carta"]
            },
            "algedonic_channel": true,
            "disturbance_classes": ["d1", "d2", "d3"],
            "response_classes": ["r1", "r2", "r3"],
            "observed_disturbance_classes": ["d1", "d2"],
            "critical_threshold": 2
        });
        let eval = |case: &str, changes: serde_json::Value| {
            let env = scientific_env(&base, changes);
            eval_sandboxed(&form, &env)
                .unwrap_or_else(|error| panic!("{case}: PC-1 failed to evaluate: {error}"))
        };
        let loop_with = |property: &str, value: &str| {
            let mut analysis = base["loop_analysis"].clone();
            analysis[property] = json!(value);
            json!({"loop_analysis": analysis})
        };

        let viable = eval("viable", json!({}));
        assert_eq!(viable["verdict"], json!("viable"));
        assert_eq!(viable["broken_properties"], json!([]));
        assert_eq!(viable["uncovered_subsystems"], json!([]));
        assert_eq!(viable["system_variety"], json!(3));
        assert_eq!(viable["regulator_variety"], json!(3));
        assert_eq!(viable["deficit"], json!(0));
        assert_eq!(viable["ashby_status"], json!("satisfied"));

        // Each property's broken value is a broken loop and is named.
        for (property, value) in [
            ("polarity", "none"),
            ("delay", "excessive"),
            ("gain", "zero"),
            ("closure", "open"),
            ("fidelity", "low"),
        ] {
            let out = eval(property, loop_with(property, value));
            assert_eq!(out["verdict"], json!("broken_loop"), "{property}={value}");
            assert_eq!(
                out["broken_properties"],
                json!([property]),
                "{property}={value}"
            );
        }
        // A degraded value is not broken. "excessive" is delay's broken
        // value; on gain it is overshoot, which the template rates degraded.
        for (property, value) in [
            ("polarity", "reinforcing"),
            ("delay", "moderate"),
            ("gain", "weak"),
            ("gain", "excessive"),
            ("closure", "partial"),
            ("fidelity", "moderate"),
        ] {
            let out = eval(property, loop_with(property, value));
            assert_eq!(out["verdict"], json!("viable"), "{property}={value}");
            assert_eq!(out["broken_properties"], json!([]), "{property}={value}");
        }

        let blocked = eval("blocked channel", json!({"algedonic_channel": false}));
        assert_eq!(blocked["verdict"], json!("unviable"));
        // The case the former Convergence form passed as viable: a
        // subsystem with no component.
        let mut uncovered_mapping = base["vsm_mapping"].clone();
        uncovered_mapping["S2_coordination"] = json!([]);
        uncovered_mapping["S4_intelligence"] = json!([]);
        let uncovered = eval(
            "empty subsystems",
            json!({"vsm_mapping": uncovered_mapping}),
        );
        assert_eq!(uncovered["verdict"], json!("unviable"));
        assert_eq!(
            uncovered["uncovered_subsystems"],
            json!(["S2_coordination", "S4_intelligence"])
        );
        // Unviable is not masked by a variety deficit, and a broken loop
        // is not masked by either.
        let masked = eval(
            "blocked channel and deficit",
            json!({"algedonic_channel": false, "response_classes": ["r1"]}),
        );
        assert_eq!(masked["verdict"], json!("unviable"));
        let mut broken_and_blocked = loop_with("closure", "open");
        broken_and_blocked["algedonic_channel"] = json!(false);
        assert_eq!(
            eval("broken loop and blocked channel", broken_and_blocked)["verdict"],
            json!("broken_loop")
        );

        // The other case the former form passed as viable: an idle window.
        let idle = eval("idle window", json!({"observed_disturbance_classes": []}));
        assert_eq!(idle["verdict"], json!("idle_no_signal"));
        assert_eq!(idle["ashby_status"], json!("idle_no_signal"));

        // Deficit 2 at threshold 2 is a deficit; deficit 3 is critical.
        let deficit = eval("deficit at threshold", json!({"response_classes": ["r1"]}));
        assert_eq!(deficit["verdict"], json!("variety_deficit"));
        assert_eq!(deficit["deficit"], json!(2));
        assert_eq!(deficit["ashby_status"], json!("deficit"));
        let critical = eval("deficit above threshold", json!({"response_classes": []}));
        assert_eq!(critical["verdict"], json!("variety_deficit"));
        assert_eq!(critical["deficit"], json!(3));
        assert_eq!(critical["ashby_status"], json!("critical"));
        // A domain with no threshold never reports critical.
        let unthresholded = eval(
            "no threshold",
            json!({"response_classes": [], "critical_threshold": 0}),
        );
        assert_eq!(unthresholded["ashby_status"], json!("deficit"));
        // Surplus regulator variety satisfies the law.
        let surplus = eval(
            "regulator surplus",
            json!({"response_classes": ["r1", "r2", "r3", "r4"]}),
        );
        assert_eq!(surplus["verdict"], json!("viable"));
        assert_eq!(surplus["deficit"], json!(-1));
    }

    #[test]
    fn scientific_gate_ps_1_applies_modifiers_once_then_the_provenance_caps() {
        let form = skill_candidate("pragmatic-semantics", "domain_anchored");
        let eval = |base: f64, provenance: &str, flags: [bool; 4]| -> f64 {
            eval_sandboxed(
                &form,
                &json!({
                    "base_confidence": base, "provenance": provenance,
                    "domain_anchored": flags[0], "sumo_anchored": flags[1],
                    "spec_checked": flags[2], "unanchored": flags[3]
                }),
            )
            .unwrap_or_else(|error| panic!("PS-1 ({provenance}, {base}): {error}"))
            .as_f64()
            .expect("PS-1 returns a number")
        };
        let close = |actual: f64, expected: f64, case: &str| {
            assert!(
                (actual - expected).abs() < 1e-9,
                "{case}: {actual} != {expected}"
            );
        };
        let none = [false, false, false, false];
        close(eval(0.7, "runtime", none), 0.7, "no modifier");
        close(
            eval(0.7, "runtime", [true, false, false, false]),
            0.75,
            "domain anchor",
        );
        close(
            eval(0.7, "runtime", [false, true, false, false]),
            0.73,
            "SUMO anchor",
        );
        close(
            eval(0.7, "runtime", [false, false, false, true]),
            0.6,
            "unanchored",
        );
        close(
            eval(0.99, "runtime", [true, false, false, false]),
            1.0,
            "upper clamp",
        );
        close(
            eval(0.05, "runtime", [false, false, false, true]),
            0.0,
            "lower clamp",
        );
        // Unknown provenance is capped at 0.3 however confident the base.
        close(
            eval(0.9, "unknown", [true, false, false, false]),
            0.3,
            "unknown cap",
        );
        close(
            eval(0.2, "unknown", none),
            0.2,
            "unknown below the cap is untouched",
        );
        // A specification that was not checked is capped like unknown;
        // a checked one is held at 0.8 or above.
        close(
            eval(0.9, "specification", none),
            0.3,
            "unchecked specification",
        );
        close(
            eval(0.5, "specification", [false, false, true, false]),
            0.8,
            "checked floor",
        );
        close(
            eval(0.9, "specification", [false, false, true, false]),
            0.95,
            "checked above floor",
        );
    }

    #[test]
    fn scientific_gate_ps_2_ranks_by_strict_domination() {
        let form = skill_candidate("pragmatic-semantics", "define anchor-rank");
        let statement =
            |id: &str, mode: &str, epistemic: &str, force: &str, provenance: &str, anchor: &str| {
                json!({
                    "id": id, "statement": "fixture", "ontological_mode": mode,
                    "epistemic_mode": epistemic, "constraint_force": force,
                    "provenance": provenance, "ontology_anchor": anchor
                })
            };
        let rank = |case: &str, statements: serde_json::Value| {
            eval_sandboxed(&form, &json!({"statements": statements}))
                .unwrap_or_else(|error| panic!("{case}: PS-2 failed to evaluate: {error}"))
        };

        // Tier 1 dominates: a subjunctive, unanchored OUGHT of unknown
        // provenance beats a declarative, specification-backed IS.
        let ought_beats_is = rank(
            "OUGHT over IS",
            json!([
                statement(
                    "is",
                    "IS",
                    "declarative",
                    "Evidence",
                    "specification",
                    "omc:Asset"
                ),
                statement(
                    "ought",
                    "OUGHT",
                    "subjunctive",
                    "Hypothesis",
                    "unknown",
                    "unanchored"
                )
            ]),
        );
        assert_eq!(ought_beats_is["outcome"], json!("ranked"));
        assert_eq!(ought_beats_is["winner"], json!("ought"));
        assert_eq!(ought_beats_is["winner_ranks"], json!([1, 3, 5, 8, 9]));

        // Each lower tier decides only when every higher tier is equal.
        for (case, a, b, winner) in [
            (
                "tier 2",
                statement(
                    "a",
                    "OUGHT",
                    "probabilistic",
                    "Prohibition",
                    "specification",
                    "omc:X",
                ),
                statement(
                    "b",
                    "OUGHT",
                    "declarative",
                    "Hypothesis",
                    "unknown",
                    "unanchored",
                ),
                "b",
            ),
            (
                "tier 3",
                statement(
                    "a",
                    "OUGHT",
                    "declarative",
                    "Guideline",
                    "specification",
                    "omc:X",
                ),
                statement(
                    "b",
                    "OUGHT",
                    "declarative",
                    "Guardrail",
                    "unknown",
                    "unanchored",
                ),
                "b",
            ),
            (
                "tier 4: external ranks below memory and above inference",
                statement("a", "IS", "declarative", "Evidence", "inference", "omc:X"),
                statement(
                    "b",
                    "IS",
                    "declarative",
                    "Evidence",
                    "external",
                    "unanchored",
                ),
                "b",
            ),
            (
                "tier 4: memory over external",
                statement("a", "IS", "declarative", "Evidence", "memory", "unanchored"),
                statement("b", "IS", "declarative", "Evidence", "external", "omc:X"),
                "a",
            ),
            (
                "tier 5: domain supplement over SUMO",
                statement(
                    "a",
                    "IS",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "sumo:Process",
                ),
                statement(
                    "b",
                    "IS",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "sepio:Assertion",
                ),
                "b",
            ),
            (
                "tier 5: core over unanchored",
                statement(
                    "a",
                    "IS",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "unanchored",
                ),
                statement("b", "IS", "declarative", "Evidence", "runtime", "5w1h:What"),
                "b",
            ),
        ] {
            let out = rank(case, json!([a, b]));
            assert_eq!(out["outcome"], json!("ranked"), "{case}");
            assert_eq!(out["winner"], json!(winner), "{case}");
        }

        // Three statements: the winner is found wherever it sits.
        let three = rank(
            "three statements",
            json!([
                statement(
                    "low",
                    "IS",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "5w1h:What"
                ),
                statement(
                    "top",
                    "OUGHT",
                    "declarative",
                    "Guardrail",
                    "design",
                    "pko:Step"
                ),
                statement(
                    "mid",
                    "OUGHT",
                    "probabilistic",
                    "Guideline",
                    "design",
                    "pko:Step"
                )
            ]),
        );
        assert_eq!(three["winner"], json!("top"));

        // Two Prohibitions escalate even though one outranks the other.
        let prohibitions = rank(
            "two Prohibitions",
            json!([
                statement(
                    "p1",
                    "OUGHT",
                    "declarative",
                    "Prohibition",
                    "specification",
                    "omc:X"
                ),
                statement(
                    "p2",
                    "OUGHT",
                    "declarative",
                    "Prohibition",
                    "design",
                    "omc:X"
                )
            ]),
        );
        assert_eq!(prohibitions["outcome"], json!("escalate_prohibitions"));

        // Equal on all five tiers (dcterms and bibo share a rank): a tie.
        let tie = rank(
            "five-tier tie",
            json!([
                statement(
                    "t1",
                    "IS",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "dcterms:creator"
                ),
                statement(
                    "t2",
                    "IS",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "bibo:Article"
                )
            ]),
        );
        assert_eq!(tie["outcome"], json!("escalate_tie"));
        assert_eq!(tie["winner"], json!("t1"));
        assert_eq!(
            tie["tied"],
            json!(["t2"]),
            "the statements tied with the winner"
        );

        for (case, bad) in [
            (
                "unknown mode",
                statement(
                    "x",
                    "MAYBE",
                    "declarative",
                    "Evidence",
                    "runtime",
                    "5w1h:What",
                ),
            ),
            (
                "unknown force",
                statement("x", "IS", "declarative", "Rule", "runtime", "5w1h:What"),
            ),
            (
                "capitalised provenance",
                statement("x", "IS", "declarative", "Evidence", "Runtime", "5w1h:What"),
            ),
        ] {
            let out = rank(
                case,
                json!([
                    bad,
                    statement(
                        "ok",
                        "IS",
                        "declarative",
                        "Evidence",
                        "runtime",
                        "5w1h:What"
                    )
                ]),
            );
            assert_eq!(out["outcome"], json!("invalid_classification"), "{case}");
            assert_eq!(out["invalid"], json!(["x"]), "{case}");
        }

        // An anchor outside the Tier-5 ranking is a valid classification.
        // It never matters while a higher tier decides, and escalates when
        // tiers 1-4 tie.
        let decided_higher = rank(
            "unranked anchor, decided at tier 4",
            json!([
                statement(
                    "q",
                    "IS",
                    "declarative",
                    "Evidence",
                    "design",
                    "qb:Observation"
                ),
                statement("w", "IS", "declarative", "Evidence", "runtime", "omc:Asset")
            ]),
        );
        assert_eq!(decided_higher["outcome"], json!("ranked"));
        assert_eq!(decided_higher["winner"], json!("q"));
        for first in ["q", "w"] {
            let (a, b) = if first == "q" {
                ("qb:Observation", "5w1h:What")
            } else {
                ("5w1h:What", "qb:Observation")
            };
            let out = rank(
                "unranked anchor, tiers 1-4 tie",
                json!([
                    statement("first", "IS", "declarative", "Evidence", "runtime", a),
                    statement("second", "IS", "declarative", "Evidence", "runtime", b)
                ]),
            );
            assert_eq!(out["outcome"], json!("escalate_unranked_anchor"), "{first}");
            assert_eq!(out["unranked_anchor"], json!(["second"]), "{first}");
        }

        let single = rank(
            "one statement",
            json!([statement(
                "only",
                "IS",
                "declarative",
                "Evidence",
                "runtime",
                "5w1h:What"
            )]),
        );
        assert_eq!(single["outcome"], json!("no_conflict"));
        assert_eq!(
            rank("no statements", json!([]))["outcome"],
            json!("no_conflict")
        );
    }

    #[test]
    fn scientific_gate_ps_3_names_unrouted_and_unnamed_steps() {
        let form = skill_candidate("pragmatic-semantics", "define regimes");
        let check = |case: &str, tags: serde_json::Value| {
            eval_sandboxed(&form, &json!({"tags": tags}))
                .unwrap_or_else(|error| panic!("{case}: PS-3 failed to evaluate: {error}"))
        };
        let clean = check(
            "clean table",
            json!([
                {"step": "1", "regime": "D", "oracle": "lisp_eval"},
                {"step": "2", "regime": "P:propose-verify", "oracle": "PS-2"},
                {"step": "3", "regime": "P:forecast", "oracle": "resolved outcomes"}
            ]),
        );
        assert_eq!(clean["passed"], json!(true));
        assert_eq!(clean["d_steps"], json!(1));
        assert_eq!(clean["p_steps"], json!(2));

        let unrouted = check(
            "unknown regime",
            json!([
                {"step": "1", "regime": "D", "oracle": "cargo"},
                {"step": "2", "regime": "P", "oracle": "a gate"}
            ]),
        );
        assert_eq!(unrouted["passed"], json!(false));
        assert_eq!(unrouted["unrouted"], json!(["2"]));

        // A D tag with no oracle fails, as a P tag with no gate does.
        let unnamed = check(
            "no oracle",
            json!([
                {"step": "1", "regime": "D", "oracle": ""},
                {"step": "2", "regime": "P:probabilistic", "oracle": ""},
                {"step": "3", "regime": "D", "oracle": "lean_check"}
            ]),
        );
        assert_eq!(unnamed["passed"], json!(false));
        assert_eq!(unnamed["unnamed_oracle"], json!(["1", "2"]));

        assert_eq!(check("empty table", json!([]))["passed"], json!(false));
    }

    #[test]
    fn scientific_gate_ps_convergence_covers_all_three_analyses() {
        let form = skill_candidate("pragmatic-semantics", "classification_provenance_unknown");
        let base = json!({
            "classification_provenance": "implementation", "unverifiable_gaps": [],
            "chain_confidence": 0.85, "conflict_outcome": "no_conflict"
        });
        let gate = |case: &str, changes: serde_json::Value| {
            eval_sandboxed(&form, &scientific_env(&base, changes))
                .unwrap_or_else(|error| panic!("{case}: convergence failed to evaluate: {error}"))
        };
        assert_eq!(gate("clean", json!({}))["converged"], json!(true));
        assert_eq!(
            gate("ranked conflict", json!({"conflict_outcome": "ranked"}))["converged"],
            json!(true)
        );
        assert_eq!(
            gate("at the threshold", json!({"chain_confidence": 0.8}))["converged"],
            json!(true)
        );

        for (case, changes, reason) in [
            (
                "a gap",
                json!({"unverifiable_gaps": ["missing spec"]}),
                "unverifiable_gaps",
            ),
            (
                "low confidence",
                json!({"chain_confidence": 0.79}),
                "chain_confidence_below_0.8",
            ),
            // The two the former gate passed over a gap-free trace.
            (
                "unknown provenance",
                json!({"classification_provenance": "unknown"}),
                "classification_provenance_unknown",
            ),
            (
                "skipped conflict step",
                json!({"conflict_outcome": "skipped"}),
                "conflict_resolution_skipped",
            ),
            (
                "escalated tie",
                json!({"conflict_outcome": "escalate_tie"}),
                "conflict_unresolved",
            ),
            (
                "two Prohibitions",
                json!({"conflict_outcome": "escalate_prohibitions"}),
                "conflict_unresolved",
            ),
            (
                "invalid classification",
                json!({"conflict_outcome": "invalid_classification"}),
                "conflict_unresolved",
            ),
        ] {
            let out = gate(case, changes);
            assert_eq!(out["converged"], json!(false), "{case}");
            assert_eq!(out["reasons"], json!([reason]), "{case}");
        }
        let several = gate(
            "every condition fails",
            json!({
                "classification_provenance": "unknown", "unverifiable_gaps": ["g"],
                "chain_confidence": 0.1, "conflict_outcome": "skipped"
            }),
        );
        assert_eq!(
            several["reasons"],
            json!([
                "classification_provenance_unknown",
                "unverifiable_gaps",
                "chain_confidence_below_0.8",
                "conflict_resolution_skipped"
            ])
        );
    }

    #[test]
    fn scientific_gate_ht1_only_updates_on_the_four_declared_triggers() {
        let form = skill_candidate("hypothesis-testing", "\"new_category\"");
        for trigger in [
            "contradiction",
            "new_category",
            "flip",
            "refinement",
            "reconfirmation",
            "",
            "other",
        ] {
            let expected =
                if ["contradiction", "new_category", "flip", "refinement"].contains(&trigger) {
                    "update"
                } else {
                    "keep"
                };
            assert_scientific_case(
                &form,
                trigger,
                &json!({"trigger": trigger}),
                json!(expected),
            );
        }
    }

    #[test]
    fn scientific_gate_negative_controls_detect_semantic_mutations() {
        // Mutate only an in-memory copy of the authored form. A syntactically
        // valid weakening must disagree with the same negative-case oracle.
        for (skill, needle, original, replacement, env, expected) in [
            (
                "hypothesis-testing",
                "define valid-rubric-kinds",
                "(t (not t))",
                "(t t)",
                scientific_mapping_env("registry_schema_mismatch"),
                json!(false),
            ),
            (
                "hypothesis-testing",
                "define valid-rubric-kinds",
                "(string= expected_headroom_arm mapped_headroom_arm)",
                "t",
                scientific_env(
                    &scientific_mapping_env("compatible_binary"),
                    json!({"mapped_headroom_arm": "compatible_scientific"}),
                ),
                json!(false),
            ),
            (
                "experimental-design",
                "\"corroborated\"",
                "(and prediction_contradicted precision_adequate)",
                "prediction_contradicted",
                scientific_env(
                    &scientific_findings_env(),
                    json!({"prediction_contradicted": true, "precision_adequate": false}),
                ),
                json!("inconclusive"),
            ),
            (
                "experimental-design",
                "(length run_reports)",
                "(not measurement_valid)",
                "true",
                scientific_env(&scientific_findings_env(), json!({"run_reports": []})),
                json!(false),
            ),
        ] {
            let form = skill_candidate(skill, needle);
            assert_eq!(
                form.matches(original).count(),
                1,
                "mutation target must remain unique: {original}"
            );
            assert_scientific_case(&form, original, &env, expected.clone());
            let mutant = form.replacen(original, replacement, 1);
            parse(&mutant).expect("negative control remains syntactically valid");
            assert_ne!(
                eval_sandboxed(&mutant, &env).expect("mutant evaluates"),
                expected,
                "the decision oracle must detect {original} -> {replacement}"
            );
        }
    }

    #[test]
    fn vc0_typed_predicate_admits_content_and_rejects_invisible_leaders() {
        // The VC-0 typed-reference predicate (verification-compression PLAN
        // step 0). Its old form tested three literal prefixes (space, tab,
        // newline) against a stated "no leading whitespace" invariant — a
        // blocklist that CR, VT, FF, NBSP and ZWSP all passed (live
        // 2026-10-18). This drives the live skill form end to end: admissible
        // rows pass the gate, and every bypass form fails it.
        let form = skill_candidate("verification-compression", "define typed");
        let row_with = |artifact_id: &str| {
            json!({
                "artifact_id": artifact_id,
                "status": "current_signal",
                "expectation_id": "expectation:gate",
                "expectation_authority": "authority:operator-2026-10-18",
                "falsifier_id": "falsifier:bypass",
                "observed_boundary": "boundary:a b c",
                "live_disturbance_channel": "channel:test",
                "corrective_action": "correction:reject",
                "return_path": "return:plan",
                "retirement_trigger": "trigger:channel-removed",
                "retirement_trigger_status": "pending",
                "decision_record": "",
                "retirement_authority": ""
            })
        };
        let run = |artifact_id: &str| {
            let row = row_with(artifact_id);
            let env = json!({
                "artifact_inventory": [row],
                "relevance_classification": [row]
            });
            let result = eval_sandboxed(&form, &env).expect("VC-0 form should evaluate");
            result
                .get("gate_passed")
                .cloned()
                .expect("VC-0 result must carry gate_passed")
        };

        assert_eq!(run("artifact:skill/foo"), json!(true));
        assert_eq!(
            run("artifact:value with inner spaces"),
            json!(true),
            "boundary-scoped: inner whitespace is legitimate content"
        );
        assert_eq!(
            run("artifact:"),
            json!(false),
            "the prefix alone carries no content"
        );
        // Every form the space/tab/newline blocklist let through.
        for (label, artifact_id) in [
            ("space", "artifact: hidden"),
            ("tab", "artifact:\thidden"),
            ("newline", "artifact:\nhidden"),
            ("carriage return", "artifact:\rhidden"),
            ("vertical tab", "artifact:\u{000b}hidden"),
            ("form feed", "artifact:\u{000c}hidden"),
            ("no-break space", "artifact:\u{00a0}hidden"),
            ("zero-width space", "artifact:\u{200b}hidden"),
            ("ideographic space", "artifact:\u{3000}hidden"),
            ("soft hyphen", "artifact:\u{00ad}hidden"),
            ("BOM", "artifact:\u{feff}hidden"),
        ] {
            assert_eq!(run(artifact_id), json!(false), "{label} must fail the gate");
        }
    }

    #[test]
    fn prompt_enhance_state_contract_uses_authored_form() {
        smol::block_on(async {
            let form = skill_candidate("prompt-enhance", "define pe-get");
            let cases: serde_json::Value = serde_json::from_str(include_str!(
                "../../tests/fixtures/prompt_enhance_state.json"
            ))
            .expect("NEW-data PE fixtures are valid JSON");
            let cases = cases.as_array().expect("PE fixture array");
            assert!(!cases.is_empty(), "PE gate must not run zero cases");
            let mut admitting = 0;
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
                let filter = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../.agents/skills/prompt-enhance/raw-shape.jq");
                let mut child = smol::process::Command::new("jq")
                    .args(["-e", "-f"])
                    .arg(filter)
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
                let result = eval_sandboxed(&form, &env);

                if case["expect_error"] == json!(true) {
                    assert!(result.is_err(), "{id}: malformed required data must refuse");
                    if let Some(stage) = case.get("expect_stage") {
                        assert_eq!(stage, "lisp", "{id}: raw refusal was not enforced");
                    }

                    refusing += 1;
                } else {
                    let actual = result.unwrap_or_else(|error| panic!("{id}: {error}"));
                    let expected = case["expect"].as_object().expect("expected fields");
                    assert!(!expected.is_empty(), "{id}: no vacuous oracle");
                    for (key, value) in expected {
                        assert_eq!(&actual[key], value, "{id}: {key}");
                    }
                    if let Some(keys) = case.get("forbid") {
                        for key in keys.as_array().expect("forbidden output fields") {
                            assert!(
                                actual.get(key.as_str().expect("field name")).is_none(),
                                "{id}: gate must not echo the review"
                            );
                        }
                    }
                    if let Some(expected_raw) = case.get("expect_selected_raw") {
                        let selected = actual["selected_attempt_id"]
                            .as_u64()
                            .expect("scalar selector");
                        assert!(selected > 0, "{id}: expected a selected review");
                        let original = &env["critique_runs"][(selected - 1) as usize];
                        assert_eq!(
                            original["attempt_id"], selected,
                            "{id}: original ID must match"
                        );
                        assert_eq!(
                            original, expected_raw,
                            "{id}: select original JSON losslessly"
                        );
                    }
                    if actual["admit"] == json!(true) {
                        admitting += 1;
                    } else {
                        refusing += 1;
                    }
                }
            }
            assert!(
                admitting > 0 && refusing > 0,
                "both admission directions are pinned"
            );
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
            "../../tests/fixtures/prompt_enhance_probes.json"
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
    fn arithmetic_tb_count_adjustment_and_directives_share_one_result() {
        let count_form = skill_candidate("task-breakdown", "\"task_count\"");
        let gate_form = skill_candidate("task-breakdown", "\"base_gate_scores\"");
        for (count, penalty) in [(0, 0.1), (2, 0.1), (3, 0.0), (20, 0.0), (21, 0.1)] {
            assert_scientific_case(
                &count_form,
                "task-count boundary",
                &json!({
                    "tasks": vec![json!({"id": "fixture"}); count]
                }),
                json!({"task_count": count, "task_count_penalty": penalty}),
            );
        }
        let size_directive = json!([{
            "criterion": "task_sizing", "what_is_wrong": "Review the supplied task sizes/count.",
            "expected_fix": "Reslice the named fixture tasks."
        }]);
        let environment = |penalty: f64, sizing: f64, directives: serde_json::Value| {
            json!({
                "base_gate_scores": {
                    "target_condition_coverage": 0.0, "task_sizing": sizing,
                    "vertical_slice_integrity": 0.0, "ac_specificity": 0.0,
                    "dependency_ordering": 0.0, "checkpoint_presence": 0.0,
                    "red_flag_absence": 0.0
                },
                "task_count_penalty": penalty, "refinement_directives": directives,
                "gate_findings": [{"finding": "retain this reviewer observation"}], "red_flags": []
            })
        };
        for (penalty, base, adjusted, pass) in [
            (0.1, 0.0, 0.1, true),
            (0.0, 0.0, 0.0, true),
            (0.1, 0.2, 0.3, true),
            (0.1, 0.2001, 0.3001, false),
            (0.1, 0.95, 1.0, false),
            (0.0, 0.25, 0.25, true),
        ] {
            let directives = if adjusted > 0.0 {
                size_directive.clone()
            } else {
                json!([])
            };
            let env = environment(penalty, base, directives);
            let result = eval_sandboxed(&gate_form, &env).expect("authored gate evaluates");
            assert_eq!(result["input_valid"], json!(true));
            assert_eq!(result["directives_complete"], json!(true));
            assert_eq!(result["gate_pass"], json!(pass));
            let effective = result["gate_scores"]["task_sizing"]
                .as_f64()
                .expect("effective score");
            assert!((effective - adjusted).abs() < 1e-12);
            assert_eq!(result["base_gate_scores"], env["base_gate_scores"]);
            assert_eq!(result["gate_findings"], env["gate_findings"]);
            assert_eq!(
                result["refinement_directives"],
                env["refinement_directives"]
            );
        }
        let missing = eval_sandboxed(&gate_form, &environment(0.1, 0.0, json!([])))
            .expect("missing directive is surfaced separately from numeric grading");
        assert_eq!(missing["input_valid"], json!(true));
        assert_eq!(missing["directives_complete"], json!(false));
        assert_eq!(missing["gate_pass"], json!(false));
        assert_eq!(
            missing["missing_refinement_criteria"],
            json!(["task_sizing"])
        );
        for invalid in [
            json!(-0.01),
            json!(1.01),
            json!("not a score"),
            serde_json::Value::Null,
        ] {
            let mut env = environment(0.1, 0.0, size_directive.clone());
            env["base_gate_scores"]["task_sizing"] = invalid;
            let result = eval_sandboxed(&gate_form, &env).expect("invalid scores are surfaced");
            assert_eq!(result["input_valid"], json!(false));
            assert_eq!(result["gate_pass"], json!(false));
        }
        let mut edge = environment(
            0.1,
            0.2,
            json!([
                {"criterion":"task_sizing"}, {"criterion":"target_condition_coverage"},
                {"criterion":"vertical_slice_integrity"}
            ]),
        );
        edge["base_gate_scores"]["target_condition_coverage"] = json!(0.3);
        edge["base_gate_scores"]["vertical_slice_integrity"] = json!(0.2);
        let result = eval_sandboxed(&gate_form, &edge).expect("exact acceptance boundary");
        assert_eq!(result["gate_weighted_total"], json!(0.15));
        assert_eq!(result["gate_scores"]["task_sizing"], json!(0.3));
        assert_eq!(result["gate_pass"], json!(true));
        edge["base_gate_scores"]["vertical_slice_integrity"] = json!(0.2001);
        let result = eval_sandboxed(&gate_form, &edge).expect("just above acceptance boundary");
        assert_eq!(result["gate_pass"], json!(false));
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
