//! Reasoning-runaway watchdog — the C1a stream guard from the agent-loop
//! guardrails plan (`kask/docs/plans/agent-loop-guardrails-plan.md` §8).
//!
//! Some reasoning models behind OpenAI-compatible gateways burn their
//! entire output budget inside the reasoning channel: the stream delivers
//! only thinking deltas — no visible text, no tool call — until
//! `max_tokens` lands (the zero-content MaxTokens signature D43 warns
//! about) or the user cancels. The source system arms these bounds in all
//! six shipped profiles (120 s / 16384 estimated tokens), and the operator
//! ruling on record (DIVERGENCE.md D42) names timeouts as the enforcement
//! mechanism for runaway processes.
//!
//! Bounds: the token cap is primary — load-invariant under endpoint
//! concurrency (the source system's measured tuning note: the time bound's
//! token-equivalent shrinks as concurrency rises; trust the token cap).
//! Both bounds are checked per delta, so no GPUI timer race is needed while
//! reasoning is flowing; a stream that STALLS mid-reasoning is the
//! deferred inter-chunk stall bound (which needs the timer race). Productive
//! output (visible text or a tool call) permanently disarms the watchdog:
//! the model is answering, not looping.

use std::time::Instant;

use language_model::LanguageModelCompletionEvent;

/// Liveness estimate: ~4 chars per reasoning token (the source system's
/// estimate — early cancellation commonly prevents the terminal billing
/// chunk from arriving, so provider usage is not usable here).
const CHARS_PER_TOKEN: usize = 4;

/// Abort after this many estimated reasoning-only tokens.
pub(crate) const REASONING_ONLY_MAX_TOKENS: u64 = 16_384;

/// Abort after this many seconds of reasoning-only output.
pub(crate) const REASONING_ONLY_TIMEOUT_S: u64 = 120;

/// Which bound fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunawayTrigger {
    Tokens,
    Time,
}

impl RunawayTrigger {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RunawayTrigger::Tokens => "token",
            RunawayTrigger::Time => "time",
        }
    }
}

/// Per-stream watchdog state. Lives as a loop-local in
/// `run_turn_internal` — one instance per completion request attempt,
/// never persisted.
#[derive(Default)]
pub(crate) struct ReasoningRunawayWatchdog {
    /// When the first reasoning-only delta arrived.
    reasoning_started_at: Option<Instant>,
    /// Total reasoning characters observed while not yet productive.
    reasoning_chars: usize,
    /// Whether visible text or a tool call has arrived — permanently
    /// disarms the watchdog.
    productive: bool,
}

impl ReasoningRunawayWatchdog {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Observe one raw completion event; returns the trigger when a bound
    /// is crossed. Call once per event, in stream order.
    pub(crate) fn observe(
        &mut self,
        event: &LanguageModelCompletionEvent,
    ) -> Option<RunawayTrigger> {
        match event {
            // Productive output: the model is answering, not looping.
            LanguageModelCompletionEvent::Text(_) | LanguageModelCompletionEvent::ToolUse(_) => {
                self.productive = true;
                None
            }
            LanguageModelCompletionEvent::Thinking { text, .. }
            | LanguageModelCompletionEvent::RedactedThinking { data: text } => {
                if self.productive {
                    return None;
                }
                self.reasoning_chars += text.len();
                if self.reasoning_started_at.is_none() {
                    self.reasoning_started_at = Some(Instant::now());
                }
                if self.estimated_tokens() >= REASONING_ONLY_MAX_TOKENS {
                    return Some(RunawayTrigger::Tokens);
                }
                if self.elapsed_seconds() >= REASONING_ONLY_TIMEOUT_S as f64 {
                    return Some(RunawayTrigger::Time);
                }
                None
            }
            _ => None,
        }
    }

    /// Estimated reasoning tokens so far (chars/4).
    pub(crate) fn estimated_tokens(&self) -> u64 {
        (self.reasoning_chars / CHARS_PER_TOKEN) as u64
    }

    /// Seconds since the first reasoning-only delta (0 before it starts).
    pub(crate) fn elapsed_seconds(&self) -> f64 {
        self.reasoning_started_at
            .map_or(0.0, |started| started.elapsed().as_secs_f64())
    }

    /// Test seam: construct with the reasoning clock already running, so
    /// the time bound is testable without waiting real seconds.
    #[cfg(test)]
    fn test_with_reasoning_started_at(started_at: Instant) -> Self {
        Self {
            reasoning_started_at: Some(started_at),
            reasoning_chars: 0,
            productive: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thinking(text: String) -> LanguageModelCompletionEvent {
        LanguageModelCompletionEvent::Thinking {
            text,
            signature: None,
        }
    }

    #[test]
    fn token_bound_fires_on_crossing() {
        let mut watchdog = ReasoningRunawayWatchdog::new();
        // Just under the bound: no fire.
        let just_under = "x".repeat(CHARS_PER_TOKEN * (REASONING_ONLY_MAX_TOKENS as usize - 1));
        assert_eq!(watchdog.observe(&thinking(just_under)), None);
        // The next delta crosses it.
        let over = "x".repeat(CHARS_PER_TOKEN * 2);
        assert_eq!(
            watchdog.observe(&thinking(over)),
            Some(RunawayTrigger::Tokens)
        );
        assert!(watchdog.estimated_tokens() >= REASONING_ONLY_MAX_TOKENS);
    }

    #[test]
    fn productive_output_disarms_the_watchdog() {
        let mut watchdog = ReasoningRunawayWatchdog::new();
        // Visible text first: the model is answering.
        assert_eq!(
            watchdog.observe(&LanguageModelCompletionEvent::Text("answer".into())),
            None
        );
        // A reasoning flood after productive output never fires — the
        // model legitimately reasons between answer segments.
        let flood = "x".repeat(CHARS_PER_TOKEN * (REASONING_ONLY_MAX_TOKENS as usize + 1));
        assert_eq!(watchdog.observe(&thinking(flood)), None);
    }

    #[test]
    fn estimate_is_chars_over_four() {
        let mut watchdog = ReasoningRunawayWatchdog::new();
        watchdog.observe(&thinking("x".repeat(8)));
        assert_eq!(watchdog.estimated_tokens(), 2);
    }

    #[test]
    fn redacted_thinking_counts_as_reasoning() {
        let mut watchdog = ReasoningRunawayWatchdog::new();
        let flood = "x".repeat(CHARS_PER_TOKEN * REASONING_ONLY_MAX_TOKENS as usize);
        assert_eq!(
            watchdog.observe(&LanguageModelCompletionEvent::RedactedThinking { data: flood }),
            Some(RunawayTrigger::Tokens)
        );
    }

    #[test]
    fn time_bound_fires_on_elapsed() {
        // Shift the reasoning start into the past instead of waiting real
        // seconds: the bound compares against the recorded start.
        let started = Instant::now() - std::time::Duration::from_secs(REASONING_ONLY_TIMEOUT_S + 1);
        let mut watchdog = ReasoningRunawayWatchdog::test_with_reasoning_started_at(started);
        // A tiny delta — far under the token bound — fires the time bound.
        assert_eq!(
            watchdog.observe(&thinking("x".repeat(8))),
            Some(RunawayTrigger::Time)
        );
    }

    #[test]
    fn non_reasoning_events_are_ignored() {
        let mut watchdog = ReasoningRunawayWatchdog::new();
        assert_eq!(
            watchdog.observe(&LanguageModelCompletionEvent::Started),
            None
        );
        assert_eq!(
            watchdog.observe(&LanguageModelCompletionEvent::Stop(
                language_model::StopReason::EndTurn
            )),
            None
        );
        assert_eq!(watchdog.estimated_tokens(), 0);
        assert_eq!(watchdog.elapsed_seconds(), 0.0);
    }

    #[test]
    fn thread_loop_wires_the_watchdog() {
        // Source-structure pin (the D43 `turn_failure_surfaces_provider_rejection_detail`
        // precedent): reads thread.rs's source — a different file from this
        // test's own source, so the needles cannot be self-satisfied. The
        // behavioral turn-level test (fake model emitting only thinking
        // deltas, turn ends with the typed error) is the follow-up recorded
        // in the plan's execution record.
        let source = include_str!("thread.rs");
        for needle in [
            "ReasoningRunawayWatchdog::new()",
            "watchdog.observe(event)",
            "Ok(CompletionError::ReasoningRunaway",
            "reasoning-only runaway aborted",
        ] {
            assert!(
                source.contains(needle),
                "thread.rs must wire the reasoning-runaway watchdog: missing {needle:?}"
            );
        }
    }
}
