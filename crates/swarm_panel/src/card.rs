//! Agent and swarm card renderer for the browse list.
//!
//! Cards: name + truncated description on the left, action buttons in a
//! horizontal row on the right. Both name and description truncate so long
//! text never blows out the fixed-height card.

use gpui::{Context, SharedString, Window};
use marketplace_ui_common::MarketplaceCard;
use ui::{ContextMenu, ContextMenuEntry, DropdownMenu, Tooltip, prelude::*};

use crate::parse::AgentSource;
use crate::{SwarmEntry, SwarmPanel};

impl SwarmPanel {
    pub(crate) fn render_card(
        &mut self,
        entry: SwarmEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> MarketplaceCard {
        match entry {
            SwarmEntry::Agent(agent) => {
                let agent_name = agent.id.clone();
                let source = agent.source.clone();
                let show_clone = source == AgentSource::Cloud;
                let show_push = source == AgentSource::Local;
                let show_remove = source == AgentSource::Local;
                let show_publish = source != AgentSource::Local;
                let hire_name = agent_name.clone();
                let clone_name = agent_name.clone();
                let push_name = agent_name.clone();
                let remove_name = agent_name.clone();
                let publish_name = agent_name.clone();
                let edit_name = agent_name.clone();
                let edit_source = source;
                MarketplaceCard::new().child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_1()
                                .id("agent-card-body")
                                .on_click(cx.listener({
                                    let edit_name = edit_name.clone();
                                    let edit_source = edit_source.clone();
                                    move |this, _event, window, cx| {
                                        this.load_agent_into_author(
                                            edit_name.clone(),
                                            edit_source.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                }))
                                .child(
                                    Label::new(agent.id.clone())
                                        .color(Color::Default)
                                        .truncate(),
                                )
                                .child(
                                    Label::new(agent.description.clone())
                                        .color(Color::Muted)
                                        .size(LabelSize::XSmall)
                                        .truncate(),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .flex_shrink_0()
                                .items_center()
                                .child(
                                    Button::new(
                                        SharedString::from(format!("edit-{edit_name}")),
                                        "Edit",
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(Tooltip::text(
                                        "Open this agent's settings in the author panel \
                                         to view and adjust its details.",
                                    ))
                                    .on_click(cx.listener({
                                        let edit_name = edit_name.clone();
                                        let edit_source = edit_source.clone();
                                        move |this, _, window, cx| {
                                            this.load_agent_into_author(
                                                edit_name.clone(),
                                                edit_source.clone(),
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                                )
                                .child(
                                    Button::new(
                                        SharedString::from(format!("hire-{agent_name}")),
                                        if self.spend_in_flight.as_deref()
                                            == Some(agent_name.as_str())
                                        {
                                            "Hiring…"
                                        } else {
                                            "Hire"
                                        },
                                    )
                                    .style(ButtonStyle::Filled)
                                    .label_size(LabelSize::XSmall)
                                    .disabled(self.spend_in_flight.is_some())
                                    .tooltip(Tooltip::text(
                                        "Hire this agent into the selected swarm. \
                                         Pre-flighted for cost and consent-gated.",
                                    ))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.begin_hire(hire_name.clone(), cx);
                                        },
                                    )),
                                )
                                .when(
                                    show_clone || show_push || show_publish || show_remove,
                                    |this| {
                                        let menu = cx.new(|cx| {
                                            ContextMenu::new(window, cx, |menu, _window, _cx| {
                                                menu.when(show_clone, |m| {
                                                    m.entry(
                                                        ContextMenuEntry::new("Clone to Local")
                                                            .handler(move |_, window, cx| {
                                                                drop(window);
                                                                drop(cx);
                                                                clone_name_clone
                                                                    .spawn_clone(cx);
                                                            }),
                                                    )
                                                })
                                            })
                                        });
                                        this.child(
                                            DropdownMenu::new(
                                                SharedString::from(format!("more-{agent_name}")),
                                                "More",
                                                menu,
                                            )
                                            .trigger_size(ButtonSize::Compact)
                                            .style(DropdownStyle::Subtle),
                                        )
                                    },
                                ),
                        ),
                )
            }
            SwarmEntry::Swarm(swarm) => {
                let swarm_id = swarm.id.clone();
                let swarm_name = swarm.name.clone();
                let swarm_source = swarm.source.clone();
                let swarm_mission = swarm.description.clone();
                let detail_id = swarm_id.clone();
                let detail_name = swarm_name.clone();
                let detail_source = swarm_source;
                let detail_mission = swarm_mission;
                let detail_agent_count = swarm.agent_count;
                let detail_budget = swarm.budget;
                let detail_remaining = swarm.remaining;
                let runs_id = swarm_id.clone();
                let runs_name = swarm_name;
                MarketplaceCard::new().child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_1()
                                .child(
                                    Label::new(swarm.name.clone())
                                        .color(Color::Default)
                                        .truncate(),
                                )
                                .child(
                                    Label::new(swarm.description.clone())
                                        .color(Color::Muted)
                                        .size(LabelSize::XSmall)
                                        .truncate(),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .flex_shrink_0()
                                .items_center()
                                .child(
                                    Button::new(
                                        SharedString::from(format!("detail-{swarm_id}")),
                                        "Details",
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(Tooltip::text(
                                        "Open this swarm's roster and configuration view \
                                         — add/remove agents, view mission.",
                                    ))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.open_swarm_detail(
                                                detail_id.clone(),
                                                detail_name.clone(),
                                                detail_source.clone(),
                                                detail_mission.clone(),
                                                detail_agent_count,
                                                detail_budget,
                                                detail_remaining,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                .child(
                                    Button::new(
                                        SharedString::from(format!("runs-{swarm_id}")),
                                        "Runs",
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(Tooltip::text(
                                        "Show recent run activity (workspace messages) \
                                         for this swarm.",
                                    ))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.show_run_status(
                                                runs_id.clone(),
                                                runs_name.clone(),
                                                cx,
                                            );
                                        },
                                    )),
                                ),
                        ),
                )
            }
        }
    }
}
