//! Tool retry tracker — hard enforcement of the agent-loop retry cap.
//!
//! Prevents the "tool retry death spiral" where an agent retries the same
//! failing tool call (or the same tool with trivially different inputs) in a
//! zero-gain loop. Two tracking dimensions:
//!
//! 1. **Per-input tracker** — `(tool_name, input_hash) → failure_count`. Catches
//!    identical retries (same tool, same input). After `WARN_THRESHOLD` identical
//!    failures, the tool result carries a warning with the Bayesian probability
//!    of success. After `HARD_CAP`, the tool hard-refuses.
//!
//! 2. **Per-tool consecutive-failure tracker** — `tool_name → consecutive_failure_count`.
//!    Catches "trivially different" loops where the agent changes the input
//!    slightly but keeps failing with the same tool. After `WARN_THRESHOLD`
//!    consecutive failures on the same tool (regardless of input), the warning
//!    fires. After `HARD_CAP`, the tool hard-refuses for *any* input.
//!
//! A successful call resets both trackers for that tool/input.
//!
//! **Bayesian probability:** with a uniform prior on the success rate, after N
//! consecutive failures the posterior predictive probability of success on the
//! next attempt is `1/(N+2)`. After 3 failures: ~20%. After 4: ~17%. This gives
//! the agent a quantitative signal, not just "try again."

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// Number of failures before the warning escalates to a directive with
/// Bayesian probability.
pub const WARN_THRESHOLD: u32 = 3;

/// Hard cap — after this many failures, the tool refuses to run.
pub const HARD_CAP: u32 = 5;

/// Per-tool hard-cap overrides. Tools listed here use the override value
/// instead of `HARD_CAP`. The `skill` tool runs a multi-step PDCA cascade
/// (manifest executor) that can legitimately fail several times in a row
/// while the cascade iterates toward convergence — the default cap of 5
/// is too tight for a single skill invocation that may retry internally.
/// Raising the cap for `skill` only (not all tools) preserves the
/// death-spiral guard for read_file/grep/terminal/etc. while allowing
/// the skill cascade room to converge.
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
    /// to switch tools or stop. `reason` explains which tracker fired.
    Refuse { attempt: u32, reason: RefuseReason },
}

/// Why the call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefuseReason {
    /// The same (tool, input) pair has failed `HARD_CAP` times.
    IdenticalInput,
    /// The tool has failed `HARD_CAP` consecutive times with any input.
    ConsecutiveFailures,
}

/// Tracks repeated tool call failures per thread.
///
/// Lives on `Thread` so the state persists across tool calls within a turn.
/// Never persisted to DB — lives and dies with the thread.
#[derive(Default)]
pub struct ToolRetryTracker {
    /// `(tool_name, input_hash) → failure_count` — per-input tracker.
    per_input: Mutex<HashMap<(String, u64), u32>>,
    /// `tool_name → consecutive_failure_count` — per-tool tracker.
    /// Incremented on every failure (any input), reset on any success.
    /// Catches "trivially different" loops where the input changes but the
    /// tool keeps failing.
    per_tool: Mutex<HashMap<String, u32>>,
}

/// Maximum entries retained in the `per_input` map before oldest are evicted.
/// Bounds memory growth in long-running threads where a tool fails
/// repeatedly with different inputs (common for `edit_file`, `grep`,
/// `read_file` with varying paths/queries). Entries are removed on success
/// for the same input, but failed inputs with no subsequent success
/// accumulate. When the map reaches this cap, the oldest entries are
/// evicted — the per-tool tracker (`per_tool`) still catches consecutive
/// failure loops regardless of input.
const MAX_PER_INPUT_ENTRIES: usize = 500;

impl ToolRetryTracker {
    /// Check whether a tool call should be allowed, warned, or refused.
    /// Call this *before* `tool.run()`.
    pub fn check(&self, tool_name: &str, input: &serde_json::Value) -> RetryVerdict {
        let input_key = (tool_name.to_string(), input_hash(input));
        let per_input_count = {
            let per_input = self.per_input.lock().expect("retry tracker mutex poisoned");
            per_input.get(&input_key).copied().unwrap_or(0)
        };
        let consecutive_count = {
            let per_tool = self.per_tool.lock().expect("retry tracker mutex poisoned");
            per_tool.get(tool_name).copied().unwrap_or(0)
        };

        let effective_cap = hard_cap_for(tool_name);

        // Hard cap: either tracker hitting the effective cap refuses the call.
        if per_input_count >= effective_cap {
            return RetryVerdict::Refuse {
                attempt: per_input_count,
                reason: RefuseReason::IdenticalInput,
            };
        }
        if consecutive_count >= effective_cap {
            return RetryVerdict::Refuse {
                attempt: consecutive_count,
                reason: RefuseReason::ConsecutiveFailures,
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

    /// Record that a tool call failed. Call this when `tool.run()` returns `Err`.
    pub fn record_failure(&self, tool_name: &str, input: &serde_json::Value) {
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
            *per_input.entry(input_key).or_default() += 1;
        }
        {
            let mut per_tool = self.per_tool.lock().expect("retry tracker mutex poisoned");
            *per_tool.entry(tool_name.to_string()).or_default() += 1;
        }
    }

    /// Record that a tool call succeeded. Resets both the per-input counter
    /// for this key and the per-tool consecutive-failure counter — a successful
    /// call means the agent is making progress, not stuck.
    pub fn record_success(&self, tool_name: &str, input: &serde_json::Value) {
        let input_key = (tool_name.to_string(), input_hash(input));
        {
            let mut per_input = self.per_input.lock().expect("retry tracker mutex poisoned");
            per_input.remove(&input_key);
        }
        {
            let mut per_tool = self.per_tool.lock().expect("retry tracker mutex poisoned");
            per_tool.remove(tool_name);
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
         or reframing the approach. After {effective_cap} failures, this tool will be hard-refused."
    )
}

/// Format the refusal message returned when the hard cap is reached.
pub fn format_refusal(tool_name: &str, attempt: u32, reason: RefuseReason) -> String {
    let reason_str = match reason {
        RefuseReason::IdenticalInput => format!("failed {attempt} times with the same input"),
        RefuseReason::ConsecutiveFailures => {
            format!("failed {attempt} consecutive times (with varying inputs)")
        }
    };
    format!(
        "Tool '{tool_name}' has {reason_str}. Hard cap reached — this tool is refused. \
         Switch to a different tool (grep, terminal, find_path, spawn_agent) or report the \
         blocker to the user. Do not retry — that is a zero-gain loop."
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
            RetryVerdict::Refuse { reason, .. } => {
                assert_eq!(reason, RefuseReason::IdenticalInput);
            }
            other => panic!("expected Refuse, got {other:?}"),
        }
    }

    #[test]
    fn consecutive_tracker_catches_trivially_different_inputs() {
        let tracker = ToolRetryTracker::default();
        // Fail with 5 different inputs on the same tool — the per-input tracker
        // never hits the cap (each input fails once), but the per-tool consecutive
        // tracker hits HARD_CAP.
        for i in 0..HARD_CAP {
            let input = serde_json::json!({"path": format!("file_{i}.rs")});
            tracker.record_failure("read_file", &input);
        }
        // Now any input should be refused — the consecutive tracker fired.
        let new_input = serde_json::json!({"path": "totally_new.rs"});
        match tracker.check("read_file", &new_input) {
            RetryVerdict::Refuse { reason, .. } => {
                assert_eq!(
                    reason,
                    RefuseReason::ConsecutiveFailures,
                    "should refuse via consecutive tracker, not per-input"
                );
            }
            other => panic!("expected Refuse via consecutive tracker, got {other:?}"),
        }
    }

    #[test]
    fn consecutive_tracker_warns_before_refusing() {
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
        tracker.record_success("read_file", &input);
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
        match tracker.check("skill", &input) {
            RetryVerdict::Refuse { reason, .. } => {
                assert_eq!(reason, RefuseReason::IdenticalInput);
            }
            other => panic!("expected Refuse at SKILL_TOOL_HARD_CAP, got {other:?}"),
        }
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
    fn format_refusal_includes_reason() {
        let msg = format_refusal("read_file", 5, RefuseReason::IdenticalInput);
        assert!(
            msg.contains("5 times"),
            "refusal should include attempt count: {msg}"
        );
        assert!(
            msg.contains("same input"),
            "refusal should include reason: {msg}"
        );
        assert!(
            msg.contains("refused"),
            "refusal should include directive: {msg}"
        );

        let msg = format_refusal("read_file", 5, RefuseReason::ConsecutiveFailures);
        assert!(
            msg.contains("consecutive"),
            "refusal should include consecutive reason: {msg}"
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
}
