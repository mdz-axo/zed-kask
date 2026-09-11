//! Kask-owned compaction planning (D8): split a compaction request into two
//! chronological half-requests and recombine their summaries into one merge
//! request. Pure request algebra — no `Thread`, no `App`, no model — so the
//! planning is testable where the behavioral contract lives, in this crate.
//! The streaming, per-call usage accounting, cancellation, and summary
//! insertion stay native in `thread.rs` (`stream_compaction`), which keeps
//! the upstream-shaped lifecycle in the upstream file and the fork's
//! planning policy in this kask-owned one.
//!
//! Bytes balance the halves; they do not certify token fit. Histories with
//! no safe internal split return `None` and compact in a single pass.

use agent_settings::COMPACTION_PROMPT;
use anyhow::{Context, Result};
use collections::HashSet;
use language_model::{LanguageModelRequest, LanguageModelRequestMessage, MessageContent, Role};

/// Choose a roughly byte-balanced boundary after a complete assistant response
/// or tool exchange. Bytes balance the halves; they do not certify token fit.
///
/// The returned index addresses the full message list (system prefix
/// included) and is always strictly before the final summarization
/// instruction. A tool call is never separated from its result: a cut is only
/// valid where no tool use is outstanding, and a history whose exchanges
/// cannot be cleanly ordered has no split at all.
fn compaction_split_point(messages: &[LanguageModelRequestMessage]) -> Result<Option<usize>> {
    let start = messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let end = messages.len().saturating_sub(1); // final summarization instruction
    if start >= end {
        return Ok(None);
    }
    let history = &messages[start..end];
    let sizes = history
        .iter()
        .map(|message| serde_json::to_vec(message).map(|bytes| bytes.len()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let target = sizes.iter().sum::<usize>() / 2;
    let mut outstanding = HashSet::default();
    let mut bytes = 0usize;
    let mut best: Option<(usize, usize)> = None;
    for (index, (message, size)) in history.iter().zip(sizes).enumerate() {
        let mut has_result = false;
        for part in &message.content {
            match part {
                MessageContent::ToolUse(call) => {
                    outstanding.insert(&call.id);
                }
                MessageContent::ToolResult(result) => {
                    has_result = true;
                    if !outstanding.remove(&result.tool_use_id) {
                        return Ok(None);
                    }
                }
                _ => {}
            }
        }
        bytes += size;
        let cut = start + index + 1;
        if cut < end && outstanding.is_empty() && (message.role == Role::Assistant || has_result) {
            let distance = bytes.abs_diff(target);
            if best.is_none_or(|(_, previous)| distance < previous) {
                best = Some((cut, distance));
            }
        }
    }
    Ok(if outstanding.is_empty() {
        best.map(|(cut, _)| cut)
    } else {
        None
    })
}

/// Split a compaction request into two chronological half-requests, each
/// carrying the system prefix and the summarization instruction. Returns
/// `None` when the history has no safe internal split (single pass).
pub(crate) fn plan_halves(
    request: &LanguageModelRequest,
) -> Result<Option<(LanguageModelRequest, LanguageModelRequest)>> {
    let Some(split) = compaction_split_point(&request.messages)? else {
        return Ok(None);
    };
    let prefix_len = request
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    // `split` addresses the full message list (prefix included); the final
    // summarization instruction is never part of either half.
    let end = request.messages.len().saturating_sub(1);
    let instruction = request
        .messages
        .last()
        .context("Missing compaction instruction")?;
    let mut earlier = request.clone();
    earlier.messages = request.messages[..prefix_len].to_vec();
    earlier.messages.push(half_context(
        "Summarize the earlier half below. Do not infer missing context.".into(),
    ));
    earlier
        .messages
        .extend(request.messages[prefix_len..split].iter().cloned());
    earlier.messages.push(instruction.clone());
    let mut later = request.clone();
    later.messages = request.messages[..prefix_len].to_vec();
    later.messages.push(half_context(
        "Summarize the later half below. Do not infer missing context.".into(),
    ));
    later
        .messages
        .extend(request.messages[split..end].iter().cloned());
    later.messages.push(instruction.clone());
    Ok(Some((earlier, later)))
}

/// Build the final merge request from two completed half-summaries. Consumes
/// the base request, keeping its scalar fields and system prefix; the merge
/// instruction preserves later corrections and unresolved conflicts.
pub(crate) fn merge_request(
    mut base: LanguageModelRequest,
    earlier: &str,
    later: &str,
) -> LanguageModelRequest {
    let prefix_len = base
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    base.messages.truncate(prefix_len);
    base.messages.push(half_context(format!(
        "Merge these chronological summaries. Preserve later corrections and unresolved conflicts.\n\nEarlier half:\n{earlier}\n\nLater half:\n{later}"
    )));
    base.messages.push(half_context(COMPACTION_PROMPT.into()));
    base
}

fn half_context(text: String) -> LanguageModelRequestMessage {
    LanguageModelRequestMessage {
        role: Role::User,
        content: vec![text.into()],
        cache: false,
        reasoning_details: None,
    }
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
        LanguageModelRequestMessage {
            role: Role::User,
            content: vec![MessageContent::ToolResult(LanguageModelToolResult {
                tool_use_id: id.into(),
                tool_name: "terminal".into(),
                is_error: false,
                content: vec![LanguageModelToolResultContent::Text("output".into())],
                output: None,
            })],
            cache: false,
            reasoning_details: None,
        }
    }

    #[test]
    fn compaction_split_balances_bytes_and_keeps_indivisible_history_single_pass() -> Result<()> {
        let mut messages = vec![text_message(Role::System, "system")];
        for (index, size) in [2000, 20, 20].into_iter().enumerate() {
            messages.push(text_message(Role::User, &format!("request {index}")));
            messages.push(text_message(Role::Assistant, &"x".repeat(size)));
        }
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        assert_eq!(compaction_split_point(&messages)?, Some(3));
        messages.truncate(3);
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        assert_eq!(compaction_split_point(&messages)?, None);
        // A tool call and its result must never land in different halves.
        let mut messages = vec![
            text_message(Role::User, "Keep authentication."),
            tool_use_message("earlier"),
            tool_result_message("earlier"),
        ];
        messages.push(text_message(Role::User, COMPACTION_PROMPT));
        assert_eq!(
            compaction_split_point(&messages)?,
            None,
            "cannot split one tool exchange"
        );
        Ok(())
    }
}
