//! Test utilities for TUI rendering regression tests.
//!
//! Provides a minimal mock bridge implementing both `SystemBridge`
//! and `ReplBridge` so workspace rendering tests can run without
//! the full agent service stack.

use std::sync::Arc;

use crate::tui::repl_bridge::{
    InferenceRequestId, InferenceState, McpInvokeRequestId, McpInvokeState, ModelSwitchResult,
    ReplBridge, SessionBridge, SettingsBridge, SystemBridge, ToolInvokeBridge, TuiModelInfo,
};

/// A minimal mock bridge that returns defaults for all methods.
/// Used in rendering guard tests where only the render pipeline matters.
pub(crate) struct MockReplBridge {
    pub agent_name: String,
    pub model_name: String,
}

impl SystemBridge for MockReplBridge {
    fn userpod_name(&self) -> &str {
        &self.agent_name
    }
    fn model_name(&self) -> &str {
        &self.model_name
    }
    fn gas_remaining(&self) -> u64 {
        10_000
    }
    fn gas_cap(&self) -> u64 {
        10_000
    }
    fn reg_alert_count(&self) -> u32 {
        0
    }
    fn context_pressure(&self) -> f64 {
        0.0
    }
    fn mcp_status(&self) -> (usize, usize) {
        (0, 0)
    }
    fn pod_counts(&self) -> Option<(usize, usize)> {
        Some((1, 1))
    }
    fn reg_domains(&self) -> Vec<(String, bool)> {
        Vec::new()
    }
}

impl ReplBridge for MockReplBridge {
    fn start_inference(&self, _input: String) -> InferenceRequestId {
        InferenceRequestId::new()
    }
    fn poll_inference(&self, _request: InferenceRequestId) -> InferenceState {
        InferenceState::Idle
    }
    fn streaming_text(&self, _request: InferenceRequestId) -> String {
        String::new()
    }
    fn send_curator_message(&self, _input: &str) -> String {
        String::new()
    }
}

impl SettingsBridge for MockReplBridge {
    fn set_model(&self, name: &str) -> ModelSwitchResult {
        ModelSwitchResult {
            resolved_name: name.to_string(),
            detail: String::new(),
        }
    }
    fn list_models(&self) -> anyhow::Result<Vec<TuiModelInfo>> {
        Ok(Vec::new())
    }
    fn settings_display(&self) -> String {
        "(settings unavailable in test mock)".to_string()
    }
    fn set_setting(&self, key: &str, value: &str) -> anyhow::Result<String> {
        // Mirror the real `apply_setting` confirmation so tests can verify the
        // key/value were passed through the bridge wiring correctly.
        Ok(format!("{} set to {}", key, value))
    }
}

/// Create a mock bridge for tests. Returns both system and repl Arcs
/// backed by the same MockReplBridge instance.
pub(crate) fn mock_bridges() -> (Arc<dyn SystemBridge>, Arc<dyn ReplBridge>) {
    let bridge = Arc::new(MockReplBridge {
        agent_name: "test-agent".to_string(),
        model_name: "test-model".to_string(),
    });
    let system: Arc<dyn SystemBridge> = bridge.clone();
    let repl: Arc<dyn ReplBridge> = bridge;
    (system, repl)
}

/// A mock `SettingsBridge` for tests that exercise `/model` or `/repl`.
pub(crate) fn mock_settings_bridge() -> Arc<dyn SettingsBridge> {
    Arc::new(MockReplBridge {
        agent_name: "test-agent".to_string(),
        model_name: "test-model".to_string(),
    })
}

impl SessionBridge for MockReplBridge {
    fn current_agent(&self) -> String {
        self.agent_name.clone()
    }
    fn list_agents_display(&self) -> String {
        "(mock agents)".to_string()
    }
    fn history_display(&self) -> String {
        "(mock history)".to_string()
    }
}

impl ToolInvokeBridge for MockReplBridge {
    fn start_mcp_tool_invoke(
        &self,
        _server: &str,
        _tool: &str,
        _args: serde_json::Value,
    ) -> McpInvokeRequestId {
        McpInvokeRequestId::new()
    }
    fn poll_mcp_tool_invoke(&self, _request: McpInvokeRequestId) -> McpInvokeState {
        McpInvokeState::Idle
    }
}

/// A mock `SessionBridge` for tests that exercise `/agent`, `/agents`, `/history`.
pub(crate) fn mock_session_bridge() -> Arc<dyn SessionBridge> {
    Arc::new(MockReplBridge {
        agent_name: "test-agent".to_string(),
        model_name: "test-model".to_string(),
    })
}
