//! Agent and swarm card renderer for the browse list. Extracted from
//! `swarm_panel.rs` — the renderer stays a method on `SwarmPanel` (it
//! dispatches via `cx.listener` into panel methods); this module owns the
//! card construction.
//!
//! Cards are intentionally minimal: the agent/swarm **name** and a **brief
//! description** on the left, the action buttons on the right. Metadata
//! (agent_type, execution count, author, source badge, staleness, budget,
//! agent count) lives in the detail drill-down, not on the small card —
//! crowding the card with labels makes the list hard to scan.

use gpui::{Context, SharedString};
use marketplace_ui_common::MarketplaceCard;
use ui::{Tooltip, prelude::*};

use crate::parse::AgentSource;
use crate::{SwarmEntry, SwarmPanel};

impl SwarmPanel {
    pub(crate) fn render_card(
        &mut self,
        entry: SwarmEntry,
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
                                .child(Label::new(agent.id.clone()).color(Color::Default))
                                .child(Label::new(agent.description).color(Color::Muted)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .items_end()
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
                                            "Hire…"
                                        },
                                    )
                                    .style(ButtonStyle::Subtle)
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
                                .when(show_clone, |this| {
                                    this.child(
                                        Button::new(
                                            SharedString::from(format!("clone-{clone_name}")),
                                            "Clone to Local",
                                        )
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.spend_in_flight.is_some())
                                        .tooltip(Tooltip::text(
                                            "Copies this ABW agent to the local registry \
                                             (agents/local/curated) and marks it synced.",
                                        ))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.clone_to_local(clone_name.clone(), cx);
                                            }),
                                        ),
                                    )
                                })
                                .when(show_push, |this| {
                                    this.child(
                                        Button::new(
                                            SharedString::from(format!("push-{push_name}")),
                                            "Push to Cloud",
                                        )
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.spend_in_flight.is_some())
                                        .tooltip(Tooltip::text(
                                            "Publishes this local agent to the ABW catalogue \
                                             and links it via cloud_id (becomes synced).",
                                        ))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.push_to_cloud(push_name.clone(), cx);
                                            }),
                                        ),
                                    )
                                })
                                .when(show_publish, |this| {
                                    this.child(
                                        Button::new(
                                            SharedString::from(format!("publish-{publish_name}")),
                                            "Publish…",
                                        )
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.spend_in_flight.is_some())
                                        .tooltip(Tooltip::text(
                                            "Runs publish preflight checks, then publishes \
                                             the agent to the ABW catalogue. An admin \
                                             force-publish path is available if checks fail.",
                                        ))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.begin_publish(publish_name.clone(), cx);
                                            }),
                                        ),
                                    )
                                })
                                .when(show_remove, |this| {
                                    this.child(
                                        Button::new(
                                            SharedString::from(format!("remove-{remove_name}")),
                                            "Remove",
                                        )
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.spend_in_flight.is_some())
                                        .tooltip(Tooltip::text(
                                            "Deletes this local-only agent card. A synced \
                                             card's ABW agent is untouched.",
                                        ))
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.remove_local_agent(remove_name.clone(), cx);
                                            }),
                                        ),
                                    )
                                }),
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
                                .child(Label::new(swarm.name.clone()).color(Color::Default))
                                .child(Label::new(swarm.description).color(Color::Muted)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .items_end()
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
                                        "Run Status",
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
