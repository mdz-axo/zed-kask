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
- Issue CuratorDirectives via the `curator_directive` tool to adjust
  thresholds, capabilities, and energy budgets
- Evolve MCP tool schemas via the `curator_directive` tool's
  `evolve_mcp_tool_schema` variant — when skill-use reports reveal schema
  mismatches, missing inputs, or confusing output shapes, issue a directive
  to record the evolution request for a developer to act on
- Escalate domain-level concerns to the user for human review\n\
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
            cx.update(|cx| {
                agent.update(cx, |agent, _cx| {
                    let context = match extra_context {
                        Some(extra) => {
                            SharedString::from(format!("{CURATOR_STATIC_CONTEXT}\n{extra}"))
                        }
                        None => SharedString::from(CURATOR_STATIC_CONTEXT),
                    };
                    agent.set_curator_static_context(context);
                    if let Some(scope) = mcp_server_scope {
                        agent.set_mcp_server_scope(scope);
                    }
                });
            });
            Ok(Rc::new(crate::NativeAgentConnection(agent)) as Rc<dyn acp_thread::AgentConnection>)
        })
    }

    fn into_any(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }
}
