//! Swarm CRUD operations: local agent removal, local swarm delete, and
//! local-swarm metadata save. Extracted from `swarm_panel.rs` — the
//! operations stay methods on `SwarmPanel` (they mutate panel state and
//! re-dispatch into `fetch_all`); this module owns the tool invocations.
//! See `author.rs` for the same extraction pattern.

use gpui::Context;
use serde_json::json;

use crate::DestructiveAction;
use crate::SWARM_SERVER;
use crate::SwarmPanel;

impl SwarmPanel {
    /// Remove a local agent card. Calls `swarm_remove_local`, which deletes
    /// the card directory. On success, re-fetches so the list updates.
    pub(crate) fn remove_local_agent(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.op_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
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
                    match result {
                        Ok(_) => {
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.op_error =
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

    /// Permanently delete a local swarm. Calls `swarm_delete_local_swarm` —
    /// the roster is dropped; member agents are NOT deleted. On success,
    /// re-fetches the swarm list so the deleted swarm disappears.
    pub(crate) fn delete_local_swarm(&mut self, swarm_id: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.op_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
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
                    match result {
                        Ok(_) => {
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.op_error = Some(format!("Failed to delete swarm: {err}").into());
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
    pub(crate) fn request_delete_swarm(
        &mut self,
        swarm_id: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.pending_destructive = Some(DestructiveAction::DeleteSwarm { swarm_id, name });
        cx.notify();
    }

    /// Cancel a pending destructive action.
    pub(crate) fn cancel_destructive(&mut self, cx: &mut Context<Self>) {
        self.pending_destructive = None;
        cx.notify();
    }

    /// Execute the pending destructive action. Dispatches to the local
    /// swarm delete and clears the pending state. The operation surfaces
    /// failures to `op_error`.
    pub(crate) fn confirm_destructive(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.pending_destructive.take() else {
            return;
        };
        match action {
            DestructiveAction::DeleteSwarm { swarm_id, .. } => {
                self.delete_local_swarm(swarm_id, cx);
            }
        }
    }

    /// Save the edited name and mission via `swarm_update_local_swarm`. On
    /// success, re-opens the detail so the header reflects the new metadata.
    pub(crate) fn save_swarm_metadata(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.op_error = Some("Tool invoker not wired.".into());
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
            self.op_error = Some("Swarm name must be non-empty.".into());
            cx.notify();
            return;
        }
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
                    match result {
                        Ok(_) => {
                            this.compose.editing_swarm_id = None;
                            this.compose.status = Some("Swarm updated.".into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.op_error =
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
