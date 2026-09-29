//! Kask-owned compaction planning (D8): split a compaction request into
//! chronological segment-requests that each fit the compaction model's
//! input budget, and recombine their summaries into one merge request.
//! Pure request algebra — no `Thread`, no `App`, no model — so the planning
//! is testable where the behavioral contract lives, in this crate. The
//! streaming, per-call usage accounting, cancellation, and summary
//! insertion stay native in `thread.rs` (`stream_compaction`), which keeps
//! the upstream-shaped lifecycle in the upstream file and the fork's
//! planning policy in this kask-owned one.
//!
//! Planning is budget-aware. With a usable input capacity (at least
//! [`crate::thread::MIN_COMPACTION_CONTEXT_WINDOW`] tokens) the history is
//! packed into segments whose model-visible bytes fit that capacity; a
//! history that cannot be cut at a safe boundary (one exchange larger than
//! the whole budget) is head+tail elided on the summarizer's request copy
//! only — the stored thread history is never modified. Without a usable
//! capacity the planner falls back to the two-half split, which balances
//! bytes but does not certify token fit.

use std::ops::Range;
use std::sync::Arc;

use agent_settings::COMPACTION_PROMPT;
use anyhow::{Context, Result};
use collections::HashSet;
use language_model::{
    LanguageModelRequest, LanguageModelRequestMessage, LanguageModelToolResultContent,
    MessageContent, Role,
};

/// Conservative bytes-per-token used to turn the model's input capacity
/// into a byte budget. The crate-wide convention estimates
/// `tokens = bytes / 3`; inverting it over-estimates the token cost of
/// planned segments, which is the safe direction for fitting.
const BYTES_PER_TOKEN: usize = 3;

/// Fraction of the estimated byte budget assigned to history content; the
/// remainder absorbs estimation error and provider-side counting
/// differences.
const BUDGET_SAFETY_FACTOR: f64 = 0.85;

/// Smallest head+tail (in bytes) kept when a text part is elided.
const ELISION_FLOOR_BYTES: usize = 256;

/// Byte allowance reserved for the per-segment context message when
/// computing the per-request overhead.
const SEGMENT_CONTEXT_OVERHEAD_BYTES: usize = 256;

/// Byte allowance reserved for the elision marker inserted into an elided
/// text part (the marker's elided-byte count has few enough digits that
/// this always covers it).
const ELISION_MARKER_ALLOWANCE: usize = 192;

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

/// Plan how to summarize `request`'s history under `input_capacity_tokens`
/// (the compaction model's input capacity, output allowance already
/// reserved). Histories that fit keep the ratified two-half split; larger
/// histories are packed into budget-fitting segments; an indivisible
/// history that exceeds the budget is elided on the summarizer's copy. A
/// capacity below `MIN_COMPACTION_CONTEXT_WINDOW` is not a usable budget,
/// so planning falls back to the two-half split.
pub(crate) fn plan_compaction(
    request: &LanguageModelRequest,
    input_capacity_tokens: u64,
) -> Result<CompactionPlan> {
    let prefix_len = request
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let end = request.messages.len().saturating_sub(1); // final summarization instruction
    if prefix_len >= end {
        return Ok(CompactionPlan::Single(request.clone()));
    }
    let instruction = request
        .messages
        .last()
        .context("Missing compaction instruction")?;
    let history = &request.messages[prefix_len..end];
    let layout = history_layout(history)?;
    let total: usize = layout.sizes.iter().sum();

    let history_budget = if input_capacity_tokens >= crate::thread::MIN_COMPACTION_CONTEXT_WINDOW {
        usable_history_budget(request, prefix_len, instruction, input_capacity_tokens)?
    } else {
        None
    };

    let plan = match history_budget {
        Some(budget) if layout.cuts.is_empty() && total > budget => {
            // One unbreakable exchange larger than the budget: summarize it
            // from an elided copy. The stored history is never modified.
            let mut elided = request.clone();
            elide_history_to_budget(&mut elided.messages[prefix_len..end], budget)?;
            CompactionPlan::Single(elided)
        }
        Some(budget) if total > budget => CompactionPlan::Segments(segment_requests(
            request,
            &layout,
            prefix_len,
            instruction,
            budget,
        )?),
        // No usable budget, or the history fits: keep the ratified two-half
        // split for splittable histories, single pass for the rest.
        _ => two_half_plan(request, &layout, prefix_len, instruction)
            .map(|(earlier, later)| CompactionPlan::Segments(vec![earlier, later]))
            .unwrap_or_else(|| CompactionPlan::Single(request.clone())),
    };
    Ok(plan)
}

/// Byte sizes and safe segment boundaries for the history between the
/// system prefix and the final summarization instruction.
struct HistoryLayout {
    /// Model-visible byte size of each history message. The replay-only
    /// raw tool `output` may hold a second, uncompressed copy of a tool
    /// result; it is not the text the model summarizes and is not sized.
    sizes: Vec<usize>,
    /// History indices after which a segment may end: no tool use is
    /// outstanding and the boundary message completed an assistant
    /// response or a tool exchange. Empty when the history cannot be
    /// cleanly ordered or ends with an outstanding tool use — such
    /// histories have no safe internal split.
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

/// Per-request byte budget for history content: the model's input capacity
/// translated to bytes with a safety factor, minus the request's own system
/// prefix, summarization instruction, and segment-context overhead.
/// Returns `None` when nothing is left for history.
fn usable_history_budget(
    request: &LanguageModelRequest,
    prefix_len: usize,
    instruction: &LanguageModelRequestMessage,
    input_capacity_tokens: u64,
) -> Result<Option<usize>> {
    let capacity_bytes = (input_capacity_tokens as usize).saturating_mul(BYTES_PER_TOKEN);
    let usable = (capacity_bytes as f64 * BUDGET_SAFETY_FACTOR).floor() as usize;
    let mut overhead = SEGMENT_CONTEXT_OVERHEAD_BYTES;
    for message in &request.messages[..prefix_len] {
        overhead = overhead.saturating_add(message_size(message)?);
    }
    overhead = overhead.saturating_add(message_size(instruction)?);
    let budget = usable.saturating_sub(overhead);
    Ok((budget > 0).then_some(budget))
}

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
/// segment's model-visible bytes fit `budget`; a segment that cannot be
/// cut small enough (an exchange larger than the budget) is elided on its
/// request copy.
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

    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut seg_start = 0usize;
    let mut last_cut: Option<usize> = None;
    for index in 0..history_len {
        let current = prefix_sums[index].saturating_sub(prefix_sums[seg_start]);
        if index > seg_start && current.saturating_add(layout.sizes[index]) > budget {
            if let Some(cut) = last_cut.filter(|cut| *cut + 1 > seg_start) {
                ranges.push(seg_start..cut + 1);
                seg_start = cut + 1;
            }
            // No safe boundary since this segment started: keep packing; the
            // over-budget segment is elided on its request copy below.
        }
        if layout.cuts.binary_search(&index).is_ok() {
            last_cut = Some(index);
        }
    }
    ranges.push(seg_start..history_len);

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

fn context_message(text: String) -> LanguageModelRequestMessage {
    LanguageModelRequestMessage {
        role: Role::User,
        content: vec![text.into()],
        cache: false,
        reasoning_details: None,
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use language_model::{
        LanguageModelToolResult, LanguageModelToolResultContent, LanguageModelToolUse,
        LanguageModelToolUseInput,
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
        LanguageModelRequestMessage {
            role: Role::User,
            content: vec![MessageContent::ToolResult(LanguageModelToolResult {
                tool_use_id: id.into(),
                tool_name: "terminal".into(),
                is_error: false,
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
    /// capacity of 80,000 tokens for `request`, mirroring the planner's
    /// formula so the tests pin it.
    fn planned_budget(request: &LanguageModelRequest) -> Result<usize> {
        let prefix_len = request
            .messages
            .iter()
            .take_while(|message| message.role == Role::System)
            .count();
        let instruction = request
            .messages
            .last()
            .context("Missing compaction instruction")?;
        usable_history_budget(request, prefix_len, instruction, 80_000)?.context("no usable budget")
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
        let before = plan_compaction(&request_with_messages(messages.clone()), 0)?;
        let Some(MessageContent::ToolResult(result)) = messages
            .get_mut(2)
            .and_then(|message| message.content.first_mut())
        else {
            anyhow::bail!("missing tool result fixture");
        };
        result.output = Some(serde_json::json!({"raw": "x".repeat(1_000_000)}));
        let after = plan_compaction(&request_with_messages(messages), 0)?;
        let segment_texts = |plan: &CompactionPlan| match plan {
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
        let plan = plan_compaction(&request, 0)?;
        let CompactionPlan::Segments(segments) = plan else {
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
        let plan = plan_compaction(&request_with_messages(messages), 0)?;
        assert!(matches!(plan, CompactionPlan::Single(_)));

        // A tool call and its result must never land in different segments.
        let messages = vec![
            text_message(Role::User, "Keep authentication."),
            tool_use_message("earlier"),
            tool_result_message("earlier"),
            text_message(Role::User, COMPACTION_PROMPT),
        ];
        let plan = plan_compaction(&request_with_messages(messages), 0)?;
        assert!(
            matches!(plan, CompactionPlan::Single(_)),
            "cannot split one tool exchange"
        );
        Ok(())
    }

    #[test]
    fn below_floor_capacity_keeps_legacy_two_half_plan() -> Result<()> {
        // 79,999 tokens is below MIN_COMPACTION_CONTEXT_WINDOW: no usable
        // budget, so the two-half split applies regardless of size — the
        // documented legacy behavior (bytes balance, no token-fit
        // guarantee, no elision).
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..2 {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(150_000)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let plan = plan_compaction(&request_with_messages(messages), 79_999)?;
        let CompactionPlan::Segments(segments) = plan else {
            anyhow::bail!("expected the legacy two-half plan");
        };
        assert_eq!(segments.len(), 2);
        for segment in &segments {
            let text = request_text(segment);
            assert!(!text.contains("compaction elided"));
            assert!(text.contains('x'));
        }
        Ok(())
    }

    #[test]
    fn over_budget_history_plans_budget_fitting_segments() -> Result<()> {
        let mut messages = vec![text_message(Role::System, "system")];
        for index in 0..3 {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(110_000)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request)?;
        let plan = plan_compaction(&request, 80_000)?;
        let CompactionPlan::Segments(segments) = plan else {
            anyhow::bail!("expected a segmented plan for an over-budget history");
        };
        assert_eq!(segments.len(), 3, "one exchange per budget-fitting segment");
        for (ordinal, segment) in segments.iter().enumerate() {
            assert_eq!(
                segment.messages.first().map(|message| message.role),
                Some(Role::System),
                "every segment carries the system prefix"
            );
            assert!(
                request_text(segment).contains(&format!("segment {} of 3", ordinal + 1)),
                "segment {ordinal} must be labeled"
            );
            assert!(
                segment
                    .messages
                    .last()
                    .is_some_and(|message| message.string_contents() == COMPACTION_PROMPT),
                "every segment carries the summarization instruction"
            );
            assert!(
                request_text(segment).contains(&format!("request {ordinal}")),
                "segment {ordinal} must carry exchange {ordinal}"
            );
            assert!(
                !request_text(segment).contains("compaction elided"),
                "a fitting segment is never elided"
            );
            let history_bytes = history_bytes(segment_history_slice(segment, 1))?;
            assert!(
                history_bytes <= budget,
                "segment {ordinal} history ({history_bytes} bytes) must fit the budget ({budget} bytes)"
            );
        }
        Ok(())
    }

    #[test]
    fn over_budget_tool_exchange_never_splits_and_elides() -> Result<()> {
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
        let budget = planned_budget(&request)?;
        let plan = plan_compaction(&request, 80_000)?;
        let CompactionPlan::Segments(segments) = plan else {
            anyhow::bail!("expected a segmented plan");
        };
        assert_eq!(segments.len(), 2);
        let first = segments.first().context("first segment")?;
        let parts: Vec<_> = first.messages.iter().flat_map(|m| &m.content).collect();
        assert!(
            parts
                .iter()
                .any(|part| matches!(part, MessageContent::ToolUse(call) if call.id.to_string() == "earlier")),
            "the tool call stays with its result"
        );
        assert!(
            parts
                .iter()
                .any(|part| matches!(part, MessageContent::ToolResult(result) if result.tool_use_id.to_string() == "earlier")),
            "the tool result stays with its call"
        );
        assert!(
            request_text(first).contains("compaction elided"),
            "the over-budget segment is elided on the summarizer's copy"
        );
        let history_bytes = history_bytes(segment_history_slice(first, 1))?;
        assert!(
            history_bytes <= budget,
            "elided segment ({history_bytes} bytes) must fit the budget ({budget} bytes)"
        );
        assert!(request_text(segments.last().context("last segment")?).contains("done"));
        Ok(())
    }

    #[test]
    fn oversized_indivisible_history_elides_for_summarizer_only() -> Result<()> {
        let giant = format!("HEAD-{}-TAIL", "x".repeat(300_000));
        let mut messages = vec![text_message(Role::System, "system")];
        messages.push(text_message(Role::User, "Keep authentication."));
        messages.push(text_message(Role::Assistant, &giant));
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        let request = request_with_messages(messages);
        let budget = planned_budget(&request)?;
        let plan = plan_compaction(&request, 80_000)?;
        let CompactionPlan::Single(elided) = plan else {
            anyhow::bail!("expected a single elided request for an indivisible history");
        };
        let text = request_text(&elided);
        assert!(text.contains("compaction elided"));
        assert!(text.contains("HEAD-"), "the head is kept");
        assert!(text.contains("-TAIL"), "the tail is kept");
        let history_bytes = history_bytes(segment_history_slice(&elided, 1))?;
        assert!(
            history_bytes <= budget,
            "elided history ({history_bytes} bytes) must fit the budget ({budget} bytes)"
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
}
