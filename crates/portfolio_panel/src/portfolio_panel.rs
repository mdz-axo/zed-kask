//! Portfolio Steer panel — a Steer-only surface for the `hkask-mcp-portfolio`
//! MCP server.
//!
//! Unlike the kanban/swarm panels, this panel deliberately has **no browse
//! forms** — the portfolio widget already renders artifacts inline in chat
//! (the D18 seam), and hand-written management forms would duplicate the
//! Steer conversation's chat-driven CRUD. The panel's sole affordance is a
//! scoped curator `ConversationView` (via `hkask_steer::SteerSurface`) whose
//! prompt advertises the portfolio server's generated `TOOL_NAMES`.

mod panel_button;

use gpui::{App, Entity, FocusHandle, SharedString, WeakEntity, Window};
use ui::prelude::*;
use workspace::{
    Item, ItemEvent, Workspace,
    item::register_serializable_item,
};

pub use panel_button::PortfolioPanelButton;

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

/// The Steer panel. Deliberately Steer-only: all portfolio CRUD (create,
/// import/export, ledger edits) is reachable through the scoped curator
/// conversation. The browse forms a traditional management panel would
/// carry duplicate what the chat can already do.
pub struct PortfolioPanel {
    focus_handle: FocusHandle,
    steer: hkask_steer::SteerSurface,
    project: Entity<project::Project>,
    fs: std::sync::Arc<dyn fs::Fs>,
    workspace_handle: WeakEntity<Workspace>,
}

impl PortfolioPanel {
    fn new(workspace: &Workspace, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let project = workspace.project().clone();
        let fs = workspace.fs().clone();
        let workspace_handle = workspace.downgrade();
        Self {
            focus_handle: cx.focus_handle(),
            steer: hkask_steer::SteerSurface::new(),
            project,
            fs,
            workspace_handle,
        }
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
            },
            hkask_mcp_portfolio::TOOL_NAMES,
            &["portfolio_", "ledger_"],
            window,
            cx,
        );
    }
}

/// The Steer prompt. Text-only; verified against the server's generated
/// TOOL_NAMES by `verify_tool_advertisement` inside `ensure_steer`.
fn steer_system_prompt() -> SharedString {
    let prompt = "## Portfolio Panel — Steer Mode\n\
         You are operating in the Portfolio panel's Steer mode, scoped to the \
         `hkask-mcp-portfolio` MCP server. All portfolio management is driven \
         through chat — there are no management forms in this panel.\n\
         \n\
         **Portfolio tools**: `portfolio_create` (idempotent), `portfolio_list`, \
         `portfolio_snapshot`, `portfolio_returns`, `portfolio_roll`, \
         `portfolio_delete`, `portfolio_rebuild_views`, \
         `portfolio_materialize_returns`, `portfolio_daily_returns`.\n\
         **Ledger tools**: `ledger_apply` (buy/sell/roll/weight/deposit/\
         withdrawal/dividend), `ledger_read`, `ledger_import`, `ledger_export` \
         (CSV or JSON).\n\
         **Price feed**: `portfolio_seed_price` writes a price-cache entry the \
         returns tools read.\n\
         \n\
         The portfolio widget (the ```markdown portfolio block) already renders \
         artifacts inline — use it for visualization; this conversation is the \
         management surface.";
    prompt.into()
}

impl gpui::Render for PortfolioPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Lazily ensure the Steer surface the first time the panel draws —
        // `ensure_steer` needs `&mut Window`.
        self.ensure_steer(window, cx);
        let conversation = self.steer.conversation().cloned();
        div()
            .size_full()
            .when_some(conversation, |div, conversation| div.child(conversation))
    }
}

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

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::IconName> {
        Some(ui::IconName::ChartBar)
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
        for name in hkask_steer::advertised_tool_names(
            &prompt,
            &["portfolio_", "ledger_"],
        ) {
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
