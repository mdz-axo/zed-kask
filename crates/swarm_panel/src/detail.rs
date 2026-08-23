//! The swarm-detail (roster drill-down) renderer. Extracted from
//! `swarm_panel.rs` — the renderer stays a method on `SwarmPanel` (it
//! dispatches via `cx.listener` into panel methods); this module owns the view
//! construction. See `author.rs` / `compose.rs` for the same extraction
//! pattern.

use gpui::{Context, SharedString};
use ui::{Tooltip, prelude::*};

use crate::DestructiveAction;
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
        let pending = self.detail.pending_destructive.is_some();
        let in_flight = self.spend.in_flight.is_some();

        v_flex()
            .w_full()
            .gap_2()
            // ── Header: back + headline + source badge ─────────────────────
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        IconButton::new("back-to-swarms", IconName::ArrowLeft)
                            .icon_size(IconSize::Small)
                            .tooltip(Tooltip::text("Back to the swarm list."))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_swarm_detail(cx);
                            })),
                    )
                    .child(
                        Headline::new(format!("{} — roster", detail.name))
                            .size(HeadlineSize::Small),
                    )
                    .child(div().flex_1())
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
            // ── Metadata: edit form or read-only mission ───────────────────
            .when(detail.editing_metadata, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(
                                    Label::new("Name")
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .id("swarm-edit-name")
                                        .border_1()
                                        .border_color(border)
                                        .rounded_sm()
                                        .child(self.detail.edit_name_editor.clone()),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .items_start()
                                .child(
                                    Label::new("Mission")
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .id("swarm-edit-mission")
                                        .border_1()
                                        .border_color(border)
                                        .rounded_sm()
                                        .child(self.detail.edit_mission_editor.clone()),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(
                                        SharedString::from(format!(
                                            "save-metadata-{}",
                                            detail.workspace_id
                                        )),
                                        "Save",
                                    )
                                    .style(ButtonStyle::Filled)
                                    .label_size(LabelSize::XSmall)
                                    .disabled(in_flight)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_swarm_metadata(cx);
                                    })),
                                )
                                .child(
                                    Button::new(
                                        SharedString::from(format!(
                                            "cancel-metadata-{}",
                                            detail.workspace_id
                                        )),
                                        "Cancel",
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_edit_metadata(cx);
                                    })),
                                ),
                        ),
                )
            })
            .when(!detail.editing_metadata, |this| {
                this.when(!detail.mission.is_empty(), |this| {
                    this.child(
                        Label::new(format!("mission: {}", detail.mission))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                })
            })
            // ── Budget signals (3 labels, measured) ─────────────────────────
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Label::new(format!(
                            "agents: {}",
                            detail
                                .agent_count
                                .map_or("-".to_string(), |c| c.to_string())
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!(
                            "budget: {}",
                            detail.budget.map_or("-".to_string(), |b| b.to_string())
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    )
                    .child(
                        Label::new(format!(
                            "remaining: {}",
                            detail.remaining.map_or("-".to_string(), |r| r.to_string())
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    ),
            )
            // ── Confirmation banner for destructive actions ────────────────
            .when_some(self.detail.pending_destructive.clone(), |this, action| {
                this.child(self.render_destructive_confirmation(&action, detail, cx))
            })
            // ── Swarm-level actions: Edit, Clone, Delete (3 buttons max) ────
            .when(!pending && !detail.editing_metadata, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        // Edit metadata — local only (ABW has no edit endpoint).
                        .when(is_local, |this| {
                            this.child(
                                Button::new(
                                    SharedString::from(format!(
                                        "edit-swarm-{}",
                                        detail.workspace_id
                                    )),
                                    "Edit",
                                )
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .disabled(in_flight)
                                .tooltip(Tooltip::text(
                                    "Edit this swarm's name and mission. \
                                     Local swarms only — ABW has no metadata-edit \
                                     endpoint.",
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.begin_edit_metadata(window, cx);
                                })),
                            )
                        })
                        // Clone — both backends. Local: one-click via
                        // `swarm_clone_local_swarm`. ABW: pre-fills the Compose
                        // form for operator review (clone = read + create).
                        .child(
                            Button::new(
                                SharedString::from(format!(
                                    "clone-swarm-{}",
                                    detail.workspace_id
                                )),
                                "Copy",
                            )
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .disabled(in_flight)
                            .tooltip(Tooltip::text(if is_local {
                                "Creates a copy of this swarm with a fresh id, \
                                 same mission and roster. No cost."
                            } else {
                                "Copies this swarm's name, mission, and roster \
                                 into the Compose form for review. The create \
                                 handles consent and credit cost."
                            }))
                            .on_click(cx.listener({
                                let swarm_id = detail.workspace_id.clone();
                                move |this, _, window, cx| {
                                    if is_local {
                                        this.clone_local_swarm(swarm_id.clone(), cx);
                                    } else {
                                        this.clone_swarm_to_compose(window, cx);
                                    }
                                }
                            })),
                        )
                        .child(div().flex_1())
                        // Delete — both backends. Stages a confirmation; the
                        // actual delete fires from `confirm_destructive`.
                        .child(
                            Button::new(
                                SharedString::from(format!(
                                    "delete-swarm-{}",
                                    detail.workspace_id
                                )),
                                if is_local { "Delete Swarm" } else { "Delete" },
                            )
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .disabled(in_flight)
                            .tooltip(Tooltip::text(if is_local {
                                "Permanently deletes this local swarm and its \
                                 roster. Member agent cards are NOT deleted. \
                                 Confirmation required."
                            } else {
                                "Permanently deletes this ABW workspace (swarm) \
                                 and its roster. Irreversible. Confirmation \
                                 required; active runs are shown before delete."
                            }))
                            .on_click(cx.listener({
                                let swarm_id = detail.workspace_id.clone();
                                let source = detail.source.clone();
                                let name = detail.name.clone();
                                move |this, _, _, cx| {
                                    this.request_delete_swarm(
                                        swarm_id.clone(),
                                        source.clone(),
                                        name.clone(),
                                        cx,
                                    );
                                }
                            })),
                        ),
                )
            })
            // ── Add-agent affordance: input + button ───────────────────────
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
                            .child(self.detail.add_agent_editor.clone()),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("add-agent-{}", detail.workspace_id)),
                            add_label,
                        )
                        .style(ButtonStyle::Filled)
                        .label_size(LabelSize::XSmall)
                        .disabled(in_flight)
                        .tooltip(Tooltip::text(add_tooltip))
                        .on_click(cx.listener({
                            let workspace_id = detail.workspace_id.clone();
                            move |this, _, window, cx| {
                                let agent_name = this
                                    .detail
                                    .add_agent_editor
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
                                    this.selected_workspace = Some(workspace_id.clone());
                                    this.begin_hire(agent_name, cx);
                                }
                                this.detail
                                    .add_agent_editor
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
            // ── Roster rows: composition view with accepts/produces ─────────
            .when(!detail.agents.is_empty(), |this| {
                this.child(v_flex().gap_1().children(detail.agents.iter().map(|a| {
                    let row_agent_id = a.agent_id.clone();
                    let row_workspace = detail.workspace_id.clone();
                    let row_remove_label = remove_label;
                    let row_is_local = is_local;
                    let row_source = detail.source.clone();
                    h_flex()
                        .gap_2()
                        .items_start()
                        // Left: agent identity + ports (flex_1, min_w_0 so the
                        // remove button keeps its width and the text column
                        // absorbs the squeeze).
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(
                                            Label::new(a.agent_id.clone())
                                                .color(Color::Default)
                                                .truncate(),
                                        )
                                        .when(!a.agent_type.is_empty(), |this| {
                                            this.child(
                                                Label::new(a.agent_type.clone())
                                                    .color(Color::Accent)
                                                    .size(LabelSize::XSmall),
                                            )
                                        }),
                                )
                                // Port labels: composition signal. Shown
                                // only when non-empty (local rosters enriched
                                // from the agent catalogue; ABW rosters may
                                // carry them).
                                .when(!a.accepts.is_empty() || !a.produces.is_empty(), |this| {
                                    this.child(
                                        h_flex()
                                            .gap_1()
                                            .flex_wrap()
                                            .when(!a.accepts.is_empty(), |this| {
                                                this.child(
                                                    Label::new(format!(
                                                        "→ {}",
                                                        a.accepts.join(", ")
                                                    ))
                                                    .color(Color::Muted)
                                                    .size(LabelSize::XSmall),
                                                )
                                            })
                                            .when(!a.produces.is_empty(), |this| {
                                                this.child(
                                                    Label::new(format!(
                                                        "← {}",
                                                        a.produces.join(", ")
                                                    ))
                                                    .color(Color::Muted)
                                                    .size(LabelSize::XSmall),
                                                )
                                            }),
                                    )
                                })
                                .when(!a.description.is_empty(), |this| {
                                    this.child(
                                        Label::new(a.description.clone())
                                            .color(Color::Muted)
                                            .size(LabelSize::XSmall)
                                            .truncate(),
                                    )
                                }),
                        )
                        // Right: remove/fire button (flex_shrink_0).
                        .child(
                            Button::new(
                                SharedString::from(format!("roster-remove-{row_agent_id}")),
                                row_remove_label,
                            )
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .disabled(in_flight || pending)
                            .tooltip(Tooltip::text(if row_is_local {
                                "Removes this agent from the swarm's roster. \
                                 Idempotent. The agent card is not deleted. \
                                 Confirmation required."
                            } else {
                                "Fires this agent from the ABW workspace. \
                                 No credit cost; the agent itself is not deleted. \
                                 Confirmation required."
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_remove_agent(
                                    row_workspace.clone(),
                                    row_agent_id.clone(),
                                    row_source.clone(),
                                    cx,
                                );
                            })),
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
    }

    /// Render the confirmation banner for a pending destructive action.
    /// Shows the action description, any active-run warning (for ABW swarm
    /// deletes), and Confirm / Cancel buttons. The Confirm button dispatches
    /// to `confirm_destructive`; Cancel dispatches to `cancel_destructive`.
    fn render_destructive_confirmation(
        &self,
        action: &DestructiveAction,
        detail: &SwarmDetailView,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let in_flight = self.spend.in_flight.is_some();
        let (warning_text, confirm_label) = match action {
            DestructiveAction::DeleteSwarm { name, .. } => {
                let run_warning = self
                    .detail
                    .run_status
                    .as_ref()
                    .filter(|rs| !rs.messages.is_empty() && rs.error.is_none())
                    .map(|rs| {
                        format!(
                            " ⚠ {} active run message(s) — deleting will lose this history.",
                            rs.messages.len()
                        )
                    })
                    .unwrap_or_default();
                (
                    format!(
                        "Delete swarm '{}'? This is irreversible — the swarm and its roster will be removed.{}",
                        name, run_warning
                    ),
                    "Confirm Delete",
                )
            }
            DestructiveAction::RemoveAgent { agent_id, .. } => (
                format!(
                    "Remove '{}' from this swarm? The agent card is not deleted.",
                    agent_id
                ),
                "Confirm Remove",
            ),
        };

        v_flex()
            .gap_1()
            .p_2()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_sm()
            .child(
                Label::new(warning_text)
                    .size(LabelSize::XSmall)
                    .color(Color::Warning),
            )
            // Active-run messages (ABW swarm delete only). Surface the
            // recent messages so the operator can see what will be lost.
            .when_some(
                self.detail.run_status.as_ref().and_then(|rs| {
                    if rs.messages.is_empty() || rs.error.is_some() {
                        None
                    } else {
                        Some(rs.messages.clone())
                    }
                }),
                |this, messages| {
                    this.child(
                        v_flex()
                            .gap_0p5()
                            .children(
                                messages
                                    .iter()
                                    .take(3)
                                    .map(|msg| {
                                        Label::new(msg.clone())
                                            .size(LabelSize::XSmall)
                                            .color(Color::Muted)
                                            .truncate()
                                    }),
                            ),
                    )
                },
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(
                            SharedString::from(format!(
                                "confirm-destructive-{}",
                                detail.workspace_id
                            )),
                            confirm_label,
                        )
                        .style(ButtonStyle::Filled)
                        .label_size(LabelSize::XSmall)
                        .disabled(in_flight)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_destructive(cx);
                        })),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!(
                                "cancel-destructive-{}",
                                detail.workspace_id
                            )),
                            "Cancel",
                        )
                        .style(ButtonStyle::Subtle)
                        .label_size(LabelSize::XSmall)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_destructive(cx);
                        })),
                    ),
            )
    }
}
