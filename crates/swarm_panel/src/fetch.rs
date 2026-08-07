//! Data fetching from ABW/local APIs. Extracted from `swarm_panel.rs` —
//! the fetchers stay methods on `SwarmPanel` (they mutate panel state via
//! `cx.spawn` + `this.update`); this module owns the ABW/local tool
//! invocation and response parsing. See `detail.rs` / `author.rs` for the
//! same extraction pattern.

use gpui::Context;
use hkask_types::tool_response::parse_tool_response;
use serde_json::json;

use crate::SWARM_SERVER;
use crate::SwarmEntry;
use crate::SwarmPanel;
use crate::parse::{
    AgentCard, AgentListResponse, AgentSource, LocalAgentListResponse, LocalSwarmListResponse,
    SwarmCard, WorkspaceListResponse, extract_wallet_balance,
};

impl SwarmPanel {
    /// Fetch agents and swarms via the governed MCP tool path.
    pub(crate) fn fetch_all(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.agents_error = Some(
                "Tool invoker not wired — the swarm MCP server is unavailable. \
                 Ensure kask MCP servers are enabled (kask.mcp.load_default)."
                    .into(),
            );
            cx.notify();
            return;
        };

        self.in_flight = 4;
        self.agents_error = None;
        self.swarms_error = None;
        cx.notify();

        // Agents (keyless-capable).
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_agents", json!({ "limit": 200 }))
                    .await;
                this.update(cx, |this, cx| {
                    this.in_flight = this.in_flight.saturating_sub(1);
                    if let Ok(balance) = &result
                        && let Some(b) = extract_wallet_balance(balance)
                    {
                        this.wallet_balance = Some(b);
                    }
                    match result {
                        Ok(output) => {
                            // The invoker wraps tool output in {"content": {...}}.
                            // Unwrap the envelope first, then deserialize the
                            // inner content into the typed response. The prior
                            // direct `from_str::<AgentListResponse>(&output)` always
                            // failed because the top-level key is `content`, not
                            // `agents` — the panel silently showed a parse error
                            // instead of the agent list.
                            let parsed = parse_tool_response(&output)
                                .and_then(|c| serde_json::from_value::<AgentListResponse>(c).ok());
                            match parsed {
                                Some(response) => {
                                    let agents = response
                                        .agents
                                        .into_iter()
                                        .map(|a| {
                                            SwarmEntry::Agent(AgentCard {
                                                id: a.agent_id.unwrap_or_default(),
                                                agent_type: a.agent_type.unwrap_or_default(),
                                                description: a.description.unwrap_or_default(),
                                                author: a.author.unwrap_or_default(),
                                                executions: a
                                                    .execution_stats
                                                    .and_then(|s| s.total_executions)
                                                    .unwrap_or(0),
                                                updated_at: a.updated_at,
                                                source: AgentSource::Cloud,
                                            })
                                        })
                                        .collect::<Vec<_>>();
                                    // Replace cloud agent entries, keep swarm + local entries.
                                    this.entries.retain(|e| matches!(e, SwarmEntry::Swarm(_)));
                                    this.entries.extend(agents);
                                    this.agents_error = None;
                                    this.filter_entries(Self::current_swarm_mode(cx), cx);
                                }
                                None => {
                                    this.agents_error =
                                        Some(format!("Failed to parse agents: {output}").into());
                                    this.filter_entries(Self::current_swarm_mode(cx), cx);
                                }
                            }
                        }
                        Err(err) => {
                            this.agents_error =
                                Some(format!("Failed to list agents: {err}").into());
                            this.filter_entries(Self::current_swarm_mode(cx), cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();

        // Swarms (requires the ABW API key).
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_get_swarm", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    this.in_flight = this.in_flight.saturating_sub(1);
                    match result {
                        Ok(output) => {
                            if let Some(b) = extract_wallet_balance(&output) {
                                this.wallet_balance = Some(b);
                            }
                            match parse_tool_response(&output).and_then(|c| {
                                serde_json::from_value::<WorkspaceListResponse>(c).ok()
                            }) {
                                Some(response) => {
                                    let mut swarms = response
                                        .workspaces
                                        .into_iter()
                                        .map(|w| {
                                            SwarmEntry::Swarm(SwarmCard {
                                                id: w.id.unwrap_or_default(),
                                                name: w.name.unwrap_or_default(),
                                                description: w.description.unwrap_or_default(),
                                                agent_count: w.agent_count,
                                                budget: w.workspace_budget,
                                                remaining: w.workspace_remaining,
                                                source: AgentSource::Cloud,
                                            })
                                        })
                                        .collect::<Vec<_>>();
                                    // Replace cloud swarm entries, keep agent entries
                                    // and any local swarm entries (fetched by the
                                    // `swarm_list_local_swarms` spawn below). The
                                    // prior `retain` only kept agents, which would
                                    // silently drop local swarms on every cloud
                                    // refresh.
                                    this.entries.retain(|e| match e {
                                        SwarmEntry::Agent(_) => true,
                                        SwarmEntry::Swarm(s) => s.source != AgentSource::Cloud,
                                    });
                                    swarms.append(&mut this.entries);
                                    this.entries = swarms;
                                    // Default the hire target to the first swarm if unset,
                                    // or re-validate it if the selected swarm disappeared.
                                    let selected_still_present =
                                        this.selected_workspace.as_ref().is_some_and(|sel| {
                                            this.entries.iter().any(|e| match e {
                                                SwarmEntry::Swarm(s) => &s.id == sel,
                                                _ => false,
                                            })
                                        });
                                    if !selected_still_present {
                                        this.selected_workspace =
                                            this.entries.iter().find_map(|e| match e {
                                                SwarmEntry::Swarm(s) if !s.id.is_empty() => {
                                                    Some(s.id.clone())
                                                }
                                                _ => None,
                                            });
                                    }
                                    this.swarms_error = None;
                                    this.filter_entries(Self::current_swarm_mode(cx), cx);
                                }
                                None => {
                                    this.swarms_error = Some(
                                        format!("Failed to parse workspaces: {output}").into(),
                                    );
                                    this.filter_entries(Self::current_swarm_mode(cx), cx);
                                }
                            }
                        }
                        Err(err) => {
                            // Auth failures here are expected when no key is configured —
                            // degrade to agents-only rather than an error state.
                            log::warn!(
                                "swarm-panel: could not fetch workspaces (agents-only mode): {err}"
                            );
                            this.filter_entries(Self::current_swarm_mode(cx), cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();

        // Local agents (from agents/local/curated/ via swarm_list_local_agents).
        // This fetch always succeeds (it reads the local filesystem, not ABW) —
        // the only failure mode is the MCP server not being running, which is
        // the same as the other fetches. Local agents are merged with cloud
        // agents: if a local agent's `cloud_id` matches a cloud agent's id,
        // the cloud agent is upgraded to `Synced` and the local agent is
        // dropped (the cloud card is the display row; the local card is the
        // execution target for local mode).
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_local_agents", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    this.in_flight = this.in_flight.saturating_sub(1);
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output).and_then(|c| {
                                serde_json::from_value::<LocalAgentListResponse>(c).ok()
                            });
                            if let Some(response) = parsed {
                                let local_agents = response.agents;
                                // Mark cloud agents that have a matching local card as Synced.
                                let local_ids: std::collections::HashSet<String> =
                                    local_agents.iter().map(|a| a.agent_id.clone()).collect();
                                let local_cloud_ids: std::collections::HashSet<String> =
                                    local_agents
                                        .iter()
                                        .filter_map(|a| a.cloud_id.clone())
                                        .collect();
                                for entry in this.entries.iter_mut() {
                                    if let SwarmEntry::Agent(card) = entry
                                        && (local_ids.contains(&card.id)
                                            || local_cloud_ids.contains(&card.id))
                                    {
                                        card.source = AgentSource::Synced;
                                    }
                                }
                                // Add local-only agents (no matching cloud id) as Local entries.
                                let existing_cloud_ids: std::collections::HashSet<String> = this
                                    .entries
                                    .iter()
                                    .filter_map(|e| match e {
                                        SwarmEntry::Agent(c) if c.source != AgentSource::Local => {
                                            Some(c.id.clone())
                                        }
                                        _ => None,
                                    })
                                    .collect();
                                for local in local_agents {
                                    // Skip if already present as a cloud/synced agent.
                                    if existing_cloud_ids.contains(&local.agent_id)
                                        || local_cloud_ids.contains(&local.agent_id)
                                    {
                                        continue;
                                    }
                                    this.entries.push(SwarmEntry::Agent(AgentCard {
                                        id: local.agent_id,
                                        agent_type: local.agent_type,
                                        description: local.description,
                                        author: String::new(),
                                        executions: 0,
                                        updated_at: None,
                                        source: AgentSource::Local,
                                    }));
                                }
                                this.filter_entries(Self::current_swarm_mode(cx), cx);
                            }
                        }
                        Err(err) => {
                            // Local agents fetch failure is not fatal — the
                            // panel still shows cloud agents. Log and continue.
                            log::debug!(
                                "swarm-panel: local agents fetch failed (non-fatal): {err}"
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();
                // Read the local ledger balance (v2 §15), in the async scope
                // (the update closure above is sync). Independent of the list
                // fetch; a failure leaves the balance unknown (None), never a
                // fabricated zero.
                let balance_result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_balance_local", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    match balance_result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            if let Some(content) = parsed {
                                this.local_balance =
                                    content.get("balance").and_then(|b| b.as_i64());
                            }
                        }
                        Err(err) => {
                            log::debug!(
                                "swarm-panel: local balance fetch failed (non-fatal): {err}"
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();

        // Local swarms (from `agents/local/swarms/` via
        // `swarm_list_local_swarms`). This fetch always succeeds (it reads the
        // local filesystem, not ABW). Local swarms are tagged `Local` so the
        // backend-mode toggle can filter the browse list to local swarms only
        // — previously local swarms were never fetched, so the Local toggle
        // showed an empty swarm list even when local swarms existed on disk.
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_local_swarms", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    this.in_flight = this.in_flight.saturating_sub(1);
                    match result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output).and_then(|c| {
                                serde_json::from_value::<LocalSwarmListResponse>(c).ok()
                            });
                            if let Some(response) = parsed {
                                let local_swarms = response
                                    .swarms
                                    .into_iter()
                                    .map(|s| {
                                        SwarmEntry::Swarm(SwarmCard {
                                            id: s.swarm_id.unwrap_or_default(),
                                            name: s.name.unwrap_or_default(),
                                            description: s.mission,
                                            agent_count: Some(s.members.len() as u64),
                                            budget: None,
                                            remaining: None,
                                            source: AgentSource::Local,
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                // Replace local swarm entries, keep agent entries
                                // and any cloud swarm entries.
                                this.entries.retain(|e| match e {
                                    SwarmEntry::Agent(_) => true,
                                    SwarmEntry::Swarm(s) => s.source != AgentSource::Local,
                                });
                                this.entries.extend(local_swarms);
                                this.filter_entries(Self::current_swarm_mode(cx), cx);
                            }
                        }
                        Err(err) => {
                            // Local swarms fetch failure is not fatal — the
                            // panel still shows cloud swarms. Log and continue.
                            log::debug!(
                                "swarm-panel: local swarms fetch failed (non-fatal): {err}"
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

    /// Clone an ABW (cloud) agent to the local registry. Calls
    /// `swarm_clone_to_local` on the swarm MCP server, which fetches the ABW
    /// card, writes it to `agents/local/curated/<id>/agent_card.json`, and
    /// sets `cloud_id` to mark it as synced. On success, re-fetches the agent
    /// list so the source badge updates to `synced`.
    pub(crate) fn clone_to_local(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("clone-{agent_name}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_clone_to_local",
                        json!({ "agent_name": agent_name }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.spend_in_flight = None;
                    match result {
                        Ok(_) => {
                            // Re-fetch to update the source badge.
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.hire_error =
                                Some(format!("Failed to clone to local: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Push a local agent to ABW (cloud). Calls `swarm_push_to_cloud` on the
    /// swarm MCP server, which creates or updates the ABW agent from the local
    /// card and sets `cloud_id` on the local card to mark it as synced. On
    /// success, re-fetches the agent list so the source badge updates to
    /// `synced`.
    pub(crate) fn push_to_cloud(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend_in_flight = Some(format!("push-{agent_name}"));
        cx.notify();
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_push_to_cloud",
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
                                Some(format!("Failed to push to cloud: {err}").into());
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
