#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hkask_lisp::{LispValue, eval_sandboxed, parse};

/// The agent loader refuses any SKILL.md over 100 KB: `MAX_SKILL_FILE_SIZE`
/// in `crates/agent_skills/agent_skills.rs` rejects the file at index time
/// (before reading it), so the skill never enters the catalog and the
/// `skill` tool cannot resolve it. The checker cannot import the agent
/// workspace, so the bound is restated here — change the two together.
const MAX_SKILL_FILE_SIZE: usize = 100 * 1024;

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
    let Some(first) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    if first.as_os_str() == OsStr::new("--eval") {
        let Some(form_file) = args.next() else {
            usage();
            return ExitCode::from(2);
        };
        let Some(env_json) = args.next() else {
            usage();
            return ExitCode::from(2);
        };
        if args.next().is_some() {
            usage();
            return ExitCode::from(2);
        }
        return eval_main(&PathBuf::from(form_file), &env_json);
    }

    let Some(templates_dir) = args.next() else {
        usage();
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        usage();
        return ExitCode::from(2);
    }

    let skills_dir = PathBuf::from(first);
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

fn usage() {
    eprintln!("usage: check-skill-lisp-forms SKILLS_DIR TEMPLATES_DIR");
    eprintln!("       check-skill-lisp-forms --eval FORM_FILE ENV_JSON");
}

/// The `--eval FORM_FILE ENV_JSON` outcome. `EnvParse` is the malformed-env
/// class the CLI prints as `ENV_PARSE_ERROR`; `Failed` carries the engine's
/// message for form-read, parse and runtime errors.
#[derive(Debug, PartialEq)]
enum EvalOutcome {
    Ok(serde_json::Value),
    EnvParse(String),
    Failed(String),
}

fn eval_main(form_path: &Path, env_json: &OsStr) -> ExitCode {
    match eval_form_file(form_path, env_json) {
        EvalOutcome::Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        EvalOutcome::EnvParse(message) => {
            eprintln!("ENV_PARSE_ERROR: {message}");
            ExitCode::FAILURE
        }
        EvalOutcome::Failed(message) => {
            eprintln!("EVAL FAILED: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Read the form file, parse ENV_JSON as a JSON object and evaluate the form
/// through `eval_sandboxed` — the same path and budget the corpus scan's
/// gate tests evaluate authored forms with. A non-object env is rejected up
/// front: the engine would bind nothing and surface the gap as unbound
/// symbols, the way the `lisp_eval` tool refuses a non-object document
/// before evaluation.
fn eval_form_file(form_path: &Path, env_json: &OsStr) -> EvalOutcome {
    let form = match fs::read_to_string(form_path) {
        Ok(form) => form,
        Err(error) => {
            return EvalOutcome::Failed(format!(
                "cannot read form file {}: {error}",
                display_path(form_path)
            ));
        }
    };
    let Some(env_json) = env_json.to_str() else {
        return EvalOutcome::EnvParse("ENV_JSON is not valid UTF-8".into());
    };
    let env: serde_json::Value = match serde_json::from_str(env_json) {
        Ok(env) => env,
        Err(error) => return EvalOutcome::EnvParse(error.to_string()),
    };
    if !env.is_object() {
        return EvalOutcome::EnvParse(format!("ENV_JSON must be a JSON object, got: {env}"));
    }
    match eval_sandboxed(form.trim(), &env) {
        Ok(value) => EvalOutcome::Ok(value),
        Err(error) => EvalOutcome::Failed(error.to_string()),
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

        // The loader rejects an oversized SKILL.md at index time — the
        // skill silently disappears from the catalog and the `skill` tool.
        // Pin the same bound here so the composition error is a corpus
        // defect the moment it lands.
        if is_skill && content.len() > MAX_SKILL_FILE_SIZE {
            stats.findings.push(Finding {
                path: path.clone(),
                line: 1,
                message: format!(
                    "SKILL.md is {} bytes, over the {}KB loader cap (agent_skills \
                     MAX_SKILL_FILE_SIZE): the loader rejects it at index time, so the \
                     skill is absent from the catalog and the skill tool; move large \
                     verification data (a big lisp-receipts table) out of the load path",
                    content.len(),
                    MAX_SKILL_FILE_SIZE / 1024,
                ),
            });
        }

        // Receipts are a SKILL.md mechanism: every `lisp-receipts` fence is
        // evaluated against the form candidate it was authored under. The
        // fence carries its rows inline or names a sibling receipts file —
        // the large-table placement that keeps fixture data out of the
        // load-time SKILL.md. Rows from a named file report against that
        // file's path. The templates corpus carries no receipts, and a
        // green corpus reports nothing here, so the scan's summary line is
        // unchanged.
        if is_skill {
            let skill_dir = path.parent().unwrap_or_else(|| Path::new(""));
            let (receipts, malformed) = extract_receipts(&content, skill_dir);
            for (line, source, message) in malformed {
                stats.findings.push(Finding {
                    path: source.unwrap_or_else(|| path.clone()),
                    line,
                    message: format!("malformed receipt: {message}"),
                });
            }
            evaluate_receipts(&candidates, &receipts, &mut |receipt, message| {
                stats.findings.push(Finding {
                    path: receipt.source_path.clone().unwrap_or_else(|| path.clone()),
                    line: receipt.line,
                    message,
                });
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

/// One receipt from a `lisp-receipts` fence in a SKILL.md: a full env and the
/// exact expected result, authored beside the form it pins. The gate and
/// case name identify the receipt in defect messages; the fence's position
/// associates it with the nearest form candidate above the fence. Rows
/// live inline in the SKILL.md (`source_path` None, `line` numbering the
/// SKILL.md) or in a sibling receipts file the fence names (`source_path`
/// Some, `line` numbering that file) — the placement that keeps a large
/// fixture table out of the load-time file without leaving the skill
/// directory.
#[derive(Debug)]
struct Receipt {
    line: usize,
    fence_line: usize,
    source_path: Option<PathBuf>,
    gate: String,
    case: String,
    env: serde_json::Value,
    expected: serde_json::Value,
}

/// Extract every `lisp-receipts` fence from a SKILL.md body. A fence is a
/// fenced block whose info string starts with `lisp-receipts`; every
/// non-blank line inside it is one receipt, a JSON object carrying `gate`
/// (string), `case` (string), `env` (object) and `expected` (the exact
/// result). The info string may instead name a sibling receipts file
/// (` ```lisp-receipts <file> ` with an empty body): the rows then live one
/// per line in that file beside the SKILL.md — the large-table placement —
/// while the fence keeps the form association. Malformed rows are returned
/// with their line numbers and source file instead of being skipped: a
/// receipt that cannot be parsed cannot check anything.
fn extract_receipts(
    content: &str,
    skill_dir: &Path,
) -> (Vec<Receipt>, Vec<(usize, Option<PathBuf>, String)>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut receipts = Vec::new();
    let mut malformed = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        // ` ```lisp-receipts ` carries its rows inline; ` ```lisp-receipts
        // <file> ` names a sibling receipts file and owns no inline rows.
        let fence_info = lines[index].trim_start().strip_prefix("```").map(str::trim);
        let (is_receipts_fence, receipts_file) = match fence_info {
            Some(info) => match info.split_once(char::is_whitespace) {
                Some(("lisp-receipts", file)) => (true, Some(file.trim().to_string())),
                _ if info == "lisp-receipts" => (true, None),
                _ => (false, None),
            },
            None => {
                index += 1;
                continue;
            }
        };
        let fence_line = index + 1;
        index += 1;
        if !is_receipts_fence {
            continue;
        }
        match receipts_file {
            Some(file_name) => {
                let mut has_inline_rows = false;
                while index < lines.len() && !lines[index].trim_start().starts_with("```") {
                    has_inline_rows |= !lines[index].trim().is_empty();
                    index += 1;
                }
                index += 1; // the closing fence
                if has_inline_rows {
                    malformed.push((
                        fence_line,
                        None,
                        "a lisp-receipts fence that names a receipts file cannot also carry inline rows"
                            .into(),
                    ));
                    continue;
                }
                if file_name.contains(['/', '\\'])
                    || file_name.contains("..")
                    || file_name == "SKILL.md"
                {
                    malformed.push((
                        fence_line,
                        None,
                        format!(
                            "lisp-receipts file name must be a plain sibling file name, got '{file_name}'"
                        ),
                    ));
                    continue;
                }
                let receipts_path = skill_dir.join(&file_name);
                let rows = match fs::read_to_string(&receipts_path) {
                    Ok(rows) => rows,
                    Err(error) => {
                        malformed.push((
                            fence_line,
                            None,
                            format!("lisp-receipts file '{file_name}' cannot be read: {error}"),
                        ));
                        continue;
                    }
                };
                for (offset, row) in rows.lines().enumerate() {
                    let row = row.trim();
                    if row.is_empty() {
                        continue;
                    }
                    let row_line = offset + 1;
                    match parse_receipt_line(row) {
                        Ok((gate, case, env, expected)) => receipts.push(Receipt {
                            line: row_line,
                            fence_line,
                            source_path: Some(receipts_path.clone()),
                            gate,
                            case,
                            env,
                            expected,
                        }),
                        Err(message) => {
                            malformed.push((row_line, Some(receipts_path.clone()), message))
                        }
                    }
                }
            }
            None => {
                while index < lines.len() && !lines[index].trim_start().starts_with("```") {
                    let line_number = index + 1;
                    let line = lines[index].trim();
                    if !line.is_empty() {
                        match parse_receipt_line(line) {
                            Ok((gate, case, env, expected)) => receipts.push(Receipt {
                                line: line_number,
                                fence_line,
                                source_path: None,
                                gate,
                                case,
                                env,
                                expected,
                            }),
                            Err(message) => malformed.push((line_number, None, message)),
                        }
                    }
                    index += 1;
                }
                // `index` now names the closing fence (or the end of input);
                // the outer loop's increment moves past it.
                index += 1;
            }
        }
    }
    (receipts, malformed)
}

/// Parse one receipt line. The field set is fixed and minimal on purpose:
/// `gate`, `case`, `env` and `expected` — anything a receipt needs to say,
/// it says through these four, so the format cannot grow silent optional
/// structure the checker interprets differently from a reader.
fn parse_receipt_line(
    line: &str,
) -> Result<(String, String, serde_json::Value, serde_json::Value), String> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|error| format!("not valid JSON: {error}"))?;
    let object = value.as_object().ok_or("a receipt must be a JSON object")?;
    let string_field = |key: &str| {
        object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or(format!("missing string field \"{key}\""))
    };
    let gate = string_field("gate")?;
    let case = string_field("case")?;
    let env = object.get("env").cloned().ok_or("missing field \"env\"")?;
    if !env.is_object() {
        return Err("field \"env\" must be a JSON object".into());
    }
    let expected = object
        .get("expected")
        .cloned()
        .ok_or("missing field \"expected\"")?;
    Ok((gate, case, env, expected))
}

/// Evaluate every receipt against its associated form and report each
/// failure through `report` with the receipt it failed on — the caller reads
/// the row's own line and source file from it. The associated form is the
/// form candidate that starts on the last line before the receipt's fence:
/// the receipt pins the form it was authored under, and a move of the fence
/// re-associates it visibly (a mismatch, not a silent skip). A fence that
/// names a sibling receipts file keeps that association: the fence stays
/// beside the form, the rows stay out of the load-time file.
fn evaluate_receipts(
    candidates: &[Candidate],
    receipts: &[Receipt],
    report: &mut impl FnMut(&Receipt, String),
) {
    for receipt in receipts {
        let form = candidates
            .iter()
            .filter(|candidate| candidate.line < receipt.fence_line)
            .max_by_key(|candidate| candidate.line);
        let Some(form) = form else {
            report(
                receipt,
                format!(
                    "receipt {} / {}: no form precedes its lisp-receipts block",
                    receipt.gate, receipt.case
                ),
            );
            continue;
        };
        match eval_sandboxed(&form.source, &receipt.env) {
            Err(error) => report(
                receipt,
                format!(
                    "receipt {} / {}: evaluation failed: {error}",
                    receipt.gate, receipt.case
                ),
            ),
            Ok(actual) if actual != receipt.expected => report(
                receipt,
                format!(
                    "receipt {} / {}: expected {}, got {}",
                    receipt.gate,
                    receipt.case,
                    serde_json::to_string(&receipt.expected)
                        .unwrap_or_else(|_| receipt.expected.to_string()),
                    serde_json::to_string(&actual).unwrap_or_else(|_| actual.to_string()),
                ),
            ),
            Ok(_) => {}
        }
    }
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
#[path = "check_skill_lisp_forms/tests.rs"]
mod tests;
