//! Swarm Panel — a center-pane `Item` listing Agent Bestiary World agents and
//! swarms (workspaces) as cards, mirroring the Kask Extensions panel layout.
//!
//! Entities are **agents** (from the ABW catalogue) and **swarms** (the
//! operator's workspaces), not skills. Data is fetched through the global
//! `ToolInvoker` hook (the same governed, OCAP-gated path the kask panel's
//! visualization views use), so all ABW calls flow through `hkask-mcp-swarm`
//! and the kask MCP runtime rather than ad-hoc HTTP from the UI.
//!
//! Layout mirrors `KaskExtensionsPage`: headline, search bar, filter toggle
//! (All / Swarms / Agents), a uniform list of `MarketplaceCard`s, and an
//! empty state that surfaces fetch errors. v1 is read-only — hire/fire and
//! spend actions are gated behind the cost/consent gate (see
//! `kask/docs/plans/abw-swarm-intelligence.md` §3.6).

mod panel_button;

pub use panel_button::SwarmPanelButton;

use std::ops::Range;
use std::time::Duration;

use anyhow::Result;
use editor::Editor;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, Render, Task, UniformListScrollHandle, Window,
    actions, uniform_list,
};
use marketplace_ui_common::{MarketplaceCard, marketplace_empty_state, marketplace_search_bar};
use serde::Deserialize;
use serde_json::json;
use ui::{
    ScrollableHandle, ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle,
    ToggleButtonSimple, WithScrollbar, prelude::*,
};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

actions!(
    swarm_panel,
    [
        /// Deploys a new Swarm Panel if none is open, else focuses the
        /// existing one. Used by the View menu entry and the status bar button.
        Toggle,
        /// Focuses an existing Swarm Panel (no-op if none is open).
        ToggleFocus,
    ]
);

/// The MCP server id (matches `BUILT_IN_MCP_SERVERS`).
const SWARM_SERVER: &str = "swarm";

pub fn init(cx: &mut App) {
    register_serializable_item::<SwarmPanel>(cx);
    cx.observe_new(move |workspace: &mut Workspace, window, _cx| {
        let Some(_window) = window else {
            return;
        };
        // Per the `.rules` trap "Center-pane Item Toggle vs ToggleFocus", the
        // View menu entry uses `Toggle` (deploys a new item if none exists),
        // not `ToggleFocus` (silent no-op when absent).
        workspace
            .register_action(move |workspace, _: &Toggle, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<SwarmPanel>());

                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                } else {
                    let swarm_panel = SwarmPanel::new(window, cx);
                    workspace.add_item_to_active_pane(
                        Box::new(swarm_panel.clone()),
                        None,
                        true,
                        window,
                        cx,
                    );
                    // The panel's `focus_handle` delegates to the query editor
                    // (constructed inside `cx.new`), so per the `.rules`
                    // deploy-and-focus trap we focus it explicitly.
                    swarm_panel.focus_handle(cx).focus(window, cx);
                }
            })
            .register_action(move |workspace, _: &ToggleFocus, window, cx| {
                let existing = workspace
                    .active_pane()
                    .read(cx)
                    .items()
                    .find_map(|item| item.downcast::<SwarmPanel>());
                if let Some(existing) = existing {
                    workspace.activate_item(&existing, true, true, window, cx);
                }
            });
    })
    .detach();
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum SwarmFilter {
    All,
    Swarms,
    Agents,
}

/// The three surfaces of the panel: browsing existing agents/swarms, authoring
/// a new agent, and composing agents into a swarm. Sharing (extensions) is
/// represented by the browse surface's discovery role.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PanelMode {
    Browse,
    Author,
    Compose,
}

// ── View model ─────────────────────────────────────────────────────────────

/// One row in the panel — either an ABW agent or an ABW swarm (workspace).
#[derive(Clone, Debug)]
enum SwarmEntry {
    Agent(AgentCard),
    Swarm(SwarmCard),
}

#[derive(Clone, Debug)]
struct AgentCard {
    id: String,
    agent_type: String,
    description: String,
    author: String,
    executions: u64,
}

#[derive(Clone, Debug)]
struct SwarmCard {
    id: String,
    name: String,
    description: String,
    agent_count: u64,
    budget: u64,
    remaining: u64,
}

// ── MCP response structs (minimal, mirror hkask-mcp-swarm's tool output) ────

#[derive(Debug, Deserialize)]
struct AgentListResponse {
    agents: Vec<AgentInfo>,
}

#[derive(Debug, Deserialize)]
struct AgentInfo {
    agent_id: Option<String>,
    agent_type: Option<String>,
    description: Option<String>,
    author: Option<String>,
    execution_stats: Option<ExecutionStats>,
}

#[derive(Debug, Deserialize)]
struct ExecutionStats {
    total_executions: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResponse {
    workspaces: Vec<WorkspaceInfo>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceInfo {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    agent_count: Option<u64>,
    workspace_budget: Option<u64>,
    workspace_remaining: Option<u64>,
}

/// Extract the algedonic wallet balance from a tool response. The server
/// wraps tool output in `{"content": {...}}` and attaches `wallet.balance`
/// when authenticated. Returns `None` when absent — never a fabricated zero.
fn extract_wallet_balance(output: &str) -> Option<i64> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    value
        .get("content")
        .and_then(|c| c.get("wallet"))
        .and_then(|w| w.get("balance"))
        .and_then(|b| b.as_i64())
}

/// Extract agent-name mentions from a Xaman Ek composition response. The
/// curator recommends members in its `response` text and `in_progress` plan;
/// we match `lowercase_with_underscores` tokens that look like agent names.
/// Heuristic by design — the operator reviews before applying.
fn extract_agent_mentions(content: &serde_json::Value) -> Vec<String> {
    let mut found = Vec::new();
    // Prefer the structured plan when present.
    if let Some(members) = content
        .get("in_progress")
        .and_then(|p| p.get("members"))
        .and_then(|m| m.as_array())
    {
        for member in members {
            if let Some(name) = member
                .get("agent_id")
                .and_then(|a| a.as_str())
                .or_else(|| member.get("agent_name").and_then(|a| a.as_str()))
            {
                found.push(name.to_string());
            }
        }
    }
    if !found.is_empty() {
        return found;
    }
    // Fall back to scanning the response text for agent-name-shaped tokens.
    if let Some(text) = content.get("response").and_then(|r| r.as_str()) {
        for token in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if token.len() > 3
                && token.contains('_')
                && token
                    .chars()
                    .all(|c| c.is_lowercase() || c.is_numeric() || c == '_')
            {
                found.push(token.to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

// ── Panel ──────────────────────────────────────────────────────────────────

pub struct SwarmPanel {
    list: UniformListScrollHandle,
    is_fetching: bool,
    fetch_error: Option<SharedString>,
    filter: SwarmFilter,
    entries: Vec<SwarmEntry>,
    filtered_entry_indices: Vec<usize>,
    query_editor: Entity<Editor>,
    _subscriptions: [gpui::Subscription; 1],
    search_task: Option<Task<()>>,
    /// Current ABW wallet balance (the algedonic channel). `None` = unknown
    /// (unauthenticated or the balance query failed) — never a fabricated zero.
    wallet_balance: Option<i64>,
    /// In-flight consent prompt for a hire action: the agent being considered
    /// plus its pre-flight cost estimate. `Some` renders the consent banner.
    pending_hire: Option<PendingHire>,
    /// The workspace (swarm) id new hires target. Defaults to the first
    /// workspace once swarms load; selectable when there are several.
    selected_workspace: Option<String>,
    /// A spend currently in flight (after consent), shown as a busy state.
    spend_in_flight: Option<String>,
    /// Which surface is active: browse, author, or compose.
    mode: PanelMode,
    /// Authoring form state.
    author: AuthorForm,
    /// Composition form state.
    compose: ComposeForm,
}

/// State for the agent-authoring surface.
struct AuthorForm {
    name: Entity<Editor>,
    description: Entity<Editor>,
    system_prompt: Entity<Editor>,
    agent_type: String,
    /// Result of the last create attempt (success id or error).
    status: Option<SharedString>,
    busy: bool,
}

/// State for the swarm-composition surface.
struct ComposeForm {
    name: Entity<Editor>,
    mission: Entity<Editor>,
    /// Agent names to hire, comma-separated (kept as a single-line editor for v1).
    agents: Entity<Editor>,
    status: Option<SharedString>,
    busy: bool,
    /// Xaman Ek consultation: the operator's composition question.
    xaman_query: Entity<Editor>,
    /// The active Xaman Ek composition session id (continues across messages).
    xaman_session: Option<String>,
    /// The curator's latest response text.
    xaman_response: Option<SharedString>,
    /// Agent names Xaman Ek recommended (extracted from a composition plan),
    /// offered as a one-click pre-fill of the agents field.
    xaman_suggested_agents: Vec<String>,
    xaman_busy: bool,
}

/// A hire awaiting operator consent. The gate holds the pre-flight estimate
/// and blocks the (v2) spend until the operator explicitly authorizes it.
#[derive(Clone, Debug)]
struct PendingHire {
    agent_name: String,
    total_hire_cost: u64,
    required_cost: u64,
    optional_cost: u64,
    within_budget: bool,
    max_credits: u32,
}

impl SwarmPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Entity<Self> {
        cx.new(|cx| {
            let query_editor = cx.new(|cx| {
                let mut input = Editor::single_line(window, cx);
                input.set_placeholder_text("Search agents and swarms...", window, cx);
                input
            });
            let subscriptions = [cx.subscribe(&query_editor, Self::on_query_change)];

            let scroll_handle = UniformListScrollHandle::new();

            let author = AuthorForm {
                name: cx.new(|cx| {
                    let mut e = Editor::single_line(window, cx);
                    e.set_placeholder_text("agent_name (lowercase_with_underscores)", window, cx);
                    e
                }),
                description: cx.new(|cx| {
                    let mut e = Editor::single_line(window, cx);
                    e.set_placeholder_text("One-sentence description", window, cx);
                    e
                }),
                system_prompt: cx.new(|cx| {
                    let mut e = Editor::single_line(window, cx);
                    e.set_placeholder_text("System prompt — the agent's instructions", window, cx);
                    e
                }),
                agent_type: "research".to_string(),
                status: None,
                busy: false,
            };
            let compose = ComposeForm {
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
                    e.set_placeholder_text(
                        "Agents to hire, comma-separated (optional)",
                        window,
                        cx,
                    );
                    e
                }),
                status: None,
                busy: false,
                xaman_query: cx.new(|cx| {
                    let mut e = Editor::single_line(window, cx);
                    e.set_placeholder_text("Ask Xaman Ek to plan your team…", window, cx);
                    e
                }),
                xaman_session: None,
                xaman_response: None,
                xaman_suggested_agents: Vec::new(),
                xaman_busy: false,
            };

            let mut this = Self {
                list: scroll_handle,
                is_fetching: false,
                fetch_error: None,
                filter: SwarmFilter::All,
                entries: Vec::new(),
                filtered_entry_indices: Vec::new(),
                query_editor,
                _subscriptions: subscriptions,
                search_task: None,
                wallet_balance: None,
                pending_hire: None,
                selected_workspace: None,
                spend_in_flight: None,
                mode: PanelMode::Browse,
                author,
                compose,
            };
            this.fetch_all(cx);
            this
        })
    }

    /// Fetch agents and swarms via the governed MCP tool path.
    fn fetch_all(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
            self.fetch_error = Some(
                "Tool invoker not wired — the swarm MCP server is unavailable. \
                 Ensure kask MCP servers are enabled (kask.mcp.load_default)."
                    .into(),
            );
            cx.notify();
            return;
        };

        self.is_fetching = true;
        self.fetch_error = None;
        cx.notify();

        // Agents (keyless-capable).
        cx.spawn({
            let invoker = invoker.clone();
            async move |this, cx| {
                let result = invoker
                    .invoke_tool(SWARM_SERVER, "swarm_list_agents", json!({ "limit": 200 }))
                    .await;
                this.update(cx, |this, cx| {
                    this.is_fetching = false;
                    if let Ok(balance) = &result
                        && let Some(b) = extract_wallet_balance(balance)
                    {
                        this.wallet_balance = Some(b);
                    }
                    match result {
                        Ok(output) => match serde_json::from_str::<AgentListResponse>(&output) {
                            Ok(response) => {
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
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                // Replace agent entries, keep swarm entries.
                                this.entries.retain(|e| matches!(e, SwarmEntry::Swarm(_)));
                                this.entries.extend(agents);
                                this.fetch_error = None;
                                this.filter_entries(cx);
                            }
                            Err(err) => {
                                this.fetch_error =
                                    Some(format!("Failed to parse agents: {err}").into());
                                this.filter_entries(cx);
                            }
                        },
                        Err(err) => {
                            this.fetch_error = Some(format!("Failed to list agents: {err}").into());
                            this.filter_entries(cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();

        // Swarms (requires the ABW API key).
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(SWARM_SERVER, "swarm_get_swarm", json!({}))
                .await;
            this.update(cx, |this, cx| match result {
                Ok(output) => {
                    if let Some(b) = extract_wallet_balance(&output) {
                        this.wallet_balance = Some(b);
                    }
                    match serde_json::from_str::<WorkspaceListResponse>(&output) {
                        Ok(response) => {
                            let swarms = response
                                .workspaces
                                .into_iter()
                                .map(|w| {
                                    SwarmEntry::Swarm(SwarmCard {
                                        id: w.id.unwrap_or_default(),
                                        name: w.name.unwrap_or_default(),
                                        description: w.description.unwrap_or_default(),
                                        agent_count: w.agent_count.unwrap_or(0),
                                        budget: w.workspace_budget.unwrap_or(0),
                                        remaining: w.workspace_remaining.unwrap_or(0),
                                    })
                                })
                                .collect::<Vec<_>>();
                            // Replace swarm entries, keep agent entries.
                            this.entries.retain(|e| matches!(e, SwarmEntry::Agent(_)));
                            let mut swarms = swarms;
                            swarms.extend(this.entries.drain(..));
                            this.entries = swarms;
                            // Default the hire target to the first swarm if unset.
                            if this.selected_workspace.is_none() {
                                this.selected_workspace =
                                    this.entries.iter().find_map(|e| match e {
                                        SwarmEntry::Swarm(s) if !s.id.is_empty() => {
                                            Some(s.id.clone())
                                        }
                                        _ => None,
                                    });
                            }
                            this.filter_entries(cx);
                        }
                        Err(err) => {
                            this.fetch_error =
                                Some(format!("Failed to parse workspaces: {err}").into());
                            this.filter_entries(cx);
                        }
                    }
                }
                Err(err) => {
                    // Auth failures here are expected when no key is configured —
                    // degrade to agents-only rather than an error state.
                    log::warn!("swarm-panel: could not fetch workspaces (agents-only mode): {err}");
                    this.filter_entries(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Fetch the pre-flight hire cost for an agent and open the consent gate.
    /// This is the entry point to the cost/consent flow: read-only, spends
    /// nothing, and populates `pending_hire` so the banner renders.
    fn begin_hire(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
            self.fetch_error = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
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
                        let parsed: Option<serde_json::Value> = serde_json::from_str(&output)
                            .ok()
                            .and_then(|v: serde_json::Value| v.get("content").cloned().or(Some(v)));
                        match parsed {
                            Some(content) => {
                                this.pending_hire = Some(PendingHire {
                                    agent_name: agent_name.clone(),
                                    total_hire_cost: content
                                        .get("total_hire_cost")
                                        .and_then(|c| c.as_u64())
                                        .unwrap_or(0),
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
                                    // Fallback mirrors `SwarmConfig::default().max_credits_per_dispatch`
                                    // (50) — the server always sends this field, so the fallback
                                    // only fires on a malformed response. Keep in sync with the
                                    // server default if it changes.
                                    max_credits: content
                                        .get("max_credits_per_dispatch")
                                        .and_then(|c| c.as_u64())
                                        .unwrap_or(50)
                                        as u32,
                                });
                            }
                            None => {
                                this.fetch_error =
                                    Some(format!("Failed to parse hire cost: {output}").into());
                            }
                        }
                    }
                    Err(err) => {
                        this.fetch_error =
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
    fn confirm_hire(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_hire.take() else {
            return;
        };
        let Some(workspace_id) = self.selected_workspace.clone() else {
            self.fetch_error =
                Some("No swarm selected to hire into. Create a workspace on ABW first.".into());
            cx.notify();
            return;
        };
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
            self.fetch_error = Some("Tool invoker not wired.".into());
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
                Ok(output) => {
                    let parsed: Option<serde_json::Value> = serde_json::from_str(&output)
                        .ok()
                        .and_then(|v: serde_json::Value| v.get("content").cloned().or(Some(v)));
                    parsed.and_then(|c| {
                        c.get("consent_token")
                            .and_then(|t| t.as_str())
                            .map(str::to_string)
                    })
                }
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.spend_in_flight = None;
                        this.fetch_error = Some(format!("Consent failed: {err}").into());
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            let Some(token) = token else {
                this.update(cx, |this, cx| {
                    this.spend_in_flight = None;
                    this.fetch_error = Some("Consent did not return a token.".into());
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
                    }
                    Err(err) => {
                        this.fetch_error = Some(format!("Hire failed: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Operator declined the hire — clear the gate without spending.
    fn cancel_hire(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_hire.take() {
            log::info!(
                "swarm-panel: operator declined hire of '{}' (gate aborted)",
                pending.agent_name
            );
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: PanelMode, cx: &mut Context<Self>) {
        self.mode = mode;
        cx.notify();
    }

    /// Create a new agent from the authoring form.
    fn create_agent(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let name = self.author.name.read(cx).text(cx);
        let description = self.author.description.read(cx).text(cx);
        let system_prompt = self.author.system_prompt.read(cx).text(cx);
        if name.trim().is_empty() || system_prompt.trim().is_empty() {
            self.author.status = Some("Name and system prompt are required.".into());
            cx.notify();
            return;
        }
        let agent_type = self.author.agent_type.clone();
        self.author.busy = true;
        self.author.status = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_create_agent",
                    json!({
                        "agent_name": name.trim(),
                        "agent_type": agent_type,
                        "system_prompt": system_prompt.trim(),
                        "description": description.trim(),
                    }),
                )
                .await;
            this.update(cx, |this, cx| {
                this.author.busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.wallet_balance = Some(b);
                        }
                        this.author.status =
                            Some(format!("Agent '{}' created.", name.trim()).into());
                        // Refresh so the new agent appears in browse.
                        this.fetch_all(cx);
                    }
                    Err(err) => {
                        this.author.status = Some(format!("Create failed: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Create a new swarm from the compose form, hiring any listed agents.
    fn create_swarm(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
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
        let mission = self.compose.mission.read(cx).text(cx);
        let agents_raw = self.compose.agents.read(cx).text(cx);
        let agents: Vec<String> = agents_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        self.compose.busy = true;
        self.compose.status = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            // Mint a consent token per agent to hire (each hire is gated).
            let mut consent_tokens = Vec::new();
            for agent in &agents {
                match invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_request_consent",
                        json!({ "action": "hire", "target": agent, "credits_authorized": 5 }),
                    )
                    .await
                {
                    Ok(output) => {
                        let token = serde_json::from_str::<serde_json::Value>(&output)
                            .ok()
                            .and_then(|v| v.get("content").cloned().or(Some(v)))
                            .and_then(|c| {
                                c.get("consent_token")
                                    .and_then(|t| t.as_str())
                                    .map(str::to_string)
                            });
                        if let Some(t) = token {
                            consent_tokens.push(t);
                        }
                    }
                    Err(_) => {}
                }
            }

            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_create_swarm",
                    json!({
                        "name": name.trim(),
                        "mission": if mission.trim().is_empty() { None } else { Some(mission.trim()) },
                        "agents": agents,
                        "consent_tokens": consent_tokens,
                    }),
                )
                .await;
            this.update(cx, |this, cx| {
                this.compose.busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.wallet_balance = Some(b);
                        }
                        this.compose.status =
                            Some(format!("Swarm '{}' created.", name.trim()).into());
                        this.fetch_all(cx);
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

    /// Consult Xaman Ek (composition_design session) from the Compose surface.
    /// Continues the active session across messages so the operator can refine
    /// the team iteratively, then surfaces any recommended agents for pre-fill.
    fn ask_xaman(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
            self.compose.xaman_response = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let message = self.compose.xaman_query.read(cx).text(cx);
        if message.trim().is_empty() {
            return;
        }
        self.compose.xaman_busy = true;
        self.compose.xaman_response = None;
        cx.notify();
        let session_id = self.compose.xaman_session.clone();

        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_xaman",
                    json!({
                        "message": message.trim(),
                        "session_type": "composition_design",
                        "session_id": session_id,
                    }),
                )
                .await;
            this.update(cx, |this, cx| {
                this.compose.xaman_busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.wallet_balance = Some(b);
                        }
                        let parsed = serde_json::from_str::<serde_json::Value>(&output)
                            .ok()
                            .and_then(|v| v.get("content").cloned().or(Some(v)));
                        if let Some(content) = parsed {
                            // Continue the session.
                            if let Some(sid) = content.get("session_id").and_then(|s| s.as_str()) {
                                this.compose.xaman_session = Some(sid.to_string());
                            }
                            if let Some(resp) = content.get("response").and_then(|r| r.as_str()) {
                                this.compose.xaman_response = Some(resp.to_string().into());
                            }
                            // Extract recommended agent names from the response
                            // text (Xaman Ek lists members by name) so the
                            // operator can pre-fill the agents field.
                            this.compose.xaman_suggested_agents = extract_agent_mentions(&content);
                        }
                    }
                    Err(err) => {
                        this.compose.xaman_response =
                            Some(format!("Xaman Ek unavailable: {err}").into());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pre-fill the agents field with Xaman Ek's recommended team.
    fn apply_xaman_suggestions(&mut self, cx: &mut Context<Self>) {
        if self.compose.xaman_suggested_agents.is_empty() {
            return;
        }
        let joined = self.compose.xaman_suggested_agents.join(", ");
        let agents_editor = self.compose.agents.clone();
        agents_editor.update(cx, |editor, cx| {
            editor.set_text(joined, cx);
        });
        cx.notify();
    }

    fn filter_entries(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter;
        let query = self.search_query(cx).map(|q| q.to_lowercase());
        let indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                let kind_matches = match (filter, entry) {
                    (SwarmFilter::All, _) => true,
                    (SwarmFilter::Swarms, SwarmEntry::Swarm(_)) => true,
                    (SwarmFilter::Agents, SwarmEntry::Agent(_)) => true,
                    _ => false,
                };
                if !kind_matches {
                    return false;
                }
                match &query {
                    None => true,
                    Some(q) => {
                        let haystack = match entry {
                            SwarmEntry::Agent(a) => {
                                format!("{} {} {} {}", a.id, a.agent_type, a.description, a.author)
                            }
                            SwarmEntry::Swarm(s) => {
                                format!("{} {} {}", s.id, s.name, s.description)
                            }
                        };
                        haystack.to_lowercase().contains(q)
                    }
                }
            })
            .map(|(ix, _)| ix)
            .collect();
        self.filtered_entry_indices = indices;
        cx.notify();
    }

    fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        self.list
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }

    fn render_entries(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<MarketplaceCard> {
        let mut cards = Vec::new();
        for ix in range {
            if ix >= self.filtered_entry_indices.len() {
                break;
            }
            let entry_ix = self.filtered_entry_indices[ix];
            let entry = self.entries[entry_ix].clone();
            cards.push(self.render_card(entry, cx));
        }
        cards
    }

    fn render_card(&mut self, entry: SwarmEntry, cx: &mut Context<Self>) -> MarketplaceCard {
        match entry {
            SwarmEntry::Agent(agent) => {
                let agent_name = agent.id.clone();
                MarketplaceCard::new().child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            v_flex()
                                .min_w_0()
                                .flex_1()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(Label::new(agent.id.clone()).color(Color::Default))
                                        .child(
                                            Label::new(agent.agent_type.clone())
                                                .color(Color::Accent),
                                        )
                                        .child(
                                            Label::new(format!("▶ {}", agent.executions))
                                                .color(Color::Muted),
                                        )
                                        .child(
                                            Label::new(format!("by {}", agent.author))
                                                .color(Color::Muted),
                                        ),
                                )
                                .child(Label::new(agent.description.clone()).color(Color::Muted)),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .items_end()
                                .child(
                                    Label::new("Agent")
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                                .child(
                                    Button::new(
                                        SharedString::from(format!("hire-{agent_name}")),
                                        if self.spend_in_flight.as_deref()
                                            == Some(agent_name.as_str())
                                        {
                                            "Hiring…"
                                        } else {
                                            "Hire…"
                                        },
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .label_size(LabelSize::XSmall)
                                    .disabled(self.spend_in_flight.is_some())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.begin_hire(agent_name.clone(), cx);
                                        },
                                    )),
                                ),
                        ),
                )
            }
            SwarmEntry::Swarm(swarm) => MarketplaceCard::new().child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(Label::new(swarm.name.clone()).color(Color::Default))
                                    .child(
                                        Label::new(format!("{} agents", swarm.agent_count))
                                            .color(Color::Accent),
                                    )
                                    .child(
                                        Label::new(format!(
                                            "⛽ {}/{}",
                                            swarm.remaining, swarm.budget
                                        ))
                                        .color(Color::Muted),
                                    ),
                            )
                            .child(Label::new(swarm.description.clone()).color(Color::Muted)),
                    )
                    .child(
                        Label::new("Swarm")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            ),
        }
    }

    fn render_search(&self, cx: &mut Context<Self>) -> Div {
        marketplace_search_bar(&self.query_editor, false, cx)
    }

    /// The agent-authoring surface: name, description, system prompt, create.
    fn render_author(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(Headline::new("Author an Agent").size(HeadlineSize::Small))
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
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.author.name.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Description")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.author.description.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("System prompt")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.author.system_prompt.clone()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new(
                            "create-agent",
                            if self.author.busy {
                                "Creating…"
                            } else {
                                "Create Agent"
                            },
                        )
                        .style(ButtonStyle::Filled)
                        .disabled(self.author.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.create_agent(cx);
                        })),
                    )
                    .when_some(self.author.status.clone(), |this, status| {
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
    }

    /// The swarm-composition surface: name, mission, agents, create.
    fn render_compose(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border;
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(Headline::new("Compose a Swarm").size(HeadlineSize::Small))
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
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
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
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.compose.mission.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("Agents to hire (comma-separated)")
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .border_1()
                            .border_color(border)
                            .rounded_sm()
                            .child(self.compose.agents.clone()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new(
                            "create-swarm",
                            if self.compose.busy {
                                "Creating…"
                            } else {
                                "Create Swarm"
                            },
                        )
                        .style(ButtonStyle::Filled)
                        .disabled(self.compose.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.create_swarm(cx);
                        })),
                    )
                    .when_some(self.compose.status.clone(), |this, status| {
                        this.child(
                            Label::new(status)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
    }

    fn on_query_change(
        &mut self,
        _: Entity<Editor>,
        event: &editor::EditorEvent,
        cx: &mut Context<Self>,
    ) {
        if let editor::EditorEvent::Edited { .. } = event {
            self.refresh_search(cx);
        }
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        // Debounce search, then filter locally — both lists arrive in one
        // fetch each, so keystrokes must not re-hit the network.
        self.search_task = Some(cx.spawn(async move |this, cx| {
            let search = this
                .update(cx, |this, cx| this.search_query(cx))
                .ok()
                .flatten();

            if search.is_some() {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
            };

            this.update(cx, |this, cx| {
                this.filter_entries(cx);
                this.scroll_to_top(cx);
            })
            .ok();
        }));
    }

    pub fn search_query(&self, cx: &mut App) -> Option<String> {
        let search = self.query_editor.read(cx).text(cx);
        if search.trim().is_empty() {
            None
        } else {
            Some(search)
        }
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_search = self.search_query(cx).is_some();

        let message: SharedString = if self.is_fetching {
            "Loading agents and swarms…".into()
        } else if let Some(fetch_error) = &self.fetch_error {
            format!("Failed to load swarm data: {fetch_error}").into()
        } else {
            match self.filter {
                SwarmFilter::All => {
                    if has_search {
                        "No agents or swarms that match your search."
                    } else {
                        "No agents or swarms. Set HKASK_ABW_API_KEY to see your swarms."
                    }
                }
                SwarmFilter::Swarms => {
                    if has_search {
                        "No swarms that match your search."
                    } else {
                        "No swarms. Set HKASK_ABW_API_KEY to see your workspaces."
                    }
                }
                SwarmFilter::Agents => {
                    if has_search {
                        "No agents that match your search."
                    } else {
                        "No agents."
                    }
                }
            }
            .into()
        };

        marketplace_empty_state(message, self.fetch_error.is_some())
    }

    /// The cost/consent gate banner. Renders only when a hire is pending
    /// operator authorization. Shows the pre-flight estimate and blocks the
    /// spend until the operator explicitly confirms or cancels — the
    /// enforcement point for the `.rules` "advertised invariants need
    /// enforcement points" trap (the gate blocks, it does not just warn).
    fn render_consent_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.pending_hire.clone()?;
        let border = cx.theme().colors().border;
        let warning = cx.theme().status().warning;

        let cost_line = if pending.optional_cost > 0 {
            format!(
                "Hire '{}' — {} credits (required {} + optional {})",
                pending.agent_name,
                pending.total_hire_cost,
                pending.required_cost,
                pending.optional_cost
            )
        } else {
            format!(
                "Hire '{}' — {} credits",
                pending.agent_name, pending.total_hire_cost
            )
        };

        let budget_note = if pending.within_budget {
            format!("Within your {}-credit dispatch limit.", pending.max_credits)
        } else {
            format!(
                "Exceeds your {}-credit dispatch limit — confirm to override.",
                pending.max_credits
            )
        };

        Some(
            v_flex()
                .w_full()
                .gap_2()
                .p_3()
                .rounded_sm()
                .border_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Label::new("Confirm spend").color(Color::Default))
                        .child(Label::new(cost_line).size(LabelSize::Small).color(
                            if pending.within_budget {
                                Color::Muted
                            } else {
                                Color::Warning
                            },
                        )),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Label::new(budget_note).size(LabelSize::XSmall).color(
                            if pending.within_budget {
                                Color::Muted
                            } else {
                                Color::Warning
                            },
                        ))
                        .child(div().flex_1())
                        .child(
                            Button::new("confirm-hire", "Confirm")
                                .style(ButtonStyle::Filled)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm_hire(cx);
                                })),
                        )
                        .child(
                            Button::new("cancel-hire", "Cancel")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_hire(cx);
                                })),
                        ),
                )
                .when(!pending.within_budget, |this| {
                    this.child(div().w_full().h(px(2.)).bg(warning))
                }),
        )
    }
}

impl Render for SwarmPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .child(
                v_flex()
                    .gap_4()
                    .pt_4()
                    .px_4()
                    .bg(cx.theme().colors().editor_background)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .justify_between()
                            .child(Headline::new("Agent Swarm").size(HeadlineSize::Large))
                            // The algedonic channel: the operator's ABW credit
                            // balance is always visible when known, so a spend
                            // never happens out of sight. Hidden when unknown
                            // (unauthenticated) — never a fabricated zero.
                            .when_some(self.wallet_balance, |this, balance| {
                                this.child(
                                    Label::new(format!("⛽ {balance} credits"))
                                        .size(LabelSize::Small)
                                        .color(if balance <= 0 {
                                            Color::Warning
                                        } else {
                                            Color::Muted
                                        }),
                                )
                            }),
                    )
                    .children(self.render_consent_banner(cx))
                    // The three surfaces: Browse (discovery/sharing), Author
                    // (agents), Compose (swarms).
                    .child(
                        div().child(
                            ToggleButtonGroup::single_row(
                                "swarm-mode-buttons",
                                [
                                    ToggleButtonSimple::new(
                                        "Browse",
                                        cx.listener(|this, _event, _, cx| {
                                            this.set_mode(PanelMode::Browse, cx);
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Author",
                                        cx.listener(|this, _event, _, cx| {
                                            this.set_mode(PanelMode::Author, cx);
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Compose",
                                        cx.listener(|this, _event, _, cx| {
                                            this.set_mode(PanelMode::Compose, cx);
                                        }),
                                    ),
                                ],
                            )
                            .style(ToggleButtonGroupStyle::Outlined)
                            .size(ToggleButtonGroupSize::Custom(rems_from_px(30.)))
                            .label_size(LabelSize::Default)
                            .auto_width()
                            .selected_index(match self.mode {
                                PanelMode::Browse => 0,
                                PanelMode::Author => 1,
                                PanelMode::Compose => 2,
                            })
                            .into_any_element(),
                        ),
                    )
                    .when(self.mode == PanelMode::Browse, |this| {
                        this.child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .gap_2()
                                .child(self.render_search(cx))
                                .child(
                                    div().child(
                                        ToggleButtonGroup::single_row(
                                            "swarm-filter-buttons",
                                            [
                                                ToggleButtonSimple::new(
                                                    "All",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.filter = SwarmFilter::All;
                                                        this.filter_entries(cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Swarms",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.filter = SwarmFilter::Swarms;
                                                        this.filter_entries(cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Agents",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.filter = SwarmFilter::Agents;
                                                        this.filter_entries(cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                            ],
                                        )
                                        .style(ToggleButtonGroupStyle::Outlined)
                                        .size(ToggleButtonGroupSize::Custom(rems_from_px(30.)))
                                        .label_size(LabelSize::Default)
                                        .auto_width()
                                        .selected_index(match self.filter {
                                            SwarmFilter::All => 0,
                                            SwarmFilter::Swarms => 1,
                                            SwarmFilter::Agents => 2,
                                        })
                                        .into_any_element(),
                                    ),
                                ),
                        )
                    }),
            )
            .child(
                v_flex()
                    .px_4()
                    .size_full()
                    .overflow_y_hidden()
                    .map(|this| match self.mode {
                        PanelMode::Author => this.child(self.render_author(cx)).into_any_element(),
                        PanelMode::Compose => {
                            this.child(self.render_compose(cx)).into_any_element()
                        }
                        PanelMode::Browse => {
                            let count = self.filtered_entry_indices.len();
                            if count == 0 {
                                this.child(self.render_empty_state(cx)).into_any_element()
                            } else {
                                let scroll_handle = &self.list;
                                this.child(
                                    uniform_list(
                                        "swarm-entries",
                                        count,
                                        cx.processor(Self::render_entries),
                                    )
                                    .flex_grow_1()
                                    .pb_4()
                                    .track_scroll(scroll_handle),
                                )
                                .vertical_scrollbar_for(scroll_handle, window, cx)
                                .into_any_element()
                            }
                        }
                    }),
            )
    }
}

impl EventEmitter<ItemEvent> for SwarmPanel {}

impl Focusable for SwarmPanel {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.query_editor.read(cx).focus_handle(cx)
    }
}

impl Item for SwarmPanel {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "Agent Swarm".into()
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Swarm Panel Opened")
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Share).color(Color::Muted))
    }

    fn show_toolbar(&self) -> bool {
        false
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(workspace::item::ItemEvent)) {
        f(*event)
    }
}

impl SerializableItem for SwarmPanel {
    fn serialized_item_kind() -> &'static str {
        "SwarmPanel"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: workspace::WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Entity<Self>>> {
        // Stateless item — nothing to persist beyond the fact that it's open.
        // The panel reconstructs its state from ABW on first render.
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| SwarmPanel::new(window, cx))
        })
    }

    fn serialize(
        &mut self,
        _workspace: &mut Workspace,
        _item_id: workspace::ItemId,
        _closing: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Task<Result<()>>> {
        // Stateless item — nothing to persist beyond the fact that it's open.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pin the tool names the panel calls against the server's tool surface —
    // a rename in `hkask-mcp-swarm` must fail here, not silently degrade the
    // panel to an empty state.
    #[test]
    fn panel_tool_names_match_server() {
        // These strings must match the #[tool] fn names in
        // `hkask-mcp-swarm/src/hkask_mcp_swarm.rs`.
        assert_eq!(SWARM_SERVER, "swarm");
        for tool in [
            "swarm_list_agents",
            "swarm_get_swarm",
            "swarm_hire_cost",
            "swarm_request_consent",
            "swarm_hire",
            "swarm_delegate",
            "swarm_run_status",
            "swarm_generate_prompt",
            "swarm_generate_ontology",
            "swarm_create_agent",
            "swarm_create_swarm",
        ] {
            assert!(tool.starts_with("swarm_"));
        }
    }

    // The algedonic wallet signal must survive the content envelope and never
    // be fabricated. These pin the extraction against the server's actual
    // output shape (`{"content": {..., "wallet": {"balance": N}}}`).
    #[test]
    fn extract_wallet_balance_reads_content_envelope() {
        let out = r#"{"content":{"count":2,"wallet":{"balance":9977}}}"#;
        assert_eq!(extract_wallet_balance(out), Some(9977));
    }

    #[test]
    fn extract_wallet_balance_absent_when_no_wallet() {
        // Catalogue-only mode: no wallet key → None, never a fabricated zero.
        let out = r#"{"content":{"count":2,"authenticated":false}}"#;
        assert_eq!(extract_wallet_balance(out), None);
    }

    #[test]
    fn extract_wallet_balance_absent_on_garbage() {
        assert_eq!(extract_wallet_balance("not json"), None);
        assert_eq!(extract_wallet_balance("{}"), None);
    }
}
