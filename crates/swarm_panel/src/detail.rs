//! The swarm-detail (roster drill-down) renderer. Extracted from
//! `swarm_panel.rs` — the renderer stays a method on `SwarmPanel` (it
//! dispatches via `cx.listener` into panel methods); this module owns the view
//! construction. See `author.rs` / `compose.rs` for the same extraction
//! pattern.

use gpui::{Context, SharedString};
use ui::{Tooltip, prelude::*};

use crate::SwarmDetailView;
use crate::SwarmPanel;
use crate::parse::AgentSource;

impl SwarmPanel {
    pub(crate) fn render_swarm_detail(
        &self,
        detail: &SwarmDetailView,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let border = cx.theme().colors().border;
        let is_local = detail.source == AgentSource::Local;
        let source_badge = detail.source.badge();
        let source_label = detail.source.label();
        let add_label = if is_local { "Add" } else { "Hire…" };
        let remove_label = if is_local { "Remove" } else { "Fire" };
        let add_tooltip = if is_local {
            "Adds the named local agent id to this swarm's roster. \
             Idempotent, no cost. The agent need not exist in the registry yet."
        } else {
            "Hires the named ABW agent into this workspace. Pre-flighted for \
             cost and consent-gated — the consent banner appears before any \
             credits are spent."
        };
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("back-to-swarms", "← Back")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .tooltip(Tooltip::text("Back to the swarm list."))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_swarm_detail(cx);
                            })),
                    )
                    .child(
                        Headline::new(format!("{} — roster", detail.name))
                            .size(HeadlineSize::Small),
                    )
                    .child(
                        Label::new(format!("{} {}", source_badge, source_label))
                            .color(Color::Accent)
                            .size(LabelSize::XSmall),
                    ),
            )
            .child(
                Label::new(format!("id: {}", detail.workspace_id))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .when(!detail.mission.is_empty(), |this| {
                this.child(
                    Label::new(format!("mission: {}", detail.mission))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
            // Add-agent affordance: an input + button. Mode-aware dispatch.
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .id("swarm-detail-add-agent")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(if is_local {
                                "Local agent id to add to this swarm's roster."
                            } else {
                                "ABW agent name to hire into this workspace."
                            }))
                            .child(self.swarm_add_agent_editor.clone()),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("add-agent-{}", detail.workspace_id)),
                            add_label,
                        )
                        .style(ButtonStyle::Filled)
                        .label_size(LabelSize::XSmall)
                        .disabled(self.spend_in_flight.is_some())
                        .tooltip(Tooltip::text(add_tooltip))
                        .on_click(cx.listener({
                            let workspace_id = detail.workspace_id.clone();
                            move |this, _, window, cx| {
                                let agent_name = this
                                    .swarm_add_agent_editor
                                    .read(cx)
                                    .text(cx)
                                    .trim()
                                    .to_string();
                                if agent_name.is_empty() {
                                    return;
                                }
                                if is_local {
                                    this.add_agent_to_swarm(workspace_id.clone(), agent_name, cx);
                                } else {
                                    // ABW: target the open workspace and
                                    // route through the consent-gated hire
                                    // flow. `confirm_hire` re-opens this
                                    // detail on success.
                                    this.selected_workspace = Some(workspace_id.clone());
                                    this.begin_hire(agent_name, cx);
                                }
                                // Clear the input whether the add/hire
                                // succeeded or is awaiting consent.
                                this.swarm_add_agent_editor
                                    .update(cx, |editor, cx| editor.clear(window, cx));
                            }
                        })),
                    ),
            )
            .when(detail.loading, |this| {
                this.child(
                    Label::new("Loading roster…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .when_some(detail.error.clone(), |this, err| {
                this.child(Label::new(err).size(LabelSize::Small).color(Color::Warning))
            })
            .when(!detail.agents.is_empty(), |this| {
                this.child(v_flex().gap_1().children(detail.agents.iter().map(|a| {
                    let row_agent_id = a.agent_id.clone();
                    let row_workspace = detail.workspace_id.clone();
                    let row_remove_label = remove_label;
                    let row_is_local = is_local;
                    h_flex()
                        .gap_2()
                        .child(Label::new(a.agent_id.clone()).color(Color::Default))
                        .when(!a.agent_type.is_empty(), |this| {
                            this.child(Label::new(a.agent_type.clone()).color(Color::Accent))
                        })
                        .child(div().flex_1())
                        .when(!a.description.is_empty(), |this| {
                            this.child(Label::new(a.description.clone()).color(Color::Muted))
                        })
                        .child(
                            Button::new(
                                SharedString::from(format!("roster-remove-{row_agent_id}")),
                                row_remove_label,
                            )
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .disabled(self.spend_in_flight.is_some())
                            .tooltip(Tooltip::text(if row_is_local {
                                "Removes this agent from the swarm's roster. \
                                 Idempotent. The agent card is not deleted."
                            } else {
                                "Fires this agent from the ABW workspace. \
                                 No credit cost; the agent itself is not deleted."
                            }))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if row_is_local {
                                        this.remove_agent_from_swarm(
                                            row_workspace.clone(),
                                            row_agent_id.clone(),
                                            cx,
                                        );
                                    } else {
                                        this.fire_agent(
                                            row_workspace.clone(),
                                            row_agent_id.clone(),
                                            cx,
                                        );
                                    }
                                },
                            )),
                        )
                })))
            })
            .when(
                detail.agents.is_empty() && !detail.loading && detail.error.is_none(),
                |this| {
                    this.child(
                        Label::new("No agents in this swarm. Add one above.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                },
            )
            // Local swarms can be deleted from the panel (ABW swarm deletion
            // stays on the swarm card / server surface to avoid an accidental
            // irreversible delete from the detail view).
            .when(is_local, |this| {
                this.child(
                    h_flex().gap_2().child(div().flex_1()).child(
                        Button::new(
                            SharedString::from(format!("delete-swarm-{}", detail.workspace_id)),
                            "Delete Swarm",
                        )
                        .style(ButtonStyle::Subtle)
                        .label_size(LabelSize::XSmall)
                        .disabled(self.spend_in_flight.is_some())
                        .tooltip(Tooltip::text(
                            "Permanently deletes this local swarm and its \
                                 roster. Member agent cards are NOT deleted.",
                        ))
                        .on_click(cx.listener({
                            let swarm_id = detail.workspace_id.clone();
                            move |this, _, _, cx| {
                                this.delete_local_swarm(swarm_id.clone(), cx);
                            }
                        })),
                    ),
                )
            })
    }
}
