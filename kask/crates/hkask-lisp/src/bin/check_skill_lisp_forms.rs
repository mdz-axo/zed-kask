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
    fn scientific_gate_ir_1_requires_every_reported_code_to_be_cited() {
        let form = skill_candidate("idiomatic-rust", "unexplained_codes");
        let form = form.as_str();
        let base = json!({
            "gate": {"baseline_status": "ran", "open_codes": ["E0308", "clippy::ptr_arg"]},
            "invariants": ["a queue is never empty"],
            "principle_violations": [{"principle": 1, "diagnostic_codes": ["E0308"]}],
            "invalid_states": [
                {"state": "s", "diagnostic_codes": ["clippy::ptr_arg"]},
                {"state": "t", "diagnostic_codes": []}
            ]
        });
        let eval = |case: &str, changes: serde_json::Value| {
            eval_sandboxed(form, &scientific_env(&base, changes))
                .unwrap_or_else(|error| panic!("{case}: IR-1 failed to evaluate: {error}"))
        };
        let pass = eval("pass", json!({}));
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["unexplained_codes"], json!([]));
        assert_eq!(pass["invariants"], json!(1));

        // A clean baseline has nothing to cite and still passes.
        let clean = eval(
            "clean baseline",
            json!({"gate": {"baseline_status": "ran", "open_codes": []},
                   "principle_violations": [], "invalid_states": []}),
        );
        assert_eq!(clean["verdict"], json!("pass"));

        let uncited = eval(
            "uncited code",
            json!({"invalid_states": [{"state": "t", "diagnostic_codes": []}]}),
        );
        assert_eq!(uncited["verdict"], json!("incomplete"));
        assert_eq!(uncited["unexplained_codes"], json!(["clippy::ptr_arg"]));

        let no_invariants = eval("no invariants", json!({"invariants": []}));
        assert_eq!(no_invariants["verdict"], json!("incomplete"));
        assert_eq!(no_invariants["invariants"], json!(0));

        for status in ["not_run", "unexplained_failure"] {
            let ungrounded = eval(
                status,
                json!({"gate": {"baseline_status": status, "open_codes": []}}),
            );
            assert_eq!(ungrounded["verdict"], json!("ungrounded"), "{status}");
        }
    }

    #[test]
    fn scientific_gate_ir_2_converges_only_on_a_measured_clean_proposal() {
        let form = skill_candidate("idiomatic-rust", "define confirmed-in");
        let form = form.as_str();
        let base = json!({
            "gate": {
                "baseline_status": "ran", "proposed_status": "ran", "open_codes": [],
                "miri": "not_run", "compile_errors": 0, "new_lints": 0, "unsafe_lines": 0,
                "history": [], "iteration": 1
            },
            "gaps": [], "edge_cases": [], "guideline_violations": [],
            "reference_results": ["Found 3 references to `Queue`"]
        });
        let with_gate = |changes: serde_json::Value| {
            let mut gate = base["gate"].clone();
            gate.as_object_mut()
                .expect("gate object")
                .extend(changes.as_object().expect("gate changes").clone());
            json!({"gate": gate})
        };
        let eval = |case: &str, changes: serde_json::Value| {
            eval_sandboxed(form, &scientific_env(&base, changes))
                .unwrap_or_else(|error| panic!("{case}: IR-2 failed to evaluate: {error}"))
        };

        let converged = eval("converged", json!({}));
        assert_eq!(converged["verdict"], json!("converged"));
        assert_eq!(converged["reasons"], json!([]));
        assert_eq!(converged["iteration"], json!(1));
        assert_eq!(converged["confirmed"], json!(0));
        assert_eq!(converged["notes"], json!([]));

        // Each open reason alone blocks convergence and is named.
        for (case, changes, reason) in [
            (
                "compile error",
                with_gate(json!({"compile_errors": 1, "open_codes": ["E0308"]})),
                "compile_errors",
            ),
            ("new lint", with_gate(json!({"new_lints": 2})), "new_lints"),
            (
                "no references",
                json!({"reference_results": []}),
                "references_not_examined",
            ),
            (
                "open guideline",
                json!({"guideline_violations": [
                    {"guideline": "C-CONV", "status": "open"},
                    {"guideline": "C-DEBUG", "status": "fixed"},
                    {"guideline": "C-SEND-SYNC", "status": "justified"}
                ]}),
                "guideline_open",
            ),
            (
                "miri ub",
                with_gate(json!({"unsafe_lines": 2, "miri": "ub"})),
                "miri_ub",
            ),
        ] {
            let result = eval(case, changes);
            assert_eq!(result["verdict"], json!("continue"), "{case}");
            assert_eq!(result["reasons"], json!([reason]), "{case}");
        }
        let open = eval(
            "open guideline names the row",
            json!({"guideline_violations": [
                {"guideline": "C-CONV", "status": "open"},
                {"guideline": "C-DEBUG", "status": "fixed"}
            ]}),
        );
        assert_eq!(open["open_guidelines"], json!(["C-CONV"]));
        // A status the gate does not know is open, not silently accepted.
        let odd = eval(
            "unknown guideline status",
            json!({"guideline_violations": [{"guideline": "C-CONV", "status": "waived"}]}),
        );
        assert_eq!(odd["verdict"], json!("continue"));
        // Fixed and justified rows do not block.
        let settled = eval(
            "settled guidelines",
            json!({"guideline_violations": [
                {"guideline": "C-DEBUG", "status": "fixed"},
                {"guideline": "C-SEND-SYNC", "status": "justified"}
            ]}),
        );
        assert_eq!(settled["verdict"], json!("converged"));
        // UB reported with no unsafe line in the proposal is not this proposal's finding.
        let stale_miri = eval("miri ub without unsafe", with_gate(json!({"miri": "ub"})));
        assert_eq!(stale_miri["verdict"], json!("converged"));

        // A proposal the compiler has not seen is never converged.
        for (case, changes) in [
            (
                "proposal not applied",
                json!({"proposed_status": "not_run"}),
            ),
            (
                "build failed silently",
                json!({"proposed_status": "unexplained_failure"}),
            ),
            ("no baseline", json!({"baseline_status": "not_run"})),
        ] {
            assert_eq!(
                eval(case, with_gate(changes))["verdict"],
                json!("undetermined"),
                "{case}"
            );
        }

        // The bound: a third iteration with an open reason stops, a clean third
        // iteration converges, and a fourth is refused whatever it shows.
        let third_open = eval(
            "third iteration, open",
            with_gate(json!({"iteration": 3, "history": [0, 0], "new_lints": 1})),
        );
        assert_eq!(third_open["verdict"], json!("stop_bound"));
        assert_eq!(third_open["reasons"], json!(["new_lints"]));
        let second_open = eval(
            "second iteration, open",
            with_gate(json!({"iteration": 2, "history": [0], "new_lints": 1})),
        );
        assert_eq!(second_open["verdict"], json!("continue"));
        let third_clean = eval(
            "third iteration, clean",
            with_gate(json!({"iteration": 3, "history": [0, 0]})),
        );
        assert_eq!(third_clean["verdict"], json!("converged"));
        let fourth = eval(
            "fourth iteration",
            with_gate(json!({"iteration": 4, "history": [0, 0, 0]})),
        );
        assert_eq!(fourth["verdict"], json!("stop_bound"));

        // A finding is confirmed only when every code it cites was measured.
        let findings = eval(
            "confirmation",
            json!({
                "gate": {
                    "baseline_status": "ran", "proposed_status": "ran",
                    "open_codes": ["clippy::ptr_arg", "unused_variables"],
                    "miri": "not_run", "compile_errors": 0, "new_lints": 0, "unsafe_lines": 0,
                    "history": [], "iteration": 1
                },
                "gaps": [
                    {"gap": "a", "diagnostic_codes": ["clippy::ptr_arg"]},
                    {"gap": "b", "diagnostic_codes": ["clippy::ptr_arg", "unused_variables"]},
                    {"gap": "c", "diagnostic_codes": ["clippy::ptr_arg", "E9999"]}
                ],
                "edge_cases": [
                    {"scenario": "d", "diagnostic_codes": []},
                    {"scenario": "e", "diagnostic_codes": ["unused_variables"]}
                ]
            }),
        );
        assert_eq!(findings["confirmed"], json!(3));
        assert_eq!(findings["unsupported_citations"], json!(["E9999"]));
        assert_eq!(findings["verdict"], json!("converged"));

        // Oscillation reads the last three counts: history plus this iteration's.
        let two_confirmed = json!([
            {"gap": "a", "diagnostic_codes": ["clippy::ptr_arg"]},
            {"gap": "b", "diagnostic_codes": ["clippy::ptr_arg"]}
        ]);
        let oscillation = |history: serde_json::Value| {
            eval(
                "oscillation",
                json!({
                    "gate": {
                        "baseline_status": "ran", "proposed_status": "ran",
                        "open_codes": ["clippy::ptr_arg"], "miri": "not_run",
                        "compile_errors": 0, "new_lints": 0, "unsafe_lines": 0,
                        "history": history, "iteration": 3
                    },
                    "gaps": two_confirmed.clone()
                }),
            )
        };
        assert_eq!(oscillation(json!([1, 4]))["verdict"], json!("oscillating"));
        assert_eq!(oscillation(json!([4, 1]))["verdict"], json!("oscillating"));
        assert_eq!(oscillation(json!([4, 3]))["verdict"], json!("converged"));
        assert_eq!(oscillation(json!([2, 2]))["verdict"], json!("converged"));

        // unsafe that Miri did not check is disclosed beside the verdict.
        for (miri, noted) in [("not_run", true), ("failed", true), ("passed", false)] {
            let result = eval(miri, with_gate(json!({"unsafe_lines": 1, "miri": miri})));
            assert_eq!(result["verdict"], json!("converged"), "{miri}");
            assert_eq!(
                result["notes"],
                if noted {
                    json!(["unsafe_not_miri_checked"])
                } else {
                    json!([])
                },
                "{miri}"
            );
        }
    }

    #[test]
    fn scientific_gate_cg_1_requires_a_bounded_plan_that_quotes_the_task() {
        let form = skill_candidate("coding-guidelines", "define untraced");
        let form = form.as_str();
        let step = |name: &str, quote: serde_json::Value, lines: serde_json::Value| {
            json!({"step": name, "files_to_touch": ["src/fetch.rs"], "files_not_to_touch": [],
                   "lines_estimate": lines, "traces_to": quote, "verify": "v"})
        };
        let base = json!({
            "task_description": "Please add a retry to the fetch call.",
            "success_criteria": [{"goal": "a"}, {"goal": "b"}],
            "constrained_plan": [step("retry", json!("add a retry to the fetch call"), json!(50))]
        });
        let eval = |case: &str, changes: serde_json::Value| {
            eval_sandboxed(form, &scientific_env(&base, changes))
                .unwrap_or_else(|error| panic!("{case}: CG-1 failed to evaluate: {error}"))
        };
        let pass = eval("pass", json!({}));
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["reasons"], json!([]));
        assert_eq!(pass["criteria"], json!(2));
        let five = eval(
            "five criteria",
            json!({"success_criteria": [{}, {}, {}, {}, {}]}),
        );
        assert_eq!(five["verdict"], json!("pass"));

        for (case, criteria) in [
            ("one criterion", json!([{}])),
            ("six criteria", json!([{}, {}, {}, {}, {}, {}])),
        ] {
            let result = eval(case, json!({"success_criteria": criteria}));
            assert_eq!(result["verdict"], json!("revise"), "{case}");
            assert_eq!(result["reasons"], json!(["criteria_count"]), "{case}");
        }
        let empty = eval("empty plan", json!({"constrained_plan": []}));
        assert_eq!(empty["reasons"], json!(["empty_plan"]));

        // The quote must be present, non-empty, and found in the task text exactly.
        for (case, quote) in [
            ("missing quote", serde_json::Value::Null),
            ("empty quote", json!("")),
            ("invented quote", json!("add structured logging")),
            ("wrong case", json!("Add a retry")),
            (
                "quote longer than the task",
                json!("Please add a retry to the fetch call. And everything else."),
            ),
        ] {
            let result = eval(
                case,
                json!({"constrained_plan": [step("s", quote, json!(10))]}),
            );
            assert_eq!(result["verdict"], json!("revise"), "{case}");
            assert_eq!(result["reasons"], json!(["untraced_steps"]), "{case}");
            assert_eq!(result["untraced_steps"], json!(["s"]), "{case}");
        }
        for (case, lines) in [("51 lines", json!(51)), ("estimate is text", json!("12"))] {
            let result = eval(
                case,
                json!({"constrained_plan": [step("big", json!("add a retry"), lines)]}),
            );
            assert_eq!(result["reasons"], json!(["oversized_steps"]), "{case}");
            assert_eq!(result["oversized_steps"], json!(["big"]), "{case}");
        }
        let mut no_files = step("bare", json!("fetch call"), json!(5));
        no_files["files_to_touch"] = json!([]);
        let fileless = eval("no files", json!({"constrained_plan": [no_files]}));
        assert_eq!(fileless["reasons"], json!(["steps_without_files"]));
        assert_eq!(fileless["steps_without_files"], json!(["bare"]));

        // One bad step among good ones is named; the good ones are not.
        let mixed = eval(
            "mixed plan",
            json!({"constrained_plan": [
                step("good", json!("add a retry"), json!(20)),
                step("stray", json!("rewrite the client"), json!(20)),
                step("also good", json!("fetch call"), json!(20))
            ]}),
        );
        assert_eq!(mixed["untraced_steps"], json!(["stray"]));
        assert_eq!(mixed["oversized_steps"], json!([]));
    }

    #[test]
    fn scientific_gate_cg_2_decides_from_the_audit_the_scope_and_the_record() {
        let form = skill_candidate("coding-guidelines", "define malformed");
        let form = form.as_str();
        let scores = |a: f64, b: f64, c: f64, d: f64| {
            json!({"think_before_coding": a, "simplicity_first": b,
                   "surgical_changes": c, "goal_driven_execution": d})
        };
        let scope = |changes: serde_json::Value| {
            let mut scope = json!({"changed": ["src/a.rs"], "unplanned": [], "untouched_planned": [],
                                   "forbidden_touched": [], "prior_audits": []});
            scope
                .as_object_mut()
                .expect("scope object")
                .extend(changes.as_object().expect("scope changes").clone());
            json!({"scope": scope})
        };
        let violation = |principle: &str, severity: &str, description: &str| {
            json!({"principle": principle, "severity": severity, "description": description,
                   "location": "src/a.rs:1", "correction": "c"})
        };
        let base = json!({
            "violations": [violation("surgical-changes", "low", "renamed a local")],
            "compliance_score": scores(1.0, 0.8, 0.9, 0.7),
            "scope": scope(json!({}))["scope"]
        });
        let eval = |case: &str, changes: serde_json::Value| {
            eval_sandboxed(form, &scientific_env(&base, changes))
                .unwrap_or_else(|error| panic!("{case}: CG-2 failed to evaluate: {error}"))
        };

        let pass = eval("pass", json!({}));
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["reasons"], json!([]));
        assert_eq!(pass["critical"], json!([]));
        assert_eq!(pass["audit"], json!(1));
        // The mean is computed, with the engine's own float arithmetic.
        assert_eq!(pass["overall"], json!(0.8500000000000001));

        // Each failing reason alone asks for the one correction pass.
        for (case, changes, reason) in [
            (
                "critical violation",
                json!({"violations": [violation("simplicity-first", "critical", "single-use trait"),
                                      violation("surgical-changes", "high", "touched b")]}),
                "critical_violations",
            ),
            (
                "below the floor",
                json!({"compliance_score": scores(0.6, 0.7, 0.7, 0.79)}),
                "below_floor",
            ),
            (
                "unplanned file",
                scope(json!({"changed": ["src/a.rs", "src/z.rs"], "unplanned": ["src/z.rs"]})),
                "unplanned_files",
            ),
            (
                "forbidden file",
                scope(
                    json!({"changed": ["src/a.rs", "src/c.rs"], "forbidden_touched": ["src/c.rs"]}),
                ),
                "forbidden_files_touched",
            ),
        ] {
            let result = eval(case, changes);
            assert_eq!(result["verdict"], json!("correct"), "{case}");
            assert_eq!(result["reasons"], json!([reason]), "{case}");
        }
        let critical = eval(
            "critical is selected from the whole report",
            json!({"violations": [violation("simplicity-first", "critical", "single-use trait"),
                                  violation("surgical-changes", "high", "touched b"),
                                  violation("goal-driven-execution", "critical", "no criteria")]}),
        );
        assert_eq!(
            critical["critical"],
            json!(["single-use trait", "no criteria"])
        );
        // Exactly at the floor passes.
        let floor = eval(
            "at the floor",
            json!({"compliance_score": scores(0.7, 0.7, 0.7, 0.7)}),
        );
        assert_eq!(floor["verdict"], json!("pass"));
        assert_eq!(floor["overall"], json!(0.7));

        // A vocabulary slip cannot hide a critical violation or a score.
        for (case, changes) in [
            (
                "capitalised severity",
                json!({"violations": [violation("simplicity-first", "Critical", "x")]}),
            ),
            (
                "unknown severity",
                json!({"violations": [violation("simplicity-first", "blocker", "x")]}),
            ),
            (
                "underscored principle",
                json!({"violations": [violation("simplicity_first", "high", "x")]}),
            ),
            (
                "severity missing",
                json!({"violations": [{"principle": "simplicity-first", "description": "x"}]}),
            ),
            (
                "score as text",
                json!({"compliance_score": {"think_before_coding": "0.9", "simplicity_first": 0.9, "surgical_changes": 0.9, "goal_driven_execution": 0.9}}),
            ),
            (
                "score above one",
                json!({"compliance_score": scores(1.5, 0.9, 0.9, 0.9)}),
            ),
            (
                "score below zero",
                json!({"compliance_score": scores(-0.1, 0.9, 0.9, 0.9)}),
            ),
            (
                "score missing",
                json!({"compliance_score": {"think_before_coding": 0.9, "simplicity_first": 0.9, "surgical_changes": 0.9}}),
            ),
        ] {
            assert_eq!(eval(case, changes)["verdict"], json!("invalid"), "{case}");
        }
        let slip = eval(
            "one malformed row is counted",
            json!({"violations": [violation("simplicity-first", "Critical", "x"),
                                  violation("surgical-changes", "low", "y")]}),
        );
        assert_eq!(slip["malformed_violations"], json!(1));
        assert_eq!(slip["scores_valid"], json!(true));

        // Nothing changed: the post-implementation gate has nothing to pass.
        let unchanged = eval("no implementation", scope(json!({"changed": []})));
        assert_eq!(unchanged["verdict"], json!("no_implementation"));

        // The bound: the second audit may pass, a second failure stops, and a
        // third audit is refused whatever it reports.
        let second = scope(json!({"prior_audits": ["correct"]}));
        let second_pass = eval("second audit passes", second.clone());
        assert_eq!(second_pass["verdict"], json!("pass"));
        assert_eq!(second_pass["audit"], json!(2));
        let mut second_fail = second;
        second_fail["compliance_score"] = scores(0.2, 0.2, 0.2, 0.2);
        let stopped = eval("second audit fails", second_fail);
        assert_eq!(stopped["verdict"], json!("stop_bound"));
        assert_eq!(stopped["reasons"], json!(["below_floor"]));
        let third = eval(
            "third audit",
            scope(json!({"prior_audits": ["correct", "stop_bound"]})),
        );
        assert_eq!(third["verdict"], json!("stop_bound"));
        assert_eq!(third["audit"], json!(3));
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

    /// The essentialist loop gate: one `ESS-1` call per gate evaluation,
    /// state threaded back. These helpers build the env the SKILL.md documents
    /// and run a sequence of steps the way the skill does, so a test reads as
    /// a run ("G1 reduce, G1 clean, G2, G3, next round ...") rather than as
    /// a pile of envs.
    fn ess_form() -> String {
        skill_candidate("essentialist", "define triggers")
    }

    fn ess_snapshot(items: &[&str], interfaces: &[&str]) -> serde_json::Value {
        json!({"surviving_items": items, "structure": ["mod wrapper"], "interfaces": interfaces})
    }

    fn ess_finding(item: &str, force: &str, action: &str) -> serde_json::Value {
        json!({"item": item, "constraint_force": force, "action": action,
               "issue": "i", "recommendation": "r"})
    }

    fn ess_step(
        form: &str,
        request: &str,
        mode: &str,
        gate: &str,
        findings: serde_json::Value,
        decisions: serde_json::Value,
        snapshot: serde_json::Value,
        state: serde_json::Value,
    ) -> serde_json::Value {
        let env = json!({"request": request, "mode": mode, "gate": gate, "findings": findings,
                         "decisions": decisions, "snapshot": snapshot, "state": state});
        eval_sandboxed(form, &env)
            .unwrap_or_else(|error| panic!("ESS-1 failed to evaluate at {gate}: {error}"))
    }

    /// Run `steps` of (gate, findings, snapshot) in autonomous mode with the
    /// request "simplify", threading state; return the last result.
    fn ess_run(
        form: &str,
        steps: &[(&str, serde_json::Value, serde_json::Value)],
    ) -> serde_json::Value {
        let mut state = json!({});
        let mut last = json!(null);
        for (gate, findings, snapshot) in steps {
            last = ess_step(
                form,
                "simplify",
                "autonomous",
                gate,
                findings.clone(),
                json!([]),
                snapshot.clone(),
                state,
            );
            assert_ne!(
                last["verdict"],
                json!("invalid"),
                "unexpected invalid step at {gate}: {last}"
            );
            state = last["state"].clone();
        }
        last
    }

    #[test]
    fn scientific_gate_ess_1_resolves_mode_from_the_request_not_the_claim() {
        let form = ess_form();
        let snap = ess_snapshot(
            &["Wrapper::get", "Wrapper::put", "Config", "Inner"],
            &["fn get", "fn put"],
        );
        let finding = json!([ess_finding("Wrapper::get", "prohibition", "DELETE")]);
        let step = |request: &str, mode: &str| {
            ess_step(
                &form,
                request,
                mode,
                "G1",
                finding.clone(),
                json!([]),
                snap.clone(),
                json!({}),
            )
        };

        let asked = step("please simplify this wrapper", "autonomous");
        assert_eq!(asked["mode"], json!("autonomous"));
        assert_eq!(asked["verdict"], json!("reduce"));
        assert_eq!(asked["items"], json!(["Wrapper::get"]));

        let stripped = step("Strip this module down", "autonomous");
        assert_eq!(stripped["mode"], json!("autonomous"));

        // In autonomous mode a guardrail reduces as a prohibition does; the
        // informational forces never do.
        let mixed = ess_step(
            &form,
            "simplify",
            "autonomous",
            "G1",
            json!([
                ess_finding("Config", "guardrail", "MERGE"),
                ess_finding("depth", "evidence", "RETAIN"),
                ess_finding("maybe", "guideline", "MERGE"),
                ess_finding("guess", "hypothesis", "DELETE"),
            ]),
            json!([]),
            snap.clone(),
            json!({}),
        );
        assert_eq!(mixed["verdict"], json!("reduce"));
        assert_eq!(mixed["items"], json!(["Config"]));

        // A claim without the user's words, and a negated request, both resolve
        // to advisory: the finding is presented, nothing is reduced.
        for (case, request) in [
            ("no intent", "review this wrapper"),
            ("negated", "don't simplify this, just look at it"),
            ("not", "this is not a simplify request"),
        ] {
            let result = step(request, "autonomous");
            assert_eq!(result["mode"], json!("advisory"), "{case}");
            assert_eq!(result["verdict"], json!("await_human"), "{case}");
            assert_eq!(
                result["present"],
                json!({"Wrapper::get": "REQUIRED"}),
                "{case}"
            );
            assert_eq!(
                result["state"]["log"],
                json!([]),
                "{case}: an undecided step is not logged"
            );
        }

        // The caller's own advisory claim is never upgraded by the words.
        let advisory = step("simplify it", "advisory");
        assert_eq!(advisory["mode"], json!("advisory"));
        assert_eq!(advisory["verdict"], json!("await_human"));
    }

    #[test]
    fn scientific_gate_ess_1_applies_only_the_human_decisions_in_advisory_mode() {
        let form = ess_form();
        let snap = ess_snapshot(
            &["Wrapper::get", "Wrapper::put", "Config", "Inner"],
            &["fn get", "fn put"],
        );
        let findings = json!([
            ess_finding("Wrapper::get", "prohibition", "DELETE"),
            ess_finding("Config", "guardrail", "MERGE"),
            ess_finding("depth", "evidence", "RETAIN"),
        ]);
        let decide = |decisions: serde_json::Value| {
            ess_step(
                &form,
                "review",
                "advisory",
                "G1",
                findings.clone(),
                decisions,
                snap.clone(),
                json!({}),
            )
        };

        let labels = decide(json!([]));
        assert_eq!(labels["verdict"], json!("await_human"));
        assert_eq!(
            labels["present"],
            json!({"Wrapper::get": "REQUIRED", "Config": "REQUIRED (overridable with a reason)", "depth": "INFO"})
        );

        let refused = decide(json!([
            {"item": "Wrapper::get", "response": "reject", "reason": "needed"},
            {"item": "Config", "response": "accept"},
            {"item": "depth", "response": "accept"}
        ]));
        assert_eq!(refused["verdict"], json!("escalate"));
        assert_eq!(refused["reason"], json!("prohibition_rejected"));
        assert_eq!(refused["items"], json!(["Wrapper::get"]));

        let bare = decide(json!([
            {"item": "Wrapper::get", "response": "accept"},
            {"item": "Config", "response": "reject"},
            {"item": "depth", "response": "accept"}
        ]));
        assert_eq!(bare["verdict"], json!("escalate"));
        assert_eq!(bare["reason"], json!("guardrail_rejected_without_reason"));
        assert_eq!(bare["items"], json!(["Config"]));

        // A reasoned guardrail rejection stands; the accepted prohibition reduces;
        // the accepted RETAIN is not a reduction.
        let reasoned = decide(json!([
            {"item": "Wrapper::get", "response": "accept"},
            {"item": "Config", "response": "reject", "reason": "separate operational domain"},
            {"item": "depth", "response": "accept"}
        ]));
        assert_eq!(reasoned["verdict"], json!("reduce"));
        assert_eq!(reasoned["items"], json!(["Wrapper::get"]));

        // An override needs its alternative; without it the item is undecided.
        let bare_override = decide(json!([
            {"item": "Wrapper::get", "response": "override"},
            {"item": "Config", "response": "accept"},
            {"item": "depth", "response": "accept"}
        ]));
        assert_eq!(bare_override["verdict"], json!("await_human"));
        assert_eq!(bare_override["items"], json!(["Wrapper::get"]));
        let with_override = decide(json!([
            {"item": "Wrapper::get", "response": "override", "alternative": "make it private"},
            {"item": "Config", "response": "accept"},
            {"item": "depth", "response": "accept"}
        ]));
        assert_eq!(with_override["verdict"], json!("reduce"));
        assert_eq!(with_override["items"], json!(["Wrapper::get", "Config"]));

        // Only informational findings, all decided: the gate passes.
        let info_only = ess_step(
            &form,
            "review",
            "advisory",
            "G1",
            json!([ess_finding("depth", "evidence", "RETAIN")]),
            json!([{"item": "depth", "response": "accept"}]),
            snap.clone(),
            json!({}),
        );
        assert_eq!(info_only["verdict"], json!("next_gate"));
    }

    #[test]
    fn scientific_gate_ess_1_counts_retries_and_rounds_from_its_own_log() {
        let form = ess_form();
        let four = ess_snapshot(
            &["Wrapper::get", "Wrapper::put", "Config", "Inner"],
            &["fn get", "fn put"],
        );
        let three = ess_snapshot(&["Wrapper::put", "Config", "Inner"], &["fn put"]);
        let delete_get = json!([ess_finding("Wrapper::get", "prohibition", "DELETE")]);
        let clean = json!([]);

        // The third reduce on one gate within a round escalates.
        let exhausted = ess_run(
            &form,
            &[
                ("G1", delete_get.clone(), four.clone()),
                ("G1", delete_get.clone(), four.clone()),
                ("G1", delete_get.clone(), four.clone()),
            ],
        );
        assert_eq!(exhausted["verdict"], json!("escalate"));
        assert_eq!(exhausted["reason"], json!("retries_exhausted"));
        assert_eq!(exhausted["round"], json!(1));

        // The documented regression case: G1 fails the pass-through, the reduced
        // artifact passes all three gates, the round changed so another round
        // runs, that round is unchanged -> complete with one of four removed.
        let complete = ess_run(
            &form,
            &[
                ("G1", delete_get, four.clone()),
                ("G1", clean.clone(), three.clone()),
                ("G2", clean.clone(), three.clone()),
                ("G3", clean.clone(), three.clone()),
                ("G1", clean.clone(), three.clone()),
                ("G2", clean.clone(), three.clone()),
                ("G3", clean.clone(), three.clone()),
            ],
        );
        assert_eq!(complete["verdict"], json!("complete"));
        assert_eq!(complete["round"], json!(2));
        assert_eq!(complete["initial_items"], json!(4));
        assert_eq!(complete["removed_items"], json!(1));
        assert_eq!(complete["score"], json!(25.0));
        assert_eq!(complete["band"], json!("minor_reduction"));
        let log = complete["state"]["log"].as_array().expect("log array");
        assert_eq!(log.len(), 7, "every decisive step is logged");
        assert_eq!(log[3]["verdict"], json!("next_round"));

        // A loop that is closed refuses another step.
        let after = ess_step(
            &form,
            "simplify",
            "autonomous",
            "G1",
            clean.clone(),
            json!([]),
            three.clone(),
            complete["state"].clone(),
        );
        assert_eq!(after["verdict"], json!("invalid"));
        assert_eq!(after["items"], json!(["loop_closed"]));

        // Zero-delta is exact over all three lists: a round that changed only
        // the interfaces is not complete.
        let same_items_new_signature =
            ess_snapshot(&["Wrapper::put", "Config", "Inner"], &["fn put(&self)"]);
        let interface_changed = ess_run(
            &form,
            &[
                ("G1", clean.clone(), three.clone()),
                ("G2", clean.clone(), three),
                ("G3", clean.clone(), same_items_new_signature),
            ],
        );
        assert_eq!(interface_changed["verdict"], json!("next_round"));

        // An unchanged first round is complete at once with score 0.
        let minimal = ess_run(
            &form,
            &[
                ("G1", clean.clone(), four.clone()),
                ("G2", clean.clone(), four.clone()),
                ("G3", clean.clone(), four),
            ],
        );
        assert_eq!(minimal["verdict"], json!("complete"));
        assert_eq!(minimal["score"], json!(0.0));
        assert_eq!(minimal["band"], json!("already_minimal"));

        // Three rounds that each change the artifact exhaust the round bound.
        let shrink = |n: usize| {
            let names: Vec<String> = (n..7).map(|i| format!("item{i}")).collect();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            ess_snapshot(&refs, &[])
        };
        let mut steps = Vec::new();
        for round in 1..=3 {
            let before = shrink(round);
            let after = shrink(round + 1);
            steps.push((
                "G1",
                json!([ess_finding(
                    &format!("item{round}"),
                    "prohibition",
                    "DELETE"
                )]),
                before,
            ));
            steps.push(("G1", clean.clone(), after.clone()));
            steps.push(("G2", clean.clone(), after.clone()));
            steps.push(("G3", clean.clone(), after));
        }
        let rounds = ess_run(&form, &steps);
        assert_eq!(rounds["verdict"], json!("escalate"));
        assert_eq!(rounds["reason"], json!("rounds_exhausted"));
        assert_eq!(rounds["round"], json!(3));
    }

    #[test]
    fn scientific_gate_ess_1_rejects_malformed_input_without_recording_it() {
        let form = ess_form();
        let snap = ess_snapshot(&["a"], &[]);
        let invalid = |case: &str,
                       gate: &str,
                       mode: &str,
                       findings: serde_json::Value,
                       snapshot: serde_json::Value,
                       state: serde_json::Value| {
            let result = ess_step(
                &form,
                "simplify",
                mode,
                gate,
                findings,
                json!([]),
                snapshot,
                state,
            );
            assert_eq!(result["verdict"], json!("invalid"), "{case}: {result}");
            result
        };

        let order = invalid(
            "G3 before G2",
            "G3",
            "autonomous",
            json!([]),
            snap.clone(),
            json!({}),
        );
        assert_eq!(order["items"], json!(["gate_out_of_order"]));

        let cased = invalid(
            "mis-cased force",
            "G1",
            "autonomous",
            json!([ess_finding("x", "Prohibition", "DELETE")]),
            snap.clone(),
            json!({}),
        );
        assert_eq!(cased["items"], json!(["malformed_findings"]));
        let action = invalid(
            "unknown action",
            "G1",
            "autonomous",
            json!([ess_finding("x", "prohibition", "REMOVE")]),
            snap.clone(),
            json!({}),
        );
        assert_eq!(action["items"], json!(["malformed_findings"]));

        let partial = invalid(
            "snapshot without interfaces",
            "G1",
            "autonomous",
            json!([]),
            json!({"surviving_items": ["a"], "structure": []}),
            json!({}),
        );
        assert_eq!(partial["items"], json!(["snapshot_missing"]));
        let text = invalid(
            "snapshot is text",
            "G1",
            "autonomous",
            json!([]),
            json!("x"),
            json!({}),
        );
        assert_eq!(text["items"], json!(["snapshot_missing"]));

        let both = invalid(
            "unknown mode and gate",
            "G4",
            "auto",
            json!([]),
            snap,
            json!({}),
        );
        assert_eq!(
            both["items"],
            json!(["unknown_mode", "unknown_gate", "gate_out_of_order"])
        );
        assert_eq!(
            both["state"]["log"],
            json!([]),
            "an invalid step is not logged"
        );
    }

    fn ki_session(drill: &str, automaticity: f64, day: u64) -> serde_json::Value {
        json!({"drill": drill, "automaticity": automaticity, "day": day})
    }

    #[test]
    fn scientific_gate_ki_1_selects_the_drill_from_the_log_not_from_typed_figures() {
        let form = skill_candidate("kata-improvement", "define max-day");
        let select = |sessions: serde_json::Value, today: u64| {
            eval_sandboxed(&form, &json!({"sessions": sessions, "today": today}))
                .unwrap_or_else(|error| panic!("KI-1 failed to evaluate: {error}"))
        };
        let obs = "beginner-observation-drill";
        let pdca = "beginner-pdca-cycle";

        let fresh = select(json!([]), 100);
        assert_eq!(fresh["selection"], json!(obs));
        assert_eq!(fresh["history_count"], json!(0));

        // A first session has no prior value: it reads 0, never errors.
        let one = select(json!([ki_session(obs, 0.6, 99)]), 100);
        assert_eq!(one["selection"], json!(pdca));
        assert_eq!(one["obs_auto_prev"], json!(0));
        assert_eq!(one["days_since_last"], json!(1));

        // The documented receipt, derived from a four-session log.
        let documented = select(
            json!([
                ki_session(obs, 0.55, 90),
                ki_session(pdca, 0.35, 91),
                ki_session(obs, 0.6, 98),
                ki_session(pdca, 0.4, 98)
            ]),
            100,
        );
        assert_eq!(documented["selection"], json!(pdca));
        assert_eq!(documented["obs_auto"], json!(0.6));
        assert_eq!(documented["obs_auto_prev"], json!(0.55));
        assert_eq!(documented["pdca_auto"], json!(0.4));
        assert_eq!(documented["pdca_auto_prev"], json!(0.35));

        let strong = json!([
            ki_session(obs, 0.55, 95),
            ki_session(pdca, 0.55, 96),
            ki_session(obs, 0.6, 98),
            ki_session(pdca, 0.6, 99)
        ]);
        assert_eq!(select(strong.clone(), 100)["selection"], json!("graduated"));
        // Exactly 0.5 is not above 0.5: no graduation, and the tie-break on
        // equal latest values (0.6 each) goes to the observation drill.
        let mut boundary = strong.clone();
        boundary[1]["automaticity"] = json!(0.5);
        assert_eq!(select(boundary, 100)["selection"], json!(obs));
        let mut lower_pdca = strong.clone();
        lower_pdca[1]["automaticity"] = json!(0.5);
        lower_pdca[3]["automaticity"] = json!(0.55);
        assert_eq!(select(lower_pdca, 100)["selection"], json!(pdca));
        // A stale log restarts with observation, days derived from the log;
        // six days is not stale, seven is.
        assert_eq!(select(strong.clone(), 105)["selection"], json!("graduated"));
        let seven = select(strong.clone(), 106);
        assert_eq!(seven["selection"], json!(obs));
        assert_eq!(seven["days_since_last"], json!(7));
        let stale = select(strong, 118);
        assert_eq!(stale["selection"], json!(obs));
        assert_eq!(stale["days_since_last"], json!(19));
        // Observation automaticity under 0.3 overrides a strong PDCA; at 0.3
        // the lower-latest rule applies instead.
        let weak_obs = select(
            json!([ki_session(obs, 0.2, 99), ki_session(pdca, 0.9, 99)]),
            100,
        );
        assert_eq!(weak_obs["selection"], json!(obs));
        let floor = select(
            json!([ki_session(obs, 0.3, 99), ki_session(pdca, 0.25, 99)]),
            100,
        );
        assert_eq!(floor["selection"], json!(pdca));
        let just_under = select(
            json!([ki_session(obs, 0.25, 99), ki_session(pdca, 0.2, 99)]),
            100,
        );
        assert_eq!(just_under["selection"], json!(obs));
        // One strong session per drill is not two: the prior reads 0, so a
        // learner cannot graduate from a single session each.
        let single_each = select(
            json!([ki_session(obs, 0.9, 99), ki_session(pdca, 0.9, 99)]),
            100,
        );
        assert_eq!(single_each["selection"], json!(obs));
        assert_eq!(single_each["obs_auto_prev"], json!(0));
    }

    fn ki_assessment(question: u64, assessment: &str) -> serde_json::Value {
        json!({"question": question, "coach_assessment": assessment})
    }

    #[test]
    fn scientific_gate_ki_2_drives_the_coaching_loop_from_the_rendered_assessments() {
        let form = skill_candidate("kata-improvement", "define passes");
        let next = |assessments: Vec<serde_json::Value>| {
            eval_sandboxed(&form, &json!({"assessments": assessments}))
                .unwrap_or_else(|error| panic!("KI-2 failed to evaluate: {error}"))
        };

        let start = next(vec![]);
        assert_eq!(start["verdict"], json!("ask"));
        assert_eq!(start["question"], json!(1));

        let vague = next(vec![ki_assessment(1, "vague")]);
        assert_eq!(vague["verdict"], json!("follow_up"));
        assert_eq!(vague["question"], json!(1));

        let second_vague = next(vec![ki_assessment(1, "vague"), ki_assessment(1, "vague")]);
        assert_eq!(second_vague["verdict"], json!("follow_up"));
        assert_eq!(second_vague["followups"], json!([1, 0, 0, 0, 0]));

        // The third non-passing assessment records the gap and moves on.
        let gap = next(vec![
            ki_assessment(1, "vague"),
            ki_assessment(1, "vague"),
            ki_assessment(1, "vague"),
        ]);
        assert_eq!(gap["verdict"], json!("ask"));
        assert_eq!(gap["question"], json!(2));
        assert_eq!(gap["statuses"][0], json!("gap_recorded"));
        assert_eq!(gap["gaps"], json!([1]));

        let through_q4 = vec![
            ki_assessment(1, "clear"),
            ki_assessment(2, "data-driven"),
            ki_assessment(3, "scattered"),
            ki_assessment(3, "focused"),
            ki_assessment(4, "testable"),
        ];
        let ask_five = next(through_q4.clone());
        assert_eq!(ask_five["verdict"], json!("ask"));
        assert_eq!(ask_five["question"], json!(5));

        let mut complete_run = through_q4;
        complete_run.push(ki_assessment(5, "tight-loop"));
        let complete = next(complete_run);
        assert_eq!(complete["verdict"], json!("complete"));
        assert_eq!(complete["followups"], json!([0, 0, 1, 0, 0]));
        assert_eq!(complete["gaps"], json!([]));

        let with_gap = next(vec![
            ki_assessment(1, "clear"),
            ki_assessment(2, "assumption-based"),
            ki_assessment(2, "assumption-based"),
            ki_assessment(2, "assumption-based"),
            ki_assessment(3, "focused"),
            ki_assessment(4, "testable"),
            ki_assessment(5, "tight-loop"),
        ]);
        assert_eq!(with_gap["verdict"], json!("complete_with_gaps"));
        assert_eq!(with_gap["gaps"], json!([2]));
        assert_eq!(with_gap["followups"], json!([0, 2, 0, 0, 0]));

        // A passing word for the wrong question is not a pass of this one.
        let wrong_word = next(vec![ki_assessment(1, "clear"), ki_assessment(2, "clear")]);
        assert_eq!(wrong_word["verdict"], json!("follow_up"));
        assert_eq!(wrong_word["question"], json!(2));
        // Vocabulary is exact.
        let cased = next(vec![ki_assessment(1, "Clear")]);
        assert_eq!(cased["verdict"], json!("follow_up"));
        let out_of_range = next(vec![ki_assessment(6, "clear")]);
        assert_eq!(out_of_range["verdict"], json!("invalid"));
        assert_eq!(out_of_range["malformed"], json!(1));
        let empty = next(vec![json!({"question": 1, "coach_assessment": ""})]);
        assert_eq!(empty["verdict"], json!("invalid"));
    }

    #[test]
    fn scientific_gate_ki_3_checks_the_focus_metric_from_the_storyboard_objects() {
        let form = skill_candidate("kata-improvement", "define max-experiments");
        let before = json!({"pass_rate": 5, "latency_ms": {"value": 300, "unit": "ms"}});
        let target = json!({"pass_rate": 10, "latency_ms": {"value": 200, "unit": "ms"}});
        let check = |focus: &str, after: serde_json::Value, experiments: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"metric_before": before, "metrics_target": target, "focus_metric": focus,
                        "metric_after": after, "experiments": experiments}),
            )
            .unwrap_or_else(|error| panic!("KI-3 failed to evaluate: {error}"))
        };

        // The documented receipt: baseline 5, target 10, observed 7.
        let partial = check("pass_rate", json!(7), json!([]));
        assert_eq!(partial["verdict"], json!("continue"));
        assert_eq!(partial["gap_to_target"], json!(3));
        assert_eq!(partial["improvement"], json!(2));
        assert_eq!(partial["experiment"], json!(1));

        let met = check("pass_rate", json!(10), json!([]));
        assert_eq!(met["verdict"], json!("target_attained"));
        assert_eq!(met["gap_to_target"], json!(0));

        let regressed = check("pass_rate", json!(4), json!([{}, {}]));
        assert_eq!(regressed["verdict"], json!("stop_bound"));
        assert_eq!(regressed["improvement"], json!(-1));
        assert_eq!(regressed["experiment"], json!(3));
        let second = check("pass_rate", json!(4), json!([{}]));
        assert_eq!(second["verdict"], json!("continue"));

        let pending = check("pass_rate", serde_json::Value::Null, json!([]));
        assert_eq!(pending["verdict"], json!("pending_check"));
        assert_eq!(pending["gap_to_target"], serde_json::Value::Null);

        // A lower-is-better metric carried as {value, unit}: at or below the
        // target is attained, above it is not.
        let faster = check("latency_ms", json!(190), json!([]));
        assert_eq!(faster["verdict"], json!("target_attained"));
        assert_eq!(faster["improvement"], json!(-110));
        let slower = check("latency_ms", json!(250), json!([]));
        assert_eq!(slower["verdict"], json!("continue"));

        let missing = check("throughput", json!(1), json!([]));
        assert_eq!(missing["verdict"], json!("invalid"));
        assert_eq!(
            missing["problems"],
            json!([
                "focus_metric_missing_in_metric_before",
                "focus_metric_missing_in_metrics_target"
            ])
        );
        let textual = check("pass_rate", json!("7"), json!([]));
        assert_eq!(textual["verdict"], json!("invalid"));
        assert_eq!(textual["problems"], json!(["metric_after_not_a_number"]));
    }

    #[test]
    fn scientific_gate_alg_1_reads_the_loop_reading_as_returned() {
        let form = skill_candidate("algedonic-review", "investigate_wiring");
        let read = |value: serde_json::Value| {
            eval_sandboxed(&form, &json!({"loop_reading": value}))
                .unwrap_or_else(|error| panic!("ALG-1 failed to evaluate: {error}"))
        };
        assert_eq!(read(json!("homeostatic")), json!("proceed"));
        assert_eq!(read(json!("turning")), json!("proceed"));
        assert_eq!(read(json!("broken")), json!("investigate_wiring"));
        assert_eq!(read(json!("wiring-closed")), json!("investigate_wiring"));
        assert_eq!(read(json!("unobserved")), json!("unobserved"));
        assert_eq!(read(serde_json::Value::Null), json!("unobserved"));
    }

    #[test]
    fn scientific_gate_alg_2_names_the_open_cards_without_a_recorded_comment() {
        let form = skill_candidate("algedonic-review", "define silent");
        let check = |tasks: serde_json::Value| {
            eval_sandboxed(&form, &json!({"tasks": tasks}))
                .unwrap_or_else(|error| panic!("ALG-2 failed to evaluate: {error}"))
        };
        let card = |id: &str, status: &str, commented: bool| {
            let activity = if commented {
                json!({"text": "observed: settled", "kind": "comment", "at": "2026-11-12T10:00:00Z"})
            } else {
                serde_json::Value::Null
            };
            json!({"task_id": id, "board_id": "b", "title": id, "status": status,
                   "assignee": null, "criteria": [], "advances_count": 0, "activity": activity})
        };

        let mixed = check(json!([
            card("a", "review", true),
            card("b", "review", false),
            card("c", "backlog", false),
            card("d", "in_progress", true),
            card("e", "done", false),
        ]));
        assert_eq!(mixed["verdict"], json!("gaps_remain"));
        assert_eq!(mixed["review_without_observation"], json!(["b"]));
        assert_eq!(mixed["open_without_next_action"], json!(["c"]));
        assert_eq!(mixed["cards_read"], json!(5));

        let ready_gap = check(json!([card("r", "ready", false)]));
        assert_eq!(ready_gap["open_without_next_action"], json!(["r"]));

        let clean = check(json!([
            card("a", "review", true),
            card("d", "in_progress", true)
        ]));
        assert_eq!(clean["verdict"], json!("target_met"));
        let empty = check(json!([]));
        assert_eq!(empty["verdict"], json!("target_met"));
        assert_eq!(empty["cards_read"], json!(0));

        // A failed read is unknown, never clear.
        assert_eq!(
            check(serde_json::Value::Null)["verdict"],
            json!("board_unread")
        );
        assert_eq!(
            check(json!("board not found"))["verdict"],
            json!("board_unread")
        );
    }

    /// A `dme-harness summary` object as the harness writes it (the fields
    /// the inline gates read).
    fn dme_summary(n: u64, hit: u64, wrong: u64, correct: u64, hash: &str) -> serde_json::Value {
        json!({"n": n, "counts": {"hit_limit": hit, "wrong_complete": wrong, "correct_complete": correct},
               "gguf_sha256": hash})
    }

    #[test]
    fn scientific_gate_dme_1_routes_from_the_summary_file_and_checks_it_sums() {
        let form = skill_candidate("direct-model-edit", "summary-counts-do-not-sum");
        let route = |summary: serde_json::Value| {
            eval_sandboxed(&form, &json!({"summary": summary}))
                .unwrap_or_else(|error| panic!("DME-1 failed to evaluate: {error}"))
        };
        let anchor = route(dme_summary(96, 85, 4, 7, "a"));
        assert_eq!(anchor[0], json!("blocker-removal-route"));
        assert_eq!(anchor[1]["n"], json!(96));
        let gemma = route(dme_summary(100, 8, 60, 32, "a"));
        assert_eq!(gemma[0], json!("capability-gap-outside-this-skill"));
        assert_eq!(gemma[1]["wrong_complete_share"], json!(0.6));
        assert_eq!(
            route(dme_summary(100, 30, 30, 40, "a"))[0],
            json!("collect-more-evidence")
        );
        // Exactly half is the route threshold.
        assert_eq!(
            route(dme_summary(100, 50, 10, 40, "a"))[0],
            json!("blocker-removal-route")
        );
        assert_eq!(
            route(dme_summary(100, 49, 50, 1, "a"))[0],
            json!("capability-gap-outside-this-skill")
        );
        assert_eq!(
            route(dme_summary(0, 0, 0, 0, "a"))[0],
            json!("empty-baseline")
        );
        let short = route(dme_summary(96, 85, 4, 6, "a"));
        assert_eq!(short[0], json!("summary-counts-do-not-sum"));
        assert_eq!(short[1]["sum"], json!(95));
        assert_eq!(route(json!({"n": 96}))[0], json!("summary-malformed"));
        assert_eq!(
            route(
                json!({"n": "96", "counts": {"hit_limit": 85, "wrong_complete": 4, "correct_complete": 7}})
            )[0],
            json!("summary-malformed")
        );
    }

    #[test]
    fn scientific_gate_dme_2_reads_the_surface_from_the_convert_manifest() {
        let form = skill_candidate("direct-model-edit", "manifest-missing-projection");
        let check = |manifest: serde_json::Value| {
            eval_sandboxed(&form, &json!({"manifest": manifest}))
                .unwrap_or_else(|error| panic!("DME-2 failed to evaluate: {error}"))
        };
        let anchor = check(
            json!({"projected_weights_touched": 131072, "total_weights": 752393024,
                                  "projected_edited_share": 0.00017420682518183475}),
        );
        assert_eq!(anchor[0], json!("within-budget"));
        assert_eq!(anchor[1]["budget"], json!(752393.024));
        assert_eq!(anchor[1]["edited_weights"], json!(131072));
        assert_eq!(
            check(json!({"projected_weights_touched": 752393, "total_weights": 752393024}))[0],
            json!("within-budget")
        );
        // Exactly the cap is within budget.
        assert_eq!(
            check(json!({"projected_weights_touched": 1000, "total_weights": 1000000}))[0],
            json!("within-budget")
        );
        assert_eq!(
            check(json!({"projected_weights_touched": 1001, "total_weights": 1000000}))[0],
            json!("over-budget")
        );
        assert_eq!(
            check(json!({"projected_weights_touched": 752394, "total_weights": 752393024}))[0],
            json!("over-budget")
        );
        assert_eq!(
            check(json!({"projected_weights_touched": 800000, "total_weights": 752393024}))[0],
            json!("over-budget")
        );
        let lying = check(
            json!({"projected_weights_touched": 131072, "total_weights": 752393024,
                                 "projected_edited_share": 0.5}),
        );
        assert_eq!(lying[0], json!("manifest-share-disagrees-with-counts"));
        assert_eq!(lying[1]["stated"], json!(0.5));
        assert_eq!(check(json!({}))[0], json!("manifest-missing-projection"));
        assert_eq!(
            check(json!({"projected_weights_touched": 1, "total_weights": 0}))[0],
            json!("manifest-missing-projection")
        );
    }

    #[test]
    fn scientific_gate_dme_3b_computes_the_sibling_delta_from_two_summaries() {
        let form = skill_candidate("direct-model-edit", "sibling-runs-differ-in-size");
        let verdict = |before: serde_json::Value, after: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"sibling_before": before, "sibling_after": after}),
            )
            .unwrap_or_else(|error| panic!("DME-3b failed to evaluate: {error}"))
        };
        // The anchor's Gemma control: 6.8 -> 7.0 points.
        let flat = verdict(
            dme_summary(500, 10, 456, 34, "base"),
            dme_summary(500, 10, 455, 35, "edited"),
        );
        assert_eq!(flat[0], json!("sibling-flat"));
        assert!((flat[1]["control_delta"].as_f64().expect("delta") - 0.2).abs() < 1e-9);
        assert_eq!(flat[1]["flat_band"], json!(1.0));
        // Exactly one point is flat; just over is not.
        assert_eq!(
            verdict(
                dme_summary(500, 10, 456, 34, "base"),
                dme_summary(500, 10, 451, 39, "edited")
            )[0],
            json!("sibling-flat")
        );
        assert_eq!(
            verdict(
                dme_summary(500, 10, 456, 34, "base"),
                dme_summary(500, 10, 450, 40, "edited")
            )[0],
            json!("sibling-moved-general-booster-suspect")
        );
        // A loss beyond the band is a move too.
        assert_eq!(
            verdict(
                dme_summary(500, 10, 456, 34, "base"),
                dme_summary(500, 10, 466, 24, "edited")
            )[0],
            json!("sibling-moved-general-booster-suspect")
        );
        assert_eq!(
            verdict(
                dme_summary(500, 10, 456, 34, "base"),
                dme_summary(500, 10, 455, 35, "base")
            )[0],
            json!("sibling-runs-on-the-same-model-file")
        );
        assert_eq!(
            verdict(
                dme_summary(500, 10, 456, 34, "base"),
                dme_summary(400, 10, 355, 35, "edited")
            )[0],
            json!("sibling-runs-differ-in-size")
        );
        assert_eq!(
            verdict(json!({"n": 5}), json!({}))[0],
            json!("sibling-summary-malformed")
        );
    }

    #[test]
    fn scientific_gate_dme_4_converges_on_the_hit_limit_counts_of_the_pass_summaries() {
        let form = skill_candidate("direct-model-edit", "define max-passes");
        let pass = |hit: u64| dme_summary(96, hit, 0, 96 - hit, "e");
        let check = |passes: Vec<serde_json::Value>| {
            eval_sandboxed(&form, &json!({"passes": passes}))
                .unwrap_or_else(|error| panic!("DME-4 failed to evaluate: {error}"))
        };
        let converged = check(vec![pass(61), pass(12), pass(12), pass(12)]);
        assert_eq!(converged[0], json!("converged"));
        assert_eq!(converged[1], json!([61, 12, 12, 12]));
        assert_eq!(
            check(vec![pass(61), pass(12), pass(12), pass(4)])[0],
            json!("continue")
        );
        assert_eq!(check(vec![pass(12), pass(12)])[0], json!("continue"));
        assert_eq!(
            check(vec![pass(12), pass(12), pass(12)])[0],
            json!("converged")
        );
        assert_eq!(
            check(vec![pass(61), pass(50), pass(40), pass(30), pass(20)])[0],
            json!("stop-bound-unstable")
        );
        // A fifth pass that converges is converged, not stopped.
        assert_eq!(
            check(vec![pass(61), pass(50), pass(20), pass(20), pass(20)])[0],
            json!("converged")
        );
        assert_eq!(
            check(vec![
                pass(61),
                pass(50),
                pass(40),
                pass(30),
                pass(20),
                pass(10)
            ])[0],
            json!("pass-bound-exceeded")
        );
        assert_eq!(
            check(vec![json!({"n": 1})])[0],
            json!("pass-summary-malformed")
        );
    }

    #[test]
    fn scientific_gate_vc_1_checks_the_candidate_partition_and_the_signal_mapping() {
        let form = skill_candidate("verification-compression", "removed_without_mapping");
        let inventory = json!([
            {"artifact_id": "artifact:a"}, {"artifact_id": "artifact:b"}, {"artifact_id": "artifact:c"}
        ]);
        let candidate = |removed: &[&str], retained: &[&str], merged: &[&str]| json!({"removed_artifacts": removed, "retained_artifacts": retained, "merged_artifacts": merged});
        let mapping = |removed: &str, retained: &str, status: &str| {
            json!({"removed_artifact": removed, "retained_artifact": retained, "status": status,
                   "signal_key": {}, "empirical_falsifier": "f"})
        };
        let gate = |candidate: serde_json::Value, mapping: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"artifact_inventory": inventory, "candidate": candidate, "signal_mapping": mapping}),
            )
            .unwrap_or_else(|error| panic!("VC-1 failed to evaluate: {error}"))
        };

        let covered = json!([mapping("artifact:c", "artifact:a", "covered")]);
        let pass = gate(
            candidate(&["artifact:c"], &["artifact:a", "artifact:b"], &[]),
            covered.clone(),
        );
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["problems"], json!([]));
        assert_eq!(pass["counts"]["removed"], json!(1));

        let unclassified = gate(
            candidate(&["artifact:c"], &["artifact:a"], &[]),
            covered.clone(),
        );
        assert_eq!(unclassified["problems"], json!(["unclassified_artifacts"]));
        assert_eq!(unclassified["unclassified"], json!(["artifact:b"]));

        let twice = gate(
            candidate(
                &["artifact:c"],
                &["artifact:a", "artifact:b", "artifact:c"],
                &[],
            ),
            covered.clone(),
        );
        assert_eq!(twice["problems"], json!(["artifact_in_two_classes"]));

        let merged_ok = gate(
            candidate(&[], &["artifact:a"], &["artifact:b", "artifact:c"]),
            json!([]),
        );
        assert_eq!(merged_ok["verdict"], json!("pass"));

        let missing = gate(
            candidate(&["artifact:c"], &["artifact:a", "artifact:b"], &[]),
            json!([mapping("artifact:c", "artifact:a", "missing")]),
        );
        assert_eq!(missing["problems"], json!(["unpreserved_signals"]));
        assert_eq!(missing["unpreserved"], json!(["artifact:c"]));

        let unmapped = gate(
            candidate(&["artifact:c"], &["artifact:a", "artifact:b"], &[]),
            json!([]),
        );
        assert_eq!(unmapped["problems"], json!(["removed_without_mapping"]));
        assert_eq!(unmapped["removed_without_mapping"], json!(["artifact:c"]));

        let stray_target = gate(
            candidate(&["artifact:c"], &["artifact:a", "artifact:b"], &[]),
            json!([mapping("artifact:c", "artifact:z", "covered")]),
        );
        assert_eq!(stray_target["problems"], json!(["unpreserved_signals"]));

        let outside = gate(
            candidate(
                &["artifact:c", "artifact:d"],
                &["artifact:a", "artifact:b"],
                &[],
            ),
            covered,
        );
        assert_eq!(
            outside["problems"],
            json!(["artifacts_outside_inventory", "removed_without_mapping"])
        );
        assert_eq!(outside["outside_inventory"], json!(["artifact:d"]));

        let all_retained = gate(
            candidate(&[], &["artifact:a", "artifact:b", "artifact:c"], &[]),
            json!([]),
        );
        assert_eq!(all_retained["verdict"], json!("pass"));

        let empty = eval_sandboxed(
            &form,
            &json!({"artifact_inventory": [], "candidate": candidate(&[], &[], &[]), "signal_mapping": []}),
        )
        .expect("empty inventory evaluates");
        assert_eq!(empty["problems"], json!(["empty_inventory"]));
    }

    fn pa_check(id: &str, verdict: &str, evidence: &str) -> serde_json::Value {
        json!({"id": id, "verdict": verdict, "evidence": evidence, "note": ""})
    }

    #[test]
    fn scientific_gate_pa_2_reads_the_inbound_and_outbound_checks_as_rendered() {
        let form = skill_candidate("provider-audit", "uncited_failures");
        let gate = |inbound: serde_json::Value,
                    outbound: serde_json::Value,
                    trigger: &str,
                    replacements: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"inbound": inbound, "outbound": outbound, "trigger": trigger,
                        "replacement_providers": replacements}),
            )
            .unwrap_or_else(|error| panic!("PA-2 failed to evaluate: {error}"))
        };
        let cited = gate(
            json!({"checks": [pa_check("B1", "pass", "mcp_servers.rs:12"), pa_check("B3", "fail", "https://v/terms 2026-11-13")]}),
            json!({"checks": [pa_check("O1", "pass", "providers.rs:40"), pa_check("O2", "unverified", "")]}),
            "scheduled",
            json!([]),
        );
        assert_eq!(cited["verdict"], json!("pass"));
        assert_eq!(cited["failed"], json!(["B3"]));
        assert_eq!(cited["unverified"], json!(["O2"]));
        assert_eq!(cited["checks"], json!(4));

        let uncited = gate(
            json!({"checks": [pa_check("B3", "fail", "")]}),
            json!({"checks": []}),
            "scheduled",
            json!([]),
        );
        assert_eq!(uncited["verdict"], json!("blocked"));
        assert_eq!(uncited["problems"], json!(["uncited_failures"]));
        assert_eq!(uncited["uncited"], json!(["B3"]));

        let cased = gate(
            json!({"checks": [pa_check("B1", "PASS", "x")]}),
            json!({}),
            "drift",
            json!([]),
        );
        assert_eq!(cased["problems"], json!(["malformed_checks"]));
        assert_eq!(cased["malformed"], json!(["B1"]));

        let empty = gate(json!({}), json!({}), "drift", json!([]));
        assert_eq!(empty["problems"], json!(["no_checks"]));

        let ok = json!({"checks": [pa_check("B1", "pass", "x")]});
        let retire_bare = gate(ok.clone(), json!({"checks": []}), "retirement", json!([]));
        assert_eq!(
            retire_bare["problems"],
            json!(["retirement_without_replacement"])
        );
        let retire_ok = gate(
            ok.clone(),
            json!({"checks": []}),
            "retirement",
            json!(["openrouter"]),
        );
        assert_eq!(retire_ok["verdict"], json!("pass"));
        // Replacements are only required on the retirement trigger.
        let adoption = gate(ok, json!({"checks": []}), "adoption", json!([]));
        assert_eq!(adoption["verdict"], json!("pass"));
    }

    #[test]
    fn scientific_gate_pa_3_calibrates_from_the_two_retrodiction_renders() {
        let form = skill_candidate("provider-audit", "define good-b1");
        let gate = |good: serde_json::Value, bad: serde_json::Value| {
            eval_sandboxed(&form, &json!({"known_good": good, "known_bad": bad}))
                .unwrap_or_else(|error| panic!("PA-3 failed to evaluate: {error}"))
        };
        let good = json!({"checks": [pa_check("B1", "pass", "x")]});
        let b1_fail = json!({"checks": [pa_check("B1", "fail", "y"), pa_check("B2", "pass", "z")]});
        let b2_fail = json!({"checks": [pa_check("B1", "pass", "y"), pa_check("B2", "fail", "z")]});
        let clean = json!({"checks": [pa_check("B1", "pass", "y"), pa_check("B2", "pass", "z")]});
        assert_eq!(
            gate(good.clone(), b1_fail.clone())["verdict"],
            json!("calibrated")
        );
        assert_eq!(gate(good.clone(), b2_fail)["verdict"], json!("calibrated"));
        let missed = gate(good.clone(), clean);
        assert_eq!(missed["verdict"], json!("uncalibrated"));
        assert_eq!(missed["known_bad_b1"], json!("pass"));
        let good_fails = gate(json!({"checks": [pa_check("B1", "fail", "x")]}), b1_fail);
        assert_eq!(good_fails["verdict"], json!("uncalibrated"));
        let empty = gate(json!({}), json!({}));
        assert_eq!(empty["verdict"], json!("uncalibrated"));
        assert_eq!(empty["known_good_b1"], json!(""));
        // Only B1 of the known-good counts: a B2 pass there is not calibration,
        // and a passing B1 listed after a failing B2 still is.
        let wrong_check = gate(json!({"checks": [pa_check("B2", "pass", "x")]}), good);
        assert_eq!(wrong_check["verdict"], json!("uncalibrated"));
        let b1_second = gate(
            json!({"checks": [pa_check("B2", "fail", "x"), pa_check("B1", "pass", "x")]}),
            json!({"checks": [pa_check("B1", "fail", "y")]}),
        );
        assert_eq!(b1_second["verdict"], json!("calibrated"));
    }

    #[test]
    fn scientific_gate_pa_4_converges_on_routed_blockers_calibration_and_the_log() {
        let form = skill_candidate("provider-audit", "define max-reentries");
        let calibrated = json!({"verdict": "calibrated"});
        let gate = |blocking: serde_json::Value,
                    routed: serde_json::Value,
                    calibration: serde_json::Value,
                    log: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"inbound_renders": [{"blocking_failures": blocking}],
                        "findings": {"routed": {"blocking": routed}}, "calibration": calibration, "log": log}),
            )
            .unwrap_or_else(|error| panic!("PA-4 failed to evaluate: {error}"))
        };
        let done = gate(
            json!(["B1"]),
            json!([{"id": "B1", "evidence": "e"}]),
            calibrated.clone(),
            json!([]),
        );
        assert_eq!(done["verdict"], json!("done"));
        assert_eq!(done["blocking_unrouted"], json!([]));
        let open = gate(
            json!(["B1", "B3"]),
            json!([{"id": "B1"}]),
            calibrated.clone(),
            json!([]),
        );
        assert_eq!(open["verdict"], json!("iterate"));
        assert_eq!(open["blocking_unrouted"], json!(["B3"]));
        let once = gate(
            json!(["B1", "B3"]),
            json!([{"id": "B1"}]),
            calibrated.clone(),
            json!([{}]),
        );
        assert_eq!(once["verdict"], json!("iterate"));
        let twice = gate(
            json!(["B1", "B3"]),
            json!([{"id": "B1"}]),
            calibrated.clone(),
            json!([{}, {}]),
        );
        assert_eq!(twice["verdict"], json!("stop-and-report"));
        assert_eq!(twice["reentries"], json!(2));
        let uncalibrated = gate(
            json!([]),
            json!([]),
            json!({"verdict": "uncalibrated"}),
            json!([]),
        );
        assert_eq!(uncalibrated["verdict"], json!("iterate"));
        assert_eq!(uncalibrated["calibrated"], json!(false));
        // Nothing blocking and calibrated is done even with a log.
        let quiet = gate(json!([]), json!([]), calibrated, json!([{}]));
        assert_eq!(quiet["verdict"], json!("done"));
    }

    #[test]
    fn scientific_gate_ktm_1_derives_phase_goal_readiness_and_wip_from_the_inputs() {
        let form = skill_candidate("kanban-task-management", "column_not_found");
        let columns = json!([
            {"name": "Backlog", "status": "backlog", "wip_limit": null},
            {"name": "In Progress", "status": "in_progress", "wip_limit": 2}
        ]);
        let gate = |description: &str,
                    task: serde_json::Value,
                    board: &str,
                    goal: serde_json::Value,
                    status: &str,
                    tasks: usize| {
            eval_sandboxed(
                &form,
                &json!({"project_description": description, "task_to_delegate": task, "board_id": board, "goal": goal,
                        "board_columns": columns, "target_status": status, "column_tasks": vec![json!({}); tasks]}),
            )
            .unwrap_or_else(|error| panic!("KTM-1 failed to evaluate: {error}"))
        };
        let goal = json!({"id": "g1", "criteria": [{"description": "a"}, {"description": "b"}]});

        let decompose = gate(
            "build the thing",
            serde_json::Value::Null,
            "",
            goal.clone(),
            "in_progress",
            2,
        );
        assert_eq!(decompose["phase"], json!("decompose"));
        assert_eq!(decompose["goal_ready"], json!(true));
        assert_eq!(decompose["criteria"], json!(2));
        assert_eq!(decompose["wip"], json!("at_limit"));

        let delegate = gate(
            "",
            json!({"task_id": "t"}),
            "",
            serde_json::Value::Null,
            "in_progress",
            1,
        );
        assert_eq!(delegate["phase"], json!("delegate"));
        assert_eq!(delegate["goal_ready"], json!(false));
        assert_eq!(delegate["wip"], json!("under_limit"));

        let operate = gate(
            "",
            serde_json::Value::Null,
            "board-1",
            json!({"id": "g", "criteria": []}),
            "backlog",
            7,
        );
        assert_eq!(operate["phase"], json!("operate"));
        assert_eq!(
            operate["goal_ready"],
            json!(false),
            "a goal with no criteria is not ready"
        );
        assert_eq!(operate["wip"], json!("no_limit"));

        // A null task object is not a task; an empty object is not either.
        assert_eq!(
            gate(
                "",
                serde_json::Value::Null,
                "",
                serde_json::Value::Null,
                "review",
                0
            )["phase"],
            json!("missing_input")
        );
        assert_eq!(
            gate("", json!({}), "", serde_json::Value::Null, "review", 0)["phase"],
            json!("missing_input")
        );
        // Decompose wins when several inputs are present.
        assert_eq!(
            gate("p", json!({"task_id": "t"}), "b", goal.clone(), "review", 0)["phase"],
            json!("decompose")
        );
        assert_eq!(
            gate("p", serde_json::Value::Null, "", goal, "review", 0)["wip"],
            json!("column_not_found")
        );
    }

    fn ktm_row(satisfied: bool, class: &str) -> serde_json::Value {
        json!({"criterion": "c", "satisfied": satisfied, "oracle_class": class, "evidence": "e", "feedback": "f"})
    }

    #[test]
    fn scientific_gate_ktm_2_reduces_the_oracle_tiers_from_the_render_rows() {
        let form = skill_candidate("kanban-task-management", "asserted_marked_satisfied");
        let gate = |passed: bool, rows: Vec<serde_json::Value>| {
            eval_sandboxed(
                &form,
                &json!({"verification": {"passed": passed, "reasoning": "r", "verification_tier": "x", "criteria_results": rows}}),
            )
            .unwrap_or_else(|error| panic!("KTM-2 failed to evaluate: {error}"))
        };
        let documented = gate(
            true,
            vec![
                ktm_row(true, "ran-and-pasted"),
                ktm_row(true, "ran-and-pasted"),
                ktm_row(true, "demonstrated"),
            ],
        );
        assert_eq!(documented["tier"], json!("demonstrated"));
        assert_eq!(documented["pass"], json!(true));
        assert_eq!(documented["render_claim_agrees"], json!(true));
        assert_eq!(documented["criteria"], json!(3));

        let pasted = gate(
            true,
            vec![
                ktm_row(true, "ran-and-pasted"),
                ktm_row(true, "ran-and-pasted"),
            ],
        );
        assert_eq!(pasted["tier"], json!("ran-and-pasted"));
        assert_eq!(pasted["pass"], json!(true));

        let asserted = gate(
            false,
            vec![ktm_row(true, "ran-and-pasted"), ktm_row(false, "asserted")],
        );
        assert_eq!(asserted["tier"], json!("asserted"));
        assert_eq!(asserted["pass"], json!(false));
        assert_eq!(asserted["render_claim_agrees"], json!(true));

        let laundered = gate(true, vec![ktm_row(true, "asserted")]);
        assert_eq!(laundered["tier"], json!("asserted_marked_satisfied"));
        assert_eq!(laundered["pass"], json!(false));
        assert_eq!(laundered["render_claim_agrees"], json!(false));

        let unsatisfied = gate(true, vec![ktm_row(false, "ran-and-pasted")]);
        assert_eq!(unsatisfied["tier"], json!("unsatisfied"));
        assert_eq!(unsatisfied["pass"], json!(false));

        assert_eq!(
            gate(true, vec![ktm_row(true, "Demonstrated")])["tier"],
            json!("malformed_rows")
        );
        let none = gate(true, vec![]);
        assert_eq!(none["tier"], json!("no_criteria"));
        assert_eq!(none["pass"], json!(false));
    }

    #[test]
    fn scientific_gate_ktm_3_names_sweep_obstacles_from_the_relisted_rows() {
        let form = skill_candidate("kanban-task-management", "sweep_clean");
        let card = |id: &str, status: &str, commented: bool| {
            let activity = if commented {
                json!({"text": "ran: cargo test -- ok", "kind": "comment", "at": "2026-11-12T10:00:00Z"})
            } else {
                serde_json::Value::Null
            };
            json!({"task_id": id, "status": status, "activity": activity})
        };
        let gate = |tasks: serde_json::Value, stale: serde_json::Value| {
            eval_sandboxed(&form, &json!({"tasks": tasks, "stale_in_progress": stale}))
                .unwrap_or_else(|error| panic!("KTM-3 failed to evaluate: {error}"))
        };
        let mixed = gate(
            json!([
                card("a", "review", true),
                card("b", "review", false),
                card("c", "in_progress", true),
                card("e", "in_progress", true)
            ]),
            json!(["c", "gone"]),
        );
        assert_eq!(mixed["verdict"], json!("escalate"));
        assert_eq!(mixed["unverified_review"], json!(["b"]));
        assert_eq!(
            mixed["stalled_in_progress"],
            json!(["c"]),
            "a stale id no longer In Progress is dropped"
        );
        assert_eq!(mixed["cards_read"], json!(4));

        let clean = gate(
            json!([card("a", "review", true), card("c", "in_progress", true)]),
            json!([]),
        );
        assert_eq!(clean["verdict"], json!("sweep_clean"));
        assert_eq!(gate(json!([]), json!([]))["verdict"], json!("sweep_clean"));
        assert_eq!(
            gate(serde_json::Value::Null, json!([]))["verdict"],
            json!("board_unread")
        );
        // An uncommented Backlog card is not a sweep obstacle.
        assert_eq!(
            gate(json!([card("z", "backlog", false)]), json!([]))["verdict"],
            json!("sweep_clean")
        );
    }

    #[test]
    fn scientific_gate_sp_2_reads_the_quality_render_and_the_scenario_set() {
        let form = skill_candidate("scenario-planning", "scenario_count_not_four");
        let four = json!([{"name": "a"}, {"name": "b"}, {"name": "c"}, {"name": "d"}]);
        let quality = |status: &str,
                       div: serde_json::Value,
                       con: f64,
                       cov: f64,
                       flag: serde_json::Value| {
            json!({"gate_findings": [{"criterion": "divergence", "status": status, "note": "n"}],
                   "divergence_score": div, "consistency_score": con, "coverage_score": cov,
                   "parametric_variation_flag": flag})
        };
        let gate = |scenarios: serde_json::Value, quality: serde_json::Value| {
            eval_sandboxed(&form, &json!({"scenarios": scenarios, "quality": quality}))
                .unwrap_or_else(|error| panic!("SP-2 failed to evaluate: {error}"))
        };
        let pass = gate(
            four.clone(),
            quality("pass", json!(0.8), 0.7, 0.6, json!(false)),
        );
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["gate_pass"], json!(true));
        assert_eq!(pass["render_findings_agree"], json!(true));

        let low = gate(
            four.clone(),
            quality("fail", json!(0.59), 0.8, 0.8, json!(false)),
        );
        assert_eq!(low["verdict"], json!("revise"));
        assert_eq!(low["reasons"], json!(["divergence_below_0.6"]));
        assert_eq!(low["render_findings_agree"], json!(true));
        let silent_fail = gate(
            four.clone(),
            quality("pass", json!(0.5), 0.8, 0.8, json!(false)),
        );
        assert_eq!(silent_fail["verdict"], json!("revise"));
        assert_eq!(silent_fail["render_findings_agree"], json!(false));

        let three = gate(
            json!([{}, {}, {}]),
            quality("fail", json!(0.8), 0.8, 0.8, json!(false)),
        );
        assert_eq!(three["reasons"], json!(["scenario_count_not_four"]));
        let parametric = gate(
            four.clone(),
            quality("fail", json!(0.8), 0.8, 0.8, json!(true)),
        );
        assert_eq!(parametric["reasons"], json!(["parametric_variation"]));
        let several = gate(
            json!([{}, {}, {}, {}, {}]),
            quality("fail", json!(0.8), 0.5, 0.5, json!(true)),
        );
        assert_eq!(
            several["reasons"],
            json!([
                "scenario_count_not_four",
                "parametric_variation",
                "consistency_below_0.6",
                "coverage_below_0.6"
            ])
        );

        assert_eq!(
            gate(
                four.clone(),
                quality("pass", json!("high"), 0.8, 0.8, json!(false))
            )["verdict"],
            json!("malformed_render")
        );
        assert_eq!(
            gate(
                four.clone(),
                quality("pass", json!(1.2), 0.8, 0.8, json!(false))
            )["verdict"],
            json!("malformed_render")
        );
        assert_eq!(
            gate(four, quality("pass", json!(0.8), 0.8, 0.8, json!("no")))["verdict"],
            json!("malformed_render")
        );
    }

    #[test]
    fn scientific_gate_sp_3_completes_only_when_every_due_event_is_scored() {
        let form = skill_candidate("scenario-planning", "unresolved_past_deadline");
        let per_event = |id: &str, outcome: bool| json!({"event_id": id, "forecast_probability": 0.6, "outcome": outcome, "brier_score": 0.16, "scoring_status": "scored"});
        let score = |scored: u64, events: Vec<serde_json::Value>, unmatched: Vec<&str>| {
            json!({"forecast_id": "f", "event_count": events.len(), "scored_count": scored, "per_event": events,
                   "unmatched_outcome_ids": unmatched})
        };
        let gate = |score: serde_json::Value, due: Vec<&str>| {
            eval_sandboxed(&form, &json!({"score": score, "past_deadline_ids": due}))
                .unwrap_or_else(|error| panic!("SP-3 failed to evaluate: {error}"))
        };
        let two = score(
            2,
            vec![per_event("e1", true), per_event("e3", false)],
            vec![],
        );
        let complete = gate(two.clone(), vec!["e1", "e3"]);
        assert_eq!(complete["verdict"], json!("complete"));
        assert_eq!(complete["scored_count"], json!(2));
        assert_eq!(complete["calibration_claimable"], json!(false));
        assert_eq!(complete["resolved_events"], json!(["e1", "e3"]));

        let open = gate(two.clone(), vec!["e1", "e3", "e4"]);
        assert_eq!(open["verdict"], json!("unresolved_past_deadline"));
        assert_eq!(open["unresolved_past_deadline"], json!(["e4"]));
        // A future-dated event does not block completion.
        assert_eq!(gate(two, vec!["e1"])["verdict"], json!("complete"));

        let unmatched = gate(score(2, vec![per_event("e1", true)], vec!["zz"]), vec![]);
        assert_eq!(unmatched["verdict"], json!("outcomes_unmatched"));
        assert_eq!(
            gate(score(0, vec![], vec![]), vec![])["verdict"],
            json!("nothing_scored")
        );
        assert_eq!(
            gate(serde_json::Value::Null, vec![])["verdict"],
            json!("score_missing")
        );
        assert_eq!(
            gate(json!("error"), vec![])["verdict"],
            json!("score_missing")
        );
        let many = gate(score(12, vec![per_event("e1", true)], vec![]), vec!["e1"]);
        assert_eq!(many["calibration_claimable"], json!(true));
        let nine = gate(
            score(9, vec![per_event("e1", true)], vec!["e1"]),
            vec!["e1"],
        );
        assert_eq!(nine["calibration_claimable"], json!(false));
    }

    #[test]
    fn scientific_gate_ur_2_compares_unit_ids_across_the_inventory_order_and_force_table() {
        let form = skill_candidate("upstream-rebase", "units_missing_from_order");
        let units = json!([
            {"id": "u1", "line_start": 1, "line_end": 9, "purpose": "p"},
            {"id": "u2", "line_start": 10, "line_end": 20, "purpose": "q"}
        ]);
        let table = json!([
            {"id": "u1", "force": "Prohibition", "enforcement_point": "test x", "pinning_test": "t"},
            {"id": "u2", "force": "Guideline", "enforcement_point": "", "pinning_test": ""}
        ]);
        let gate = |units: serde_json::Value,
                    order: serde_json::Value,
                    table: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"functional_units": units, "topological_order": order, "constraint_force_table": table}),
            )
            .unwrap_or_else(|error| panic!("UR-2 failed to evaluate: {error}"))
        };
        // The map render's real shape: unit objects against an id order.
        let pass = gate(units.clone(), json!(["u2", "u1"]), table.clone());
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["units"], json!(2));

        let short = gate(units.clone(), json!(["u1"]), table.clone());
        assert_eq!(short["problems"], json!(["units_missing_from_order"]));
        assert_eq!(short["missing_from_order"], json!(["u2"]));
        let stray = gate(units.clone(), json!(["u1", "u2", "u9"]), table);
        assert_eq!(stray["problems"], json!(["order_names_unknown_units"]));
        assert_eq!(stray["unknown_in_order"], json!(["u9"]));

        let bare = gate(
            units.clone(),
            json!(["u1", "u2"]),
            json!([{"id": "u1", "force": "Prohibition", "enforcement_point": ""}]),
        );
        assert_eq!(
            bare["problems"],
            json!([
                "units_without_force",
                "prohibition_without_enforcement_point"
            ])
        );
        assert_eq!(bare["without_force"], json!(["u2"]));
        assert_eq!(bare["bare_prohibitions"], json!(["u1"]));

        let cased = gate(
            units,
            json!(["u1", "u2"]),
            json!([{"id": "u1", "force": "prohibition", "enforcement_point": "x"}, {"id": "u2", "force": "Guideline"}]),
        );
        assert_eq!(cased["problems"], json!(["unknown_force"]));
        assert_eq!(
            gate(json!([]), json!([]), json!([]))["problems"],
            json!(["no_units"])
        );
        let duplicated = gate(
            json!(["u1", "u1"]),
            json!(["u1"]),
            json!([{"id": "u1", "force": "Evidence"}]),
        );
        assert_eq!(duplicated["problems"], json!(["duplicate_unit_ids"]));
    }

    #[test]
    fn scientific_gate_ur_3_reads_printed_exits_the_grep_count_and_the_inventory() {
        let form = skill_candidate("upstream-rebase", "results_not_measured");
        let units = json!([{"id": "u1"}, {"id": "u2"}]);
        let gate = |check: serde_json::Value,
                    test: serde_json::Value,
                    invariant: serde_json::Value,
                    markers: serde_json::Value,
                    strategy: &str| {
            eval_sandboxed(
                &form,
                &json!({"check_exit": check, "test_exit": test, "invariant_exit": invariant, "marker_count": markers,
                        "functional_units": units, "strategy": strategy}),
            )
            .unwrap_or_else(|error| panic!("UR-3 failed to evaluate: {error}"))
        };
        let pass = gate(
            json!(0),
            json!(0),
            json!(0),
            json!(2),
            "mapped-reapplication",
        );
        assert_eq!(pass["verdict"], json!("pass"));
        assert_eq!(pass["unit_count"], json!(2));
        let short = gate(
            json!(0),
            json!(0),
            json!(0),
            json!(1),
            "mapped-reapplication",
        );
        assert_eq!(short["verdict"], json!("return_to_step_5"));
        assert_eq!(
            short["problems"],
            json!(["marker_count_differs_from_units"])
        );
        // Under git merge the marker count is not a re-application count.
        assert_eq!(
            gate(json!(0), json!(0), json!(0), json!(1), "git-merge")["verdict"],
            json!("pass")
        );
        let failing = gate(
            json!(101),
            json!(0),
            json!(1),
            json!(2),
            "mapped-reapplication",
        );
        assert_eq!(
            failing["problems"],
            json!(["cargo_check_failed", "no_zed_deps_invariant_failed"])
        );
        assert_eq!(
            gate(json!(0), json!(1), json!(0), json!(2), "git-merge")["problems"],
            json!(["pinning_tests_failed"])
        );
        let typed = gate(
            json!(true),
            json!(0),
            json!(0),
            json!(2),
            "mapped-reapplication",
        );
        assert_eq!(typed["problems"], json!(["results_not_measured"]));
        let text_count = gate(
            json!(0),
            json!(0),
            json!(0),
            json!("2"),
            "mapped-reapplication",
        );
        assert_eq!(text_count["problems"], json!(["results_not_measured"]));
    }

    #[test]
    fn scientific_gate_eur_1_reconciles_funding_sections_and_the_registry_readback() {
        let form = skill_candidate("eureka-cycle", "funded_without_section");
        let readback = json!({"count": 2, "experiments": [
            {"id": "exp-a", "prediction": {"claim": "c", "confidence": 0.7}},
            {"id": "exp-b", "prediction": {"claim": "d", "confidence": 0.4}}
        ]});
        let gate = |funded: Vec<&str>, sections: serde_json::Value, readback: serde_json::Value| {
            let funded: Vec<serde_json::Value> = funded
                .into_iter()
                .map(|id| json!({"candidate_id": id}))
                .collect();
            eval_sandboxed(
                &form,
                &json!({"funded_candidates": funded, "candidate_sections": sections, "readback": readback}),
            )
            .unwrap_or_else(|error| panic!("EUR-1 failed to evaluate: {error}"))
        };
        let two = json!([
            {"candidate_id": "c1", "experiment_id": "exp-a"},
            {"candidate_id": "c2", "experiment_id": "exp-b"}
        ]);
        let ok = gate(vec!["c1", "c2"], two.clone(), readback.clone());
        assert_eq!(ok["verdict"], json!("reconciled"));
        assert_eq!(ok["funded"], json!(2));
        assert_eq!(ok["sections"], json!(2));

        let missing = gate(
            vec!["c1", "c2"],
            json!([{"candidate_id": "c1", "experiment_id": "exp-a"}]),
            readback.clone(),
        );
        assert_eq!(missing["problems"], json!(["funded_without_section"]));
        assert_eq!(missing["funded_without_section"], json!(["c2"]));
        let extra = gate(vec!["c1"], two, readback.clone());
        assert_eq!(extra["problems"], json!(["section_without_funding"]));
        assert_eq!(extra["section_without_funding"], json!(["c2"]));

        let unresolved = gate(
            vec!["c1"],
            json!([{"candidate_id": "c1", "experiment_id": "exp-z"}]),
            readback.clone(),
        );
        assert_eq!(unresolved["problems"], json!(["experiment_ids_unresolved"]));
        assert_eq!(unresolved["unresolved_experiment_ids"], json!(["exp-z"]));

        let unsealed = gate(
            vec!["c1"],
            json!([{"candidate_id": "c1", "experiment_id": "exp-a"}]),
            json!({"experiments": [{"id": "exp-a", "prediction": {"claim": "c"}}]}),
        );
        assert_eq!(unsealed["problems"], json!(["confidence_not_recorded"]));
        assert_eq!(unsealed["confidence_not_recorded"], json!(["exp-a"]));

        // A research-run resolution has a section but no experiment id.
        let research = gate(
            vec!["c1"],
            json!([{"candidate_id": "c1"}]),
            readback.clone(),
        );
        assert_eq!(research["verdict"], json!("reconciled"));
        assert_eq!(
            gate(vec![], json!([]), readback)["problems"],
            json!(["no_funded_candidates"])
        );
    }

    #[test]
    fn scientific_gate_eur_2_scores_resolved_predictions_and_leaves_open_ones_unscored() {
        let form = skill_candidate("eureka-cycle", "cycle_mean_brier");
        let score = |predictions: serde_json::Value| {
            eval_sandboxed(&form, &json!({"predictions": predictions}))
                .unwrap_or_else(|error| panic!("EUR-2 failed to evaluate: {error}"))
        };
        let mixed = score(json!([
            {"claim": "a", "confidence": 0.7, "outcome": true},
            {"claim": "b", "confidence": 0.4, "outcome": false},
            {"claim": "c", "confidence": 0.9, "outcome": null}
        ]));
        let per = mixed["per_prediction"]
            .as_array()
            .expect("per prediction list");
        assert!((per[0].as_f64().expect("held") - 0.09).abs() < 1e-9);
        assert!((per[1].as_f64().expect("refuted") - 0.16).abs() < 1e-9);
        assert!(per[2].is_null());
        assert_eq!(mixed["scored"], json!(2));
        assert_eq!(mixed["unscored"], json!(1));
        assert!((mixed["cycle_mean_brier"].as_f64().expect("mean") - 0.125).abs() < 1e-9);

        let out_of_range = score(json!([{"confidence": 1.2, "outcome": true}]));
        assert_eq!(out_of_range["scored"], json!(0));
        assert!(out_of_range["cycle_mean_brier"].is_null());
        let textual = score(json!([{"confidence": "0.7", "outcome": true}]));
        assert_eq!(textual["unscored"], json!(1));
        let none = score(json!([]));
        assert_eq!(none["scored"], json!(0));
        assert!(none["cycle_mean_brier"].is_null());
        // A certain claim that held scores zero; one that was refuted scores one.
        let extremes = score(
            json!([{"confidence": 1.0, "outcome": true}, {"confidence": 1.0, "outcome": false}]),
        );
        assert_eq!(extremes["per_prediction"], json!([0.0, 1.0]));
    }

    #[test]
    fn scientific_gate_cr_2_computes_open_blockers_from_the_rows_and_the_verdict_from_them() {
        let form = skill_candidate("code-review", "health_assessment_missing");
        let finding = |id: &str, severity: &str, waived: bool| {
            let waiver = if waived {
                json!({"reason": "r", "by": "operator", "date": "d"})
            } else {
                serde_json::Value::Null
            };
            json!({"id": id, "severity": severity, "waiver": waiver})
        };
        let gate = |findings: Vec<serde_json::Value>,
                    rejected: usize,
                    fixed: Vec<&str>,
                    health: &str| {
            eval_sandboxed(
                &form,
                &json!({"adjudicated_findings": findings, "rejected_findings": vec![json!({}); rejected],
                        "prior_blockers": 0, "predicted_findings": 3, "predicted_blockers": 1,
                        "fixed_ids": fixed, "health_assessment": health}),
            )
            .unwrap_or_else(|error| panic!("CR-2 failed to evaluate: {error}"))
        };
        let mixed = vec![
            finding("CR-1", "Blocker", false),
            finding("CR-2", "Blocker", true),
            finding("CR-3", "Nit", false),
        ];
        let open = gate(mixed.clone(), 1, vec![], "improves");
        assert_eq!(open["verdict"], json!("Request changes"));
        assert_eq!(open["open_blockers"], json!(["CR-1"]));
        assert_eq!(open["waived_blockers"], json!(["CR-2"]));
        assert_eq!(open["severity_counts"]["blockers"], json!(2));
        assert_eq!(open["prediction_gap"], json!([1, 1]));
        assert_eq!(open["blocker_delta"], json!(2));

        let fixed = gate(mixed, 1, vec!["CR-1"], "improves");
        assert_eq!(fixed["verdict"], json!("Approve"));
        assert_eq!(fixed["open_blockers"], json!([]));
        // A fix id does not clear a waived Blocker's count, only its openness.
        assert_eq!(fixed["severity_counts"]["blockers"], json!(2));

        assert_eq!(
            gate(vec![finding("CR-3", "Nit", false)], 0, vec![], "neutral")["verdict"],
            json!("Comment")
        );
        assert_eq!(
            gate(vec![], 0, vec![], "regresses")["verdict"],
            json!("Request changes")
        );
        assert_eq!(
            gate(vec![], 0, vec![], "improves")["verdict"],
            json!("Approve")
        );
        assert_eq!(
            gate(vec![], 0, vec![], "Improves")["verdict"],
            json!("health_assessment_missing")
        );
        // A Should-fix is never an open Blocker.
        let should_fix = gate(
            vec![finding("CR-9", "Should-fix", false)],
            0,
            vec![],
            "improves",
        );
        assert_eq!(should_fix["open_blockers"], json!([]));
        assert_eq!(should_fix["verdict"], json!("Approve"));
    }

    #[test]
    fn scientific_gate_sa_2_reads_the_harness_rows_and_the_adjudicated_surfaces() {
        let form = skill_candidate("security-audit", "harness_version_not_pinned");
        let run = |version: &str| {
            json!({"row": "run", "script": "security-audit-probe-suite", "version": version, "target": "t",
                   "surfaces": "S2,S3", "started": "x"})
        };
        let probe = |id: &str, surface: &str, validated: bool| {
            json!({"row": "probe", "probe_id": id, "surface": surface, "instrument_validated": validated,
                   "production_matches": 0, "test_scoped_matches": 0, "matches": []})
        };
        let surfaces = json!([
            {"id": "S2", "status": "checked"}, {"id": "S3", "status": "checked"},
            {"id": "S6", "status": "checked"}, {"id": "S4", "status": "not_checked"}
        ]);
        let gate = |rows: Vec<serde_json::Value>, classes: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"harness_rows": rows, "surfaces": surfaces, "harness_classes": classes}),
            )
            .unwrap_or_else(|error| panic!("SA-2 failed to evaluate: {error}"))
        };
        let met = gate(
            vec![run("1.0"), probe("p1", "S2", true), probe("p2", "S3", true)],
            serde_json::Value::Null,
        );
        assert_eq!(met["verdict"], json!("floor_met"));
        assert_eq!(met["version"], json!("1.0"));
        assert_eq!(met["probes"], json!(2));

        let unvalidated = gate(
            vec![
                run("1.0"),
                probe("p1", "S2", true),
                probe("p2", "S3", false),
            ],
            serde_json::Value::Null,
        );
        assert_eq!(unvalidated["verdict"], json!("invalid"));
        assert_eq!(
            unvalidated["problems"],
            json!([
                "unvalidated_probes",
                "checked_surfaces_without_validated_rows"
            ])
        );
        assert_eq!(unvalidated["unvalidated_probes"], json!(["p2"]));
        assert_eq!(unvalidated["unbacked_surfaces"], json!(["S3"]));

        let unbacked = gate(
            vec![run("1.0"), probe("p1", "S2", true)],
            serde_json::Value::Null,
        );
        assert_eq!(
            unbacked["problems"],
            json!(["checked_surfaces_without_validated_rows"])
        );
        assert_eq!(
            unbacked["unbacked_surfaces"],
            json!(["S3"]),
            "S6 is outside the harness classes"
        );
        // When the harness was asked only for S2, a checked S3 is not its gap.
        assert_eq!(
            gate(vec![run("1.0"), probe("p1", "S2", true)], json!(["S2"]))["verdict"],
            json!("floor_met")
        );

        let drifted = gate(
            vec![run("1.1"), probe("p1", "S2", true), probe("p2", "S3", true)],
            serde_json::Value::Null,
        );
        assert_eq!(drifted["problems"], json!(["harness_version_not_pinned"]));
        let headless = gate(
            vec![probe("p1", "S2", true), probe("p2", "S3", true)],
            serde_json::Value::Null,
        );
        assert_eq!(headless["problems"], json!(["harness_run_header_missing"]));
        assert_eq!(
            gate(vec![], serde_json::Value::Null)["verdict"],
            json!("invalid")
        );
    }

    #[test]
    fn scientific_gate_tr_2_reads_the_render_the_pass_stats_and_the_locate_responses() {
        let form = skill_candidate("transcript-reel", "summary_quotes_unlocated");
        let render =
            json!({"output": "/g/reel.mp4", "clips": 5, "clip_plan": [], "gallery_asset_id": "a1"});
        let pass = |rate: serde_json::Value| json!({"layer_id": "l", "pass_stats": {"attempts": 10, "rejections": 1, "rejection_rate": rate}});
        let locate = |quote: &str, status: &str, count: u64| json!({"quote": quote, "status": status, "count": count, "ranges": []});
        let gate = |render: serde_json::Value,
                    pass: serde_json::Value,
                    locates: Vec<serde_json::Value>| {
            eval_sandboxed(
                &form,
                &json!({"render": render, "paragraph_pass": pass, "locate_results": locates}),
            )
            .unwrap_or_else(|error| panic!("TR-2 failed to evaluate: {error}"))
        };
        let green = gate(
            render.clone(),
            pass(json!(0.1)),
            vec![
                locate("the Cinderella curve", "located", 1),
                locate("second", "located", 2),
            ],
        );
        assert_eq!(green["verdict"], json!("converged"));
        assert_eq!(green["clips"], json!(5));
        assert_eq!(green["quotes_checked"], json!(2));

        let missed = gate(
            render.clone(),
            pass(json!(0.1)),
            vec![locate("x", "no_match", 0)],
        );
        assert_eq!(missed["problems"], json!(["summary_quotes_unlocated"]));
        assert_eq!(missed["unlocated_quotes"], json!(["x"]));
        // A located status with a zero count is not a location either.
        assert_eq!(
            gate(
                render.clone(),
                pass(json!(0.1)),
                vec![locate("y", "located", 0)]
            )["problems"],
            json!(["summary_quotes_unlocated"])
        );

        let empty = gate(json!({"output": "", "clips": 0}), pass(json!(0.1)), vec![]);
        assert_eq!(empty["problems"], json!(["reel_not_rendered", "no_clips"]));
        assert_eq!(
            gate(serde_json::Value::Null, pass(json!(0.1)), vec![])["problems"],
            json!(["reel_not_rendered", "no_clips"])
        );

        assert_eq!(
            gate(render.clone(), pass(json!(0.3)), vec![])["problems"],
            json!(["paragraph_rejection_rate_high"])
        );
        assert_eq!(
            gate(render.clone(), pass(json!(0.29)), vec![])["verdict"],
            json!("converged")
        );
        assert_eq!(
            gate(render.clone(), serde_json::Value::Null, vec![])["problems"],
            json!(["paragraph_pass_stats_missing"])
        );
        assert_eq!(
            gate(render, json!({"layer_id": "l"}), vec![])["problems"],
            json!(["paragraph_pass_stats_missing"])
        );
    }

    #[test]
    fn scientific_gate_sd_0_recomputes_every_fit_from_the_route_render() {
        let form = skill_candidate("skill-discovery", "too_many_recommendations");
        let rec = |name: &str, c: f64, d: f64, t: f64| {
            json!({"rank": 1, "skill_name": name,
                   "dimension_scores": {"capability_overlap": c, "description_alignment": d, "trigger_alignment": t}})
        };
        let no_boost = json!({"applied": false, "reason": "no epistemic_state supplied"});
        let gate = |recs: Vec<serde_json::Value>, boost: serde_json::Value| {
            eval_sandboxed(
                &form,
                &json!({"recommendations": recs, "boost_applied": boost, "max_recommendations": 3}),
            )
            .unwrap_or_else(|error| panic!("SD-0 failed to evaluate: {error}"))
        };
        let boosted = gate(
            vec![rec("falsifiability", 0.9, 0.8, 0.9)],
            json!({"applied": true, "reason": "confidence 0.3", "skills": ["falsifiability"]}),
        );
        assert_eq!(boosted["verdict"], json!("ok"));
        assert!(
            (boosted["best_fit"].as_f64().expect("fit") - 0.9).abs() < 1e-9,
            "t + 0.2 clamps at 1"
        );
        assert_eq!(boosted["coverage"], json!("full"));
        assert_eq!(boosted["fits"][0]["eligible"], json!(true));

        let partial = gate(vec![rec("a", 0.5, 0.5, 0.5)], no_boost.clone());
        assert!((partial["best_fit"].as_f64().expect("fit") - 0.5).abs() < 1e-9);
        assert_eq!(partial["coverage"], json!("partial"));
        // The band boundaries are exact: 0.79 is partial, 0.80 is full; 0.39 is none.
        assert_eq!(
            gate(vec![rec("a", 0.79, 0.79, 0.79)], no_boost.clone())["coverage"],
            json!("partial")
        );
        assert_eq!(
            gate(vec![rec("a", 0.8, 0.8, 0.8)], no_boost.clone())["coverage"],
            json!("full")
        );
        assert_eq!(
            gate(vec![rec("a", 0.39, 0.39, 0.39)], no_boost.clone())["coverage"],
            json!("none")
        );

        let weak = gate(vec![rec("a", 0.3, 0.2, 0.1)], no_boost.clone());
        assert!((weak["best_fit"].as_f64().expect("fit") - 0.225).abs() < 1e-9);
        assert_eq!(weak["coverage"], json!("none"));
        assert_eq!(weak["eligible_count"], json!(0));

        let mixed = gate(
            vec![rec("a", 0.3, 0.2, 0.1), rec("b", 0.9, 0.9, 0.9)],
            no_boost.clone(),
        );
        assert_eq!(mixed["coverage"], json!("full"));
        assert_eq!(mixed["eligible_count"], json!(1));
        assert_eq!(gate(vec![], no_boost.clone())["coverage"], json!("none"));

        let malformed = gate(vec![rec("a", 1.2, 0.2, 0.1)], no_boost.clone());
        assert_eq!(malformed["verdict"], json!("malformed_scores"));
        assert_eq!(malformed["malformed"], json!(["a"]));
        let four = gate(
            vec![
                rec("a", 0.5, 0.5, 0.5),
                rec("b", 0.5, 0.5, 0.5),
                rec("c", 0.5, 0.5, 0.5),
                rec("d", 0.5, 0.5, 0.5),
            ],
            no_boost,
        );
        assert_eq!(four["verdict"], json!("too_many_recommendations"));

        // The boost lifts only the named skill's trigger term, and only when applied.
        let named = gate(
            vec![rec("a", 0.5, 0.5, 0.5), rec("b", 0.5, 0.5, 0.5)],
            json!({"applied": true, "skills": ["a"]}),
        );
        assert!((named["fits"][0]["fit"].as_f64().expect("a") - 0.55).abs() < 1e-9);
        assert!((named["fits"][1]["fit"].as_f64().expect("b") - 0.5).abs() < 1e-9);
        let unapplied = gate(
            vec![rec("a", 0.5, 0.5, 0.5)],
            json!({"applied": false, "skills": ["a"]}),
        );
        assert!((unapplied["fits"][0]["fit"].as_f64().expect("a") - 0.5).abs() < 1e-9);
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
