//! The swarm-composition surface: form state, editor construction, the
//! `render_compose` renderer (including the Xaman Ek composition
//! consultant), the compose-form entry points (`load_swarm_into_compose`,
//! `clone_swarm_to_compose`, `reset_compose_form_for_create`), and the
//! `create_swarm` submit flow. Extracted from `swarm_panel.rs` — the
//! renderer and the flows stay methods on `SwarmPanel` (they mutate panel
//! state via `cx.spawn` + `this.update`); this module owns the form struct,
//! the view construction, and the flows.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;
use ui::{
    ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip,
    prelude::*,
};

use crate::CreateTarget;
use crate::MIN_AGENTS_TO_LAUNCH;
use crate::PanelMode;
use crate::PendingCompositionPrompt;
use crate::SWARM_SERVER;
use crate::SwarmPanel;
use crate::parse::{AgentSource, extract_wallet_balance};
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
    /// When `Some`, the form is editing an existing swarm (loaded via the
    /// Edit button on the browse card). The name field is read-only for
    /// local swarms (renaming is a separate `swarm_update_local_swarm`
    /// operation). When `None`, the form is creating a new swarm.
    pub(crate) editing_swarm_id: Option<String>,
    /// The source of the swarm being edited (`Cloud`, `Local`, `Synced`).
    pub(crate) editing_swarm_source: Option<crate::parse::AgentSource>,
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
            editing_swarm_id: None,
            editing_swarm_source: None,
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
        let is_editing = self.compose.editing_swarm_id.is_some();
        let create_label: SharedString = if self.compose.busy {
            "Creating…"
        } else if is_local {
            "Create Local Swarm"
        } else {
            "Create Swarm"
        }
        .into();
        let create_tooltip: &str = if is_local {
            "Creates the swarm on the local substrate (agents/local/swarms). No cost, no consent."
        } else {
            "Creates the ABW workspace and hires the listed agents. Each hire is pre-flighted for cost and consent-gated; the swarm is not created if any consent fails."
        };
        v_flex()
            .w_full()
            .gap_3()
            // py — the top pad is the gap between the mode-toggle row and the
            // form headline (the content column has no top pad of its own);
            // the bottom pad gives the scroll end breathing room. Horizontal
            // inset comes from the column's px_4 — px here would double it vs
            // Browse.
            .py_4()
            .child(Headline::new("Compose a Swarm").size(HeadlineSize::Small))
            // When editing an existing swarm, show the swarm id as
            // context. Using .when(is_editing) so the label is only
            // rendered when editing — the editing_swarm_id string is
            // dynamic (comes from the loaded swarm), so it survives LTO.
            .when(is_editing, |this| {
                this.child(
                    Label::new(self.compose.editing_swarm_id.as_deref().unwrap_or(""))
                        .size(LabelSize::XSmall)
                        .color(Color::Accent),
                )
            })
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

    /// Open the compose panel with `swarm`'s existing details loaded, so the
    /// operator can view and edit the composition. Mirrors
    /// `load_agent_into_author` for swarms: fetches the swarm's roster, pre-fills
    /// the compose form (name, mission, agents), and switches to Compose mode.
    ///
    /// Sets `editing_swarm_id` on the form so the submit path knows it's an
    /// edit, not a create. The name field is kept editable for local swarms
    /// (the save calls `swarm_update_local_swarm`); for cloud swarms the name
    /// is read-only (ABW has no metadata-edit endpoint) but the roster can
    /// still be reviewed.
    pub(crate) fn load_swarm_into_compose(
        &mut self,
        swarm_id: String,
        name: String,
        source: AgentSource,
        mission: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Set editing state on the compose form.
        self.compose.editing_swarm_id = Some(swarm_id.clone());
        self.compose.editing_swarm_source = Some(source.clone());
        self.compose.create_target = match source {
            AgentSource::Cloud => crate::CreateTarget::Cloud,
            AgentSource::Local | AgentSource::Synced => crate::CreateTarget::Local,
        };
        self.compose.status = Some(
            ["Loading swarm details…", "Loading swarm details…"][0]
                .to_string()
                .into(),
        );
        self.compose.busy = false;

        // Pre-fill the form with the known values. The roster (agents)
        // will be enriched after the fetch — for now set what we have
        // from the card.
        self.compose
            .name
            .update(cx, |e, cx| e.set_text(name, window, cx));
        self.compose
            .mission
            .update(cx, |e, cx| e.set_text(mission, window, cx));
        self.compose
            .agents
            .update(cx, |e, cx| e.set_text(String::new(), window, cx));

        // Close any open detail view and switch to Compose mode.
        self.close_swarm_detail(cx);
        self.set_mode(crate::PanelMode::Compose, window, cx);

        // Fetch the swarm's roster to populate the agents field.
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.compose.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let is_local = source == AgentSource::Local;
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = if is_local {
                    invoker
                        .invoke_tool(
                            crate::SWARM_SERVER,
                            "swarm_get_local_swarm",
                            json!({ "swarm_id": swarm_id }),
                        )
                        .await
                } else {
                    invoker
                        .invoke_tool(
                            crate::SWARM_SERVER,
                            "swarm_get_swarm",
                            json!({ "workspace_id": swarm_id }),
                        )
                        .await
                };
                this.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            // Extract member agent ids from the response.
                            let members: Vec<String> = if is_local {
                                parsed
                                    .and_then(|c| {
                                        c.get("members").and_then(|m| m.as_array()).map(|members| {
                                            members
                                                .iter()
                                                .filter_map(|m| m.as_str().map(str::to_string))
                                                .collect::<Vec<_>>()
                                        })
                                    })
                                    .unwrap_or_default()
                            } else {
                                // ABW roster: extract agent ids from the
                                // workspace payload.
                                parsed
                                    .and_then(|c| {
                                        let candidates = [
                                            c.get("agents"),
                                            c.get("workspace").and_then(|w| w.get("agents")),
                                            c.get("team").and_then(|t| t.get("agents")),
                                        ];
                                        candidates.into_iter().find_map(|c| c?.as_array()).map(
                                            |agents| {
                                                agents
                                                    .iter()
                                                    .filter_map(|a| {
                                                        a.get("agent_id")
                                                            .or_else(|| a.get("agent_name"))
                                                            .and_then(|v| v.as_str())
                                                            .map(str::to_string)
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        )
                                    })
                                    .unwrap_or_default()
                            };
                            let agents_str = members.join(", ");
                            this.compose
                                .agents
                                .update(cx, |e, cx| e.set_text(agents_str, window, cx));
                            this.compose.status = None;
                        }
                        Err(err) => {
                            this.compose.status =
                                Some(format!("Failed to load swarm roster: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Copy an ABW swarm by pre-filling the Compose form with the source
    /// swarm's name, mission, and roster, then navigating to Compose mode.
    /// The operator reviews and completes the create (which handles consent
    /// and credit cost). This avoids inventing an ABW clone endpoint — clone
    /// = read + create, using existing tools. Missing agents surface as hire
    /// failures during the create flow.
    pub(crate) fn clone_swarm_to_compose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.swarm_detail.as_ref() else {
            return;
        };
        let name = format!("{} (copy)", detail.name);
        let mission = detail.mission.clone();
        let agents = detail
            .agents
            .iter()
            .map(|a| a.agent_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        self.compose
            .name
            .update(cx, |e, cx| e.set_text(name, window, cx));
        self.compose
            .mission
            .update(cx, |e, cx| e.set_text(mission, window, cx));
        self.compose
            .agents
            .update(cx, |e, cx| e.set_text(agents, window, cx));
        self.compose.create_target = crate::CreateTarget::Cloud;
        self.compose.status = Some("Review and create to copy this ABW swarm.".into());
        self.close_swarm_detail(cx);
        self.set_mode(crate::PanelMode::Compose, window, cx);
        cx.notify();
    }

    /// Reset the compose form to a fresh create state (clear
    /// `editing_swarm_id`, make the name field editable again). Called
    /// when the operator clicks the Compose mode toggle in the header.
    pub(crate) fn reset_compose_form_for_create(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compose.editing_swarm_id = None;
        self.compose.editing_swarm_source = None;
        self.compose.create_target = self.active_backend;
        self.compose.status = None;
        self.compose.name.update(cx, |e, cx| e.clear(window, cx));
        self.compose.mission.update(cx, |e, cx| e.clear(window, cx));
        self.compose.agents.update(cx, |e, cx| e.clear(window, cx));
    }

    /// Create a new swarm from the compose form. Mode-aware: in Local mode the
    /// swarm is created on the local substrate via `swarm_create_local_swarm`
    /// (no cost, no consent — members are agent ids); in ABW mode the existing
    /// consent-gated `swarm_create_swarm` path is used, hiring any listed agents.
    fn create_swarm(&mut self, cx: &mut Context<Self>) {
        // If editing an existing swarm, dispatch to the update path
        // instead of creating a new one. This handles the case where
        // the operator loaded a swarm into the compose form via the
        // Edit button and is now saving changes.
        if self.compose.editing_swarm_id.is_some() {
            self.save_swarm_metadata(cx);
            return;
        }

        let Some(invoker) = crate::shared_tool_invoker() else {
            self.compose.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let name = self.compose.name.read(cx).text(cx);
        if name.trim().is_empty() {
            self.compose.status = Some("Swarm name is required.".into());
            cx.notify();
            return;
        }
        // Warn on excessively long names — the server may truncate or reject,
        // and a name over 128 chars is almost certainly a paste error.
        if name.trim().chars().count() > 128 {
            self.compose.status = Some("Swarm name is too long (max 128 characters).".into());
            cx.notify();
            return;
        }
        let mission = self.compose.mission.read(cx).text(cx);
        let agents_raw = self.compose.agents.read(cx).text(cx);
        let agents: Vec<String> = agents_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let is_local = self.compose.create_target == CreateTarget::Local;

        // Launch gate: a swarm is not ready for the swarm-intelligence PDCA
        // loop unless it has a mission (the task context SENSE derives
        // required_transforms from) and at least MIN_AGENTS_TO_LAUNCH agents
        // (below that, variety_coverage and diversity are trivially 0/1 and
        // the loop converges without doing composition work). The operator
        // must complete the compose form before creating.
        if mission.trim().is_empty() {
            self.compose.status = Some(
                "Mission is required to launch a swarm. Describe what the swarm should do.".into(),
            );
            cx.notify();
            return;
        }
        if agents.len() < MIN_AGENTS_TO_LAUNCH {
            self.compose.status = Some(format!(
                "At least {} agents are required to launch a swarm. Add agents to the roster ({} provided).",
                MIN_AGENTS_TO_LAUNCH,
                agents.len()
            ).into());
            cx.notify();
            return;
        }

        self.compose.busy = true;
        self.compose.status = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            // Local mode: create the swarm on the local substrate directly — no
            // cost, no consent tokens. Members are agent ids (the local swarm
            // roster is ids; resolution happens at delegation time).
            if is_local {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_create_local_swarm",
                        json!({
                            "name": name.trim(),
                            "mission": mission.trim(),
                            "agents": agents,
                        }),
                    )
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.compose.busy = false;
                    match result {
                        Ok(output) => {
                            // Extract the swarm_id from the response so we can
                            // select it and navigate to Steer.
                            let swarm_id = parse_tool_response(&output)
                                .and_then(|c| c.get("swarm_id").and_then(|v| v.as_str()).map(str::to_string));
                            this.compose.status =
                                Some(format!("Local swarm '{}' created.", name.trim()).into());
                            this.fetch_all(cx);
                            // Navigate to Steer with the new swarm selected.
                            if let Some(id) = swarm_id {
                                this.selected_workspace = Some(id.clone());
                                // Drop any existing Steer conversation so the
                                // next construction bakes in the new swarm.
                                this.steer.invalidate();
                                this.set_mode(PanelMode::Steer, window, cx);
                                // Queue the composition prompt for injection
                                // after `render` constructs the Steer
                                // conversation. The prompt carries the mode,
                                // swarm_id, mission, and seeded agents so
                                // swarm-intelligence SENSE can derive
                                // required_transforms and assess the initial
                                // roster.
                                this.pending_composition_prompt =
                                    Some(PendingCompositionPrompt {
                                        swarm_id: id,
                                        mission: mission.trim().to_string(),
                                        agents: agents.clone(),
                                        is_local: true,
                                    });
                            }
                        }
                        Err(err) => {
                            this.compose.status = Some(format!("Create failed: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
                return;
            }
            // Mint a consent token per agent to hire (each hire is gated).
            // Fetch the real hire cost per agent first (BH-02): a hardcoded
            // `credits_authorized: 5` would under-authorize an agent that
            // costs 20, and the server's re-verify would reject the hire —
            // but only after the workspace was already created. Fetching the
            // cost up front lets us abort before any ABW mutation and pass
            // the real ceiling to the consent token.
            // A spend path must not silently degrade: if any consent mint
            // fails, abort the create rather than hiring a partial team.
            let mut consent_tokens = Vec::new();
            let mut consent_failures = Vec::new();
            for agent in &agents {
                // Step 1: fetch the real hire cost.
                let cost_result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_hire_cost",
                        json!({ "agent_name": agent }),
                    )
                    .await;
                let credits = match cost_result {
                    Ok(output) => {
                        parse_tool_response(&output).and_then(|c| {
                            c.get("total_hire_cost").and_then(|v| v.as_u64())
                        }).map(|c| c as u32)
                    }
                    Err(err) => {
                        log::warn!("swarm-panel: hire cost fetch for '{agent}' failed: {err}");
                        consent_failures.push(agent.clone());
                        continue;
                    }
                };
                let Some(credits) = credits else {
                    log::warn!("swarm-panel: hire cost fetch for '{agent}' returned no total_hire_cost");
                    consent_failures.push(agent.clone());
                    continue;
                };
                // Step 2: mint the consent token with the real cost.
                match invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_request_consent",
                        json!({ "action": "hire", "target": agent, "credits_authorized": credits }),
                    )
                    .await
                {
                    Ok(output) => {
                        let token = parse_tool_response(&output).and_then(|c| {
                            c.get("consent_token")
                                .and_then(|t| t.as_str())
                                .map(str::to_string)
                        });
                        match token {
                            Some(t) => consent_tokens.push(t),
                            None => {
                                log::warn!("swarm-panel: consent mint for '{agent}' returned no token");
                                consent_failures.push(agent.clone());
                            }
                        }
                    }
                    Err(err) => {
                        log::warn!("swarm-panel: consent mint for '{agent}' failed: {err}");
                        consent_failures.push(agent.clone());
                    }
                }
            }

            // Abort on any consent failure — do not create a swarm with a
            // silently under-consented team.
            if !consent_failures.is_empty() {
                this.update(cx, |this, cx| {
                    this.compose.busy = false;
                    this.compose.status = Some(
                        format!(
                            "Consent failed for {} — swarm not created.",
                            consent_failures.join(", ")
                        )
                        .into(),
                    );
                    cx.notify();
                })
                .ok();
                return;
            }

            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_create_swarm",
                    json!({
                        "name": name.trim(),
                        "mission": if mission.trim().is_empty() { None } else { Some(mission.trim()) },
                        "agents": agents,
                        "consent_tokens": consent_tokens,
                    }),
                )
                .await;
            this.update_in(cx, |this, window, cx| {
                this.compose.busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.spend.wallet_balance = Some(b);
                        }
                        // Surface any per-hire errors the server reported
                        // (BH-07): the workspace is created but some hires may
                        // have failed (cost re-verify, network drop). The
                        // operator must not see "Swarm created." while all
                        // hires silently failed.
                        let hire_errors = parse_tool_response(&output)
                            .and_then(|c| {
                                c.get("hire_errors").and_then(|e| e.as_array()).cloned()
                            })
                            .unwrap_or_default();
                        if hire_errors.is_empty() {
                            this.compose.status =
                                Some(format!("Swarm '{}' created.", name.trim()).into());
                        } else {
                            let failed: Vec<String> = hire_errors
                                .iter()
                                .filter_map(|e| {
                                    e.get("agent").and_then(|a| a.as_str()).map(str::to_string)
                                })
                                .collect();
                            this.compose.status = Some(format!(
                                "Swarm '{}' created, but {} hire(s) failed: {}",
                                name.trim(),
                                failed.len(),
                                failed.join(", ")
                            ).into());
                        }
                        // Extract the workspace_id so we can select it and
                        // navigate to Steer.
                        let workspace_id = parse_tool_response(&output)
                            .and_then(|c| c.get("workspace_id").and_then(|v| v.as_str()).map(str::to_string));
                        this.fetch_all(cx);
                        // Navigate to Steer with the new swarm selected.
                        if let Some(id) = workspace_id {
                            this.selected_workspace = Some(id.clone());
                            this.steer.invalidate();
                            this.set_mode(PanelMode::Steer, window, cx);
                            // Queue the composition prompt for injection
                            // (same as the local path).
                            this.pending_composition_prompt =
                                Some(PendingCompositionPrompt {
                                    swarm_id: id,
                                    mission: mission.trim().to_string(),
                                    agents: agents.clone(),
                                    is_local: false,
                                });
                        }
                    }
                    Err(err) => {
                        this.compose.status = Some(format!("Create failed: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
