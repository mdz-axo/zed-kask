//! Hire/publish consent flows. Extracted from `swarm_panel.rs` — the flows
//! stay methods on `SwarmPanel` (they mutate `pending_hire` /
//! `pending_publish` / `spend_in_flight` and re-dispatch into `fetch_all` /
//! `open_swarm_detail`); this module owns the cost-preflight, consent-token,
//! and spend tool invocations. See `detail.rs` / `author.rs` for the same
//! extraction pattern.

use gpui::Context;
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;

use crate::PendingHire;
use crate::SWARM_SERVER;
use crate::SwarmPanel;
use crate::parse::{extract_wallet_balance, parse_publish_checks};

impl SwarmPanel {
    /// Fetch the pre-flight hire cost for an agent and open the consent gate.
    /// This is the entry point to the cost/consent flow: read-only, spends
    /// nothing, and populates `pending_hire` so the banner renders.
    pub(crate) fn begin_hire(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        // Clear any stale pending consent — a new Hire click replaces it, and
        // a failed cost fetch must not leave a confirmable banner against an
        // unknown cost basis (the M2 finding).
        if self.pending_hire.take().is_some() {
            log::info!("swarm-panel: replaced pending hire consent with a new request");
        }
        self.hire_error = None;
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_hire_cost",
                    json!({ "agent_name": agent_name }),
                )
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.wallet_balance = Some(b);
                        }
                        // Parse the pre-flight estimate out of the content envelope.
                        match parse_tool_response(&output) {
                            Some(content) => {
                                // The server contract: a successful `swarm_hire_cost`
                                // response always carries `total_hire_cost`. A missing
                                // field means the response is malformed or ABW drifted —
                                // the cost is *unknown*, not zero. Fabricating 0 would
                                // show the operator a free hire and then fail at the
                                // consent gate (which rejects `credits_authorized: 0`
                                // for spend actions). Surface the error here instead.
                                // Mirrors the server's own `swarm_hire_cost` guard.
                                let Some(total_hire_cost) =
                                    content.get("total_hire_cost").and_then(|c| c.as_u64())
                                else {
                                    this.hire_error = Some(
                                        "Hire cost unknown — the server response was \
                                         missing total_hire_cost."
                                            .into(),
                                    );
                                    cx.notify();
                                    return;
                                };
                                this.pending_hire = Some(PendingHire {
                                    agent_name: agent_name.clone(),
                                    total_hire_cost,
                                    required_cost: content
                                        .get("required_cost")
                                        .and_then(|c| c.as_u64())
                                        .unwrap_or(0),
                                    optional_cost: content
                                        .get("optional_cost")
                                        .and_then(|c| c.as_u64())
                                        .unwrap_or(0),
                                    within_budget: content
                                        .get("within_budget")
                                        .and_then(|c| c.as_bool())
                                        .unwrap_or(false),
                                    // Fallback mirrors the server default — the
                                    // server always sends this field, so the
                                    // fallback only fires on a malformed response.
                                    // Read from `Default` (single source of truth)
                                    // rather than a magic number.
                                    max_credits: content
                                        .get("max_credits_per_dispatch")
                                        .and_then(|c| c.as_u64())
                                        .unwrap_or_else(|| {
                                            u64::from(
                                                kask_bridge::KaskSwarmSettings::default()
                                                    .max_credits_per_dispatch,
                                            )
                                        }) as u32,
                                });
                            }
                            None => {
                                this.hire_error =
                                    Some(format!("Failed to parse hire cost: {output}").into());
                            }
                        }
                    }
                    Err(err) => {
                        this.hire_error =
                            Some(format!("Failed to estimate hire cost: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Operator authorized the hire. Mint a single-use consent token via
    /// `swarm_request_consent`, then invoke the gated `swarm_hire` spend tool
    /// with it. The token is action-scoped ("hire") and target-scoped (the
    /// agent name), so it cannot be replayed for a different agent or spend.
    pub(crate) fn confirm_hire(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_hire.take() else {
            return;
        };
        let Some(workspace_id) = self.selected_workspace.clone() else {
            self.hire_error =
                Some("No swarm selected to hire into. Create a workspace on ABW first.".into());
            cx.notify();
            return;
        };
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };

        let agent_name = pending.agent_name.clone();
        let credits = pending.total_hire_cost as u32;
        self.spend_in_flight = Some(agent_name.clone());
        cx.notify();

        cx.spawn(async move |this, cx| {
            // Step 1: mint the consent token (records the operator's authorization).
            let consent = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_request_consent",
                    json!({
                        "action": "hire",
                        "target": agent_name,
                        "credits_authorized": credits,
                    }),
                )
                .await;

            let token = match consent {
                Ok(output) => parse_tool_response(&output).and_then(|c| {
                    c.get("consent_token")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                }),
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.spend_in_flight = None;
                        // Restore the banner so the operator can retry from the
                        // estimate they already reviewed (the M4 finding).
                        this.pending_hire = Some(pending.clone());
                        this.hire_error = Some(format!("Consent failed: {err}").into());
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            let Some(token) = token else {
                this.update(cx, |this, cx| {
                    this.spend_in_flight = None;
                    this.pending_hire = Some(pending.clone());
                    this.hire_error = Some("Consent did not return a token.".into());
                    cx.notify();
                })
                .ok();
                return;
            };

            // Step 2: invoke the gated spend tool with the consent token.
            let hire = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_hire",
                    json!({
                        "workspace_id": workspace_id,
                        "agent_name": agent_name,
                        "include_optional": false,
                        "consent_token": token,
                        "credits_authorized": credits,
                    }),
                )
                .await;

            this.update(cx, |this, cx| {
                this.spend_in_flight = None;
                match hire {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.wallet_balance = Some(b);
                        }
                        log::info!("swarm-panel: hired '{agent_name}' into {workspace_id}");
                        // Refresh so the new hire appears in the swarm roster.
                        this.fetch_all(cx);
                        // If the roster drill-down is open for this workspace,
                        // re-open it so the new member appears immediately
                        // (fetch_all refreshes the card list, not the detail).
                        // Guard against re-opening a detail for a *different*
                        // workspace — a browse-card hire (which targets
                        // `selected_workspace`, not necessarily the open detail)
                        // must not refresh an unrelated roster.
                        if let Some(detail) = this.swarm_detail.clone() {
                            if detail.workspace_id == workspace_id {
                                this.open_swarm_detail(
                                    detail.workspace_id.clone(),
                                    detail.name,
                                    detail.source,
                                    detail.mission,
                                    cx,
                                );
                            }
                        }
                    }
                    Err(err) => {
                        this.hire_error = Some(format!("Hire failed: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Operator declined the hire — clear the gate without spending.
    pub(crate) fn cancel_hire(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_hire.take() {
            log::info!(
                "swarm-panel: operator declined hire of '{}' (gate aborted)",
                pending.agent_name
            );
        }
        cx.notify();
    }

    /// Preflight a publish — calls `swarm_publish_checks` (fermi v0.10.15) and
    /// opens the publish banner. Read-only: spends nothing and mutates no ABW
    /// state. When `can_publish` is false the banner shows the failing checks
    /// and a reason input for the admin force-publish path.
    pub(crate) fn begin_publish(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        if self.pending_publish.take().is_some() {
            log::info!("swarm-panel: replaced pending publish with a new request");
        }
        self.hire_error = None;
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_publish_checks",
                    json!({ "agent_name": agent_name }),
                )
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(output) => {
                        let Some(checks) = parse_tool_response(&output) else {
                            this.hire_error = Some(
                                format!("Unexpected publish-checks response: {output}").into(),
                            );
                            cx.notify();
                            return;
                        };
                        match parse_publish_checks(agent_name.clone(), &checks) {
                            Ok(pending) => {
                                this.pending_publish = Some(pending);
                            }
                            Err(msg) => {
                                this.hire_error = Some(msg.into());
                            }
                        }
                    }
                    Err(err) => {
                        this.hire_error =
                            Some(format!("Failed to preflight publish: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Confirm the publish. When `can_publish` is true, publishes directly.
    /// When false, reads the reason editor and force-publishes (admin path,
    /// audited to `admin_bypass_events`); an empty reason is refused client-side
    /// so the audit row is never blank.
    pub(crate) fn confirm_publish(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_publish.clone() else {
            return;
        };
        let agent_name = pending.agent_name.clone();
        let (force, reason) = if pending.can_publish {
            (false, String::new())
        } else {
            let reason = self.publish_reason.read(cx).text(cx);
            if reason.trim().is_empty() {
                self.hire_error = Some(
                    "A reason is required to force-publish past failing checks \
                     (audited to admin_bypass_events)."
                        .into(),
                );
                cx.notify();
                return;
            }
            (true, reason)
        };
        self.do_publish(agent_name, force, reason, cx);
    }

    /// Operator cancelled the publish — clear the banner without publishing.
    pub(crate) fn cancel_publish(&mut self, cx: &mut Context<Self>) {
        if self.pending_publish.take().is_some() {
            log::info!("swarm-panel: operator cancelled publish (gate aborted)");
        }
        cx.notify();
    }

    /// Invoke `swarm_publish_agent` and re-fetch on success. Restores the
    /// banner on error so the operator can retry without re-preflighting.
    pub(crate) fn do_publish(
        &mut self,
        agent_name: String,
        force: bool,
        reason: String,
        cx: &mut Context<Self>,
    ) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let pending = self.pending_publish.clone();
        self.spend_in_flight = Some(format!("publish-{agent_name}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_publish_agent",
                        json!({
                            "agent_name": agent_name,
                            "force": force,
                            "reason": reason,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            this.pending_publish = None;
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            // Restore the banner so the operator can retry from
                            // the checks they already reviewed.
                            this.pending_publish = pending;
                            this.hire_error = Some(format!("Failed to publish: {err}").into());
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
