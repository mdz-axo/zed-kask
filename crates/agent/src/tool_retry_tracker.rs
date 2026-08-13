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

        // Hard cap: either tracker hitting HARD_CAP refuses the call.
        if per_input_count >= HARD_CAP {
            return RetryVerdict::Refuse {
                attempt: per_input_count,
                reason: RefuseReason::IdenticalInput,
            };
        }
        if consecutive_count >= HARD_CAP {
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
pub fn format_warning(tool_name: &str, attempt: u32, consecutive: u32, probability: f64) -> String {
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
         or reframing the approach. After {HARD_CAP} failures, this tool will be hard-refused."
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
        let msg = format_warning("read_file", 3, 3, 0.2);
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
}
