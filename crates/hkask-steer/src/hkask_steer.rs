//! hkask-steer — the shared Steer-mode surface for kask panels.
//!
//! Steer mode embeds a curator `ConversationView` in a panel, scoped to the
//! panel's MCP server (`with_mcp_server_scope` enforces, the system prompt
//! declares). Both `kanban_panel` and `swarm_panel` hand-rolled this
//! lifecycle with divergent wiring; this crate is the single deep module
//! that owns it:
//!
//! - `SteerSurface` — lazy construction + invalidation of the conversation.
//!   Invalidation keeps the `AgentConnectionStore` so a rebuilt conversation
//!   reuses the connection.
//! - `verify_tool_advertisement` — the backtick scanner both panels
//!   duplicated as inline `debug_assert!` blocks. Panels keep their
//!   behavioral prose; the MCP server's build.rs-generated `TOOL_NAMES` is
//!   the source of truth for which names may be advertised.
//!
//! The panel's obligation shrinks to: build the prompt, call `ensure` when
//! the Steer tab is shown, call `invalidate` when scope-relevant state
//! (selected board, swarm mode, workspace) changes.

use std::rc::Rc;
use std::sync::Arc;

use agent::{CuratorAgentServer, ThreadStore};
use agent_ui::{Agent, AgentConnectionStore, ConversationView};
use fs::Fs;
use gpui::{App, AppContext, Entity, SharedString, WeakEntity, Window};
use project::Project;
use workspace::Workspace;

pub mod thread_picker;
pub use thread_picker::ThreadPicker;

/// The per-panel inputs a Steer conversation is constructed from.
pub struct SteerContext {
    /// The MCP server id the conversation is scoped to (e.g. `"kanban"`,
    /// `"swarm"`). Must match the server's `ContextServerId`.
    pub server_scope: SharedString,
    /// The panel-owned system prompt (behavioral prose + tool
    /// advertisement). Verify it with `verify_tool_advertisement`.
    pub system_prompt: SharedString,
    pub fs: Arc<dyn Fs>,
    pub project: Entity<Project>,
    pub workspace: WeakEntity<Workspace>,
    /// Resume an existing thread (from the thread database) instead of
    /// starting a fresh one. `None` starts a new thread — the historical
    /// behavior. Set via `open_steer_thread`, not by panels directly.
    pub resume_session_id: Option<agent_client_protocol::schema::v1::SessionId>,
}

impl SteerContext {
    /// Wire the context into a `ConversationView` bound to its curator
    /// server. Panels pass `|ctx, cs, ts, window, cx| ctx.make_view(...)`
    /// to `SteerSurface::ensure`; tests can substitute a stub factory.
    pub fn make_view(
        self,
        connection_store: Entity<AgentConnectionStore>,
        thread_store: Entity<ThreadStore>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<ConversationView> {
        let agent_server = Rc::new(
            CuratorAgentServer::new(self.fs, thread_store.clone())
                .with_extra_static_context(self.system_prompt)
                .with_mcp_server_scope(self.server_scope),
        );
        let thread_id = agent_ui::ThreadId::new();
        let resume_session_id = self.resume_session_id.clone();
        // A resumed thread reuses its stored session; only a fresh thread
        // needs a newly minted id.
        let new_thread_id = if resume_session_id.is_some() {
            None
        } else {
            Some(thread_id)
        };
        cx.new(|cx| {
            ConversationView::new(
                agent_server,
                connection_store,
                Agent::Curator,
                resume_session_id,
                new_thread_id,
                None,
                None,
                None,
                self.workspace,
                self.project,
                Some(thread_store),
                agent_ui::AgentThreadSource::AgentPanel,
                window,
                cx,
            )
        })
    }
}

/// Owns the Steer-mode `ConversationView` lifecycle for a panel.
#[derive(Default)]
pub struct SteerSurface {
    conversation: Option<Entity<ConversationView>>,
    connection_store: Option<Entity<AgentConnectionStore>>,
}

impl SteerSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// The live conversation, if one has been constructed.
    pub fn conversation(&self) -> Option<&Entity<ConversationView>> {
        self.conversation.as_ref()
    }

    /// Drop the conversation so the next `ensure` rebuilds it. Call this
    /// when scope-relevant panel state changes (selected board, swarm
    /// backend mode, active workspace) — the system prompt is baked at
    /// construction, so a stale conversation would steer against the
    /// previous state. The connection store survives invalidation.
    pub fn invalidate(&mut self) {
        self.conversation = None;
    }

    /// Lazily construct the conversation. No-op if one is already live.
    /// The `make` closure receives (context, connection_store, thread_store,
    /// window, cx) and must return the conversation entity — production
    /// panels use `SteerContext::make_view`; the hook keeps the lifecycle
    /// (invalidation reusing `connection_store`) testable without a real
    /// `ConversationView`.
    pub fn ensure(
        &mut self,
        make: impl FnOnce(
            SteerContext,
            Entity<AgentConnectionStore>,
            Entity<ThreadStore>,
            &mut Window,
            &mut App,
        ) -> Entity<ConversationView>,
        context: SteerContext,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.conversation.is_some() {
            return;
        }
        let thread_store = ThreadStore::global(cx);
        let connection_store = self
            .connection_store
            .get_or_insert_with(|| {
                cx.new(|cx| AgentConnectionStore::new(context.project.clone(), cx))
            })
            .clone();
        self.conversation = Some(make(context, connection_store, thread_store, window, cx));
    }
}

/// One-call helper combining `verify_tool_advertisement` and
/// `SteerSurface::ensure`. A panel's Steer construction shrinks to building
/// the prompt and calling this — the shared surface caches and verifies.
pub fn ensure_steer(
    surface: &mut SteerSurface,
    context: SteerContext,
    server_tools: &[&str],
    prefixes: &[&str],
    window: &mut Window,
    cx: &mut App,
) {
    verify_tool_advertisement(&context.system_prompt, server_tools, prefixes);
    surface.ensure(SteerContext::make_view, context, window, cx);
}

/// Open an existing thread (by session id, from the thread database) in the
/// panel's Steer surface, replacing the live conversation. The next
/// `ensure` rebuilds the `ConversationView` resuming that thread's history.
/// Panels call this from their thread-picker callback; anything observing the
/// previous conversation's thread entity (e.g. the media panel's viewer
/// ingest) must drop its observation so the next render re-wires it.
pub fn open_steer_thread(
    surface: &mut SteerSurface,
    mut context: SteerContext,
    server_tools: &[&str],
    prefixes: &[&str],
    session_id: agent_client_protocol::schema::v1::SessionId,
    window: &mut Window,
    cx: &mut App,
) {
    context.resume_session_id = Some(session_id);
    surface.invalidate();
    ensure_steer(surface, context, server_tools, prefixes, window, cx);
}

/// Extract the backticked tool names carrying one of `prefixes` from a
/// Steer prompt. Backtick spans are the prompt's tool-advertisement
/// convention; a name must start with a prefix and have a non-empty suffix.
pub fn advertised_tool_names(prompt: &str, prefixes: &[&str]) -> Vec<String> {
    prompt
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|span| {
            span.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|name| {
            prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix) && name.len() > prefix.len())
        })
        .collect()
}

/// Verify that every tool name a Steer prompt advertises exists in the
/// server's canonical `TOOL_NAMES`. Advertising a tool the server does not
/// expose is worse than omitting it: the model calls a name that cannot
/// resolve and the turn fails at dispatch. Logs a warning always and
/// `debug_assert!`s in dev builds; panels should also pin this with a test.
pub fn verify_tool_advertisement(prompt: &str, server_tools: &[&str], prefixes: &[&str]) {
    let unknown: Vec<String> = advertised_tool_names(prompt, prefixes)
        .into_iter()
        .filter(|name| !server_tools.contains(&name.as_str()))
        .collect();
    for name in &unknown {
        log::warn!(
            "hkask-steer: steer prompt advertises `{name}`, which is not in the \
             MCP server's TOOL_NAMES — the model will hit \"tool not found\" at dispatch"
        );
    }
    debug_assert!(
        unknown.is_empty(),
        "steer prompt advertises tools not exposed by the server: {unknown:?}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_names_extracts_backticked_prefixed_tools() {
        let prompt = "Use `kanban_task_move` and `kanban_task_list`. \
                      The `kata-kanban` server id is not a tool (no prefix match on \
                      the suffix rule applies to `kanban_` alone). \
                      Also `contract_propose_expect` and plain text.";
        let names = advertised_tool_names(prompt, &["kanban_", "contract_"]);
        assert_eq!(
            names,
            vec![
                "kanban_task_move".to_string(),
                "kanban_task_list".to_string(),
                "contract_propose_expect".to_string(),
            ]
        );
    }

    #[test]
    fn advertised_names_rejects_bare_prefix() {
        let names = advertised_tool_names("`kanban_` `kanban_task_create`", &["kanban_"]);
        assert_eq!(names, vec!["kanban_task_create".to_string()]);
    }

    #[test]
    fn verify_accepts_known_tools() {
        verify_tool_advertisement(
            "Use `kanban_task_move`.",
            &["kanban_task_move", "kanban_task_list"],
            &["kanban_"],
        );
    }

    #[test]
    fn verify_warns_on_unknown_tool() {
        // Exercises the log::warn branch; the debug_assert is compiled out
        // under `--release` but this test runs in dev, so we only call it
        // through a catch to keep the test itself from asserting.
        let result = std::panic::catch_unwind(|| {
            verify_tool_advertisement("Use `kanban_nope`.", &["kanban_task_move"], &["kanban_"]);
        });
        assert!(result.is_err(), "debug_assert must fire on unknown tool");
    }
}
