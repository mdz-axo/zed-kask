//! Runtime policy enforcement for agent actions.
//!
//! Source: VeriGuard pattern (Zylos Research) + AgentGuard (arXiv:2509.23864)
//!
//! Before each tool invocation, the proposed action is checked against a
//! runtime policy. This is Layer 6 (runtime monitoring) in the defense-in-depth
//! stack.

use hkask_types::tool_taint::ToolTaint;
use std::collections::HashSet;

/// Verdict from the runtime policy check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// Action is allowed — proceed.
    Allow,
    /// Action is blocked — do not execute.
    Block(String),
    /// Action requires human confirmation before proceeding.
    RequireHuman(String),
    /// Action is allowed but logged for monitoring.
    Log(String),
}

/// Configuration for the default runtime policy.
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// Tools that require human confirmation (by name).
    pub human_in_loop_tools: HashSet<String>,
    /// Maximum actions per session before rate limiting.
    pub max_actions_per_session: u64,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            human_in_loop_tools: HashSet::new(),
            max_actions_per_session: 1000,
        }
    }
}

/// Runtime policy trait — checks proposed actions before execution.
///
/// Implementations enforce taint flow rules, rate limiting, human-in-the-loop
/// gates, and action distribution shift detection.
pub trait RuntimePolicy: Send + Sync {
    /// Check a proposed action against the policy.
    ///
    /// expect: "The system checks every proposed tool invocation before execution"
    /// pre:  tool_name is the tool being invoked
    ///       tool_taint is the FIDES taint label of the tool
    ///       has_untrusted_input indicates whether any input arguments carry untrusted data
    ///       action_number is the action's position in the session trajectory (1-based)
    /// post: returns Allow, Block, RequireHuman, or Log verdict
    fn check(
        &self,
        tool_name: &str,
        tool_taint: ToolTaint,
        has_untrusted_input: bool,
        action_number: u64,
    ) -> PolicyVerdict;
}

/// Default policy: enforces FIDES taint flow rules + rate limiting.
pub struct DefaultPolicy {
    config: PolicyConfig,
}

impl DefaultPolicy {
    /// Create a new default policy with the given configuration.
    ///
    /// expect: "The system constructs a runtime policy from explicit configuration"
    /// post: returns a DefaultPolicy holding the supplied config
    pub fn new(config: PolicyConfig) -> Self {
        Self { config }
    }
}

impl Default for DefaultPolicy {
    fn default() -> Self {
        Self::new(PolicyConfig::default())
    }
}

impl RuntimePolicy for DefaultPolicy {
    fn check(
        &self,
        tool_name: &str,
        tool_taint: ToolTaint,
        has_untrusted_input: bool,
        action_number: u64,
    ) -> PolicyVerdict {
        // Rule 1: Human-in-the-loop tools require confirmation.
        if self.config.human_in_loop_tools.contains(tool_name) {
            return PolicyVerdict::RequireHuman(format!(
                "Tool '{}' requires human confirmation",
                tool_name
            ));
        }

        // Rule 2: FIDES taint flow — untrusted input to Sink is blocked.
        if tool_taint == ToolTaint::Sink && has_untrusted_input {
            return PolicyVerdict::Block(format!(
                "Untrusted data cannot flow to Sink tool '{}' without Endorser",
                tool_name
            ));
        }

        // Rule 3: Rate limiting.
        if action_number > self.config.max_actions_per_session {
            return PolicyVerdict::Block(format!(
                "Session exceeded max actions ({})",
                self.config.max_actions_per_session
            ));
        }

        // Rule 4: Source tools returning untrusted data are logged.
        if tool_taint == ToolTaint::Source {
            return PolicyVerdict::Log(format!(
                "Source tool '{}' returning untrusted data",
                tool_name
            ));
        }

        PolicyVerdict::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_with_human_tool(tool: &str) -> DefaultPolicy {
        let mut human = HashSet::new();
        human.insert(tool.to_string());
        DefaultPolicy::new(PolicyConfig {
            human_in_loop_tools: human,
            max_actions_per_session: 1000,
        })
    }

    #[test]
    fn rule1_human_in_loop_requires_confirmation() {
        let p = policy_with_human_tool("delete_file");
        let v = p.check("delete_file", ToolTaint::Sink, false, 1);
        assert!(matches!(v, PolicyVerdict::RequireHuman(_)));
    }

    #[test]
    fn rule1_human_in_loop_takes_precedence_over_taint() {
        // Even with untrusted input to a Sink, human-in-loop fires first.
        let p = policy_with_human_tool("dangerous_tool");
        let v = p.check("dangerous_tool", ToolTaint::Sink, true, 1);
        assert!(matches!(v, PolicyVerdict::RequireHuman(_)));
    }

    #[test]
    fn rule2_untrusted_to_sink_blocked() {
        let p = DefaultPolicy::default();
        let v = p.check("write_file", ToolTaint::Sink, true, 1);
        match v {
            PolicyVerdict::Block(msg) => {
                assert!(msg.contains("write_file"));
                assert!(msg.contains("Endorser"));
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn rule2_trusted_to_sink_allowed() {
        let p = DefaultPolicy::default();
        let v = p.check("write_file", ToolTaint::Sink, false, 1);
        assert_eq!(v, PolicyVerdict::Allow);
    }

    #[test]
    fn rule3_rate_limit_blocks() {
        let p = DefaultPolicy::new(PolicyConfig {
            human_in_loop_tools: HashSet::new(),
            max_actions_per_session: 5,
        });
        let v = p.check("pure_tool", ToolTaint::Pure, false, 6);
        assert!(matches!(v, PolicyVerdict::Block(_)));
    }

    #[test]
    fn rule3_at_limit_allowed() {
        let p = DefaultPolicy::new(PolicyConfig {
            human_in_loop_tools: HashSet::new(),
            max_actions_per_session: 5,
        });
        let v = p.check("pure_tool", ToolTaint::Pure, false, 5);
        assert_eq!(v, PolicyVerdict::Allow);
    }

    #[test]
    fn rule4_source_logged() {
        let p = DefaultPolicy::default();
        let v = p.check("fetch_url", ToolTaint::Source, false, 1);
        assert!(matches!(v, PolicyVerdict::Log(_)));
    }

    #[test]
    fn pure_tool_allowed() {
        let p = DefaultPolicy::default();
        let v = p.check("to_uppercase", ToolTaint::Pure, false, 1);
        assert_eq!(v, PolicyVerdict::Allow);
    }

    #[test]
    fn endorser_allowed() {
        let p = DefaultPolicy::default();
        let v = p.check("extract_entities", ToolTaint::Endorser, true, 1);
        assert_eq!(v, PolicyVerdict::Allow);
    }

    #[test]
    fn config_defaults() {
        let c = PolicyConfig::default();
        assert!(c.human_in_loop_tools.is_empty());
        assert_eq!(c.max_actions_per_session, 1000);
    }

    #[test]
    fn default_policy_default_matches_config_default() {
        let p = DefaultPolicy::default();
        // Pure tool, no untrusted input, action 1 → Allow.
        assert_eq!(
            p.check("pure", ToolTaint::Pure, false, 1),
            PolicyVerdict::Allow
        );
    }

    #[test]
    fn allow_verdict_has_no_payload() {
        // Allow is a unit variant — no message.
        assert_eq!(PolicyVerdict::Allow, PolicyVerdict::Allow);
    }

    #[test]
    fn block_carries_message() {
        let p = DefaultPolicy::default();
        if let PolicyVerdict::Block(msg) = p.check("s", ToolTaint::Sink, true, 1) {
            assert!(!msg.is_empty());
        } else {
            panic!("expected Block");
        }
    }

    #[test]
    fn require_human_carries_message() {
        let p = policy_with_human_tool("x");
        if let PolicyVerdict::RequireHuman(msg) = p.check("x", ToolTaint::Pure, false, 1) {
            assert!(msg.contains("x"));
        } else {
            panic!("expected RequireHuman");
        }
    }

    #[test]
    fn log_carries_message() {
        let p = DefaultPolicy::default();
        if let PolicyVerdict::Log(msg) = p.check("src", ToolTaint::Source, false, 1) {
            assert!(msg.contains("src"));
        } else {
            panic!("expected Log");
        }
    }
}
