//! JSONL run receipts — the archived row schema is the contract
//! (direct-model-edit-archive/logs/probe-*.jsonl), so legs stay
//! comparable by key. Fields below are ordered as the archived
//! instrument emitted them, followed by the additive instrument block:
//! the Rust instrument adds keys under new names; it never renames or
//! drops an archived key.
//!
//! The archived grading logic (ported from the retired grade.rs, P4):
//! boxed extraction with the GSM8K `####` fallback, numeric comparison
//! at 1e-6 tolerance, and the text-token tail-distinct ratio. The
//! retired instrument's string-normalization equality is NOT the
//! grader — `2.0` vs gold `2` grades CORRECT numerically.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RunRow {
    // --- the archived schema (the emission order) ---
    pub ts: f64,
    pub dropped_gen_kwargs: Vec<String>,
    pub config: String,
    pub problem_id: String,
    pub source: String,
    pub gold: String,
    pub kind: String,
    pub prompt_tokens: u32,
    pub new_tokens: u32,
    pub wall_secs: f64,
    pub tok_per_sec: f64,
    pub hit_limit: bool,
    pub extracted: Option<String>,
    pub extraction_via: String,
    pub correct: bool,
    pub label: String,
    pub tail_distinct_ratio: f64,
    pub cap: u32,
    pub seed: u64,
    pub enable_thinking_applied: bool,
    pub presence_penalty_applied: f64,
    pub model: String,
    pub dtype: String,
    pub device: String,
    pub threads: u32,
    /// The retired Python instrument's kernelization knob, carried as a
    /// verbatim archived key; the Rust instrument states its own truth:
    /// it does not kernelize.
    pub kernelize: bool,
    pub param_count: u64,
    pub config_file: serde_json::Value,
    // --- the additive instrument block (new keys only) ---
    pub harness: String,
    pub backend: String,
    pub gguf_sha256: String,
    /// The presence-penalty window the route pins (`repeat_last_n` on
    /// the request, echoed by the server) — n_ctx, the whole window
    /// (the card semantics; the recorded deviation from the retired
    /// transformers-side processor).
    pub penalty_last_n: i32,
    pub n_ctx: u32,
    /// The serving engine's `build_info` (from the server's `/props`) —
    /// the engine receipt on every row.
    pub engine_version: String,
    /// The server's `generation_settings` echo for this row — the
    /// sampling-parity receipt: every knob was verified against it
    /// (a mismatch is a typed leg failure, never an approximation).
    pub server_generation_settings: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct StageSummary {
    pub ts: f64,
    pub config: String,
    pub n: u32,
    pub counts: Counts,
    pub cap: u32,
    pub mean_wall_secs: f64,
    pub mean_tok_per_sec: f64,
    pub gguf: String,
    pub gguf_sha256: String,
    pub dtype: String,
    pub elapsed_secs: f64,
    pub harness: String,
    // --- the additive route receipts ---
    pub model_id: String,
    pub engine_version: String,
    pub endpoint: String,
    pub budget_secs: u64,
    pub first_problem_ids: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub correct_complete: u32,
    pub wrong_complete: u32,
    pub hit_limit: u32,
}

/// Extract the final answer: the last "\boxed{...}" (brace nesting
/// aware), falling back to the GSM8K "#### N" marker — the archived
/// instrument's extraction order. Returns the value and the `via` tag
/// for the receipt (`boxed` | `hash` | `none`).
pub fn extract_answer(text: &str) -> (Option<String>, String) {
    if let Some(v) = extract_boxed(text) {
        return (Some(v), "boxed".into());
    }
    if let Some(pos) = text.rfind("####") {
        let rest = &text[pos + 4..];
        let line = rest.lines().next().unwrap_or("").trim();
        let cleaned: String = line.chars().filter(|c| !matches!(c, '$' | ',')).collect();
        let cleaned = cleaned.trim().trim_end_matches('.').to_string();
        if !cleaned.is_empty() {
            return (Some(cleaned), "hash".into());
        }
    }
    (None, "none".into())
}

/// The archived grader: numeric comparison at 1e-6 tolerance after
/// stripping `,` `$` and trailing `.`; exact trimmed string equality
/// otherwise.
pub fn numeric_grade(extracted: &str, gold: &str) -> bool {
    let norm = |s: &str| -> Option<f64> {
        let s = s.trim().replace([',', '$'], "");
        let s = s.trim_end_matches('.');
        s.parse::<f64>().ok()
    };
    if let (Some(a), Some(b)) = (norm(extracted), norm(gold)) {
        return (a - b).abs() < 1e-6;
    }
    extracted.trim() == gold.trim()
}

/// Distinct-token ratio over the LAST `tail` whitespace tokens of the
/// generated text — the archived instrument's text-token semantics,
/// rounded to 4 decimals.
pub fn tail_distinct_ratio(text: &str, tail: usize) -> f64 {
    let toks: Vec<&str> = text.split_whitespace().collect();
    let seg: Vec<&str> = if toks.len() > tail {
        toks[toks.len() - tail..].to_vec()
    } else {
        toks
    };
    if seg.is_empty() {
        return 0.0;
    }
    let d: std::collections::HashSet<&&str> = seg.iter().collect();
    let r = d.len() as f64 / seg.len() as f64;
    (r * 10000.0).round() / 10000.0
}

/// Extract the last "\boxed{...}" with brace nesting (the boxed arm of
/// [`extract_answer`]; capture uses this directly).
pub fn extract_boxed(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut last_start = None;
    let mut i = 0;
    while i + 7 <= bytes.len() {
        // byte comparison, never a str slice: an arbitrary byte offset can
        // sit inside a multi-byte UTF-8 char (an edited model generated
        // 其 as its first token and panicked the str-slice form)
        if &bytes[i..i + 7] == b"\\boxed{" {
            last_start = Some(i + 7);
        }
        i += 1;
    }
    let start = last_start?;
    let mut depth = 1i32;
    let mut j = start;
    let mut end = None;
    while j < text.len() {
        let c = text.as_bytes()[j];
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                end = Some(j);
                break;
            }
        }
        j += 1;
    }
    let end = end?;
    Some(text[start..end].trim().to_string())
}

/// Two-decimal rounding (the archived wall/tok receipts).
pub fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// One-decimal rounding (the archived ts/summary receipts).
pub fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_extraction() {
        let t = "so we get \\boxed{42} done";
        assert_eq!(extract_boxed(t).as_deref(), Some("42"));
        let t2 = "nested \\boxed{\\frac{1}{2}} here";
        assert_eq!(extract_boxed(t2).as_deref(), Some("\\frac{1}{2}"));
        let t3 = "first \\boxed{1} then \\boxed{7}";
        assert_eq!(extract_boxed(t3).as_deref(), Some("7"));
        assert_eq!(extract_boxed("no answer"), None);
    }

    #[test]
    fn answer_extraction_order_is_boxed_then_hash() {
        let both = "work \\boxed{9} then #### 12";
        let (v, via) = extract_answer(both);
        assert_eq!((v.as_deref(), via.as_str()), (Some("9"), "boxed"));
        let hash_only = "so the answer is\n#### 1,200.\nthanks";
        let (v, via) = extract_answer(hash_only);
        assert_eq!((v.as_deref(), via.as_str()), (Some("1200"), "hash"));
        let (v, via) = extract_answer("no marker at all");
        assert_eq!((v, via.as_str()), (None, "none"));
    }

    #[test]
    fn numeric_grade_matches_the_archived_tolerance() {
        // The archived grader: 1e-6 numeric tolerance — `2.0` grades
        // CORRECT against gold `2` (the string grader would not).
        assert!(numeric_grade("2.0", "2"));
        assert!(numeric_grade("1,000", "1000"));
        assert!(numeric_grade("$5.", "5"));
        assert!(numeric_grade("72", "72.0"));
        assert!(!numeric_grade("71", "72"));
        assert!(numeric_grade("abc", "abc"));
    }

    #[test]
    fn tail_distinct_ratio_over_text_tokens() {
        let looping = "the the the the the the the the the the";
        assert!(tail_distinct_ratio(looping, 8) < 0.2);
        let varied = "one two three four five six seven eight";
        assert!((tail_distinct_ratio(varied, 8) - 1.0).abs() < 1e-9);
        assert_eq!(tail_distinct_ratio("", 8), 0.0);
    }
}
