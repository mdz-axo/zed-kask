//! Swarm CRUD operations: roster drill-down, run status, agent add/remove/fire,
//! local swarm delete. Extracted from `swarm_panel.rs` — the operations stay
//! methods on `SwarmPanel` (they mutate panel state and re-dispatch into
//! `fetch_all` / `open_swarm_detail`); this module owns the tool invocations.
//! See `detail.rs` / `author.rs` for the same extraction pattern.

use gpui::{Context, Window};
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;

use crate::DestructiveAction;
use crate::PendingActionsView;
use crate::RunStatusView;
use crate::SWARM_SERVER;
use crate::SwarmDetailView;
use crate::SwarmPanel;
use crate::SwarmRosterAgent;
use crate::parse::{
    AgentSource, LocalAgentInfo, LocalAgentListResponse, extract_wallet_balance,
    parse_run_status_messages, parse_swarm_roster,
};

impl SwarmPanel {
    /// Open the roster drill-down for a swarm (item 4). The fetch is
    /// mode-aware: `Local` swarms are read via `swarm_get_local_swarm`
    /// (members are agent ids; agent_type/description are not carried by the
    /// local swarm record, so the roster rows show the id only), while `Cloud`
    /// swarms are read via `swarm_get_swarm` (ABW's server-sanitized roster
    /// payload, parsed defensively across plausible envelope shapes).
    pub(crate) fn open_swarm_detail(
        &mut self,
        workspace_id: String,
        name: String,
        source: AgentSource,
        mission: String,
        agent_count: Option<u64>,
        budget: Option<u64>,
        remaining: Option<u64>,
        cloud_workspace_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let is_local = source == AgentSource::Local;
        self.detail.swarm_detail = Some(SwarmDetailView {
            workspace_id: workspace_id.clone(),
            name,
            mission,
            source,
            agent_count,
            budget,
            remaining,
            loading: true,
            error: None,
            agents: Vec::new(),
            editing_metadata: false,
            cloud_workspace_id,
        });
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                if is_local {
                    // Fetch the swarm roster and the local agent catalogue
                    // in parallel so member ids can be enriched with
                    // agent_type, description, accepts, and produces from
                    // the card data. A failed agent-list fetch must NOT
                    // blank the roster — the swarm fetch is the authority;
                    // enrichment is best-effort.
                    let swarm_result = invoker
                        .invoke_tool(
                            SWARM_SERVER,
                            "swarm_get_local_swarm",
                            json!({ "swarm_id": workspace_id }),
                        )
                        .await;
                    let agents_result = invoker
                        .invoke_tool(SWARM_SERVER, "swarm_list_local_agents", json!({}))
                        .await;
                    this.update(cx, |this, cx| {
                        let Some(detail) = this.detail.swarm_detail.as_mut() else {
                            return;
                        };
                        detail.loading = false;
                        match swarm_result {
                            Ok(output) => {
                                let parsed = parse_tool_response(&output);
                                let member_ids = parsed.and_then(|c| {
                                    c.get("members").and_then(|m| m.as_array()).map(|members| {
                                        members
                                            .iter()
                                            .filter_map(|m| m.as_str().map(str::to_string))
                                            .collect::<Vec<_>>()
                                    })
                                });
                                match member_ids {
                                    Some(ids) => {
                                        // Build a lookup from the local
                                        // agent catalogue response. A failed
                                        // or unparseable catalogue yields an
                                        // empty vec — members are still
                                        // shown with empty metadata.
                                        let cards: Vec<LocalAgentInfo> = agents_result
                                            .as_ref()
                                            .ok()
                                            .and_then(|out| parse_tool_response(out))
                                            .and_then(|c| {
                                                serde_json::from_value::<LocalAgentListResponse>(c)
                                                    .ok()
                                            })
                                            .map(|r| r.agents)
                                            .unwrap_or_default();
                                        detail.agents = ids
                                            .iter()
                                            .map(|id| {
                                                if let Some(card) =
                                                    cards.iter().find(|c| c.agent_id == *id)
                                                {
                                                    SwarmRosterAgent {
                                                        agent_id: id.clone(),
                                                        agent_type: card.agent_type.clone(),
                                                        description: card.description.clone(),
                                                        accepts: card.accepts.clone(),
                                                        produces: card.produces.clone(),
                                                    }
                                                } else {
                                                    // Member id has no card
                                                    // (deleted or not yet
                                                    // created) — preserve the
                                                    // id with empty metadata.
                                                    // The roster is ids;
                                                    // resolution happens at
                                                    // delegation time.
                                                    SwarmRosterAgent {
                                                        agent_id: id.clone(),
                                                        agent_type: String::new(),
                                                        description: String::new(),
                                                        accepts: Vec::new(),
                                                        produces: Vec::new(),
                                                    }
                                                }
                                            })
                                            .collect();
                                    }
                                    None => {
                                        detail.error = Some(
                                            format!("Failed to parse roster: {output}").into(),
                                        );
                                    }
                                }
                            }
                            Err(err) => {
                                detail.error =
                                    Some(format!("Failed to fetch roster: {err}").into());
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                } else {
                    let result = invoker
                        .invoke_tool(
                            SWARM_SERVER,
                            "swarm_get_swarm",
                            json!({ "workspace_id": workspace_id }),
                        )
                        .await;
                    // Fetch pending actions in parallel for cloud swarms.
                    // The action protocol is ABW-only; local swarms have no
                    // pending-actions surface. A failed fetch must NOT blank
                    // the roster — the roster is the authority; pending
                    // actions are a best-effort enrichment.
                    let pending_result = invoker
                        .invoke_tool(
                            SWARM_SERVER,
                            "swarm_workspace_pending_actions",
                            json!({ "workspace_id": workspace_id }),
                        )
                        .await;
                    this.update(cx, |this, cx| {
                        let Some(detail) = this.detail.swarm_detail.as_mut() else {
                            return;
                        };
                        detail.loading = false;
                        match result {
                            Ok(output) => {
                                let parsed = parse_tool_response(&output);
                                match parsed.and_then(parse_swarm_roster) {
                                    Some(agents) => detail.agents = agents,
                                    None => {
                                        detail.error = Some(
                                            format!("Failed to parse roster: {output}").into(),
                                        );
                                    }
                                }
                            }
                            Err(err) => {
                                detail.error =
                                    Some(format!("Failed to fetch roster: {err}").into());
                            }
                        }
                        // Parse pending actions defensively — a failed or
                        // unparseable response yields an empty list, not an
                        // error (the roster is the authority).
                        let pending_actions = match &pending_result {
                            Ok(output) => parse_tool_response(output)
                                .map(crate::parse::parse_pending_actions)
                                .unwrap_or_default(),
                            Err(_) => Vec::new(),
                        };
                        this.detail.pending_actions = Some(PendingActionsView {
                            workspace_id: workspace_id.clone(),
                            loading: false,
                            error: None,
                            actions: pending_actions,
                        });
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    /// Back out of the roster drill-down.
    pub(crate) fn close_swarm_detail(&mut self, cx: &mut Context<Self>) {
        self.detail.swarm_detail = None;
        self.detail.pending_actions = None;
        cx.notify();
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

    /// Fetch and show a swarm's recent run status (item 3):
    /// `swarm_run_status(workspace_id)`. Rendered as a dismissible strip.
    pub(crate) fn show_run_status(
        &mut self,
        workspace_id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.detail.run_status = Some(RunStatusView {
            name,
            loading: true,
            error: None,
            messages: Vec::new(),
        });
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_run_status",
                        json!({ "workspace_id": workspace_id, "limit": 20 }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    let Some(status) = this.detail.run_status.as_mut() else {
                        return;
                    };
                    status.loading = false;
                    match result {
                        Ok(output) => {
                            match parse_tool_response(&output).and_then(parse_run_status_messages) {
                                Some(messages) => status.messages = messages,
                                None => {
                                    status.error = Some(
                                        format!("Failed to parse run status: {output}").into(),
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            status.error =
                                Some(format!("Failed to fetch run status: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Dismiss the run-status strip.
    pub(crate) fn dismiss_run_status(&mut self, cx: &mut Context<Self>) {
        self.detail.run_status = None;
        cx.notify();
    }

    /// Remove a local-only agent card (item 5 local counterpart of firing).
    /// Calls `swarm_remove_local`, which deletes the card directory. A synced
    /// card's ABW agent is untouched. On success, re-fetches so the list and
    /// source badges update.
    pub(crate) fn remove_local_agent(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("remove-{agent_name}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_remove_local",
                        json!({ "agent_name": agent_name }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to remove local agent: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Fire an agent from the ABW workspace shown in the roster drill-down
    /// (item 4 management surface). Calls `swarm_fire` (verified live
    /// 2026-08-02: `DELETE /workspaces/{id}/agents/{agent}` — removes the
    /// agent from the roster; no credit cost; the agent itself is not
    /// deleted). On success, re-opens the detail so the fired row disappears.
    pub(crate) fn fire_agent(
        &mut self,
        workspace_id: String,
        agent_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("fire-{agent_id}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_fire",
                        json!({ "workspace_id": workspace_id, "agent_name": agent_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            // Re-open the detail so the fired agent disappears
                            // from the roster.
                            if let Some(detail) = this.detail.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    detail.cloud_workspace_id,
                                    cx,
                                );
                            }
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to fire agent: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Add a local agent to the open local swarm's roster (item 4 local
    /// management). Calls `swarm_add_agent_local` — idempotent, no cost, no
    /// consent. On success, re-opens the detail so the new member appears.
    pub(crate) fn add_agent_to_swarm(
        &mut self,
        swarm_id: String,
        agent_name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("add-{agent_name}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_add_agent_local",
                        json!({ "swarm_id": swarm_id, "agent_name": agent_name }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            if let Some(detail) = this.detail.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    detail.cloud_workspace_id,
                                    cx,
                                );
                            }
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to add agent to swarm: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Remove a local agent from the open local swarm's roster (item 4 local
    /// management). Calls `swarm_remove_agent_local` — idempotent, does not
    /// delete the agent card. On success, re-opens the detail.
    pub(crate) fn remove_agent_from_swarm(
        &mut self,
        swarm_id: String,
        agent_name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("roster-remove-{agent_name}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_remove_agent_local",
                        json!({ "swarm_id": swarm_id, "agent_name": agent_name }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            if let Some(detail) = this.detail.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    detail.cloud_workspace_id,
                                    cx,
                                );
                            }
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to remove agent from swarm: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Permanently delete a local swarm (item 4 local management). Calls
    /// `swarm_delete_local_swarm` — the roster is dropped; member agents are
    /// NOT deleted. On success, closes the detail and re-fetches the swarm
    /// list so the deleted swarm disappears.
    pub(crate) fn delete_local_swarm(&mut self, swarm_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("delete-swarm-{swarm_id}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_delete_local_swarm",
                        json!({ "swarm_id": swarm_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.close_swarm_detail(cx);
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to delete swarm: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Permanently delete the agent currently loaded in the author form.
    /// Branches on the editing source:
    /// - `Local` / `Synced`: calls `swarm_remove_local` — deletes the local
    ///   card directory. A synced card's ABW agent is NOT touched (the cloud
    ///   copy can be deleted separately from the cloud card's "..." menu).
    /// - `Cloud`: calls `swarm_delete_agent` — irreversible ABW delete. The
    ///   agent is removed from the operator's library and every workspace
    ///   roster. A synced local card is NOT touched.
    /// On success, resets the author form to create mode and re-fetches the
    /// browse list so the deleted agent disappears.
    pub(crate) fn delete_edited_agent(&mut self, cx: &mut Context<Self>) {
        let Some(agent_name) = self.author.editing_id.clone() else {
            self.author.status = Some("No agent is loaded for deletion.".into());
            cx.notify();
            return;
        };
        let source = self
            .author
            .editing_source
            .clone()
            .unwrap_or(AgentSource::Local);
        let is_local = matches!(source, AgentSource::Local | AgentSource::Synced);
        let tool_name = if is_local {
            "swarm_remove_local"
        } else {
            "swarm_delete_agent"
        };
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.author.busy = true;
        self.author.status = Some("Deleting agent…".into());
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, tool_name, json!({ "agent_name": agent_name }))
                    .await;
                this.update(cx, |this, cx| {
                    this.author.busy = false;
                    match result {
                        Ok(_) => {
                            // Defer the form reset and mode switch to the next
                            // `render` frame — `Editor::clear` and `set_mode`
                            // need `&mut Window`, which the spawn closure cannot
                            // hold. `render` consumes `pending_author_reset`.
                            this.pending_author_reset = true;
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.author.status =
                                Some(format!("Failed to delete agent: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── Confirmation flow for destructive actions ───────────────────────────

    /// Stage a delete-swarm action for confirmation. The detail view renders
    /// a confirmation banner; the operator must click "Confirm" to execute.
    /// For ABW swarms, the confirmation auto-fetches run status so active
    /// runs are visible before the irreversible delete.
    pub(crate) fn request_delete_swarm(
        &mut self,
        swarm_id: String,
        source: AgentSource,
        name: String,
        cx: &mut Context<Self>,
    ) {
        // Check before moving `source` into the enum — AgentSource is Clone
        // but not Copy.
        let is_cloud = source != AgentSource::Local;
        self.detail.pending_destructive = Some(DestructiveAction::DeleteSwarm {
            swarm_id: swarm_id.clone(),
            source,
            name: name.clone(),
        });
        // For ABW swarms, fetch run status so the confirmation banner can
        // surface active runs. Local swarms have no ABW run-status endpoint.
        if is_cloud {
            self.show_run_status(swarm_id, name, cx);
        }
        cx.notify();
    }

    /// Stage a remove/fire-agent action for confirmation.
    pub(crate) fn request_remove_agent(
        &mut self,
        swarm_id: String,
        agent_id: String,
        source: AgentSource,
        cx: &mut Context<Self>,
    ) {
        self.detail.pending_destructive = Some(DestructiveAction::RemoveAgent {
            swarm_id,
            agent_id,
            source,
        });
        cx.notify();
    }

    /// Cancel a pending destructive action.
    pub(crate) fn cancel_destructive(&mut self, cx: &mut Context<Self>) {
        self.detail.pending_destructive = None;
        cx.notify();
    }

    /// Execute the pending destructive action. Dispatches to the appropriate
    /// backend operation and clears the pending state. The individual
    /// operations set `in_flight` and surface errors to `hire_error`.
    pub(crate) fn confirm_destructive(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.detail.pending_destructive.take() else {
            return;
        };
        match action {
            DestructiveAction::DeleteSwarm {
                swarm_id,
                source,
                name: _,
            } => {
                if source == AgentSource::Local {
                    self.delete_local_swarm(swarm_id, cx);
                } else {
                    self.delete_cloud_swarm(swarm_id, cx);
                }
            }
            DestructiveAction::RemoveAgent {
                swarm_id,
                agent_id,
                source,
            } => {
                if source == AgentSource::Local {
                    self.remove_agent_from_swarm(swarm_id, agent_id, cx);
                } else {
                    self.fire_agent(swarm_id, agent_id, cx);
                }
            }
        }
    }

    // ── Cloud swarm delete ──────────────────────────────────────────────────

    /// Permanently delete an ABW workspace (swarm). Calls `swarm_delete_swarm`
    /// (`DELETE /api/teams/{id}`). Irreversible — the workspace and its roster
    /// are removed. On success, closes the detail and re-fetches the swarm
    /// list so the deleted swarm disappears.
    pub(crate) fn delete_cloud_swarm(&mut self, workspace_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("delete-cloud-swarm-{workspace_id}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_delete_swarm",
                        json!({ "workspace_id": workspace_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(output) => {
                            if let Some(b) = extract_wallet_balance(&output) {
                                this.spend.wallet_balance = Some(b);
                            }
                            this.close_swarm_detail(cx);
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to delete ABW swarm: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── Edit swarm metadata (local only) ────────────────────────────────────

    /// Enter metadata-edit mode: populates the name and mission editors from
    /// the loaded swarm and flips `editing_metadata`. Local swarms only — ABW
    /// has no metadata-edit endpoint.
    pub(crate) fn begin_edit_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.swarm_detail.as_ref() else {
            return;
        };
        let name = detail.name.clone();
        let mission = detail.mission.clone();
        self.detail
            .edit_name_editor
            .update(cx, |e, cx| e.set_text(name, window, cx));
        self.detail
            .edit_mission_editor
            .update(cx, |e, cx| e.set_text(mission, window, cx));
        if let Some(detail) = self.detail.swarm_detail.as_mut() {
            detail.editing_metadata = true;
        }
        cx.notify();
    }

    /// Exit metadata-edit mode without saving.
    pub(crate) fn cancel_edit_metadata(&mut self, cx: &mut Context<Self>) {
        if let Some(detail) = self.detail.swarm_detail.as_mut() {
            detail.editing_metadata = false;
        }
        cx.notify();
    }

    /// Save the edited name and mission via `swarm_update_local_swarm`. On
    /// success, re-opens the detail so the header reflects the new metadata.
    pub(crate) fn save_swarm_metadata(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        // The swarm id comes from either the compose form's editing_swarm_id
        // (when editing via the compose surface) or the detail view's
        // swarm_detail (when editing via the detail surface).
        let (swarm_id, name, mission) =
            if let Some(editing_id) = self.compose.editing_swarm_id.as_ref() {
                (
                    editing_id.clone(),
                    self.compose.name.read(cx).text(cx).trim().to_string(),
                    self.compose.mission.read(cx).text(cx),
                )
            } else if let Some(detail) = self.detail.swarm_detail.as_ref() {
                (
                    detail.workspace_id.clone(),
                    self.detail
                        .edit_name_editor
                        .read(cx)
                        .text(cx)
                        .trim()
                        .to_string(),
                    self.detail.edit_mission_editor.read(cx).text(cx),
                )
            } else {
                return;
            };
        if name.is_empty() {
            self.spend.hire_error = Some("Swarm name must be non-empty.".into());
            cx.notify();
            return;
        }
        self.spend.in_flight = Some(format!("edit-swarm-{swarm_id}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_update_local_swarm",
                        json!({
                            "swarm_id": swarm_id,
                            "name": name,
                            "mission": mission,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            if let Some(detail) = this.detail.swarm_detail.as_mut() {
                                detail.editing_metadata = false;
                            }
                            // Clear the compose editing state if the
                            // save was triggered from the compose form.
                            this.compose.editing_swarm_id = None;
                            this.compose.editing_swarm_source = None;
                            this.compose.status = Some("Swarm updated.".into());
                            // Re-open the detail to reflect the new metadata.
                            if let Some(detail) = this.detail.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    detail.cloud_workspace_id,
                                    cx,
                                );
                            }
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to update swarm metadata: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── Copy swarm ─────────────────────────────────────────────────────────

    /// Copy a local swarm via `swarm_clone_local_swarm`. Creates a new swarm
    /// with a fresh slug id, copying the mission and roster. On success,
    /// re-fetches the swarm list so the copy appears.
    pub(crate) fn clone_local_swarm(&mut self, swarm_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("clone-swarm-{swarm_id}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_clone_local_swarm",
                        json!({ "swarm_id": swarm_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(output) => {
                            let cloned_id = parse_tool_response(&output).and_then(|c| {
                                c.get("swarm_id")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            });
                            this.fetch_all(cx);
                            if let Some(id) = cloned_id {
                                this.spend.hire_error =
                                    Some(format!("Copied swarm created: {id}").into());
                            }
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to copy swarm: {err}").into());
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

    // ── Push local swarm to cloud ────────────────────────────────────────────

    /// Push a local swarm to ABW. Calls `swarm_push_local_swarm` which creates
    /// an ABW workspace from the local swarm's name, mission, and roster.
    /// Consent tokens for member hires are minted first (same flow as
    /// `create_swarm`). On success, re-fetches so the swarm list shows the
    /// updated `cloud_workspace_id` link.
    pub(crate) fn push_local_swarm_to_cloud(&mut self, swarm_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };

        // Fetch the local swarm to get its members for consent minting.
        let invoker_clone = invoker.clone();
        self.spend.in_flight = Some(format!("push-swarm-{swarm_id}"));
        cx.notify();
        cx.spawn({
            async move |this, cx| {
                // Step 1: fetch the local swarm to get its members.
                let swarm_result = invoker_clone
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_get_local_swarm",
                        json!({ "swarm_id": swarm_id }),
                    )
                    .await;

                let members: Vec<String> = match swarm_result {
                    Ok(output) => parse_tool_response(&output)
                        .and_then(|c| {
                            c.get("members").and_then(|m| m.as_array()).map(|members| {
                                members
                                    .iter()
                                    .filter_map(|m| m.as_str().map(str::to_string))
                                    .collect::<Vec<_>>()
                            })
                        })
                        .unwrap_or_default(),
                    Err(err) => {
                        this.update(cx, |this, cx| {
                            this.spend.in_flight = None;
                            this.spend.hire_error =
                                Some(format!("Failed to fetch local swarm: {err}").into());
                            cx.notify();
                        })
                        .ok();
                        return;
                    }
                };

                // Step 2: mint consent tokens for each member agent.
                let mut consent_tokens = Vec::new();
                let mut consent_failures = Vec::new();
                for agent in &members {
                    let cost_result = invoker_clone
                        .invoke_tool(
                            SWARM_SERVER,
                            "swarm_hire_cost",
                            json!({ "agent_name": agent }),
                        )
                        .await;
                    let credits = match cost_result {
                        Ok(output) => parse_tool_response(&output).and_then(|c| {
                            c.get("total_hire_cost").and_then(|v| v.as_u64())
                        }).map(|c| c as u32),
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
                    match invoker_clone
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

                if !consent_failures.is_empty() {
                    this.update(cx, |this, cx| {
                        this.spend.in_flight = None;
                        this.spend.hire_error = Some(
                            format!(
                                "Consent failed for {} — swarm not pushed.",
                                consent_failures.join(", ")
                            )
                            .into(),
                        );
                        cx.notify();
                    })
                    .ok();
                    return;
                }

                // Step 3: push the swarm.
                let result = invoker_clone
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_push_local_swarm",
                        json!({
                            "swarm_id": swarm_id,
                            "consent_tokens": consent_tokens,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(output) => {
                            if let Some(b) = extract_wallet_balance(&output) {
                                this.spend.wallet_balance = Some(b);
                            }
                            let hire_errors = parse_tool_response(&output)
                                .and_then(|c| {
                                    c.get("hire_errors")
                                        .and_then(|e| e.as_array())
                                        .cloned()
                                })
                                .unwrap_or_default();
                            if hire_errors.is_empty() {
                                this.spend.hire_error =
                                    Some("Swarm pushed to ABW.".into());
                            } else {
                                let failed: Vec<String> = hire_errors
                                    .iter()
                                    .filter_map(|e| {
                                        e.get("agent").and_then(|a| a.as_str()).map(str::to_string)
                                    })
                                    .collect();
                                this.spend.hire_error = Some(format!(
                                    "Swarm pushed to ABW, but {} hire(s) failed: {}",
                                    failed.len(),
                                    failed.join(", ")
                                ).into());
                            }
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to push swarm to ABW: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── Pull ABW swarm to local ──────────────────────────────────────────────

    /// Pull an ABW workspace to local. Calls `swarm_pull_swarm_to_local` which
    /// reads the ABW workspace and creates a local swarm copy. No consent
    /// needed (local creates are free). On success, re-fetches so the new local
    /// swarm appears in the list.
    pub(crate) fn pull_cloud_swarm_to_local(
        &mut self,
        workspace_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("pull-swarm-{workspace_id}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_pull_swarm_to_local",
                        json!({ "workspace_id": workspace_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(output) => {
                            let local_id = parse_tool_response(&output).and_then(|c| {
                                c.get("swarm_id")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            });
                            this.fetch_all(cx);
                            this.spend.hire_error = Some(
                                format!(
                                    "Pulled ABW swarm to local{}.",
                                    local_id.map(|id| format!(": {id}")).unwrap_or_default()
                                )
                                .into(),
                            );
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to pull ABW swarm: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── App primitive — authoring (load + create/update) ────────────────────

    /// Load an existing App's manifest into the App form for editing.
    /// Fetches the App via `swarm_get_app`, stores the result in
    /// `pending_app_load`, and switches to `AppAuthor` mode. The form
    /// fields are populated in `render` via `apply_pending_app_load` (which
    /// has `&mut Window` for `Editor::set_text`). The slug field is set
    /// read-only immediately (renaming would change the App's identity).
    pub(crate) fn load_app_into_form(
        &mut self,
        slug: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.app_form.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.app_form.editing_slug = Some(slug.clone());
        self.app_form.status = Some("Loading App…".into());
        self.app_form.busy = false;
        self.app_form.slug.update(cx, |e, _| e.set_read_only(true));
        self.app_form
            .slug
            .update(cx, |e, cx| e.set_text(slug.clone(), window, cx));
        self.pending_app_load = None;
        self.set_mode(crate::PanelMode::AppAuthor, window, cx);
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_get_app", json!({ "slug": slug }))
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            match parsed {
                                Some(app) => {
                                    let get_str = |key: &str| {
                                        app.get(key)
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string()
                                    };
                                    let template_str = app
                                        .get("workspace_template")
                                        .map(|v| {
                                            serde_json::to_string_pretty(v).unwrap_or_default()
                                        })
                                        .unwrap_or_default();
                                    let visibility = {
                                        let v = get_str("visibility");
                                        if v.is_empty() {
                                            "private".to_string()
                                        } else {
                                            v
                                        }
                                    };
                                    this.pending_app_load = Some(crate::AppDetailLoad {
                                        name: get_str("name"),
                                        tagline: get_str("tagline"),
                                        description: get_str("description"),
                                        homepage_url: get_str("homepage_url"),
                                        icon_url: get_str("icon_url"),
                                        workspace_template: template_str,
                                        visibility,
                                    });
                                    this.app_form.status = None;
                                }
                                None => {
                                    this.app_form.status = Some(
                                        format!("Failed to parse App response: {output}").into(),
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            this.app_form.status =
                                Some(format!("Failed to load App: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Apply a pending App detail load to the form. Called from `render`
    /// because `Editor::set_text` requires `&mut Window`, which the spawn
    /// closure does not have. Mirrors `apply_pending_author_load`.
    pub(crate) fn apply_pending_app_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(load) = self.pending_app_load.take() else {
            return;
        };
        self.app_form
            .name
            .update(cx, |e, cx| e.set_text(load.name, window, cx));
        self.app_form
            .tagline
            .update(cx, |e, cx| e.set_text(load.tagline, window, cx));
        self.app_form
            .description
            .update(cx, |e, cx| e.set_text(load.description, window, cx));
        self.app_form
            .homepage_url
            .update(cx, |e, cx| e.set_text(load.homepage_url, window, cx));
        self.app_form
            .icon_url
            .update(cx, |e, cx| e.set_text(load.icon_url, window, cx));
        self.app_form
            .workspace_template
            .update(cx, |e, cx| e.set_text(load.workspace_template, window, cx));
        self.app_form.visibility = load.visibility;
    }

    /// Create a new App or update an existing one. Branches on
    /// `editing_slug`: when `Some`, calls `swarm_update_app`; when `None`,
    /// calls `swarm_create_app_direct`. Validates the slug and workspace
    /// template JSON before dispatching.
    pub(crate) fn create_or_update_app(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.app_form.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let slug = self.app_form.slug.read(cx).text(cx);
        let name = self.app_form.name.read(cx).text(cx);
        let tagline = self.app_form.tagline.read(cx).text(cx);
        let description = self.app_form.description.read(cx).text(cx);
        let homepage_url = self.app_form.homepage_url.read(cx).text(cx);
        let icon_url = self.app_form.icon_url.read(cx).text(cx);
        let template_raw = self.app_form.workspace_template.read(cx).text(cx);
        let visibility = self.app_form.visibility.clone();
        let editing_slug = self.app_form.editing_slug.clone();

        if slug.trim().is_empty() {
            self.app_form.status = Some("Slug is required.".into());
            cx.notify();
            return;
        }
        // Validate slug for new Apps (editing keeps the original slug).
        if editing_slug.is_none() {
            let len = slug.trim().chars().count();
            let valid = (3..=64).contains(&len)
                && slug
                    .trim()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                self.app_form.status = Some(
                    "Slug must be 3-64 chars: lowercase letters, digits, underscores only.".into(),
                );
                cx.notify();
                return;
            }
        }
        // Parse the workspace template JSON if non-empty.
        let workspace_template: Option<serde_json::Value> = if template_raw.trim().is_empty() {
            None
        } else {
            match serde_json::from_str::<serde_json::Value>(&template_raw) {
                Ok(v) if v.is_object() => Some(v),
                Ok(_) => {
                    self.app_form.status = Some("Workspace template must be a JSON object.".into());
                    cx.notify();
                    return;
                }
                Err(e) => {
                    self.app_form.status =
                        Some(format!("Invalid JSON in workspace template: {e}").into());
                    cx.notify();
                    return;
                }
            }
        };

        self.app_form.busy = true;
        self.app_form.status = Some(
            if editing_slug.is_some() {
                "Updating…"
            } else {
                "Creating…"
            }
            .into(),
        );
        cx.notify();

        let is_editing = editing_slug.is_some();
        let tool_name = if is_editing {
            "swarm_update_app"
        } else {
            "swarm_create_app_direct"
        };
        // Build the request payload. For update, the slug is the editing
        // slug (immutable). For create, it's the form's slug field.
        let mut payload = serde_json::json!({});
        let obj = payload.as_object_mut().expect("just constructed object");
        if is_editing {
            obj.insert(
                "slug".into(),
                serde_json::json!(editing_slug.clone().unwrap_or_default()),
            );
        } else {
            obj.insert("slug".into(), serde_json::json!(slug));
        }
        if !name.trim().is_empty() {
            obj.insert("name".into(), serde_json::json!(name));
        }
        if !tagline.trim().is_empty() {
            obj.insert("tagline".into(), serde_json::json!(tagline));
        }
        if !description.trim().is_empty() {
            obj.insert("description".into(), serde_json::json!(description));
        }
        if !homepage_url.trim().is_empty() {
            obj.insert("homepage_url".into(), serde_json::json!(homepage_url));
        }
        if !icon_url.trim().is_empty() {
            obj.insert("icon_url".into(), serde_json::json!(icon_url));
        }
        if let Some(v) = workspace_template {
            obj.insert("workspace_template".into(), v);
        }
        obj.insert("visibility".into(), serde_json::json!(visibility));

        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker.invoke_tool(SWARM_SERVER, tool_name, payload).await;
                this.update(cx, |this, cx| {
                    this.app_form.busy = false;
                    match result {
                        Ok(_) => {
                            this.app_form.status = Some(
                                if is_editing {
                                    "App updated."
                                } else {
                                    "App created."
                                }
                                .into(),
                            );
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.app_form.status = Some(
                                format!(
                                    "Failed to {} App: {err}",
                                    if is_editing { "update" } else { "create" }
                                )
                                .into(),
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── App primitive — spawn / publish / archive (fermi v0.10.15+) ──────────
    //
    // Card actions for App entries in the browse list. These are the
    // operator-facing App lifecycle actions: spawn a workspace from an App,
    // promote an App to public, and archive an App.

    /// Spawn a new workspace from an App. Calls `swarm_spawn_app_workspace`
    /// and refreshes the swarm list on success so the new workspace appears.
    pub(crate) fn spawn_app_workspace(&mut self, slug: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("spawn-app-{slug}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_spawn_app_workspace",
                        json!({ "slug": slug }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(output) => {
                            let ws_id = parse_tool_response(&output).and_then(|c| {
                                c.get("id")
                                    .or_else(|| c.get("workspace_id"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            });
                            this.spend.hire_error = Some(
                                format!(
                                    "Spawned workspace from App '{slug}'{}.",
                                    ws_id.map(|id| format!(": {id}")).unwrap_or_default()
                                )
                                .into(),
                            );
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error = Some(
                                format!("Failed to spawn workspace from App '{slug}': {err}")
                                    .into(),
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Publish an App (promote visibility to public). Calls
    /// `swarm_publish_app` and refreshes the app list on success.
    pub(crate) fn publish_app(&mut self, slug: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("publish-app-{slug}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_publish_app", json!({ "slug": slug }))
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.spend.hire_error = Some(format!("Published App '{slug}'.").into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to publish App '{slug}': {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Archive an App. Calls `swarm_archive_app` and refreshes the app list
    /// on success. No confirmation modal — the card button is the
    /// confirmation (archived apps are visible with an "archived" badge and
    /// the Spawn button is disabled, so the action is reversible in effect:
    /// the App stays visible, just can't spawn).
    pub(crate) fn archive_app(&mut self, slug: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("archive-app-{slug}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_archive_app", json!({ "slug": slug }))
                    .await;
                this.update(cx, |this, cx| {
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.spend.hire_error = Some(format!("Archived App '{slug}'.").into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
                                Some(format!("Failed to archive App '{slug}': {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    // ── Workspace action protocol (fermi v0.10.15+) ──────────────────────────
    //
    // The action protocol is the human-in-the-loop confirmation surface:
    // agents propose mutations (mutate_document, fork_state), the panel
    // surfaces them as pending actions, and the operator accepts or rejects.
    // These three methods wire the panel's review queue to the MCP tools.

    /// Accept a pending workspace action. Calls `swarm_workspace_accept_action`
    /// and refreshes the pending-actions list on success.
    pub(crate) fn accept_pending_action(
        &mut self,
        workspace_id: String,
        action_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        // Mark loading so the UI shows a spinner on the accept button.
        if let Some(pa) = self.detail.pending_actions.as_mut() {
            pa.loading = true;
            pa.error = None;
        }
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_workspace_accept_action",
                        json!({
                            "workspace_id": workspace_id,
                            "action_id": action_id,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(_) => {
                            // Refresh the pending-actions list. A failed
                            // refresh is non-fatal — the accept succeeded.
                            this.refresh_pending_actions(cx);
                        }
                        Err(err) => {
                            if let Some(pa) = this.detail.pending_actions.as_mut() {
                                pa.loading = false;
                                pa.error = Some(format!("Failed to accept action: {err}").into());
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Reject a pending workspace action. Calls `swarm_workspace_reject_action`
    /// and refreshes the pending-actions list on success.
    pub(crate) fn reject_pending_action(
        &mut self,
        workspace_id: String,
        action_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        if let Some(pa) = self.detail.pending_actions.as_mut() {
            pa.loading = true;
            pa.error = None;
        }
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_workspace_reject_action",
                        json!({
                            "workspace_id": workspace_id,
                            "action_id": action_id,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(_) => {
                            this.refresh_pending_actions(cx);
                        }
                        Err(err) => {
                            if let Some(pa) = this.detail.pending_actions.as_mut() {
                                pa.loading = false;
                                pa.error = Some(format!("Failed to reject action: {err}").into());
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Refresh the pending-actions list for the currently-open cloud swarm
    /// detail. Called after accept/reject and available as a manual refresh.
    /// No-op when no cloud swarm detail is open.
    pub(crate) fn refresh_pending_actions(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            return;
        };
        let Some(detail) = self.detail.swarm_detail.as_ref() else {
            return;
        };
        // Only cloud swarms have the action protocol.
        if detail.source == AgentSource::Local {
            return;
        }
        let workspace_id = detail.workspace_id.clone();
        if let Some(pa) = self.detail.pending_actions.as_mut() {
            pa.loading = true;
            pa.error = None;
        }
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_workspace_pending_actions",
                        json!({ "workspace_id": workspace_id }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    match result {
                        Ok(output) => {
                            let actions = parse_tool_response(&output)
                                .map(crate::parse::parse_pending_actions)
                                .unwrap_or_default();
                            this.detail.pending_actions = Some(PendingActionsView {
                                workspace_id,
                                loading: false,
                                error: None,
                                actions,
                            });
                        }
                        Err(err) => {
                            if let Some(pa) = this.detail.pending_actions.as_mut() {
                                pa.loading = false;
                                pa.error =
                                    Some(format!("Failed to load pending actions: {err}").into());
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}
