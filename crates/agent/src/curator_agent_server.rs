//! Curator agent server — an overlay on the Zed Agent that adds
//! metacognition, curator tools, and regulatory monitoring.
//!
//! The Curator is NOT a separate agent with its own system prompt. It IS
//! the Zed Agent — same coding tools, same system prompt, same model —
//! PLUS:
//!
//! - **Curator tools**: `curator_status` for checking regulation health
//! - **Curator context**: appended to the system prompt via `static_context`,
//!   describing the Curator's role and current system state
//! - **Background metacognition**: a detached task that runs the
//!   sense→compare→compute→act governance loop
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

use crate::{
    CURATOR_AGENT_ID, NativeAgent, NativeAgentConnection, ThreadStore, templates::Templates,
};

/// The Curator's static context — appended to the system prompt.
///
/// This is NOT a full system prompt override. It's injected via
/// `Thread::static_context` and rendered after the project context section.
/// The Zed Agent's system prompt remains intact — the Curator gets all the
/// coding instructions PLUS this regulatory context.
const CURATOR_STATIC_CONTEXT: &str = "\
## Curator Role\n\
\n\
You are also the Curator — the cybernetic regulator for the hKask system.\n\
In addition to your coding agent capabilities, you:\n\
- Monitor system health via the `curator_status` tool\n\
- Apply metacognitive self-calibration when thresholds are breached\n\
- Issue CuratorDirectives to adjust thresholds, capabilities, and energy budgets\n\
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
/// 3. Runs a background metacognition task
#[derive(Clone)]
pub struct CuratorAgentServer {
    fs: Arc<dyn Fs>,
    thread_store: Entity<ThreadStore>,
}

impl CuratorAgentServer {
    pub fn new(fs: Arc<dyn Fs>, thread_store: Entity<ThreadStore>) -> Self {
        Self { fs, thread_store }
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
        log::debug!("CuratorAgentServer::connect");
        let fs = self.fs.clone();
        let thread_store = self.thread_store.clone();
        cx.spawn(async move |cx| {
            log::debug!("Creating templates for Curator agent");
            let templates = Templates::new();

            log::debug!("Creating native agent entity for Curator");
            let agent = cx.update(|cx| NativeAgent::new(thread_store, templates, fs, cx));

            // Set the Curator static context — this is appended to the system
            // prompt, NOT a full override. The Zed Agent's coding instructions
            // remain intact.
            cx.update(|cx| {
                agent.update(cx, |agent, cx| {
                    agent
                        .set_curator_static_context(SharedString::from(CURATOR_STATIC_CONTEXT), cx);
                });
            });

            // Create the connection wrapper
            let connection = NativeAgentConnection(agent);
            log::debug!("CuratorAgentServer connection established successfully");

            Ok(Rc::new(connection) as Rc<dyn acp_thread::AgentConnection>)
        })
    }

    fn into_any(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }
}
