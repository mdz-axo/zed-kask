#![forbid(unsafe_code)]
//! hkask-steer — the shared Steer-mode surface for kask panels.
//!
//! Steer mode embeds a curator `ConversationView` in a panel with the full
//! cross-domain MCP surface. The panel prompt supplies workflow context but
//! does not restrict which servers the conversation can use. This crate is the
//! single deep module that owns that lifecycle across `kanban_panel`,
//! `media_panel`, and `nostr_panel`:
//!
//! - `SteerSurface` — lazy construction, resume staging, active-thread
//!   observation, and invalidation of the conversation. Invalidation keeps the
//!   `AgentConnectionStore` so a rebuilt conversation reuses the connection.
//! - `vertical_split` — shared draggable viewer-above-director split layout
//!   and state for `media_panel` and `nostr_panel`.
//! - tool-advertisement rendering + `verify_tool_advertisement` — the
//!   zed-free prompt-truth logic, re-exported from `hkask-steer-core` (std
//!   + `log` only, split 2026-09-07 so it builds and tests without the
//!   editor closure). The MCP server's build.rs-generated `TOOL_NAMES` is
//!   the source of truth for which names may be advertised.
//!
//! The panel's obligation shrinks to: build the prompt, call `ensure` when
//! the Steer tab is shown, call `invalidate` when scope-relevant state
//! (selected board, workspace) changes.

use std::rc::Rc;
use std::sync::Arc;

use agent::{CuratorAgentServer, ThreadStore};
use agent_client_protocol::v2::schema::SessionId;
use agent_ui::{Agent, AgentConnectionStore, ConversationView};
use fs::Fs;
use gpui::{
    App, AppContext, Context, Entity, Focusable, SharedString, Subscription, WeakEntity, Window,
};
use project::Project;
use workspace::Workspace;

pub mod thread_picker;
pub mod vertical_split;
pub use thread_picker::ThreadPicker;
pub use vertical_split::{VerticalSplitState, render_vertical_split};

// The zed-free half of the Steer prompt surface (tool-advertisement
// rendering + verification), re-exported so panel call sites
// (`hkask_steer::render_tool_names` etc.) are unchanged by the split.
pub use hkask_steer_core::{
    advertised_tool_names, render_grouped_tool_advertisement, render_tool_names,
    verify_tool_advertisement,
};

/// The per-panel inputs a Steer conversation is constructed from.
pub struct SteerContext {
    /// The panel-owned system prompt (behavioral prose + tool
    /// advertisement). Verify it with `verify_tool_advertisement`.
    pub system_prompt: SharedString,
    pub fs: Arc<dyn Fs>,
    pub project: Entity<Project>,
    pub workspace: WeakEntity<Workspace>,
}

impl SteerContext {
    /// Wire the context into a `ConversationView` bound to its curator
    /// server. Panels pass `SteerContext::make_view` via `ensure_steer`;
    /// tests can substitute a stub factory to `SteerSurface::ensure`.
    pub fn make_view(
        self,
        resume_session_id: Option<SessionId>,
        connection_store: Entity<AgentConnectionStore>,
        thread_store: Entity<ThreadStore>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<ConversationView> {
        let agent_server = Rc::new(
            CuratorAgentServer::new(self.fs, thread_store.clone())
                .with_extra_static_context(self.system_prompt),
        );
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
    pending_resume: Option<SessionId>,
}

impl SteerSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// The live conversation, if one has been constructed.
    pub fn conversation(&self) -> Option<&Entity<ConversationView>> {
        self.conversation.as_ref()
    }

    /// Insert `text` into the active Steer conversation's message editor and
    /// focus it for operator review (never auto-submits). Returns `true` when
    /// the conversation was ready and received the text.
    pub fn insert_prompt(&self, text: &str, window: &mut Window, cx: &mut App) -> bool {
        let Some(conversation) = self.conversation.as_ref() else {
            return false;
        };
        let Some(thread_view) = conversation.read(cx).active_thread().cloned() else {
            return false;
        };
        let message_editor = thread_view.read(cx).message_editor.clone();
        message_editor.update(cx, |editor, cx| {
            editor.insert_text(text, window, cx);
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
        });
        true
    }

    /// Stage an existing database thread to resume on the next `ensure`,
    /// dropping any live conversation. Consumed once on rebuild.
    pub fn stage_resume(&mut self, session_id: SessionId) {
        self.pending_resume = Some(session_id);
        self.conversation = None;
    }

    /// Drop the conversation and any unconsumed staged resume so the next
    /// `ensure` builds a fresh conversation. Call this when scope-relevant
    /// panel state changes (selected board, backend mode, active workspace) —
    /// the system prompt is baked at construction, so a stale conversation
    /// would steer against the previous state. The connection store survives
    /// invalidation.
    pub fn invalidate(&mut self) {
        self.conversation = None;
        self.pending_resume = None;
    }

    fn take_pending_resume(&mut self) -> Option<SessionId> {
        self.pending_resume.take()
    }

    /// Observe the live conversation's active thread once it exists, invoking
    /// `on_thread` immediately with the current thread and on every subsequent
    /// update. Returns `None` while the conversation or its active thread is
    /// not yet available.
    pub fn observe_active_thread<P: 'static>(
        &self,
        cx: &mut Context<P>,
        on_thread: impl Fn(Entity<acp_thread::AcpThread>, &mut App) + 'static,
    ) -> Option<Subscription> {
        let conversation = self.conversation()?;
        let thread_view = conversation.read(cx).active_thread()?;
        let thread = thread_view.read(cx).thread.clone();
        on_thread(thread.clone(), cx);
        Some(cx.observe(&thread, move |_, thread, cx| {
            on_thread(thread, cx);
        }))
    }

    /// Lazily construct the conversation. No-op if one is already live.
    /// The `make` closure receives (context, resume_session_id,
    /// connection_store, thread_store, window, cx) and must return the
    /// conversation entity — production panels use `SteerContext::make_view`;
    /// the hook keeps the lifecycle (invalidation reusing `connection_store`)
    /// testable without a real `ConversationView`.
    pub fn ensure(
        &mut self,
        make: impl FnOnce(
            SteerContext,
            Option<SessionId>,
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
        let resume_session_id = self.take_pending_resume();
        let thread_store = ThreadStore::global(cx);
        let connection_store = self
            .connection_store
            .get_or_insert_with(|| {
                cx.new(|cx| AgentConnectionStore::new(context.project.clone(), cx))
            })
            .clone();
        self.conversation = Some(make(
            context,
            resume_session_id,
            connection_store,
            thread_store,
            window,
            cx,
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_resume_consumes_once() {
        let mut surface = SteerSurface::new();
        let session_id = SessionId::new("session-1");
        surface.stage_resume(session_id.clone());

        assert_eq!(surface.take_pending_resume(), Some(session_id));
        assert_eq!(surface.take_pending_resume(), None);
    }

    #[test]
    fn stage_resume_replaces_unconsumed_session() {
        let mut surface = SteerSurface::new();
        surface.stage_resume(SessionId::new("session-1"));
        let newer = SessionId::new("session-2");
        surface.stage_resume(newer.clone());

        assert_eq!(surface.take_pending_resume(), Some(newer));
        assert_eq!(surface.take_pending_resume(), None);
    }

    #[test]
    fn invalidate_clears_unconsumed_staged_resume() {
        let mut surface = SteerSurface::new();
        surface.stage_resume(SessionId::new("session-1"));
        surface.invalidate();

        assert_eq!(surface.take_pending_resume(), None);
        assert!(surface.conversation().is_none());
    }
}
