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

        let company = skill_candidate("company-research-deep", "define pick");
        let capability_assessments = json!({
            "capability_assessments": {
                "obvious_problem": {"verdict":"maintain","elicited_score":90,"ceiling":100,"intervention":"maintain"},
                "invisible_gorilla": {"verdict":"floor_violation","elicited_score":60,"ceiling":100,"intervention":"maintain"},
                "combinatorial_solution": {"verdict":"maintain","elicited_score":70,"ceiling":100,"intervention":"maintain"},
                "choke_point": {"verdict":"maintain","elicited_score":80,"ceiling":100,"intervention":"maintain"}
            }
        });
        let company_result = eval_sandboxed(&company, &capability_assessments)
            .expect("company partition form should evaluate");
        assert_eq!(company_result[0], json!(["invisible_gorilla"]));
        assert_eq!(
            company_result[3],
            json!(["obvious_problem", "combinatorial_solution", "choke_point"])
        );
        assert_eq!(company_result[4], json!(74.0));
        assert_eq!(company_result[5], json!("SMALL_ANIMAL"));

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
