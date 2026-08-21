#![forbid(unsafe_code)]
//! GPUI widget rendering ```` ```swarm_delegate_results ```` fenced blocks
//! inline in agent markdown. Renders each `LocalDelegateResult` from the
//! `swarm_execute_plan_local` MCP tool as a structured per-agent card — agent
//! id, task-success badge, response (truncated), model, tokens, cost, latency,
//! tool-call count — instead of the raw JSON the tool-call output block would
//! otherwise show.
//!
//! Wired behind the D18 seam via [`hkask_viz_core::block_renderer`], which
//! composes this renderer with the media, graph, kanban, portfolio, and
//! scenarios renderers. The agent (or the swarm-steering skill) calls
//! `swarm_execute_plan_local` and emits the result array wrapped in a
//! `{"viz": "swarm_delegate_results", "results": [...]}` envelope as a fenced
//! block, e.g.:
//!
//! ```text
//! ```swarm_delegate_results
//! { "viz": "swarm_delegate_results",
//!   "results": [ { "agent_id": "researcher", "response": "...",
//!                  "model": "gpt-4o", "tokens_used": 1200, "cost": 50,
//!                  "cost_uncapped": 50, "balance": 950, "latency_ms": 4200,
//!                  "tool_calls": [{"name":"web_search","ok":true}],
//!                  "task_success": {"pass": true, "provenance": "deterministic_evaluator"} } ] }
//! ```
//! ```
//!
//! This is a passive renderer — no `ToolInvoker` fetches, no dispatch
//! affordance. The data is already in the chat stream; the widget only makes it
//! readable.
#![warn(clippy::let_underscore_future)]

pub mod block;

pub use block::{DelegateResultCard, SwarmBlockBody, parse_swarm_body};

use gpui::{Context, FocusHandle, IntoElement, ParentElement, Render, Styled, Window, div};
use theme::ActiveTheme;
use ui::{Color, Label, LabelCommon, LabelSize, prelude::*};

/// Maximum number of response characters rendered on the card before
/// truncation. The full response lives in the tool-call output block; the card
/// is a scan-friendly summary, not a transcript.
const RESPONSE_TRUNCATE_CHARS: usize = 240;

/// The swarm delegate-results widget view. Renders inline in agent markdown
/// (via the D18 seam composed by `hkask-viz-core`).
pub struct SwarmWidget {
    body: SwarmBlockBody,
    focus_handle: FocusHandle,
}

impl SwarmWidget {
    /// Create a new widget for the parsed block body.
    pub fn new(body: SwarmBlockBody, cx: &mut Context<Self>) -> Self {
        // No provenance is recorded here — this widget is a pure passive
        // renderer of an already-emitted block, not a dispatch surface, so
        // there is no `reg.widget.render` span to attribute (unlike the
        // kanban/scenarios widgets, which carry dispatch provenance).
        Self {
            body,
            focus_handle: cx.focus_handle(),
        }
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let count = self.body.results.len();
        h_flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(border_color)
            .child(
                Label::new("Swarm Delegation Results")
                    .size(LabelSize::Small)
                    .color(Color::Default),
            )
            .child(
                Label::new(format!(
                    "{count} agent{}",
                    if count == 1 { "" } else { "s" }
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
    }

    fn render_empty_state(&self) -> Option<impl IntoElement> {
        if self.body.results.is_empty() {
            Some(
                div().px_4().py_6().child(
                    Label::new("No delegation results in this block.")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
        } else {
            None
        }
    }

    fn render_cards(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        let cards: Vec<_> = self
            .body
            .results
            .iter()
            .map(|card| render_card(card, border_color))
            .collect();
        v_flex().gap_2().px_4().py_3().children(cards)
    }
}

impl Render for SwarmWidget {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border_color = cx.theme().colors().border;
        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .border_1()
            .border_color(border_color)
            .rounded_md()
            .overflow_hidden()
            .child(self.render_header(cx))
            .children(self.render_empty_state())
            .child(self.render_cards(cx))
            .into_any_element()
    }
}

/// Render one delegation result as a card. Pure of `cx` state — takes only the
/// theme-derived border color so the card is cheap to re-render on every
/// parent re-render (the D18 callback fires on every streaming token).
fn render_card(card: &DelegateResultCard, border_color: gpui::Hsla) -> impl IntoElement {
    let badge = render_success_badge(card);

    v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(border_color)
        .rounded_md()
        // Header row: agent id + success badge.
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    Label::new(card.agent_id.clone())
                        .size(LabelSize::Small)
                        .color(Color::Default),
                )
                .child(badge),
        )
        // Response (truncated).
        .when_some(truncate_response(&card.response), |this, text| {
            this.child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted))
        })
        // Metrics row: model, tokens, cost, latency, tool calls.
        .child(render_metrics(card))
}

/// Render the task-success badge. `None` (no evaluator ran) is a muted
/// "not evaluated" — never a fabricated pass/fail (`.rules` advertised-invariant
/// trap: the badge must not claim a verdict that was not produced).
fn render_success_badge(card: &DelegateResultCard) -> impl IntoElement {
    match &card.task_success {
        Some(verdict) => {
            let (label, color) = if verdict.pass {
                ("PASS", Color::Success)
            } else {
                ("FAIL", Color::Error)
            };
            let text = match verdict.score {
                Some(score) => format!("{label} ({:.2})", score),
                None => label.to_string(),
            };
            div()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .child(Label::new(text).size(LabelSize::XSmall).color(color))
        }
        None => div().px_2().py_0p5().rounded_sm().child(
            Label::new("not evaluated")
                .size(LabelSize::XSmall)
                .color(Color::Hidden),
        ),
    }
}

/// Truncate the response to `RESPONSE_TRUNCATE_CHARS` characters, appending an
/// ellipsis when truncated. Returns `None` for an empty response so no row is
/// rendered (avoids a stray muted empty label).
fn truncate_response(response: &str) -> Option<String> {
    if response.is_empty() {
        return None;
    }
    if response.len() <= RESPONSE_TRUNCATE_CHARS {
        return Some(response.to_string());
    }
    // Walk by char boundary so we never split a multi-byte codepoint.
    let truncated: String = response.chars().take(RESPONSE_TRUNCATE_CHARS).collect();
    Some(format!("{truncated}…"))
}

/// Render the metrics row: model, tokens, cost (with uncapped delta when the
/// cap bound the recording), latency, tool-call count, executed-skill count.
/// Each metric is a muted label so the row scans as a single strip.
fn render_metrics(card: &DelegateResultCard) -> impl IntoElement {
    let mut metrics: Vec<String> = Vec::new();
    if !card.model.is_empty() {
        metrics.push(card.model.clone());
    }
    metrics.push(format!("{} tok", card.tokens_used));
    // Surface the uncapped delta so a capped cost is visible, not silent.
    let cost_label = if card.cost_uncapped > card.cost {
        format!("{} cr (uncapped {})", card.cost, card.cost_uncapped)
    } else {
        format!("{} cr", card.cost)
    };
    metrics.push(cost_label);
    // Balance is rendered as "—" when not measured (None), never as 0 — the
    // `.rules` broken-feedback-loop trap: a fabricated 0 would read as "no
    // deviation" in the regulation loop.
    let balance_label = match card.balance {
        Some(balance) => format!("bal {}", balance),
        None => "bal —".to_string(),
    };
    metrics.push(balance_label);
    metrics.push(format!("{} ms", card.latency_ms));
    metrics.push(format!("{} tools", card.tool_calls.len()));
    if !card.executed_skills.is_empty() {
        metrics.push(format!("{} skills", card.executed_skills.len()));
    }

    h_flex()
        .flex_wrap()
        .gap_2()
        .children(metrics.into_iter().map(|metric| {
            Label::new(metric)
                .size(LabelSize::XSmall)
                .color(Color::Muted)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_swarm_body() {
        let body = r#"{"viz":"swarm_delegate_results","results":[]}"#;
        let parsed = parse_swarm_body(body).expect("valid body parses");
        assert_eq!(parsed.viz.as_deref(), Some("swarm_delegate_results"));
    }

    #[test]
    fn falls_through_non_swarm_bodies() {
        // A media-shaped body has no `viz` field → parsed (viz None) but not
        // claimed by the swarm renderer.
        let media = r#"{"kind":"video","src":"/clip.mp4"}"#;
        let parsed = parse_swarm_body(media).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("swarm_delegate_results"));

        // A kanban-shaped body has a different `viz` → not claimed.
        let kanban = r#"{"viz":"kanban","tasks":[]}"#;
        let parsed = parse_swarm_body(kanban).expect("json parses");
        assert_ne!(parsed.viz.as_deref(), Some("swarm_delegate_results"));

        // Plain text is not JSON → parse fails → renderer returns None.
        assert!(parse_swarm_body("not json").is_err());
    }

    #[test]
    fn truncate_response_short_passthrough() {
        assert_eq!(truncate_response("hello"), Some("hello".to_string()));
    }

    #[test]
    fn truncate_response_empty_is_none() {
        assert_eq!(truncate_response(""), None);
    }

    #[test]
    fn truncate_response_long_is_ellipsized() {
        let long = "a".repeat(RESPONSE_TRUNCATE_CHARS + 10);
        let truncated = truncate_response(&long).expect("some");
        assert!(truncated.ends_with('…'));
        // The visible body (excluding the ellipsis) is exactly the cap.
        assert_eq!(truncated.len() - "…".len(), RESPONSE_TRUNCATE_CHARS);
    }

    #[test]
    fn truncate_response_exact_cap_not_ellipsized() {
        let exact = "a".repeat(RESPONSE_TRUNCATE_CHARS);
        let truncated = truncate_response(&exact).expect("some");
        assert!(!truncated.ends_with('…'));
    }

    #[test]
    fn truncate_response_multibyte_safe() {
        // A multibyte codepoint straddling the cap boundary must not panic.
        let prefix = "a".repeat(RESPONSE_TRUNCATE_CHARS - 1);
        let input = format!("{prefix}🦀extra");
        let truncated = truncate_response(&input).expect("some");
        assert!(truncated.ends_with('…'));
    }
}
