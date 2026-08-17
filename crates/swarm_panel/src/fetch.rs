//! Data fetching from ABW/local APIs. Extracted from `swarm_panel.rs` —
//! the fetchers stay methods on `SwarmPanel` (they mutate panel state via
//! `cx.spawn` + `this.update`); this module owns the ABW/local tool
//! invocation and response parsing. See `detail.rs` / `author.rs` for the
//! same extraction pattern.

use gpui::Context;
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
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
            // Not an error state yet: the invoker is wired asynchronously by the
            // deferred post-login task, so a panel opened during startup reaches
            // here before the dispatch path exists. Retry rather than presenting a
            // dead end.
            self.agents_error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
            self.schedule_fetch_retry(cx);
            cx.notify();
            return;
        };

        // 2 spawn groups: combined agents+swarms (cloud agents → cloud swarms
        // → local agents → local balance → local swarms) and local swarms.
        //
        // The cloud swarms fetch (`swarm_get_swarm`, which calls
        // `require_auth`) is sequenced AFTER the cloud agents fetch
        // (`swarm_list_agents`, which returns the `authenticated` field) so
        // `cloud_authenticated` is always set before
        // `handle_swarm_fetch_failure` runs. Without this ordering, a parallel
        // `swarm_get_swarm` failure could race ahead of `swarm_list_agents` and
        // hit `handle_swarm_fetch_failure` while `cloud_authenticated` is still
        // `None` — producing a misleading "API key status not yet confirmed"
        // message even when the key IS configured.
        //
        // The local agents fetch is chained inside the same task (not a separate
        // spawn) to prevent a race where the cloud fetch's `retain` wipes
        // Synced/Local entries the local fetch just added (the local fetch
        // reads the filesystem and completes first, then the cloud fetch's
        // retain removes all agent entries, replacing them with fresh
        // Cloud-only ones).
        self.in_flight = 2;
        self.agents_error = None;
        self.swarms_error = None;
        // Reset the server-reported API-key status so a stale `true` from a
        // prior fetch doesn't suppress the "no API key" warning if the key
        // was removed and the server restarted without it. The next
        // `swarm_list_agents` response refreshes it.
        self.cloud_authenticated = None;
        cx.notify();

        // Agents: cloud first, then local (chained). The local agents fetch
        // is sequenced after the cloud fetch inside the same task so that cloud
        // entries are already in `this.entries` when the local merge runs. This
        // prevents a race where the local fetch (filesystem, fast) completes
        // first and adds Synced/Local entries, only for the cloud fetch
        // (network, slow) to wipe all agent entries via `retain` and replace
        // them with fresh Cloud-only ones — making cloned agents invisible.
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                // 1. Cloud agents.
                let cloud_result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_agents", json!({ "limit": 200 }))
                    .await;
                this.update(cx, |this, cx| {
                    if let Ok(balance) = &cloud_result
                        && let Some(b) = extract_wallet_balance(balance)
                    {
                        this.spend.wallet_balance = Some(b);
                    }
                    match cloud_result {
                        Ok(output) => {
                            // The server returns tool errors as an Ok string
                            // carrying the `{"error": ..., "kind": ...}`
                            // envelope (see `McpToolError::to_json_string`),
                            // not as an `Err` from `invoke_tool`. Without
                            // this check, a `permission_denied` (e.g. ABW
                            // returning 401 for an invalid key) would fall
                            // through to the `AgentListResponse` parse, fail
                            // (no `agents` field), and surface as the misleading
                            // "Failed to parse agents: {…}". Route the envelope
                            // through the same classification the `Err(_)` branch
                            // uses below. Mirrors the `swarm_get_swarm` seam.
                            if let Some(err) = parse_tool_error(&output) {
                                if err.is_retryable() {
                                    this.agents_error = Some(
                                        format!("Reconnecting to the swarm server… ({})", err.message).into(),
                                    );
                                    this.schedule_fetch_retry(cx);
                                } else {
                                    // `swarm_list_agents` is keyless (the ABW
                                    // `/agents` catalogue endpoint is open), so a
                                    // non-retryable error here is NOT an API-key
                                    // problem. Do NOT set `agents_error` — that
                                    // would clobber the local agents fetch (which
                                    // runs next in this task) by showing a cloud
                                    // error even when local agents load fine. The
                                    // `swarm_get_swarm` fetch owns the API-key
                                    // warning surface (it is the one tool that
                                    // calls `require_auth`). Log the cloud agents
                                    // failure at warn so it is visible without
                                    // blocking the local list.
                                    log::warn!(
                                        "swarm-panel: cloud agents fetch failed (non-retryable, local agents still load): {} ({:?})",
                                        err.message, err.kind
                                    );
                                }
                                this.filter_entries(cx);
                                cx.notify();
                                return;
                            }
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
                                    // Read the API-key status from the same source
                                    // the MCP server uses (`is_authenticated()` →
                                    // `ctx.credentials.get("HKASK_ABW_API_KEY")`).
                                    // The `authenticated` field is the server's
                                    // own report, so the panel's "no API key"
                                    // warning reflects the server's actual state
                                    // rather than inferring it from the
                                    // `swarm_get_swarm` error message.
                                    this.cloud_authenticated = response.authenticated;
                                    // If the swarms fetch (parallel spawn) already
                                    // failed with `permission_denied` while
                                    // `cloud_authenticated` was still `None`, it
                                    // set a transient "not yet confirmed" placeholder.
                                    // Now that we know the real status, re-evaluate
                                    // the swarms error so the operator sees the
                                    // precise message ("no key" vs "key rejected")
                                    // without waiting for the next fetch cycle.
                                    if let Some(err) = &this.swarms_error
                                        && err.as_ref().contains("not yet confirmed")
                                    {
                                        let key_configured =
                                            this.cloud_authenticated.unwrap_or(false);
                                        this.swarms_error = Some(
                                            if key_configured {
                                                "Cloud swarms unavailable — ABW rejected the API key. \
                                                 Check the key in Settings > Kask > Swarm.".into()
                                            } else {
                                                "Cloud swarms unavailable — no ABW API key configured. \
                                                 Local agents and swarms still work. \
                                                 Set HKASK_ABW_API_KEY or add it in \
                                                 Settings > Kask > Swarm.".into()
                                            },
                                        );
                                    }
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
                                    // Replace cloud agent entries, keep swarm entries.
                                    // Local entries are added by the local fetch below.
                                    this.entries.retain(|e| matches!(e, SwarmEntry::Swarm(_)));
                                    this.entries.extend(agents);
                                    this.agents_error = None;
                                    this.note_fetch_success();
                                    this.filter_entries(cx);
                                }
                                None => {
                                    this.agents_error =
                                        Some(format!("Failed to parse agents: {output}").into());
                                    this.filter_entries(cx);
                                }
                            }
                        }
                        Err(err) => {
                            // A closed MCP transport (server restarting after a
                            // settings change, child process replaced) is transient
                            // — schedule a retry instead of leaving the panel empty
                            // until the operator reopens it.
                            if err.is_retryable() {
                                this.agents_error = Some(
                                    format!("Reconnecting to the swarm server… ({err})").into(),
                                );
                                this.schedule_fetch_retry(cx);
                            } else {
                                // Non-retryable transport error on the keyless
                                // `swarm_list_agents` call. Do NOT set
                                // `agents_error` — the local agents fetch (next
                                // in this task) must still be able to populate
                                // the list without a cloud error clobbering it.
                                // The `swarm_get_swarm` fetch owns the API-key
                                // warning surface.
                                log::warn!(
                                    "swarm-panel: cloud agents fetch failed (non-retryable, local agents still load): {err}"
                                );
                            }
                            this.filter_entries(cx);
                        }
                    }
                    cx.notify();
                })
                .ok();

                // 2. Cloud swarms (`swarm_get_swarm`). Sequenced AFTER the cloud
                // agents fetch so `cloud_authenticated` is set before
                // `handle_swarm_fetch_failure` runs — without this ordering, a
                // parallel `swarm_get_swarm` failure could race ahead of
                // `swarm_list_agents` and hit the failure handler while
                // `cloud_authenticated` is still `None`, producing a misleading
                // "API key status not yet confirmed" message even when the key
                // IS configured. `swarm_get_swarm` is the one tool that calls
                // `require_auth`, so it owns the API-key warning surface.
                let swarm_result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_get_swarm", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    this.in_flight = this.in_flight.saturating_sub(1);
                    match swarm_result {
                        Ok(output) => {
                            if let Some(b) = extract_wallet_balance(&output) {
                                this.spend.wallet_balance = Some(b);
                            }
                            // The server returns tool errors as an Ok string
                            // carrying the `{"error": ..., "kind": ...}`
                            // envelope (see `McpToolError::to_json_string`),
                            // not as an `Err` from `invoke_tool`. Without this
                            // check, a `permission_denied` ("no API key
                            // configured") would fall through to the
                            // `WorkspaceListResponse` parse, fail (no
                            // `workspaces` field), and surface as the misleading
                            // "Failed to parse workspaces: {…}". Route the
                            // envelope through the same classification the
                            // `Err(_)` branch uses below.
                            if let Some(err) = parse_tool_error(&output) {
                                this.handle_swarm_fetch_failure(
                                    err.is_retryable(),
                                    err.kind,
                                    &err.message,
                                    cx,
                                );
                                cx.notify();
                                return;
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
                                    this.note_fetch_success();
                                    this.filter_entries(cx);
                                }
                                None => {
                                    this.swarms_error = Some(
                                        format!("Failed to parse workspaces: {output}").into(),
                                    );
                                    this.filter_entries(cx);
                                }
                            }
                        }
                        Err(err) => {
                            // Two very different causes previously collapsed into one
                            // silent `log::warn!` labelled "agents-only mode":
                            //
                            // - No ABW key configured: genuinely expected, and
                            //   agents-only is the right degradation. Stays quiet.
                            // - The MCP transport closed: NOT expected, and silence
                            //   here is what made a routine server restart look like
                            //   a permanent empty panel. Surfaced and retried.
                            this.handle_swarm_fetch_failure(
                                err.is_retryable(),
                                None,
                                &err.to_string(),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                })
                .ok();

                // 3. Local agents (chained after cloud agents+swarms so cloud
                // entries are in `this.entries` before the merge). This fetch
                // reads the local filesystem, not ABW — it always succeeds if
                // the MCP server is running. Local agents are merged with cloud
                // agents: if a local agent's `cloud_swarm_id` matches a cloud
                // agent's id, the cloud agent is upgraded to `Synced` and the
                // local agent is dropped (the cloud card is the display row;
                // the local card is the execution target for local mode).
                let local_result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_local_agents", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    match local_result {
                        Ok(output) => {
                            // The server returns tool errors as an Ok string
                            // carrying the `{"error": ..., "kind": ...}`
                            // envelope. `swarm_list_local_agents` reads the
                            // local filesystem and does not call `require_auth`,
                            // so a `permission_denied` here would be a server
                            // bug — but it must not be silently swallowed.
                            // Without this check, an error envelope falls through
                            // to the `LocalAgentListResponse` parse, fails (no
                            // `agents` field), and the `if let Some(response)`
                            // block is skipped — local agents silently disappear
                            // with no error surfaced. Mirrors the `swarm_get_swarm`
                            // and `swarm_list_agents` seams.
                            if let Some(err) = parse_tool_error(&output) {
                                log::warn!(
                                    "swarm-panel: local agents fetch returned a server error: {} ({:?})",
                                    err.message, err.kind
                                );
                                // Do NOT set `agents_error` — local agents are a
                                // secondary fetch, and a server-side error here
                                // should not clobber a successful cloud agents
                                // list. The cloud agents fetch owns the
                                // `agents_error` slot; this is logged only.
                                cx.notify();
                                return;
                            }
                            let parsed = parse_tool_response(&output).and_then(|c| {
                                serde_json::from_value::<LocalAgentListResponse>(c).ok()
                            });
                            if let Some(response) = parsed {
                                let local_agents = response.agents;
                                // Mark cloud agents that have a matching local card as Synced.
                                let local_ids: std::collections::HashSet<String> =
                                    local_agents.iter().map(|a| a.agent_id.clone()).collect();
                                let local_cloud_swarm_ids: std::collections::HashSet<String> =
                                    local_agents
                                        .iter()
                                        .filter_map(|a| a.cloud_swarm_id.clone())
                                        .collect();
                                for entry in this.entries.iter_mut() {
                                    if let SwarmEntry::Agent(card) = entry
                                        && (local_ids.contains(&card.id)
                                            || local_cloud_swarm_ids.contains(&card.id))
                                    {
                                        card.source = AgentSource::Synced;
                                    }
                                }
                                // Add local-only agents (no matching cloud id) as Local entries.
                                let existing_cloud_swarm_ids: std::collections::HashSet<String> =
                                    this.entries
                                        .iter()
                                        .filter_map(|e| match e {
                                            SwarmEntry::Agent(c)
                                                if c.source != AgentSource::Local =>
                                            {
                                                Some(c.id.clone())
                                            }
                                            _ => None,
                                        })
                                        .collect();
                                for local in local_agents {
                                    // Skip if already present as a cloud/synced agent.
                                    if existing_cloud_swarm_ids.contains(&local.agent_id)
                                        || local_cloud_swarm_ids.contains(&local.agent_id)
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
                                this.filter_entries(cx);
                            }
                        }
                        Err(err) => {
                            // Local agents fetch failure is not fatal — the panel
                            // still shows cloud agents. A transport loss is worth a
                            // warn (it means the whole server is gone, so the cloud
                            // list is stale too); anything else stays at debug.
                            if err.is_retryable() {
                                log::warn!(
                                    "swarm-panel: local agents fetch lost the MCP transport: {err}"
                                );
                            } else {
                                log::debug!(
                                    "swarm-panel: local agents fetch failed (non-fatal): {err}"
                                );
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();

                // 4. Local ledger balance (independent of the agent lists, but
                // kept in the same task for simplicity). A failure leaves the
                // balance unknown (None), never a fabricated zero.
                let balance_result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_balance_local", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    this.in_flight = this.in_flight.saturating_sub(1);
                    match balance_result {
                        Ok(output) => {
                            let parsed = parse_tool_response(&output);
                            if let Some(content) = parsed {
                                this.spend.local_balance =
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
        // Runs as a separate spawn (parallel with the combined agents+swarms
        // task) because it reads the local filesystem and has no dependency on
        // `cloud_authenticated` or the cloud fetch ordering.
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
                            // The server returns tool errors as an Ok string
                            // carrying the `{"error": ..., "kind": ...}`
                            // envelope. `swarm_list_local_swarms` reads the
                            // local filesystem and does not call `require_auth`,
                            // so a `permission_denied` here would be a server
                            // bug — but it must not be silently swallowed.
                            // Mirrors the `swarm_get_swarm` and
                            // `swarm_list_local_agents` seams.
                            if let Some(err) = parse_tool_error(&output) {
                                log::warn!(
                                    "swarm-panel: local swarms fetch returned a server error: {} ({:?})",
                                    err.message, err.kind
                                );
                                cx.notify();
                                return;
                            }
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
                                this.filter_entries(cx);
                            }
                        }
                        Err(err) => {
                            // Local swarms fetch failure is not fatal — the panel
                            // still shows cloud swarms. A transport loss is worth a
                            // warn; anything else stays at debug.
                            if err.is_retryable() {
                                log::warn!(
                                    "swarm-panel: local swarms fetch lost the MCP transport: {err}"
                                );
                            } else {
                                log::debug!(
                                    "swarm-panel: local swarms fetch failed (non-fatal): {err}"
                                );
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

    /// Clone an ABW (cloud) agent to the local registry. Calls
    /// `swarm_clone_to_local` on the swarm MCP server, which fetches the ABW
    /// card, writes it to `agents/local/curated/<id>/agent_card.json`, and
    /// sets `cloud_swarm_id` to mark it as synced. On success, re-fetches the agent
    /// list so the source badge updates to `synced`.
    pub(crate) fn clone_to_local(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("clone-{agent_name}"));
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
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            // Re-fetch to update the source badge.
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
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
    /// card and sets `cloud_swarm_id` on the local card to mark it as synced. On
    /// success, re-fetches the agent list so the source badge updates to
    /// `synced`.
    pub(crate) fn push_to_cloud_swarm(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.spend.hire_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        self.spend.in_flight = Some(format!("push-{agent_name}"));
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
                    this.spend.in_flight = None;
                    match result {
                        Ok(_) => {
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.spend.hire_error =
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
