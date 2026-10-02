//! Tool retry tracker — enforcement of the agent-loop retry cap.
//!
//! Prevents the "tool retry death spiral" where an agent retries the same
//! failing tool call in a zero-gain loop. Two tracking dimensions:
//!
//! 1. **Per-input tracker** — `(tool_name, input_hash) → failure_count`. Catches
//!    identical retries (same tool, same input). Parallel failures from one
//!    assistant message count once; repeated messages still reach the warning
//!    and hard cap. This is the only refusing dimension.
//!
//! 2. **Per-tool consecutive-failure tracker** — `tool_name → consecutive_failure_count`.
//!    Feeds the warning (with Bayesian probability) when the agent keeps
//!    failing with the same tool across varying inputs. It deliberately does
//!    NOT refuse: a consecutive-failure refusal locks out the corrected call
//!    after a run of wrong attempts (observed 2026-09-22, h_mem `fecd1f65`:
//!    five wrong-shape `render_template` calls hit the cap and the corrected
//!    call was refused). The identical-input tracker already catches the
//!    actual pathology — a true death spiral retries the same payload.
//!
//! 3. **Per-input success-streak tracker** — `(tool_name, input_hash) →
//!    consecutive_success_count`. The complement of dimension 1: back-to-back
//!    SUCCESSFUL executions of the identical (tool, input) pair with no
//!    other dispatch in between, deduped per assistant message. The failure
//!    side cannot see this class — a success resets it — and measured on
//!    the source system, 71% of budget-exhausted sub-agents died inside
//!    runs of ten or more consecutive byte-identical calls (median 87,
//!    worst 198 of 200). Hints at `WARN_THRESHOLD` prior identical
//!    successes, refuses at the per-tool hard cap (`hard_cap_for`). A
//!    changed input is a different key and always admissible; a different
//!    dispatch in between breaks the chain (reset-on-distinct — interleaved
//!    identical calls, e.g. edit → verify → edit → verify, never
//!    accumulate); a failure of the same key resets the streak (each
//!    outcome resets the other, so a key never holds both a failure count
//!    and a success streak).
//!
//! A successful call resets both failure trackers for that tool/input and
//! advances the success streak; a failed call resets the success streak.
//!
//! **Bayesian probability:** with a uniform prior on the success rate, after N
//! failed assistant messages (or N sequential calls without message identity)
//! the posterior predictive probability of success on the next attempt is
//! `1/(N+2)`. After 3 failed messages: ~20%. After 4: ~17%. This gives
//! the agent a quantitative signal, not just "try again."

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use crate::AgentToolOutput;

/// Whether a failed tool output is a deterministic authorization failure
/// (missing credential — `permission_denied`) rather than tool-behavior
/// flakiness. The typed kind is read structurally from the error output's
/// `raw_output` (the server's `structured_content`, set by the `is_error`
/// branch in `ContextServerTool::run_inner`) — a denied call cannot succeed
/// on an identical retry (the `.rules` credential pattern classifies it as
/// an authorization failure the operator must fix), so counting it as a
/// tracker failure only produces bogus "switch tools" statistics.
/// `unavailable` is deliberately NOT skipped: it can be transient (server
/// restarting), and stopping an agent from hammering an unavailable tool is
/// the tracker's job.
pub fn is_authorization_error(output: &AgentToolOutput) -> bool {
    hkask_types::tool_response::parse_tool_error_value(&output.raw_output)
        .and_then(|envelope| envelope.kind)
        .is_some_and(|kind| matches!(kind, hkask_types::McpErrorKind::PermissionDenied))
}

/// Number of failures before the warning escalates to a directive with
/// Bayesian probability.
pub const WARN_THRESHOLD: u32 = 3;

/// Hard cap — after this many failures of the same (tool, input) pair, the
/// tool refuses that input. Varying inputs never hit the cap; they only
/// warn (see the module docs for why).
pub const HARD_CAP: u32 = 5;

/// Per-tool hard-cap overrides. Tools listed here use the override value
/// instead of `HARD_CAP`. The `skill` tool runs a multi-step PDCA cascade
/// (skill execution) that can legitimately fail several times in a row
/// while skill execution iterates toward convergence — the default cap of 5
/// is too tight for a single skill invocation that may retry internally.
/// Raising the cap for `skill` only (not all tools) preserves the
/// death-spiral guard for read_file/grep/terminal/etc. while allowing
/// skill execution room to converge.
///
/// zed-kask: per-tool override for the `skill` tool. Test:
/// `skill_tool_uses_override_hard_cap`.
pub const SKILL_TOOL_HARD_CAP: u32 = 12;

/// Resolve the effective hard cap for a tool. Tools with an override use it;
/// all others use `HARD_CAP`.
pub fn hard_cap_for(tool_name: &str) -> u32 {
    if tool_name == "skill" {
        SKILL_TOOL_HARD_CAP
    } else {
        HARD_CAP
    }
}

/// A tool call attempt's verdict from the tracker.
#[derive(Debug)]
pub enum RetryVerdict {
    /// The call may proceed. No warning needed.
    Allow,
    /// The call may proceed, but the result should carry a warning directing
    /// the agent to switch tools, including the Bayesian probability of
    /// success. `attempt` is the failure count for this key; `consecutive` is
    /// the per-tool consecutive failure count; `probability` is P(success next).
    AllowWithWarning {
        attempt: u32,
        consecutive: u32,
        probability: f64,
    },
    /// The call is refused. The tool must return an error directing the agent
    /// to switch tools or stop. `attempt` is the per-input failure count.
    Refuse { attempt: u32 },
}

/// A tool call's success-repetition verdict: the guard over consecutive
/// identical SUCCESSFUL executions of the same (tool, input) pair — the
/// class `RetryVerdict` cannot see (a success resets the failure counters).
/// Measured on the source system: 71% of budget-exhausted sub-agents died
/// inside runs of ten or more consecutive byte-identical calls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RepeatVerdict {
    /// The call may proceed. No hint needed.
    Allow,
    /// The call may proceed, but the result should carry the repetition
    /// hint: `streak` prior consecutive turns ran this identical call
    /// successfully.
    Hint { streak: u32 },
    /// The call is refused: `streak` prior consecutive successful
    /// executions of this identical input.
    Refuse { streak: u32 },
}

/// Tracks repeated tool call failures per thread.
///
/// Lives on `Thread` so the state persists across tool calls within a turn.
/// Never persisted to DB — lives and dies with the thread.
#[derive(Default)]
pub struct ToolRetryTracker {
    /// `(tool_name, input_hash) → failure_count` — per-input tracker.
    per_input: Mutex<HashMap<(String, u64), FailureCount>>,
    /// `tool_name → consecutive_failure_count` — per-tool tracker.
    /// Incremented once per failed assistant message, reset on success.
    /// Warns only; never refuses (see module docs).
    per_tool: Mutex<HashMap<String, FailureCount>>,
    /// `(tool_name, input_hash) → consecutive_success_count` — the
    /// success-streak tracker (dimension 3 in the module docs). Bounded by
    /// the same `MAX_PER_INPUT_ENTRIES` eviction as the failure map.
    success_streak: Mutex<HashMap<(String, u64), FailureCount>>,
    /// The key most recently dispatched through `check_repetition`. The
    /// success streak is true-consecutive: `check_repetition` clears a
    /// key's streak entry when a different key was dispatched since its
    /// last success (reset-on-distinct — the Lean spec's
    /// `nextStreak_distinct_resets`), so interleaved identical calls
    /// (edit → verify → edit → verify) never accumulate; only back-to-back
    /// repeats do.
    last_dispatched: Mutex<Option<(String, u64)>>,
}

/// A message-deduped counter; also used by the success-streak map (the
/// counting semantics are outcome-agnostic — the name is historical).
#[derive(Default)]
struct FailureCount {
    count: u32,
    last_message_ix: Option<usize>,
}

impl FailureCount {
    fn record(&mut self, message_ix: Option<usize>) {
        if message_ix.is_none() || self.last_message_ix != message_ix {
            self.count = self.count.saturating_add(1);
            self.last_message_ix = message_ix;
        }
    }
}

/// Maximum entries retained in the `per_input` map before oldest are evicted.
/// Bounds memory growth in long-running threads where a tool fails
/// repeatedly with different inputs (common for `edit_file`, `grep`,
/// `read_file` with varying paths/queries). Entries are removed on success
/// for the same input, but failed inputs with no subsequent success
/// accumulate. When the map reaches this cap, the oldest entries are
/// evicted — the per-tool tracker (`per_tool`) still warns on consecutive
/// failure loops regardless of input.
const MAX_PER_INPUT_ENTRIES: usize = 500;

impl ToolRetryTracker {
    /// Check whether a tool call should be allowed, warned, or refused.
    /// Call this *before* `tool.run()`.
    pub fn check(&self, tool_name: &str, input: &serde_json::Value) -> RetryVerdict {
        let input_key = (tool_name.to_string(), input_hash(input));
        let per_input_count = {
            let per_input = self.per_input.lock().expect("retry tracker mutex poisoned");
            per_input.get(&input_key).map_or(0, |failure| failure.count)
        };
        let consecutive_count = {
            let per_tool = self.per_tool.lock().expect("retry tracker mutex poisoned");
            per_tool.get(tool_name).map_or(0, |failure| failure.count)
        };

        let effective_cap = hard_cap_for(tool_name);

        // Hard cap: the per-input tracker refuses identical retries. The
        // per-tool consecutive counter deliberately does not refuse — a
        // corrected call after a run of different failed attempts must
        // still run (module docs: the 2026-09-22 render_template incident).
        if per_input_count >= effective_cap {
            return RetryVerdict::Refuse {
                attempt: per_input_count,
            };
        }

        // Warning: either tracker hitting WARN_THRESHOLD triggers a warning.
        // Use the higher of the two counts for the probability calculation —
        // the agent should see the worst-case probability.
        let max_count = per_input_count.max(consecutive_count);
        if max_count >= WARN_THRESHOLD {
            // Bayesian posterior predictive: uniform prior on success rate p,
            // after N failures the posterior is Beta(1, N+1), so the predictive
            // P(success next) = 1/(N+2).
            let probability = 1.0 / (max_count as f64 + 2.0);
            return RetryVerdict::AllowWithWarning {
                attempt: per_input_count,
                consecutive: consecutive_count,
                probability,
            };
        }

        RetryVerdict::Allow
    }

    /// Record a sequential failure without an owning-message identity.
    pub fn record_failure(&self, tool_name: &str, input: &serde_json::Value) {
        self.record_failure_inner(tool_name, input, None);
    }

    /// Record a failed call from an assistant message. Multiple sibling failures
    /// on the same input/tool in that message count as one retry episode.
    pub fn record_failure_for_message(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        owning_message_ix: usize,
    ) {
        self.record_failure_inner(tool_name, input, Some(owning_message_ix));
    }

    fn record_failure_inner(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        message_ix: Option<usize>,
    ) {
        let input_key = (tool_name.to_string(), input_hash(input));
        {
            let mut per_input = self.per_input.lock().expect("retry tracker mutex poisoned");
            // Evict oldest entries when the cap is reached. The per-input map
            // is a diagnostic ring buffer — the per-tool tracker (`per_tool`)
            // still catches consecutive failure loops regardless of input, so
            // evicting old per-input entries does not weaken the death-spiral
            // protection. `HashMap` iteration order is non-deterministic, so
            // we evict an arbitrary entry rather than the oldest by timestamp
            // (the map does not carry timestamps). This is acceptable because
            // the cap is high (500) and the eviction only fires under sustained
            // failure with varying inputs.
            if per_input.len() >= MAX_PER_INPUT_ENTRIES {
                if let Some(key) = per_input.keys().next().cloned() {
                    per_input.remove(&key);
                }
            }
            per_input
                .entry(input_key.clone())
                .or_default()
                .record(message_ix);
        }
        {
            let mut per_tool = self.per_tool.lock().expect("retry tracker mutex poisoned");
            per_tool
                .entry(tool_name.to_string())
                .or_default()
                .record(message_ix);
        }
        // A failure breaks the consecutive-success chain for this key —
        // symmetric to `record_success_for_message` resetting the failure
        // counters.
        {
            let mut success_streak = self
                .success_streak
                .lock()
                .expect("retry tracker mutex poisoned");
            success_streak.remove(&input_key);
        }
    }

    /// Check the success-repetition streak for this (tool, input) before
    /// running it. The streak counts PRIOR consecutive successful executions
    /// (deduped per assistant message), so the hint lands on the dispatch
    /// after the 3rd identical success and the refusal after the hard cap —
    /// the same check-before-call contract as the failure side. The streak
    /// is true-consecutive: a different key dispatched since this key's
    /// last success breaks the chain (the entry is cleared —
    /// reset-on-distinct, the Lean spec's `nextStreak_distinct_resets`),
    /// so interleaved identical calls never accumulate.
    pub fn check_repetition(&self, tool_name: &str, input: &serde_json::Value) -> RepeatVerdict {
        let input_key = (tool_name.to_string(), input_hash(input));
        let streak = {
            let mut last_dispatched = self
                .last_dispatched
                .lock()
                .expect("retry tracker mutex poisoned");
            let mut success_streak = self
                .success_streak
                .lock()
                .expect("retry tracker mutex poisoned");
            if last_dispatched.as_ref() != Some(&input_key) {
                // A different key was dispatched since this key's last
                // success — the consecutive chain is broken. Clear the
                // entry so interleaved identical calls (edit → verify →
                // edit → verify) never accumulate.
                success_streak.remove(&input_key);
            }
            *last_dispatched = Some(input_key.clone());
            success_streak
                .get(&input_key)
                .map_or(0, |count| count.count)
        };
        let effective_cap = hard_cap_for(tool_name);
        if streak >= effective_cap {
            RepeatVerdict::Refuse { streak }
        } else if streak >= WARN_THRESHOLD {
            RepeatVerdict::Hint { streak }
        } else {
            RepeatVerdict::Allow
        }
    }

    /// Record a successful call from an assistant message: resets the
    /// failure counters for this key (a success means progress, not stuck)
    /// AND advances the consecutive-success streak for the identical
    /// (tool, input) pair — deduped per message, so sibling identical calls
    /// in one batch count once. Replaces the message-identity-less
    /// `record_success`, which could not dedup siblings.
    pub fn record_success_for_message(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        owning_message_ix: usize,
    ) {
        let input_key = (tool_name.to_string(), input_hash(input));
        {
            let mut per_input = self.per_input.lock().expect("retry tracker mutex poisoned");
            per_input.remove(&input_key);
        }
        {
            let mut per_tool = self.per_tool.lock().expect("retry tracker mutex poisoned");
            per_tool.remove(tool_name);
        }
        {
            let mut success_streak = self
                .success_streak
                .lock()
                .expect("retry tracker mutex poisoned");
            if success_streak.len() >= MAX_PER_INPUT_ENTRIES {
                if let Some(key) = success_streak.keys().next().cloned() {
                    success_streak.remove(&key);
                }
            }
            success_streak
                .entry(input_key)
                .or_default()
                .record(Some(owning_message_ix));
        }
    }
}

/// Hash a `serde_json::Value` deterministically. Uses `serde_json::to_vec`
/// (canonical serialization) then hashes the bytes.
fn input_hash(input: &serde_json::Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(input) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}

/// Format the warning message injected into the tool result at `WARN_THRESHOLD`.
/// Includes the Bayesian probability of success and a directive to switch tools.
/// `effective_cap` is the per-tool hard cap (from `hard_cap_for`) so the message
/// reports the correct refusal threshold for overridden tools.
pub fn format_warning(
    tool_name: &str,
    attempt: u32,
    consecutive: u32,
    probability: f64,
    effective_cap: u32,
) -> String {
    let pct = (probability * 100.0).round() as u32;
    let tracker_label = if attempt >= WARN_THRESHOLD {
        format!("same input {attempt} times")
    } else {
        format!("{consecutive} consecutive times (with varying inputs)")
    };
    format!(
        "WARNING: Tool '{tool_name}' has failed {tracker_label}. \
         Estimated probability of success on the next attempt: {pct}%. \
         Consider switching to a different tool (grep, terminal, find_path, spawn_agent) \
         or reframing the approach. After {effective_cap} failures with the same input, \
         that input will be hard-refused."
    )
}

/// Format the refusal message returned when the per-input hard cap is reached.
pub fn format_refusal(tool_name: &str, attempt: u32) -> String {
    format!(
        "Tool '{tool_name}' has failed {attempt} times with the same input. \
         Hard cap reached — this input is refused. Switch to a different tool \
         (grep, terminal, find_path, spawn_agent) or report the \
         blocker to the user. Do not retry — that is a zero-gain loop."
    )
}

/// Format the repetition hint injected into the tool result when the
/// identical call has already succeeded on `streak` consecutive prior
/// turns. Teaching text: names the streak, states that the prior results
/// are already in context, and names the alternatives (lisp-repair L1 — an
/// error that does not teach produces identical retries).
pub fn format_repeat_hint(tool_name: &str, streak: u32) -> String {
    format!(
        "NOTE: '{tool_name}' has succeeded with the exact same input {streak} \
         turns in a row — the previous results are already in context. Change \
         the arguments, use a different tool, or proceed with the information \
         you already have."
    )
}

/// Format the refusal returned when the success-repetition hard cap is
/// reached.
pub fn format_repeat_refusal(tool_name: &str, streak: u32) -> String {
    format!(
        "REFUSED: '{tool_name}' has succeeded with the exact same input {streak} \
         turns in a row. Re-running the identical call is a zero-gain loop. \
         Change the arguments, use a different tool, or report the blocker \
         to the user."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_first_few_failures() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..WARN_THRESHOLD {
            assert!(matches!(
                tracker.check("read_file", &input),
                RetryVerdict::Allow
            ));
            tracker.record_failure("read_file", &input);
        }
    }

    #[test]
    fn warns_after_threshold_identical_input() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..WARN_THRESHOLD {
            tracker.record_failure("read_file", &input);
        }
        match tracker.check("read_file", &input) {
            RetryVerdict::AllowWithWarning {
                attempt,
                probability,
                ..
            } => {
                assert_eq!(attempt, WARN_THRESHOLD);
                // After 3 failures: 1/(3+2) = 0.2
                assert!((probability - 0.2).abs() < 1e-9);
            }
            other => panic!("expected AllowWithWarning, got {other:?}"),
        }
    }

    #[test]
    fn refuses_after_hard_cap_identical_input() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..HARD_CAP {
            tracker.record_failure("read_file", &input);
        }
        match tracker.check("read_file", &input) {
            RetryVerdict::Refuse { .. } => {}
            other => panic!("expected Refuse, got {other:?}"),
        }
    }

    /// expect: a corrected call after a run of different failed attempts
    /// must still run — the consecutive tracker warns but never refuses
    /// (the 2026-09-22 render_template incident: five wrong-shape calls hit
    /// the consecutive cap and the corrected call was refused).
    #[test]
    fn corrected_input_is_allowed_after_cap_of_different_failures() {
        let tracker = ToolRetryTracker::default();
        // Fail with 5 different inputs on the same tool — the per-input tracker
        // never hits the cap (each input fails once).
        for i in 0..HARD_CAP {
            let input = serde_json::json!({"path": format!("file_{i}.rs")});
            tracker.record_failure("read_file", &input);
        }
        // A new, different input is allowed (with warning) — not refused.
        let corrected = serde_json::json!({"path": "totally_new.rs"});
        match tracker.check("read_file", &corrected) {
            RetryVerdict::AllowWithWarning { .. } => {}
            other => panic!("expected AllowWithWarning, got {other:?}"),
        }
        // Even well past the cap, varying inputs never refuse.
        for i in 0..HARD_CAP {
            let input = serde_json::json!({"path": format!("more_{i}.rs")});
            tracker.record_failure("read_file", &input);
        }
        match tracker.check("read_file", &serde_json::json!({"path": "another.rs"})) {
            RetryVerdict::AllowWithWarning { .. } => {}
            other => panic!("expected AllowWithWarning past the cap, got {other:?}"),
        }
    }

    /// expect: one assistant-message batch of failed searches must not use up
    /// every retry before the model can submit a corrected next-message input.
    #[test]
    fn failed_parallel_batch_allows_corrected_request() {
        let tracker = ToolRetryTracker::default();
        for i in 0..9 {
            tracker.record_failure_for_message(
                "web_search",
                &serde_json::json!({"query": format!("title {i}"), "strategy": "hybrid"}),
                42,
            );
        }
        let corrected = serde_json::json!({"query": "title 0", "strategy": "deep"});
        assert!(matches!(
            tracker.check("web_search", &corrected),
            RetryVerdict::Allow
        ));

        let duplicate = serde_json::json!({"query": "title 0", "strategy": "hybrid"});
        assert!(matches!(
            tracker.check("web_search", &duplicate),
            RetryVerdict::Allow
        ));
    }

    /// expect: retries across separate assistant messages still warn, and the
    /// identical input still reaches the hard cap, even when every message
    /// fans out many distinct failed tool calls — but varying inputs are
    /// never refused.
    #[test]
    fn repeated_failed_batches_refuse_only_the_identical_input() {
        let tracker = ToolRetryTracker::default();
        let same_input = serde_json::json!({"query": "same", "strategy": "hybrid"});
        for message_ix in 0..HARD_CAP as usize {
            for i in 0..9 {
                tracker.record_failure_for_message(
                    "web_search",
                    &serde_json::json!({"query": format!("title {message_ix}-{i}"), "strategy": "hybrid"}),
                    message_ix,
                );
                tracker.record_failure_for_message("web_search", &same_input, message_ix);
            }
            if message_ix + 1 == WARN_THRESHOLD as usize {
                assert!(matches!(
                    tracker.check("web_search", &serde_json::json!({"strategy": "deep"})),
                    RetryVerdict::AllowWithWarning {
                        consecutive: WARN_THRESHOLD,
                        ..
                    }
                ));
            }
        }
        // A varying input warns but is allowed — the consecutive tracker no
        // longer refuses.
        assert!(matches!(
            tracker.check("web_search", &serde_json::json!({"strategy": "deep"})),
            RetryVerdict::AllowWithWarning { .. }
        ));
        // The identical input is refused via the per-input tracker.
        assert!(matches!(
            tracker.check("web_search", &same_input),
            RetryVerdict::Refuse { .. }
        ));
        tracker.record_success_for_message("web_search", &same_input, 0);
        assert!(matches!(
            tracker.check("web_search", &serde_json::json!({"strategy": "deep"})),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn consecutive_tracker_warns_without_refusing() {
        let tracker = ToolRetryTracker::default();
        // Fail with 3 different inputs — consecutive tracker hits WARN_THRESHOLD.
        for i in 0..WARN_THRESHOLD {
            let input = serde_json::json!({"path": format!("file_{i}.rs")});
            tracker.record_failure("read_file", &input);
        }
        let new_input = serde_json::json!({"path": "another_new.rs"});
        match tracker.check("read_file", &new_input) {
            RetryVerdict::AllowWithWarning {
                consecutive,
                probability,
                ..
            } => {
                assert_eq!(consecutive, WARN_THRESHOLD);
                assert!((probability - 0.2).abs() < 1e-9);
            }
            other => panic!("expected AllowWithWarning from consecutive tracker, got {other:?}"),
        }
    }

    #[test]
    fn success_resets_both_trackers() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..WARN_THRESHOLD {
            tracker.record_failure("read_file", &input);
        }
        tracker.record_success_for_message("read_file", &input, 0);
        // After success, both counters are reset — next check should Allow.
        assert!(matches!(
            tracker.check("read_file", &input),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn different_tools_tracked_separately() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"query": "foo"});
        for _ in 0..HARD_CAP {
            tracker.record_failure("read_file", &input);
        }
        // read_file is refused, but grep (different tool) with the same input
        // is still allowed.
        assert!(matches!(
            tracker.check("read_file", &input),
            RetryVerdict::Refuse { .. }
        ));
        assert!(matches!(tracker.check("grep", &input), RetryVerdict::Allow));
    }

    #[test]
    fn skill_tool_uses_override_hard_cap() {
        // zed-kask: the `skill` tool gets a higher hard cap because its cascade
        // can legitimately fail several times while iterating to convergence.
        // The default HARD_CAP (5) would refuse a skill mid-cascade. This test
        // pins the override so a future refactor cannot silently drop it.
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"name": "company-research-deep"});

        // After HARD_CAP failures, a default tool would be refused — but `skill`
        // is allowed because its effective cap is SKILL_TOOL_HARD_CAP.
        for _ in 0..HARD_CAP {
            assert!(
                matches!(
                    tracker.check("skill", &input),
                    RetryVerdict::Allow | RetryVerdict::AllowWithWarning { .. }
                ),
                "skill should still be allowed at the default HARD_CAP boundary"
            );
            tracker.record_failure("skill", &input);
        }

        // Keep failing up to (but not reaching) SKILL_TOOL_HARD_CAP — still allowed.
        for _ in HARD_CAP..SKILL_TOOL_HARD_CAP {
            assert!(
                matches!(
                    tracker.check("skill", &input),
                    RetryVerdict::AllowWithWarning { .. }
                ),
                "skill should warn but remain allowed below its override cap"
            );
            tracker.record_failure("skill", &input);
        }

        // At SKILL_TOOL_HARD_CAP, the skill tool is finally refused.
        assert!(matches!(
            tracker.check("skill", &input),
            RetryVerdict::Refuse { .. }
        ));
    }

    #[test]
    fn skill_override_does_not_leak_to_other_tools() {
        // The override is per-tool: exhausting the skill cap must not raise the
        // cap for read_file or any other tool.
        let tracker = ToolRetryTracker::default();
        let skill_input = serde_json::json!({"name": "company-research-deep"});
        for _ in 0..HARD_CAP {
            tracker.record_failure("skill", &skill_input);
        }
        // read_file still uses the default HARD_CAP.
        let read_input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..HARD_CAP {
            tracker.record_failure("read_file", &read_input);
        }
        assert!(
            matches!(
                tracker.check("read_file", &read_input),
                RetryVerdict::Refuse { .. }
            ),
            "read_file must still refuse at HARD_CAP despite the skill override"
        );
    }

    #[test]
    fn hard_cap_for_resolves_skill_override() {
        assert_eq!(hard_cap_for("skill"), SKILL_TOOL_HARD_CAP);
        assert_eq!(hard_cap_for("read_file"), HARD_CAP);
        assert_eq!(hard_cap_for("grep"), HARD_CAP);
        assert_eq!(hard_cap_for(""), HARD_CAP);
    }

    #[test]
    fn bayesian_probability_decreases_with_failures() {
        // After N failures, P(success) = 1/(N+2).
        // N=3: 0.2, N=4: ~0.167
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..3 {
            tracker.record_failure("read_file", &input);
        }
        match tracker.check("read_file", &input) {
            RetryVerdict::AllowWithWarning { probability, .. } => {
                assert!(
                    (probability - 0.2).abs() < 1e-9,
                    "P(success) after 3 failures should be 0.2"
                );
            }
            other => panic!("expected AllowWithWarning, got {other:?}"),
        }
        tracker.record_failure("read_file", &input);
        match tracker.check("read_file", &input) {
            RetryVerdict::AllowWithWarning { probability, .. } => {
                assert!(
                    (probability - 1.0 / 6.0).abs() < 1e-9,
                    "P(success) after 4 failures should be ~0.167"
                );
            }
            other => panic!("expected AllowWithWarning, got {other:?}"),
        }
    }

    #[test]
    fn format_warning_includes_probability() {
        let msg = format_warning("read_file", 3, 3, 0.2, HARD_CAP);
        assert!(
            msg.contains("20%"),
            "warning should include probability percentage: {msg}"
        );
        assert!(
            msg.contains("read_file"),
            "warning should include tool name: {msg}"
        );
        assert!(
            msg.contains("switch"),
            "warning should include switch directive: {msg}"
        );
    }

    #[test]
    fn format_refusal_includes_attempt_count() {
        let msg = format_refusal("read_file", 5);
        assert!(
            msg.contains("5 times"),
            "refusal should include attempt count: {msg}"
        );
        assert!(
            msg.contains("same input"),
            "refusal should name the identical-input cause: {msg}"
        );
        assert!(
            msg.contains("refused"),
            "refusal should include directive: {msg}"
        );
    }

    #[test]
    fn per_input_map_caps_at_max_entries() {
        // The per_input map must not grow unbounded when a tool fails with
        // many different inputs. The cap is MAX_PER_INPUT_ENTRIES (500).
        let tracker = ToolRetryTracker::default();

        // Push 600 failures with distinct inputs.
        for i in 0..600 {
            let input = serde_json::json!({"path": format!("file_{i}.rs")});
            tracker.record_failure("read_file", &input);
        }

        // The map must not exceed the cap.
        let per_input = tracker.per_input.lock().unwrap();
        assert!(
            per_input.len() <= MAX_PER_INPUT_ENTRIES,
            "per_input map must cap at {}, got {}",
            MAX_PER_INPUT_ENTRIES,
            per_input.len()
        );
    }

    #[test]
    fn success_streak_hints_after_three_identical_successes() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        // Three consecutive successful turns (one call per message).
        for message_ix in 0..WARN_THRESHOLD as usize {
            assert!(matches!(
                tracker.check_repetition("read_file", &input),
                RepeatVerdict::Allow
            ));
            tracker.record_success_for_message("read_file", &input, message_ix);
        }
        // The next dispatch sees WARN_THRESHOLD prior identical successes.
        match tracker.check_repetition("read_file", &input) {
            RepeatVerdict::Hint { streak } => assert_eq!(streak, WARN_THRESHOLD),
            other => panic!("expected Hint, got {other:?}"),
        }
    }

    #[test]
    fn success_streak_refuses_at_hard_cap_and_changed_input_resets() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        // Five consecutive successful dispatches (check-before-call, then
        // record — the production flow). Dispatches after the 3rd success
        // carry the hint but still proceed.
        for message_ix in 0..HARD_CAP as usize {
            let verdict = tracker.check_repetition("read_file", &input);
            if (message_ix as u32) < WARN_THRESHOLD {
                assert!(matches!(verdict, RepeatVerdict::Allow));
            } else {
                assert!(matches!(verdict, RepeatVerdict::Hint { .. }));
            }
            tracker.record_success_for_message("read_file", &input, message_ix);
        }
        // The next dispatch sees HARD_CAP prior consecutive successes.
        match tracker.check_repetition("read_file", &input) {
            RepeatVerdict::Refuse { streak } => assert_eq!(streak, HARD_CAP),
            other => panic!("expected Refuse, got {other:?}"),
        }
        // A changed input is a different key — always admissible (the
        // Lean-pinned no-starvation property: the guard can only refuse a
        // repeat, never new work).
        let changed = serde_json::json!({"path": "bar.rs"});
        assert!(matches!(
            tracker.check_repetition("read_file", &changed),
            RepeatVerdict::Allow
        ));
        tracker.record_success_for_message("read_file", &changed, 99);
        // The intervening dispatch also resets the ORIGINAL key's chain —
        // returning to it starts a fresh streak, not a refusal.
        assert!(matches!(
            tracker.check_repetition("read_file", &input),
            RepeatVerdict::Allow
        ));
    }

    #[test]
    fn sibling_identical_successes_in_one_message_count_once() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        // Three messages, each dispatching two identical sibling calls.
        // Deduped per message the streak reaches 3 (hint on the next
        // dispatch); without the dedup it would reach 6 (refusal).
        for message_ix in 0..WARN_THRESHOLD as usize {
            assert!(matches!(
                tracker.check_repetition("read_file", &input),
                RepeatVerdict::Allow
            ));
            tracker.record_success_for_message("read_file", &input, message_ix);
            tracker.record_success_for_message("read_file", &input, message_ix);
        }
        match tracker.check_repetition("read_file", &input) {
            RepeatVerdict::Hint { streak } => assert_eq!(streak, WARN_THRESHOLD),
            other => panic!("expected Hint, got {other:?}"),
        }
    }

    #[test]
    fn failure_resets_the_success_streak() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for message_ix in 0..WARN_THRESHOLD as usize {
            assert!(matches!(
                tracker.check_repetition("read_file", &input),
                RepeatVerdict::Allow
            ));
            tracker.record_success_for_message("read_file", &input, message_ix);
        }
        // A failure of the same key breaks the success chain — the next
        // dispatch of the identical input starts fresh (without the
        // failure reset it would hint at streak 3).
        tracker.record_failure_for_message("read_file", &input, 99);
        assert!(matches!(
            tracker.check_repetition("read_file", &input),
            RepeatVerdict::Allow
        ));
    }

    #[test]
    fn success_streak_uses_the_skill_hard_cap_override() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"name": "some-skill"});
        // Below the skill override cap the streak only hints.
        for message_ix in 0..HARD_CAP as usize {
            let verdict = tracker.check_repetition("skill", &input);
            if (message_ix as u32) < WARN_THRESHOLD {
                assert!(matches!(verdict, RepeatVerdict::Allow));
            } else {
                assert!(matches!(verdict, RepeatVerdict::Hint { .. }));
            }
            tracker.record_success_for_message("skill", &input, message_ix);
        }
        assert!(matches!(
            tracker.check_repetition("skill", &input),
            RepeatVerdict::Hint { .. }
        ));
        tracker.record_success_for_message("skill", &input, HARD_CAP as usize);
        for message_ix in (HARD_CAP as usize + 1)..SKILL_TOOL_HARD_CAP as usize {
            assert!(matches!(
                tracker.check_repetition("skill", &input),
                RepeatVerdict::Hint { .. }
            ));
            tracker.record_success_for_message("skill", &input, message_ix);
        }
        assert!(matches!(
            tracker.check_repetition("skill", &input),
            RepeatVerdict::Refuse { .. }
        ));
    }

    #[test]
    fn success_streak_map_caps_at_max_entries() {
        let tracker = ToolRetryTracker::default();
        for i in 0..600 {
            let input = serde_json::json!({"path": format!("file_{i}.rs")});
            tracker.record_success_for_message("read_file", &input, i as usize);
        }
        let success_streak = tracker.success_streak.lock().unwrap();
        assert!(
            success_streak.len() <= MAX_PER_INPUT_ENTRIES,
            "success_streak map must cap at {}, got {}",
            MAX_PER_INPUT_ENTRIES,
            success_streak.len()
        );
    }

    #[test]
    fn interleaved_identical_successes_do_not_accumulate() {
        // The edit-verify loop: the identical verification call repeats
        // across the whole thread, but an edit dispatch intervenes between
        // every pair — reset-on-distinct keeps the streak at 1 and the
        // guard never fires. This is the property the per-key-map
        // implementation lacked (2026-10-02 plan audit): interleaved
        // repeats accumulated and the 6th verification would be refused
        // as a "zero-gain loop".
        let tracker = ToolRetryTracker::default();
        let verify = serde_json::json!({"pattern": "TODO"});
        let edit = serde_json::json!({"path": "a.rs", "content": "x"});
        for message_ix in 0..10 {
            for (tool, input) in [("grep", &verify), ("edit_file", &edit)] {
                assert!(
                    matches!(tracker.check_repetition(tool, input), RepeatVerdict::Allow),
                    "interleaved identical calls must never hint or refuse \
                     (round {message_ix}, {tool})"
                );
                tracker.record_success_for_message(tool, input, message_ix);
            }
        }
    }

    #[test]
    fn intervening_dispatch_resets_the_streak() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        let other = serde_json::json!({"path": "bar.rs"});
        // Three consecutive successes → the next dispatch hints.
        for message_ix in 0..WARN_THRESHOLD as usize {
            assert!(matches!(
                tracker.check_repetition("read_file", &input),
                RepeatVerdict::Allow
            ));
            tracker.record_success_for_message("read_file", &input, message_ix);
        }
        assert!(matches!(
            tracker.check_repetition("read_file", &input),
            RepeatVerdict::Hint { streak: 3 }
        ));
        // One different dispatch breaks the chain: the same input's next
        // check starts fresh — no hint, no refusal.
        assert!(matches!(
            tracker.check_repetition("read_file", &other),
            RepeatVerdict::Allow
        ));
        assert!(matches!(
            tracker.check_repetition("read_file", &input),
            RepeatVerdict::Allow
        ));
    }
}
