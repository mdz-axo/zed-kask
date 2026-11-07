//! JSONL run receipts — the archived row schema is the contract
//! (direct-model-edit-archive/logs/probe-*.jsonl), so legs stay comparable
//! by key. The Rust instrument adds a small additive block (harness,
//! backend, gguf_sha256, penalty_last_n, n_ctx) under new keys; it never
//! renames or drops archived keys.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RunRow {
    pub ts: f64,
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
    pub dropped_gen_kwargs: Vec<String>,
    pub presence_penalty_applied: f64,
    pub model: String,
    pub dtype: String,
    pub threads: u32,
    pub param_count: u64,
    pub config_file: serde_json::Value,
    // --- Rust-instrument additions (additive keys; archived schema above) ---
    pub harness: String,
    pub backend: String,
    pub gguf_sha256: String,
    pub penalty_last_n: i32,
    pub n_ctx: u32,
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
    pub order: String,
    pub harness: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub correct_complete: u32,
    pub wrong_complete: u32,
    pub hit_limit: u32,
}

/// Extract the final answer: last "\boxed{...}" with brace nesting.
pub fn extract_boxed(text: &str) -> Option<String> {
    let mut open: Option<usize> = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut last_start = None;
    while i + 7 <= bytes.len() {
        if &text[i..i + 7] == "\\boxed{" {
            last_start = Some(i + 7);
        }
        i += 1;
    }
    open = last_start;
    let start = open?;
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

/// Numeric normalization for gold/answer comparison: strip commas,
/// whitespace, and trailing periods.
pub fn normalize_numeric(s: &str) -> String {
    s.trim()
        .trim_matches('.')
        .replace([',', '$', ' '], "")
        .replace("\\!", "")
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
    fn normalization() {
        assert_eq!(normalize_numeric("1,000"), "1000");
        assert_eq!(normalize_numeric(" 72."), "72");
        assert_eq!(normalize_numeric("$5"), "5");
    }
}
