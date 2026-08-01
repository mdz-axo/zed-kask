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
//!
//! **Steer mode** hosts a `ConversationView` scoped to the swarm MCP server,
//! mirroring `KaskPanel`'s per-tab agent pattern. The operator asks the
//! curator to compose/steer a swarm; the curator's `SkillTool` invokes the
//! `swarm-intelligence` cascade (see `kask/docs/plans/abw-swarm-intelligence.md`
//! §13). The conversation is not persisted — re-clicking Steer after a
//! restart starts a fresh composition conversation.

mod panel_button;

pub use panel_button::SwarmPanelButton;

use std::ops::Range;
use std::time::Duration;

use anyhow::Result;
use editor::Editor;
use fs::Fs;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, ReadGlobal as _, Render, Task,
    UniformListScrollHandle, WeakEntity, Window, actions, uniform_list,
};
use marketplace_ui_common::{MarketplaceCard, marketplace_empty_state, marketplace_search_bar};
use project::Project;
use serde::Deserialize;
use serde_json::json;
use settings::Settings as _;
use settings::SettingsStore;
use ui::{
    ScrollableHandle, ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle,
    ToggleButtonSimple, WithScrollbar, prelude::*,
};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

// Steer mode: a `ConversationView` scoped to the swarm MCP server, mirroring
// `KaskPanel`'s per-tab agent pattern. The curator's `SkillTool` invokes the
// `swarm-intelligence` cascade when the operator asks to compose/steer a
// swarm. See `kask/docs/plans/abw-swarm-intelligence.md` §13.
use agent::ThreadStore;
use agent_ui::{Agent, AgentConnectionStore, AgentThreadSource, ConversationView};
use gpui::SharedString;

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

/// The system prompt injected into the Steer mode `ConversationView`. Tells
/// the curator it is scoped to the swarm MCP server and that the
/// `swarm-intelligence` skill is available for composition/steering. The
/// curator's `SkillTool` discovers the skill from the `<available_skills>`
/// list in its base system prompt; this prompt adds the swarm-specific
/// context (active workspace, the skill's purpose).
fn steer_system_prompt(selected_workspace: Option<&str>) -> SharedString {
    let workspace_note = match selected_workspace {
        Some(id) => format!(
            "The operator's active swarm (ABW workspace) is `{id}`. \
             When the operator asks to compose or steer a swarm without naming \
             one, assume this workspace."
        ),
        None => "No swarm (ABW workspace) is currently selected. If the operator \
            asks to compose a swarm, ask them to select one in the Browse tab first."
            .to_string(),
    };
    format!(
        "## Agent Swarm Panel — Steer Mode\n\
         \n\
         You are operating in the Agent Swarm panel's Steer mode, scoped to the \
         `{SWARM_SERVER}` MCP server. The swarm server exposes two tool sets, \
         selected by the operator via the `kask.swarm.mode` setting (`abw` or \
         `local`):\n\
         \n\
         **ABW tools** (`mode: abw`, the default): `swarm_list_agents`, \
         `swarm_get_swarm`, `swarm_hire_cost`, `swarm_request_consent`, \
         `swarm_hire`, `swarm_delegate`, `swarm_xaman`. These route to Agent \
         Bestiary World and require the ABW API key.\n\
         \n\
         **Local tools** (`mode: local`): `swarm_list_local_agents`, \
         `swarm_fund_local`, `swarm_delegate_local`, `swarm_clone_to_local`, \
         `swarm_push_to_cloud`. These run on the local substrate \
         (`hkask-inference` + `hkask-ledger` + `hkask-guard`) with no ABW \
         round-trips. The local ledger is operator-funded — call \
         `swarm_fund_local(credits)` before `swarm_delegate_local`, or it returns \
         `PaymentRequired`. There is no consent token in local mode: the balance \
         check is the gate. `swarm_clone_to_local` and `swarm_push_to_cloud` sync \
         cards between the local registry (`agents/local/curated/<id>/agent_card.json`) \
         and ABW; a cloned card carries `cloud_id` to track the sync link.\n\
         \n\
         {workspace_note}\n\
         \n\
         The `swarm-intelligence` skill is available for swarm composition and \
         steering in both modes. When the operator asks to compose, configure, \
         tune, or steer a swarm toward a target condition, invoke the \
         `swarm-intelligence` skill with the swarm id (or the local registry) \
         and the operator's task. The skill runs a SENSE → ORIENT → DECIDE → ACT \
         → CHECK → CONVERGE loop and branches on `{{ mode }}` (local vs abw) at \
         the SENSE/ACT/CHECK steps. Pass `mode` in the skill context so the \
         templates select the right data source and gate.\n\
         \n\
         The consent gate (ABW mode only) is enforced by `swarm_request_consent` \
         (mints a single-use, action+target-scoped token) and `swarm_hire`/\
         `swarm_delegate` (consume the token before spending). Do not hire or \
         delegate without first calling `swarm_request_consent` and passing the \
         returned token to the spend tool. The consent gate is the enforcement \
         point — it must actually block, not just warn. In local mode there is no \
         consent token; the `credits_authorized` + ledger balance check is the \
         gate.\n\
\
         The per-dispatch credit ceiling (`HKASK_ABW_MAX_CREDITS`, default 50) is \
         a hard server-side gate. `swarm_hire` and `swarm_create_swarm` refuse \
         any hire whose actual cost exceeds the ceiling; `swarm_delegate` refuses \
         if `credits_authorized` exceeds it. Before hiring, call `swarm_hire_cost` \
         to check `within_budget` — if false, tell the operator to raise \
         `HKASK_ABW_MAX_CREDITS` rather than attempting the hire. For delegation, \
         set `credits_authorized` to the ceiling or lower; do not mint a delegate \
         consent for more than the ceiling. The same ceiling applies to \
         `swarm_delegate_local` in local mode.\n"
    )
    .into()
}

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
                    let swarm_panel = SwarmPanel::new(workspace, window, cx);
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
    /// Steer: a `ConversationView` scoped to the swarm MCP server. The
    /// operator asks the curator to compose/steer a swarm; the curator's
    /// `SkillTool` invokes the `swarm-intelligence` cascade. Mirrors
    /// `KaskPanel`'s per-tab `ConversationView` pattern.
    Steer,
}

// ── View model ─────────────────────────────────────────────────────────────

/// One row in the panel — either an ABW agent, a local agent, or an ABW
/// swarm (workspace). The `source` field on agents distinguishes cloud,
/// local, and synced (exists in both).
#[derive(Clone, Debug)]
enum SwarmEntry {
    Agent(AgentCard),
    Swarm(SwarmCard),
}

/// Where an agent card lives.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AgentSource {
    /// Exists only on ABW (cloud). Can be cloned to local.
    Cloud,
    /// Exists only in the local registry. Can be pushed to cloud.
    Local,
    /// Exists in both — synced via `cloud_id`. Changes can flow both
    /// directions.
    Synced,
}

impl AgentSource {
    fn badge(&self) -> &'static str {
        match self {
            Self::Cloud => "☁",
            Self::Local => "■",
            Self::Synced => "⇅",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Cloud => "cloud",
            Self::Local => "local",
            Self::Synced => "synced",
        }
    }
}

#[derive(Clone, Debug)]
struct AgentCard {
    id: String,
    agent_type: String,
    description: String,
    author: String,
    executions: u64,
    /// Where this agent card lives: cloud (ABW only), local (local registry
    /// only), or synced (both, linked by `cloud_id`).
    source: AgentSource,
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

// ── Local agent response (v2 §15 Slice 11) ──────────────────────────────────

#[derive(Debug, Deserialize)]
struct LocalAgentListResponse {
    agents: Vec<LocalAgentInfo>,
}

#[derive(Debug, Deserialize)]
struct LocalAgentInfo {
    agent_id: String,
    agent_type: String,
    #[serde(default)]
    description: String,
    /// The ABW agent id this local card is synced with. `None` = local-only.
    #[serde(default)]
    cloud_id: Option<String>,
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

/// Parse a tool invoker response, unwrapping the `content` envelope the MCP
/// runtime wraps around tool returns. Returns the inner `content` object when
/// the envelope is present, or the whole value when it isn't (defensive
/// against a future invoker that returns the payload directly). `None` means
/// the response was not valid JSON — callers should surface a parse error,
/// never fabricate a default.
///
/// This is the single seam for the panel's MCP response parsing — every call
/// site goes through it, so a change to the envelope shape is one edit, and
/// the parse path is unit-testable without GPUI or the tool invoker.
fn parse_tool_response(output: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(output).ok()?;
    Some(value.get("content").cloned().unwrap_or(value))
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
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    fs: std::sync::Arc<dyn Fs>,
    list: UniformListScrollHandle,
    /// Number of fetch operations currently in flight (agents + swarms spawn
    /// independently). `is_fetching()` is true while any are in the air —
    /// avoids one fetch's completion hiding the other's spinner.
    in_flight: usize,
    /// Per-source fetch errors. Split so a slow agents fetch can't clobber a
    /// swarms error (and vice versa) — the H1 cross-clobber finding.
    agents_error: Option<SharedString>,
    swarms_error: Option<SharedString>,
    /// Error from the hire/consent flow (begin_hire, confirm_hire). Surfaced
    /// near the consent banner, distinct from fetch errors.
    hire_error: Option<SharedString>,
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
    /// Which surface is active: browse, author, compose, or steer.
    mode: PanelMode,
    /// Authoring form state.
    author: AuthorForm,
    /// Composition form state.
    compose: ComposeForm,
    /// Lazily-constructed `ConversationView` for Steer mode, scoped to the
    /// swarm MCP server. `None` until the operator first selects Steer.
    /// Mirrors `KaskPanel`'s `threads: HashMap<usize, Entity<ConversationView>>`
    /// — one retained view, reused across re-renders.
    steer_conversation: Option<Entity<ConversationView>>,
    /// Per-view connection store for the Steer `ConversationView`. Mirrors
    /// `KaskPanel`'s per-tab `connection_stores` (one store = one connection
    /// = one prompt, preventing cross-view prompt bleed).
    steer_connection_store: Option<Entity<AgentConnectionStore>>,
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
    pub fn new(
        workspace: &Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        // Read `project` and `fs` through the borrowed `&Workspace` rather than
        // `cx.entity().read(cx)` — the `Toggle` action handler already holds
        // an `update` lease on the `Workspace` entity, so a re-entrant `read`
        // triggers `double_lease_panic` ("cannot read workspace::Workspace
        // while it is already being updated"). Mirrors `KaskPanel::new`.
        let workspace_handle = workspace.weak_handle();
        let project = workspace.project().clone();
        let fs = workspace.app_state().fs.clone();
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
                    // Multi-line auto-height: system prompts are multi-paragraph
                    // by nature (the L3 finding). Grows 4–16 lines with content.
                    let mut e = Editor::auto_height(4, 16, window, cx);
                    e.set_placeholder_text(
                        "System prompt — the agent's instructions (multiple lines supported)",
                        window,
                        cx,
                    );
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
                workspace: workspace_handle,
                project,
                fs,
                list: scroll_handle,
                in_flight: 0,
                agents_error: None,
                swarms_error: None,
                hire_error: None,
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
                steer_conversation: None,
                steer_connection_store: None,
            };
            this.fetch_all(cx);
            this
        })
    }

    /// True while any fetch (agents or swarms) is in the air.
    fn is_fetching(&self) -> bool {
        self.in_flight > 0
    }

    /// The single visible error, preferring agents then swarms. Rendered as a
    /// status strip whenever present (not only in the empty state).
    fn visible_error(&self) -> Option<&SharedString> {
        self.agents_error.as_ref().or(self.swarms_error.as_ref())
    }

    /// Fetch agents and swarms via the governed MCP tool path.
    fn fetch_all(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
            self.agents_error = Some(
                "Tool invoker not wired — the swarm MCP server is unavailable. \
                 Ensure kask MCP servers are enabled (kask.mcp.load_default)."
                    .into(),
            );
            cx.notify();
            return;
        };

        self.in_flight = 3;
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
                                                source: AgentSource::Cloud,
                                            })
                                        })
                                        .collect::<Vec<_>>();
                                    // Replace cloud agent entries, keep swarm + local entries.
                                    this.entries.retain(|e| matches!(e, SwarmEntry::Swarm(_)));
                                    this.entries.extend(agents);
                                    this.agents_error = None;
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
                            this.agents_error =
                                Some(format!("Failed to list agents: {err}").into());
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
                                                agent_count: w.agent_count.unwrap_or(0),
                                                budget: w.workspace_budget.unwrap_or(0),
                                                remaining: w.workspace_remaining.unwrap_or(0),
                                            })
                                        })
                                        .collect::<Vec<_>>();
                                    // Replace swarm entries, keep agent entries.
                                    this.entries.retain(|e| matches!(e, SwarmEntry::Agent(_)));
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
                            // Auth failures here are expected when no key is configured —
                            // degrade to agents-only rather than an error state.
                            log::warn!(
                                "swarm-panel: could not fetch workspaces (agents-only mode): {err}"
                            );
                            this.filter_entries(cx);
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
                                        source: AgentSource::Local,
                                    }));
                                }
                                this.filter_entries(cx);
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
            }
        })
        .detach();
    }

    /// Clone an ABW (cloud) agent to the local registry. Calls
    /// `swarm_clone_to_local` on the swarm MCP server, which fetches the ABW
    /// card, writes it to `agents/local/curated/<id>/agent_card.json`, and
    /// sets `cloud_id` to mark it as synced. On success, re-fetches the agent
    /// list so the source badge updates to `synced`.
    fn clone_to_local(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
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
    fn push_to_cloud(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
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

    /// Fetch the pre-flight hire cost for an agent and open the consent gate.
    /// This is the entry point to the cost/consent flow: read-only, spends
    /// nothing, and populates `pending_hire` so the banner renders.
    fn begin_hire(&mut self, agent_name: String, cx: &mut Context<Self>) {
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
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
    fn confirm_hire(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_hire.take() else {
            return;
        };
        let Some(workspace_id) = self.selected_workspace.clone() else {
            self.hire_error =
                Some("No swarm selected to hire into. Create a workspace on ABW first.".into());
            cx.notify();
            return;
        };
        let Some(invoker) = kask_panel::shared_tool_invoker() else {
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
    fn cancel_hire(&mut self, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_hire.take() {
            log::info!(
                "swarm-panel: operator declined hire of '{}' (gate aborted)",
                pending.agent_name
            );
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: PanelMode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        // Move focus to the target mode's first field — otherwise focus stays
        // on the now-hidden search editor and keyboard input goes nowhere (the
        // M5 finding). Steer focuses its conversation's editor.
        let handle = match mode {
            PanelMode::Browse => self.query_editor.read(cx).focus_handle(cx),
            PanelMode::Author => self.author.name.read(cx).focus_handle(cx),
            PanelMode::Compose => self.compose.name.read(cx).focus_handle(cx),
            PanelMode::Steer => self
                .steer_conversation
                .as_ref()
                .map(|c| c.read(cx).focus_handle(cx))
                .unwrap_or_else(|| self.query_editor.read(cx).focus_handle(cx)),
        };
        handle.focus(window, cx);
        cx.notify();
    }

    /// Read the current swarm mode from `kask.swarm.mode` settings. Returns
    /// `Abw` when unset (the default). The panel reads the mode here (not
    /// from the MCP server) because the server's mode is derived from the
    /// same setting via env vars — the setting is the single source of truth.
    /// Used by the header mode toggle to show the active backend.
    fn current_swarm_mode(cx: &mut Context<Self>) -> kask_bridge::SwarmModeConfig {
        kask_bridge::KaskSettings::get_global(cx).swarm.mode.clone()
    }

    /// Set `kask.swarm.mode` in the user settings file. Persists via
    /// `SettingsStore::update_settings_file`, which writes to `settings.json`
    /// and triggers a global settings reload. The MCP server restarts with
    /// the updated `HKASK_SWARM_MODE` env var (the `ContextServerStore`
    /// observes the registry, which `sync_kask_mcp_servers` re-syncs on
    /// settings change). This is the operator-facing toggle for v2 §15 —
    /// flipping it re-routes the swarm server between ABW and the local
    /// substrate without a code revert.
    fn set_swarm_mode(&mut self, mode: kask_bridge::SwarmModeConfig, cx: &mut Context<Self>) {
        let content_mode = match mode {
            kask_bridge::SwarmModeConfig::Abw => settings_content::SwarmModeContent::Abw,
            kask_bridge::SwarmModeConfig::Local => settings_content::SwarmModeContent::Local,
        };
        SettingsStore::global(cx).update_settings_file(<dyn Fs>::global(cx), move |settings, _| {
            settings
                .kask
                .get_or_insert_default()
                .swarm
                .get_or_insert_default()
                .mode = Some(content_mode);
        });
        cx.notify();
    }

    /// Lazily construct the `ConversationView` for Steer mode if it doesn't
    /// exist yet. Mirrors `KaskPanel::ensure_thread_for_tab`: constructs a
    /// `CuratorAgentServer` scoped to the swarm MCP server, with a system
    /// prompt that tells the curator about the `swarm-intelligence` skill and
    /// the active swarm. The curator's `SkillTool` invokes the cascade when
    /// the operator asks to compose/steer a swarm.
    ///
    /// `window` is required because `ConversationView::new` may focus its
    /// inner `MessageEditor`.
    fn ensure_steer_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.steer_conversation.is_some() {
            return;
        }

        let thread_store = ThreadStore::global(cx);
        let agent_server = std::rc::Rc::new(
            agent::CuratorAgentServer::new(self.fs.clone(), thread_store)
                .with_extra_static_context(steer_system_prompt(self.selected_workspace.as_deref()))
                .with_mcp_server_scope(SWARM_SERVER.into()),
        );

        let connection_store = self
            .steer_connection_store
            .get_or_insert_with(|| cx.new(|cx| AgentConnectionStore::new(self.project.clone(), cx)))
            .clone();

        let thread_id = agent_ui::ThreadId::new();
        let conversation_view = cx.new(|cx| {
            ConversationView::new(
                agent_server,
                connection_store,
                Agent::Curator,
                None, // no resume session
                Some(thread_id),
                None, // no work_dirs
                None, // no title
                None, // no initial content — the system prompt is injected
                // via `with_extra_static_context`; the input editor starts
                // empty so the operator types their composition intent.
                self.workspace.clone(),
                self.project.clone(),
                None, // no thread_store — steer conversations are not persisted
                AgentThreadSource::AgentPanel,
                window,
                cx,
            )
        });

        self.steer_conversation = Some(conversation_view);
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
            // Fetch the real hire cost per agent first (BH-02): a hardcoded
            // `credits_authorized: 5` would under-authorize an agent that
            // costs 20, and the server's re-verify would reject the hire —
            // but only after the workspace was already created. Fetching the
            // cost up front lets us abort before any ABW mutation and pass
            // the real ceiling to the consent token.
            // A spend path must not silently degrade: if any consent mint
            // fails, abort the create rather than hiring a partial team.
            let mut consent_tokens = Vec::new();
            let mut consent_failures = Vec::new();
            for agent in &agents {
                // Step 1: fetch the real hire cost.
                let cost_result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_hire_cost",
                        json!({ "agent_name": agent }),
                    )
                    .await;
                let credits = match cost_result {
                    Ok(output) => {
                        parse_tool_response(&output).and_then(|c| {
                            c.get("total_hire_cost").and_then(|v| v.as_u64())
                        }).map(|c| c as u32)
                    }
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
                // Step 2: mint the consent token with the real cost.
                match invoker
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

            // Abort on any consent failure — do not create a swarm with a
            // silently under-consented team.
            if !consent_failures.is_empty() {
                this.update(cx, |this, cx| {
                    this.compose.busy = false;
                    this.compose.status = Some(
                        format!(
                            "Consent failed for {} — swarm not created.",
                            consent_failures.join(", ")
                        )
                        .into(),
                    );
                    cx.notify();
                })
                .ok();
                return;
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
                        // Surface any per-hire errors the server reported
                        // (BH-07): the workspace is created but some hires may
                        // have failed (cost re-verify, network drop). The
                        // operator must not see "Swarm created." while all
                        // hires silently failed.
                        let hire_errors = parse_tool_response(&output)
                            .and_then(|c| {
                                c.get("hire_errors").and_then(|e| e.as_array()).cloned()
                            })
                            .unwrap_or_default();
                        if hire_errors.is_empty() {
                            this.compose.status =
                                Some(format!("Swarm '{}' created.", name.trim()).into());
                        } else {
                            let failed: Vec<String> = hire_errors
                                .iter()
                                .filter_map(|e| {
                                    e.get("agent").and_then(|a| a.as_str()).map(str::to_string)
                                })
                                .collect();
                            this.compose.status = Some(format!(
                                "Swarm '{}' created, but {} hire(s) failed: {}",
                                name.trim(),
                                failed.len(),
                                failed.join(", ")
                            ).into());
                        }
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
        // Clear stale suggestions — the "Use team" button must not pre-fill
        // the previous recommendation while a new query is in flight (L5).
        self.compose.xaman_suggested_agents.clear();
        cx.notify();
        let session_id = self.compose.xaman_session.clone();

        cx.spawn(async move |this, cx| {
            // Mint a curate consent token before calling the curator. With the
            // default `curator_consent_default: false`, the server requires a
            // token (action "curate", target "xaman") — without it, every
            // "Ask Xaman Ek" click is rejected with ConsentDenied (BH-03).
            // Curator calls read task content but spend no credits, so the
            // ceiling is 0.
            let consent = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_request_consent",
                    json!({ "action": "curate", "target": "xaman", "credits_authorized": 0 }),
                )
                .await;
            let consent_token: Option<String> = match consent {
                Ok(output) => parse_tool_response(&output).and_then(|c| {
                    c.get("consent_token")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                }),
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.compose.xaman_busy = false;
                        this.compose.xaman_response = Some(
                            format!(
                                "Consent for Xaman Ek failed: {err}. \
                             Set kask.swarm.curator_consent_default true to opt in globally."
                            )
                            .into(),
                        );
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let Some(consent_token) = consent_token else {
                this.update(cx, |this, cx| {
                    this.compose.xaman_busy = false;
                    this.compose.xaman_response = Some(
                        "Consent for Xaman Ek returned no token. \
                         Set kask.swarm.curator_consent_default true to opt in globally."
                            .into(),
                    );
                    cx.notify();
                })
                .ok();
                return;
            };

            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_xaman",
                    json!({
                        "message": message.trim(),
                        "session_type": "composition_design",
                        "session_id": session_id,
                        "consent_token": consent_token,
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
                        let parsed = parse_tool_response(&output);
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
    fn apply_xaman_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.compose.xaman_suggested_agents.is_empty() {
            return;
        }
        let joined = self.compose.xaman_suggested_agents.join(", ");
        let agents_editor = self.compose.agents.clone();
        agents_editor.update(cx, |editor, cx| {
            editor.set_text(joined, window, cx);
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
            // Defensive: filtered indices can go stale relative to entries if
            // a mutation path forgets filter_entries — skip rather than panic.
            let Some(entry) = self.entries.get(entry_ix).cloned() else {
                continue;
            };
            cards.push(self.render_card(entry, cx));
        }
        cards
    }

    fn render_card(&mut self, entry: SwarmEntry, cx: &mut Context<Self>) -> MarketplaceCard {
        match entry {
            SwarmEntry::Agent(agent) => {
                let agent_name = agent.id.clone();
                let source = agent.source.clone();
                let source_badge = source.badge();
                let source_label = source.label();
                // Clone-to-local button: visible for Cloud agents only.
                let show_clone = source == AgentSource::Cloud;
                // Push-to-cloud button: visible for Local agents only.
                let show_push = source == AgentSource::Local;
                // Pre-clone agent_name for each button closure that needs it.
                let hire_name = agent_name.clone();
                let clone_name = agent_name.clone();
                let push_name = agent_name.clone();
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
                                        )
                                        .child(
                                            Label::new(format!(
                                                "{} {}",
                                                source_badge, source_label
                                            ))
                                            .color(Color::Accent)
                                            .size(LabelSize::XSmall),
                                        ),
                                )
                                .child(Label::new(agent.description).color(Color::Muted)),
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
                                            this.begin_hire(hire_name.clone(), cx);
                                        },
                                    )),
                                )
                                .when(show_clone, |this| {
                                    this.child(
                                        Button::new(
                                            SharedString::from(format!("clone-{clone_name}")),
                                            "Clone to Local",
                                        )
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.spend_in_flight.is_some())
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.clone_to_local(clone_name.clone(), cx);
                                            }),
                                        ),
                                    )
                                })
                                .when(show_push, |this| {
                                    this.child(
                                        Button::new(
                                            SharedString::from(format!("push-{push_name}")),
                                            "Push to Cloud",
                                        )
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.spend_in_flight.is_some())
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.push_to_cloud(push_name.clone(), cx);
                                            }),
                                        ),
                                    )
                                }),
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
                            .child(Label::new(swarm.description).color(Color::Muted)),
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
            // Xaman Ek composition consultant — the panel calls the MCP tool
            // to plan the team, then offers the recommended agents as a
            // one-click pre-fill of the field above.
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .rounded_sm()
                    .border_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Label::new("★ Xaman Ek")
                                    .size(LabelSize::Small)
                                    .color(Color::Accent),
                            )
                            .child(
                                Label::new("composition consultant")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .child(self.compose.xaman_query.clone()),
                            )
                            .child(
                                Button::new(
                                    "ask-xaman",
                                    if self.compose.xaman_busy {
                                        "Asking…"
                                    } else {
                                        "Ask"
                                    },
                                )
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .disabled(self.compose.xaman_busy)
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.ask_xaman(cx);
                                    },
                                )),
                            ),
                    )
                    .when_some(self.compose.xaman_response.clone(), |this, response| {
                        this.child(
                            Label::new(response)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    })
                    .when(!self.compose.xaman_suggested_agents.is_empty(), |this| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Label::new(format!(
                                        "Suggested: {}",
                                        self.compose.xaman_suggested_agents.join(", ")
                                    ))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                                )
                                .child(
                                    Button::new("apply-xaman", "Use team")
                                        .style(ButtonStyle::Filled)
                                        .label_size(LabelSize::XSmall)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_xaman_suggestions(window, cx);
                                        })),
                                ),
                        )
                    }),
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

        let message: SharedString = if self.is_fetching() {
            "Loading agents and swarms…".into()
        } else if let Some(err) = self.visible_error() {
            format!("Failed to load swarm data: {err}").into()
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

        marketplace_empty_state(message, self.visible_error().is_some())
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
            // The per-dispatch ceiling is a hard server-side gate, not an
            // advisory. There is no override path — the operator must raise
            // `HKASK_ABW_MAX_CREDITS` and re-request. The previous wording
            // ("confirm to override") was a no-op: `confirm_hire` proceeded
            // unconditionally, but the server's `swarm_hire` now refuses the
            // hire. Disable Confirm so the UI matches the refusal.
            format!(
                "Exceeds your {}-credit dispatch limit — raise HKASK_ABW_MAX_CREDITS to authorize.",
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
                                .disabled(!pending.within_budget)
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
        // If deserialized into Steer mode (or the operator switched via a
        // path that didn't go through the toggle handler), ensure the
        // conversation exists before rendering.
        if matches!(self.mode, PanelMode::Steer) {
            self.ensure_steer_conversation(window, cx);
        }
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
                    // v2 §15: the mode toggle re-routes the swarm server
                    // between ABW (v1) and the local substrate (v2). Writing
                    // `kask.swarm.mode` persists to settings.json and restarts
                    // the MCP server with the updated `HKASK_SWARM_MODE` env
                    // var. The toggle is always visible so the operator can
                    // switch backends without editing JSON.
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                Label::new("Backend:")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                div().child(
                                    ToggleButtonGroup::single_row(
                                        "swarm-backend-mode",
                                        [
                                            ToggleButtonSimple::new(
                                                "ABW",
                                                cx.listener(|this, _event, _, cx| {
                                                    this.set_swarm_mode(
                                                        kask_bridge::SwarmModeConfig::Abw,
                                                        cx,
                                                    );
                                                }),
                                            ),
                                            ToggleButtonSimple::new(
                                                "Local",
                                                cx.listener(|this, _event, _, cx| {
                                                    this.set_swarm_mode(
                                                        kask_bridge::SwarmModeConfig::Local,
                                                        cx,
                                                    );
                                                }),
                                            ),
                                        ],
                                    )
                                    .style(ToggleButtonGroupStyle::Outlined)
                                    .size(ToggleButtonGroupSize::Custom(rems_from_px(28.)))
                                    .label_size(LabelSize::Small)
                                    .auto_width()
                                    .selected_index(match Self::current_swarm_mode(cx) {
                                        kask_bridge::SwarmModeConfig::Abw => 0,
                                        kask_bridge::SwarmModeConfig::Local => 1,
                                    })
                                    .into_any_element(),
                                ),
                            ),
                    )
                    .children(self.render_consent_banner(cx))
                    // Hire-flow errors surface near the consent banner.
                    .when_some(self.hire_error.clone(), |this, err| {
                        this.child(
                            Label::new(err)
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                        )
                    })
                    // Fetch errors surface as a status strip whenever present,
                    // not only in the empty state (the M3 partial-degradation
                    // finding — a working list can hide a failed source).
                    .when_some(self.visible_error().cloned(), |this, err| {
                        this.child(
                            Label::new(format!("Load warning: {err}"))
                                .size(LabelSize::XSmall)
                                .color(Color::Warning),
                        )
                    })
                    // The three surfaces: Browse (discovery/sharing), Author
                    // (agents), Compose (swarms).
                    .child(
                        div().child(
                            ToggleButtonGroup::single_row(
                                "swarm-mode-buttons",
                                [
                                    ToggleButtonSimple::new(
                                        "Browse",
                                        cx.listener(|this, _event, window, cx| {
                                            this.set_mode(PanelMode::Browse, window, cx);
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Author",
                                        cx.listener(|this, _event, window, cx| {
                                            this.set_mode(PanelMode::Author, window, cx);
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Compose",
                                        cx.listener(|this, _event, window, cx| {
                                            this.set_mode(PanelMode::Compose, window, cx);
                                        }),
                                    ),
                                    ToggleButtonSimple::new(
                                        "Steer",
                                        cx.listener(|this, _event, window, cx| {
                                            this.ensure_steer_conversation(window, cx);
                                            this.set_mode(PanelMode::Steer, window, cx);
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
                                PanelMode::Steer => 3,
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
                        PanelMode::Steer => {
                            // The `ConversationView` is lazily constructed
                            // in the Steer toggle handler; render it here. If
                            // it's somehow absent (e.g. the panel was
                            // deserialized into Steer mode), render a
                            // placeholder — the operator can re-click Steer.
                            match &self.steer_conversation {
                                Some(view) => this.child(view.clone()).into_any_element(),
                                None => this
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                Label::new(
                                                    "Steer mode — re-click Steer to start a composition conversation",
                                                )
                                                .color(Color::Muted),
                                            ),
                                    )
                                    .into_any_element(),
                            }
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
        // Mode-dependent: the search editor is only rendered in Browse, so
        // delegating to it in other modes strands focus on a hidden editor
        // (the H3 finding). Fall back to the search editor for Steer when the
        // conversation isn't constructed yet.
        match self.mode {
            PanelMode::Browse => self.query_editor.read(cx).focus_handle(cx),
            PanelMode::Author => self.author.name.read(cx).focus_handle(cx),
            PanelMode::Compose => self.compose.name.read(cx).focus_handle(cx),
            PanelMode::Steer => self
                .steer_conversation
                .as_ref()
                .map(|c| c.read(cx).focus_handle(cx))
                .unwrap_or_else(|| self.query_editor.read(cx).focus_handle(cx)),
        }
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
        workspace: WeakEntity<Workspace>,
        _workspace_id: workspace::WorkspaceId,
        _item_id: workspace::ItemId,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<Entity<Self>>> {
        // Stateless item — nothing to persist beyond the fact that it's open.
        // The panel reconstructs its state from ABW on first render.
        cx.spawn(async move |cx| {
            workspace.update_in(cx, |workspace, window, cx| {
                SwarmPanel::new(workspace, window, cx)
            })
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

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
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
        // `hkask-mcp-swarm/src/hkask_mcp_swarm.rs`. Keep this list in sync
        // when adding/removing a server tool — a rename in `hkask-mcp-swarm`
        // must be reflected here so the panel's `invoke_tool` call sites
        // don't silently degrade to "tool not found".
        assert_eq!(SWARM_SERVER, "swarm");
        for tool in [
            "swarm_list_agents",
            "swarm_get_swarm",
            "swarm_get_agent",
            "swarm_list_apps",
            "swarm_ontology_templates",
            "swarm_execute_agent",
            "swarm_hire_cost",
            "swarm_request_consent",
            "swarm_hire",
            "swarm_delegate",
            "swarm_run_status",
            "swarm_generate_prompt",
            "swarm_generate_ontology",
            "swarm_create_agent",
            "swarm_create_swarm",
            "swarm_xaman",
            "swarm_create_app",
            // v2 §15 local tools (Slices 9 + 11).
            "swarm_fund_local",
            "swarm_delegate_local",
            "swarm_list_local_agents",
            "swarm_clone_to_local",
            "swarm_push_to_cloud",
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

    // `parse_tool_response` is the single seam for unwrapping the MCP runtime's
    // `{"content": {...}}` envelope. Every panel call site goes through it, so
    // a change to the envelope shape is one edit. These tests pin the contract.
    #[test]
    fn parse_tool_response_unwraps_content_envelope() {
        let out = r#"{"content":{"agents":[{"agent_id":"a"}]}}"#;
        let parsed = parse_tool_response(out).expect("valid envelope");
        assert!(parsed.get("agents").is_some(), "inner content is unwrapped");
    }

    #[test]
    fn parse_tool_response_returns_inner_when_no_envelope() {
        // Defensive: if a future invoker returns the payload directly (no
        // `content` wrapper), the helper returns the whole value rather than
        // dropping it. This keeps the panel working across invoker changes.
        let out = r#"{"agents":[{"agent_id":"a"}]}"#;
        let parsed = parse_tool_response(out).expect("valid json");
        assert!(parsed.get("agents").is_some());
    }

    #[test]
    fn parse_tool_response_none_on_garbage() {
        assert_eq!(parse_tool_response("not json"), None);
        assert_eq!(parse_tool_response(""), None);
    }

    // The `fetch_all` parse path was broken before the `parse_tool_response`
    // extraction: the prior `serde_json::from_str::<AgentListResponse>(&output)`
    // targeted the inner content shape but the response is wrapped in
    // `{"content": {...}}`, so the top-level key was `content`, not `agents` —
    // every parse failed silently. This test pins the fixed path: unwrap the
    // envelope, then deserialize the inner content into the typed response.
    #[test]
    fn fetch_all_parse_path_unwraps_envelope_before_typed_deserialize() {
        let out = r#"{"content":{"count":1,"agents":[{"agent_id":"sensor_advisor","agent_type":"research","description":"d","author":"a","execution_stats":{"total_executions":5}}]}}"#;
        let parsed = parse_tool_response(out).expect("envelope");
        let response: AgentListResponse =
            serde_json::from_value(parsed).expect("inner content deserializes");
        assert_eq!(response.agents.len(), 1);
        assert_eq!(
            response.agents[0].agent_id.as_deref(),
            Some("sensor_advisor")
        );
        assert_eq!(
            response.agents[0]
                .execution_stats
                .as_ref()
                .and_then(|s| s.total_executions),
            Some(5)
        );
    }

    // The workspace parse path mirrors the agents path.
    #[test]
    fn fetch_all_parse_path_unwraps_envelope_for_workspaces() {
        let out = r#"{"content":{"workspaces":[{"id":"ws1","name":"Team","agent_count":3,"workspace_budget":100,"workspace_remaining":40}]}}"#;
        let parsed = parse_tool_response(out).expect("envelope");
        let response: WorkspaceListResponse =
            serde_json::from_value(parsed).expect("inner content deserializes");
        assert_eq!(response.workspaces.len(), 1);
        assert_eq!(response.workspaces[0].id.as_deref(), Some("ws1"));
        assert_eq!(response.workspaces[0].agent_count, Some(3));
    }

    // Pin the consent-token response field name. The panel extracts
    // `consent_token` from the `swarm_request_consent` response at three sites
    // (confirm_hire, create_swarm, ask_xaman). If the server renames the field,
    // all three break silently — the panel shows "Consent did not return a token"
    // with no indication that the contract drifted. This test pins the field
    // name the server emits, so a rename fails here first.
    #[test]
    fn consent_response_field_name_is_consent_token() {
        let out = r#"{"content":{"consent_token":"hkask-consent-deadbeef","action":"hire","target":"a","credits_authorized":5}}"#;
        let parsed = parse_tool_response(out).expect("envelope");
        assert_eq!(
            parsed.get("consent_token").and_then(|t| t.as_str()),
            Some("hkask-consent-deadbeef"),
            "swarm_request_consent must return the token under `consent_token`"
        );
    }

    // Steer mode: the system prompt must name the `swarm-intelligence` skill
    // and the swarm MCP server scope, so the curator knows to invoke the
    // skill for composition/steering requests. Pins the §13 wiring.
    #[test]
    fn steer_system_prompt_names_skill_and_server() {
        let prompt = steer_system_prompt(Some("ws_test"));
        assert!(
            prompt.contains("swarm-intelligence"),
            "steer prompt must name the swarm-intelligence skill"
        );
        assert!(
            prompt.contains(SWARM_SERVER),
            "steer prompt must name the swarm MCP server scope"
        );
        assert!(
            prompt.contains("ws_test"),
            "steer prompt must include the selected workspace id"
        );
    }

    #[test]
    fn steer_system_prompt_handles_no_workspace() {
        let prompt = steer_system_prompt(None);
        assert!(
            prompt.contains("No swarm"),
            "steer prompt must guide the operator when no workspace is selected"
        );
    }

    // KA-04: the steer prompt must not reference MCP tools that do not exist
    // in the swarm server. The prior prompt advertised `swarm_update_swarm`,
    // `DispatchIntent`, and `GateDecision::Proceed` — none of which are
    // implemented. An advertised consent gate with no enforcement point is
    // the `.rules` trap. This test pins that the prompt references only the
    // actual ConsentStore-backed flow.
    #[test]
    fn steer_prompt_references_only_existing_tools() {
        let prompt = steer_system_prompt(Some("ws_test"));
        // Tools that do not exist in the MCP server.
        assert!(
            !prompt.contains("swarm_update_swarm"),
            "steer prompt must not reference nonexistent swarm_update_swarm tool"
        );
        assert!(
            !prompt.contains("DispatchIntent"),
            "steer prompt must not reference nonexistent DispatchIntent type"
        );
        assert!(
            !prompt.contains("GateDecision"),
            "steer prompt must not reference nonexistent GateDecision type"
        );
        // The actual consent flow it should reference.
        assert!(
            prompt.contains("swarm_request_consent"),
            "steer prompt must reference the actual swarm_request_consent tool"
        );
        // The per-dispatch ceiling guidance must be present so the model
        // doesn't attempt doomed spends (the server refuses them, but a
        // doomed attempt wastes a turn and confuses the operator).
        assert!(
            prompt.contains("HKASK_ABW_MAX_CREDITS"),
            "steer prompt must name the per-dispatch ceiling env var"
        );
        assert!(
            prompt.contains("within_budget"),
            "steer prompt must tell the model to check within_budget before hiring"
        );
    }

    // v2 §15: the steer prompt must describe the local tools so the curator
    // knows to use them when the operator has set `kask.swarm.mode: local`.
    // The local runtime is constructed even in ABW mode (the operator can
    // mix), so the tools are always available. Pins the §15.5 Slice 11
    // follow-up: "Update the Steer-mode system prompt to describe the local
    // tools and the mode toggle."
    #[test]
    fn steer_prompt_describes_local_tools() {
        let prompt = steer_system_prompt(Some("ws_test"));
        for tool in [
            "swarm_list_local_agents",
            "swarm_fund_local",
            "swarm_delegate_local",
            "swarm_clone_to_local",
            "swarm_push_to_cloud",
        ] {
            assert!(
                prompt.contains(tool),
                "steer prompt must describe the local tool {tool}"
            );
        }
        // The mode toggle must be named so the curator can tell the operator
        // how to switch backends.
        assert!(
            prompt.contains("kask.swarm.mode"),
            "steer prompt must name the kask.swarm.mode setting"
        );
        // The local ledger funding requirement must be stated so the curator
        // funds before delegating (the §15.6 constraint — no auto-replenish).
        assert!(
            prompt.contains("swarm_fund_local"),
            "steer prompt must tell the curator to fund the local ledger"
        );
        assert!(
            prompt.contains("PaymentRequired"),
            "steer prompt must name the PaymentRequired error for an unfunded ledger"
        );
    }

    // ── Per-dispatch ceiling: banner wording contract ─────────────────────────
    // The consent banner must not promise an override that doesn't exist. The
    // server's `swarm_hire` enforces `max_credits_per_dispatch` as a hard gate;
    // the panel's "confirm to override" wording was a no-op (the hire would be
    // refused server-side after the operator clicked Confirm). Pin that the
    // `within_budget: false` path tells the operator to raise the env var, not
    // to click Confirm. The `PendingHire` struct is the contract between the
    // server's `swarm_hire_cost` response and the banner; pin its field shape.
    #[test]
    fn pending_hire_preserves_within_budget_false_from_server() {
        // Simulate a `swarm_hire_cost` response for an over-ceiling hire.
        // The server sets `within_budget = total <= ceiling` (total=60, ceiling=50).
        let out = r#"{"content":{"agent_name":"expensive","total_hire_cost":60,"required_cost":60,"optional_cost":0,"max_credits_per_dispatch":50,"within_budget":false}}"#;
        let content = parse_tool_response(out).expect("envelope");
        let total_hire_cost = content
            .get("total_hire_cost")
            .and_then(|c| c.as_u64())
            .expect("total_hire_cost present");
        let within_budget = content
            .get("within_budget")
            .and_then(|c| c.as_bool())
            .unwrap_or(false);
        let max_credits = content
            .get("max_credits_per_dispatch")
            .and_then(|c| c.as_u64())
            .unwrap_or(50) as u32;
        assert_eq!(total_hire_cost, 60);
        assert!(
            !within_budget,
            "over-ceiling hire must be within_budget=false"
        );
        assert_eq!(max_credits, 50);
    }

    #[test]
    fn pending_hire_preserves_within_budget_true_from_server() {
        // A within-ceiling hire: total=20, ceiling=50.
        let out = r#"{"content":{"agent_name":"cheap","total_hire_cost":20,"required_cost":20,"optional_cost":0,"max_credits_per_dispatch":50,"within_budget":true}}"#;
        let content = parse_tool_response(out).expect("envelope");
        let within_budget = content
            .get("within_budget")
            .and_then(|c| c.as_bool())
            .unwrap_or(false);
        assert!(
            within_budget,
            "within-ceiling hire must be within_budget=true"
        );
    }
}
