//! Data fetching from ABW/local APIs. Extracted from `swarm_panel.rs` —
//! this module owns the whole fetch lifecycle: the [`FetchState`] state
//! object (in-flight count, per-source errors, cloud-auth signal, retry /
//! backoff), the fetchers (which stay methods on `SwarmPanel` because they
//! mutate panel state via `cx.spawn` + `this.update`), and the ABW/local
//! tool invocation and response parsing. See `detail.rs` / `author.rs` for
//! the same extraction pattern.

use std::time::Duration;

use gpui::{Context, SharedString, Task};
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde_json::json;

use crate::SWARM_SERVER;
use crate::SwarmEntry;
use crate::SwarmPanel;
use crate::parse::{
    AgentCard, AgentListResponse, AgentSource, LocalAgentInfo, LocalAgentListResponse,
    LocalSwarmListResponse, SwarmCard, WorkspaceListResponse, extract_wallet_balance,
};

/// First automatic-retry delay after a retryable fetch failure. Doubles per
/// attempt (1s, 2s, 4s, 8s, 16s).
const FETCH_RETRY_BASE_DELAY: Duration = Duration::from_secs(1);

/// Maximum consecutive automatic retries before the panel settles on a visible
/// error and waits for a manual refresh. Bounded so a permanently broken server
/// does not become an unbounded background poll.
const MAX_FETCH_RETRIES: u32 = 5;

/// The backoff delay for the next retry, or `None` once the attempt budget is
/// spent.
///
/// Kept free of panel state and the GPUI executor so the retry *policy* is
/// unit-testable without constructing a `Workspace` (the same reason
/// `hkask-kanban-widget` splits its dispatch decision out of the handler).
fn fetch_retry_delay(attempts_so_far: u32) -> Option<Duration> {
    if attempts_so_far >= MAX_FETCH_RETRIES {
        return None;
    }
    Some(FETCH_RETRY_BASE_DELAY * 2u32.pow(attempts_so_far))
}

/// The fetch lifecycle state: in-flight count, per-source errors, the
/// server-reported cloud-auth signal, and the automatic-retry/backoff state.
///
/// Grouped so the fetch concern is one cohesive state object whose
/// transitions live next to the fetchers that drive them. The fields are
/// private to this module — the fetchers and the retry orchestration below
/// mutate them directly (same-module privacy); the rest of the crate reads
/// through the accessors.
#[derive(Default)]
pub(crate) struct FetchState {
    /// Number of fetch operations currently in flight (agents + swarms spawn
    /// independently). `is_fetching()` is true while any are in the air —
    /// avoids one fetch's completion hiding the other's spinner.
    in_flight: usize,
    /// Per-source fetch errors. Split so a slow agents fetch can't clobber a
    /// swarms error (and vice versa) — the H1 cross-clobber finding.
    agents_error: Option<SharedString>,
    swarms_error: Option<SharedString>,
    /// Whether the swarm MCP server reports the ABW API key as configured
    /// (`authenticated` field from the `swarm_list_agents` response). Read
    /// from the same source the server uses (`ctx.credentials.get(
    /// "HKASK_ABW_API_KEY")`), so the panel's "no API key" warning is
    /// accurate rather than inferred from the `swarm_get_swarm` error
    /// message (which conflates "no key" with "key rejected by ABW").
    /// `None` until the first `swarm_list_agents` response arrives.
    cloud_authenticated: Option<bool>,
    /// Pending automatic retry after a *retryable* fetch failure (MCP transport
    /// closed, invoker not yet wired). Held so it is cancelled on drop and so a
    /// manual refresh can supersede it.
    ///
    /// Without this the panel fetched exactly once, in the constructor: a single
    /// MCP server restart — which happens routinely when settings change or the
    /// inference socket resolves after launch — left the panel permanently empty
    /// with only a `log::warn!` the operator never sees.
    retry_task: Option<Task<()>>,
    /// Consecutive retryable-failure count, for backoff. Reset on any success or
    /// manual refresh.
    retry_attempt: u32,
}

impl FetchState {
    /// True while any fetch (agents or swarms) is in the air.
    pub(crate) fn is_fetching(&self) -> bool {
        self.in_flight > 0
    }

    /// The single visible error, preferring agents then swarms. Rendered as a
    /// status strip whenever present (not only in the empty state).
    pub(crate) fn visible_error(&self) -> Option<&SharedString> {
        self.agents_error.as_ref().or(self.swarms_error.as_ref())
    }

    /// The server-reported ABW API-key status, for the empty-state key hint.
    pub(crate) fn cloud_authenticated(&self) -> Option<bool> {
        self.cloud_authenticated
    }

    /// Clear the retry backoff after a successful fetch, so a later transient
    /// failure starts from the short delay again.
    fn note_fetch_success(&mut self) {
        self.retry_attempt = 0;
    }

    /// Cancel any pending automatic retry and reset the backoff — a manual
    /// refresh supersedes the automatic schedule.
    fn cancel_retry(&mut self) {
        self.retry_task = None;
        self.retry_attempt = 0;
    }

    /// Whether an automatic retry is already pending — a sibling fetch's
    /// failure rides on it rather than stacking a second timer.
    fn retry_pending(&self) -> bool {
        self.retry_task.is_some()
    }

    /// The backoff delay for the next retry, consuming one attempt from the
    /// budget. `None` once the budget is spent — the caller settles on a
    /// visible error instead of polling forever.
    fn next_retry_delay(&mut self) -> Option<Duration> {
        let delay = fetch_retry_delay(self.retry_attempt)?;
        self.retry_attempt += 1;
        Some(delay)
    }

    /// Begin a fetch cycle: count the spawned fetch groups and reset the
    /// per-cycle state. The cloud-auth signal is reset to `None` so a stale
    /// `true` from a prior fetch doesn't suppress the "no API key" warning
    /// if the key was removed and the server restarted without it — the
    /// next `swarm_list_agents` response refreshes it.
    fn begin_cycle(&mut self, spawn_groups: usize) {
        self.in_flight = spawn_groups;
        self.agents_error = None;
        self.swarms_error = None;
        self.cloud_authenticated = None;
    }

    /// One spawn group completed.
    fn fetch_completed(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }
}

impl SwarmPanel {
    /// Fetch agents and swarms via the governed MCP tool path.
    pub(crate) fn fetch_all(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            // Not an error state yet: the invoker is wired asynchronously by the
            // deferred post-login task, so a panel opened during startup reaches
            // here before the dispatch path exists. Retry rather than presenting a
            // dead end.
            self.fetch.agents_error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
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
        self.fetch.begin_cycle(3);
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
                                    this.fetch.agents_error = Some(
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
                                    this.fetch.cloud_authenticated = response.authenticated;
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
                                                display_name: a.display_alias.unwrap_or_default(),
                                                updated_at: a.updated_at,
                                                source: AgentSource::Cloud,
                                            })
                                        })
                                        .collect::<Vec<_>>();
                                    // Replace cloud agent entries, keep swarm and
                                    // app entries. Local entries are added by
                                    // the local fetch below.
                                    this.entries.retain(|e| {
                                        matches!(e, SwarmEntry::Swarm(_) | SwarmEntry::App(_))
                                    });
                                    this.entries.extend(agents);
                                    this.fetch.agents_error = None;
                                    this.fetch.note_fetch_success();
                                    this.filter_entries(cx);
                                }
                                None => {
                                    this.fetch.agents_error =
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
                                this.fetch.agents_error = Some(
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
                    // Note: `in_flight` is NOT decremented here — the combined
                    // task decrements it once at the end (step 4). The cloud
                    // swarms fetch is sequenced inside the combined task
                    // specifically to ensure `cloud_authenticated` is set
                    // before `handle_swarm_fetch_failure` runs.
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
                                                cloud_workspace_id: None,
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
                                        SwarmEntry::App(_) => true,
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
                                    this.fetch.swarms_error = None;
                                    this.fetch.note_fetch_success();
                                    this.filter_entries(cx);
                                }
                                None => {
                                    this.fetch.swarms_error = Some(
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
                                merge_local_agents(&mut this.entries, response.agents);
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

                // 4. End of the combined task — decrement `in_flight` here.
                // The local ledger balance is NOT fetched: the local ledger is
                // accounting-only (it records accumulated local spend and may
                // legitimately be negative — see `steer_system_prompt`), not a
                // spendable wallet. Surfacing it in the header would present
                // it as a budget the operator must track, which is not how
                // local swarms work. `swarm_balance_local` remains available
                // to the curator in Steer for reconciliation.
                this.update(cx, |this, cx| {
                    this.fetch.fetch_completed();
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
                    this.fetch.fetch_completed();
                    match result {
                        Ok(output) => {
                            // The server returns tool errors as an Ok string
                            // carrying the `{"error": ..., "kind": ...}` envelope.
                            // `swarm_list_local_swarms` reads the
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
                                        let is_synced = s.cloud_workspace_id.is_some();
                                        SwarmEntry::Swarm(SwarmCard {
                                            id: s.swarm_id.unwrap_or_default(),
                                            name: s.name.unwrap_or_default(),
                                            description: s.mission,
                                            agent_count: Some(s.members.len() as u64),
                                            budget: None,
                                            remaining: None,
                                            source: if is_synced {
                                                AgentSource::Synced
                                            } else {
                                                AgentSource::Local
                                            },
                                            cloud_workspace_id: s.cloud_workspace_id,
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                // Replace local swarm entries, keep agent entries
                                // and any cloud swarm entries.
                                this.entries.retain(|e| match e {
                                    SwarmEntry::Agent(_) => true,
                                    SwarmEntry::Swarm(s) => s.source != AgentSource::Local,
                                    SwarmEntry::App(_) => true,
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

        // Apps (from `swarm_list_apps`). Apps are cloud-only (ABW
        // catalogue). Runs as a separate spawn (parallel with the combined
        // agents+swarms task and the local swarms fetch) because it has no
        // dependency on either. A failed fetch is non-fatal — the panel
        // still shows agents and swarms.
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_apps", json!({ "limit": 200 }))
                    .await;
                this.update(cx, |this, cx| {
                    this.fetch.fetch_completed();
                    match result {
                        Ok(output) => {
                            // Route tool-error envelopes (e.g.
                            // `permission_denied` when no API key is
                            // configured) away from the parse path — same
                            // seam as the agents/swarms fetches.
                            if let Some(err) = parse_tool_error(&output) {
                                log::warn!(
                                    "swarm-panel: apps fetch returned a server error: {} ({:?})",
                                    err.message,
                                    err.kind
                                );
                                cx.notify();
                                return;
                            }
                            let apps = parse_tool_response(&output)
                                .map(crate::parse::parse_app_list)
                                .unwrap_or_default();
                            let app_entries = apps
                                .into_iter()
                                .map(|a| {
                                    SwarmEntry::App(crate::AppCard {
                                        slug: a.slug,
                                        name: a.name,
                                        tagline: a.tagline,
                                        description: a.description,
                                        visibility: a.visibility,
                                        archived: a.archived,
                                    })
                                })
                                .collect::<Vec<_>>();
                            // Replace existing App entries, keep agents and
                            // swarms.
                            this.entries.retain(|e| !matches!(e, SwarmEntry::App(_)));
                            this.entries.extend(app_entries);
                            this.filter_entries(cx);
                        }
                        Err(err) => {
                            if err.is_retryable() {
                                log::warn!("swarm-panel: apps fetch lost the MCP transport: {err}");
                            } else {
                                log::debug!("swarm-panel: apps fetch failed (non-fatal): {err}");
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

    /// Re-fetch now, cancelling any pending automatic retry and resetting the
    /// backoff. Bound to the refresh affordance.
    pub(crate) fn refresh_now(&mut self, cx: &mut Context<Self>) {
        self.fetch.cancel_retry();
        self.fetch_all(cx);
    }

    /// Schedule a re-fetch after a retryable failure, with exponential backoff.
    ///
    /// Called by the fetchers when a failure is transport-level rather than
    /// semantic. Backoff is capped at [`MAX_FETCH_RETRIES`] attempts so a
    /// genuinely broken server produces a bounded number of retries and then a
    /// stable visible error, rather than an unbounded poll.
    fn schedule_fetch_retry(&mut self, cx: &mut Context<Self>) {
        if self.fetch.retry_pending() {
            // A retry is already pending; the sibling fetch's failure rides on it
            // rather than stacking a second timer.
            return;
        }
        let Some(delay) = self.fetch.next_retry_delay() else {
            log::warn!(
                "swarm-panel: giving up after {MAX_FETCH_RETRIES} retries — \
                 use the Retry button once the MCP server is available"
            );
            return;
        };
        log::info!(
            "swarm-panel: fetch failed transiently — retrying in {}s (attempt {}/{})",
            delay.as_secs(),
            self.fetch.retry_attempt,
            MAX_FETCH_RETRIES
        );
        self.fetch.retry_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                this.fetch.retry_task = None;
                this.fetch_all(cx);
            })
            .ok();
        }));
    }

    /// Classify a swarm-list fetch failure and update `fetch.swarms_error`.
    ///
    /// Shared by the `Err(_)` branch of `invoke_tool` (transport-level failure)
    /// and the `Ok(output)` branch when the server returned a tool error envelope
    /// `{"error": ..., "kind": ...}` (e.g. `permission_denied` for "no API key
    /// configured"). Before this helper existed, the envelope case fell through to
    /// `WorkspaceListResponse` parsing and surfaced as the misleading
    /// "Failed to parse workspaces: {…}".
    ///
    /// - Retryable (`Unavailable`/`Timeout`/`RateLimited`, or a retryable
    ///   transport error): show the reconnect banner and schedule a retry.
    /// - `PermissionDenied` (no API key, etc.): a quiet agents-only degradation is
    ///   the right behavior, but the operator previously had no signal that the
    ///   swarm list was empty *because* of auth rather than because they have no
    ///   swarms. Surface a short, non-alarming status so the cause is visible
    ///   without a retry loop (retrying with no key is pointless).
    /// - Other non-retryable: log at warn and stay quiet (agents-only mode).
    ///
    /// API-key status is read from `fetch.cloud_authenticated` (the `authenticated`
    /// field the server returns in the `swarm_list_agents` response), NOT from
    /// the error message. The error message conflates "no key configured"
    /// (from `require_auth()`) with "key configured but rejected by ABW"
    /// (from the ABW 401 body) — both surface as `permission_denied`, and a
    /// 401 body can contain "no API key" text, which would misclassify a
    /// rejected key as "not configured." The `authenticated` field is the
    /// server's own report of whether `ctx.credentials` has the key, so it is
    /// the same source the MCP server uses.
    fn handle_swarm_fetch_failure(
        &mut self,
        retryable: bool,
        kind: Option<hkask_types::McpErrorKind>,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        if retryable {
            self.fetch.swarms_error =
                Some(format!("Reconnecting to the swarm server… ({message})").into());
            self.schedule_fetch_retry(cx);
        } else if matches!(kind, Some(hkask_types::McpErrorKind::PermissionDenied)) {
            // Auth failure: either no ABW key is configured, or the key is
            // configured but rejected by ABW (401/403). Retry is pointless in
            // both cases (a missing key won't appear without a settings
            // change; an invalid key won't become valid). Surface the cause as
            // a quiet status so an empty swarm list is not mistaken for "you
            // have no swarms".
            //
            // Distinguish the two causes using `fetch.cloud_authenticated` (the
            // server's own report of whether the key is configured) rather
            // than the error message. The message-based check
            // (`message.contains("no API key")`) was a false positive when ABW
            // returned a 401 body containing "no API key" text for a key that
            // WAS configured but rejected — the panel showed "no ABW API key
            // configured" even though the key was present.
            //
            // `fetch.cloud_authenticated` is set by the `swarm_list_agents` fetch,
            // which is sequenced BEFORE the `swarm_get_swarm` fetch in the same
            // task (see `fetch_all`). So by the time this runs,
            // `cloud_authenticated` is `Some(_)` in the normal case. `None` is
            // a defensive fallback (e.g. the agents fetch failed before
            // reaching the parse step) — treat it as "key not confirmed" rather
            // than guessing.
            let status: SharedString = match self.fetch.cloud_authenticated {
                Some(true) => format!(
                    "Cloud swarms unavailable — ABW rejected the API key: {message}. \
                     Check the key in Settings > Kask > Swarm."
                )
                .into(),
                Some(false) => "Cloud swarms unavailable — no ABW API key configured. \
                 Local agents and swarms still work. Set HKASK_ABW_API_KEY or add it \
                 in Settings > Kask > Swarm."
                    .into(),
                None => "Cloud swarms unavailable — API key status not yet confirmed. \
                 Local agents and swarms still work. Retry to refresh."
                    .into(),
            };
            self.fetch.swarms_error = Some(status);
            log::warn!("swarm-panel: swarm list unavailable (agents-only mode): {message}");
        } else {
            log::warn!("swarm-panel: could not fetch workspaces (agents-only mode): {message}");
        }
        self.filter_entries(cx);
    }

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
                                Some(format!("Failed to copy to local: {err}").into());
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

/// Merge local-agent cards into the panel entry list.
///
/// Cloud rows whose id matches a local card's `agent_id` or `cloud_swarm_id`
/// are upgraded to `Synced`. Local-only cards (no matching cloud row) are
/// appended as `Local` entries. A local clone whose `cloud_swarm_id` points
/// at a displayed cloud row is suppressed — the cloud row (now `Synced`) is
/// the display row. Clones carry a `-clone`-suffixed `agent_id` that never
/// equals the cloud id, so the dedup must key on `cloud_swarm_id` too.
pub(crate) fn merge_local_agents(entries: &mut Vec<SwarmEntry>, local_agents: Vec<LocalAgentInfo>) {
    let local_ids: std::collections::HashSet<String> =
        local_agents.iter().map(|a| a.agent_id.clone()).collect();
    let local_cloud_swarm_ids: std::collections::HashSet<String> = local_agents
        .iter()
        .filter_map(|a| a.cloud_swarm_id.clone())
        .collect();
    for entry in entries.iter_mut() {
        if let SwarmEntry::Agent(card) = entry
            && (local_ids.contains(&card.id) || local_cloud_swarm_ids.contains(&card.id))
        {
            card.source = AgentSource::Synced;
        }
    }
    let existing_cloud_swarm_ids: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|e| match e {
            SwarmEntry::Agent(c) if c.source != AgentSource::Local => Some(c.id.clone()),
            _ => None,
        })
        .collect();
    for local in local_agents {
        // Suppress the Local row when the cloud counterpart is already
        // displayed. Clones carry a `-clone`-suffixed agent_id that never
        // matches the cloud id, so dedup on cloud_swarm_id too. When the
        // cloud fetch fails (no cloud row present), the clone is not
        // suppressed and appears as a standalone Local row.
        if existing_cloud_swarm_ids.contains(&local.agent_id)
            || local
                .cloud_swarm_id
                .as_ref()
                .is_some_and(|cid| existing_cloud_swarm_ids.contains(cid))
        {
            continue;
        }
        entries.push(SwarmEntry::Agent(AgentCard {
            id: local.agent_id,
            agent_type: local.agent_type,
            description: local.description,
            display_name: local.display_name,
            author: String::new(),
            executions: 0,
            updated_at: None,
            source: AgentSource::Local,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The panel used to fetch exactly once, in its constructor. A single MCP
    // server restart — routine when settings change, or when the inference
    // socket resolves after launch — left it permanently empty with only a
    // `log::warn!` the operator never saw. These pin the recovery policy.

    /// The backoff doubles and is bounded, so a permanently broken server
    /// produces a finite number of retries rather than an unbounded poll.
    #[test]
    fn fetch_retry_backs_off_then_gives_up() {
        assert_eq!(fetch_retry_delay(0), Some(Duration::from_secs(1)));
        assert_eq!(fetch_retry_delay(1), Some(Duration::from_secs(2)));
        assert_eq!(fetch_retry_delay(2), Some(Duration::from_secs(4)));
        assert_eq!(fetch_retry_delay(3), Some(Duration::from_secs(8)));
        assert_eq!(fetch_retry_delay(4), Some(Duration::from_secs(16)));
        assert_eq!(
            fetch_retry_delay(MAX_FETCH_RETRIES),
            None,
            "the attempt budget must be finite so a broken server settles on a \
             visible error instead of polling forever"
        );
    }

    /// The retry budget is spent monotonically — no attempt count within the
    /// budget may yield `None`, and none beyond it may yield `Some`.
    #[test]
    fn fetch_retry_budget_is_monotonic() {
        for attempt in 0..MAX_FETCH_RETRIES {
            assert!(
                fetch_retry_delay(attempt).is_some(),
                "attempt {attempt} is within the budget of {MAX_FETCH_RETRIES}"
            );
        }
        for attempt in MAX_FETCH_RETRIES..(MAX_FETCH_RETRIES + 3) {
            assert!(
                fetch_retry_delay(attempt).is_none(),
                "attempt {attempt} exceeds the budget of {MAX_FETCH_RETRIES}"
            );
        }
    }
}
