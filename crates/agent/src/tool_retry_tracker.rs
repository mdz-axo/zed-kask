//! Tool retry tracker — hard enforcement of the agent-loop retry cap.
//!
//! Prevents the "tool retry death spiral" where an agent retries the same
//! failing tool call with trivially different parameter orderings, never
//! getting new information (a zero-gain loop / Ashby variety-deficit).
//!
//! Tracks `(tool_name, input_hash) → failure_count`. After `WARN_THRESHOLD`
//! identical failures, the tool result includes a directive to switch tools.
//! After `HARD_CAP` identical failures, the tool hard-refuses with an error.
//! A successful call resets the counter for that key.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// Number of identical failures before the warning escalates to a directive.
pub const WARN_THRESHOLD: u32 = 3;

/// Hard cap — after this many identical failures, the tool refuses to run.
pub const HARD_CAP: u32 = 5;

/// A tool call attempt's verdict from the tracker.
pub enum RetryVerdict {
    /// The call may proceed. The failure count is below the threshold.
    Allow,
    /// The call may proceed, but the result should carry a warning directing
    /// the agent to switch tools. The failure count has reached `WARN_THRESHOLD`
    /// but not yet `HARD_CAP`.
    AllowWithWarning { attempt: u32, cap: u32 },
    /// The call is refused. The failure count has reached `HARD_CAP`. The tool
    /// must return an error directing the agent to switch tools or stop.
    Refuse { attempt: u32 },
}

/// Tracks repeated identical tool call failures per thread.
///
/// Lives on `Thread` so the state persists across tool calls within a turn
/// (and across turns, since the agent may retry across turn boundaries).
/// Never persisted to DB — lives and dies with the thread.
#[derive(Default)]
pub struct ToolRetryTracker {
    /// `(tool_name, input_hash) → failure_count`
    failures: Mutex<HashMap<(String, u64), u32>>,
}

impl ToolRetryTracker {
    /// Check whether a tool call should be allowed, warned, or refused.
    /// Call this *before* `tool.run()`.
    pub fn check(&self, tool_name: &str, input: &serde_json::Value) -> RetryVerdict {
        let key = (tool_name.to_string(), input_hash(input));
        let failures = self.failures.lock().expect("retry tracker mutex poisoned");
        let count = failures.get(&key).copied().unwrap_or(0);
        if count >= HARD_CAP {
            RetryVerdict::Refuse { attempt: count }
        } else if count >= WARN_THRESHOLD {
            RetryVerdict::AllowWithWarning {
                attempt: count,
                cap: HARD_CAP,
            }
        } else {
            RetryVerdict::Allow
        }
    }

    /// Record that a tool call failed. Call this when `tool.run()` returns `Err`.
    pub fn record_failure(&self, tool_name: &str, input: &serde_json::Value) {
        let key = (tool_name.to_string(), input_hash(input));
        let mut failures = self.failures.lock().expect("retry tracker mutex poisoned");
        *failures.entry(key).or_default() += 1;
    }

    /// Record that a tool call succeeded. Resets the failure counter for this
    /// key — a successful call means the agent is making progress, not stuck.
    pub fn record_success(&self, tool_name: &str, input: &serde_json::Value) {
        let key = (tool_name.to_string(), input_hash(input));
        let mut failures = self.failures.lock().expect("retry tracker mutex poisoned");
        failures.remove(&key);
    }
}

/// Hash a `serde_json::Value` deterministically. Uses `serde_json::to_string`
/// (canonical serialization) then hashes the bytes — this is stable across
/// calls with the same logical input even if the key order in the JSON object
/// differs (serde_json serializes in insertion order, but the agent typically
/// sends the same key order on retries).
fn input_hash(input: &serde_json::Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Ok(bytes) = serde_json::to_vec(input) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
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
    fn warns_after_threshold() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..WARN_THRESHOLD {
            tracker.record_failure("read_file", &input);
        }
        // Next check should warn (at threshold, not yet at cap)
        assert!(matches!(
            tracker.check("read_file", &input),
            RetryVerdict::AllowWithWarning { .. }
        ));
    }

    #[test]
    fn refuses_after_hard_cap() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..HARD_CAP {
            tracker.record_failure("read_file", &input);
        }
        assert!(matches!(
            tracker.check("read_file", &input),
            RetryVerdict::Refuse { .. }
        ));
    }

    #[test]
    fn success_resets_counter() {
        let tracker = ToolRetryTracker::default();
        let input = serde_json::json!({"path": "foo.rs"});
        for _ in 0..WARN_THRESHOLD {
            tracker.record_failure("read_file", &input);
        }
        tracker.record_success("read_file", &input);
        // After success, the counter is reset — next check should Allow.
        assert!(matches!(
            tracker.check("read_file", &input),
            RetryVerdict::Allow
        ));
    }

    #[test]
    fn different_inputs_tracked_separately() {
        let tracker = ToolRetryTracker::default();
        let input_a = serde_json::json!({"path": "a.rs"});
        let input_b = serde_json::json!({"path": "b.rs"});
        for _ in 0..HARD_CAP {
            tracker.record_failure("read_file", &input_a);
        }
        // input_a is refused, but input_b (different input) is still allowed.
        assert!(matches!(
            tracker.check("read_file", &input_a),
            RetryVerdict::Refuse { .. }
        ));
        assert!(matches!(
            tracker.check("read_file", &input_b),
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
        // is still allowed — the tracker keys on (tool_name, input_hash).
        assert!(matches!(
            tracker.check("read_file", &input),
            RetryVerdict::Refuse { .. }
        ));
        assert!(matches!(tracker.check("grep", &input), RetryVerdict::Allow));
    }
}
