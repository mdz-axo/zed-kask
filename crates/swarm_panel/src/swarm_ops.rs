//! Swarm CRUD operations: roster drill-down, run status, agent add/remove/fire,
//! local swarm delete. Extracted from `swarm_panel.rs` — the operations stay
//! methods on `SwarmPanel` (they mutate panel state and re-dispatch into
//! `fetch_all` / `open_swarm_detail`); this module owns the tool invocations.
//! See `detail.rs` / `author.rs` for the same extraction pattern.

use gpui::Context;
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;

use crate::RunStatusView;
use crate::SWARM_SERVER;
use crate::SwarmDetailView;
use crate::SwarmPanel;
use crate::SwarmRosterAgent;
use crate::parse::{AgentSource, parse_run_status_messages, parse_swarm_roster};

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
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let is_local = source == AgentSource::Local;
        self.swarm_detail = Some(SwarmDetailView {
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
        });
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = if is_local {
                    invoker
                        .invoke_tool(
                            SWARM_SERVER,
                            "swarm_get_local_swarm",
                            json!({ "swarm_id": workspace_id }),
                        )
                        .await
                } else {
                    invoker
                        .invoke_tool(
                            SWARM_SERVER,
                            "swarm_get_swarm",
                            json!({ "workspace_id": workspace_id }),
                        )
                        .await
                };
                this.update(cx, |this, cx| {
                    let Some(detail) = this.swarm_detail.as_mut() else {
                        return;
                    };
                    detail.loading = false;
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            let agents = if is_local {
                                parsed.and_then(|c| {
                                    c.get("members").and_then(|m| m.as_array()).map(|members| {
                                        members
                                            .iter()
                                            .filter_map(|m| m.as_str().map(str::to_string))
                                            .map(|agent_id| SwarmRosterAgent {
                                                agent_id,
                                                agent_type: String::new(),
                                                description: String::new(),
                                            })
                                            .collect()
                                    })
                                })
                            } else {
                                parsed.and_then(parse_swarm_roster)
                            };
                            match agents {
                                Some(agents) => detail.agents = agents,
                                None => {
                                    detail.error =
                                        Some(format!("Failed to parse roster: {output}").into());
                                }
                            }
                        }
                        Err(err) => {
                            detail.error = Some(format!("Failed to fetch roster: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Back out of the roster drill-down.
    pub(crate) fn close_swarm_detail(&mut self, cx: &mut Context<Self>) {
        self.swarm_detail = None;
        cx.notify();
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
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.run_status = Some(RunStatusView {
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
                    let Some(status) = this.run_status.as_mut() else {
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
        self.run_status = None;
        cx.notify();
    }

    /// Remove a local-only agent card (item 5 local counterpart of firing).
    /// Calls `swarm_remove_local`, which deletes the card directory. A synced
    /// card's ABW agent is untouched. On success, re-fetches so the list and
    /// source badges update.
    pub(crate) fn remove_local_agent(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("remove-{agent_name}"));
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
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.hire_error =
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
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("fire-{agent_id}"));
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
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            // Re-open the detail so the fired agent disappears
                            // from the roster.
                            if let Some(detail) = this.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    cx,
                                );
                            }
                        }
                        Err(err) => {
                            this.hire_error = Some(format!("Failed to fire agent: {err}").into());
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
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("add-{agent_name}"));
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
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            if let Some(detail) = this.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    cx,
                                );
                            }
                        }
                        Err(err) => {
                            this.hire_error =
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
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("roster-remove-{agent_name}"));
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
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            if let Some(detail) = this.swarm_detail.clone() {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    detail.agent_count,
                                    detail.budget,
                                    detail.remaining,
                                    cx,
                                );
                            }
                        }
                        Err(err) => {
                            this.hire_error =
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
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("delete-swarm-{swarm_id}"));
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
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            this.close_swarm_detail(cx);
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.hire_error = Some(format!("Failed to delete swarm: {err}").into());
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
