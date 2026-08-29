//! Web-search ranking: re-ranking and domain-agnostic utilities.
//!
//! The domain-agnostic functions (`rrf_score`, `parse_age_to_days`) were
//! moved here from `hkask-memory::ranking`. They have a single consumer (this
//! crate) and had nothing to do with memory.

use chrono::Datelike;
use futures_util::StreamExt;

use hkask_types::InferencePort;
use hkask_types::json_extract::extract_json_from_response;
use hkask_types::template::LLMParameters;

use crate::research::types::{RankedResult, RerankSignal};

// ── Domain-agnostic ranking utilities ──────────────────────────────────────

/// Reciprocal Rank Fusion score for a set of rank positions.
///
/// `k` is the smoothing constant (commonly 60). Each rank position is
/// 0-based (rank 0 = first result).
///
/// pre:  k > 0, ranks contains valid 0-based positions
/// post: returns sum of 1/(k + rank + 1) for each rank
/// post: result is always ≥ 0.0
pub(crate) fn rrf_score(k: u64, ranks: &[usize]) -> f64 {
    ranks
        .iter()
        .map(|&r| 1.0 / (k as f64 + r as f64 + 1.0))
        .sum()
}

/// Parse a human-readable age string into days.
///
/// Supports: "3 days ago", "2 weeks ago", ISO dates like "2024-01-15",
/// fuzzy dates like "Jan 15, 2024", and "published ..." prefixes.
/// Returns -1.0 for unparsable input.
///
/// pre:  age is a valid &str
/// post: returns days as f64 (≥ 0.0 for valid dates)
/// post: returns -1.0 for unparsable or empty input
pub(crate) fn parse_age_to_days(age: &str) -> f64 {
    let lower = age.to_lowercase();
    let lower = lower.trim();

    if lower.is_empty() {
        return -1.0;
    }

    // Strip "published" prefix first so that "published 3 days ago"
    // recurses into "3 days ago" instead of hitting the " ago" suffix
    // with "published 3 days" (which fails f64 parsing).
    if let Some(rest) = lower.strip_prefix("published ") {
        return parse_age_to_days(rest);
    }

    if let Some(rest) = lower.strip_suffix(" ago") {
        let rest = rest.trim();
        return parse_relative_age(rest);
    }

    if let Ok(days) = parse_iso_date_to_days(lower) {
        return days;
    }

    parse_fuzzy_date(lower)
}

fn parse_relative_age(rest: &str) -> f64 {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 2 {
        return -1.0;
    }
    let num: f64 = match parts[0].parse() {
        Ok(n) => n,
        Err(_) => return -1.0,
    };
    match parts[1] {
        s if s.starts_with("second") => num / 86400.0,
        s if s.starts_with("minute") => num / 1440.0,
        s if s.starts_with("hour") => num / 24.0,
        s if s.starts_with("day") => num,
        s if s.starts_with("week") => num * 7.0,
        s if s.starts_with("month") => num * 30.0,
        s if s.starts_with("year") => num * 365.0,
        _ => -1.0,
    }
}

fn parse_iso_date_to_days(s: &str) -> Result<f64, ()> {
    let s = s.trim();
    if s.len() < 10 {
        return Err(());
    }
    let year: i32 = s.get(0..4).ok_or(())?.parse().map_err(|_| ())?;
    let month: i32 = s.get(5..7).ok_or(())?.parse().map_err(|_| ())?;
    let day: i32 = s.get(8..10).ok_or(())?.parse().map_err(|_| ())?;

    if !(2000..=2100).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(());
    }

    let now = chrono::Utc::now();
    let now_ordinal = now.ordinal0() as i32 + 1;
    let now_yday = now.year() * 366 + now_ordinal;

    let target_ordinal = ordinal_day(year, month, day);
    let target_yday = year * 366 + target_ordinal;

    let diff = now_yday - target_yday;
    if diff < 0 {
        return Ok(0.0);
    }
    Ok(diff as f64)
}

fn ordinal_day(year: i32, month: i32, day: i32) -> i32 {
    let days_in_months = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let mut ordinal = day;
    for m in 1..month {
        ordinal += days_in_months[m as usize];
        if m == 2 && leap {
            ordinal += 1;
        }
    }
    ordinal
}

fn parse_fuzzy_date(s: &str) -> f64 {
    let parts: Vec<&str> = s.split(|c: char| !c.is_alphanumeric()).collect();
    let mut year: Option<i32> = None;
    let mut month: Option<i32> = None;
    let mut day: Option<i32> = None;
    let month_names = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];

    for part in parts {
        if part.is_empty() {
            continue;
        }
        if let Ok(n) = part.parse::<i32>() {
            if (2000..=2100).contains(&n) && year.is_none() {
                year = Some(n);
            } else if (1..=12).contains(&n) && month.is_none() {
                month = Some(n);
            } else if (1..=31).contains(&n) && day.is_none() {
                day = Some(n);
            }
        } else {
            let lower = part.to_lowercase();
            for (i, name) in month_names.iter().enumerate() {
                if lower.starts_with(name) {
                    month = Some((i + 1) as i32);
                    break;
                }
            }
        }
    }

    if let Some(y) = year {
        let m = month.unwrap_or(1);
        let d = day.unwrap_or(1);
        parse_iso_date_to_days(&format!("{y:04}-{m:02}-{d:02}")).unwrap_or(-1.0)
    } else {
        -1.0
    }
}

// ── Web-search-specific ranking ────────────────────────────────────────────

pub(crate) fn apply_rerank(results: &mut [RankedResult], signal: RerankSignal) {
    match signal {
        RerankSignal::Recency => {
            for r in results.iter_mut() {
                if let Some(ref published) = r.published {
                    let days = parse_age_to_days(published);
                    if days >= 0.0 {
                        let boost = 1.0 / (1.0 + days / 7.0);
                        r.rrf_score += boost * 0.1;
                    }
                }
            }
        }
        RerankSignal::Semantic => {
            for r in results.iter_mut() {
                if let Some(score) = r.semantic_score {
                    r.rrf_score += score * 0.05;
                }
            }
        }
        RerankSignal::ContentQuality => {
            for r in results.iter_mut() {
                if r.content_preview.is_some() || r.extracted_content.is_some() {
                    r.rrf_score += 0.05;
                }
            }
        }
    }
    results.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

// ── LLM rerank (deep strategy) ─────────────────────────────────────────────

/// Cap on title/description field lengths fed to the rerank prompt, so a
/// deep search over 50 results with full extracted content cannot blow the
/// model's context window.
const RERANK_FIELD_MAX_CHARS: usize = 400;
const RERANK_CONTENT_MAX_CHARS: usize = 1200;

/// Default cap on concurrent rerank scoring calls. The functional driver is
/// responsiveness: a deep search must not serialize its scoring calls (the
/// user feels the program is stuck) nor fire them unbounded (the provider
/// rate-limits and every call fails). Fanout ramps up to this cap.
const DEFAULT_RERANK_MAX_CONCURRENCY: usize = 8;

fn truncate_field(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        text.to_string()
    } else {
        // Cut at a char boundary, not mid-codepoint.
        let mut end = max_chars;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    }
}

/// Resolve the rerank fanout cap: `HKASK_RERANK_MAX_CONCURRENCY` → default.
/// A malformed value warns naming the value (never a silent fallback) and
/// uses the default; a zero-or-negative parse is malformed, not a cap of 0.
pub(crate) fn rerank_max_concurrency() -> usize {
    match std::env::var("HKASK_RERANK_MAX_CONCURRENCY") {
        Ok(value) => match value.parse::<usize>() {
            Ok(parsed) if parsed >= 1 => parsed,
            _ => {
                tracing::warn!(
                    target: "hkask.web",
                    value = %value,
                    "HKASK_RERANK_MAX_CONCURRENCY malformed — using default \
                     {DEFAULT_RERANK_MAX_CONCURRENCY}"
                );
                DEFAULT_RERANK_MAX_CONCURRENCY
            }
        },
        Err(_) => DEFAULT_RERANK_MAX_CONCURRENCY,
    }
}

/// Build one pairwise scoring prompt: the query plus a single candidate.
///
/// The rerank model (default `DeepInfra/Qwen/Qwen3-Reranker-8B`) is a
/// dedicated query–document relevance scorer, so each candidate is judged
/// in its own call — one (query, candidate) pair per prompt. This is also
/// what makes fanout possible: N candidates become N independent calls
/// that run concurrently up to the cap.
pub(crate) fn build_score_prompt(query: &str, result: &RankedResult) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "You are a search relevance scorer. Rate how well the single \
         candidate result below satisfies the query's intent.\n\n",
    );
    prompt.push_str(&format!("Query: {query}\n\n"));
    prompt.push_str("Candidate:\n");
    prompt.push_str(&format!(
        "Title: {}\n",
        truncate_field(&result.title, RERANK_FIELD_MAX_CHARS)
    ));
    prompt.push_str(&format!("URL: {}\n", result.url));
    if let Some(ref description) = result.description {
        prompt.push_str(&format!(
            "Description: {}\n",
            truncate_field(description, RERANK_FIELD_MAX_CHARS)
        ));
    }
    if let Some(ref published) = result.published {
        prompt.push_str(&format!("Published: {published}\n"));
    }
    if let Some(ref content) = result.extracted_content {
        prompt.push_str(&format!(
            "Content: {}\n",
            truncate_field(content, RERANK_CONTENT_MAX_CHARS)
        ));
    } else if let Some(ref preview) = result.content_preview {
        prompt.push_str(&format!(
            "Preview: {}\n",
            truncate_field(preview, RERANK_FIELD_MAX_CHARS)
        ));
    }
    prompt.push_str("\nReturn ONLY a JSON object, no prose, no markdown fences:\n");
    prompt.push_str("{\"score\": <integer 0-100, higher = more relevant>}\n");
    prompt
}

/// Outcome of a rerank fanout, surfaced by the caller in the tool output.
#[derive(Debug)]
pub(crate) struct RerankOutcome {
    /// Candidates that received a valid relevance score.
    pub scored: usize,
    /// Candidates whose scoring call failed (inference error or unparseable
    /// output) — they keep their heuristic order at the end of the results.
    pub failed: usize,
    /// First scoring failure's reason — present when `failed > 0`.
    pub first_error: Option<String>,
}

fn rerank_parameters() -> LLMParameters {
    LLMParameters {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 40,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        min_p: 0.0,
        typical_p: 0.0,
        seed: None,
        thinking_allowed: false,
        adapter: None,
        system_prompt: Some(
            "You are a precise search relevance scorer. You respond with strict \
             JSON only — no prose, no markdown fences."
                .to_string(),
        ),
    }
}

/// Parse a scoring response: `{"score": N}` with N in 0..=100. A bare
/// integer is also accepted — some backends strip the JSON wrapper.
fn parse_score(raw: &str) -> Result<u64, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| format!("unparseable score JSON: {error}"))?;
    let score = match parsed {
        serde_json::Value::Object(map) => map.get("score").and_then(|value| value.as_u64()),
        serde_json::Value::Number(number) => number.as_u64(),
        _ => None,
    };
    score
        .filter(|score| (0..=100).contains(score))
        .ok_or_else(|| "score missing or outside 0-100".to_string())
}

async fn score_candidate(
    inference_port: &dyn InferencePort,
    prompt: &str,
    rerank_model: &str,
    parameters: &LLMParameters,
) -> Result<u64, String> {
    let result = inference_port
        .generate_with_model(prompt, parameters, Some(rerank_model), None)
        .await
        .map_err(|error| format!("inference: {error}"))?;
    parse_score(&extract_json_from_response(&result.text))
}

/// Reorder `results` by LLM relevance scores, fanning out one scoring call
/// per candidate with concurrency bounded by `max_concurrency`.
///
/// Scored candidates sort by descending score (ties keep heuristic order);
/// candidates whose call failed keep their heuristic relative order after
/// the scored ones. When every call fails the heuristic order is kept
/// untouched. The outcome counts are returned for the caller to surface —
/// never a silent fallback.
pub(crate) async fn llm_rerank_with_limit(
    inference_port: &dyn InferencePort,
    query: &str,
    results: &mut Vec<RankedResult>,
    max_concurrency: usize,
) -> RerankOutcome {
    let total = results.len();
    // The rerank model is a named constant with an env override
    // (`HKASK_RERANK_MODEL`) — resolved per call so an operator override
    // takes effect without a server restart.
    let rerank_model = hkask_inference::model_constants::rerank_model();
    let parameters = rerank_parameters();

    let prompts: Vec<(usize, String)> = results
        .iter()
        .enumerate()
        .map(|(index, result)| (index, build_score_prompt(query, result)))
        .collect();

    // Bounded fanout: up to `max_concurrency` scoring calls in flight at
    // once; the rest queue as permits free up.
    let scored: Vec<(usize, Result<u64, String>)> = futures_util::stream::iter(prompts)
        .map(|(index, prompt)| {
            let rerank_model = &rerank_model;
            let parameters = &parameters;
            async move {
                let outcome =
                    score_candidate(inference_port, &prompt, rerank_model, parameters).await;
                (index, outcome)
            }
        })
        .buffer_unordered(max_concurrency.max(1))
        .collect()
        .await;

    let mut scores: Vec<Option<u64>> = vec![None; total];
    let mut failed = 0;
    let mut first_error: Option<String> = None;
    for (index, outcome) in scored {
        match outcome {
            Ok(score) => scores[index] = Some(score),
            Err(error) => {
                tracing::warn!(
                    target: "hkask.web",
                    index,
                    error = %error,
                    "rerank scoring call failed — candidate keeps heuristic order"
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
                failed += 1;
            }
        }
    }

    let scored_count = total - failed;
    if scored_count > 0 {
        // Stable sort: scored before unscored, descending score, ties and
        // unscored candidates keep their heuristic relative order.
        let mut order: Vec<usize> = (0..total).collect();
        order.sort_by_key(|&index| {
            (
                scores[index].is_none(),
                std::cmp::Reverse(scores[index].unwrap_or(0)),
            )
        });
        let original = std::mem::take(results);
        *results = order
            .into_iter()
            .map(|index| original[index].clone())
            .collect();
    }

    RerankOutcome {
        scored: scored_count,
        failed,
        first_error,
    }
}

/// `llm_rerank_with_limit` with the fanout cap resolved from
/// `HKASK_RERANK_MAX_CONCURRENCY` (per call, so an operator override takes
/// effect without a server restart).
pub(crate) async fn llm_rerank(
    inference_port: &dyn InferencePort,
    query: &str,
    results: &mut Vec<RankedResult>,
) -> RerankOutcome {
    let limit = rerank_max_concurrency();
    llm_rerank_with_limit(inference_port, query, results, limit).await
}

// ── Tests: bounded fanout for the deep-strategy rerank ─────────────────────

#[cfg(test)]
mod rerank_tests {
    use super::*;
    use hkask_types::{InferenceError, InferencePort, InferenceResult, InferenceUsage};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn candidate(title: &str, url: &str) -> RankedResult {
        RankedResult {
            title: title.to_string(),
            url: url.to_string(),
            description: None,
            source: None,
            published: None,
            rrf_score: 1.0,
            provider_count: 1,
            providers: vec!["stub".to_string()],
            best_rank: None,
            content_preview: None,
            semantic_score: None,
            extracted_content: None,
        }
    }

    fn three_candidates() -> Vec<RankedResult> {
        vec![
            candidate("Alpha", "https://example.com/alpha"),
            candidate("Beta", "https://example.com/beta"),
            candidate("Gamma", "https://example.com/gamma"),
        ]
    }

    /// Scoring stub: returns a per-URL score, and counts how many calls are
    /// in flight at once (max tracked) so tests can pin the concurrency cap.
    struct CountingScoringPort {
        scores: HashMap<&'static str, u64>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }

    impl CountingScoringPort {
        fn new(scores: HashMap<&'static str, u64>) -> Self {
            Self {
                scores,
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
            }
        }

        fn max_in_flight(&self) -> usize {
            self.max_in_flight.load(Ordering::SeqCst)
        }
    }

    impl InferencePort for CountingScoringPort {
        fn generate(
            &self,
            prompt: &str,
            _parameters: &hkask_types::template::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                    + Send
                    + '_,
            >,
        > {
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(current, Ordering::SeqCst);
            let score = self
                .scores
                .iter()
                .find(|(url, _)| prompt.contains(*url))
                .map(|(_, score)| *score)
                .unwrap_or(50);
            let counter = &self.in_flight;
            Box::pin(async move {
                // Hold the call open briefly so sibling fanout calls overlap.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                counter.fetch_sub(1, Ordering::SeqCst);
                Ok(InferenceResult {
                    text: format!("{{\"score\": {score}}}"),
                    model: "stub".to_string(),
                    usage: InferenceUsage::default(),
                    finish_reason: "stop".to_string(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    /// Fanout contract: with a cap ≥ candidate count, all scoring calls run
    /// concurrently (max in-flight reaches the candidate count) and the
    /// score ordering reaches the caller.
    #[tokio::test]
    async fn rerank_fanout_runs_candidates_concurrently() {
        let mut scores = HashMap::new();
        scores.insert("alpha", 50u64);
        scores.insert("beta", 10u64);
        scores.insert("gamma", 90u64);
        let port = Arc::new(CountingScoringPort::new(scores));
        let mut results = three_candidates();

        let outcome = llm_rerank_with_limit(port.as_ref(), "test", &mut results, 8).await;

        assert_eq!(outcome.scored, 3);
        assert_eq!(outcome.failed, 0);
        assert!(outcome.first_error.is_none());
        assert_eq!(
            results.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec![
                "https://example.com/gamma",
                "https://example.com/alpha",
                "https://example.com/beta",
            ],
            "descending score order must reach the caller"
        );
        assert_eq!(
            port.max_in_flight(),
            3,
            "with cap 8 and 3 candidates, all 3 scoring calls must be in flight at once"
        );
    }

    /// Cap contract: with a cap of 1, calls serialize — max in-flight is
    /// pinned at exactly 1 (deterministic: the permit count enforces it).
    #[tokio::test]
    async fn rerank_cap_one_serializes_scoring_calls() {
        let mut scores = HashMap::new();
        scores.insert("alpha", 50u64);
        scores.insert("beta", 10u64);
        scores.insert("gamma", 90u64);
        let port = Arc::new(CountingScoringPort::new(scores));
        let mut results = three_candidates();

        let outcome = llm_rerank_with_limit(port.as_ref(), "test", &mut results, 1).await;

        assert_eq!(outcome.scored, 3);
        assert_eq!(outcome.failed, 0);
        assert_eq!(
            port.max_in_flight(),
            1,
            "cap 1 must serialize scoring calls — max in-flight is exactly 1"
        );
    }

    /// Partial-failure contract: a failed scoring call keeps its candidate
    /// in heuristic order at the end, and the failure is counted and named.
    #[tokio::test]
    async fn rerank_partial_failure_keeps_unscored_at_end() {
        let mut results = vec![
            candidate("Alpha", "https://example.com/alpha"),
            candidate("Beta", "https://example.com/beta"),
            candidate("Broken", "https://example.com/broken"),
        ];

        struct PartialFailPort;
        impl InferencePort for PartialFailPort {
            fn generate(
                &self,
                prompt: &str,
                _parameters: &hkask_types::template::LLMParameters,
                _tools: Option<&[hkask_types::ChatToolDefinition]>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                        + Send
                        + '_,
                >,
            > {
                let fails = prompt.contains("broken");
                let score = if prompt.contains("alpha") { 50 } else { 10 };
                Box::pin(async move {
                    if fails {
                        Err(InferenceError::Connection("stub: broken".to_string()))
                    } else {
                        Ok(InferenceResult {
                            text: format!("{{\"score\": {score}}}"),
                            model: "stub".to_string(),
                            usage: InferenceUsage::default(),
                            finish_reason: "stop".to_string(),
                            tool_calls: Vec::new(),
                            reasoning: None,
                            cost_usd: None,
                        })
                    }
                })
            }
        }

        let inference_port: &dyn InferencePort = &PartialFailPort;
        let outcome = llm_rerank_with_limit(inference_port, "test", &mut results, 8).await;

        assert_eq!(outcome.scored, 2);
        assert_eq!(outcome.failed, 1);
        assert!(
            outcome
                .first_error
                .as_deref()
                .is_some_and(|error| error.contains("stub: broken")),
            "partial failure must name its cause; got {:?}",
            outcome.first_error
        );
        assert_eq!(
            results.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec![
                "https://example.com/alpha",
                "https://example.com/beta",
                "https://example.com/broken",
            ],
            "scored candidates sort by score; the unscored one keeps heuristic order at the end"
        );
    }
}
