//! Agent and swarm card renderer for the browse list.
//!
//! Layout: name + truncated description on the left (flex_1, min_w_0,
//! truncate). Primary actions (Edit) as visible buttons on the right
//! (flex_shrink_0). The Remove action for agents is collapsed behind an
//! ellipsis PopoverMenu so the button row never overflows the text column
//! on narrow dock panels.
//!
//! Pattern reference: `agent_panel.rs`, `render_panel_options_menu`
//! (PopoverMenu + IconButton Ellipsis + ContextMenu::build + WeakEntity
//! handler capture).

use gpui::{Context, SharedString, WeakEntity, Window};
use marketplace_ui_common::MarketplaceCard;
use ui::{ContextMenu, IconButton, IconName, IconSize, PopoverMenu, Tooltip, prelude::*};

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
                let remove_name = agent_name.clone();
                let edit_name = agent_name.clone();
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
                                    move |this, _event, window, cx| {
                                        this.load_agent_into_author(edit_name.clone(), window, cx);
                                    }
                                }))
                                .child(
                                    Label::new(if agent.display_name.is_empty() {
                                        agent.id.clone()
                                    } else {
                                        agent.display_name.clone()
                                    })
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
                        // Right: primary action (Edit) as a visible button,
                        // the destructive Remove behind a PopoverMenu ellipsis
                        // trigger. flex_shrink_0 so the button column keeps its
                        // width and the text column absorbs the squeeze.
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
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                                )
                                .child(
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
                                        move |window, cx| {
                                            let panel_handle = panel_handle.clone();
                                            let remove_name = remove_name.clone();
                                            Some(ContextMenu::build(
                                                window,
                                                cx,
                                                |mut menu, _window, _cx| {
                                                    let panel_handle = panel_handle.clone();
                                                    menu =
                                                        menu.entry("Remove", None, move |_, cx| {
                                                            if let Some(panel) =
                                                                panel_handle.upgrade()
                                                            {
                                                                panel.update(cx, |this, cx| {
                                                                    this.remove_local_agent(
                                                                        remove_name.clone(),
                                                                        cx,
                                                                    );
                                                                });
                                                            }
                                                        });
                                                    menu
                                                },
                                            ))
                                        }
                                    }),
                                ),
                        ),
                )
            }
            SwarmEntry::Swarm(swarm) => {
                let swarm_id = swarm.id.clone();
                let swarm_name = swarm.name.clone();
                let swarm_mission = swarm.description.clone();
                let detail_id = swarm_id.clone();
                let detail_name = swarm_name.clone();
                let detail_mission = swarm_mission;
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
                                        "Edit",
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(Tooltip::text(
                                        "Open this swarm in the compose view to \
                                         view and edit its details, mission, and roster.",
                                    ))
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.select_swarm_for_steer(detail_id.clone(), cx);
                                            this.load_swarm_into_compose(
                                                detail_id.clone(),
                                                detail_name.clone(),
                                                detail_mission.clone(),
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                // Delete button — visible on the card. Stages
                                // a confirmation banner in the browse list.
                                .child(
                                    Button::new(
                                        SharedString::from(format!("card-delete-{swarm_id}")),
                                        "Delete",
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .tooltip(Tooltip::text(
                                        "Delete this swarm. Shows a confirmation \
                                         prompt above the list.",
                                    ))
                                    .on_click(cx.listener({
                                        let name = swarm_name;
                                        move |this, _, _, cx| {
                                            // Stage the delete confirmation —
                                            // the browse list renders the
                                            // confirmation banner.
                                            this.request_delete_swarm(
                                                swarm_id.clone(),
                                                name.clone(),
                                                cx,
                                            );
                                        }
                                    })),
                                ),
                        ),
                )
            }
        }
    }
}
