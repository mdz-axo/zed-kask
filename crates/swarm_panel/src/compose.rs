//! The swarm-composition surface: form state, editor construction, and the
//! `render_compose` renderer (including the Xaman Ek composition consultant).
//! Extracted from `swarm_panel.rs` — the renderer stays a method on
//! `SwarmPanel` (it dispatches via `cx.listener` into panel methods); this
//! module owns the form struct and the view construction.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    prelude::*,
};

use crate::SwarmPanel;
use crate::status_is_warning;

/// State for the swarm-composition surface.
pub(crate) struct ComposeForm {
    pub(crate) name: Entity<Editor>,
    pub(crate) mission: Entity<Editor>,
    /// Agent names to hire, comma-separated (kept as a single-line editor for v1).
    pub(crate) agents: Entity<Editor>,
    pub(crate) status: Option<SharedString>,
    pub(crate) busy: bool,
    /// Xaman Ek consultation: the operator's composition question.
    pub(crate) xaman_query: Entity<Editor>,
    /// The active Xaman Ek composition session id (continues across messages).
    pub(crate) xaman_session: Option<String>,
    /// The curator's latest response text.
    pub(crate) xaman_response: Option<SharedString>,
    /// Agent names Xaman Ek recommended (extracted from a composition plan),
    /// offered as a one-click pre-fill of the agents field.
    pub(crate) xaman_suggested_agents: Vec<String>,
    /// Which backend to create the swarm on (Cloud = ABW, Local = local
    /// substrate). A per-form choice — not gated on `kask.swarm.mode`.
    pub(crate) create_target: super::CreateTarget,
    pub(crate) xaman_busy: bool,
}

impl ComposeForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SwarmPanel>) -> Self {
        Self {
            name: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Swarm name", window, cx);
                e
            }),
            mission: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Mission (optional)", window, cx);
                e
            }),
            agents: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Agents to hire, comma-separated (optional)", window, cx);
                e
            }),
            status: None,
            busy: false,
            xaman_query: cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Ask Xaman Ek to plan your team…", window, cx);
                e
            }),
            xaman_session: None,
            xaman_response: None,
            xaman_suggested_agents: Vec::new(),
            create_target: super::CreateTarget::Cloud,
            xaman_busy: false,
        }
    }
}

impl SwarmPanel {
    /// The swarm-composition surface: name, mission, agents, create. Each
    /// field carries a tooltip; the create button label reflects the active
    /// backend (Local vs ABW) and its tooltip explains the cost/consent model.
    pub(crate) fn render_compose(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        let is_local = self.compose.create_target == super::CreateTarget::Local;
        let create_label = if self.compose.busy {
            "Creating…"
        } else if is_local {
            "Create Local Swarm"
        } else {
            "Create Swarm"
        };
        let create_tooltip: &str = if is_local {
            "Creates the swarm on the local substrate (agents/local/swarms). No cost, no consent."
        } else {
            "Creates the ABW workspace and hires the listed agents. Each hire is pre-flighted for cost and consent-gated; the swarm is not created if any consent fails."
        };
        v_flex()
            .w_full()
            .gap_3()
            // pb only — the content column already carries the panel's px_4
            // inset, so p_4 here doubled the horizontal padding vs Browse.
            .pb_4()
            .child(Headline::new("Compose a Swarm").size(HeadlineSize::Small))
            // Cloud/Local target toggle — a per-form choice, not a global
            // setting. Both backends are always available.
            .child(
                v_flex().gap_1().child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Label::new("Target:")
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                        .child(
                            div().child(
                                ToggleButtonGroup::single_row(
                                    "compose-create-target",
                                    [
                                        ToggleButtonSimple::new(
                                            "Cloud",
                                            cx.listener(|this, _, _, cx| {
                                                this.compose.create_target =
                                                    super::CreateTarget::Cloud;
                                                this.active_backend = super::CreateTarget::Cloud;
                                                cx.notify();
                                            }),
                                        ),
                                        ToggleButtonSimple::new(
                                            "Local",
                                            cx.listener(|this, _, _, cx| {
                                                this.compose.create_target =
                                                    super::CreateTarget::Local;
                                                this.active_backend = super::CreateTarget::Local;
                                                cx.notify();
                                            }),
                                        ),
                                    ],
                                )
                                .style(ToggleButtonGroupStyle::Outlined)
                                .size(ToggleButtonGroupSize::Custom(rems_from_px(24.0_f32)))
                                .label_size(LabelSize::XSmall)
                                .auto_width()
                                .selected_index(if is_local { 1 } else { 0 })
                                .into_any_element(),
                            ),
                        ),
                ),
            )
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
                            .id("compose-name")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "Swarm (workspace) name. Required. A path-safe id is \
                                 derived from this name on the local substrate.",
                            ))
                            .child(self.compose.name.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Mission")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("compose-mission")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "Mission / description for the swarm. Optional. Shown \
                                 on the swarm card and in the detail header.",
                            ))
                            .child(self.compose.mission.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new(if is_local {
                            "Agents to add (comma-separated)"
                        } else {
                            "Agents to hire (comma-separated)"
                        })
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("compose-agents")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(if is_local {
                                "Local agent ids to seed the roster with, \
                                 comma-separated. Optional. The agents need not exist \
                                 in the registry yet (the roster is ids; resolution \
                                 happens at delegation time)."
                            } else {
                                "ABW agent names to hire into the new swarm, \
                                 comma-separated. Optional. Each hire is consent-gated \
                                 and pre-flighted for cost before the swarm is created."
                            }))
                            .child(self.compose.agents.clone()),
                    ),
            )
            // Xaman Ek composition consultant — the panel calls the MCP tool
            // to plan the team, then offers the recommended agents as a
            // one-click pre-fill of the field above.
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .rounded_sm()
                    .border_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Label::new("Xaman Ek")
                                    .size(LabelSize::Small)
                                    .color(Color::Accent),
                            )
                            .child(
                                Label::new("composition consultant")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .id("compose-xaman-query")
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .tooltip(Tooltip::text(
                                        "Ask the Xaman Ek curator to recommend a team \
                                         for your task. The session continues across \
                                         messages so you can refine the plan.",
                                    ))
                                    .child(self.compose.xaman_query.clone()),
                            )
                            .child(
                                Button::new(
                                    "ask-xaman",
                                    if self.compose.xaman_busy {
                                        "Asking…"
                                    } else {
                                        "Ask"
                                    },
                                )
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .disabled(self.compose.xaman_busy)
                                .tooltip(Tooltip::text(
                                    "Sends the question to Xaman Ek (consent-gated \
                                     curator call; costs no credits).",
                                ))
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.ask_xaman(cx);
                                    },
                                )),
                            ),
                    )
                    .when_some(self.compose.xaman_response.clone(), |this, response| {
                        this.child(
                            Label::new(response)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    })
                    .when(!self.compose.xaman_suggested_agents.is_empty(), |this| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Label::new(format!(
                                        "Suggested: {}",
                                        self.compose.xaman_suggested_agents.join(", ")
                                    ))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                                )
                                .child(
                                    Button::new("apply-xaman", "Use team")
                                        .style(ButtonStyle::Filled)
                                        .label_size(LabelSize::XSmall)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_xaman_suggestions(window, cx);
                                        })),
                                ),
                        )
                    }),
            )
            // AI Assist + validation — model-backed suggestions and a
            // well-formedness check before create. Same row + banner pattern
            // as the Author surface, scoped to "swarm".
            .child(self.render_ai_assist_row("swarm", self.compose.busy, cx))
            .children(self.render_ai_suggestions_banner("swarm", cx))
            .children(self.render_validation_banner("swarm", cx))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("create-swarm", create_label)
                            .style(ButtonStyle::Filled)
                            .disabled(self.compose.busy)
                            .tooltip(Tooltip::text(create_tooltip))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.create_swarm(cx);
                            })),
                    )
                    .when_some(self.compose.status.clone(), |this, status| {
                        let is_warning = status_is_warning(&status);
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(if is_warning {
                                    Color::Warning
                                } else {
                                    Color::Muted
                                }),
                        )
                    }),
            )
    }
}
