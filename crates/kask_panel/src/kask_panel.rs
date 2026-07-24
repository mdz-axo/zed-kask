//! Kask Panel — native GPUI panel for direct MCP tool invocation (D10).
//!
//! A dockable panel that provides per-server access to the 10 built-in kask
//! MCP servers. Users can directly invoke tools via `:tool args` syntax and
//! see results inline. This replaces the deleted `hkask-repl` `mcp_scoped`.
//!
//! The panel is registered in the workspace alongside the Agent Panel and
//! toggled via `kask_panel::Toggle` / `kask_panel::ToggleFocus` actions.

use std::sync::Arc;

use gpui::{
    Action, AnyElement, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle,
    Focusable, Pixels, Render, Task, WeakEntity, prelude::*,
};
use ui::{IconName, prelude::*};
use workspace::{DockPosition, Panel, PanelEvent};

use zed_actions::kask_panel::{Toggle, ToggleFocus};

const KASK_PANEL_KEY: &str = "KaskPanel";
const MIN_PANEL_WIDTH: Pixels = px(300.);

/// The 10 built-in kask MCP servers (matches `kask_page.rs`).
const BUILT_IN_MCP_SERVERS: &[&str] = &[
    "codegraph",
    "companies",
    "condenser",
    "corpus",
    "curator",
    "kata-kanban",
    "media",
    "research",
    "scenarios",
    "training",
];

/// The kask panel — a dockable panel for direct MCP tool invocation.
pub struct KaskPanel {
    workspace: WeakEntity<workspace::Workspace>,
    focus_handle: FocusHandle,
    /// Currently selected server (index into `BUILT_IN_MCP_SERVERS`).
    selected_server: usize,
    /// The tool invocation input text.
    tool_input: String,
    /// The result output text.
    output: String,
    /// Whether a tool invocation is in progress.
    busy: bool,
}

impl KaskPanel {
    /// Create a new kask panel.
    pub fn new(
        workspace: &workspace::Workspace,
        _window: &mut gpui::Window,
        cx: &mut Context<workspace::Workspace>,
    ) -> Entity<Self> {
        cx.new(|cx| Self {
            workspace: workspace.weak_handle(),
            focus_handle: cx.focus_handle(),
            selected_server: 0,
            tool_input: String::new(),
            output: String::new(),
            busy: false,
        })
    }

    /// Load the panel asynchronously (matches the `Panel::load` pattern).
    pub fn load(
        workspace: WeakEntity<workspace::Workspace>,
        cx: &mut AsyncWindowContext,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                Ok(Self::new(workspace, window, cx))
            })?
        })
    }

    fn selected_server_name(&self) -> &'static str {
        BUILT_IN_MCP_SERVERS
            .get(self.selected_server)
            .copied()
            .unwrap_or("none")
    }

    fn invoke_tool(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.tool_input.is_empty() {
            return;
        }

        let server = self.selected_server_name().to_string();
        let input = self.tool_input.clone();
        self.busy = true;
        self.output = format!("Invoking {server}: {input}...\n");
        cx.notify();

        // The actual tool invocation would go through the BridgeToolPort /
        // McpRuntime. For now, this is a placeholder that logs the invocation.
        // The full wiring requires a global ToolPort hook (similar to
        // set_manifest_executor / set_memory_port) so the panel can invoke
        // tools without depending on kask_bridge.
        let output = format!(
            "[kask_panel] Tool invocation placeholder — server: {server}, input: {input}\n\
             Full tool invocation wiring requires a global ToolPort hook.\n"
        );

        cx.background_executor()
            .timer(std::time::Duration::from_millis(100))
            .detach();

        self.output = output;
        self.busy = false;
        cx.notify();
    }

    fn render_server_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let server_names: Vec<SharedString> =
            BUILT_IN_MCP_SERVERS.iter().map(|s| (*s).into()).collect();

        // Simple label-based selector — a full dropdown would use PopoverMenu.
        let current = self.selected_server_name();
        let buttons: Vec<AnyElement> = BUILT_IN_MCP_SERVERS
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let is_selected = index == self.selected_server;
                Button::new(("server-btn", index), *name)
                    .style(if is_selected {
                        ButtonStyle::Tinted(ui::TintColor::Accent)
                    } else {
                        ButtonStyle::Subtle
                    })
                    .label_size(LabelSize::XSmall)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_server = index;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_1()
            .child(
                Label::new("MCP Server")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(h_flex().gap_1().flex_wrap().children(buttons))
            .child(
                Label::new(format!("Selected: {current}"))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
    }

    fn render_tool_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(
                Label::new("Tool Invocation")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        ui::Input::new(self.focus_handle.clone())
                            .placeholder(":tool args")
                            .text(self.tool_input.clone())
                            .on_input(cx.listener(|this, text, _, cx| {
                                this.tool_input = text;
                                cx.notify();
                            }))
                            .on_submit(cx.listener(|this, _, window, cx| {
                                this.invoke_tool(cx);
                                window.focus(&this.focus_handle);
                            })),
                    )
                    .child(
                        Button::new("invoke-btn", "Invoke")
                            .style(ButtonStyle::Filled)
                            .disabled(self.busy || self.tool_input.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.invoke_tool(cx);
                            })),
                    ),
            )
    }

    fn render_output(&self) -> impl IntoElement {
        v_flex()
            .gap_1()
            .flex_1()
            .min_h_0()
            .child(
                Label::new("Output")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .rounded_sm()
                    .border_1()
                    .border_color(cx_theme_border())
                    .child(Label::new(self.output.clone()).size(LabelSize::Small)),
            )
    }
}

fn cx_theme_border() -> Hsla {
    // Simplified — a real implementation would read from cx.theme()
    Hsla::gray(0.3)
}

impl Focusable for KaskPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for KaskPanel {}

impl Panel for KaskPanel {
    fn persistent_name() -> &'static str {
        "KaskPanel"
    }

    fn panel_key() -> &'static str {
        KASK_PANEL_KEY
    }

    fn position(&self, _window: &gpui::Window, _cx: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        position != DockPosition::Bottom
    }

    fn set_position(
        &mut self,
        _position: DockPosition,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) {
        // Kask panel is always on the right dock for now.
    }

    fn default_size(&self, _window: &gpui::Window, _cx: &App) -> Pixels {
        px(400.)
    }

    fn min_size(&self, _window: &gpui::Window, _cx: &App) -> Option<Pixels> {
        Some(MIN_PANEL_WIDTH)
    }

    fn icon(&self, _window: &gpui::Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Settings)
    }

    fn icon_tooltip(&self, _window: &gpui::Window, _cx: &App) -> Option<&'static str> {
        Some("Kask Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(Toggle)
    }

    fn activation_priority(&self) -> u32 {
        5
    }
}

impl Render for KaskPanel {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .track_focus(&self.focus_handle)
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Kask Panel").size(LabelSize::Large))
                    .child(
                        Label::new("Direct MCP tool invocation for the 10 built-in kask servers.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(self.render_server_selector(cx))
            .child(self.render_tool_input(cx))
            .child(self.render_output())
            .into_any_element()
    }
}

/// Initialize the kask panel — registers actions on the workspace.
pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut workspace::Workspace, _window, _cx: &mut Context<workspace::Workspace>| {
            workspace.register_action(|workspace, _: &Toggle, window, cx| {
                workspace.toggle_panel::<KaskPanel>(window, cx);
            });
            workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
                workspace.focus_panel::<KaskPanel>(window, cx);
            });
        },
    )
    .detach();
}
