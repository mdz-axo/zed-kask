//! Curator agent server — an overlay on the Zed Agent that adds
//! curator tools and regulatory context.
//!
//! The Curator is NOT a separate agent with its own system prompt. It IS
//! the Zed Agent — same coding tools, same system prompt, same model —
//! PLUS:
//!
//! - **Shared read tool**: native and Curator sessions both use the single
//!   `curator_status` implementation for regulation health
//! - **Curator action tools**: directives and explicit log maintenance
//! - **Curator context**: appended to the system prompt via `static_context`,
//!   describing the Curator's role and current system state
//!
//! The background metacognition loop (sense→compare→compute→act) is spawned
//! once, process-globally, in `crates/zed/src/main.rs` — not by this server.
//! Every native and Curator thread reads from that shared loop via the same
//! `CuratorStatusTool`; this overlay does not register a second status tool.
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
///
/// The `### Learning loop` block states the Curator's part in the skill
/// learning loop; the loop itself is documented and diagrammed as
/// `DIAG-ARCH-LEARNING-LOOP-001` in `kask/docs/diagrams/architecture.md`.
/// Keep the two in step.
pub const CURATOR_STATIC_CONTEXT: &str = "\
## Curator Role\n\
\n\
You are also the Curator — the cybernetic regulator for the hKask system.\n\
In addition to your coding agent capabilities, you:\n\
- Monitor system health via the `curator_status` tool\n\
- Issue CuratorDirectives via the `curator_directive` tool to adjust\n\
  thresholds, capabilities, and energy budgets\n\
- Evolve MCP tool schemas via the `curator_directive` tool's\n\
  `evolve_mcp_tool_schema` variant — when skill-use reports reveal schema\n\
  mismatches, missing inputs, or confusing output shapes, issue a directive\n\
  to record the request, then hand it to an agent: create a kanban task with\n\
  the request, its evidence and verification criteria, and delegate it\n\
- Escalate domain-level concerns to the user for human review\n\
\n\
### Learning loop\n\
\n\
Learning capture is always on. Every skill activation, by you or by the\n\
Z-K agent, is recorded automatically with who ran it, and every tool\n\
failure while a skill is active is recorded against that skill. Add the\n\
classified signals yourself, without being asked:\n\
- A skill or tool that misbehaves → `curator_report_skill_use_issue` with\n\
  its `failure_origin`\n\
- A durable lesson with evidence → `memory_insert` citing the evidence h_mem;\n\
  contradictions and reification into skills, templates or rules → `therapy`\n\
- A skill that needs changing → a `skill-maintenance` proposal\n\
- Proposals are decided in the `algedonic-review` gemba walk by the user,\n\
  or by you under a recorded user grant. Accepted work goes to a delegated\n\
  agent, and nothing is done until it is verified, unless the user or you\n\
  explicitly skip verification.\n\
\n\
- Review pending escalations with `algedonic-review` when the status tool\n\
  reports cap pressure; the in-memory log self-evicts. Do not clear it as\n\
  an outcome of that review. Any separate log-maintenance action requires\n\
  an explicit operator decision.\n\
\n\
Your methods are skills: `pragmatic-cybernetics`, `pragmatic-semantics`,\n\
`metacognition` and `superforecasting`. Load the one the situation needs.\n\
\n\
### Values grounding\n\
\n\
P1-P12 are grounded in the dignity of the human person — the values charter\n\
is `kask/docs/architecture/core/magnifica-humanitas.md`, distilled from\n\
*Magnifica Humanitas* (Leo XIV, 2026). Its eight values: V1 ontological\n\
dignity, V2 truth as a common good, V3 accountability, V4 subsidiarity,\n\
V5 non-neutrality, V6 the dignity of work, V7 the limit as positive, V8\n\
universal destination of goods and solidarity. Its discernment test, from\n\
the encyclical's own closing question: does this make human life more\n\
human? Before a value-laden decision (user-facing surfaces, tradeoffs\n\
between performance and the person), pull the charter via\n\
`curator_federated_search` and answer to that test.\n\
";

/// The user–Curator dyad level as session guidance, appended to the Curator
/// overlay for each new Curator thread (`NativeAgent::new_session`). The level
/// is the user's setting (`kask.curator.interaction_mode`); the semantics are
/// documented in `kask/docs/architecture/functional-interaction-spec.md` §9.
/// Keep the two in step.
pub fn interaction_mode_context(mode: settings::CuratorInteractionMode) -> &'static str {
    use settings::CuratorInteractionMode::*;
    match mode {
        Control => {
            "\
### Interaction mode: Level 1 — control\n\
\n\
The user is the controller and regulator of this work. You propose; the user\n\
decides. Every proposal, `curator_directive`, skill change and algedonic\n\
review decision waits for the user's explicit decision — do not act under\n\
standing grants. Present options with their evidence and consequences, and\n\
give a recommendation when asked. Keep recording learning signals (issue\n\
reports, evidenced lessons): they are observations, not decisions.\n\
\n\
The user sets this level in Settings > Kask > Curator. Never change it.\n\
"
        }
        Collaboration => {
            "\
### Interaction mode: Level 2 — collaboration\n\
\n\
You and the user work toward shared understanding and shared solutions.\n\
Restate the user's goal as an interpretation for them to correct; give your\n\
recommendation with its reasoning; decide functional questions together and\n\
technical ones yourself, with their functional consequence. In\n\
`algedonic-review`, reach decisions jointly and record the shared reasoning\n\
on the card. Act under a user grant only when it is recorded on the card.\n\
\n\
The user sets this level in Settings > Kask > Curator. Never change it.\n\
"
        }
        LearningCollaboration => {
            "\
### Interaction mode: Level 3 — learning collaboration\n\
\n\
Everything in Level 2 (shared understanding, joint decisions recorded on the\n\
card, grants only when recorded), plus learning in both directions:\n\
- Challenge the user when evidence disagrees with a premise or decision:\n\
  name the evidence and what would change your view. Do not smooth a\n\
  disagreement over; record it and its resolution on the card or in memory.\n\
- Invite the user's challenges. Treat a correction as evidence about your\n\
  own method: record the lesson with `memory_insert` and propose method\n\
  changes through `skill-maintenance` or `therapy`.\n\
- Share what the user can learn from you: your calibration record (goal\n\
  Brier scores), your uncertainty, and the reasoning behind a recommendation.\n\
- End each bit of work by naming what each side learned.\n\
\n\
The user sets this level in Settings > Kask > Curator. Never change it; you\n\
may argue for a different level, with evidence.\n\
"
        }
    }
}

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
    let acceptance_rate = snapshot
        .get("regulation_acceptance_rate")
        .and_then(|v| v.as_f64())
        .map(|v| format!("{:.0}%", v * 100.0))
        .unwrap_or_else(|| "unavailable".to_string());
    let measured_count = |field| {
        snapshot
            .get(field)
            .and_then(|value| value.as_u64())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unavailable".to_string())
    };
    let escalations = measured_count("escalation_count");
    let pending_escalations = measured_count("pending_escalations");
    let critical = measured_count("critical_alerts");
    let memory_degraded = snapshot
        .get("memory")
        .and_then(|memory| memory.get("degraded"))
        .and_then(|value| value.as_bool())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let alert_log = match (
        snapshot
            .get("alert_log_count")
            .and_then(|value| value.as_u64()),
        snapshot
            .get("alert_log_cap")
            .and_then(|value| value.as_u64()),
    ) {
        (Some(count), Some(cap)) => format!("{count}/{cap}"),
        _ => "unavailable".to_string(),
    };
    let alert_log_status = match snapshot
        .get("alert_log_approaching_cap")
        .and_then(|value| value.as_bool())
    {
        Some(true) => "approaching cap",
        Some(false) => "not approaching cap",
        None => "unavailable",
    };
    let loop_reading = snapshot
        .get("loop_reading")
        .and_then(|value| value.as_str())
        .unwrap_or("unavailable");

    format!(
        "## Current System State (snapshot at session start — pull curator_status for live updates)\n\
        - Regulation acceptance rate: {acceptance_rate}\n\
        - Escalations (current cycle): {escalations}\n\
        - Algedonic review cards not Done: {pending_escalations}\n\
        - Critical alerts: {critical}\n\
        - Memory degraded: {memory_degraded}\n\
        - Alert log: {alert_log} ({alert_log_status})\n\
        - Loop reading: {loop_reading}"
    )
}

/// The Curator agent server — an overlay on the Zed Agent.
///
/// Like `NativeAgentServer`, but:
/// 1. Injects curator static context into each thread's system prompt
/// 2. Adds Curator-only action tools; `curator_status` is already registered
///    once on every native session by `NativeAgent::new_session`.
///
/// The optional `extra_static_context` is appended to
/// `CURATOR_STATIC_CONTEXT` when the connection establishes. This is used by
/// kask panels to inject a per-panel system prompt describing the panel's
/// workflow while preserving the full cross-domain MCP tool surface.
#[derive(Clone)]
pub struct CuratorAgentServer {
    fs: Arc<dyn Fs>,
    thread_store: Entity<ThreadStore>,
    extra_static_context: Option<SharedString>,
}

impl CuratorAgentServer {
    pub fn new(fs: Arc<dyn Fs>, thread_store: Entity<ThreadStore>) -> Self {
        Self {
            fs,
            thread_store,
            extra_static_context: None,
        }
    }

    /// Set extra static context appended to `CURATOR_STATIC_CONTEXT`.
    ///
    /// Used by kask panels to inject workflow context. The extra context is
    /// rendered after the base curator context, so the curator sees both its
    /// regulatory role and the panel-specific task framing.
    pub fn with_extra_static_context(mut self, context: SharedString) -> Self {
        self.extra_static_context = Some(context);
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
            let state_block = if let Some(provider) = crate::metacognition_provider() {
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
                        context = SharedString::from(format!("{context}\n\n{state_block}"));
                    }
                    agent.set_curator_static_context(context);
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

#[cfg(test)]
mod interaction_mode_tests {
    use super::*;
    use settings::CuratorInteractionMode::*;

    #[test]
    fn each_level_gives_the_curator_distinct_guidance() {
        let control = interaction_mode_context(Control);
        let collaboration = interaction_mode_context(Collaboration);
        let learning = interaction_mode_context(LearningCollaboration);
        assert!(control.contains("Level 1") && control.contains("do not act under"));
        assert!(collaboration.contains("Level 2") && collaboration.contains("jointly"));
        assert!(learning.contains("Level 3") && learning.contains("Challenge the user"));
        for text in [control, collaboration, learning] {
            assert!(text.contains("Never change it"));
            assert!(!text.contains("Level 4"));
        }
    }
}

#[cfg(test)]
mod status_snapshot_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_state_does_not_turn_missing_measurements_into_health() {
        let state = format_state_block(&json!({}));
        assert!(state.contains("Regulation acceptance rate: unavailable"));
        assert!(state.contains("Escalations (current cycle): unavailable"));
        assert!(state.contains("Algedonic review cards not Done: unavailable"));
        assert!(state.contains("Critical alerts: unavailable"));
        assert!(state.contains("Memory degraded: unavailable"));
        assert!(state.contains("Alert log: unavailable (unavailable)"));
        assert!(state.contains("Loop reading: unavailable"));
        assert!(!state.contains("nominal"));
    }

    #[test]
    fn session_state_displays_the_providers_actual_readings() {
        let state = format_state_block(&json!({
            "regulation_acceptance_rate": 0.75,
            "escalation_count": 0,
            "critical_alerts": 0,
            "memory": {"degraded": false},
            "alert_log_count": 199,
            "alert_log_cap": 200,
            "alert_log_approaching_cap": true,
            "loop_reading": "turning"
        }));
        assert!(state.contains("Regulation acceptance rate: 75%"));
        assert!(state.contains("Memory degraded: false"));
        assert!(state.contains("Alert log: 199/200 (approaching cap)"));
        assert!(state.contains("Loop reading: turning"));
    }
}

#[cfg(test)]
mod values_grounding_tests {
    use super::*;

    /// The Curator's static context carries the Magnifica Humanitas values
    /// charter's eight values, its discernment test, and the standing pull
    /// instruction (investigation report 2026-10-08, recommendation R1) —
    /// and the pre-existing Role and Learning-loop blocks survive the
    /// addition.
    #[test]
    fn curator_static_context_carries_values_grounding() {
        assert!(CURATOR_STATIC_CONTEXT.contains("### Values grounding"));
        assert!(CURATOR_STATIC_CONTEXT.contains("V1 ontological"));
        assert!(CURATOR_STATIC_CONTEXT.contains("V8"));
        assert!(CURATOR_STATIC_CONTEXT.contains("does this make human life more"));
        assert!(CURATOR_STATIC_CONTEXT.contains("human? Before a value-laden decision"));
        assert!(CURATOR_STATIC_CONTEXT.contains("curator_federated_search"));
        assert!(CURATOR_STATIC_CONTEXT.contains("## Curator Role"));
        assert!(CURATOR_STATIC_CONTEXT.contains("### Learning loop"));
        assert!(CURATOR_STATIC_CONTEXT.contains("pragmatic-cybernetics"));
    }
}
