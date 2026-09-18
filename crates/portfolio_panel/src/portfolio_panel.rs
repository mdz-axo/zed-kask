//! Portfolio analysis panel — investor reports above a Steer workspace for
//! the `hkask-mcp-portfolio` MCP server.
//!
//! The upper viewer renders server-authored characteristics, contribution,
//! benchmark-relative attribution, and what-if reports. The lower Steer
//! conversation drives portfolio operations and can resume existing threads.
//! A shared draggable split keeps both surfaces usable without duplicating the
//! media panel's split-state mathematics.

pub mod panel_button;
pub mod portfolio_viewer;

use gpui::{
    App, ClickEvent, Context, DefiniteLength, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Task, WeakEntity, Window, actions, px,
};
use ui::{Icon, IconName, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

pub use panel_button::PortfolioPanelButton;
pub use portfolio_viewer::PortfolioViewer;

/// The MCP server id this panel's Steer conversation is scoped to.
const PORTFOLIO_SERVER: &str = "hkask-mcp-portfolio";

actions!(
    portfolio_panel,
    [
        /// Deploys a new Portfolio Panel if none is open, else focuses the
        /// existing one. Used by the View menu entry.
        Toggle,
        /// Focuses an existing Portfolio Panel (no-op if none is open).
        ToggleFocus,
    ]
);

/// Register the panel's actions on every new `Workspace`.
pub fn init(cx: &mut App) {
    register_serializable_item::<PortfolioPanel>(cx);
    cx.observe_new(move |workspace: &mut Workspace, window, _cx| {
        let Some(_window) = window else {
            return;
        };
        // Per the `.rules` trap "Center-pane Item Toggle vs ToggleFocus", the
        // View menu entry uses `Toggle` (deploys a new item if none exists),
        // not `ToggleFocus` (silent no-op when absent).
        workspace
            .register_action(move |workspace, _: &Toggle, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<PortfolioPanel>());

                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                } else {
                    let panel = PortfolioPanel::new(workspace, window, cx);
                    workspace.add_item_to_active_pane(
                        Box::new(panel.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    panel.focus_handle(cx).focus(window, cx);
                }
            })
            .register_action(move |workspace, _: &ToggleFocus, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<PortfolioPanel>());
                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                }
            });
    })
    .detach();
}

/// Investor reports above a Steer workspace, split by a draggable divider.
pub struct PortfolioPanel {
    focus_handle: FocusHandle,
    steer: hkask_steer::SteerSurface,
    project: Entity<project::Project>,
    fs: std::sync::Arc<dyn fs::Fs>,
    workspace_handle: WeakEntity<Workspace>,
    /// Investor report surface in the upper pane.
    viewer: Entity<PortfolioViewer>,
    /// Observation of the active or resumed conversation thread.
    thread_observation: Option<gpui::Subscription>,
    /// Shared viewer/director split state.
    split: hkask_steer::VerticalSplitState,
    /// The "Open Thread" picker — resumes a database thread in this panel's
    /// Steer surface.
    thread_picker: Entity<hkask_steer::ThreadPicker>,
    /// The session id to resume on the next `ensure_steer` — set by
    /// `open_thread`, consumed by `ensure_steer`.
    pending_resume: Option<agent_client_protocol::schema::v1::SessionId>,
}

impl PortfolioPanel {
    pub fn new(
        workspace: &Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let workspace_handle = workspace.weak_handle();
        let project = workspace.project().clone();
        let fs = workspace.app_state().fs.clone();
        cx.new(|cx| {
            let panel_handle: gpui::WeakEntity<PortfolioPanel> = cx.weak_entity();
            let thread_picker = cx.new(|cx| {
                hkask_steer::ThreadPicker::new(
                    std::rc::Rc::new(move |session_id, window, cx: &mut gpui::App| {
                        panel_handle
                            .update(cx, |panel, cx| panel.open_thread(session_id, window, cx))
                            .log_err();
                    }),
                    cx,
                )
            });
            Self {
                focus_handle: cx.focus_handle(),
                steer: hkask_steer::SteerSurface::new(),
                project,
                fs,
                workspace_handle: workspace_handle.clone(),
                viewer: cx.new(|_| PortfolioViewer::new()),
                thread_observation: None,
                split: hkask_steer::VerticalSplitState::default(),
                thread_picker,
                pending_resume: None,
            }
        })
    }

    /// Lazily construct the Steer `ConversationView`. Scoped to the portfolio
    /// MCP server; verified against its generated `TOOL_NAMES`.
    fn ensure_steer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        hkask_steer::ensure_steer(
            &mut self.steer,
            hkask_steer::SteerContext {
                server_scope: PORTFOLIO_SERVER.into(),
                system_prompt: steer_system_prompt(),
                fs: self.fs.clone(),
                project: self.project.clone(),
                workspace: self.workspace_handle.clone(),
                resume_session_id: self.pending_resume.take(),
            },
            hkask_mcp_portfolio::TOOL_NAMES,
            &["portfolio_", "ledger_"],
            window,
            cx,
        );
    }

    /// Resume a database thread in this panel's Steer surface, replacing the
    /// live conversation.
    fn open_thread(
        &mut self,
        session_id: agent_client_protocol::schema::v1::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_resume = Some(session_id);
        self.steer.invalidate();
        self.thread_observation = None;
        self.ensure_steer(window, cx);
        cx.notify();
    }

    fn render_split_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("portfolio-panel-split-divider")
            .relative()
            .w_full()
            .flex_shrink_0()
            .h(px(1.))
            .bg(cx.theme().colors().border_variant)
            .child(
                div()
                    .id("portfolio-panel-split-handle")
                    .absolute()
                    .top(px(-hkask_steer::SPLIT_HANDLE_HIT_HEIGHT / 2.0))
                    .h(px(hkask_steer::SPLIT_HANDLE_HIT_HEIGHT))
                    .w_full()
                    .cursor_row_resize()
                    .block_mouse_except_scroll()
                    .on_click(cx.listener(|this, event: &ClickEvent, _window, cx| {
                        if event.click_count() >= 2 {
                            this.split.reset();
                            cx.notify();
                        }
                        cx.stop_propagation();
                    }))
                    .on_drag(hkask_steer::VerticalSplitDrag, |_, _, _, cx| {
                        cx.new(|_| gpui::Empty)
                    }),
            )
    }
}

/// The Steer prompt. Behavioral prose is written by hand; the tool list is
/// rendered from the server's generated `TOOL_NAMES` by
/// `render_grouped_tool_advertisement`, so a rename/merge/addition in the
/// server propagates at the next build instead of degrading to "tool not
/// found" at dispatch. `verify_tool_advertisement` inside `ensure_steer`
/// still checks the prose's tool mentions.
fn steer_system_prompt() -> SharedString {
    let tool_section = hkask_steer::render_grouped_tool_advertisement(
        hkask_mcp_portfolio::TOOL_NAMES,
        &[
            ("Portfolio tools", &["portfolio_"]),
            ("Ledger tools", &["ledger_"]),
        ],
    );
    let prompt = format!(
        "## Portfolio Panel — Steer Mode\n\
         You are operating in the Portfolio panel's Steer mode, scoped to the \
         `hkask-mcp-portfolio` MCP server. Use the portfolio report tools for \
         investor-oriented characteristics, contribution, benchmark-relative \
         attribution, and current or historical what-if analysis. Do not frame \
         the workspace around daily-return monitoring.\n\
         \n\
         {tool_section}\
         \n\
         Server-authored ```portfolio display hints render in the viewer above. \
         Contribution is absolute; attribution requires an explicit benchmark. \
         Historical what-if output is hindsight, not an ex-ante forecast."
    );
    prompt.into()
}

impl gpui::Render for PortfolioPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_steer(window, cx);

        if self.thread_observation.is_none()
            && let Some(conversation) = self.steer.conversation()
            && let Some(thread_view) = conversation.read(cx).active_thread()
        {
            let thread = thread_view.read(cx).thread.clone();
            let viewer = self.viewer.clone();
            self.thread_observation = Some(cx.observe(&thread, move |_, thread, cx| {
                viewer.update(cx, |viewer, cx| viewer.ingest_thread(thread.clone(), cx));
            }));
            let thread_for_ingest = thread.clone();
            self.viewer
                .update(cx, |viewer, cx| viewer.ingest_thread(thread_for_ingest, cx));
        }

        let conversation = self.steer.conversation().cloned();
        let director = div()
            .w_full()
            .h(DefiniteLength::Fraction(self.split.bottom_fraction()))
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(self.thread_picker.clone()),
            )
            .when_some(conversation, |element, conversation| {
                element.child(conversation)
            });

        v_flex()
            .size_full()
            .on_drag_move::<hkask_steer::VerticalSplitDrag>(cx.listener(
                |this, event, _window, cx| {
                    this.split.update_from_drag(event);
                    cx.notify();
                },
            ))
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.viewer.clone()),
            )
            .child(self.render_split_handle(cx))
            .child(director)
    }
}

impl EventEmitter<ItemEvent> for PortfolioPanel {}

impl Focusable for PortfolioPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for PortfolioPanel {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Portfolio".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Blocks).color(Color::Muted))
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, function: &mut dyn FnMut(ItemEvent)) {
        function(*event)
    }
}

impl SerializableItem for PortfolioPanel {
    fn serialized_item_kind() -> &'static str {
        "PortfolioPanel"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    fn serialize(
        &mut self,
        _workspace: &mut Workspace,
        _item_id: workspace::ItemId,
        _closing: bool,
        _cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        None
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                PortfolioPanel::new(workspace, window, cx)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `portfolio_*`/`ledger_*` token the Steer prompt names in
    /// backticks must exist in the server's generated TOOL_NAMES — a rename
    /// fails here, not at dispatch.
    #[test]
    fn steer_prompt_advertises_only_known_tools() {
        let prompt = steer_system_prompt();
        for name in hkask_steer::advertised_tool_names(&prompt, &["portfolio_", "ledger_"]) {
            assert!(
                hkask_mcp_portfolio::TOOL_NAMES.contains(&name.as_str()),
                "steer prompt advertises `{name}`, not in hkask_mcp_portfolio::TOOL_NAMES"
            );
        }
    }

    /// Every tool the server exposes should be advertised in the prompt — a
    /// missing name means the curator cannot discover it in Steer mode.
    #[test]
    fn server_tools_are_all_advertised() {
        let prompt = steer_system_prompt();
        for tool in hkask_mcp_portfolio::TOOL_NAMES {
            assert!(
                prompt.contains(tool),
                "hkask_mcp_portfolio::TOOL_NAMES lists `{tool}` but the Steer prompt never mentions it"
            );
        }
    }
}
