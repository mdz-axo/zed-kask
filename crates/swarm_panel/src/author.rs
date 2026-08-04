//! The agent-authoring surface: form state, editor construction, and the
//! `render_author` renderer. Extracted from `swarm_panel.rs` — the renderer
//! stays a method on `SwarmPanel` (it dispatches via `cx.listener` into panel
//! methods); this module owns the form struct and the view construction.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    prelude::*,
};

use crate::SwarmPanel;

/// State for the agent-authoring surface.
pub(crate) struct AuthorForm {
    pub(crate) name: Entity<Editor>,
    pub(crate) description: Entity<Editor>,
    pub(crate) system_prompt: Entity<Editor>,
    pub(crate) agent_type: String,
    /// Result of the last create attempt (success id or error).
    pub(crate) status: Option<SharedString>,
    pub(crate) busy: bool,
}

impl AuthorForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SwarmPanel>) -> Self {
        Self {
            name: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("agent_name (lowercase_with_underscores)", window, cx);
                e
            }),
            description: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("One-sentence description", window, cx);
                e
            }),
            system_prompt: cx.new(|cx| {
                // Multi-line auto-height: system prompts are multi-paragraph
                // by nature (the L3 finding). Grows 4–16 lines with content.
                let mut e = Editor::auto_height(4, 16, window, cx);
                e.set_placeholder_text(
                    "System prompt — the agent's instructions (multiple lines supported)",
                    window,
                    cx,
                );
                e
            }),
            agent_type: "research".to_string(),
            status: None,
            busy: false,
        }
    }
}

impl SwarmPanel {
    /// The agent-authoring surface: name, agent type, description, system
    /// prompt, create. Every field carries a tooltip so the operator always
    /// has a nudge for what to enter and which backend it targets.
    pub(crate) fn render_author(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        let is_local = Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local;
        let create_label = if self.author.busy {
            "Creating…"
        } else if is_local {
            "Create Local Agent"
        } else {
            "Create Agent"
        };
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(Headline::new("Author an Agent").size(HeadlineSize::Small))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Name")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-name")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "Agent identifier (lowercase_with_underscores). Becomes the \
                                 system id used to hire, delegate, and reference the agent.",
                            ))
                            .child(self.author.name.clone()),
                    ),
            )
            // Agent type selector — previously the type was hardcoded to
            // "research" with no UI control, so the operator could never
            // choose it. Wired to `self.author.agent_type`.
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Agent type")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-agent-type-group")
                            .tooltip(Tooltip::text(
                                "The agent's role category. Drives how the catalogue \
                                 groups the agent and which delegation filters match it.",
                            ))
                            .child(
                                ToggleButtonGroup::single_row(
                                    "author-agent-type",
                                    [
                                        ToggleButtonSimple::new(
                                            "research",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.agent_type = "research".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "creative",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.agent_type = "creative".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "meta",
                                            cx.listener(|this, _event, _, cx| {
                                                this.author.agent_type = "meta".to_string();
                                                cx.notify();
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(28.)))
                                .label_size(LabelSize::Small)
                                .auto_width()
                                .selected_index(match self.author.agent_type.as_str() {
                                    "creative" => 1,
                                    "meta" => 2,
                                    _ => 0,
                                })
                                .into_any_element(),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Description")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-description")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "One-sentence summary shown on the agent card in the \
                                 browse list. Optional but recommended for discovery.",
                            ))
                            .child(self.author.description.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("System prompt")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("author-system-prompt")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "The agent's instructions — what it should do, how \
                                 it should behave, and any constraints. Multiple lines \
                                 supported. Required.",
                            ))
                            .child(self.author.system_prompt.clone()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("create-agent", create_label)
                            .style(ButtonStyle::Filled)
                            .disabled(self.author.busy)
                            .tooltip(Tooltip::text(if is_local {
                                "Creates the agent on the local substrate \
                                 (agents/local/curated). No cost, no consent."
                            } else {
                                "Creates the agent in the ABW catalogue. \
                                 The catalogue may apply its own validation."
                            }))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.create_agent(cx);
                            })),
                    )
                    .when_some(self.author.status.clone(), |this, status| {
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
    }
}
