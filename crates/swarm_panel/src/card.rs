//! Agent and swarm card renderer for the browse list.
//!
//! Layout: name + truncated description on the left (flex_1, min_w_0,
//! truncate). Primary actions (Edit, Hire) as visible buttons on the
//! right (flex_shrink_0). Secondary actions (Clone, Push, Publish,
//! Remove) collapsed behind an ellipsis PopoverMenu so the button row
//! never overflows the text column on narrow dock panels.
//!
//! Pattern reference: `agent_panel.rs`, `render_panel_options_menu`
//! (PopoverMenu + IconButton Ellipsis + ContextMenu::build + WeakEntity
//! handler capture).

use gpui::{Context, SharedString, WeakEntity, Window};
use marketplace_ui_common::MarketplaceCard;
use ui::{ContextMenu, IconButton, IconName, IconSize, PopoverMenu, Tooltip, prelude::*};

use crate::parse::AgentSource;
use crate::{SwarmEntry, SwarmPanel};

impl SwarmPanel {
    pub(crate) fn render_card(
        &mut self,
        entry: SwarmEntry,
        _window: &mut Window,
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
                let has_secondary = show_clone || show_push || show_publish || show_remove;
                let hire_name = agent_name.clone();
                let clone_name = agent_name.clone();
                let push_name = agent_name.clone();
                let remove_name = agent_name.clone();
                let publish_name = agent_name.clone();
                let edit_name = agent_name.clone();
                let edit_source = source;
                let panel_handle: WeakEntity<SwarmPanel> = cx.entity().downgrade();

                MarketplaceCard::new().child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        // Left: name + description. min_w_0 + flex_1 so the
                        // text column shrinks (not the buttons) when space is
                        // tight. truncate so long names/descriptions ellipsize
                        // instead of blowing out the card.
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
                                    Label::new(agent.description)
                                        .color(Color::Muted)
                                        .size(LabelSize::XSmall)
                                        .truncate(),
                                ),
                        )
                        // Right: primary actions (Edit + Hire) as visible
                        // buttons, secondary actions behind a PopoverMenu
                        // ellipsis trigger. flex_shrink_0 so the button
                        // column keeps its width and the text column absorbs
                        // the squeeze.
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
                                        if self.spend.in_flight.as_deref()
                                            == Some(agent_name.as_str())
                                        {
                                            "Hiring…"
                                        } else {
                                            "Hire"
                                        },
                                    )
                                    .style(ButtonStyle::Filled)
                                    .label_size(LabelSize::XSmall)
                                    .disabled(self.spend.in_flight.is_some())
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
                                .when(has_secondary, |this| {
                                    this.child(
                                        PopoverMenu::new(SharedString::from(format!(
                                            "agent-secondary-actions-{agent_name}",
                                        )))
                                            .trigger_with_tooltip(
                                                IconButton::new(
                                                    SharedString::from(format!(
                                                        "agent-secondary-trigger-{agent_name}",
                                                    )),
                                                    IconName::Ellipsis,
                                                )
                                                .icon_size(IconSize::Small),
                                                Tooltip::text("More actions"),
                                            )
                                            .menu({
                                                let panel_handle = panel_handle.clone();
                                                move |window, cx| {
                                                    let panel_handle = panel_handle.clone();
                                                    let clone_name = clone_name.clone();
                                                    let push_name = push_name.clone();
                                                    let publish_name = publish_name.clone();
                                                    let remove_name = remove_name.clone();
                                                    let show_clone = show_clone;
                                                    let show_push = show_push;
                                                    let show_publish = show_publish;
                                                    let show_remove = show_remove;
                                                    Some(ContextMenu::build(
                                                        window,
                                                        cx,
                                                        |mut menu, _window, _cx| {
                                                            if show_clone {
                                                                let panel_handle =
                                                                    panel_handle.clone();
                                                                menu = menu.entry(
                                                                    "Clone to Local",
                                                                    None,
                                                                    move |_, cx| {
                                                                        if let Some(panel) =
                                                                            panel_handle.upgrade()
                                                                        {
                                                                            panel.update(cx, |this, cx| {
                                                                                this.clone_to_local(clone_name.clone(), cx);
                                                                            });
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                            if show_push {
                                                                let panel_handle =
                                                                    panel_handle.clone();
                                                                menu = menu.entry(
                                                                    "Push to Cloud",
                                                                    None,
                                                                    move |_, cx| {
                                                                        if let Some(panel) =
                                                                            panel_handle.upgrade()
                                                                        {
                                                                            panel.update(cx, |this, cx| {
                                                                                this.push_to_cloud(push_name.clone(), cx);
                                                                            });
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                            if show_publish {
                                                                let panel_handle =
                                                                    panel_handle.clone();
                                                                menu = menu.entry(
                                                                    "Publish…",
                                                                    None,
                                                                    move |_, cx| {
                                                                        if let Some(panel) =
                                                                            panel_handle.upgrade()
                                                                        {
                                                                            panel.update(cx, |this, cx| {
                                                                                this.begin_publish(publish_name.clone(), cx);
                                                                            });
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                            if show_remove {
                                                                let panel_handle =
                                                                    panel_handle.clone();
                                                                menu = menu.entry(
                                                                    "Remove",
                                                                    None,
                                                                    move |_, cx| {
                                                                        if let Some(panel) =
                                                                            panel_handle.upgrade()
                                                                        {
                                                                            panel.update(cx, |this, cx| {
                                                                                this.remove_local_agent(remove_name.clone(), cx);
                                                                            });
                                                                        }
                                                                    },
                                                                );
                                                            }
                                                            menu
                                                        },
                                                    ))
                                                }
                                            }),
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
                                .child(
                                    Label::new(swarm.name.clone())
                                        .color(Color::Default)
                                        .truncate(),
                                )
                                .child(
                                    Label::new(swarm.description)
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
