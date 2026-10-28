//! Data fetching from the local swarm substrate. Extracted from
//! `swarm_panel.rs` — this module owns the whole fetch lifecycle: the
//! [`FetchState`] state object (in-flight count, per-source errors, retry /
//! backoff), the fetchers (which stay methods on `SwarmPanel` because they
//! mutate panel state via `cx.spawn` + `this.update`), and the tool
//! invocation and response parsing.

use std::time::Duration;

use gpui::{Context, SharedString, Task};
use hkask_types::tool_response::{parse_tool_error, parse_tool_response};
use serde_json::json;

use crate::SWARM_SERVER;
use crate::SwarmEntry;
use crate::SwarmPanel;
use crate::parse::{AgentCard, LocalAgentListResponse, LocalSwarmListResponse, SwarmCard};

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

/// The fetch lifecycle state: in-flight count, per-source errors, and the
/// automatic-retry/backoff state.
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
    /// per-cycle state.
    fn begin_cycle(&mut self, spawn_groups: usize) {
        self.in_flight = spawn_groups;
        self.agents_error = None;
        self.swarms_error = None;
    }

    /// One spawn group completed.
    fn fetch_completed(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }
}

/// Which per-source error slot a fetch failure writes to.
#[derive(Clone, Copy)]
enum FetchErrorSlot {
    Agents,
    Swarms,
}

impl SwarmPanel {
    /// Fetch local agents and local swarms via the governed MCP tool path.
    pub(crate) fn fetch_all(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
            // Not an error state yet: the invoker is wired asynchronously by the
            // deferred post-login task, so a panel opened during startup reaches
            // here before the dispatch path exists. Retry rather than presenting
            // a dead end.
            self.fetch.agents_error = Some(hkask_tool_invoker::NOT_WIRED_MESSAGE.into());
            self.schedule_fetch_retry(cx);
            cx.notify();
            return;
        };

        // 2 spawn groups: local agents and local swarms. Both read the local
        // filesystem through the swarm MCP server — neither depends on the
        // other, so they run in parallel and decrement `in_flight`
        // independently.
        self.fetch.begin_cycle(2);
        cx.notify();

        // Local agents (from `agents/local/curated/` via
        // `swarm_list_local_agents`). Replaces the agent rows; swarm rows are
        // retained.
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_local_agents", json!({}))
                    .await;
                this.update(cx, |this, cx| {
                    this.fetch.fetch_completed();
                    match result {
                        Ok(output) => {
                            // The server returns tool errors as an Ok string
                            // carrying the `{"error": ..., "kind": ...}`
                            // envelope (see `McpToolError::to_json_string`),
                            // not as an `Err` from `invoke_tool`. Route the
                            // envelope through the same classification the
                            // `Err(_)` branch below uses, so a server error
                            // surfaces instead of degrading to a parse failure.
                            if let Some(err) = parse_tool_error(&output) {
                                this.handle_local_fetch_failure(
                                    FetchErrorSlot::Agents,
                                    err.is_retryable(),
                                    &err.message,
                                    cx,
                                );
                            } else {
                                match parse_tool_response(&output).and_then(|c| {
                                    serde_json::from_value::<LocalAgentListResponse>(c).ok()
                                }) {
                                    Some(response) => {
                                        let agents = response
                                            .agents
                                            .into_iter()
                                            .map(|a| {
                                                SwarmEntry::Agent(AgentCard {
                                                    id: a.agent_id,
                                                    agent_type: a.agent_type,
                                                    description: a.description,
                                                    display_name: a.display_name,
                                                })
                                            })
                                            .collect::<Vec<_>>();
                                        // Replace agent entries, keep swarm entries.
                                        this.entries.retain(|e| matches!(e, SwarmEntry::Swarm(_)));
                                        this.entries.extend(agents);
                                        this.fetch.agents_error = None;
                                        this.fetch.note_fetch_success();
                                        this.filter_entries(cx);
                                    }
                                    None => {
                                        this.fetch.agents_error = Some(
                                            format!("Failed to parse agents: {output}").into(),
                                        );
                                        this.filter_entries(cx);
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            this.handle_local_fetch_failure(
                                FetchErrorSlot::Agents,
                                err.is_retryable(),
                                &err.to_string(),
                                cx,
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
        // `swarm_list_local_swarms`). Replaces the swarm rows; agent rows are
        // retained.
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
                            // Same error-envelope seam as the agents fetch — a
                            // server error must surface, not degrade to a
                            // silent empty list.
                            if let Some(err) = parse_tool_error(&output) {
                                this.handle_local_fetch_failure(
                                    FetchErrorSlot::Swarms,
                                    err.is_retryable(),
                                    &err.message,
                                    cx,
                                );
                            } else {
                                match parse_tool_response(&output).and_then(|c| {
                                    serde_json::from_value::<LocalSwarmListResponse>(c).ok()
                                }) {
                                    Some(response) => {
                                        let local_swarms = response
                                            .swarms
                                            .into_iter()
                                            .map(|s| {
                                                SwarmEntry::Swarm(SwarmCard {
                                                    id: s.swarm_id.unwrap_or_default(),
                                                    name: s.name.unwrap_or_default(),
                                                    description: s.mission,
                                                })
                                            })
                                            .collect::<Vec<_>>();
                                        // Replace swarm entries, keep agent
                                        // entries.
                                        this.entries.retain(|e| matches!(e, SwarmEntry::Agent(_)));
                                        this.entries.extend(local_swarms);
                                        this.fetch.swarms_error = None;
                                        this.fetch.note_fetch_success();
                                        this.filter_entries(cx);
                                    }
                                    None => {
                                        this.fetch.swarms_error = Some(
                                            format!("Failed to parse swarms: {output}").into(),
                                        );
                                        this.filter_entries(cx);
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            this.handle_local_fetch_failure(
                                FetchErrorSlot::Swarms,
                                err.is_retryable(),
                                &err.to_string(),
                                cx,
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

    /// Classify a local-fetch failure and update the given per-source error
    /// slot.
    ///
    /// Shared by the `Err(_)` branch of `invoke_tool` (transport-level failure)
    /// and the `Ok(output)` branch when the server returned a tool error
    /// envelope `{"error": ..., "kind": ...}`.
    ///
    /// - Retryable (transport closed, not yet wired): show the reconnect banner
    ///   and schedule a retry.
    /// - Non-retryable: show the cause as the per-source error so an empty list
    ///   is not mistaken for "you have none".
    fn handle_local_fetch_failure(
        &mut self,
        slot: FetchErrorSlot,
        retryable: bool,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        let status: SharedString = if retryable {
            let status = format!("Reconnecting to the swarm server… ({message})").into();
            self.schedule_fetch_retry(cx);
            status
        } else {
            log::warn!("swarm-panel: local fetch failed: {message}");
            format!("Swarm server error: {message}").into()
        };
        match slot {
            FetchErrorSlot::Agents => self.fetch.agents_error = Some(status),
            FetchErrorSlot::Swarms => self.fetch.swarms_error = Some(status),
        }
        self.filter_entries(cx);
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
