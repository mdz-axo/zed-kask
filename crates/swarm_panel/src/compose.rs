//! The swarm-composition surface: form state, editor construction, the
//! `render_compose` renderer, the compose-form entry points
//! (`load_swarm_into_compose`, `reset_compose_form_for_create`), and the
//! `create_swarm` submit flow. Extracted from `swarm_panel.rs` — the
//! renderer and the flows stay methods on `SwarmPanel` (they mutate panel
//! state via `cx.spawn` + `this.update`); this module owns the form struct,
//! the view construction, and the flows.

use editor::Editor;
use gpui::{Context, Entity, SharedString, Window};
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;
use ui::{Tooltip, prelude::*};

use crate::MIN_AGENTS_TO_LAUNCH;
use crate::PanelMode;
use crate::SWARM_SERVER;
use crate::SwarmPanel;
use crate::status_is_warning;

/// State for the swarm-composition surface.
pub(crate) struct ComposeForm {
    pub(crate) name: Entity<Editor>,
    pub(crate) mission: Entity<Editor>,
    /// Local agent ids to seed the roster with, comma-separated.
    pub(crate) agents: Entity<Editor>,
    pub(crate) status: Option<SharedString>,
    pub(crate) busy: bool,
    /// When `Some`, the form is editing an existing swarm (loaded via the
    /// Edit button on the browse card). The name field is read-only for
    /// local swarms (renaming is a separate `swarm_update_local_swarm`
    /// operation). When `None`, the form is creating a new swarm.
    pub(crate) editing_swarm_id: Option<String>,
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
                e.set_placeholder_text("Agents to add, comma-separated (optional)", window, cx);
                e
            }),
            status: None,
            busy: false,
            editing_swarm_id: None,
        }
    }
}

impl SwarmPanel {
    /// The swarm-composition surface: name, mission, agents, create. Each
    /// field carries a tooltip; the create button creates the swarm on the
    /// local substrate (no cost, no consent — local delegation runs on the
    /// operator's own substrate).
    pub(crate) fn render_compose(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        let is_editing = self.compose.editing_swarm_id.is_some();
        let create_label: SharedString = if self.compose.busy {
            "Creating…"
        } else {
            "Create Local Swarm"
        }
        .into();
        let create_tooltip: &str =
            "Creates the swarm on the local substrate (agents/local/swarms). No cost, no consent.";
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
                                "Swarm name. Required. A path-safe id is \
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
                        Label::new("Agents to add (comma-separated)")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .id("compose-agents")
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .tooltip(Tooltip::text(
                                "Local agent ids to seed the roster with, \
                                 comma-separated. Optional. The agents need not exist \
                                 in the registry yet (the roster is ids; resolution \
                                 happens at delegation time).",
                            ))
                            .child(self.compose.agents.clone()),
                    ),
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
    /// operator can view and edit the composition. Fetches the swarm's
    /// roster, pre-fills the compose form (name, mission, agents), and
    /// switches to Compose mode.
    ///
    /// Sets `editing_swarm_id` on the form so the submit path knows it's an
    /// edit, not a create. The name field is kept editable for local swarms
    /// (the save calls `swarm_update_local_swarm`).
    pub(crate) fn load_swarm_into_compose(
        &mut self,
        swarm_id: String,
        name: String,
        mission: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Set editing state on the compose form.
        self.compose.editing_swarm_id = Some(swarm_id.clone());
        self.compose.status = Some("Loading swarm details…".to_string().into());
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

        self.set_mode(crate::PanelMode::Compose, window, cx);

        // Fetch the swarm's roster to populate the agents field.
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.compose.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        crate::SWARM_SERVER,
                        "swarm_get_local_swarm",
                        json!({ "swarm_id": swarm_id }),
                    )
                    .await;
                this.update_in(cx, |this, window, cx| {
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            // Extract member agent ids from the response.
                            let members: Vec<String> = parsed
                                .and_then(|c| {
                                    c.get("members").and_then(|m| m.as_array()).map(|members| {
                                        members
                                            .iter()
                                            .filter_map(|m| m.as_str().map(str::to_string))
                                            .collect::<Vec<_>>()
                                    })
                                })
                                .unwrap_or_default();
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

    /// Reset the compose form to a fresh create state (clear
    /// `editing_swarm_id`, make the name field editable again). Called
    /// when the operator clicks the Compose mode toggle in the header.
    pub(crate) fn reset_compose_form_for_create(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compose.editing_swarm_id = None;
        self.compose.status = None;
        self.compose.name.update(cx, |e, cx| e.clear(window, cx));
        self.compose.mission.update(cx, |e, cx| e.clear(window, cx));
        self.compose.agents.update(cx, |e, cx| e.clear(window, cx));
    }

    /// Create a new swarm from the compose form on the local substrate via
    /// `swarm_create_local_swarm` (no cost, no consent — members are agent
    /// ids).
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
            // Create the swarm on the local substrate — no cost, no consent
            // tokens. Members are agent ids (the local swarm roster is ids;
            // resolution happens at delegation time).
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
                        let swarm_id = parse_tool_response(&output).and_then(|c| {
                            c.get("swarm_id")
                                .and_then(|v| v.as_str())
                                .map(str::to_string)
                        });
                        this.compose.status =
                            Some(format!("Local swarm '{}' created.", name.trim()).into());
                        this.fetch_all(cx);
                        // Navigate to Steer with the new swarm selected.
                        if let Some(id) = swarm_id {
                            this.select_swarm_for_steer(id, cx);
                            // Drop any existing Steer conversation so the
                            // next construction bakes in the new swarm.
                            this.steer.invalidate();
                            this.set_mode(PanelMode::Steer, window, cx);
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
