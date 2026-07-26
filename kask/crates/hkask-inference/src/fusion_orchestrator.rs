//! Fusion orchestration engine — provider-agnostic multi-model deliberation modes.
//!
//! The judge is the strategy. When `fusion.judge == "algo"`, the orchestrator
//! runs the panel in parallel and merges JSON responses algorithmically — no
//! LLM call. The algo judge preserves both viewpoints (union, case-insensitive
//! dedup, diverging strings annotated `[A:... B:...]`) without a methodology
//! lens — use judge-based modes for methodology-anchored evaluation.
//!
//! Each LLM fusion mode defines how the judge interacts with the panel:
//! - BestOfN: Judge picks the single best response.
//! - Synthesis: Judge composes a unified response from all panelists.
//! - Critique: 2-round: draft → panel critique → revised final.
//! - Deliberation: Multi-round with convergence check.
//! - PlanImplement: 2-phase: strategy plan → implementation plan.
//!
//! Skills anchor the judge's reasoning with hKask's pragmatic methodology.

use crate::config::{AlgoMethod, ConvergenceVerdict, FusionConfig, FusionMode, FusionSkill};
use hkask_types::template::LLMParameters;
use hkask_types::{
    ChatToolDefinition, InferenceError, InferencePort, InferenceResult, InferenceUsage,
    StructuredToolCall,
};
use tracing::info;

// ── Skill Anchor Prompts ─────────────────────────────────────────────────────

/// The compact methodology prompt injected for each anchored skill.
fn skill_prompt(skill: &FusionSkill) -> &'static str {
    match skill {
        FusionSkill::PragmaticSemantics => {
            "Pragmatic Semantics: Classify every claim by certainty level (IS vs OUGHT, \
             declarative vs probabilistic vs subjunctive). Surface unstated assumptions. \
             Flag conflation of fact and preference. Trace provenance of key claims."
        }
        FusionSkill::PragmaticCybernetics => {
            "Pragmatic Cybernetics: Identify feedback loops, measure variety, assess \
             homeostasis. Every system change must have an observable feedback mechanism. \
             Prefer closed-loop over open-loop interventions. Map control channels."
        }
        FusionSkill::CodingGuidelines => {
            "Coding Guidelines (Karpathy): (1) Think before coding — surface assumptions, \
             present alternatives. (2) Simplicity first — minimum code, no speculative features. \
             (3) Surgical changes — touch only what you must, match existing style. \
             (4) Goal-driven — define verifiable success criteria, loop until verified."
        }
        FusionSkill::DeepModule => {
            "Deep Module (Ousterhout): Apply the deletion test — can this module's callers \
             be deleted without losing complexity? Interface minimalism — ≤7 public items. \
             Dependency direction — depend on what's stable, not what's convenient."
        }
        FusionSkill::Essentialist => {
            "Essentialist: Apply the 3-gate challenge loop: (1) Exist — does this artifact \
             earn its existence? (2) Surface — is its interface minimal? (3) Contract — \
             are its behavioral contracts explicit and verified?"
        }
        FusionSkill::Superforecasting => {
            "Superforecasting (Tetlock GJP 8-stage): Triage the question into the \
             Goldilocks zone before investing effort. Fermi-decompose into sub-questions. \
             Anchor on outside-view base rates, then adjust with inside-view evidence. \
             Update with Bayesian likelihood ratios. Synthesize a dragonfly-eye view \
             (steelman opposing models). Calibrate to a precise probability with a \
             defensible range. Record for Brier-scored post-mortem. Run the independent \
             quality gate and convergence check. Express uncertainty as calibrated \
             probability ranges, not binary predictions."
        }
        FusionSkill::Mcda => {
            "Multi-Criteria Decision Analysis: Identify criteria, weight and score \
             alternatives, check for compensation masking. Perform sensitivity analysis. \
             Prefer robust options that perform well across weight ranges."
        }
        FusionSkill::TestDrivenDevelopment => {
            "TDD: Red-Green-Refactor. Write the contract first (pre:/post:), then a \
             property-based test verifying it (RED), implement minimally (GREEN), \
             refactor while contracts hold. Vertical tracer-bullet: one thin slice end-to-end."
        }
        FusionSkill::BugHunt => {
            "Bug Hunt: Define quality as value to someone who matters. Apply Beizer's bug \
             taxonomy and Bach/Bolton's heuristic test strategy. Use exploratory charters. \
             Reproduce before diagnosing. Isolate one variable at a time."
        }
        FusionSkill::Diagnose => {
            "Diagnose: Cybernetic debugging — build feedback loop, reproduce, hypothesize, \
             instrument, fix, regression-test. Align sense→orient→decide→act. Never change \
             code without a reproducing test first."
        }
        FusionSkill::Falsifiability => {
            "Falsifiability (Popper/Platt/Chamberlin): Rule out the untestable. Generate \
             multiple falsifiable hypotheses. Construct minimal counterfactuals. Design \
             discriminating tests. Eliminate the falsified — corroborate survivors, \
             never confirm."
        }
        FusionSkill::GrillMe => {
            "Grill Me: Socratic interrogation at escalating difficulty — Recall → Mechanism \
             → Rationale → Edge Cases → Synthesis. Probe gaps, challenge assumptions, \
             produce gap analysis. Do not accept hand-waving."
        }
        FusionSkill::IdiomaticRust => {
            "Idiomatic Rust (Hoare): Make wrong usage impossible — validating newtypes, \
             two-variant enums for bools, non-empty collections. Single owners, explicit \
             error domains, thiserror for libraries. Many small traits over few large ones."
        }
        FusionSkill::Metacognition => {
            "Metacognition: Decompose goals, self-assess progress, detect ellipses via Bloom's \
             method, rotate perspectives, calibrate strategy. Be honest — overestimating \
             progress is worse than underestimating. Improve through GEPA optimization."
        }
        FusionSkill::RefactorArchitecture => {
            "Refactor Architecture: End-to-end architecture refactoring. Discover phase: \
             explore friction, rank deepening candidates, walk the design tree. Execution \
             phase: audit duplication, plan strangler-fig migration, verify integrity. \
             Deep-module discipline for extracted services. Vertical tracer-bullet TDD. \
             Delete only after full verification. Includes migration-strategy phase \
             (folded from strangler-fig). Merged from improve-codebase-architecture \
             and refactor-service-layer."
        }
        FusionSkill::PromptEnhance => {
            "Prompt Enhance: Typed prompt enhancement. Classify the prompt (coding, reasoning, \
             creative, classification, extraction, agent-task, meta) before refining. \
             Apply the deletion test to prompt sections — does complexity reappear in model \
             failures if deleted? Trace provenance of every hardcoded claim (Specification \
             > Implementation > Inference); flag Inference-tier claims as fragile. Keep \
             generator and critic decoupled — never let the same pass that proposes a \
             refinement also evaluate it."
        }
        FusionSkill::BehavioralLocks => {
            "Behavioral Locks (Codette): (1) Answer, then stop — reduce elaboration drift \
             and philosophical padding after the answer. (2) Constraints override all \
             modes — user format instructions beat personality. (3) Self-check completeness \
             — verify the answer is full and clean before sending. (4) No incomplete \
             outputs — avoid ending mid-thought; simplify instead of cramming."
        }
        FusionSkill::AegisProSocial => {
            "AEGIS Pro-Social Ethics (experimental): Evaluate the response across six \
             frameworks with relational priority — Care 0.30 (vulnerability, relational \
             harm), Reciprocity 0.25 (mutual obligation across time and stakeholders, \
             anti-extraction), Ubuntu 0.20 (communal trust, \"I am because we are\"), \
             Utilitarian 0.10 (aggregate welfare — floor, not ceiling), Deontological \
             0.10 (rights and consent — hard floor, no pro-social benefit justifies \
             violating consent), Virtue 0.05 (intellectual honesty, humility). \
             Constraint floors: a response that violates consent or causes net harm \
             is flagged regardless of pro-social score. Verdict: allow / rewrite \
             (specify under-served framework) / escalate (human review). Advisory only — \
             does not gate the response."
        }
    }
}

/// Build the skill anchor section of the judge's system prompt.
fn build_skill_anchor(skills: &[FusionSkill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut anchor = String::from(
        "\n\n## Reasoning Framework\n\
         You are anchored on the following methodologies. Apply them in your analysis:\n\n",
    );
    for skill in skills {
        anchor.push_str(&format!("- {}\n", skill_prompt(skill)));
    }
    anchor
}

/// Determine the effective panel for a fusion call, applying complexity
/// routing and pressure adaptation when enabled.
///
/// Returns a slice of `fusion.panel`. When both features are disabled
/// (the default), returns the full panel unchanged.
fn resolve_effective_panel<'a>(prompt: &str, fusion: &'a FusionConfig) -> &'a [String] {
    let complexity = classify_complexity(prompt);
    let effective = effective_panel(&fusion.panel, complexity, fusion.panel_sizing_enabled);
    pressure_adjusted_panel(effective, fusion.pressure_adaptive_enabled)
}

// ── Panel Dispatch ───────────────────────────────────────────────────────────

/// Result from a single panel model.
struct PanelResponse {
    model_name: String,
    text: String,
    usage: InferenceUsage,
    /// Tool calls requested by this panelist (OpenAI function calling).
    /// Carried through so the algo judge can pass them to the caller when
    /// any panelist requests tools. Without this, fusion silently drops
    /// tool calls even when the panel unanimously requests them.
    tool_calls: Vec<StructuredToolCall>,
    /// Why this panelist stopped generating ("stop", "tool_calls", etc.).
    finish_reason: String,
}

/// Dispatch to all panel models in parallel.
async fn dispatch_panel(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    panel: &[String],
) -> Vec<PanelResponse> {
    use futures_util::future::join_all;

    // Panel models must bypass fusion to avoid routing back through the judge.
    // Adapter must be cleared so the panel model_override is respected, not the
    // caller's LoRA adapter (which is for the non-fusion dispatch path).
    let panel_params = LLMParameters {
        bypass_fusion: true,
        adapter: None,
        ..params.clone()
    };
    let panel_params = &panel_params;

    let futures: Vec<_> = panel
        .iter()
        .map(|model_name| async move {
            match router
                .generate_with_model(prompt, panel_params, Some(model_name.as_str()), tools)
                .await
            {
                Ok(result) => Some(PanelResponse {
                    model_name: model_name.clone(),
                    text: result.text,
                    usage: result.usage,
                    tool_calls: result.tool_calls,
                    finish_reason: result.finish_reason,
                }),
                Err(e) => {
                    tracing::warn!(
                        target: "reg.inference",
                        panel_model = %model_name,
                        error = %e,
                        "Panel model generation failed"
                    );
                    None
                }
            }
        })
        .collect();

    join_all(futures).await.into_iter().flatten().collect()
}

// ── Codette-inspired: Epistemic Tension & Coherence ─────────────────────────
//
// Computes the variance of panel response embeddings around their centroid.
// Based on Codette's RC+ξ formalism (Harrison 2026, §3.3–3.4, eq. 2–3):
//   ξ = (1/k) Σ ‖A_i(x) − Ā(x)‖²
//   Γ = 1 / (1 + ξ)
// where A_i are the panel response embeddings and Ā is their weighted mean.
// Lower disagreement (ξ↓) implies higher coherence (Γ↑).
//
// This is a *measurement* of inter-panelist disagreement, not a correctness
// claim. It complements the judge's self-reported convergence verdict in
// deliberation mode — the judge may see subtleties the embedding misses, so
// the judge verdict still wins when they disagree.

/// Compute epistemic tension (ξ) from a set of response embeddings.
///
/// ξ is the mean squared Euclidean distance of each embedding from their
/// centroid. Returns 0.0 for a single response (no disagreement possible).
fn epistemic_tension(embeddings: &[Vec<f32>]) -> f64 {
    if embeddings.len() <= 1 {
        return 0.0;
    }
    let dim = embeddings[0].len();
    if dim == 0 {
        return 0.0;
    }
    // Guard against mismatched embedding dimensions — skip any embedding
    // whose length differs from the first. This prevents index-out-of-bounds
    // panics when the embedding source returns inconsistent vectors.
    let valid: Vec<&Vec<f32>> = embeddings.iter().filter(|e| e.len() == dim).collect();
    if valid.len() <= 1 {
        return 0.0;
    }
    let k = valid.len() as f64;
    // Centroid: element-wise mean.
    let centroid: Vec<f64> = (0..dim)
        .map(|j| valid.iter().map(|e| e[j] as f64).sum::<f64>() / k)
        .collect();
    // Mean squared distance from centroid.
    let sum_sq: f64 = valid
        .iter()
        .map(|e| {
            (0..dim)
                .map(|j| {
                    let diff = e[j] as f64 - centroid[j];
                    diff * diff
                })
                .sum::<f64>()
        })
        .sum::<f64>();
    sum_sq / k
}

/// Coherence index Γ = 1 / (1 + ξ). Range (0, 1].
/// Higher Γ means panel responses are more similar (less disagreement).
fn coherence(xi: f64) -> f64 {
    1.0 / (1.0 + xi)
}

/// Fetch embeddings for a set of texts via the inference router.
///
/// Uses the configured embedding model (`HKASK_EMBEDDING_MODEL`, default
/// `DeepInfra/Qwen/Qwen3-Embedding-0.6B`). Returns empty vec on failure — the
/// caller treats empty embeddings as "measurement unavailable" and skips
/// the ξ/Γ computation.
async fn fetch_embeddings(router: &dyn InferencePort, texts: &[String]) -> Vec<Vec<f32>> {
    // The InferencePort trait doesn't expose embeddings directly. We use
    // a lightweight approach: ask the router to generate a JSON array of
    // embeddings by prompting the embedding model. This is a fallback —
    // production use should route through EmbeddingRouter directly.
    //
    // For now, return empty — the ξ/Γ feature is opt-in and requires
    // the operator to wire an embedding source. When embeddings are empty,
    // mode_deliberation skips the measured-convergence signal.
    let _ = (router, texts);
    Vec::new()
}

// ── Codette-inspired: Query Complexity Router ───────────────────────────────
//
// Codette §4.1 classifies queries as SIMPLE/MEDIUM/COMPLEX to set agent
// weights. We adapt this to panel sizing: simple queries dispatch fewer
// panel models, reducing cost without sacrificing quality.

/// Query complexity classification for panel sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryComplexity {
    /// Direct factual queries → 1 panel model.
    Simple,
    /// Conceptual queries → 2 panel models.
    Medium,
    /// Multi-domain/ethical/complex queries → full panel.
    Complex,
}

/// Classify a prompt's complexity using a keyword + length heuristic.
///
/// This is a fast, deterministic classifier — no LLM call. It uses:
/// - Prompt length (short → simple)
/// - Question word density (high → complex)
/// - Code presence (code blocks → medium+)
/// - Multi-domain indicators (ethics, tradeoff, compare, design → complex)
fn classify_complexity(prompt: &str) -> QueryComplexity {
    let trimmed = prompt.trim();
    let len = trimmed.len();
    let lower = trimmed.to_lowercase();

    // Complex indicators: multi-domain reasoning keywords.
    let complex_keywords = [
        "ethics",
        "ethical",
        "tradeoff",
        "trade-offs",
        "compare",
        "contrast",
        "design",
        "architecture",
        "analyze",
        "evaluate",
        "assess",
        "justify",
        "should",
        "ought",
        "moral",
        "stakeholder",
        "consequence",
        "implication",
        "multi",
        "interdisciplinary",
        "holistic",
    ];
    let complex_hits = complex_keywords
        .iter()
        .filter(|kw| lower.contains(*kw))
        .count();

    // Code presence (triple backtick or common code patterns).
    let has_code = lower.contains("```") || lower.contains("fn ") || lower.contains("def ");

    // Simple indicators: very short, direct factual questions.
    let is_short = len < 100;
    let simple_patterns = [
        "what is", "what's", "who is", "who's", "when did", "where is",
    ];
    let is_simple_factual = is_short && simple_patterns.iter().any(|p| lower.starts_with(p));

    if complex_hits >= 2 || (complex_hits >= 1 && len > 300) {
        QueryComplexity::Complex
    } else if has_code || complex_hits == 1 || len > 200 {
        QueryComplexity::Medium
    } else if is_simple_factual || len < 50 {
        QueryComplexity::Simple
    } else {
        // Default to Medium for ambiguous classifications — safer than Simple.
        QueryComplexity::Medium
    }
}

/// Determine the effective panel size based on complexity and config.
///
/// Returns a slice of the panel to dispatch. When panel sizing is disabled,
/// returns the full panel.
fn effective_panel(
    panel: &[String],
    complexity: QueryComplexity,
    panel_sizing_enabled: bool,
) -> &[String] {
    if !panel_sizing_enabled {
        return panel;
    }
    let max = match complexity {
        QueryComplexity::Simple => 1,
        QueryComplexity::Medium => 2,
        QueryComplexity::Complex => panel.len(),
    };
    let take = max.min(panel.len());
    &panel[..take]
}

// ── Codette-inspired: Substrate-Aware Pressure Tracking ────────────────────
//
// Codette §9 monitors hardware pressure and reduces reasoning depth under
// load. We adapt this to panel-model latency: if panel models are slow
// (high rolling-average latency), reduce panel size to avoid timeouts.
// Design position: degraded output is better than hard failure.

/// Rolling-average latency tracker for substrate-aware degradation.
///
/// Tracks the mean dispatch_panel duration over recent calls. When the
/// rolling average exceeds thresholds, the orchestrator reduces panel size.
/// This is a process-local static — it persists across fusion calls within
/// the same process lifetime.
use std::sync::atomic::{AtomicU64, Ordering};

static ROLLING_LATENCY_MS: AtomicU64 = AtomicU64::new(0);
static LATENCY_SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Update the rolling latency with a new sample.
/// Uses an exponential moving average with α = 0.3.
///
/// Uses `fetch_update` for atomic read-modify-write — the orchestrator can
/// be called concurrently from multiple threads, so a plain load→store
/// sequence would lose updates.
fn record_latency(duration_ms: u64) {
    ROLLING_LATENCY_MS
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |prev| {
            let new_avg = if prev == 0 {
                duration_ms
            } else {
                // EMA: new = α * sample + (1-α) * prev, with α = 0.3
                ((0.3 * duration_ms as f64) + (0.7 * prev as f64)) as u64
            };
            Some(new_avg)
        })
        .ok();
    LATENCY_SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Compute the current pressure score P ∈ [0, 1].
///
/// Maps rolling latency to a 0–1 score:
/// - < 2000ms → P ≈ 0 (low pressure)
/// - 2000–8000ms → P scales linearly (moderate)
/// - > 8000ms → P ≈ 1 (high pressure)
fn compute_pressure() -> f64 {
    let avg_ms = ROLLING_LATENCY_MS.load(Ordering::Relaxed) as f64;
    if avg_ms <= 0.0 {
        return 0.0;
    }
    // Linear mapping with clamping: 2000ms → 0.0, 8000ms → 1.0
    ((avg_ms - 2000.0) / 6000.0).clamp(0.0, 1.0)
}

/// Determine panel size after substrate-aware pressure reduction.
///
/// When pressure_adaptive is enabled:
/// - P < 0.3 → full panel
/// - 0.3 ≤ P < 0.7 → 2 models
/// - P ≥ 0.7 → 1 model
fn pressure_adjusted_panel(panel: &[String], pressure_adaptive_enabled: bool) -> &[String] {
    if !pressure_adaptive_enabled {
        return panel;
    }
    let pressure = compute_pressure();
    let max = if pressure >= 0.7 {
        1
    } else if pressure >= 0.3 {
        2
    } else {
        panel.len()
    };
    let take = max.min(panel.len());
    &panel[..take]
}

/// Format panel responses for judge consumption (identity display order).
fn format_panel_responses(responses: &[PanelResponse]) -> String {
    let order: Vec<usize> = (0..responses.len()).collect();
    format_panel_responses_in_order(responses, &order)
}

/// Format panel responses in a given display order. `order` maps display slot
/// → index into `responses`. Varying the order across judge calls mitigates
/// position bias (Zheng et al. 2024, arXiv:2406.07791): no single response
/// always occupies the favored first position.
fn format_panel_responses_in_order(responses: &[PanelResponse], order: &[usize]) -> String {
    let mut sections = String::new();
    for (slot, &idx) in order.iter().enumerate() {
        let resp = &responses[idx];
        sections.push_str(&format!(
            "\n### Panelist {}: {}\n{}\n",
            slot + 1,
            resp.model_name,
            resp.text
        ));
    }
    sections
}

/// Identify which panel response a judge's verbatim pick corresponds to, by
/// maximum Jaccard similarity. Used by best-of-n swap-revote to compare picks
/// across display orderings without relying on exact string equality (LLMs
/// occasionally add minor whitespace when copying verbatim).
fn identify_pick(pick_text: &str, responses: &[PanelResponse]) -> usize {
    let mut best = 0usize;
    let mut best_score = -1.0f64;
    for (i, resp) in responses.iter().enumerate() {
        let s = jaccard(pick_text, &resp.text);
        if s > best_score {
            best_score = s;
            best = i;
        }
    }
    best
}

/// Sum a collection of InferenceUsage values into a single aggregate.
fn sum_usage(usages: impl IntoIterator<Item = InferenceUsage>) -> InferenceUsage {
    usages
        .into_iter()
        .fold(InferenceUsage::default(), |acc, u| InferenceUsage {
            prompt_tokens: acc.prompt_tokens + u.prompt_tokens,
            completion_tokens: acc.completion_tokens + u.completion_tokens,
            total_tokens: acc.total_tokens + u.total_tokens,
        })
}

/// Collect tool calls from panel responses.
///
/// Returns `(tool_calls, finish_reason)`. If any panelist returned
/// `finish_reason=tool_calls` with a non-empty `tool_calls` vector, the tool
/// calls from the first such panelist are passed through, and the finish_reason
/// is set to `"tool_calls"`. Otherwise, returns an empty vector and `"stop"`.
///
/// This prevents fusion from silently dropping tool requests when the panel
/// unanimously agrees a tool is needed. The first-panelist-wins strategy is
/// deterministic and avoids the complexity of merging tool call arguments
/// across panelists (which would require semantic equality checking).
fn collect_tool_calls(responses: &[PanelResponse]) -> (Vec<StructuredToolCall>, String) {
    for r in responses {
        if r.finish_reason == "tool_calls" && !r.tool_calls.is_empty() {
            tracing::info!(
                target: "reg.fusion",
                panel_model = %r.model_name,
                tool_call_count = r.tool_calls.len(),
                "Fusion: passing through tool calls from panelist"
            );
            return (r.tool_calls.clone(), "tool_calls".to_string());
        }
    }
    (Vec::new(), "stop".to_string())
}

/// Add intermediate usage (panel models, prior judge rounds) to the final result.
fn with_aggregated_usage(
    mut result: InferenceResult,
    intermediate_usages: &[InferenceUsage],
) -> InferenceResult {
    let total = sum_usage(intermediate_usages.iter().cloned());
    result.usage.prompt_tokens += total.prompt_tokens;
    result.usage.completion_tokens += total.completion_tokens;
    result.usage.total_tokens += total.total_tokens;
    result
}

// ── Algo Judge (algorithmic merge, no LLM) ────────────────────────────────────

/// Sentinel judge model name for algorithmic merge (no LLM call).
pub(crate) const ALGO_JUDGE: &str = "algo";

/// Parse a JSON value from a model response text, tolerating markdown fences
/// and surrounding prose. Falls back to `Value::Null` on parse failure.
fn parse_json_lenient(text: &str) -> serde_json::Value {
    use serde_json::Value;

    // Direct parse
    if let Ok(v) = serde_json::from_str(text) {
        return v;
    }

    let trimmed = text.trim();

    // Markdown code fence
    if let Some(json_start) = trimmed.find("```json") {
        let after_fence = &trimmed[json_start + 7..];
        if let Some(v) = after_fence
            .find("```")
            .and_then(|end| serde_json::from_str(after_fence[..end].trim()).ok())
        {
            return v;
        }
    }

    // Bare JSON object boundaries
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}'))
        && let Ok(v) = serde_json::from_str(&trimmed[start..=end])
    {
        return v;
    }

    Value::Null
}

/// Merge two JSON values from panel responses (algo / no-judge path).
///
/// Objects: merges keys recursively.
/// Arrays: concatenates with case-insensitive, trim-tolerant dedup for strings
/// and value dedup for primitives (numbers, bools, null). Objects/arrays are
/// kept verbatim — structural differences between panelists are meaningful.
/// Strings/scalars: uses A when equal (case-insensitive, trimmed), otherwise
/// annotates `[A:... B:...]`.
///
/// # Pairwise contract (N=2)
///
/// The `[A:... B:...]` divergence annotation is a **pairwise** output contract
/// (documented in `hkask-memory` and `FUNCTIONAL_SPECIFICATION.md`) for the
/// algo/no-judge path's two-peer merge. `algo_merge` folds the panel with
/// `reduce`, so for **N>2 panelists** divergent strings nest as
/// `[A:[A:x B:y] B:z]` — still a valid JSON string, but no longer a flat
/// pairwise annotation. The algo/no-judge path is specified for two peers; for
/// N>2 panelists use a judge-based mode (`synthesis`, `critique`, …) instead.
fn merge_json_values(a: &serde_json::Value, b: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    use std::collections::HashSet;

    // Single normalization for both string dedup (arrays) and string equality
    // (scalars) — previously these used two different rules.
    fn norm_key(s: &str) -> String {
        s.to_lowercase().trim().to_string()
    }

    match (a, b) {
        (Value::Object(map_a), Value::Object(map_b)) => {
            let mut merged = map_a.clone();
            for (key, val_b) in map_b {
                merged
                    .entry(key.clone())
                    .and_modify(|existing| *existing = merge_json_values(existing, val_b))
                    .or_insert_with(|| val_b.clone());
            }
            Value::Object(merged)
        }
        (Value::Array(arr_a), Value::Array(arr_b)) => {
            let mut seen_strings: HashSet<String> = HashSet::new();
            let mut seen_prims: Vec<Value> = Vec::new();
            let mut result = Vec::new();
            for v in arr_a.iter().chain(arr_b.iter()) {
                match v {
                    Value::String(s) => {
                        if seen_strings.insert(norm_key(s)) {
                            result.push(v.clone());
                        }
                    }
                    // Primitives: dedup by value so [1,1,1] collapses to [1].
                    Value::Number(_) | Value::Bool(_) | Value::Null => {
                        if !seen_prims.contains(v) {
                            seen_prims.push(v.clone());
                            result.push(v.clone());
                        }
                    }
                    // Objects/arrays: keep all (structural differences matter).
                    _ => result.push(v.clone()),
                }
            }
            Value::Array(result)
        }
        (Value::String(sa), Value::String(sb)) => {
            if norm_key(sa) == norm_key(sb) {
                a.clone()
            } else {
                Value::String(format!("[A:{} B:{}]", sa, sb))
            }
        }
        (Value::Null, _) => b.clone(),
        (_, Value::Null) => a.clone(),
        _ if a == b => a.clone(),
        _ => Value::String(format!("[A:{} B:{}]", a, b)),
    }
}

/// Algorithmic judge: parse panel responses as JSON, merge via recursive union.
/// No LLM call — deterministic, zero-cost judge. Panel model usage is aggregated.
///
/// Tool-call pass-through: if any panelist returned `finish_reason=tool_calls`,
/// the tool calls from the first such panelist are passed through unchanged.
/// This prevents fusion from silently dropping tool requests when the panel
/// unanimously agrees a tool is needed.
fn algo_merge(responses: &[PanelResponse]) -> InferenceResult {
    let merged = responses
        .iter()
        .map(|r| parse_json_lenient(&r.text))
        .reduce(|a, b| merge_json_values(&a, &b))
        .unwrap_or(serde_json::Value::Null);

    let total_usage = sum_usage(responses.iter().map(|r| r.usage.clone()));

    // Tool-call pass-through: take the first panelist that requested tools.
    let (tool_calls, finish_reason) = collect_tool_calls(responses);

    InferenceResult {
        text: merged.to_string(),
        model: ALGO_JUDGE.to_string(),
        usage: total_usage,
        finish_reason,
        token_probabilities: None,
        tool_calls,
        reasoning: None,
    }
}

// ── Algo Judge: majority-vote method ──────────────────────────────────────────

/// Case-insensitive/trim-tolerant equality for strings; serde equality otherwise.
fn values_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::String(sa), Value::String(sb)) => {
            sa.to_lowercase().trim() == sb.to_lowercase().trim()
        }
        _ => a == b,
    }
}

/// Stable normalized key for dedup: lowercase-trimmed string for strings,
/// `to_string()` for everything else.
fn norm_value_key(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.to_lowercase().trim().to_string(),
        serde_json::Value::Object(o) => {
            // Canonical form: sorted keys, recursive — so two objects equal modulo
            // key insertion order dedup/match as the same item.
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("{}:{}", k, norm_value_key(&o[k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        other => other.to_string(),
    }
}

/// Majority-vote merge of panel JSON values. Unlike `merge_json_values` (binary
/// pairwise union), this folds all panelists at once: object fields take the
/// value appearing in a majority of panelists (recursively); array items are
/// kept only if a majority of panelists include them; scalars take the majority
/// value. Scales beyond 2 panelists where the pairwise `[A:... B:...]`
/// annotation degrades. With no majority, the first value is retained.
fn vote_json_values(values: &[serde_json::Value]) -> serde_json::Value {
    use serde_json::Value;
    use std::collections::{BTreeMap, HashSet};
    if values.is_empty() {
        return Value::Null;
    }
    // All objects → per-key recursive vote.
    if values.iter().all(|v| v.is_object()) {
        let mut key_vals: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for v in values {
            for (k, val) in v.as_object().unwrap() {
                key_vals.entry(k.clone()).or_default().push(val.clone());
            }
        }
        let mut out = serde_json::Map::new();
        for (k, vs) in key_vals {
            out.insert(k, vote_json_values(&vs));
        }
        return Value::Object(out);
    }
    // All arrays → keep items appearing in a majority of panelists.
    if values.iter().all(|v| v.is_array()) {
        let threshold = values.len() / 2 + 1;
        let mut kept: Vec<Value> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for v in values {
            for item in v.as_array().unwrap() {
                if !seen.insert(norm_value_key(item)) {
                    continue;
                }
                let count = values
                    .iter()
                    .filter(|p| p.as_array().unwrap().iter().any(|x| values_equal(x, item)))
                    .count();
                if count >= threshold {
                    kept.push(item.clone());
                }
            }
        }
        return Value::Array(kept);
    }
    // Scalars / mixed → majority vote by normalized equality.
    let threshold = values.len() / 2 + 1;
    for v in values {
        let count = values.iter().filter(|o| values_equal(o, v)).count();
        if count >= threshold {
            return v.clone();
        }
    }
    // No majority — keep the first.
    values[0].clone()
}

/// Algorithmic judge (vote): parse panel responses as JSON, merge via majority
/// vote. No LLM call — deterministic, zero-cost judge. Panel usage is aggregated.
fn algo_vote(responses: &[PanelResponse]) -> InferenceResult {
    // Vote needs ≥3 panelists to form a majority. With fewer, a strict-majority
    // vote degenerates to first-wins (not a vote) — fall back to merge.
    if responses.len() < 3 {
        tracing::warn!(
            target: "reg.fusion",
            algo_method = "vote",
            panel_count = responses.len(),
            "algo:vote requires ≥3 panelists — falling back to merge"
        );
        return algo_merge(responses);
    }
    let values: Vec<serde_json::Value> = responses
        .iter()
        .map(|r| parse_json_lenient(&r.text))
        .collect();
    let voted = vote_json_values(&values);
    let total_usage = sum_usage(responses.iter().map(|r| r.usage.clone()));
    // Tool-call pass-through: take the first panelist that requested tools.
    let (tool_calls, finish_reason) = collect_tool_calls(responses);
    InferenceResult {
        text: voted.to_string(),
        model: ALGO_JUDGE.to_string(),
        usage: total_usage,
        finish_reason,
        token_probabilities: None,
        tool_calls,
        reasoning: None,
    }
}

/// Call the judge model with a given prompt.
async fn call_judge(
    router: &dyn InferencePort,
    judge_model: &str,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
) -> Result<InferenceResult, InferenceError> {
    let judge_params = LLMParameters {
        bypass_fusion: true,
        adapter: None,
        ..params.clone()
    };
    router
        .generate_with_model(prompt, &judge_params, Some(judge_model), tools)
        .await
}

// ── Deliberation convergence (structured judge verdict) ──────────────────────
//
// `deliberation` convergence is decided by the judge emitting a STRUCTURED
// verdict — `{"converged": bool, "synthesis"|"follow_up": "…"}` — parsed with
// the existing `parse_json_lenient`. This is semantically sound (the judge reads
// the panel responses and reports stabilization) and format-robust (structured
// parse, not a `FOLLOW_UP:` prose prefix). The former Beta+KS+Jaccard external
// detector was removed: its lexical-Jaccard agreement signal measured token
// overlap, not semantic convergence, so it never fired for the diverse
// (mixed-provider) panels that `deliberation` exists to serve.

/// Parse a judge's structured deliberation verdict.
///
/// Accepts `{"converged": true, "synthesis": "…"}` → `(Converged, Some(synthesis))`
/// or `{"converged": false, "follow_up": "…"}` → `(Continue, Some(follow_up))`.
/// If a declared payload field is absent the raw text is used. A non-JSON
/// response is treated as a final synthesis (matching the former `FOLLOW_UP:`
/// fallback: a response not declaring a follow-up is the final answer).
///
/// expect: "Deliberation converges when the judge reports stabilization, parsed structurally"
/// [P9] Motivating: Homeostatic Self-Regulation — closed-loop convergence detection
/// pre:  `text` is the judge model's response to a structured-verdict prompt
/// post: `Converged` → payload is the final synthesis; `Continue` → payload is a follow-up question
fn parse_convergence_verdict(text: &str) -> (ConvergenceVerdict, Option<String>) {
    let v = parse_json_lenient(text);
    if let Some(obj) = v.as_object() {
        return match obj.get("converged").and_then(|x| x.as_bool()) {
            Some(true) => {
                let synth = obj
                    .get("synthesis")
                    .and_then(|s| s.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| text.trim().to_string());
                (ConvergenceVerdict::Converged, Some(synth))
            }
            Some(false) => {
                let follow_up = obj
                    .get("follow_up")
                    .and_then(|s| s.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| text.trim().to_string());
                (ConvergenceVerdict::Continue, Some(follow_up))
            }
            None => (ConvergenceVerdict::Converged, Some(text.trim().to_string())),
        };
    }
    // Non-JSON: treat the response as a final synthesis (former FOLLOW_UP: fallback).
    (ConvergenceVerdict::Converged, Some(text.trim().to_string()))
}

/// Token-set Jaccard similarity in `[0, 1]`. Case-insensitive, whitespace-split.
/// Two empty texts are vacuously identical (1.0); one empty → 0.0.
fn jaccard(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let lowered_a = a.to_lowercase();
    let lowered_b = b.to_lowercase();
    let set_a: HashSet<&str> = lowered_a.split_whitespace().collect();
    let set_b: HashSet<&str> = lowered_b.split_whitespace().collect();
    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 1.0;
    }
    set_a.intersection(&set_b).count() as f64 / union as f64
}

// ── Mode Implementations ─────────────────────────────────────────────────────

/// Best-of-N: Judge evaluates all panel responses and picks the single best.
///
/// Position-bias mitigation (Zheng et al. 2024, arXiv:2406.07791): with two or
/// more panelists the judge votes twice — once with candidates in dispatch
/// order, once reversed — and the picks are compared by matching the verbatim
/// output back to its source response. Agreement yields high confidence;
/// disagreement flags position bias (logged, first pick returned). A single
/// panelist skips the swap (no position to bias).
async fn mode_best_of_n(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    fusion: &FusionConfig,
) -> Result<InferenceResult, InferenceError> {
    let effective = resolve_effective_panel(prompt, fusion);
    let responses = dispatch_panel(router, prompt, params, tools, effective).await;
    if responses.is_empty() {
        return Err(InferenceError::Generation("All panel models failed".into()));
    }

    let skill_anchor = build_skill_anchor(&fusion.skills);
    let n = responses.len();
    let panel_usages: Vec<InferenceUsage> = responses.iter().map(|r| r.usage.clone()).collect();

    let build_prompt = |candidates: &str| {
        format!(
            "You are a best-of-N judge. Below are responses from {n} models to the same prompt. \
             Evaluate each response and select the single best one. Output ONLY the chosen \
             response verbatim — no commentary, no synthesis, no justification.{skills}

\
             ## Original Prompt
{prompt}

## Candidate Responses{candidates}",
            n = n,
            skills = skill_anchor,
            candidates = candidates,
        )
    };

    // Single panelist — no position to bias; one judge call.
    if n == 1 {
        let judge_prompt = build_prompt(&format_panel_responses(&responses));
        let result = call_judge(router, &fusion.judge, &judge_prompt, params, tools).await?;
        return Ok(with_aggregated_usage(result, &panel_usages));
    }

    // Swap-revote: two display orderings, compare identified picks.
    // The two judge calls are independent — run them concurrently to halve latency.
    let order_a: Vec<usize> = (0..n).collect();
    let order_b: Vec<usize> = (0..n).rev().collect();
    let prompt_a = build_prompt(&format_panel_responses_in_order(&responses, &order_a));
    let prompt_b = build_prompt(&format_panel_responses_in_order(&responses, &order_b));

    let (result_a, result_b) = futures_util::join!(
        call_judge(router, &fusion.judge, &prompt_a, params, tools),
        call_judge(router, &fusion.judge, &prompt_b, params, tools),
    );
    let result_a = result_a?;
    let result_b = result_b?;
    let idx_a = identify_pick(&result_a.text, &responses);
    let idx_b = identify_pick(&result_b.text, &responses);

    let mut all_usages = panel_usages;
    all_usages.push(result_b.usage.clone());

    if idx_a == idx_b {
        info!(
            target: "reg.fusion",
            fusion_mode = "best-of-n",
            verdict = "agree",
            picked = idx_a,
            "Best-of-N swap-revote agreed"
        );
    } else {
        info!(
            target: "reg.fusion",
            fusion_mode = "best-of-n",
            verdict = "disagree",
            position_bias = true,
            pick_a = idx_a,
            pick_b = idx_b,
            "Best-of-N swap-revote disagreed — position bias suspected"
        );
    }
    // result_a.text is the judge's verbatim copy of responses[idx_a]; return it.
    // result_b.usage is aggregated; result_a.usage is folded in by with_aggregated_usage.
    Ok(with_aggregated_usage(result_a, &all_usages))
}

/// Synthesis: Judge composes a unified response from all panelists.
async fn mode_synthesis(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    fusion: &FusionConfig,
) -> Result<InferenceResult, InferenceError> {
    let effective = resolve_effective_panel(prompt, fusion);
    let responses = dispatch_panel(router, prompt, params, tools, effective).await;
    if responses.is_empty() {
        return Err(InferenceError::Generation(
            "All panel models failed — cannot synthesize".into(),
        ));
    }

    let skill_anchor = build_skill_anchor(&fusion.skills);
    let judge_prompt = format!(
        "You are a synthesis judge. Below are responses from a panel of models to the \
         same prompt. Synthesize the best answer, incorporating the strongest elements \
         from each response. Resolve any contradictions explicitly. Be concise and \
         accurate.{skills}\n\n\
         ## Original Prompt\n{prompt}\n\n## Panel Responses{candidates}",
        skills = skill_anchor,
        prompt = prompt,
        candidates = format_panel_responses(&responses),
    );

    let result = call_judge(router, &fusion.judge, &judge_prompt, params, tools).await?;
    let panel_usages: Vec<InferenceUsage> = responses.iter().map(|r| r.usage.clone()).collect();
    Ok(with_aggregated_usage(result, &panel_usages))
}

/// Critique: 2-round — draft synthesis, panel critiques draft, judge revises.
async fn mode_critique(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    fusion: &FusionConfig,
) -> Result<InferenceResult, InferenceError> {
    let skill_anchor = build_skill_anchor(&fusion.skills);

    // Round 1: Initial synthesis
    let effective = resolve_effective_panel(prompt, fusion);
    let r1_responses = dispatch_panel(router, prompt, params, tools, effective).await;
    if r1_responses.is_empty() {
        return Err(InferenceError::Generation(
            "All panel models failed in round 1".into(),
        ));
    }

    let r1_judge_prompt = format!(
        "You are a synthesis judge (Round 1). Below are responses from a panel of models. \
         Produce an initial draft synthesis incorporating the strongest elements.{skills}\n\n\
         ## Original Prompt\n{prompt}\n\n## Panel Responses{candidates}\n\n\
         ## Instructions\nProduce your draft synthesis now.",
        skills = skill_anchor,
        prompt = prompt,
        candidates = format_panel_responses(&r1_responses),
    );
    let draft = call_judge(router, &fusion.judge, &r1_judge_prompt, params, tools).await?;
    let draft_text = &draft.text;

    info!(
        target: "reg.fusion",
        fusion_mode = "critique",
        round = 1,
        draft_len = draft_text.len(),
        "Critique round 1 complete"
    );

    // Round 2: Panel critiques the draft
    // F3 fix: skill-anchor the panel critique so it evaluates against the
    // same methodology the judge uses. Without this, panel critiques are
    // methodology-blind while the judge drafts and revises with methodology.
    let critique_prompt = format!(
        "You are a panelist reviewing a draft synthesis. Identify weaknesses, gaps, \
         contradictions, or improvements in the draft below. Be specific and constructive.{skills}\n\n\
         ## Original Prompt\n{prompt}\n\n## Draft Synthesis\n{draft_text}\n\n\
         ## Instructions\nProvide your critique. Focus on what the draft gets wrong, \
         misses, or could improve.",
        skills = skill_anchor,
    );
    let critiques = dispatch_panel(
        router,
        &critique_prompt,
        params,
        tools,
        resolve_effective_panel(prompt, fusion),
    )
    .await;

    // Round 2: Judge revises based on critiques
    let critique_sections = format_panel_responses(&critiques);
    let r2_judge_prompt = format!(
        "You are a synthesis judge (Round 2 — Final). You produced a draft synthesis. \
         The panel has reviewed it and provided critiques. Revise your synthesis, \
         incorporating the valid critiques and improving weaknesses.{skills}\n\n\
         ## Original Prompt\n{prompt}\n\n## Your Draft\n{draft_text}\n\n\
         ## Panel Critiques{critique_sections}\n\n\
         ## Instructions\nProduce your final revised synthesis.",
        skills = skill_anchor,
    );
    let result = call_judge(router, &fusion.judge, &r2_judge_prompt, params, tools).await?;
    let mut intermediate: Vec<InferenceUsage> =
        r1_responses.iter().map(|r| r.usage.clone()).collect();
    intermediate.push(draft.usage.clone());
    intermediate.extend(critiques.iter().map(|r| r.usage.clone()));
    Ok(with_aggregated_usage(result, &intermediate))
}

/// Deliberation: Multi-round with a structured judge stabilization verdict.
///
/// Each round, the judge emits a structured verdict —
/// `{"converged": true, "synthesis": "…"}` or `{"converged": false, "follow_up": "…"}`
/// — parsed by `parse_convergence_verdict`. Convergence is a *stabilization*
/// report (has the panel stopped diverging?), not a correctness claim: the
/// judge reads the responses and reports it. If `max_rounds` is reached without
/// convergence, the judge is forced to synthesize from the last round.
async fn mode_deliberation(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    fusion: &FusionConfig,
) -> Result<InferenceResult, InferenceError> {
    let skill_anchor = build_skill_anchor(&fusion.skills);
    let max_rounds = fusion.max_rounds as usize;

    // Codette-inspired: determine effective panel based on query complexity
    // and substrate pressure. Both are opt-in (default: full panel).
    let effective = resolve_effective_panel(prompt, fusion);
    let complexity = classify_complexity(prompt);
    let pressure = compute_pressure();
    if effective.len() < fusion.panel.len() {
        info!(
            target: "reg.fusion",
            fusion_mode = "deliberation",
            complexity = ?complexity,
            pressure,
            panel_size = effective.len(),
            full_panel_size = fusion.panel.len(),
            downgraded = effective.len() < fusion.panel.len(),
            "Panel size reduced (complexity/pressure routing)"
        );
    }

    // Round 1: Initial panel responses.
    let dispatch_start = std::time::Instant::now();
    let mut prior_responses = dispatch_panel(router, prompt, params, tools, effective).await;
    record_latency(dispatch_start.elapsed().as_millis() as u64);
    if prior_responses.is_empty() {
        return Err(InferenceError::Generation(
            "All panel models failed in round 1".into(),
        ));
    }

    // Codette-inspired: compute epistemic tension ξ and coherence Γ from
    // panel response embeddings. Advisory signal — the judge verdict still
    // wins. Only computed when coherence_threshold is set.
    let mut measured_coherence: Option<f64> = None;
    if let Some(_threshold) = fusion.coherence_threshold {
        let texts: Vec<String> = prior_responses.iter().map(|r| r.text.clone()).collect();
        let embeddings = fetch_embeddings(router, &texts).await;
        if !embeddings.is_empty() {
            let xi = epistemic_tension(&embeddings);
            let gamma = coherence(xi);
            measured_coherence = Some(gamma);
            info!(
                target: "reg.fusion",
                fusion_mode = "deliberation",
                round = 1,
                epistemic_tension = xi,
                coherence = gamma,
                "Measured epistemic tension (round 1)"
            );
        }
    }

    let mut intermediate: Vec<InferenceUsage> =
        prior_responses.iter().map(|r| r.usage.clone()).collect();
    let mut prior_text = format_panel_responses(&prior_responses);

    for round in 1..=max_rounds {
        // One judge call per round decides convergence AND produces the output
        // (synthesis if converged, follow-up if not) as a structured verdict.
        let json_spec = "Emit STRICT JSON only — no prose outside the JSON:
      \
                 if converged: {\"converged\": true, \"synthesis\": \"<final answer>\"}
      \
                 if not converged: {\"converged\": false, \"follow_up\": \"<one follow-up question>\"}";
        let judge_prompt = format!(
            "You are a deliberation judge (Round {round}/{max_rounds}). Below are the \
                 latest responses from the panel. Decide whether the panel has converged on \
                 a consistent answer. {json_spec}{skills}

    \
                 ## Original Prompt
    {prompt}

    ## Current Round Responses{prior_text}",
            round = round,
            max_rounds = max_rounds,
            json_spec = json_spec,
            skills = skill_anchor,
        );
        let judge_result = call_judge(router, &fusion.judge, &judge_prompt, params, tools).await?;
        intermediate.push(judge_result.usage.clone());
        let (verdict, payload) = parse_convergence_verdict(&judge_result.text);

        if verdict == ConvergenceVerdict::Converged {
            info!(
                target: "reg.fusion",
                fusion_mode = "deliberation",
                round = round,
                convergence_rounds = round,
                verdict = ConvergenceVerdict::Converged.as_str(),
                measured_coherence =? measured_coherence,
                "Deliberation converged (judge stabilization verdict)"
            );
            // Codette-inspired: if measured coherence exceeds threshold,
            // emit an advisory measured-convergence signal. The judge
            // verdict already won — this is an additional observability span.
            if let (Some(gamma), Some(threshold)) = (measured_coherence, fusion.coherence_threshold)
                && gamma > threshold
            {
                info!(
                    target: "reg.fusion",
                    fusion_mode = "deliberation",
                    round = round,
                    measured_convergence = true,
                    coherence = gamma,
                    threshold,
                    "Measured coherence exceeded threshold (advisory)"
                );
            }
            let result = InferenceResult {
                text: payload.unwrap_or_default(),
                ..judge_result
            };
            return Ok(with_aggregated_usage(result, &intermediate));
        }

        // Continue: payload is the follow-up question for the panel.
        let follow_up = payload.unwrap_or_default();
        info!(
            target: "reg.fusion",
            fusion_mode = "deliberation",
            round = round,
            verdict = ConvergenceVerdict::Continue.as_str(),
            "Deliberation continuing (judge stabilization verdict)"
        );
        let dispatch_start = std::time::Instant::now();
        prior_responses = dispatch_panel(router, &follow_up, params, tools, effective).await;
        record_latency(dispatch_start.elapsed().as_millis() as u64);
        intermediate.extend(prior_responses.iter().map(|r| r.usage.clone()));

        // Recompute ξ/Γ for the new round of responses.
        if let Some(_threshold) = fusion.coherence_threshold {
            let texts: Vec<String> = prior_responses.iter().map(|r| r.text.clone()).collect();
            let embeddings = fetch_embeddings(router, &texts).await;
            if !embeddings.is_empty() {
                let xi = epistemic_tension(&embeddings);
                let gamma = coherence(xi);
                measured_coherence = Some(gamma);
                info!(
                    target: "reg.fusion",
                    fusion_mode = "deliberation",
                    round = round + 1,
                    epistemic_tension = xi,
                    coherence = gamma,
                    "Measured epistemic tension (round {})",
                    round + 1
                );
            }
        }

        prior_text = format_panel_responses(&prior_responses);
    }

    // Max rounds reached without convergence — force final synthesis.
    let final_prompt = format!(
        "You are a deliberation judge (Final). Maximum rounds reached without convergence. \
         Synthesize a final response from the last round of panel discussion.{skills}

\
         ## Original Prompt
{prompt}

## Final Round Responses{prior_text}

\
         ## Instructions
Produce the final synthesis now.",
        skills = skill_anchor,
    );
    let result = call_judge(router, &fusion.judge, &final_prompt, params, tools).await?;
    info!(
        target: "reg.fusion",
        fusion_mode = "deliberation",
        round = max_rounds,
        convergence_rounds = max_rounds,
        verdict = "max_rounds",
        "Deliberation capped at max rounds"
    );
    Ok(with_aggregated_usage(result, &intermediate))
}

/// Plan-Implement: 2-phase — Phase 1: strategy plan, Phase 2: implementation plan.
async fn mode_plan_implement(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    fusion: &FusionConfig,
) -> Result<InferenceResult, InferenceError> {
    let skill_anchor = build_skill_anchor(&fusion.skills);

    // ── Phase 1: Strategy Plan ──────────────────────────────────────────────
    // Skill-anchor the panel (same fix as mode_critique's F3) so panelists
    // evaluate against the same methodology the judge uses, not methodology-blind.
    let phase1_plan_prompt = format!(
        "You are a strategy panelist. Given the task below, propose a high-level \
             strategy or approach. Focus on architecture, key decisions, tradeoffs, and \
             the overall plan — NOT implementation details.{skills}\n\n\
             ## Task\n{prompt}\n\n\
             ## Instructions\nPropose a strategy. Be specific about approach, not code.",
        skills = skill_anchor,
    );

    let phase1_responses = dispatch_panel(
        router,
        &phase1_plan_prompt,
        params,
        tools,
        resolve_effective_panel(prompt, fusion),
    )
    .await;
    if phase1_responses.is_empty() {
        return Err(InferenceError::Generation(
            "All panel models failed in strategy phase".into(),
        ));
    }

    let p1_judge_prompt = format!(
        "You are a strategy synthesis judge (Phase 1: Plan). Below are strategy \
             proposals from the panel. Synthesize a unified strategy plan incorporating \
             the best approaches. Resolve contradictions. This is the STRATEGY only — \
             no implementation details.{skills}\n\n\
             ## Original Task\n{prompt}\n\n## Strategy Proposals{candidates}\n\n\
             ## Instructions\nProduce the unified strategy plan.",
        skills = skill_anchor,
        candidates = format_panel_responses(&phase1_responses),
    );
    let strategy = call_judge(router, &fusion.judge, &p1_judge_prompt, params, tools).await?;
    let strategy_text = &strategy.text;

    info!(
        target: "reg.fusion",
        fusion_mode = "pi",
        phase = 1,
        strategy_len = strategy_text.len(),
        "P-I Phase 1 complete — strategy synthesized"
    );

    // ── Phase 2: Implementation Plan ────────────────────────────────────────
    // Skill-anchor the panel (same fix as mode_critique's F3).
    let phase2_impl_prompt = format!(
        "You are an implementation panelist. Below is a unified strategy plan. \
         Given this strategy, propose concrete implementation steps, file changes, \
         code structure, tests, and sequencing.{skills}\n\n\
         ## Original Task\n{prompt}\n## Strategy Plan\n{strategy_text}\n\n\
         ## Instructions\nPropose implementation details. Be specific about files, \
         functions, tests, and the order of work.",
        skills = skill_anchor,
    );

    let phase2_responses = dispatch_panel(
        router,
        &phase2_impl_prompt,
        params,
        tools,
        resolve_effective_panel(prompt, fusion),
    )
    .await;

    // D2 fix: if all panel models failed in phase 2, return an error rather
    // than asking the judge to hallucinate implementation details from nothing.
    if phase2_responses.is_empty() {
        return Err(InferenceError::Generation(
            "All panel models failed in implementation phase — cannot synthesize".into(),
        ));
    }

    let p2_candidates = format_panel_responses(&phase2_responses);

    let p2_judge_prompt = format!(
        "You are an implementation synthesis judge (Phase 2: Implement). Below is \
         the strategy plan and the panel's implementation proposals. Synthesize a \
         unified implementation plan with concrete steps, file changes, code \
         structure, tests, and sequencing.{skills}\n\n\
         ## Original Task\n{prompt}\n\n## Strategy Plan\n{strategy_text}\n\n\
         ## Implementation Proposals{p2_candidates}\n\n\
         ## Instructions\nProduce the unified implementation plan. Be specific. \
         Include: files to create/modify, key functions/types, test strategy, \
         and execution order.",
        skills = skill_anchor,
    );
    let result = call_judge(router, &fusion.judge, &p2_judge_prompt, params, tools).await?;
    let mut intermediate: Vec<InferenceUsage> =
        phase1_responses.iter().map(|r| r.usage.clone()).collect();
    intermediate.push(strategy.usage.clone());
    intermediate.extend(phase2_responses.iter().map(|r| r.usage.clone()));
    Ok(with_aggregated_usage(result, &intermediate))
}

// ── Public Entry Point ───────────────────────────────────────────────────────

/// Orchestrate provider-agnostic fusion deliberation.
///
/// Dispatches to the panel in parallel, then routes to the configured
/// fusion mode for judge behavior.
///
/// expect: "Fusion orchestrates multi-model deliberation provider-agnostically"
/// \[P9\] Motivating: Homeostatic Self-Regulation — hKask-side fusion orchestration
/// pre:  fusion.panel is non-empty, fusion.judge is valid
/// post: returns judge output per the configured mode
#[must_use = "result must be used"]
pub async fn orchestrate(
    router: &dyn InferencePort,
    prompt: &str,
    params: &LLMParameters,
    tools: Option<&[ChatToolDefinition]>,
    fusion: &FusionConfig,
) -> Result<InferenceResult, InferenceError> {
    info!(
        target: "reg.fusion",
        fusion_mode = %fusion.mode.as_str(),
        fusion_judge = %fusion.judge,
        panel_count = fusion.panel.len(),
        skills = fusion.skills.len(),
        "Fusion orchestration starting"
    );

    // Algorithmic judge — deterministic JSON merge, no LLM call.
    // The judge IS the strategy: "algo" means merge panel responses
    // algorithmically rather than via an LLM judge call.
    // Case-insensitive to tolerate YAML typos (e.g., "Algo", "ALGO").
    if fusion.judge.to_lowercase() == ALGO_JUDGE {
        let effective = resolve_effective_panel(prompt, fusion);
        let responses = dispatch_panel(router, prompt, params, tools, effective).await;
        if responses.is_empty() {
            return Err(InferenceError::Generation("All panel models failed".into()));
        }
        let result = match fusion.algo_method {
            AlgoMethod::Merge => algo_merge(&responses),
            AlgoMethod::Vote => algo_vote(&responses),
        };
        info!(
            target: "reg.fusion",
            fusion_judge = "algo",
            algo_method = fusion.algo_method.as_str(),
            panel_count = responses.len(),
            "Algo judge complete"
        );
        return Ok(result);
    }

    match fusion.mode {
        FusionMode::BestOfN => mode_best_of_n(router, prompt, params, tools, fusion).await,
        FusionMode::Synthesis => mode_synthesis(router, prompt, params, tools, fusion).await,
        FusionMode::Critique => mode_critique(router, prompt, params, tools, fusion).await,
        FusionMode::Deliberation => mode_deliberation(router, prompt, params, tools, fusion).await,
        FusionMode::PlanImplement => {
            mode_plan_implement(router, prompt, params, tools, fusion).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConvergenceVerdict, jaccard, merge_json_values, parse_convergence_verdict, vote_json_values,
    };
    use crate::config::{AlgoMethod, FusionConfig, FusionMode};
    use hkask_types::fusion::NonEmptyVec;
    use hkask_types::template::LLMParameters;
    use hkask_types::{
        ChatToolDefinition, InferenceError, InferencePort, InferenceResult, InferenceUsage,
    };
    use serde_json::json;

    /// A2: primitive arrays dedup by value — [1,1,1] collapses to [1].
    #[test]
    fn merge_dedups_primitive_arrays() {
        let a = json!([1, 2, 3]);
        let b = json!([1, 3, 5]);
        let merged = merge_json_values(&a, &b);
        assert_eq!(merged, json!([1, 2, 3, 5]));
    }

    /// A2: string dedup is case-insensitive and trim-tolerant (one normalization).
    #[test]
    fn merge_dedups_strings_case_insensitive_and_trimmed() {
        let a = json!(["foo", "bar"]);
        let b = json!(["FOO", " bar "]);
        let merged = merge_json_values(&a, &b);
        assert_eq!(merged, json!(["foo", "bar"]));
    }

    /// A2: objects/arrays inside arrays are kept (structural differences matter).
    #[test]
    fn merge_keeps_distinct_objects_in_arrays() {
        let a = json!([{"k": 1}]);
        let b = json!([{"k": 1}, {"k": 2}]);
        let merged = merge_json_values(&a, &b);
        assert_eq!(merged, json!([{"k": 1}, {"k": 1}, {"k": 2}]));
    }

    /// String conflict annotation preserved for divergent values.
    #[test]
    fn merge_annotates_divergent_strings() {
        let a = json!("left");
        let b = json!("right");
        let merged = merge_json_values(&a, &b);
        assert_eq!(merged, json!("[A:left B:right]"));
    }

    /// Equal strings (case/trim-insensitive) collapse to A.
    #[test]
    fn merge_equal_strings_collapse() {
        let a = json!("foo");
        let b = json!("FOO");
        let merged = merge_json_values(&a, &b);
        assert_eq!(merged, json!("foo"));
    }

    // ── T1: Deliberation convergence (structured judge verdict) ─────────────────
    // `jaccard` is retained: best-of-n `identify_pick` uses it to match a verbatim
    // judge pick back to its source response.

    /// Identical texts → Jaccard 1.0.
    #[test]
    fn jaccard_identical_texts_score_one() {
        assert_eq!(jaccard("the quick brown fox", "the quick brown fox"), 1.0);
    }

    /// Disjoint vocabularies → Jaccard 0.0.
    #[test]
    fn jaccard_disjoint_texts_score_zero() {
        assert_eq!(jaccard("alpha beta", "gamma delta"), 0.0);
    }

    /// Partial overlap → 2 shared / 4 union = 0.5.
    #[test]
    fn jaccard_partial_overlap() {
        // {apple, banana} ∩ {apple, cherry} = {apple}; union = {apple, banana, cherry} = 3
        let s = jaccard("apple banana", "apple cherry");
        assert!((s - 1.0 / 3.0).abs() < 1e-9);
    }

    /// Case-insensitive and whitespace-tolerant.
    #[test]
    fn jaccard_case_and_whitespace_insensitive() {
        assert_eq!(jaccard("  FOO  bar ", "foo BAR"), 1.0);
    }

    /// Both empty → vacuously identical (1.0); one empty → 0.0.
    #[test]
    fn jaccard_empty_edge_cases() {
        assert_eq!(jaccard("", ""), 1.0);
        assert_eq!(jaccard("", "words here"), 0.0);
        assert_eq!(jaccard("words here", ""), 0.0);
    }

    /// `{"converged": true, "synthesis": "…"}` → Converged with that synthesis.
    #[test]
    fn parse_convergence_verdict_converged_with_synthesis() {
        let (v, p) = parse_convergence_verdict(r#"{"converged": true, "synthesis": "Paris"}"#);
        assert_eq!(v, ConvergenceVerdict::Converged);
        assert_eq!(p.as_deref(), Some("Paris"));
    }

    /// `{"converged": false, "follow_up": "…"}` → Continue with that follow-up.
    #[test]
    fn parse_convergence_verdict_continue_with_follow_up() {
        let (v, p) = parse_convergence_verdict(r#"{"converged": false, "follow_up": "Why?"}"#);
        assert_eq!(v, ConvergenceVerdict::Continue);
        assert_eq!(p.as_deref(), Some("Why?"));
    }

    /// `converged: true` with no `synthesis` field → fall back to the raw text.
    #[test]
    fn parse_convergence_verdict_converged_no_synthesis_uses_raw() {
        let (v, p) = parse_convergence_verdict(r#"{"converged": true}"#);
        assert_eq!(v, ConvergenceVerdict::Converged);
        assert_eq!(p.as_deref(), Some(r#"{"converged": true}"#));
    }

    /// Markdown-fenced JSON is parsed (`parse_json_lenient` tolerates fences).
    #[test]
    fn parse_convergence_verdict_tolerates_markdown_fence() {
        let text = "```json\n{\"converged\": true, \"synthesis\": \"42\"}\n```";
        let (v, p) = parse_convergence_verdict(text);
        assert_eq!(v, ConvergenceVerdict::Converged);
        assert_eq!(p.as_deref(), Some("42"));
    }

    /// Non-JSON response → treated as a final synthesis (former FOLLOW_UP: fallback:
    /// a response not declaring a follow-up is the final answer).
    #[test]
    fn parse_convergence_verdict_non_json_is_final_synthesis() {
        let (v, p) = parse_convergence_verdict("The answer is Paris.");
        assert_eq!(v, ConvergenceVerdict::Converged);
        assert_eq!(p.as_deref(), Some("The answer is Paris."));
    }

    // ── T3: Position-bias mitigation (best-of-n swap-revote) ─────────────────────

    fn resp(name: &str, text: &str) -> super::PanelResponse {
        super::PanelResponse {
            model_name: name.into(),
            text: text.into(),
            usage: hkask_types::InferenceUsage::default(),
            tool_calls: Vec::new(),
            finish_reason: "stop".to_string(),
        }
    }

    /// Ordered formatter places responses in the given display order, labeled by slot.
    #[test]
    fn ordered_format_uses_display_order() {
        let rs = [
            resp("alpha", "AAA"),
            resp("beta", "BBB"),
            resp("gamma", "GGG"),
        ];
        let out = super::format_panel_responses_in_order(&rs, &[2, 0, 1]);
        // Slot 1 → rs[2] (gamma), slot 2 → rs[0] (alpha), slot 3 → rs[1] (beta).
        let gamma_pos = out.find("Panelist 1: gamma").unwrap();
        let alpha_pos = out.find("Panelist 2: alpha").unwrap();
        let beta_pos = out.find("Panelist 3: beta").unwrap();
        assert!(gamma_pos < alpha_pos && alpha_pos < beta_pos);
    }

    /// `identify_pick` matches a verbatim judge output back to its source response.
    #[test]
    fn identify_pick_matches_verbatim_output() {
        let rs = [
            resp("alpha", "the quick brown fox"),
            resp("beta", "a lazy dog sleeps"),
            resp("gamma", "midnight in paris"),
        ];
        assert_eq!(super::identify_pick("the quick brown fox", &rs), 0);
        assert_eq!(super::identify_pick("a lazy dog sleeps", &rs), 1);
        assert_eq!(super::identify_pick("midnight in paris", &rs), 2);
    }

    // ── T4: Algo vote/tally merge ────────────────────────────────────────────────

    /// Majority scalar vote: the value appearing in ≥ majority of panelists wins.
    #[test]
    fn vote_scalar_majority_wins() {
        let vs = vec![json!("red"), json!("blue"), json!("red")];
        assert_eq!(vote_json_values(&vs), json!("red"));
    }

    /// No majority → first value retained (deterministic).
    #[test]
    fn vote_no_majority_keeps_first() {
        let vs = vec![json!("red"), json!("blue"), json!("green")];
        assert_eq!(vote_json_values(&vs), json!("red"));
    }

    /// Object per-key recursive vote.
    #[test]
    fn vote_object_per_key_majority() {
        let vs = vec![
            json!({"color": "red", "size": "L"}),
            json!({"color": "blue", "size": "L"}),
            json!({"color": "red", "size": "S"}),
        ];
        // color: red (2/3 majority); size: L (2/3 majority).
        assert_eq!(vote_json_values(&vs), json!({"color": "red", "size": "L"}));
    }

    /// Array majority: items kept only if a majority of panelists include them.
    #[test]
    fn vote_array_keeps_majority_items() {
        let vs = vec![
            json!(["a", "b", "c"]),
            json!(["a", "b", "d"]),
            json!(["a", "c", "e"]),
        ];
        // threshold = 3/2+1 = 2. "a" in 3, "b" in 2, "c" in 2, "d" in 1, "e" in 1.
        let arr = vote_json_values(&vs);
        let arr = arr.as_array().unwrap();
        assert!(arr.contains(&json!("a")));
        assert!(arr.contains(&json!("b")));
        assert!(arr.contains(&json!("c")));
        assert!(!arr.contains(&json!("d")));
        assert!(!arr.contains(&json!("e")));
    }

    /// Empty input → null.
    #[test]
    fn vote_empty_is_null() {
        assert_eq!(vote_json_values(&[]), serde_json::Value::Null);
    }

    /// `algo:vote` with 2 panelists falls back to merge (no majority possible).
    #[test]
    fn algo_vote_with_two_panelists_falls_back_to_merge() {
        let rs = [resp("a", r#"{"k":"red"}"#), resp("b", r#"{"k":"blue"}"#)];
        let voted = super::algo_vote(&rs);
        // merge annotates divergent scalars as [A:... B:...]; vote would just pick "red".
        assert!(
            voted.text.contains("[A:"),
            "expected merge divergence annotation, got {}",
            voted.text
        );
    }

    /// Array majority canonicalizes object key order — two objects equal modulo
    /// key insertion order count as the same item (dedup to one).
    #[test]
    fn vote_array_majority_canonicalizes_object_key_order() {
        let vs = vec![
            json!([{"a": 1, "b": 2}]),
            json!([{"b": 2, "a": 1}]),
            json!([{"a": 1, "b": 2}]),
        ];
        let out = vote_json_values(&vs);
        let arr = out.as_array().expect("expected an array");
        assert_eq!(
            arr.len(),
            1,
            "key-order-equivalent objects must dedup to one item"
        );
    }

    // ── F4: Position-bias measurement harness ────────────────────────────────────
    //
    // `mode_best_of_n` runs swap-revote (two judge calls in reversed display order)
    // to detect position bias. These are INTEGRATION tests of the full
    // `mode_best_of_n` return path with mock judges — they prove the pipeline
    // runs end-to-end and the `join!` parallelization doesn't break the return.
    // They do NOT directly observe the agree/disagree verdict (that's a
    // `tracing::info!` side-effect); the bias-detection logic itself
    // (`identify_pick` + `==` comparison) is covered by the `identify_pick`
    // unit test above plus the trivial `usize ==` comparison. A live judge run
    // through this harness pattern reveals whether swap-revote is justified.

    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;

    /// A combined mock that plays both panelists and judge. Panel dispatch calls
    /// `generate_with_model` with a panel model name — the mock returns that
    /// panelist's canned text. Judge calls use the judge model name — the mock
    /// applies the configured `JudgeBehavior` to the prompt.
    struct BiasHarness {
        panel_texts: HashMap<String, String>,
        judge_model: String,
        behavior: JudgeBehavior,
    }

    enum JudgeBehavior {
        /// Bias-free: always return the same fixed text.
        FixedPick(String),
        /// Position-biased: echo the first displayed candidate's text body.
        FirstDisplayed,
    }

    fn make_result(text: String, model: &str) -> InferenceResult {
        InferenceResult {
            text,
            model: model.to_string(),
            usage: InferenceUsage::default(),
            finish_reason: "stop".to_string(),
            token_probabilities: None,
            tool_calls: Vec::new(),
            reasoning: None,
        }
    }

    impl InferencePort for BiasHarness {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            // Unused by mode_best_of_n (it always goes through generate_with_model).
            let model = self.judge_model.clone();
            Box::pin(async move { Ok(make_result(String::new(), &model)) })
        }

        fn generate_with_model(
            &self,
            prompt: &str,
            _parameters: &LLMParameters,
            model_override: Option<&str>,
            _tools: Option<&[ChatToolDefinition]>,
        ) -> Pin<Box<dyn Future<Output = Result<InferenceResult, InferenceError>> + Send + '_>>
        {
            let model = model_override.unwrap_or("");
            // Panel dispatch: return the panelist's canned text.
            if let Some(text) = self.panel_texts.get(model) {
                let out = make_result(text.clone(), model);
                return Box::pin(async move { Ok(out) });
            }
            // Judge call: apply the configured behavior.
            let picked = match &self.behavior {
                JudgeBehavior::FixedPick(text) => text.clone(),
                JudgeBehavior::FirstDisplayed => prompt
                    .split("### Panelist 1:")
                    .nth(1)
                    .and_then(|rest| rest.split("### Panelist 2:").next())
                    .map(|s| {
                        let after_name = s.split_once('\n').map(|(_, body)| body).unwrap_or(s);
                        after_name.trim().to_string()
                    })
                    .unwrap_or_default(),
            };
            let out = make_result(picked, model);
            Box::pin(async move { Ok(out) })
        }
    }

    /// Integration test: a fixed-pick judge produces a deterministic output
    /// through the full `mode_best_of_n` pipeline (panel dispatch + `join!`
    /// swap-revote + return path). The bias-detection logic itself is covered
    /// by the `identify_pick` unit test; this test proves the wiring.
    #[tokio::test]
    async fn best_of_n_bias_harness_fixed_pick_agrees() {
        let panel_texts: HashMap<String, String> = [
            ("alpha".into(), "the quick brown fox".into()),
            ("beta".into(), "a lazy dog sleeps".into()),
            ("gamma".into(), "midnight in paris".into()),
        ]
        .into_iter()
        .collect();
        // Fixed pick: always returns panel[1] ("a lazy dog sleeps").
        let judge = BiasHarness {
            panel_texts,
            judge_model: "fixed-pick-judge".into(),
            behavior: JudgeBehavior::FixedPick("a lazy dog sleeps".into()),
        };
        let params = LLMParameters::default();
        let fusion = FusionConfig {
            judge: "fixed-pick-judge".into(),
            panel: NonEmptyVec::new("alpha".into(), vec!["beta".into(), "gamma".into()]),
            mode: FusionMode::BestOfN,
            skills: vec![],
            max_rounds: 1,
            algo_method: AlgoMethod::Merge,
            coherence_threshold: None,
            panel_sizing_enabled: false,
            pressure_adaptive_enabled: false,
        };
        let result = super::mode_best_of_n(&judge, "prompt", &params, None, &fusion)
            .await
            .expect("best-of-n must succeed");
        // Both swap-revote judge calls returned the same fixed text, so
        // identify_pick resolved to the same index — agreement (no bias).
        assert_eq!(
            result.text, "a lazy dog sleeps",
            "fixed-pick judge returns panel[1] verbatim"
        );
    }

    /// Integration test: a first-displayed judge (position-biased) produces the
    /// dispatch-order pick through the full pipeline. This test exercises the
    /// same return path as the fixed-pick test with a different judge behavior;
    /// it does NOT directly assert on the agree/disagree verdict (which is a
    /// `tracing::info!` side-effect, not a return value). The detection logic
    /// (`identify_pick` + `==`) is covered by unit tests above.
    #[tokio::test]
    async fn best_of_n_bias_harness_first_displayed_disagrees() {
        let panel_texts: HashMap<String, String> = [
            ("alpha".into(), "the quick brown fox".into()),
            ("beta".into(), "a lazy dog sleeps".into()),
            ("gamma".into(), "midnight in paris".into()),
        ]
        .into_iter()
        .collect();
        let judge = BiasHarness {
            panel_texts,
            judge_model: "first-displayed-judge".into(),
            behavior: JudgeBehavior::FirstDisplayed,
        };
        let params = LLMParameters::default();
        let fusion = FusionConfig {
            judge: "first-displayed-judge".into(),
            panel: NonEmptyVec::new("alpha".into(), vec!["beta".into(), "gamma".into()]),
            mode: FusionMode::BestOfN,
            skills: vec![],
            max_rounds: 1,
            algo_method: AlgoMethod::Merge,
            coherence_threshold: None,
            panel_sizing_enabled: false,
            pressure_adaptive_enabled: false,
        };
        // The biased judge picks panel[0] in order_a (dispatch order) and
        // panel[2] in order_b (reversed). identify_pick resolves each to a
        // different index — swap-revote disagrees. mode_best_of_n returns
        // result_a (the dispatch-order pick), so the text is panel[0].text.
        let result = super::mode_best_of_n(&judge, "prompt", &params, None, &fusion)
            .await
            .expect("best-of-n must succeed");
        assert_eq!(
            result.text, "the quick brown fox",
            "biased judge returns the first-displayed pick (dispatch order = panel[0])"
        );
    }

    // ── Codette-inspired: epistemic tension & coherence tests ─────────────

    #[test]
    fn epistemic_tension_zero_for_single_embedding() {
        let embeddings = vec![vec![1.0, 2.0, 3.0]];
        let xi = super::epistemic_tension(&embeddings);
        assert_eq!(xi, 0.0, "single embedding has zero tension");
    }

    #[test]
    fn epistemic_tension_zero_for_identical_embeddings() {
        let embeddings = vec![
            vec![1.0, 2.0, 3.0],
            vec![1.0, 2.0, 3.0],
            vec![1.0, 2.0, 3.0],
        ];
        let xi = super::epistemic_tension(&embeddings);
        assert_eq!(xi, 0.0, "identical embeddings have zero tension");
    }

    #[test]
    fn epistemic_tension_positive_for_divergent_embeddings() {
        let embeddings = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let xi = super::epistemic_tension(&embeddings);
        assert!(xi > 0.0, "divergent embeddings have positive tension");
        // Centroid = (0.5, 0.5). Distance² from each = 0.25 + 0.25 = 0.5.
        // Mean = (0.5 + 0.5) / 2 = 0.5
        assert!((xi - 0.5).abs() < 1e-6, "expected 0.5, got {}", xi);
    }

    #[test]
    fn coherence_decreases_as_tension_increases() {
        let gamma_low = super::coherence(0.0);
        let gamma_mid = super::coherence(1.0);
        let gamma_high = super::coherence(10.0);
        assert_eq!(gamma_low, 1.0, "zero tension → perfect coherence");
        assert!(gamma_mid > gamma_high, "higher tension → lower coherence");
        assert!(gamma_mid < gamma_low, "any tension < perfect coherence");
    }

    // ── Codette-inspired: query complexity router tests ───────────────────

    #[test]
    fn classify_simple_factual_query() {
        let prompt = "What is 2+2?";
        assert_eq!(
            super::classify_complexity(prompt),
            super::QueryComplexity::Simple,
            "short factual question should be Simple"
        );
    }

    #[test]
    fn classify_complex_ethical_query() {
        let prompt = "Analyze the ethical tradeoffs of AI triage in healthcare, considering stakeholder consequences and moral implications for vulnerable populations.";
        assert_eq!(
            super::classify_complexity(prompt),
            super::QueryComplexity::Complex,
            "multi-domain ethical query should be Complex"
        );
    }

    #[test]
    fn classify_medium_code_query() {
        let prompt = "Review this function:\n```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```\nIs it correct?";
        assert_eq!(
            super::classify_complexity(prompt),
            super::QueryComplexity::Medium,
            "code review query should be at least Medium"
        );
    }

    #[test]
    fn effective_panel_returns_full_panel_when_disabled() {
        let panel = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = super::effective_panel(&panel, super::QueryComplexity::Simple, false);
        assert_eq!(result.len(), 3, "disabled panel sizing returns full panel");
    }

    #[test]
    fn effective_panel_reduces_for_simple_when_enabled() {
        let panel = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = super::effective_panel(&panel, super::QueryComplexity::Simple, true);
        assert_eq!(result.len(), 1, "Simple query with sizing → 1 model");
    }

    #[test]
    fn effective_panel_reduces_to_2_for_medium_when_enabled() {
        let panel = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = super::effective_panel(&panel, super::QueryComplexity::Medium, true);
        assert_eq!(result.len(), 2, "Medium query with sizing → 2 models");
    }

    #[test]
    fn effective_panel_full_for_complex_even_when_enabled() {
        let panel = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = super::effective_panel(&panel, super::QueryComplexity::Complex, true);
        assert_eq!(result.len(), 3, "Complex query → full panel");
    }

    // ── Codette-inspired: pressure tracking tests ─────────────────────────

    #[test]
    fn compute_pressure_zero_when_no_latency_recorded() {
        // Reset the static to 0 by storing 0.
        super::ROLLING_LATENCY_MS.store(0, std::sync::atomic::Ordering::Relaxed);
        let p = super::compute_pressure();
        assert_eq!(p, 0.0, "no latency recorded → zero pressure");
    }

    #[test]
    fn compute_pressure_scales_with_latency() {
        super::ROLLING_LATENCY_MS.store(5000, std::sync::atomic::Ordering::Relaxed);
        let p = super::compute_pressure();
        // 5000ms → (5000 - 2000) / 6000 = 0.5
        assert!((p - 0.5).abs() < 1e-6, "5000ms → pressure 0.5, got {}", p);
    }

    #[test]
    fn compute_pressure_clamps_at_one_for_extreme_latency() {
        super::ROLLING_LATENCY_MS.store(20000, std::sync::atomic::Ordering::Relaxed);
        let p = super::compute_pressure();
        assert_eq!(p, 1.0, "20000ms → pressure clamped to 1.0");
    }

    #[test]
    fn pressure_adjusted_panel_returns_full_when_disabled() {
        let panel = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = super::pressure_adjusted_panel(&panel, false);
        assert_eq!(result.len(), 3, "disabled → full panel");
    }
}
