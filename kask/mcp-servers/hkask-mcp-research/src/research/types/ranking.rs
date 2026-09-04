//! Web-search ranking: re-ranking and domain-agnostic utilities.
//!
//! The domain-agnostic functions (`rrf_score`, `parse_age_to_days`) were
//! moved here from `hkask-memory::ranking`. They have a single consumer (this
//! crate) and had nothing to do with memory.

use chrono::Datelike;

use hkask_types::InferencePort;

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
//
// DECISION RECORD — native rerank protocol (one call, dedicated reranker).
//
// Requirement: the deep strategy's rerank stage must be a templated LLM call
// (operator directive), and its output must be trustworthy — a general model
// asked to reorder a list can commit category errors (well-formed but
// semantically wrong orderings) that no structural validation catches.
//
// Decision: ONE `InferencePort::rerank` call carrying all candidates as
// documents, routed through the zed-side IPC bridge to the provider's
// rerank endpoint (OpenRouter `/api/v1/rerank`). The default model is a
// dedicated reranker (`OpenRouter/qwen/qwen3-reranker-8b`, override via
// `HKASK_RERANK_MODEL` / the kask models settings) whose native output is a
// per-document `relevance_score` — the model's own relevance judgment, not
// a parsed LLM generation.
//
// Why the native protocol beats both alternatives:
// 1. No category-error surface — the model cannot emit prose, hallucinate a
//    format, or misorder a list it must track; its output space is a score
//    per document. The trust problem that motivated this design is answered
//    by the protocol, not by validation layered over generation.
// 2. Consistency by construction — every candidate is judged by the same
//    model with the same internal rubric in the same request (the operator's
//    consistency requirement).
// 3. One call replaces N — the earlier per-candidate chat-completions fanout
//    (and its concurrency cap) is obsolete; cost and latency scale with one
//    request, not with `num_results`.
// 4. Dedicated rerankers are trained for exactly this shape — query–document
//    relevance judgment (Qwen3-Reranker series: Zhang et al., "Qwen3
//    Embedding: Advancing Text Embedding and Reranking Through Foundation
//    Models", arXiv:2506.05176). LLM reranking as a pattern is established
//    by RankGPT (Sun et al., "Is ChatGPT Good at Search?", EMNLP 2023,
//    arXiv:2304.09542), whose sliding-window workaround for list-length
//    limits is precisely what a native documents-array rerank endpoint makes
//    unnecessary. Positional degradation in long contexts (Liu et al., "Lost
//    in the Middle", TACL 2023, arXiv:2307.03172) motivated the earlier
//    per-candidate design; the native endpoint inherits that robustness
//    while restoring single-request economics.
//
// Canonical-pattern interactions:
// - RRF fusion (research/providers.rs): heuristic signals remain the base scoring;
//   this stage reorders on top. On total failure the RRF order is kept.
// - Inference IPC bridge: the rerank call routes through
//   `InferenceMethod::Rerank` to the zed side, which holds the OpenRouter key
//   (keychain slot at the provider `api_url`, `https://openrouter.ai/api/v1`)
//   and calls the provider directly — the MCP server never sees the
//   credential (same pattern as `GenerateBatch`).
// - Degradation surfacing: every degraded outcome (call failed, documents
//   missing from the response) is named in the tool output's `rerank` field
//   — never silent.
// - Model constants (hkask-inference/model_constants.rs):
//   the visible settings chain is the single source of truth
//   (settings_content → KaskModelsSettings → emit_models_env → this env var)
//   overrides it, and the research server's config_env allowlist passes it
//   through under governed launch.

/// Cap on title/description field lengths fed to the rerank document text,
/// so a deep search over 50 results with full extracted content cannot blow
/// the reranker's context window (Qwen3-Reranker-8B: 41K tokens per query
/// and document).
const RERANK_FIELD_MAX_CHARS: usize = 400;
const RERANK_CONTENT_MAX_CHARS: usize = 1200;

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

/// Build the document text for one candidate — the fields a relevance
/// judgment needs (title, URL, description, date, content), truncated per
/// field. Every candidate gets the same shape, so the reranker's judgments
/// are comparable by construction.
fn build_rerank_document(result: &RankedResult) -> String {
    let mut document = String::new();
    document.push_str(&format!(
        "Title: {}\n",
        truncate_field(&result.title, RERANK_FIELD_MAX_CHARS)
    ));
    document.push_str(&format!("URL: {}\n", result.url));
    if let Some(ref description) = result.description {
        document.push_str(&format!(
            "Description: {}\n",
            truncate_field(description, RERANK_FIELD_MAX_CHARS)
        ));
    }
    if let Some(ref published) = result.published {
        document.push_str(&format!("Published: {published}\n"));
    }
    if let Some(ref content) = result.extracted_content {
        document.push_str(&format!(
            "Content: {}",
            truncate_field(content, RERANK_CONTENT_MAX_CHARS)
        ));
    } else if let Some(ref preview) = result.content_preview {
        document.push_str(&format!(
            "Preview: {}",
            truncate_field(preview, RERANK_FIELD_MAX_CHARS)
        ));
    }
    document
}

/// Outcome of a rerank stage, surfaced by the caller in the tool output.
#[derive(Debug)]
pub(crate) struct RerankOutcome {
    /// Candidates that received a valid relevance score.
    pub scored: usize,
    /// Candidates without a valid score (the call failed, or their index was
    /// missing from the response) — they keep their heuristic order at the
    /// end of the results.
    pub failed: usize,
    /// First failure's reason — present when `failed > 0`.
    pub first_error: Option<String>,
}

/// Reorder `results` by the reranker's native relevance scores — ONE call
/// to `InferencePort::rerank` carrying all candidates as documents.
///
/// Scored candidates sort by descending score (ties keep heuristic order);
/// candidates without a valid score keep their heuristic relative order
/// after the scored ones. When the call fails entirely the heuristic order is
/// kept untouched. The outcome counts are returned for the caller to
/// surface — never a silent fallback.
pub(crate) async fn llm_rerank(
    inference_port: &dyn InferencePort,
    query: &str,
    results: &mut Vec<RankedResult>,
    rerank_model: &str,
) -> RerankOutcome {
    let total = results.len();
    let documents: Vec<String> = results.iter().map(build_rerank_document).collect();

    let scores = match inference_port
        .rerank(&rerank_model, query, &documents)
        .await
    {
        Ok(scores) => scores,
        Err(error) => {
            return RerankOutcome {
                scored: 0,
                failed: total,
                first_error: Some(format!("inference: {error}")),
            };
        }
    };

    let mut score_map: Vec<Option<f64>> = vec![None; total];
    for entry in scores {
        if entry.index < total {
            score_map[entry.index] = Some(entry.relevance_score);
        } else {
            tracing::warn!(
                target: "hkask.web",
                index = entry.index,
                total,
                "rerank response carried an out-of-range document index — ignoring it"
            );
        }
    }

    let scored = score_map.iter().filter(|score| score.is_some()).count();
    if scored == 0 {
        return RerankOutcome {
            scored: 0,
            failed: total,
            first_error: Some("rerank returned no valid document scores".to_string()),
        };
    }

    let failed = total - scored;
    // Stable sort: scored before unscored, descending score, ties and
    // unscored candidates keep their heuristic relative order.
    let mut order: Vec<usize> = (0..total).collect();
    order.sort_by(|&a, &b| {
        score_map[a]
            .is_none()
            .cmp(&score_map[b].is_none())
            .then_with(|| {
                score_map[b]
                    .unwrap_or(0.0)
                    .total_cmp(&score_map[a].unwrap_or(0.0))
            })
    });
    let original = std::mem::take(results);
    *results = order
        .into_iter()
        .map(|index| original[index].clone())
        .collect();

    RerankOutcome {
        scored,
        failed,
        first_error: if failed > 0 {
            Some(format!(
                "{failed} of {total} documents missing from the rerank response"
            ))
        } else {
            None
        },
    }
}

// ── Tests: native rerank protocol for the deep-strategy rerank ─────────────

#[cfg(test)]
mod rerank_tests {
    use super::*;
    use hkask_types::RerankFuture;
    use hkask_types::inference_ipc::RerankScoreEntry;
    use hkask_types::{InferenceError, InferencePort, InferenceResult};
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

    fn score_entry(index: usize, relevance_score: f64) -> RerankScoreEntry {
        RerankScoreEntry {
            index,
            relevance_score,
        }
    }

    /// Native-protocol stub: returns fixed scores and counts calls so tests
    /// can pin the single-call contract.
    struct FixedRerankPort {
        scores: Vec<RerankScoreEntry>,
        /// Error message — when set, `rerank` fails with it.
        error: Option<String>,
        call_count: AtomicUsize,
    }

    impl FixedRerankPort {
        fn with_scores(scores: Vec<RerankScoreEntry>) -> Self {
            Self {
                scores,
                error: None,
                call_count: AtomicUsize::new(0),
            }
        }

        fn with_error(error: &str) -> Self {
            Self {
                scores: Vec::new(),
                error: Some(error.to_string()),
                call_count: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl InferencePort for FixedRerankPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &hkask_types::template::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<InferenceResult, InferenceError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::Connection(
                    "stub: generate unused in rerank tests".to_string(),
                ))
            })
        }

        fn rerank<'a>(
            &'a self,
            _model: &str,
            _query: &str,
            documents: &[String],
        ) -> RerankFuture<'a> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let scores = self.scores.clone();
            let error = self.error.clone();
            let document_count = documents.len();
            Box::pin(async move {
                if let Some(error) = error {
                    return Err(InferenceError::Connection(error));
                }
                // Guard: the stub's scores must reference real documents.
                assert!(
                    scores.iter().all(|entry| entry.index < document_count),
                    "stub scores must be in range of the document list"
                );
                Ok(scores)
            })
        }
    }

    /// Success contract: the provider's relevance ordering reaches the caller
    /// via exactly ONE rerank call carrying all candidates.
    #[tokio::test]
    async fn rerank_single_call_orders_by_score() {
        let port = FixedRerankPort::with_scores(vec![
            score_entry(0, 0.50),
            score_entry(1, 0.10),
            score_entry(2, 0.90),
        ]);
        let mut results = three_candidates();

        let outcome = llm_rerank(&port, "test", &mut results, "test-rerank-model").await;

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
            "descending relevance order must reach the caller"
        );
        assert_eq!(
            port.call_count(),
            1,
            "the native protocol is ONE call carrying all documents"
        );
    }

    /// Partial-response contract: a document missing from the response keeps
    /// its heuristic order at the end, and the gap is counted and named.
    #[tokio::test]
    async fn rerank_partial_response_keeps_unscored_at_end() {
        // Index 1 (Beta) missing from the response.
        let port = FixedRerankPort::with_scores(vec![score_entry(0, 0.50), score_entry(2, 0.90)]);
        let mut results = three_candidates();

        let outcome = llm_rerank(&port, "test", &mut results, "test-rerank-model").await;

        assert_eq!(outcome.scored, 2);
        assert_eq!(outcome.failed, 1);
        assert!(
            outcome
                .first_error
                .as_deref()
                .is_some_and(|error| error.contains("missing from the rerank response")),
            "partial response must name its cause; got {:?}",
            outcome.first_error
        );
        assert_eq!(
            results.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec![
                "https://example.com/gamma",
                "https://example.com/alpha",
                "https://example.com/beta",
            ],
            "scored candidates sort by score; the unscored one keeps heuristic order at the end"
        );
    }

    /// Total-failure contract: the heuristic order is kept untouched and the
    /// inference error is surfaced.
    #[tokio::test]
    async fn rerank_total_failure_keeps_heuristic_order() {
        let port = FixedRerankPort::with_error("stub: rerank endpoint down");
        let mut results = three_candidates();

        let outcome = llm_rerank(&port, "test", &mut results, "test-rerank-model").await;

        assert_eq!(outcome.scored, 0);
        assert_eq!(outcome.failed, 3);
        assert!(
            outcome
                .first_error
                .as_deref()
                .is_some_and(|error| error.contains("stub: rerank endpoint down")),
            "total failure must name the inference error; got {:?}",
            outcome.first_error
        );
        assert_eq!(
            results.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            vec![
                "https://example.com/alpha",
                "https://example.com/beta",
                "https://example.com/gamma",
            ],
            "heuristic order must be kept untouched on total failure"
        );
    }
}
