//! The marketplace card renderer for agent and swarm entries. Extracted from
//! `swarm_panel.rs` — the renderer stays a method on `SwarmPanel` (it
//! dispatches via `cx.listener` into panel methods); this module owns the
//! view construction. See `author.rs` / `compose.rs` / `detail.rs` for the
//! same extraction pattern.

use gpui::{Context, SharedString};
use marketplace_ui_common::MarketplaceCard;
use ui::{Tooltip, prelude::*};

use crate::SwarmEntry;
use crate::SwarmPanel;
use crate::parse::{AgentSource, staleness_chip};

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
                let source_badge = source.badge();
                let source_label = source.label();
                // Clone-to-local button: visible for Cloud agents only.
                let show_clone = source == AgentSource::Cloud;
                // Push-to-cloud button: visible for Local agents only.
                let show_push = source == AgentSource::Local;
                // Remove-local button: Local-only agents can be removed (the
                // local counterpart of firing). Synced cards are kept — the
                // sync link would be orphaned.
                let show_remove = source == AgentSource::Local;
                // Publish button: visible for agents with an ABW presence
                // (Cloud or Synced). Local-only cards have no ABW agent to
                // publish — push to cloud first.
                let show_publish = source != AgentSource::Local;
                // Pre-clone agent_name for each button closure that needs it.
                let hire_name = agent_name.clone();
                let clone_name = agent_name.clone();
                let push_name = agent_name.clone();
                let remove_name = agent_name.clone();
                let publish_name = agent_name.clone();
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
                                    h_flex()
                                        .gap_2()
                                        .child(Label::new(agent.id.clone()).color(Color::Default))
                                        .child(
                                            Label::new(agent.agent_type.clone())
                                                .color(Color::Accent),
                                        )
                                        .child(
                                            Label::new(format!("▶ {}", agent.executions))
                                                .color(Color::Muted),
                                        )
                                        .child(
                                            Label::new(format!("by {}", agent.author))
                                                .color(Color::Muted),
                                        )
                                        .child(
                                            Label::new(format!(
                                                "{} {}",
                                                source_badge, source_label
                                            ))
                                            .color(Color::Accent)
                                            .size(LabelSize::XSmall),
                                        )
                                        .when_some(
                                            staleness_chip(&agent.updated_at),
                                            |this, (label, color)| {
                                                this.child(
                                                    Label::new(label)
                                                        .color(color)
                                                        .size(LabelSize::XSmall),
                                                )
                                            },
                                        ),
                                )
                                .child(Label::new(agent.description).color(Color::Muted)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .items_end()
                                .child(
                                    Label::new("Agent")
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
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
                let source_badge = swarm.source.badge();
                let source_label = swarm.source.label();
                // Each button closure gets its own clone (moved-in closures).
                let detail_id = swarm_id.clone();
                let detail_name = swarm_name.clone();
                let detail_source = swarm_source;
                let detail_mission = swarm_mission;
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
                                    h_flex()
                                        .gap_2()
                                        .child(Label::new(swarm.name.clone()).color(Color::Default))
                                        .child(
                                            Label::new(
                                                swarm
                                                    .agent_count
                                                    .map(|n| format!("{n} agents"))
                                                    .unwrap_or_else(|| "agents: -".to_string()),
                                            )
                                            .color(Color::Accent),
                                        )
                                        .child(
                                            Label::new(format!(
                                                "⛽ {}/{}",
                                                swarm
                                                    .remaining
                                                    .map_or("-".to_string(), |v| v.to_string()),
                                                swarm
                                                    .budget
                                                    .map_or("-".to_string(), |v| v.to_string())
                                            ))
                                            .color(Color::Muted),
                                        )
                                        .child(
                                            Label::new(format!(
                                                "{} {}",
                                                source_badge, source_label
                                            ))
                                            .color(Color::Accent)
                                            .size(LabelSize::XSmall),
                                        ),
                                )
                                .child(Label::new(swarm.description).color(Color::Muted)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .items_end()
                                .child(
                                    Label::new("Swarm")
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                                // Drill-down (item 4): open the roster view.
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
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                // Run status (item 3): show recent workspace
                                // messages.
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
