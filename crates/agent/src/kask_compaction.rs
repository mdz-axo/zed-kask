//! Kask-owned compaction planning (D8): shrink a compaction request
//! deterministically, then plan how the LLM summarizes what remains.
//!
//! The pipeline, in order:
//!
//! 1. **Deterministic pre-shrink** (§2) on the summarizer's *request copy*:
//!    run-length collapse of repeated lines, then head+tail windowing of any
//!    tool-result text over a per-result cap. No tool-name, JSON, error, or
//!    positional exemptions — the stored thread history is never modified,
//!    and every elision carries an in-band marker naming what was removed.
//!    This is the stage that makes the rest cheap: coding threads are
//!    dominated by large code-reading tool results that every previous
//!    deterministic policy exempted.
//! 2. **Budget calibration** (§3): the compaction model's input capacity,
//!    translated to a per-thread byte budget via the provider's own last
//!    reported token count.
//! 3. **Planning** (§4–§5): a history that fits keeps the ratified two-half
//!    split; an over-budget history packs into balanced segments; an
//!    indivisible history is elided as a last resort.
//!
//! Pure request algebra — no `Thread`, no `App`, no model — so the pipeline
//! is testable where the behavioral contract lives, in this crate. The
//! streaming, per-call usage accounting, cancellation, and summary insertion
//! stay native in `thread.rs` (`stream_compaction`), which keeps the
//! upstream-shaped lifecycle in the upstream file and the fork's planning
//! policy in this kask-owned one.
//!
//! The full specification — stages, policies, invariants, diagrams, and the
//! reference models the pipeline is based on — lives in
//! `kask/docs/architecture/compaction-pipeline-spec.md`.

use std::ops::Range;
use std::sync::Arc;

use agent_settings::COMPACTION_PROMPT;
use anyhow::{Context, Result};
use collections::HashSet;
use language_model::{
    LanguageModelRequest, LanguageModelRequestMessage, LanguageModelToolResultContent,
    MessageContent, Role,
};

// ─────────────────────────────── Public API ───────────────────────────────

/// How a compaction request is summarized.
pub(crate) enum CompactionPlan {
    /// Summarize this request in a single call. The request is the base
    /// request when the history fits (or no usable budget is known), or an
    /// elided copy when an indivisible history exceeds the budget.
    Single(LanguageModelRequest),
    /// Summarize each segment concurrently, then merge the summaries
    /// chronologically. Always two or more segments, each planned to fit
    /// the input budget.
    Segments(Vec<LanguageModelRequest>),
}

/// What the deterministic pre-shrink did, for observability. A near-zero
/// reduction on a large history is a policy smell the operator can see in
/// the log line `stream_compaction` emits. Per-pass byte attribution turns
/// each pass into an independently falsifiable claim: a pass that removes
/// no measurable bytes on real coding threads is inert, and the log says so.
pub(crate) struct ShrinkStats {
    /// Model-visible history bytes before the pre-shrink.
    pub(crate) original_bytes: usize,
    /// Model-visible history bytes after the pre-shrink.
    pub(crate) shrunk_bytes: usize,
    /// Tool-result texts examined by the shrink passes.
    pub(crate) result_texts: usize,
    /// Tool-result texts windowed head+tail to the per-result cap.
    pub(crate) windowed_results: usize,
    /// Bytes removed by head+tail windowing (pass 2).
    pub(crate) windowed_bytes: usize,
    /// Runs of identical adjacent lines collapsed to one line + count.
    pub(crate) collapsed_runs: usize,
    /// Bytes removed by run-length collapse (pass 1).
    pub(crate) collapsed_bytes: usize,
    /// The active per-result window cap (the count-aware cap's value for
    /// this history), for observability.
    pub(crate) result_cap_bytes: usize,
}

impl ShrinkStats {
    fn zero(total: usize) -> Self {
        Self {
            original_bytes: total,
            shrunk_bytes: total,
            result_texts: 0,
            windowed_results: 0,
            windowed_bytes: 0,
            collapsed_runs: 0,
            collapsed_bytes: 0,
            result_cap_bytes: 0,
        }
    }

    pub(crate) fn reduction_pct(&self) -> f64 {
        if self.original_bytes == 0 {
            0.0
        } else {
            (1.0 - (self.shrunk_bytes as f64 / self.original_bytes as f64)) * 100.0
        }
    }
}

/// The result of planning one compaction: how to summarize, and what the
/// deterministic stage did first.
pub(crate) struct PlannedCompaction {
    pub(crate) plan: CompactionPlan,
    pub(crate) shrink: ShrinkStats,
}

/// Plan how to summarize `request`'s history under `input_capacity_tokens`
/// (the compaction model's input capacity, output allowance already
/// reserved), calibrated by `last_input_tokens` (the last completed
/// request's reported input tokens, cache reads included — the provider's
/// own count of roughly this same history). The deterministic pre-shrink
/// runs first on the request copy; the plan is then made against the
/// shrunken layout. A capacity below
/// [`crate::thread::MIN_COMPACTION_CONTEXT_WINDOW`] is not a usable
/// budget: no shrink runs and the legacy two-half split applies.
pub(crate) fn plan_compaction(
    request: &LanguageModelRequest,
    input_capacity_tokens: u64,
    last_input_tokens: Option<u64>,
) -> Result<PlannedCompaction> {
    let prefix_len = request
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let end = request.messages.len().saturating_sub(1); // final summarization instruction
    if prefix_len >= end {
        return Ok(PlannedCompaction {
            plan: CompactionPlan::Single(request.clone()),
            shrink: ShrinkStats::zero(0),
        });
    }
    let instruction = request
        .messages
        .last()
        .context("Missing compaction instruction")?;
    let history = &request.messages[prefix_len..end];
    let original_total = history_bytes(history)?;
    let layout = history_layout(history)?;

    let Some(budget) = (input_capacity_tokens >= crate::thread::MIN_COMPACTION_CONTEXT_WINDOW)
        .then(|| {
            usable_history_budget(
                request,
                prefix_len,
                instruction,
                input_capacity_tokens,
                calibrated_bytes_per_token(original_total, last_input_tokens),
            )
        })
        .transpose()?
        .flatten()
    else {
        // No usable budget: legacy behavior — no shrink, no fit guarantee.
        let plan = two_half_plan(request, &layout, prefix_len, instruction)
            .map(|(earlier, later)| CompactionPlan::Segments(vec![earlier, later]))
            .unwrap_or_else(|| CompactionPlan::Single(request.clone()));
        return Ok(PlannedCompaction {
            plan,
            shrink: ShrinkStats::zero(original_total),
        });
    };

    let (mut shrunk, mut shrink) = pre_shrink(request, prefix_len, end, budget);
    let shrunk_total = shrink.shrunk_bytes;
    let layout = history_layout(&shrunk.messages[prefix_len..end])?;
    let plan = if layout.cuts.is_empty() && shrunk_total > budget {
        // Indivisible even after windowing (e.g. one prose message larger
        // than the budget): last resort, elide the whole history copy.
        elide_history_to_budget(&mut shrunk.messages[prefix_len..end], budget)?;
        shrink.shrunk_bytes = history_bytes(&shrunk.messages[prefix_len..end])?;
        CompactionPlan::Single(shrunk)
    } else if shrunk_total > budget {
        CompactionPlan::Segments(segment_requests(
            &shrunk,
            &layout,
            prefix_len,
            instruction,
            budget,
        )?)
    } else {
        // Fits after the shrink: the ratified two-half split for splittable
        // histories, single pass for the rest.
        two_half_plan(&shrunk, &layout, prefix_len, instruction)
            .map(|(earlier, later)| CompactionPlan::Segments(vec![earlier, later]))
            .unwrap_or_else(|| CompactionPlan::Single(shrunk))
    };
    Ok(PlannedCompaction { plan, shrink })
}

/// Build the final merge request from completed segment summaries, in
/// chronological order. Consumes the base request, keeping its scalar
/// fields and system prefix; the merge instruction preserves later
/// corrections and unresolved conflicts.
pub(crate) fn merge_request(
    mut base: LanguageModelRequest,
    summaries: &[String],
) -> LanguageModelRequest {
    let prefix_len = base
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    base.messages.truncate(prefix_len);
    let mut text = String::from(
        "Merge these chronological summaries. Preserve later corrections and unresolved conflicts.",
    );
    for (index, summary) in summaries.iter().enumerate() {
        text.push_str(&format!("\n\nSegment {}:\n{}", index + 1, summary));
    }
    base.messages.push(context_message(text));
    base.messages
        .push(context_message(COMPACTION_PROMPT.into()));
    base
}

// ─────────────────────── §1 Deterministic pre-shrink ──────────────────────
//
// Two passes over tool-result text, in order, on the summarizer's request
// copy only:
//
//   1. Run-length collapse: 3+ identical adjacent non-blank lines become one
//      line plus an exact count marker (run-length encoding — the honest
//      deterministic transform for repetition).
//   2. Head+tail windowing: any remaining text over the per-result cap is
//      cut to head+tail with an in-band marker naming the elided byte count
//      (bounded-display truncation — the honest deterministic transform for
//      bulk: it cannot corrupt structure the way mid-content ellipsis can).
//
// No exemptions: tool name, JSON shape, error state, and position in the
// thread do not change the policy. The stored thread history is never
// modified, and each marker states where the full text remains.

/// The per-result window cap is a fraction of the history budget so no
/// single tool result can dominate a summarization request.
const RESULT_WINDOW_BUDGET_DIVISOR: usize = 32;
const MIN_RESULT_WINDOW_BYTES: usize = 8 * 1024;
const MAX_RESULT_WINDOW_BYTES: usize = 64 * 1024;

/// Runs of fewer than this many identical adjacent lines are kept verbatim.
const REPEAT_RUN_THRESHOLD: usize = 3;

/// What a dry-run census pass reports for one request: the bytes each
/// deterministic pass would remove, with no mutation. The census is the
/// measurement gate that decides whether a pass is built at all — a pass
/// whose dry-run removes no measurable bytes on real coding threads is
/// inert, and building it would repeat the retired condenser's failure
/// (1.8% on the incident thread). Test-only: its callers are the census
/// tests, and gating it here keeps the production lib free of an instrument
/// nothing calls.
#[cfg(test)]
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Census {
    /// Tool-result texts scanned.
    pub(crate) result_texts: usize,
    /// Tool-result text bytes scanned (raw, not JSON-encoded).
    pub(crate) scanned_bytes: usize,
    /// Runs of 3+ identical adjacent non-blank lines that exact RLE would
    /// collapse (the existing pass 1).
    pub(crate) repeat_runs: usize,
    /// Bytes exact RLE would remove.
    pub(crate) repeat_run_bytes: usize,
    /// Runs of 3+ adjacent lines identical except a single trailing numeric
    /// field that the parameterized-run collapse would collapse (C4).
    pub(crate) parameterized_runs: usize,
    /// Bytes the parameterized-run collapse would remove (intermediate lines
    /// only; the first and last line of each run are kept verbatim).
    pub(crate) parameterized_bytes: usize,
    /// Fixed-size line-blocks whose exact bytes already appeared earlier in
    /// the same history slice (the cross-result dedup target, C1).
    pub(crate) duplicate_blocks: usize,
    /// Bytes cross-result block dedup would remove (the duplicate
    /// occurrence's block bytes minus the back-reference marker).
    pub(crate) duplicate_block_bytes: usize,
}

/// Dry-run census: scan the history's tool-result texts and count, without
/// mutation, the bytes the existing and proposed deterministic passes would
/// remove. Read-only — the request is never modified. Duplicate detection
/// uses fixed-size line blocks over the full history text, an upper-bound
/// approximation of the C1 proposal's balanced-block eligibility gate.
#[cfg(test)]
pub(crate) fn census_request(request: &LanguageModelRequest) -> Census {
    let prefix_len = request
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let end = request.messages.len().saturating_sub(1);
    if prefix_len >= end {
        return Census::default();
    }
    let history = &request.messages[prefix_len..end];
    let mut census = Census::default();
    let mut seen_blocks: HashSet<String> = HashSet::default();
    for message in history {
        for part in &message.content {
            let MessageContent::ToolResult(result) = part else {
                continue;
            };
            for content in &result.content {
                let LanguageModelToolResultContent::Text(text) = content else {
                    continue;
                };
                census.result_texts += 1;
                census.scanned_bytes += text.len();
                census_lines(text, &mut seen_blocks, &mut census);
            }
        }
    }
    census
}

/// Scan one result text's lines for the census, advancing non-overlapping
/// through the longest arm that fires at each position: exact-repeat run,
/// parameterized run, duplicate block, else one line.
#[cfg(test)]
fn census_lines(text: &str, seen_blocks: &mut HashSet<String>, census: &mut Census) {
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        // Exact-run arm (the existing pass 1): 3+ identical adjacent lines.
        let mut exact_len = 1usize;
        while index + exact_len < lines.len() && lines[index + exact_len] == lines[index] {
            exact_len += 1;
        }
        if exact_len >= REPEAT_RUN_THRESHOLD && !lines[index].trim().is_empty() {
            census.repeat_runs += 1;
            census.repeat_run_bytes += lines[index]
                .len()
                .saturating_add(1)
                .saturating_mul(exact_len - 1);
            index += exact_len;
            continue;
        }
        // Parameterized-run arm (C4): 3+ adjacent lines identical except a
        // single trailing numeric field. Chained pairwise: each consecutive
        // pair shares its non-numeric prefix. Only entered when the lines
        // actually differ, so an exact run never double-counts here.
        let mut param_len = 1usize;
        while index + param_len < lines.len()
            && parameterized_pair(lines[index + param_len - 1], lines[index + param_len])
        {
            param_len += 1;
        }
        if param_len >= REPEAT_RUN_THRESHOLD {
            census.parameterized_runs += 1;
            for line in &lines[index + 1..index + param_len - 1] {
                census.parameterized_bytes += line.len().saturating_add(1);
            }
            index += param_len;
            continue;
        }
        // Duplicate-block arm (C1): a fixed-size window of consecutive lines
        // whose exact bytes already appeared earlier in this history slice.
        if let Some(block) = lines.get(index..index + DEDUP_BLOCK_LINES) {
            let key = block.join("\n");
            if seen_blocks.contains(&key) {
                census.duplicate_blocks += 1;
                // The marker replaces the whole duplicate block, so the
                // removable bytes are the block's own bytes.
                census.duplicate_block_bytes += key.len().saturating_add(1);
                index += DEDUP_BLOCK_LINES;
                continue;
            } else {
                seen_blocks.insert(key);
            }
        }
        index += 1;
    }
}

/// Whether `a` and `b` are identical except for their single trailing
/// numeric field: the non-numeric prefixes are byte-equal and both lines
/// carry a non-empty trailing numeric suffix.
#[cfg(test)]
fn parameterized_pair(a: &str, b: &str) -> bool {
    match (split_trailing_numeric(a), split_trailing_numeric(b)) {
        (Some((prefix_a, _)), Some((prefix_b, _))) => prefix_a == prefix_b,
        _ => false,
    }
}

/// Split a line into (non-numeric prefix, trailing numeric suffix). Returns
/// `None` when the line has no trailing digits. The prefix keeps its own
/// trailing separator characters (spaces, `:`, `/`, ` of`, `%`) so a shared
/// prefix pins the recurring text exactly.
#[cfg(test)]
fn split_trailing_numeric(line: &str) -> Option<(&str, &str)> {
    if !line.is_char_boundary(0) {
        return None;
    }
    let suffix_start = line
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii_digit())
        .map_or(0, |(i, _)| i + 1);
    if suffix_start == 0 || !line.is_char_boundary(suffix_start) {
        return None;
    }
    let suffix = &line[suffix_start..];
    (!suffix.is_empty()).then(|| (&line[..suffix_start], suffix))
}

/// The duplicate-detection block size for the census (consecutive lines
/// hashed as one unit): small enough to catch a re-read region inside a
/// larger result, large enough that one is not amortized away by the
/// back-reference marker it would be replaced with.
#[cfg(test)]
const DEDUP_BLOCK_LINES: usize = 8;

fn result_window_cap(budget: usize) -> usize {
    (budget / RESULT_WINDOW_BUDGET_DIVISOR).clamp(MIN_RESULT_WINDOW_BYTES, MAX_RESULT_WINDOW_BYTES)
}

/// Count-aware per-result window cap: the per-result cap is also bounded by
/// the number of result texts in the history, so the shrink pass's aggregate
/// output is bounded by the budget (k x cap <= budget/2 when the divisor
/// governs) rather than unbounded in k. Replaces the fixed 8 KiB floor with
/// the 2 KiB floor (operator ruling 2026-10-18: the summarization prompt
/// extracts Goal/Constraints/Progress/Decisions/Next Steps/Critical
/// Context/Relevant Files, which live at a result's head and tail — the
/// boundaries survive a 2 KiB window).
fn result_window_cap_for(budget: usize, result_count: usize) -> usize {
    if result_count == 0 {
        return result_window_cap(budget);
    }
    // Half the history budget is available to windowed results; with k
    // results, each gets at most budget/(2k) bytes.
    let per_result = budget / 2 / result_count;
    per_result
        .clamp(MIN_RESULT_WINDOW_BYTES_C2, MAX_RESULT_WINDOW_BYTES)
        .min(result_window_cap(budget))
}

/// Smallest per-result window retained when the count-aware cap governs
/// (operator-approved 2 KiB adaptive floor).
const MIN_RESULT_WINDOW_BYTES_C2: usize = 2 * 1024;

/// Number of result texts in the history slice (the count-aware cap's k).
fn history_result_count(history: &[LanguageModelRequestMessage]) -> usize {
    history
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|part| match part {
            MessageContent::ToolResult(result) => Some(result.content.len()),
            _ => None,
        })
        .sum()
}

/// Apply both deterministic passes to every tool-result text between the
/// system prefix and the summarization instruction. Returns the shrunk
/// request copy and what the passes did.
fn pre_shrink(
    request: &LanguageModelRequest,
    prefix_len: usize,
    end: usize,
    budget: usize,
) -> (LanguageModelRequest, ShrinkStats) {
    let result_count = history_result_count(&request.messages[prefix_len..end]);
    let cap = result_window_cap_for(budget, result_count);
    let mut shrunk = request.clone();
    let mut stats = ShrinkStats {
        original_bytes: 0,
        shrunk_bytes: 0,
        result_texts: 0,
        windowed_results: 0,
        windowed_bytes: 0,
        collapsed_runs: 0,
        collapsed_bytes: 0,
        result_cap_bytes: cap,
    };
    for message in &mut shrunk.messages[prefix_len..end] {
        for part in &mut message.content {
            let MessageContent::ToolResult(result) = part else {
                continue;
            };
            for content in &mut result.content {
                let LanguageModelToolResultContent::Text(text) = content else {
                    continue;
                };
                stats.result_texts += 1;
                let original = text.to_string();
                let (collapsed, runs, collapsed_removed) = collapse_repeated_lines(&original);
                stats.collapsed_runs += runs;
                stats.collapsed_bytes += collapsed_removed;
                let mut current = collapsed;
                if current.len() > cap {
                    let elided = current.len() - cap;
                    current = elide_string(&current, cap, elided);
                    stats.windowed_results += 1;
                    stats.windowed_bytes += elided;
                }
                if current.len() != original.len() {
                    *text = Arc::from(current);
                }
            }
        }
    }
    // Sizing may fail only on malformed message content; the shrink passes
    // themselves cannot. Fall back to the original measurement rather than
    // dropping the compaction on a sizing error.
    stats.original_bytes =
        history_bytes(&request.messages[prefix_len..end]).unwrap_or(stats.original_bytes);
    stats.shrunk_bytes =
        history_bytes(&shrunk.messages[prefix_len..end]).unwrap_or(stats.original_bytes);
    (shrunk, stats)
}

/// Collapse runs of `REPEAT_RUN_THRESHOLD`+ identical adjacent non-blank
/// lines into one line plus an exact count marker. Ordering, unique lines,
/// and blank runs are preserved verbatim. Returns the text, the number of
/// runs collapsed, and the bytes removed. (Run-length encoding over lines —
/// a compression citation, not a context-management claim; the 2026-10-18
/// census measured this pass at 0.0% median on real coding threads, so it
/// stays as a cheap lossless guard, not a load-bearing pass. Moved from
/// the retired `BridgeThreadCondenser` precompression with identical marker
/// semantics.)
fn collapse_repeated_lines(input: &str) -> (String, usize, usize) {
    let mut output = String::with_capacity(input.len());
    let mut collapsed_runs = 0usize;
    let mut lines = input.lines().peekable();
    while let Some(line) = lines.next() {
        let mut count = 1usize;
        while lines.peek() == Some(&line) {
            lines.next();
            count += 1;
        }
        if count >= REPEAT_RUN_THRESHOLD && !line.trim().is_empty() {
            collapsed_runs += 1;
            output.push_str(line);
            output.push('\n');
            output.push_str(&format!(
                "[preceding line repeated {} more times]\n",
                count - 1
            ));
        } else {
            for _ in 0..count {
                output.push_str(line);
                output.push('\n');
            }
        }
    }
    if collapsed_runs > 0 && output.len() < input.len() {
        let removed = input.len() - output.len();
        (output, collapsed_runs, removed)
    } else {
        (input.to_string(), 0, 0)
    }
}

// ─────────────────────── §2 Budget calibration ───────────────────────────

/// Fallback bytes-per-token used when the thread has no reported usage to
/// calibrate against, or the calibration is untrustworthy. Live zed-kask
/// tool output measured ~2.0 bytes/token (D8: 160,157 bytes accepted as
/// 80,119 input tokens); English prose runs ~4. The asymmetry that picks
/// the dense end: an over-large budget produces a request the provider
/// rejects (the failure this planner exists to prevent), while an
/// over-small budget only adds concurrent segments — cheap wall time.
const FALLBACK_BYTES_PER_TOKEN: f64 = 2.0;

/// Trust window for the calibrated bytes-per-token ratio. Real content for
/// this workload lives roughly in [1.5, 4.5] bytes/token; a ratio outside
/// the window signals a stale denominator (the last completed request was
/// much smaller than the current history, e.g. a thread rescued after
/// growing past its window) rather than denser content, so it is discarded
/// in favor of the fallback rather than clamped to a dangerous edge.
const MIN_TRUSTED_BYTES_PER_TOKEN: f64 = 1.0;
const MAX_TRUSTED_BYTES_PER_TOKEN: f64 = 4.0;

/// Fraction of the estimated byte budget assigned to history content; the
/// remainder absorbs estimation error and provider-side counting
/// differences.
const BUDGET_SAFETY_FACTOR: f64 = 0.85;

/// Byte allowance reserved for the per-segment context message when
/// computing the per-request overhead.
const SEGMENT_CONTEXT_OVERHEAD_BYTES: usize = 256;

/// Bytes-per-token for this thread's content, calibrated from the last
/// completed request's reported input tokens: the same history that is
/// about to be summarized, measured by the provider's own tokenizer. A
/// fixed constant fails in both directions — too high and a planned segment
/// exceeds the window and is rejected (token-dense content), too low and
/// fitting histories are over-segmented into multi-call plans (byte-heavy
/// content). Calibration removes both when the denominator is current:
/// the ratio cancels out of the fit decision, so a history is segmented
/// exactly when its token count exceeds ~85% of the input capacity. A
/// ratio outside the trust window is a stale denominator, not denser
/// content — it is discarded in favor of the fallback.
fn calibrated_bytes_per_token(total_history_bytes: usize, last_input_tokens: Option<u64>) -> f64 {
    last_input_tokens
        .filter(|tokens| *tokens > 0)
        .map(|tokens| total_history_bytes as f64 / tokens as f64)
        .filter(|ratio| {
            ratio.is_finite()
                && *ratio >= MIN_TRUSTED_BYTES_PER_TOKEN
                && *ratio <= MAX_TRUSTED_BYTES_PER_TOKEN
        })
        .unwrap_or(FALLBACK_BYTES_PER_TOKEN)
}

/// Per-request byte budget for history content: the model's input capacity
/// translated to bytes at the calibrated ratio with a safety factor, minus
/// the request's own system prefix, summarization instruction, and
/// segment-context overhead. Returns `None` when nothing is left for
/// history.
fn usable_history_budget(
    request: &LanguageModelRequest,
    prefix_len: usize,
    instruction: &LanguageModelRequestMessage,
    input_capacity_tokens: u64,
    bytes_per_token: f64,
) -> Result<Option<usize>> {
    let capacity_bytes = (input_capacity_tokens as f64 * bytes_per_token * BUDGET_SAFETY_FACTOR)
        .floor()
        .max(0.0) as usize;
    let mut overhead = SEGMENT_CONTEXT_OVERHEAD_BYTES;
    for message in &request.messages[..prefix_len] {
        overhead = overhead.saturating_add(message_size(message)?);
    }
    overhead = overhead.saturating_add(message_size(instruction)?);
    let budget = capacity_bytes.saturating_sub(overhead);
    Ok((budget > 0).then_some(budget))
}

// ─────────────────────── §3 History layout ────────────────────────────────

/// Byte sizes and safe segment boundaries for the history between the
/// system prefix and the final summarization instruction.
struct HistoryLayout {
    /// Model-visible byte size of each history message. The replay-only
    /// raw tool `output` may hold a second, uncompressed copy of a tool
    /// result; it is not the text the model summarizes and is not sized.
    sizes: Vec<usize>,
    /// History indices after which a segment may end: no tool use is
    /// outstanding and the boundary message completed an assistant response
    /// or a tool exchange. Empty when the history cannot be cleanly ordered
    /// or ends with an outstanding tool use — such histories have no safe
    /// internal split.
    cuts: Vec<usize>,
}

/// Walk the history measuring model-visible bytes and recording every safe
/// segment boundary. A tool call is never separated from its result: a
/// boundary is only safe where no tool use is outstanding.
fn history_layout(history: &[LanguageModelRequestMessage]) -> Result<HistoryLayout> {
    let mut sizes = Vec::with_capacity(history.len());
    let mut cuts = Vec::new();
    let mut outstanding = HashSet::default();
    let mut ordered = true;
    for (index, message) in history.iter().enumerate() {
        let mut has_result = false;
        let mut size = 0usize;
        for part in &message.content {
            match part {
                MessageContent::ToolUse(call) => {
                    outstanding.insert(&call.id);
                    size = size.saturating_add(serde_json::to_vec(part)?.len());
                }
                MessageContent::ToolResult(result) => {
                    has_result = true;
                    if !outstanding.remove(&result.tool_use_id) {
                        ordered = false;
                    }
                    size = size.saturating_add(serde_json::to_vec(&result.content)?.len());
                }
                _ => size = size.saturating_add(serde_json::to_vec(part)?.len()),
            }
        }
        sizes.push(size);
        if index + 1 < history.len()
            && outstanding.is_empty()
            && (message.role == Role::Assistant || has_result)
        {
            cuts.push(index);
        }
    }
    if !ordered || !outstanding.is_empty() {
        cuts.clear();
    }
    Ok(HistoryLayout { sizes, cuts })
}

// ─────────────────────── §4 Planning ──────────────────────────────────────

/// The ratified two-half plan: split at the safe boundary closest to the
/// byte midpoint. Balances bytes; does not certify token fit.
fn two_half_plan(
    request: &LanguageModelRequest,
    layout: &HistoryLayout,
    prefix_len: usize,
    instruction: &LanguageModelRequestMessage,
) -> Option<(LanguageModelRequest, LanguageModelRequest)> {
    let total: usize = layout.sizes.iter().sum();
    let target = total / 2;
    let mut best: Option<(usize, usize)> = None;
    let mut cumulative = 0usize;
    for (index, size) in layout.sizes.iter().enumerate() {
        cumulative = cumulative.saturating_add(*size);
        if layout.cuts.binary_search(&index).is_ok() {
            let distance = cumulative.abs_diff(target);
            if best.is_none_or(|(_, previous)| distance < previous) {
                best = Some((index, distance));
            }
        }
    }
    let cut = best.map(|(index, _)| index)?;
    let history_len = layout.sizes.len();
    let earlier = segment_request(request, prefix_len, 0..cut + 1, 1, 2, instruction);
    let later = segment_request(request, prefix_len, cut + 1..history_len, 2, 2, instruction);
    Some((earlier, later))
}

/// Pack the history into chronological segments at safe boundaries so each
/// segment's model-visible bytes fit `budget`, balancing rather than
/// greedily filling: with `n = ceil(remaining/budget)` segments left to
/// place, a segment closes at the first safe boundary past `remaining/n`
/// bytes (never past `budget`), so no single request is near-window when
/// the boundaries allow a finer split. A segment that still cannot be cut
/// within budget (an exchange larger than the budget) runs to its next safe
/// boundary and is elided on its request copy.
fn segment_requests(
    request: &LanguageModelRequest,
    layout: &HistoryLayout,
    prefix_len: usize,
    instruction: &LanguageModelRequestMessage,
    budget: usize,
) -> Result<Vec<LanguageModelRequest>> {
    let history_len = layout.sizes.len();
    let mut prefix_sums = Vec::with_capacity(history_len + 1);
    prefix_sums.push(0usize);
    let mut cumulative = 0usize;
    for size in &layout.sizes {
        cumulative = cumulative.saturating_add(*size);
        prefix_sums.push(cumulative);
    }
    let total = prefix_sums[history_len];

    let is_cut = |index: usize| layout.cuts.binary_search(&index).is_ok();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut seg_start = 0usize;
    while seg_start < history_len {
        let remaining = total.saturating_sub(prefix_sums[seg_start]);
        if remaining <= budget {
            ranges.push(seg_start..history_len);
            break;
        }
        let segment_count = remaining.div_ceil(budget).max(1);
        let target = remaining / segment_count; // ≤ budget by construction
        let mut first_cut_any: Option<usize> = None;
        let mut last_cut_within: Option<usize> = None;
        let mut chosen: Option<usize> = None;
        for index in seg_start..history_len {
            if !is_cut(index) {
                continue;
            }
            if index + 1 >= history_len {
                break; // a segment must leave at least one message behind
            }
            first_cut_any.get_or_insert(index);
            let size = prefix_sums[index + 1].saturating_sub(prefix_sums[seg_start]);
            if size > budget {
                break; // later cuts only add bytes
            }
            last_cut_within = Some(index);
            if size >= target {
                chosen = Some(index);
                break;
            }
        }
        // First boundary past the balance target within budget, else the
        // last boundary within budget, else the next boundary at all (an
        // over-budget segment, elided below), else the history end.
        let end = chosen
            .or(last_cut_within)
            .or(first_cut_any)
            .map(|cut| cut + 1)
            .unwrap_or(history_len);
        anyhow::ensure!(end > seg_start, "segment packing made no progress");
        ranges.push(seg_start..end);
        seg_start = end;
    }

    let count = ranges.len();
    let mut requests = Vec::with_capacity(count);
    for (ordinal, range) in ranges.iter().enumerate() {
        let mut segment = segment_request(
            request,
            prefix_len,
            range.clone(),
            ordinal + 1,
            count,
            instruction,
        );
        let size = prefix_sums[range.end].saturating_sub(prefix_sums[range.start]);
        if size > budget {
            let history_start = prefix_len + 1;
            let history_end = segment.messages.len().saturating_sub(1);
            if history_start < history_end {
                elide_history_to_budget(&mut segment.messages[history_start..history_end], budget)?;
            }
        }
        requests.push(segment);
    }
    Ok(requests)
}

/// Build one segment's summarization request: the system prefix, a segment
/// context note, the segment's slice of the history, and the final
/// summarization instruction.
fn segment_request(
    request: &LanguageModelRequest,
    prefix_len: usize,
    history_range: Range<usize>,
    segment_index: usize,
    segment_count: usize,
    instruction: &LanguageModelRequestMessage,
) -> LanguageModelRequest {
    let mut segment = request.clone();
    segment.messages = request.messages[..prefix_len].to_vec();
    segment.messages.push(context_message(format!(
        "Summarize chronological segment {segment_index} of {segment_count} of the conversation below. Do not infer missing context."
    )));
    segment.messages.extend(
        request.messages[prefix_len + history_range.start..prefix_len + history_range.end]
            .iter()
            .cloned(),
    );
    segment.messages.push(instruction.clone());
    segment
}

fn context_message(text: String) -> LanguageModelRequestMessage {
    LanguageModelRequestMessage {
        role: Role::User,
        content: vec![text.into()],
        cache: false,
        reasoning_details: None,
    }
}

// ─────────────────────── §5 Elision primitive ──────────────────────────────

/// Smallest head+tail (in bytes) kept when a text part is elided.
const ELISION_FLOOR_BYTES: usize = 256;

/// Byte allowance reserved for the elision marker inserted into an elided
/// text part (the marker's elided-byte count has few enough digits that
/// this always covers it).
const ELISION_MARKER_ALLOWANCE: usize = 192;

/// Head+tail elide the largest text parts of `messages` (the summarizer's
/// copy of one segment) until its model-visible bytes fit `budget`.
/// Elision touches only message text and tool-result text; reasoning
/// metadata, images, and tool-call inputs are preserved. Fails loudly when
/// the slice cannot fit even fully elided.
fn elide_history_to_budget(
    messages: &mut [LanguageModelRequestMessage],
    budget: usize,
) -> Result<()> {
    let mut size = history_bytes(messages)?;
    while size > budget {
        // Find the largest elidable text that can still shrink.
        let mut target: Option<((usize, usize, usize), usize)> = None;
        for (message_ix, message) in messages.iter().enumerate() {
            for (part_ix, part) in message.content.iter().enumerate() {
                let candidates: Vec<((usize, usize, usize), usize)> = match part {
                    MessageContent::Text(text) => {
                        vec![((message_ix, part_ix, usize::MAX), text.len())]
                    }
                    MessageContent::ToolResult(result) => result
                        .content
                        .iter()
                        .enumerate()
                        .filter_map(|(content_ix, content)| match content {
                            LanguageModelToolResultContent::Text(text) => {
                                Some(((message_ix, part_ix, content_ix), text.len()))
                            }
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                for (location, len) in candidates {
                    if len <= ELISION_FLOOR_BYTES {
                        continue;
                    }
                    if target.is_none_or(|(_, best)| len > best) {
                        target = Some((location, len));
                    }
                }
            }
        }
        let Some(((message_ix, part_ix, content_ix), raw_len)) = target else {
            anyhow::bail!(
                "compaction segment cannot fit the model's input budget even after elision ({size} bytes > {budget} bytes)"
            );
        };
        let needed = size - budget;
        let keep = raw_len
            .saturating_sub(needed.saturating_add(ELISION_MARKER_ALLOWANCE))
            .max(ELISION_FLOOR_BYTES);
        anyhow::ensure!(keep < raw_len, "elision made no progress");
        let elided_bytes = raw_len - keep;
        let Some(old_text) = elidable_text(messages, message_ix, part_ix, content_ix) else {
            anyhow::bail!("elision target disappeared while eliding");
        };
        let new_text = elide_string(&old_text, keep, elided_bytes);
        size = size
            .saturating_sub(string_json_len(&old_text))
            .saturating_add(string_json_len(&new_text));
        anyhow::ensure!(
            replace_elidable_text(messages, (message_ix, part_ix, content_ix), new_text),
            "elision target could not be replaced"
        );
    }
    Ok(())
}

/// Read one elidable text part. `content_ix == usize::MAX` addresses a
/// message-level `Text` part; otherwise it indexes the tool result's
/// content.
fn elidable_text(
    messages: &[LanguageModelRequestMessage],
    message_ix: usize,
    part_ix: usize,
    content_ix: usize,
) -> Option<String> {
    let message = messages.get(message_ix)?;
    let part = message.content.get(part_ix)?;
    match part {
        MessageContent::Text(text) if content_ix == usize::MAX => Some(text.clone()),
        MessageContent::ToolResult(result) => match result.content.get(content_ix)? {
            LanguageModelToolResultContent::Text(text) => Some(text.to_string()),
            _ => None,
        },
        _ => None,
    }
}

fn replace_elidable_text(
    messages: &mut [LanguageModelRequestMessage],
    location: (usize, usize, usize),
    replacement: String,
) -> bool {
    let (message_ix, part_ix, content_ix) = location;
    let Some(message) = messages.get_mut(message_ix) else {
        return false;
    };
    let Some(part) = message.content.get_mut(part_ix) else {
        return false;
    };
    match part {
        MessageContent::Text(text) if content_ix == usize::MAX => {
            *text = replacement;
            true
        }
        MessageContent::ToolResult(result) => {
            let Some(LanguageModelToolResultContent::Text(text)) =
                result.content.get_mut(content_ix)
            else {
                return false;
            };
            *text = Arc::from(replacement);
            true
        }
        _ => false,
    }
}

/// Keep the head and tail of `text` (roughly `keep_bytes` total) and mark
/// the elided middle honestly. Cuts land on UTF-8 character boundaries.
/// One primitive for both elision granularities: proactive per-result
/// windowing (§1) and last-resort whole-segment elision.
fn elide_string(text: &str, keep_bytes: usize, elided_bytes: usize) -> String {
    let head = floor_char_boundary(text, keep_bytes / 2);
    let tail = floor_char_boundary(text, keep_bytes - head);
    let tail_start = ceil_char_boundary(text, text.len().saturating_sub(tail));
    format!(
        "{}\n\n[… compaction elided {elided_bytes} bytes to fit the context budget; the full text remains in the thread history …]\n\n{}",
        &text[..head],
        &text[tail_start..],
    )
}

// ─────────────────────── §6 Sizing ───────────────────────────────────────

/// Model-visible byte size of one message's content. The replay-only raw
/// tool `output` may hold a second, uncompressed copy of a tool result; it
/// is not the text the model summarizes, so only `content` is sized.
fn message_size(message: &LanguageModelRequestMessage) -> Result<usize> {
    message.content.iter().try_fold(0usize, |total, part| {
        let size = match part {
            MessageContent::ToolResult(result) => serde_json::to_vec(&result.content)?.len(),
            _ => serde_json::to_vec(part)?.len(),
        };
        Ok::<_, anyhow::Error>(total.saturating_add(size))
    })
}

fn history_bytes(messages: &[LanguageModelRequestMessage]) -> Result<usize> {
    messages.iter().try_fold(0usize, |total, message| {
        message_size(message).map(|size| total.saturating_add(size))
    })
}

/// JSON-serialized length of a string, matching how `message_size` counts
/// content. serde serialization of a plain string cannot fail; the fallback
/// keeps the estimate conservative rather than panicking.
fn string_json_len(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.len() + 2, |encoded| encoded.len())
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

// ─────────────────────── §7 Tests ─────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use language_model::{
        LanguageModelToolResult, LanguageModelToolUse, LanguageModelToolUseInput,
    };

    fn text_message(role: Role, text: &str) -> LanguageModelRequestMessage {
        LanguageModelRequestMessage {
            role,
            content: vec![text.into()],
            cache: false,
            reasoning_details: None,
        }
    }

    fn tool_use_message(id: &str) -> LanguageModelRequestMessage {
        LanguageModelRequestMessage {
            role: Role::Assistant,
            content: vec![MessageContent::ToolUse(LanguageModelToolUse {
                id: id.into(),
                name: "terminal".into(),
                raw_input: "{}".into(),
                input: LanguageModelToolUseInput::Text("{}".into()),
                is_input_complete: true,
                thought_signature: Some(id.to_string()),
            })],
            cache: false,
            reasoning_details: None,
        }
    }

    fn tool_result_message(id: &str) -> LanguageModelRequestMessage {
        tool_result_message_with_text(id, "output")
    }

    fn tool_result_message_with_text(id: &str, text: &str) -> LanguageModelRequestMessage {
        tool_result_from(id, "terminal", text, false)
    }

    fn tool_result_from(
        id: &str,
        tool_name: &str,
        text: &str,
        is_error: bool,
    ) -> LanguageModelRequestMessage {
        LanguageModelRequestMessage {
            role: Role::User,
            content: vec![MessageContent::ToolResult(LanguageModelToolResult {
                tool_use_id: id.into(),
                tool_name: tool_name.into(),
                is_error,
                content: vec![LanguageModelToolResultContent::Text(text.into())],
                output: None,
            })],
            cache: false,
            reasoning_details: None,
        }
    }

    fn request_with_messages(messages: Vec<LanguageModelRequestMessage>) -> LanguageModelRequest {
        LanguageModelRequest {
            messages,
            ..Default::default()
        }
    }

    /// The exact per-request history budget `plan_compaction` derives from a
    /// capacity of 80,000 tokens for `request` at `bytes_per_token`,
    /// mirroring the planner's formula so the tests pin it.
    fn planned_budget(request: &LanguageModelRequest, bytes_per_token: f64) -> Result<usize> {
        let prefix_len = request
            .messages
            .iter()
            .take_while(|message| message.role == Role::System)
            .count();
        let instruction = request
            .messages
            .last()
            .context("Missing compaction instruction")?;
        usable_history_budget(request, prefix_len, instruction, 80_000, bytes_per_token)?
            .context("no usable budget")
    }

    fn segment_history_slice(
        segment: &LanguageModelRequest,
        prefix_len: usize,
    ) -> &[LanguageModelRequestMessage] {
        let history_start = prefix_len + 1;
        let history_end = segment.messages.len().saturating_sub(1);
        &segment.messages[history_start..history_end]
    }

    fn request_text(request: &LanguageModelRequest) -> String {
        request
            .messages
            .iter()
            .map(|message| message.string_contents())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn history_total_bytes(request: &LanguageModelRequest) -> Result<usize> {
        let prefix_len = request
            .messages
            .iter()
            .take_while(|message| message.role == Role::System)
            .count();
        let end = request.messages.len().saturating_sub(1);
        history_bytes(&request.messages[prefix_len..end])
    }

    fn single_plan_request(planned: &PlannedCompaction) -> &LanguageModelRequest {
        match &planned.plan {
            CompactionPlan::Single(request) => request,
            CompactionPlan::Segments(_) => panic!("expected a single-request plan"),
        }
    }

    // ── §1 deterministic pre-shrink ──

    #[test]
    fn pre_shrink_windows_oversized_results_and_keeps_small_ones() -> Result<()> {
        let large = format!("LARGE-{}-END", "x".repeat(200_000));
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(text_message(Role::User, "request"));
        messages.push(tool_result_message_with_text("bulk", &large));
        messages.push(tool_result_message_with_text("small", "compact output"));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages.clone());
        let budget = planned_budget(&request, FALLBACK_BYTES_PER_TOKEN)?;
        let cap = result_window_cap(budget);
        let planned = plan_compaction(&request, 80_000, None)?;
        assert_eq!(planned.shrink.windowed_results, 1);
        assert!(planned.shrink.original_bytes > planned.shrink.shrunk_bytes);
        assert!(
            planned.shrink.reduction_pct() > 80.0,
            "a 200KB result over a {cap}-byte cap must shrink hard ({}%)",
            planned.shrink.reduction_pct()
        );
        let plan_text = request_text(single_plan_request(&planned));
        assert!(plan_text.contains("compaction elided"), "honest marker");
        assert!(plan_text.contains("LARGE-"), "the head is kept");
        assert!(plan_text.contains("-END"), "the tail is kept");
        assert!(
            plan_text.contains("compact output"),
            "sub-cap results pass through verbatim"
        );
        // The caller's request — and therefore the stored history — is
        // never modified by planning.
        assert_eq!(request.messages, messages);
        Ok(())
    }

    #[test]
    fn pre_shrink_collapses_adjacent_repeats_with_counts() -> Result<()> {
        let repeated_line = "progress: compiling the same module again";
        let bulk: String = std::iter::repeat(repeated_line)
            .take(100)
            .map(|line| format!("{line}\n"))
            .collect();
        // Total under the window cap so only the RLE pass fires.
        assert!(bulk.len() < MIN_RESULT_WINDOW_BYTES);
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(tool_result_message_with_text("build", &bulk));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let planned = plan_compaction(&request, 80_000, None)?;
        assert_eq!(planned.shrink.collapsed_runs, 1);
        assert_eq!(planned.shrink.windowed_results, 0);
        let text = request_text(single_plan_request(&planned));
        assert!(text.contains("[preceding line repeated 99 more times]"));
        assert_eq!(
            text.matches(repeated_line).count(),
            1,
            "one kept line plus the count marker"
        );
        // Unique lines and sub-threshold runs are kept verbatim.
        let (kept, runs, _) = collapse_repeated_lines("one\ntwo\n\n\nprogress\nprogress\n");
        assert_eq!(runs, 0);
        assert_eq!(kept, "one\ntwo\n\n\nprogress\nprogress\n");
        Ok(())
    }

    #[test]
    fn pre_shrink_has_no_exemptions_on_the_summarizer_copy() -> Result<()> {
        // The previous deterministic policy exempted protected tools, JSON
        // results, and error results — which is why it shrank nothing on
        // real coding threads. On the summarizer's copy, everything
        // oversized is windowed.
        let json = serde_json::to_string_pretty(
            &(0..20_000)
                .map(|index| format!("entry-{index}"))
                .collect::<Vec<_>>(),
        )?;
        let bulk = "y".repeat(100_000);
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(tool_result_from("read", "read_file", &bulk, false));
        messages.push(tool_result_from("data", "terminal", &json, false));
        messages.push(tool_result_from("fail", "terminal", &bulk, true));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let planned = plan_compaction(&request, 80_000, None)?;
        assert_eq!(
            planned.shrink.windowed_results, 3,
            "protected-tool, JSON, and error results are all windowed on the copy"
        );
        let text = request_text(single_plan_request(&planned));
        assert_eq!(text.matches("compaction elided").count(), 3);
        Ok(())
    }

    // ── §2 budget calibration ──

    #[test]
    fn calibration_decides_segmentation_in_tokens_not_bytes() -> Result<()> {
        // The calibrated ratio must cancel out of the fit decision: the same
        // history is segmented or not based on its token count relative to
        // capacity, not on how byte-heavy the content is. A dense thread
        // (~2 bytes/token, tool-output heavy) and a prose thread (~4
        // bytes/token) of the same TOKEN count get the same plan shape.
        // Assistant prose is not touched by the pre-shrink, so these
        // fixtures exercise the segmentation decision alone.
        let mut dense = vec![text_message(Role::System, "system")];
        let mut prose = vec![text_message(Role::System, "system")];
        for index in 0..3 {
            dense.push(text_message(Role::User, &format!("request {index}")));
            dense.push(text_message(Role::Assistant, &"d".repeat(60_000)));
            prose.push(text_message(Role::User, &format!("request {index}")));
            prose.push(text_message(Role::Assistant, &"p".repeat(120_000)));
        }
        dense.push(text_message(Role::User, COMPACTION_PROMPT));
        prose.push(text_message(Role::User, COMPACTION_PROMPT));
        let dense = request_with_messages(dense);
        let prose = request_with_messages(prose);
        // ~180K dense bytes ≈ 90K tokens at 2.0; ~360K prose bytes ≈ 90K
        // tokens at 4.0: both histories sit at ~90% of the 80K-token
        // capacity, so both must be segmented — and the segment COUNT must
        // match, because the ratio cancels.
        let count = |request: &LanguageModelRequest, ratio: f64| -> Result<usize> {
            let plan = plan_compaction(
                request,
                80_000,
                Some(((history_total_bytes(request)? as f64 / ratio).ceil()) as u64),
            )?;
            match plan.plan {
                CompactionPlan::Segments(segments) => Ok(segments.len()),
                CompactionPlan::Single(_) => Ok(1),
            }
        };
        let dense_count = count(&dense, 2.0)?;
        let prose_count = count(&prose, 4.0)?;
        assert_eq!(dense_count, prose_count);
        assert!(dense_count >= 2, "histories over 85% of capacity segment");
        // Growing the token count at the same ratio grows the segment count
        // proportionally — never exploding the way a fixed byte constant
        // did for dense content (a 2-segment dense history planning 4-6).
        let mut wide = vec![text_message(Role::System, "system")];
        for index in 0..6 {
            wide.push(text_message(Role::User, &format!("request {index}")));
            wide.push(text_message(Role::Assistant, &"d".repeat(60_000)));
        }
        wide.push(text_message(Role::User, COMPACTION_PROMPT));
        let wide = request_with_messages(wide);
        let wide_count = count(&wide, 2.0)?;
        assert!(
            wide_count > dense_count && wide_count <= dense_count * 2,
            "segment count must track tokens, not bytes (dense {dense_count}, wide {wide_count})"
        );
        // A stale denominator (last completed request far smaller than the
        // current history) yields an out-of-window ratio, which must be
        // discarded in favor of the fallback rather than trusted: the
        // stuck-thread shape.
        let stale = calibrated_bytes_per_token(history_total_bytes(&dense)?, Some(1));
        assert_eq!(stale, FALLBACK_BYTES_PER_TOKEN);
        Ok(())
    }

    #[test]
    fn reported_token_count_keeps_dense_history_segments_within_capacity() -> Result<()> {
        // The PromptTooLarge rescue's calibration datum: the rejection's
        // reported input-token count. Token-dense history (true 1.2
        // bytes/token) over an 80K-token capacity — with the reported count
        // the ratio is trusted and every segment's true token demand fits
        // the capacity; with the 2.0 fallback the byte budget over-plans
        // segments whose true token demand exceeds the capacity, so the
        // rescue's own summarization request would be rejected at the
        // moment recovery matters most.
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..6 {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(40_000)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let true_ratio = 1.2_f64;
        let reported_tokens = ((history_total_bytes(&request)? as f64 / true_ratio).ceil()) as u64;

        let largest_segment_tokens = |last_input_tokens: Option<u64>| -> Result<f64> {
            let planned = plan_compaction(&request, 80_000, last_input_tokens)?;
            let CompactionPlan::Segments(segments) = planned.plan else {
                anyhow::bail!("expected a segmented plan");
            };
            let mut largest = 0usize;
            for segment in &segments {
                largest = largest.max(history_bytes(segment_history_slice(segment, 1))?);
            }
            Ok(largest as f64 / true_ratio)
        };

        let calibrated = largest_segment_tokens(Some(reported_tokens))?;
        let fallback = largest_segment_tokens(None)?;

        assert!(
            calibrated <= 80_000.0,
            "the reported count's plan must fit the token capacity (estimated {calibrated} tokens)"
        );
        assert!(
            fallback > 80_000.0,
            "the 2.0 fallback must over-plan dense content (estimated {fallback} tokens) — the reported count is load-bearing"
        );
        Ok(())
    }

    #[test]
    fn below_floor_capacity_keeps_legacy_two_half_plan() -> Result<()> {
        // 79,999 tokens is below MIN_COMPACTION_CONTEXT_WINDOW: no usable
        // budget, so no pre-shrink and the two-half split applies regardless
        // of size — the documented legacy behavior (bytes balance, no
        // token-fit guarantee, no elision).
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..2 {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(150_000)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let planned = plan_compaction(&request_with_messages(messages), 79_999, None)?;
        let CompactionPlan::Segments(segments) = planned.plan else {
            anyhow::bail!("expected the legacy two-half plan");
        };
        assert_eq!(segments.len(), 2);
        assert_eq!(planned.shrink.windowed_results, 0);
        assert_eq!(planned.shrink.collapsed_runs, 0);
        for segment in &segments {
            let text = request_text(segment);
            assert!(!text.contains("compaction elided"));
            assert!(text.contains('x'));
        }
        Ok(())
    }

    // ── §3–§4 layout and planning ──

    #[test]
    fn replay_only_raw_output_does_not_distort_planning() -> Result<()> {
        let mut messages = vec![
            text_message(Role::User, "request 1"),
            tool_use_message("first"),
            tool_result_message("first"),
            text_message(Role::User, "request 2"),
            text_message(Role::Assistant, "result 2"),
            text_message(Role::User, "request 3"),
            text_message(Role::Assistant, "result 3"),
            text_message(Role::User, COMPACTION_PROMPT),
        ];
        let before = plan_compaction(&request_with_messages(messages.clone()), 0, None)?;
        let Some(MessageContent::ToolResult(result)) = messages
            .get_mut(2)
            .and_then(|message| message.content.first_mut())
        else {
            anyhow::bail!("missing tool result fixture");
        };
        result.output = Some(serde_json::json!({"raw": "x".repeat(1_000_000)}));
        let after = plan_compaction(&request_with_messages(messages), 0, None)?;
        let segment_texts = |planned: &PlannedCompaction| match &planned.plan {
            CompactionPlan::Segments(segments) => Some(
                segments
                    .iter()
                    .map(|segment| request_text(segment))
                    .collect::<Vec<_>>(),
            ),
            CompactionPlan::Single(_) => None,
        };
        assert_eq!(
            segment_texts(&before).expect("two-half plan before"),
            segment_texts(&after).expect("two-half plan after"),
            "the replay-only raw output must not change the plan"
        );
        Ok(())
    }

    #[test]
    fn two_half_plan_balances_bytes_and_keeps_indivisible_history_single_pass() -> Result<()> {
        let mut messages = vec![text_message(Role::System, "system")];
        for (index, size) in [2000, 20, 20].into_iter().enumerate() {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(size)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let planned = plan_compaction(&request, 0, None)?;
        let CompactionPlan::Segments(segments) = planned.plan else {
            anyhow::bail!("expected the ratified two-half plan");
        };
        assert_eq!(segments.len(), 2);
        // The byte-midpoint cut lands after the 2000-byte exchange.
        assert!(request_text(&segments[0]).contains("request 0"));
        assert!(request_text(&segments[0]).contains(&"x".repeat(2000)));
        assert!(request_text(&segments[1]).contains("request 2"));

        // An indivisible history summarizes in a single pass.
        let messages = vec![
            text_message(Role::User, "request 0"),
            text_message(Role::Assistant, &"x".repeat(2000)),
            text_message(Role::User, COMPACTION_PROMPT),
        ];
        let planned = plan_compaction(&request_with_messages(messages), 0, None)?;
        assert!(matches!(planned.plan, CompactionPlan::Single(_)));

        // A tool call and its result must never land in different segments.
        let messages = vec![
            text_message(Role::User, "Keep authentication."),
            tool_use_message("earlier"),
            tool_result_message("earlier"),
            text_message(Role::User, COMPACTION_PROMPT),
        ];
        let planned = plan_compaction(&request_with_messages(messages), 0, None)?;
        assert!(
            matches!(planned.plan, CompactionPlan::Single(_)),
            "cannot split one tool exchange"
        );
        Ok(())
    }

    #[test]
    fn sub_cap_over_budget_history_plans_budget_fitting_segments() -> Result<()> {
        // Forty paired 6KB tool exchanges: each result under the per-result
        // window cap (so the deterministic shrink passes them through), but
        // ~250KB total against the ~135KB budget — segmentation must engage
        // and every segment must fit. The count-aware cap (C2, operator ruling
        // 2026-10-18) windows this fleet: 40 results against a ~135KB budget
        // give a per-result cap well under 6KB, so the fleet no longer rides
        // through untouched — the fixed floor's unbounded aggregate is the
        // exact failure this pinned. The pin's intent (every segment fits;
        // fitting segments are never elided) survives; the through-untouched
        // premise is superseded by the ratified count-aware cap.
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..40 {
            let id = format!("e{index}");
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(tool_use_message(&id));
            messages.push(tool_result_message_with_text(&id, &"z".repeat(6_000)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request, FALLBACK_BYTES_PER_TOKEN)?;
        let planned = plan_compaction(&request, 80_000, None)?;
        assert_eq!(
            planned.shrink.result_texts, 40,
            "every result text is examined"
        );
        assert!(
            planned.shrink.windowed_results > 0,
            "the count-aware cap windows the sub-fixed-floor fleet (the fixed floor passed it through untouched)"
        );
        let CompactionPlan::Segments(segments) = planned.plan else {
            anyhow::bail!("expected a segmented plan for an over-budget history");
        };
        assert!(segments.len() >= 2, "over-budget history segments");
        for segment in &segments {
            let bytes = history_bytes(segment_history_slice(segment, 1))?;
            assert!(
                bytes <= budget,
                "segment history ({bytes} bytes) must fit the budget ({budget} bytes)"
            );
        }
        Ok(())
    }

    #[test]
    fn oversized_tool_result_is_windowed_not_split() -> Result<()> {
        // One tool exchange larger than the whole budget: the deterministic
        // shrink windows it, the call and its result stay together in one
        // segment, and the stored history is never modified.
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(text_message(Role::User, "Keep authentication."));
        messages.push(tool_use_message("earlier"));
        messages.push(tool_result_message_with_text(
            "earlier",
            &"x".repeat(250_000),
        ));
        messages.push(text_message(Role::Assistant, "done"));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request, FALLBACK_BYTES_PER_TOKEN)?;
        let planned = plan_compaction(&request, 80_000, None)?;
        assert_eq!(planned.shrink.windowed_results, 1);
        let CompactionPlan::Segments(segments) = &planned.plan else {
            anyhow::bail!("expected the windowed history to plan as segments");
        };
        let first = segments.first().context("first segment")?;
        let parts: Vec<_> = first.messages.iter().flat_map(|m| &m.content).collect();
        assert!(
            parts.iter().any(
                |p| matches!(p, MessageContent::ToolUse(call) if call.id.to_string() == "earlier")
            ),
            "the tool call stays with its result"
        );
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, MessageContent::ToolResult(result) if result.tool_use_id.to_string() == "earlier")),
            "the tool result stays with its call"
        );
        assert!(
            request_text(first).contains("compaction elided"),
            "the oversized result is windowed on the summarizer's copy"
        );
        let bytes = history_bytes(segment_history_slice(first, 1))?;
        assert!(
            bytes <= budget,
            "windowed segment ({bytes} bytes) must fit the budget ({budget} bytes)"
        );
        assert!(request_text(segments.last().context("last segment")?).contains("done"));
        // The caller's request — the stored history — is untouched.
        assert!(
            request
                .messages
                .get(3)
                .is_some_and(|message| message.string_contents().contains(&"x".repeat(250_000))),
            "the stored history is never modified"
        );
        Ok(())
    }

    #[test]
    fn oversized_indivisible_prose_elides_for_summarizer_only() -> Result<()> {
        // Assistant prose is never touched by the pre-shrink (only tool
        // results are), so an indivisible prose history larger than the
        // budget exercises the last-resort whole-history elision.
        let giant = format!("HEAD-{}-TAIL", "x".repeat(300_000));
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(text_message(Role::User, "Keep authentication."));
        messages.push(text_message(Role::Assistant, &giant));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request, FALLBACK_BYTES_PER_TOKEN)?;
        let planned = plan_compaction(&request, 80_000, None)?;
        assert_eq!(planned.shrink.windowed_results, 0);
        let plan_text = request_text(single_plan_request(&planned));
        assert!(plan_text.contains("compaction elided"));
        assert!(plan_text.contains("HEAD-"), "the head is kept");
        assert!(plan_text.contains("-TAIL"), "the tail is kept");
        let bytes = history_bytes(segment_history_slice(single_plan_request(&planned), 1))?;
        assert!(
            bytes <= budget,
            "elided history ({bytes} bytes) must fit the budget ({budget} bytes)"
        );
        // Elision touches only the summarizer's copy: the caller's request
        // is unchanged.
        assert!(
            request
                .messages
                .get(2)
                .is_some_and(|message| message.string_contents() == giant),
            "the stored history is never modified"
        );
        Ok(())
    }

    #[test]
    fn balanced_packing_keeps_the_largest_segment_small() -> Result<()> {
        // Dense safe boundaries: six exchanges of 40K bytes. Greedy packing
        // to budget would fill the first segment near-budget (the slowest
        // possible single request); balanced packing closes at the first
        // boundary past remaining/n, so no request is near-window when the
        // boundaries allow a finer split.
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..6 {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(40_000)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request, FALLBACK_BYTES_PER_TOKEN)?;
        let planned = plan_compaction(&request, 80_000, None)?;
        let CompactionPlan::Segments(segments) = planned.plan else {
            anyhow::bail!("expected a segmented plan");
        };
        assert!(segments.len() >= 2);
        let mut sizes = Vec::with_capacity(segments.len());
        for segment in &segments {
            sizes.push(history_bytes(segment_history_slice(segment, 1))?);
        }
        let largest = sizes.iter().copied().max().context("sizes")?;
        let smallest = sizes.iter().copied().min().context("sizes")?;
        assert!(
            largest <= budget,
            "largest segment ({largest} bytes) must fit the budget ({budget} bytes)"
        );
        assert!(
            largest <= smallest * 2,
            "segments must be balanced, not greedily filled (largest {largest} vs smallest {smallest})"
        );
        Ok(())
    }

    #[test]
    fn merge_request_lists_summaries_in_chronological_order() {
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let base = request_with_messages(messages);
        let merged = merge_request(
            base,
            &[
                "First.".to_string(),
                "Second.".to_string(),
                "Third.".to_string(),
            ],
        );
        let text = request_text(&merged);
        let first = text.find("Segment 1:\nFirst.").expect("first summary");
        let second = text.find("Segment 2:\nSecond.").expect("second summary");
        let third = text.find("Segment 3:\nThird.").expect("third summary");
        assert!(first < second && second < third);
        assert!(text.contains("later corrections"));
        assert!(
            merged
                .messages
                .last()
                .is_some_and(|message| message.string_contents() == COMPACTION_PROMPT)
        );
    }

    // ── C2 count-aware cap ──

    #[test]
    fn count_aware_cap_windows_a_fleet_of_subcap_results() -> Result<()> {
        // The fixed 8 KiB floor's blind spot: a fleet of results each under
        // the old floor passed through untouched, so k x sub-cap was
        // unbounded in k. The count-aware cap must window them so the
        // aggregate of windowed results is bounded by budget/2.
        let budget = 160 * 1024;
        // 40 results at ~6 KiB each: 240 KiB total, each under the old
        // 8 KiB floor, all above the count-aware cap.
        let result_text = "x".repeat(6 * 1024);
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..40 {
            messages.push(tool_result_message_with_text(
                &format!("result-{index}"),
                &result_text,
            ));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages.clone());
        let cap = result_window_cap_for(budget, 40);
        assert!(
            cap < 6 * 1024,
            "the count-aware cap for 40 results must be under 6 KiB (budget/2/40 = {})",
            cap
        );
        let (shrunk, stats) = pre_shrink(&request, 1, 41, budget);
        assert_eq!(stats.result_texts, 40);
        assert_eq!(
            stats.windowed_results, 40,
            "every sub-old-floor result is windowed"
        );
        assert!(stats.windowed_bytes > 0);
        assert_ne!(
            shrunk.messages, request.messages,
            "the shrunk copy differs from the original"
        );
        assert_eq!(
            request.messages, messages,
            "the caller's request — the stored history — is never modified"
        );
        Ok(())
    }

    #[test]
    fn count_aware_cap_bounds_the_windowed_fleet_by_budget_half() {
        // The count-aware cap's invariant: k results x cap <= budget/2 while
        // the count governs — the fixed floor's aggregate was unbounded in k.
        for (budget, k) in [
            (160 * 1024, 1),
            (160 * 1024, 10),
            (160 * 1024, 40),
            (32 * 1024 * 1024, 200),
        ] {
            let cap = result_window_cap_for(budget, k);
            assert!(
                cap.saturating_mul(k) <= budget / 2 || cap == MIN_RESULT_WINDOW_BYTES_C2,
                "{k} results x cap {cap} must be bounded by budget/2 (or clamped to the 2 KiB floor)",
                k = k
            );
        }
        // A single result keeps the divisor-based cap: no count pressure.
        assert_eq!(
            result_window_cap_for(160 * 1024, 1),
            result_window_cap(160 * 1024)
        );
        // Zero results: the divisor cap as the harmless default.
        assert_eq!(
            result_window_cap_for(160 * 1024, 0),
            result_window_cap(160 * 1024)
        );
    }

    // ── census ──

    #[test]
    fn census_counts_duplicate_blocks_and_parameterized_runs() {
        // Two results sharing a large block: the second occurrence counts as
        // duplicate bytes (C1's target).
        let shared_block: String = (0..12)
            .map(|n| format!("shared line {n} of the block\n"))
            .collect();
        let first = format!("header-a\n{}footer-a\n", shared_block);
        let second = format!("header-b\n{}footer-b\n", shared_block);
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(tool_result_message_with_text("r1", &first));
        messages.push(tool_result_message_with_text("r2", &second));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let census = census_request(&request_with_messages(messages));
        assert_eq!(census.result_texts, 2);
        assert!(
            census.duplicate_blocks > 0,
            "the shared block is counted as duplicate"
        );
        assert!(census.duplicate_block_bytes > 0);
        // Parameterized run inside one result is counted (C4's target): the
        // ratified single-trailing-numeric shape (version counters).
        let counter_run: String = (1..=30)
            .map(|n| format!("Compiling dep-v0.4.{}\n", n))
            .collect();
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(tool_result_message_with_text("run", &counter_run));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let census = census_request(&request_with_messages(messages));
        assert_eq!(census.parameterized_runs, 1);
        assert!(census.parameterized_bytes > 0);
    }

    #[test]
    fn per_pass_byte_attribution_is_reported() -> Result<()> {
        // The instrumentation claim: each pass's removed bytes are visible
        // separately, so an inert pass is visible as a zero.
        let large = format!("LARGE-{}-END", "x".repeat(200_000));
        let repeated_line = "identical repeated line for the run-length pass";
        let bulk: String = std::iter::repeat(repeated_line)
            .take(50)
            .map(|line| format!("{line}\n"))
            .collect();
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(tool_result_message_with_text("bulk", &large));
        messages.push(tool_result_message_with_text("run", &bulk));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request, FALLBACK_BYTES_PER_TOKEN)?;
        let (_, stats) = pre_shrink(&request, 1, 3, budget);
        assert!(
            stats.windowed_bytes > 0,
            "the windowing pass's bytes are attributed"
        );
        assert!(
            stats.collapsed_bytes > 0,
            "the run-length pass's bytes are attributed"
        );
        assert_eq!(stats.result_texts, 2);
        assert_eq!(stats.result_cap_bytes, result_window_cap_for(budget, 2));
        Ok(())
    }

    /// Live-thread census (operator-approved Decision 3): the dry-run gate
    /// that decides whether the C1/C3 dedup arms are built at all. Reads the
    /// local threads DB when present, extracts every tool-result text from
    /// the largest threads, and reports each pass's would-be-removed bytes as
    /// a share of scanned bytes. `#[ignore]`d: it depends on a
    /// machine-local database and reports a measurement rather than asserting
    /// an invariant. Run on demand with
    /// `cargo test -p agent --lib live_thread_census -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the machine-local threads DB; a measurement, not an invariant"]
    fn live_thread_census() {
        let home = std::env::var("HOME").unwrap_or_default();
        let db =
            std::path::PathBuf::from(format!("{home}/.local/share/zed-kask/threads/threads.db"));
        if !db.exists() {
            panic!("threads DB not found at {db:?} — run on the operator machine");
        }
        // Read-only immutable open: the running editor holds the DB. smol's
        // Command is the clippy-prescribed replacement for the blocking
        // std::process::Command (disallowed-methods in clippy.toml).
        let uri = format!("file:{}?mode=ro&immutable=1", db.display());
        let ids = smol::block_on(async {
            smol::process::Command::new("sqlite3")
                .arg(&uri)
                .arg("SELECT thread_id FROM thread_messages GROUP BY thread_id ORDER BY sum(length(data)) DESC LIMIT 12")
                .output()
                .await
        })
        .expect("sqlite3 available");
        assert!(ids.status.success(), "thread listing failed");
        let ids = String::from_utf8_lossy(&ids.stdout);
        let ids: Vec<&str> = ids.lines().filter(|l| !l.is_empty()).collect();
        assert!(
            ids.len() >= 8,
            "census needs N>=8 threads, found {}",
            ids.len()
        );
        // Each thread capped at 24MB of message JSON: the largest threads run
        // 60MB+, and the cap keeps the census bounded; it biases duplicate
        // detection downward (fewer cross-result opportunities), which is the
        // conservative direction for a build gate.
        const PER_THREAD_CAP: usize = 24 * 1024 * 1024;
        let mut duplicate_shares: Vec<f64> = Vec::new();
        let mut parameterized_shares: Vec<f64> = Vec::new();
        let mut repeat_shares: Vec<f64> = Vec::new();
        for id in ids {
            let rows = smol::block_on(async {
                smol::process::Command::new("sqlite3")
                    .arg(&uri)
                    .arg(format!(
                        "SELECT data FROM thread_messages WHERE thread_id = '{id}' ORDER BY ix"
                    ))
                    .output()
                    .await
            })
            .expect("thread read");
            assert!(rows.status.success(), "thread {id} read failed");
            let raw = String::from_utf8_lossy(&rows.stdout).to_string();
            let mut texts: Vec<String> = Vec::new();
            let mut consumed = 0usize;
            for line in raw.split('\n') {
                if consumed > PER_THREAD_CAP {
                    break;
                }
                consumed += line.len();
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                collect_tool_result_texts(&value, &mut texts);
            }
            if texts.is_empty() {
                continue;
            }
            let mut messages = vec![text_message(Role::System, "system")];
            for (index, text) in texts.iter().enumerate() {
                messages.push(tool_result_message_with_text(&format!("c{index}"), text));
            }
            messages.push(text_message(Role::User, COMPACTION_PROMPT));
            let census = census_request(&request_with_messages(messages));
            let share = |bytes: usize| bytes as f64 / census.scanned_bytes.max(1) as f64;
            println!(
                "census {id}: {} results, {} bytes scanned; repeat-run {}/{} = {:.1}% (existing pass 1); parameterized {}/{} = {:.1}% (C4); duplicate-block {}/{} = {:.1}% (C1/C3)",
                census.result_texts,
                census.scanned_bytes,
                census.repeat_runs,
                census.repeat_run_bytes,
                share(census.repeat_run_bytes) * 100.0,
                census.parameterized_runs,
                census.parameterized_bytes,
                share(census.parameterized_bytes) * 100.0,
                census.duplicate_blocks,
                census.duplicate_block_bytes,
                share(census.duplicate_block_bytes) * 100.0,
            );
            duplicate_shares.push(share(census.duplicate_block_bytes));
            parameterized_shares.push(share(census.parameterized_bytes));
            repeat_shares.push(share(census.repeat_run_bytes));
        }
        let median = |mut xs: Vec<f64>| {
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = xs.len();
            if n == 0 {
                0.0
            } else if n % 2 == 1 {
                xs[n / 2]
            } else {
                (xs[n / 2 - 1] + xs[n / 2]) / 2.0
            }
        };
        println!(
            "CENSUS MEDIANS over {} threads: duplicate-block {:.1}% (C1/C3 gate: build iff >= 5.0%); parameterized-run {:.1}% (C4); exact-repeat {:.1}% (existing pass 1)",
            duplicate_shares.len(),
            median(duplicate_shares.clone()) * 100.0,
            median(parameterized_shares.clone()) * 100.0,
            median(repeat_shares.clone()) * 100.0,
        );
    }

    /// Stage-4 measurement for the D8 compaction-retirement project. The
    /// protocol was fixed BEFORE this instrument ran (corpus, capacities,
    /// metrics, stop conditions — recorded in
    /// kask/docs/research/d8-compaction-retirement/stage-4-measurement.md).
    /// Geometry leg: builds a production-shaped compaction request from each
    /// thread's real message sequence (`Message::to_request`) and reports what
    /// `plan_compaction` does at the pre-declared capacities. Fidelity leg:
    /// probes verbatim identifiers from pre-compaction tool-result texts
    /// against stored real compaction summaries. `#[ignore]`d: reads the
    /// machine-local threads DB; a measurement, not an invariant. Run with
    /// `cargo test -p agent --lib live_thread_stage4 -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the machine-local threads DB; a Stage-4 measurement, not an invariant"]
    fn live_thread_stage4_measurement() {
        let home = std::env::var("HOME").unwrap_or_default();
        let db =
            std::path::PathBuf::from(format!("{home}/.local/share/zed-kask/threads/threads.db"));
        assert!(
            db.exists(),
            "threads DB not found at {db:?} — run on the operator machine"
        );
        let uri = format!("file:{}?mode=ro&immutable=1", db.display());
        let run_sqlite = |sql: String| {
            let output = smol::block_on(async {
                smol::process::Command::new("sqlite3")
                    .arg(&uri)
                    .arg(&sql)
                    .output()
                    .await
            })
            .expect("sqlite3 available");
            assert!(output.status.success(), "sqlite query failed: {sql}");
            String::from_utf8_lossy(&output.stdout).to_string()
        };
        let median = |mut xs: Vec<f64>| {
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = xs.len();
            if n == 0 {
                f64::NAN
            } else if n % 2 == 1 {
                xs[n / 2]
            } else {
                (xs[n / 2 - 1] + xs[n / 2]) / 2.0
            }
        };

        // ── Geometry leg: top-12 threads by stored bytes (protocol corpus) ──
        let ids = run_sqlite(
            "SELECT thread_id FROM thread_messages GROUP BY thread_id ORDER BY sum(length(data)) DESC LIMIT 12"
                .to_string(),
        );
        let ids: Vec<&str> = ids.lines().filter(|line| !line.is_empty()).collect();
        println!("stage4-geometry corpus n={}", ids.len());
        const PER_THREAD_CAP: usize = 24 * 1024 * 1024;
        let capacities: [(u64, &str); 5] = [
            (1_310_720, "openrouter-endpoint"),
            (1_048_576, "glm-window"),
            (917_504, "rescue-ceiling"),
            (200_000, "mid-window"),
            (80_000, "floor"),
        ];
        let mut reduction_by_cap: [Vec<f64>; 5] =
            [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        let mut calls_by_cap: [Vec<f64>; 5] =
            [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for id in &ids {
            let rows = run_sqlite(format!(
                "SELECT data FROM thread_messages WHERE thread_id = '{id}' ORDER BY ix"
            ));
            let mut messages: Vec<crate::thread::Message> = Vec::new();
            let mut consumed = 0usize;
            let mut capped = false;
            for line in rows.split('\n') {
                if consumed > PER_THREAD_CAP {
                    capped = true;
                    break;
                }
                consumed += line.len();
                if let Ok(message) = serde_json::from_str::<crate::thread::Message>(line) {
                    messages.push(message);
                }
            }
            let mut request_messages = vec![text_message(Role::System, "system")];
            for message in &messages {
                request_messages.extend(message.to_request());
            }
            request_messages.push(text_message(Role::User, COMPACTION_PROMPT));
            let request = request_with_messages(request_messages);
            for (cap_ix, (capacity, label)) in capacities.iter().enumerate() {
                let planned = plan_compaction(&request, *capacity, None).expect("plan compaction");
                let (plan_kind, segments, calls) = match &planned.plan {
                    CompactionPlan::Single(_) => ("single", 1, 1),
                    CompactionPlan::Segments(segment_requests) => {
                        let count = segment_requests.len();
                        ("segments", count, count + 1)
                    }
                };
                println!(
                    "stage4-geometry id={id} cap={label}({capacity}) capped={capped} original_bytes={} shrunk_bytes={} reduction_pct={:.1} plan={plan_kind} segments={segments} calls={calls} windowed_results={} windowed_bytes={} collapsed_runs={}",
                    planned.shrink.original_bytes,
                    planned.shrink.shrunk_bytes,
                    planned.shrink.reduction_pct(),
                    planned.shrink.windowed_results,
                    planned.shrink.windowed_bytes,
                    planned.shrink.collapsed_runs,
                );
                reduction_by_cap[cap_ix].push(planned.shrink.reduction_pct());
                calls_by_cap[cap_ix].push(calls as f64);
                if *capacity == 1_048_576 && planned.shrink.original_bytes > 0 {
                    // Sensitivity re-plan forcing the 4.0 bytes/token ratio
                    // in-window (the prose-density upper trust edge).
                    let dense = (planned.shrink.original_bytes / 4) as u64;
                    if let Ok(dense_planned) = plan_compaction(&request, *capacity, Some(dense)) {
                        let dense_calls = match &dense_planned.plan {
                            CompactionPlan::Single(_) => 1,
                            CompactionPlan::Segments(s) => s.len() + 1,
                        };
                        println!(
                            "stage4-geometry id={id} cap={label}-r4.0 plan-segments={} calls={dense_calls}",
                            match &dense_planned.plan {
                                CompactionPlan::Single(_) => 1,
                                CompactionPlan::Segments(s) => s.len(),
                            }
                        );
                    }
                }
            }
        }
        for (cap_ix, (capacity, label)) in capacities.iter().enumerate() {
            println!(
                "stage4-geometry MEDIAN cap={label}({capacity}) n={} reduction_pct={:.1} calls={}",
                reduction_by_cap[cap_ix].len(),
                median(reduction_by_cap[cap_ix].clone()),
                median(calls_by_cap[cap_ix].clone())
            );
        }

        // ── Fidelity leg: threads with a stored Summary compaction ──
        let compaction_threads = run_sqlite(
            "SELECT thread_id FROM thread_messages WHERE data LIKE '%\"Compaction\"%' GROUP BY thread_id ORDER BY sum(length(data)) DESC LIMIT 12"
                .to_string(),
        );
        let compaction_threads: Vec<&str> = compaction_threads
            .lines()
            .filter(|line| !line.is_empty())
            .collect();
        println!("stage4-fidelity corpus n={}", compaction_threads.len());
        let extensions = [
            "rs", "toml", "md", "json", "sh", "py", "ts", "js", "lock", "yml", "yaml", "txt",
        ];
        let mut pass_rates: Vec<f64> = Vec::new();
        for id in &compaction_threads {
            let rows = run_sqlite(format!(
                "SELECT data FROM thread_messages WHERE thread_id = '{id}' ORDER BY ix"
            ));
            let mut summary: Option<String> = None;
            let mut probe_texts: Vec<String> = Vec::new();
            for line in rows.split('\n') {
                if summary.is_some() {
                    break;
                }
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                if let Some(compaction) = value.get("Compaction") {
                    if let Some(text) = compaction
                        .get("Summary")
                        .and_then(|s| s.as_str())
                        .map(str::to_string)
                    {
                        summary = Some(text);
                        break;
                    }
                }
                collect_tool_result_texts(&value, &mut probe_texts);
            }
            let Some(summary) = summary else {
                println!("stage4-fidelity id={id} skipped=no-stored-summary");
                continue;
            };
            // Pre-declared probe extraction: extension-bearing path tokens and
            // backtick-quoted spans (4..=80 chars), distinct, capped at 30.
            let mut probes: Vec<String> = Vec::new();
            let add_probe = |token: &str, probes: &mut Vec<String>| {
                let token = token.trim_matches(|c: char| {
                    matches!(
                        c,
                        '.' | ',' | ':' | ';' | ')' | ']' | '(' | '"' | '\'' | '`' | '*'
                    )
                });
                if token.len() >= 4 && token.len() <= 80 && !probes.iter().any(|p| p == token) {
                    probes.push(token.to_string());
                }
            };
            for text in &probe_texts {
                for token in text.split(|c: char| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            '"' | '\''
                                | '`'
                                | '('
                                | ')'
                                | '['
                                | ']'
                                | '{'
                                | '}'
                                | ','
                                | ':'
                                | ';'
                                | '|'
                                | '*'
                        )
                }) {
                    if extensions
                        .iter()
                        .any(|ext| token.ends_with(&format!(".{ext}")))
                    {
                        add_probe(token, &mut probes);
                    }
                }
                let mut rest = text.as_str();
                while let Some(start) = rest.find('`') {
                    let after = &rest[start + 1..];
                    if let Some(end) = after.find('`') {
                        add_probe(&after[..end], &mut probes);
                        rest = &after[end + 1..];
                    } else {
                        break;
                    }
                }
            }
            probes.truncate(30);
            if probes.len() < 10 {
                println!(
                    "stage4-fidelity id={id} skipped=too-few-probes({})",
                    probes.len()
                );
                continue;
            }
            let present = probes
                .iter()
                .filter(|probe| summary.contains(probe.as_str()))
                .count();
            let pass_pct = present as f64 * 100.0 / probes.len() as f64;
            println!(
                "stage4-fidelity id={id} probes_total={} probes_present={} pass_pct={:.1}",
                probes.len(),
                present,
                pass_pct
            );
            pass_rates.push(pass_pct);
        }
        println!(
            "stage4-fidelity MEDIAN n={} pass_pct={:.1}",
            pass_rates.len(),
            median(pass_rates)
        );
    }

    /// Walk a stored message JSON value for every tool-result text string.
    /// The stored `Agent` rows carry tool results in a `tool_results` map
    /// (`AgentMessage`'s serialization: `tool_results: {id: ToolResult}`),
    /// and a `UserMessage` may also carry inline results — walk both shapes.
    fn collect_tool_result_texts(value: &serde_json::Value, texts: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(results) = map.get("tool_results").and_then(|r| r.as_object()) {
                    for result in results.values() {
                        collect_result_content_texts(result, texts);
                    }
                }
                if let Some(content) = map.get("content").and_then(|c| c.as_array()) {
                    for item in content {
                        if let Some(result) = item.get("ToolResult") {
                            collect_result_content_texts(result, texts);
                        }
                    }
                }
                for (_, child) in map {
                    collect_tool_result_texts(child, texts);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    collect_tool_result_texts(child, texts);
                }
            }
            _ => {}
        }
    }

    fn collect_result_content_texts(result: &serde_json::Value, texts: &mut Vec<String>) {
        let Some(content) = result.get("content").and_then(|c| c.as_array()) else {
            return;
        };
        for part in content {
            if let Some(text) = part.get("Text").and_then(|t| t.as_str()) {
                texts.push(text.to_string());
            }
        }
    }
}
