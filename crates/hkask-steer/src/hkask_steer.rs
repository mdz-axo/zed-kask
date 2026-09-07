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
//! - tool-advertisement rendering + `verify_tool_advertisement` — the
//!   zed-free prompt-truth logic, re-exported from `hkask-steer-core` (std
//!   + `log` only, split 2026-09-07 so it builds and tests without the
//!   editor closure). The MCP server's build.rs-generated `TOOL_NAMES` is
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

// The zed-free half of the Steer prompt surface (tool-advertisement
// rendering + verification), re-exported so panel call sites
// (`hkask_steer::render_tool_names` etc.) are unchanged by the split.
pub use hkask_steer_core::{
    advertised_tool_names, render_grouped_tool_advertisement, render_tool_names,
    verify_tool_advertisement,
};

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
        let resume_session_id = self.resume_session_id.clone();
        // A resumed thread must reuse its stored ThreadId: ConversationView
        // mints a fresh one when `thread_id` is None, and ThreadMetadataStore
        // keys sidebar rows by ThreadId — a new id here duplicates the thread
        // in the sidebar (original row + resumed row, same title). When the
        // session is unknown to the metadata store, fall back to a fresh id.
        let new_thread_id = if let Some(session_id) = &resume_session_id {
            agent_ui::thread_metadata_store::ThreadMetadataStore::global(cx)
                .read(cx)
                .entry_by_session(session_id)
                .map(|metadata| metadata.thread_id)
                .unwrap_or_else(agent_ui::ThreadId::new)
        } else {
            agent_ui::ThreadId::new()
        };
        cx.new(|cx| {
            ConversationView::new(
                agent_server,
                connection_store,
                Agent::Curator,
                resume_session_id,
                Some(new_thread_id),
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
