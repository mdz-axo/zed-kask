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

    fn grill_state_env() -> serde_json::Value {
        json!({
            "question_bank": [
                {"id": 11, "question": "What is two plus three?", "level": 2, "area": "arithmetic"},
                {"id": 12, "question": "When may an answer be graded?", "level": 2, "area": "evidence"}
            ],
            "answers": [
                {"id": 1, "question_id": 11, "text": "Six", "round": 1},
                {"id": 2, "question_id": 12, "text": "After it exists", "round": 1}
            ],
            "grade_events": [
                {"answer_id": 1, "rating": "Gap", "note": "Incorrect sum"},
                {"answer_id": 2, "rating": "Solid", "note": "Evidence precedes evaluation"}
            ],
            "level": 2, "round": 1
        })
    }

    #[test]
    fn grill_distinct_answers_not_grading_calls_drive_retirement() {
        let form = skill_candidate("grill-me", "define gm-state");
        let mut env = grill_state_env();
        let initial = eval_sandboxed(&form, &env).expect("initial authored grill state");
        assert_eq!(initial["attempt_counts"], json!([[11, 1], [12, 0]]));
        assert_eq!(initial["retired_ids"], json!([]));
        assert_eq!(initial["direction"], "hold");

        env["grade_events"]
            .as_array_mut()
            .expect("event fixture")
            .extend([
                json!({"answer_id": 1, "rating": "Gap", "note": "Repeated grading"}),
                json!({"answer_id": 1, "rating": "Gap", "note": "Another grading"}),
                json!({"answer_id": 1, "rating": "Solid", "note": "Revised semantic judgment"}),
            ]);
        let regraded = eval_sandboxed(&form, &env).expect("regrading the same answer");
        assert_eq!(regraded["attempt_counts"], initial["attempt_counts"]);
        assert_eq!(regraded["retired_ids"], json!([]));
        assert_eq!(regraded["solid_this_round"], 2);
        assert_eq!(regraded["direction"], "escalate");
        assert_eq!(regraded["next_level"], 3);
        assert_eq!(regraded["answer_records"][0]["rating"], "Solid");
        assert_eq!(regraded["answer_records"][0]["question_id"], 11);

        for (id, round) in [(3, 2), (4, 3)] {
            env["answers"]
                .as_array_mut()
                .expect("answer fixture")
                .push(json!({"id": id, "question_id": 11, "text": "Six", "round": round}));
            env["grade_events"]
                .as_array_mut()
                .expect("event fixture")
                .push(
                    json!({"answer_id": id, "rating": "Gap", "note": "A distinct failed answer"}),
                );
        }
        env["round"] = json!(3);
        let retired = eval_sandboxed(&form, &env).expect("three distinct failed answers");
        assert_eq!(retired["attempt_counts"], json!([[11, 3], [12, 0]]));
        assert_eq!(retired["retired_ids"], json!([11]));
        assert_eq!(retired["direction"], "reprobe");
    }

    #[test]
    fn grill_feedback_counts_raw_current_answers_and_preserves_bounds() {
        let form = skill_candidate("grill-me", "define gm-state");
        for (solid, level, round, direction, next) in [
            (4, 2, 1, "escalate", 3),
            (3, 2, 1, "hold", 2),
            (2, 2, 1, "hold", 2),
            (1, 2, 1, "reprobe", 2),
            (4, 5, 1, "complete", 5),
            (0, 2, 5, "complete", 2),
        ] {
            let mut env = grill_state_env();
            env["level"] = json!(level);
            env["round"] = json!(round);
            env["answers"] = json!(
                (1..=5)
                    .map(|id| json!({
                        "id": id, "question_id": 11, "text": "Fixture answer", "round": round
                    }))
                    .collect::<Vec<_>>()
            );
            env["grade_events"] = json!((1..=5).map(|id| json!({
                "answer_id": id, "rating": if id <= solid { "Solid" } else { "Gap" }, "note": "Fixture"
            })).collect::<Vec<_>>());
            let result = eval_sandboxed(&form, &env).expect("feedback boundary");
            assert_eq!(result["direction"], direction);
            assert_eq!(result["next_level"], next);
            assert_eq!(result["answered_this_round"], 5);
            assert_eq!(result["solid_this_round"], solid);
        }
        let mut env = grill_state_env();
        env["grade_events"] = json!([]);
        let awaiting = eval_sandboxed(&form, &env).expect("ungraded answers");
        assert_eq!(awaiting["direction"], "await_grades");
        assert_eq!(awaiting["ungraded_answer_ids"], json!([1, 2]));
        env["answers"] = json!([]);
        env["round"] = json!(5);
        let empty = eval_sandboxed(&form, &env).expect("no answers at round bound");
        assert_eq!(empty["direction"], "hold");
        assert_eq!(
            empty["stop"], true,
            "zero-answer hold must not open round six"
        );

        for changes in [
            json!({"level": 0}),
            json!({"round": 6}),
            json!({"round": 1.0}),
            json!({"answers": [{"id": 1, "question_id": 99, "text": "X", "round": 1}]}),
            json!({"answers": [{"id": 1, "question_id": 11, "text": "", "round": 1}]}),
            json!({"grade_events": [{"answer_id": 99, "rating": "Solid", "note": "Unknown"}]}),
            json!({"grade_events": [{"answer_id": 1, "rating": "solid", "note": "Wrong case"}]}),
            json!({"answers": [
                {"id": 1, "question_id": 11, "text": "X", "round": 1},
                {"id": 1, "question_id": 12, "text": "Y", "round": 1}
            ]}),
        ] {
            let invalid = scientific_env(&grill_state_env(), changes);
            assert_eq!(
                eval_sandboxed(&form, &invalid).expect("invalid state is surfaced"),
                "invalid_state"
            );
        }
    }

    #[test]
    fn grill_grade_batches_reconcile_exact_answer_identities() {
        let form = skill_candidate("grill-me", "define gm-grade-batch");
        let env = json!({
            "answer_ids": [1, 2], "evaluations": [
                {"answer_id": 2, "rating": "Solid", "note": "Second answer first"},
                {"answer_id": 1, "rating": "Gap", "note": "ID join, not position"}
            ], "mode": "interrogate", "calibrated_level": null
        });
        assert_eq!(
            eval_sandboxed(&form, &env).expect("out of order ID batch"),
            true
        );
        for changes in [
            json!({"evaluations": []}),
            json!({"evaluations": [
                {"answer_id": 1, "rating": "Gap", "note": "Duplicate"},
                {"answer_id": 1, "rating": "Gap", "note": "Duplicate"}
            ]}),
            json!({"evaluations": [
                {"answer_id": 1, "rating": "Gap", "note": "Known"},
                {"answer_id": 99, "rating": "Solid", "note": "Invented answer"}
            ]}),
            json!({"calibrated_level": 3}),
            json!({"mode": "unknown"}),
        ] {
            assert_eq!(
                eval_sandboxed(&form, &scientific_env(&env, changes)).expect("bad batch refused"),
                false
            );
        }
        assert_eq!(
            eval_sandboxed(
                &form,
                &scientific_env(
                    &env,
                    json!({
                        "mode": "calibrate", "calibrated_level": 3
                    })
                )
            )
            .expect("actual calibration grades"),
            true
        );
    }

    #[test]
    fn grill_question_and_assessment_handoffs_use_records_not_summaries() {
        let state = eval_sandboxed(
            &skill_candidate("grill-me", "define gm-state"),
            &grill_state_env(),
        )
        .expect("state for downstream handoff");
        let question_form = skill_candidate("grill-me", "define gm-question-plan");
        let env = json!({
            "question_bank": grill_state_env()["question_bank"],
            "retired_ids": [11], "level": 2, "round": 2,
            "explanations": [{"question_id": 11, "explanation": "Two plus three is five."}],
            "drafts": [
                {"retry_id": null, "question": "Explain addition", "area": "arithmetic"},
                {"retry_id": null, "question": "Explain evidence", "area": "evidence"}
            ]
        });
        let questions = eval_sandboxed(&question_form, &env).expect("new questions");
        assert_eq!(questions[0]["id"], 21);
        assert_eq!(questions[1]["id"], 22);
        assert_eq!(questions[0]["level"], 2);
        for explanations in [
            json!([]),
            json!([{"question_id": 99, "explanation": "Unknown question"}]),
            json!([{"question_id": 11, "explanation": ""}]),
            json!([
                {"question_id": 11, "explanation": "Duplicate one"},
                {"question_id": 11, "explanation": "Duplicate two"}
            ]),
        ] {
            assert_eq!(
                eval_sandboxed(
                    &question_form,
                    &scientific_env(
                        &env,
                        json!({
                            "explanations": explanations
                        })
                    )
                )
                .expect("bad explanation handoff refused"),
                "invalid_questions"
            );
        }
        for (retry_id, expected) in [(11, false), (12, true), (99, false)] {
            let mut fixture = env.clone();
            fixture["drafts"][0] = json!({"retry_id": retry_id, "question": null, "area": null});
            let actual = eval_sandboxed(&question_form, &fixture).expect("retry validation");
            assert_eq!(actual.is_array(), expected);
            if expected {
                assert_eq!(actual[0], fixture["question_bank"][1]);
            } else {
                assert_eq!(actual, "invalid_questions");
            }
        }
        assert_eq!(
            eval_sandboxed(&question_form, &scientific_env(&env, json!({"drafts": []})))
                .expect("empty question plan refused"),
            "invalid_questions"
        );

        let assessment_form = skill_candidate("grill-me", "define gm-assessment");
        let summary = json!([
            {"area": "arithmetic", "rating": "Gap", "answer_ids": [1], "notes": "Incorrect answer"},
            {"area": "evidence", "rating": "Solid", "answer_ids": [2], "notes": "Answered"}
        ]);
        let assessment_env = json!({
            "question_bank": grill_state_env()["question_bank"],
            "answer_records": state["answer_records"], "summary": summary
        });
        assert_eq!(
            eval_sandboxed(&assessment_form, &assessment_env).expect("evidence-linked summary"),
            true
        );
        let mut invented = assessment_env.clone();
        invented["summary"][0]["answer_ids"] = json!([99]);
        assert_eq!(
            eval_sandboxed(&assessment_form, &invented).expect("invented evidence refused"),
            false
        );
        let mut empty = assessment_env;
        empty["answer_records"] = json!([]);
        for row in empty["summary"].as_array_mut().expect("summary fixture") {
            row["rating"] = json!("Unassessed");
            row["answer_ids"] = json!([]);
        }
        assert_eq!(
            eval_sandboxed(&assessment_form, &empty).expect("honest unanswered areas"),
            true
        );
        empty["summary"][0]["rating"] = json!("Gap");
        assert_eq!(
            eval_sandboxed(&assessment_form, &empty).expect("unanswered cannot be graded"),
            false
        );
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
            "critique_runs": [{"ratings": []}],
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
            "critique_runs": [{"ratings": []}],
            "pdca_corrections": 0,
            "verdict": "pass",
            "findings": [{"area":"Mechanism","gap":"g2","severity":"Guardrail"}]
        });
        assert_eq!(
            eval_sandboxed(&prompt_enhance, &clean_pass).expect("PE-2 clean case"),
            json!([0, true])
        );
        let no_findings = json!({
            "critique_runs": [{"ratings": []}],
            "pdca_corrections": 0,
            "verdict": "pass",
            "findings": []
        });
        assert_eq!(
            eval_sandboxed(&prompt_enhance, &no_findings).expect("PE-2 empty findings"),
            json!([0, true])
        );
        let non_pass_with_defect = json!({
            "critique_runs": [{"ratings": []}, {"ratings": []}, {"ratings": []}],
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
        // receipt (base 0.7, domain anchor + spec-checked -> 0.8) is pinned
        // here so a modifier edit cannot silently diverge from the documented value.
        let pragmatic_semantics = skill_candidate("pragmatic-semantics", "domain_anchored");
        let domain_spec_checked = json!({
            "base_confidence": 0.7,
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

    fn scientific_admission_env() -> serde_json::Value {
        json!({
            "ethics_safety": true, "feasibility": true, "rivals_differ": true,
            "equivalence_prespecified": false, "identification": true,
            "measurement_validity": true, "information_adequacy": true,
            "bias_control": true, "preregistration": true, "reproducibility": true
        })
    }

    fn scientific_selection_env() -> serde_json::Value {
        json!({
            "candidate_drafts": [{"candidate_id": "a"}, {"candidate_id": "b"}],
            "gate_results": [
                {"candidate_id": "a", "result": "execute"},
                {"candidate_id": "b", "result": "design_void: not_identified"}
            ],
            "selected_design": {"candidate_id": "a"},
            "strongest_rejected_alternative": {"candidate_id": "b"}
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
    fn scientific_gate_ed1_keeps_validity_noncompensatory() {
        let form = skill_candidate("experimental-design", "blocked: ethics_safety");
        let base = scientific_admission_env();
        assert_scientific_case(&form, "admitted", &base, json!("execute"));
        for (field, expected) in [
            ("ethics_safety", "blocked: ethics_safety"),
            ("feasibility", "blocked: infeasible"),
            ("rivals_differ", "design_void: no_discrimination"),
            ("identification", "design_void: not_identified"),
            (
                "measurement_validity",
                "measurement_void: measurement_invalid",
            ),
            ("information_adequacy", "no-headroom: no_information"),
            ("bias_control", "design_void: bias_uncontrolled"),
            ("preregistration", "design_void: not_preregistered"),
            ("reproducibility", "design_void: not_reproducible"),
        ] {
            let mut env = base.clone();
            env[field] = json!(false);
            assert_scientific_case(&form, field, &env, json!(expected));
        }
        assert_scientific_case(
            &form,
            "prespecified equivalence discriminates",
            &scientific_env(
                &base,
                json!({"rivals_differ": false, "equivalence_prespecified": true}),
            ),
            json!("execute"),
        );
        assert_scientific_case(
            &form,
            "ethics cannot be compensated",
            &scientific_env(
                &base,
                json!({"ethics_safety": false, "measurement_validity": false}),
            ),
            json!("blocked: ethics_safety"),
        );
    }

    #[test]
    fn scientific_gate_ed1s_selects_only_gated_survivors() {
        let form = skill_candidate("experimental-design", "define unique-result-ids");
        let base = scientific_selection_env();
        assert_scientific_case(&form, "eligible selection", &base, json!(true));
        for (case, changes) in [
            (
                "ineligible selection",
                json!({"selected_design": {"candidate_id": "b"}}),
            ),
            (
                "unknown selection",
                json!({"selected_design": {"candidate_id": "other"}}),
            ),
            (
                "selected equals rejected",
                json!({"strongest_rejected_alternative": {"candidate_id": "a"}}),
            ),
            (
                "unknown alternative",
                json!({"strongest_rejected_alternative": {"candidate_id": "other"}}),
            ),
            (
                "missing alternative",
                json!({"strongest_rejected_alternative": null}),
            ),
            (
                "missing receipt",
                json!({"gate_results": [{"candidate_id": "a", "result": "execute"}]}),
            ),
            (
                "duplicate receipt",
                json!({"gate_results": [
                    {"candidate_id": "a", "result": "execute"}, {"candidate_id": "a", "result": "execute"}
                ]}),
            ),
            (
                "equal counts do not prove coverage",
                json!({"gate_results": [
                    {"candidate_id": "a", "result": "execute"}, {"candidate_id": "other", "result": "execute"}
                ]}),
            ),
        ] {
            assert_scientific_case(&form, case, &scientific_env(&base, changes), json!(false));
        }
        assert_scientific_case(
            &form,
            "sole candidate needs no alternative",
            &scientific_env(
                &base,
                json!({
                    "candidate_drafts": [{"candidate_id": "a"}],
                    "gate_results": [{"candidate_id": "a", "result": "execute"}],
                    "strongest_rejected_alternative": null
                }),
            ),
            json!(true),
        );
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
    fn arithmetic_pe_thresholds_use_recorded_critic_results() {
        let form = skill_candidate("prompt-enhance", "\"rating_count\"");
        let run = |solid: usize, total: usize, effort: &str, calls: usize, next: usize| {
            let ratings = (0..total)
                .map(|index| json!({"rating": if index < solid { "Solid" } else { "Gap" }}))
                .collect::<Vec<_>>();
            let env = json!({
                "effort_tier": effort,
                "next_round": next,
                "critique_runs": vec![json!({"ratings": ratings}); calls]
            });
            eval_sandboxed(&form, &env).expect("authored PE threshold gate evaluates")
        };
        for (solid, total, effort, calls, next, escalate, reprobe) in [
            (4, 5, "high", 1, 2, true, false),
            (3, 5, "high", 1, 2, false, false),
            (2, 5, "high", 1, 2, false, false),
            (1, 5, "high", 1, 2, false, true),
            (5, 5, "medium", 1, 2, false, false),
            (5, 5, "low", 1, 2, false, false),
            (5, 5, "high", 3, 3, false, false),
            (5, 5, "high", 2, 3, true, false),
        ] {
            let result = run(solid, total, effort, calls, next);
            assert_eq!(result["input_valid"], json!(true));
            assert_eq!(result["escalate"], json!(escalate));
            assert_eq!(result["reprobe"], json!(reprobe));
            assert_eq!(result["solid_count"], json!(solid));
            assert_eq!(result["rating_count"], json!(total));
            assert_eq!(result["rounds_executed"], json!(calls));
        }
        for result in [
            run(0, 0, "high", 1, 2),
            run(0, 0, "high", 0, 2),
            run(5, 5, "high", 1, 4),
        ] {
            assert_eq!(result["input_valid"], json!(false));
            assert_eq!(result["escalate"], json!(false));
            assert_eq!(result["reprobe"], json!(false));
        }
        let invalid = eval_sandboxed(
            &form,
            &json!({
                "effort_tier": "high", "next_round": 2,
                "critique_runs": [{"ratings": [{"rating": "Unrated"}]}]
            }),
        )
        .expect("invalid ratings return a surfaced invalid result");
        assert_eq!(invalid["input_valid"], json!(false));
        assert_eq!(invalid["error"], "invalid_ratings");

        let bound = skill_candidate("prompt-enhance", "count-prohibitions");
        for (calls, expected) in [(3, true), (4, false)] {
            let result = eval_sandboxed(
                &bound,
                &json!({
                    "critique_runs": vec![json!({"ratings": []}); calls],
                    "pdca_corrections": 0, "verdict": "pass", "findings": []
                }),
            )
            .expect("PE-2 counts the same actual critic log");
            assert_eq!(result, json!([0, expected]));
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
