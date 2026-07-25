//! Curator agent server — a `NativeAgentServer` variant that sets a
//! Curator-specific system prompt on each new thread.
//!
//! The Curator is the cybernetic regulator agent (D2). It uses the same
//! `NativeAgent` infrastructure as the Zed Agent, but with a distinct
//! persona/system prompt that reflects its role as a metacognitive
//! observer and regulator.
//!
//! When fusion is enabled, the Curator's threads use the fusion model
//! (the composition root sets the default model to the `FusionLanguageModel`
//! when `kask.fusion.enabled == true`).

use std::{any::Any, rc::Rc, sync::Arc};

use agent_servers::{AgentServer, AgentServerDelegate};
use anyhow::Result;
use fs::Fs;
use gpui::{App, Entity, SharedString, Task};
use project::{AgentId, Project};

use crate::{
    NativeAgent, NativeAgentConnection, ThreadStore, CURATOR_AGENT_ID, templates::Templates,
};

/// The Curator's system prompt.
///
/// Defines the Curator's persona as a cybernetic regulator: it observes
/// system state, identifies quality threats, escalates when thresholds
/// are breached, and applies metacognitive self-calibration. The prompt
/// is intentionally concise — the Curator's tools (when wired) provide
/// the structured surface for regulation actions.
const CURATOR_SYSTEM_PROMPT: &str = "\
You are the Curator — the cybernetic regulator agent for the hKask system.\n\
You observe system state, identify quality threats, and regulate agent behavior\n\
through cybernetic feedback loops.\n\
\n\
## Your Role\n\
\n\
You are the user's counterpart in the regulatory domain. Your job is not to\n\
write code directly, but to:\n\
- Monitor system health (variety, regulation effectiveness, escalation queues)\n\
- Identify quality threats and surface them with severity and evidence\n\
- Apply metacognitive self-calibration when thresholds are breached\n\
- Issue CuratorDirectives to adjust thresholds, capabilities, and energy budgets\n\
- Escalate domain-level concerns to the user for human review\n\
\n\
## Communication\n\
\n\
- Be concise and direct. Surface the signal, not the noise.\n\
- When escalating, state the trigger, the threshold, the current value, and the\n\
  recommended action.\n\
- Use calibrated probability ranges, not binary predictions.\n\
- Ground claims in observable system state (Regulation spans, variety counters,\n\
  escalation queues). Do not fabricate metrics.\n\
\n\
## Methodology\n\
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
\n\
## Today's Date\n\
\n\
{date}\n\
";

/// The Curator agent server.
///
/// Like `NativeAgentServer`, but sets the Curator system prompt on each
/// new thread via `Thread::set_system_prompt_override`.
#[derive(Clone)]
pub struct CuratorAgentServer {
    fs: Arc<dyn Fs>,
    thread_store: Entity<ThreadStore>,
}

impl CuratorAgentServer {
    pub fn new(fs: Arc<dyn Fs>, thread_store: Entity<ThreadStore>) -> Self {
        Self { fs, thread_store }
    }

    /// Render the Curator system prompt with the current date.
    fn render_system_prompt() -> SharedString {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        CURATOR_SYSTEM_PROMPT.replace("{date}", &date).into()
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

            // Set the Curator system prompt on the agent's thread factory.
            // The NativeAgent creates threads via `new_session` — we need
            // to intercept thread creation to set the override. The cleanest
            // way: set a global hook that the agent checks when creating
            // new sessions.
            //
            // For now, we set the override on the agent entity itself via
            // a dedicated method. The NativeAgent will apply it to each
            // new thread.
            let curator_prompt = Self::render_system_prompt();
            cx.update(|cx| {
                agent.update(cx, |agent, cx| {
                    agent.set_system_prompt_override(curator_prompt, cx);
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
