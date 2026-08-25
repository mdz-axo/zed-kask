//! Curator agent server — an overlay on the Zed Agent that adds
//! curator tools and regulatory context.
//!
//! The Curator is NOT a separate agent with its own system prompt. It IS
//! the Zed Agent — same coding tools, same system prompt, same model —
//! PLUS:
//!
//! - **Curator tools**: `curator_status` for checking regulation health
//! - **Curator context**: appended to the system prompt via `static_context`,
//!   describing the Curator's role and current system state
//!
//! The background metacognition loop (sense→compare→compute→act) is spawned
//! once, process-globally, in `crates/zed/src/main.rs` — not by this server.
//! Every Curator thread reads from that shared loop via `CuratorStatusTool`.
//!
//! This overlay design means the Curator can do everything the Zed Agent can
//! (write code, run terminals, edit files) while also having access to the
//! regulatory surface. The user interacts with the Curator exactly as they
//! would with the Zed Agent, but with additional capabilities.

use std::{any::Any, rc::Rc, sync::Arc};

use agent_servers::{AgentServer, AgentServerDelegate};
use anyhow::Result;
use fs::Fs;
use gpui::{App, Entity, SharedString, Task};
use project::{AgentId, Project};

use crate::{CURATOR_AGENT_ID, ThreadStore};

/// The Curator's static context — appended to the system prompt.
///
/// This is NOT a full system prompt override. It's injected via
/// `Thread::static_context` and rendered after the project context section.
/// The Zed Agent's system prompt remains intact — the Curator gets all the
/// coding instructions PLUS this regulatory context.
pub const CURATOR_STATIC_CONTEXT: &str = "\
## Curator Role\n\
\n\
You are also the Curator — the cybernetic regulator for the hKask system.\n\
In addition to your coding agent capabilities, you:\n\
- Monitor system health via the `curator_status` tool\n\
- Apply metacognitive self-calibration when thresholds are breached\n\
- Issue CuratorDirectives via the `curator_directive` tool to adjust\n\
  thresholds, capabilities, and energy budgets\n\
- Evolve MCP tool schemas via the `curator_directive` tool's\n\
  `evolve_mcp_tool_schema` variant — when skill-use reports reveal schema\n\
  mismatches, missing inputs, or confusing output shapes, issue a directive\n\
  to record the evolution request for a developer to act on\n\
- Escalate domain-level concerns to the user for human review\n\
\n\
- Clear reviewed algedonic alerts via the `curator_clear_algedonic_log` tool\n\
  when the `curator_status` tool reports the alert log is approaching its cap.\n\
  This frees the in-memory log before it evicts entries unread. Run the\n\
  `algedonic-review` skill to triage the backlog first.\n\
\n\
### Methodology\n\
\n\
You are anchored on the following methodologies:\n\
- Pragmatic Cybernetics: identify feedback loops, measure variety, assess\n\
  homeostasis. Every system change must have an observable feedback mechanism.\n\
- Pragmatic Semantics: classify every claim by certainty level (IS vs OUGHT).\n\
  Surface unstated assumptions.\n\
- Metacognition: decompose goals, self-assess progress, detect ellipses via\n\
  Bloom's method, calibrate strategy.\n\
- Superforecasting: triage questions into the Goldilocks zone, Fermi-decompose,\n\
  anchor on outside-view base rates, update with Bayesian likelihood ratios.\n\
";

/// Format a compact system-state block from the regulation loop's health
/// snapshot. The block is explicitly labeled as a snapshot so the model
/// knows it's stale at decision time and must pull `curator_status` for live
/// updates. This breaks the naive-realist trap (Dunning, Self-Insight 2005):
/// without the label, the model would treat the static text as complete
/// reality and not seek fresh state.
///
/// The block is compact — only high-signal fields that change the model's
/// regulatory posture: regulation effectiveness, escalation count, critical
/// alerts, memory degradation, alert log cap status.
fn format_state_block(snapshot: &serde_json::Value) -> String {
    let effectiveness = snapshot
        .get("regulation_effectiveness")
        .and_then(|v| v.as_f64())
        .map(|v| format!("{:.0}%", v * 100.0))
        .unwrap_or_else(|| "unavailable".to_string());
    let escalations = snapshot
        .get("escalation_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let critical = snapshot
        .get("critical_alerts")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let memory_degraded = snapshot
        .get("memory")
        .and_then(|m| m.get("degraded"))
        .and_then(|d| d.as_bool())
        .unwrap_or(false);
    let alert_log_count = snapshot
        .get("alert_log_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let alert_log_cap = snapshot
        .get("alert_log_cap")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let alert_log_status = if alert_log_cap > 0 && alert_log_count >= alert_log_cap * 8 / 10 {
        "approaching cap — review needed"
    } else {
        "nominal"
    };

    format!(
        "## Current System State (snapshot at session start — pull curator_status for live updates)\n\
        - Regulation effectiveness: {effectiveness}\n\
        - Escalations (current cycle): {escalations}\n\
        - Critical alerts: {critical}\n\
        - Memory degraded: {memory_degraded}\n\
        - Alert log: {alert_log_count}/{alert_log_cap} ({alert_log_status})"
    )
}

/// The Curator agent server — an overlay on the Zed Agent.
///
/// Like `NativeAgentServer`, but:
/// 1. Injects curator static context into each thread's system prompt
/// 2. Registers the `curator_status` tool on each thread
///
/// The optional `extra_static_context` is appended to
/// `CURATOR_STATIC_CONTEXT` when the connection establishes. This is used by
/// the kask panel to inject a per-tab system prompt describing which MCP
/// server's tools are in scope for the conversation.
#[derive(Clone)]
pub struct CuratorAgentServer {
    fs: Arc<dyn Fs>,
    thread_store: Entity<ThreadStore>,
    extra_static_context: Option<SharedString>,
    /// Per-tab MCP server scope — when set, `connect` applies
    /// `NativeAgent::set_mcp_server_scope`, filtering the thread's
    /// context-server tools to this server only.
    mcp_server_scope: Option<SharedString>,
}

impl CuratorAgentServer {
    pub fn new(fs: Arc<dyn Fs>, thread_store: Entity<ThreadStore>) -> Self {
        Self {
            fs,
            thread_store,
            extra_static_context: None,
            mcp_server_scope: None,
        }
    }

    /// Set extra static context appended to `CURATOR_STATIC_CONTEXT`.
    ///
    /// Used by the kask panel to inject a per-tab system prompt that tells
    /// the curator which MCP server's tools are in scope. The extra context
    /// is rendered after the base curator context, so the curator sees both
    /// its regulatory role AND the per-tab tool scope.
    pub fn with_extra_static_context(mut self, context: SharedString) -> Self {
        self.extra_static_context = Some(context);
        self
    }

    /// Restrict new sessions' MCP tools to one server — the enforcement
    /// half of the per-tab scoping (the prompt is the declaration half).
    ///
    /// The name must match the server's `ContextServerId` (e.g.
    /// `"companies"`). Kask panel passes the tab's server id.
    pub fn with_mcp_server_scope(mut self, server: SharedString) -> Self {
        self.mcp_server_scope = Some(server);
        self
    }
}

impl AgentServer for CuratorAgentServer {
    fn agent_id(&self) -> AgentId {
        CURATOR_AGENT_ID.clone()
    }

    fn logo(&self) -> ui::IconName {
        ui::IconName::ZedAssistant
    }

    fn connect(
        &self,
        _delegate: AgentServerDelegate,
        _project: Entity<Project>,
        cx: &mut App,
    ) -> Task<Result<Rc<dyn acp_thread::AgentConnection>>> {
        let fs = self.fs.clone();
        let thread_store = self.thread_store.clone();
        let extra_context = self.extra_static_context.clone();
        let mcp_server_scope = self.mcp_server_scope.clone();
        cx.spawn(async move |cx| {
            // Build the shared NativeAgent connection, then apply the curator
            // overlay before handing it back. The overlay is the only
            // curator-specific behavior; the spawn sequence is shared with
            // NativeAgentServer via `build_connection` so the two cannot drift.
            let templates = crate::templates::Templates::new();
            let agent = cx.update(|cx| crate::NativeAgent::new(thread_store, templates, fs, cx));

            // S6: Fetch a compact system-state snapshot from the regulation
            // loop and append it to the curator context. This breaks the
            // naive-realist trap (Dunning, Self-Insight 2005): without live
            // state, the static prompt says "monitor system health" but
            // provides no state, so the model anchors on the static text and
            // treats it as complete reality. The label explicitly tells the
            // model this is a snapshot, not complete reality — pull
            // `curator_status` for live updates.
            let state_block = if let Some(provider) =
                crate::metacognition_provider()
            {
                match provider.health_snapshot_json().await {
                    Some(snapshot) => format_state_block(&snapshot),
                    None => String::new(),
                }
            } else {
                String::new()
            };

            cx.update(|cx| {
                agent.update(cx, |agent, _cx| {
                    let mut context = match extra_context {
                        Some(extra) => {
                            SharedString::from(format!("{CURATOR_STATIC_CONTEXT}\n{extra}"))
                        }
                        None => SharedString::from(CURATOR_STATIC_CONTEXT),
                    };
                    if !state_block.is_empty() {
                        context = SharedString::from(format!(
                            "{context}\n\n{state_block}"
                        ));
                    }
                    agent.set_curator_static_context(context);
                    if let Some(scope) = mcp_server_scope {
                        agent.set_mcp_server_scope(scope);
                    }
                });
            });
            Ok(Rc::new(crate::NativeAgentConnection(
                agent,
                crate::CURATOR_AGENT_ID.clone(),
            )) as Rc<dyn acp_thread::AgentConnection>)
        })
    }

    fn into_any(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }
}
