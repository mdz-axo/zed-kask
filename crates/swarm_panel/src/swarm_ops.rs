//! Swarm CRUD operations: run status, local agent removal, swarm delete,
//! and local-swarm metadata save. Extracted from `swarm_panel.rs` — the
//! operations stay methods on `SwarmPanel` (they mutate panel state and
//! re-dispatch into `fetch_all`); this module owns the tool invocations.
//! See `author.rs` for the same extraction pattern.

use gpui::Context;
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;

use crate::DestructiveAction;
use crate::RunStatusView;
use crate::SWARM_SERVER;
use crate::SwarmPanel;
use crate::parse::{AgentSource, extract_wallet_balance, parse_run_status_messages};

impl SwarmPanel {
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

    // ── Confirmation flow for destructive actions ───────────────────────────

    /// Stage a delete-swarm action for confirmation. The browse list renders
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
        }
    }

    // ── Cloud swarm delete ──────────────────────────────────────────────────

    /// Permanently delete an ABW workspace (swarm). Calls `swarm_delete_swarm`
    /// (`DELETE /api/teams/{id}`). Irreversible — the workspace and its roster
    /// are removed. On success, re-fetches the swarm list so the deleted
    /// swarm disappears.
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

    /// Save the edited name and mission via `swarm_update_local_swarm`. On
    /// success, re-opens the detail so the header reflects the new metadata.
    pub(crate) fn save_swarm_metadata(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        // The swarm id comes from the compose form's editing_swarm_id
        // (the compose surface is the only metadata editor).
        let Some(editing_id) = self.compose.editing_swarm_id.as_ref() else {
            return;
        };
        let (swarm_id, name, mission) = (
            editing_id.clone(),
            self.compose.name.read(cx).text(cx).trim().to_string(),
            self.compose.mission.read(cx).text(cx),
        );
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
                            this.compose.editing_swarm_id = None;
                            this.compose.editing_swarm_source = None;
                            this.compose.status = Some("Swarm updated.".into());
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
}
