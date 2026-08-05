#![forbid(unsafe_code)]
//! Swarm Panel — a center-pane `Item` listing Agent Bestiary World agents and
//! swarms (workspaces) as cards, mirroring the Kask Extensions panel layout.
//!
//! Entities are **agents** (from the ABW catalogue) and **swarms** (the
//! operator's workspaces), not skills. Data is fetched through the global
//! `ToolInvoker` hook (the governed, OCAP-gated MCP runtime path), so all
//! ABW calls flow through `hkask-mcp-swarm` and the kask MCP runtime rather
//! than ad-hoc HTTP from the UI.
//!
//! Layout mirrors `KaskExtensionsPage`: headline, search bar, filter toggle
//! (All / Swarms / Agents), a uniform list of `MarketplaceCard`s, and an
//! empty state that surfaces fetch errors. The panel wires lifecycle actions
//! to `invoke_tool` calls: hire (cost/consent-gated), clone-to-local,
//! push-to-cloud, remove-local, fire, delete, and publish (fermi v0.10.15 —
//! preflight via `swarm_publish_checks`, then `swarm_publish_agent`, with an
//! audited admin force-publish path). Spend actions are gated behind the
//! cost/consent gate (see `kask/docs/plans/abw-swarm-intelligence.md` §3.6).
//!
//! **Steer mode** hosts a `ConversationView` scoped to the swarm MCP server.
//! The operator asks the curator to compose/steer a swarm; the curator's
//! `SkillTool` invokes the `swarm-intelligence` cascade (see
//! `kask/docs/plans/abw-swarm-intelligence.md` §13). The conversation is not
//! persisted — re-clicking Steer after a restart starts a fresh composition
//! conversation.

mod author;
mod card;
mod compose;
mod detail;
pub mod panel_button;
mod parse;
mod tool_invoker;

use author::AuthorForm;
use compose::ComposeForm;

pub use panel_button::SwarmPanelButton;
pub use tool_invoker::{ToolInvoker, set_tool_invoker, shared_tool_invoker};

use parse::{
    AgentCard, AgentListResponse, AgentSource, LocalAgentListResponse, LocalSwarmListResponse,
    SwarmCard, WorkspaceListResponse, extract_agent_mentions, extract_wallet_balance,
    parse_publish_checks, parse_run_status_messages, parse_swarm_roster,
};

use std::ops::Range;
use std::time::Duration;

use anyhow::Result;
use editor::Editor;
use fs::Fs;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, ReadGlobal as _, Render, Task,
    UniformListScrollHandle, WeakEntity, Window, actions, uniform_list,
};
use hkask_types::tool_response::parse_tool_response;
use marketplace_ui_common::{MarketplaceCard, marketplace_empty_state, marketplace_search_bar};
use project::Project;
use serde_json::json;
use settings::Settings as _;
use settings::SettingsStore;
use ui::{
    ScrollableHandle, ToggleButtonGroup, ToggleButtonGroupSize, ToggleButtonGroupStyle,
    ToggleButtonSimple, Tooltip, WithScrollbar, prelude::*,
};
use workspace::{
    Workspace,
    item::{Item, ItemEvent, SerializableItem},
    register_serializable_item,
};

// Steer mode: a `ConversationView` scoped to the swarm MCP server. The
// curator's `SkillTool` invokes the `swarm-intelligence` cascade when the
// operator asks to compose/steer a swarm. See
// `kask/docs/plans/abw-swarm-intelligence.md` §13.
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
/// context (active workspace, current backend mode, the skill's purpose).
/// Build the Steer-mode system prompt for the curator agent.
///
/// Design tradeoff (R7): the `mode` variable reaches the `swarm-intelligence`
/// skill cascade via the curator's `context` argument, which is prompt-level
/// instruction — not a hard-enforced input. The manifest defaults `mode` to
/// `'abw'` when the context lacks it (`{{ mode | default('abw') }}`). A
/// prompt-injected curator could omit `mode` to force ABW, or pass
/// `mode: "local"` to switch backends. This is a wrong-result risk, not a
/// security violation: both backends have their own spending gates (consent
/// tokens for ABW, ledger balance for local), so a wrong-mode cascade cannot
/// bypass spending controls. Hard enforcement (declaring `mode` as a required
/// manifest input) would change the `hkask-templates` schema and break
/// existing callers. The prompt instruction is the pragmatic tradeoff.
fn steer_system_prompt(
    selected_workspace: Option<&str>,
    mode: kask_bridge::SwarmModeConfig,
) -> SharedString {
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
    let workspace_context = selected_workspace
        .map(|id| format!("\"swarm_id\": \"{id}\""))
        .unwrap_or_else(|| "\"swarm_id\": \"\"".to_string());
    let prompt = format!(
        "## Agent Swarm Panel — Steer Mode
         \n\
         You are operating in the Agent Swarm panel's Steer mode, scoped to the \
         `{SWARM_SERVER}` MCP server. The swarm server exposes two tool sets, \
         selected by the operator via the `kask.swarm.mode` setting (`abw` or \
         `local`):\n\
         \n\
         **ABW tools** (`mode: abw`, the default): `swarm_list_agents`, \
         `swarm_get_swarm`, `swarm_hire_cost`, `swarm_request_consent`, \
         `swarm_authorize_session` (pre-authorized spend for headless \
         pipelines), `swarm_hire`, `swarm_delegate`, \
         `swarm_delegate_and_wait` (delegate + poll for response), \
         `swarm_fanout` (parallel multi-agent fan-out), `swarm_fire` (remove \
         from roster), `swarm_create_agent`, `swarm_create_swarm`, \
         `swarm_generate_prompt`, `swarm_generate_ontology`, \
         `swarm_fork_agent` (derivative fork), `swarm_run_status`, \
         `swarm_search_knowledge` (vector knowledge-graph search), \
         `swarm_publish_checks` + `swarm_publish_agent` (catalogue publish, \
         with an audited admin force-publish path), `swarm_xaman`. These \
         route to Agent Bestiary World and require the ABW API key.\n\
         \n\
         **Local tools** (`mode: local`): `swarm_list_local_agents`, \
         `swarm_balance_local`, `swarm_local_history`, `swarm_fund_local`, \
         `swarm_delegate_local`, `swarm_fanout_local`, \
         `swarm_pipeline_local` (sequential pipeline with {{prev_output}} \
         substitution), `swarm_clone_to_local`, `swarm_remove_local`, \
         `swarm_create_local_agent`, `swarm_reconfigure_local_agent`,
         `swarm_push_to_cloud`, `swarm_search_knowledge_local` (search the
         agent's prefix-scoped `hkask-memory` semantic graph — the local analog
         of `swarm_search_knowledge`), `swarm_generate_prompt_local` /
         `swarm_generate_ontology_local` (local LLM authoring aids over the
         local `InferencePort`, seeded with the agent's memory — the local
         analogs of `swarm_generate_prompt` / `swarm_generate_ontology`). These run on the local \
         substrate (`hkask-inference` + `hkask-ledger` + `hkask-guard`) with no \
         ABW round-trips. The local ledger is operator-funded — call \
         `swarm_fund_local(credits)` before `swarm_delegate_local`, or it returns \
         `PaymentRequired`. There is no consent token in local mode: the balance \
         check is the gate. `swarm_clone_to_local` and `swarm_push_to_cloud` sync \
         cards between the local registry (`agents/local/curated/<id>/agent_card.json`) \
         and ABW; a cloned card carries `cloud_id` to track the sync link. \
         `swarm_remove_local` deletes a local card (the local counterpart of \
         firing — a synced card's ABW agent is untouched); `swarm_local_history` \
         reads the local ledger's recent transactions (the run/reconciliation \
         surface in local mode).\n\
         \n\
         {workspace_note}\n\
         \n\
         The current backend (`kask.swarm.mode`) is **`{mode}`**.\n\
         \n\
         The `swarm-intelligence` skill is available for swarm composition and \
         steering in both modes. When the operator asks to compose, configure, \
         tune, or steer a swarm toward a target condition, invoke the \
         `swarm-intelligence` skill with the operator's task. Pass the current \
         backend and workspace in the skill's `context` argument so the \
         cascade selects the right data source and gate:\n\
         \
         ```json\n\
         {{\"mode\": \"{mode}\", {workspace_context}}}\n\
         ```\n\
         \
         The skill runs a SENSE → ORIENT → DECIDE → ACT → CHECK → CONVERGE \
         loop and branches on `{{{{ mode }}}}` (local vs abw) at the \
         SENSE/ACT/CHECK steps. Without `mode` in the context, the templates \
         default to `abw` — the skill would steer the ABW backend even when \
         `kask.swarm.mode` is `local`. When invoking via a slash command\n\
         (`/swarm-intelligence ...`), pass context as leading `key=value` pairs\n\
         before the task text — e.g. `/swarm-intelligence mode=local swarm_id=ws-1\n\
         compose my swarm` sets mode, swarm_id, and task.\n\
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
         `swarm_delegate_local` in local mode.\n\
         \n\
         The skill accepts an optional `task_success` context field — a\n\
         deterministic evaluator's verdict on whether the swarm's last output\n\
         solved the task (test pass/fail, schema validation, exit code,\n\
         regex/reference match). When the operator's task has a deterministic\n\
         oracle, pass `task_success` in the skill's `context` as\n\
         an object whose `pass` field is true or false (and a\n\
         `detail` string if useful). When the task is open-ended with no\n\
         oracle, OMIT `task_success` — the skill falls back to the three\n\
         swarm-health axes and the human Go See loop covers the gap. Do NOT\n\
         use an LLM to score the output as `task_success`; the judge must be\n\
         deterministic (the cybernetic plan's determinism constraint).\n\
         \n\
         Cybernetic Swarm Plan — second-order monitor + Go See (C1/C2). The\n\
         skill's CONVERGE runs a deterministic second-order monitor (C1) over\n\
         the iteration log: it flags reasoning loops (same deficit+action\n\
         repeating with no d improvement) and sensor-truth divergence (d\n\
         improving while s declines — the swarm looks healthier but fails more\n\
         tasks). When the monitor recommends go_see, surface a Go See\n\
         directive (C2): the operator should descend this Steer conversation\n\
         with the section 5 checklist — is s filtering task-failure truth, are\n\
         .rules priors still verified against the codebase, are these Steer\n\
         guides having the intended effect. DECIDE also applies a failed-edit\n\
         memory (C3), per-agent-type influence guards (C7), and a\n\
         reconfigure_agent action (C6) via swarm_reconfigure_local_agent when\n\
         ORIENT attributes fault (C5).\n"
    );
    debug_assert!(
        prompt
            .split('`')
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .all(|(_, seg)| {
                let name: String = seg
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                !name.starts_with("swarm_")
                    || name.len() <= "swarm_".len()
                    || parse::SWARM_TOOLS.contains(&name.as_str())
            }),
        "steer_system_prompt advertises a swarm_* tool not in SWARM_TOOLS"
    );
    prompt.into()
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
    /// `SkillTool` invokes the `swarm-intelligence` cascade.
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
    _subscriptions: [gpui::Subscription; 2],
    search_task: Option<Task<()>>,
    /// Current ABW wallet balance (the algedonic channel). `None` = unknown
    /// (unauthenticated or the balance query failed) — never a fabricated zero.
    wallet_balance: Option<i64>,
    /// Current local ledger balance (v2 §15). `None` = unknown or the local
    /// runtime isn't initialized. Displayed in the header when the backend
    /// mode is `local`.
    local_balance: Option<i64>,
    /// In-flight consent prompt for a hire action: the agent being considered
    /// plus its pre-flight cost estimate. `Some` renders the consent banner.
    pending_hire: Option<PendingHire>,
    /// In-flight publish prompt (fermi v0.10.15). `Some` renders the publish
    /// banner — a Confirm path when `can_publish`, or a force-publish path with
    /// a reason input when checks fail.
    pending_publish: Option<PendingPublish>,
    /// Single-line editor for the force-publish reason (audited to
    /// `admin_bypass_events`). Only read when the operator confirms a force
    /// publish.
    publish_reason: Entity<Editor>,
    /// The workspace (swarm) id new hires target. Defaults to the first
    /// workspace once swarms load; selectable when there are several.
    selected_workspace: Option<String>,
    /// A spend currently in flight (after consent), shown as a busy state.
    spend_in_flight: Option<String>,
    /// The swarm roster drill-down (item 4). `Some` renders the detail view
    /// instead of the browse list.
    swarm_detail: Option<SwarmDetailView>,
    /// Single-line editor for the swarm-detail "add agent" affordance. Reads
    /// the agent id to add to the open swarm's roster. Only read when the
    /// detail view is open and the operator clicks Add.
    swarm_add_agent_editor: Entity<Editor>,
    /// The most recently requested swarm run status (item 3), rendered as a
    /// dismissible strip. `None` = no status shown.
    run_status: Option<RunStatusView>,
    /// Which surface is active: browse, author, compose, or steer.
    mode: PanelMode,
    /// Authoring form state.
    author: AuthorForm,
    /// Composition form state.
    compose: ComposeForm,
    /// Lazily-constructed `ConversationView` for Steer mode, scoped to the
    /// swarm MCP server. `None` until the operator first selects Steer.
    /// Uses the retained-view pattern (one `ConversationView`, reused across
    /// re-renders).
    steer_conversation: Option<Entity<ConversationView>>,
    /// Per-view connection store for the Steer `ConversationView`. One store =
    /// one connection = one prompt, preventing cross-view prompt bleed.
    steer_connection_store: Option<Entity<AgentConnectionStore>>,
    /// True while an AI Assist / validate call to `swarm_ai_assist` is in
    /// flight. Gates the AI Assist / Validate buttons and shows a busy label.
    ai_assist_busy: bool,
    /// The action ("suggest" / "validate") of the in-flight AI Assist call, so
    /// the busy label can distinguish "Assisting…" from "Validating…".
    ai_assist_action: Option<String>,
    /// The last AI Assist suggestion result (action: "suggest"). `Some`
    /// renders the suggestions banner with an Apply / Dismiss pair.
    ai_assist_suggestions: Option<AiSuggestions>,
    /// The last AI Assist validation verdict (action: "validate"). `Some`
    /// renders the validation banner (success or issues list) with Dismiss.
    validation_result: Option<ValidationResult>,
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

/// In-flight publish prompt (fermi v0.10.15). `swarm_publish_checks` returns
/// `can_publish` plus the failing checks; when `can_publish` is false the banner
/// shows the checks and a reason input for the admin force-publish path
/// (`?force=true&reason=…`, audited to `admin_bypass_events`).
#[derive(Clone, Debug)]
pub(crate) struct PendingPublish {
    agent_name: String,
    can_publish: bool,
    failing_checks: Vec<String>,
}

/// One agent row in a swarm's roster (drill-down view, item 4).
#[derive(Clone, Debug)]
pub(crate) struct SwarmRosterAgent {
    agent_id: String,
    agent_type: String,
    description: String,
}

/// The swarm roster drill-down: replaces the browse list while open.
#[derive(Clone, Debug)]
struct SwarmDetailView {
    workspace_id: String,
    name: String,
    /// The swarm's mission / description. Displayed read-only in the detail
    /// header — there is no MCP tool to rename a swarm or change its mission,
    /// so the panel surfaces it as context rather than an editable field.
    mission: String,
    /// Which substrate this swarm lives on. Drives the add/remove affordances:
    /// `Local` uses `swarm_add_agent_local` / `swarm_remove_agent_local` /
    /// `swarm_delete_local_swarm`; `Cloud` uses the consent-gated `swarm_hire`
    /// and `swarm_fire`.
    source: AgentSource,
    loading: bool,
    error: Option<SharedString>,
    agents: Vec<SwarmRosterAgent>,
}

/// A swarm's recent run status (ABW workspace messages). Rendered as a
/// dismissible strip above the browse list.
#[derive(Clone, Debug)]
struct RunStatusView {
    name: String,
    loading: bool,
    error: Option<SharedString>,
    /// Rendered message lines (sender + content), newest first.
    messages: Vec<String>,
}

/// AI Assist suggestion result (action: "suggest"). Each field is a suggested
/// completion for the corresponding form field; an empty string means the field
/// was already filled or the model had no suggestion. `surface` records which
/// form the suggestions target so the Author banner doesn't render in Compose
/// (and vice versa).
#[derive(Clone, Debug)]
struct AiSuggestions {
    surface: String,
    name: String,
    agent_type: String,
    description: String,
    system_prompt: String,
    mission: String,
    agents: String,
}

/// AI Assist validation verdict (action: "validate"). `valid` is the model's
/// well-formedness check; `issues` lists the problems when `valid` is false.
/// `surface` gates which form's banner renders.
#[derive(Clone, Debug)]
struct ValidationResult {
    surface: String,
    valid: bool,
    issues: Vec<String>,
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
        // while it is already being updated").
        let workspace_handle = workspace.weak_handle();
        let project = workspace.project().clone();
        let fs = workspace.app_state().fs.clone();
        cx.new(|cx| {
            let query_editor = cx.new(|cx| {
                let mut input = Editor::single_line(window, cx);
                input.set_placeholder_text("Search agents and swarms...", window, cx);
                input
            });
            let publish_reason = cx.new(|cx| {
                let mut input = Editor::single_line(window, cx);
                input.set_placeholder_text(
                    "Reason for force-publish (audited to admin_bypass_events)",
                    window,
                    cx,
                );
                input
            });
            let query_sub = cx.subscribe(&query_editor, Self::on_query_change);
            // Re-filter the browse list whenever settings change. The backend
            // mode toggle (`set_swarm_mode`) writes `kask.swarm.mode` via
            // `update_settings_file`, which is async — `current_swarm_mode` is
            // stale until the settings reload completes. This observer fires on
            // that reload and re-runs `filter_entries` with the live mode, so
            // the list reflects the new backend even though `set_swarm_mode`'s
            // immediate filter call read a stale value. Mirrors
            // `AgentRegistryPage`'s settings observer.
            let settings_sub = cx.observe_global::<SettingsStore>(|this, cx| {
                this.filter_entries(Self::current_swarm_mode(cx), cx);
            });
            let subscriptions = [query_sub, settings_sub];

            let scroll_handle = UniformListScrollHandle::new();

            let author = AuthorForm::new(window, cx);
            let compose = ComposeForm::new(window, cx);
            let swarm_add_agent_editor = cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Agent id to add to this swarm", window, cx);
                e
            });

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
                local_balance: None,
                pending_hire: None,
                pending_publish: None,
                publish_reason,
                selected_workspace: None,
                spend_in_flight: None,
                swarm_detail: None,
                swarm_add_agent_editor,
                run_status: None,
                mode: PanelMode::Browse,
                author,
                compose,
                steer_conversation: None,
                steer_connection_store: None,
                ai_assist_busy: false,
                ai_assist_action: None,
                ai_assist_suggestions: None,
                validation_result: None,
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
    fn clone_to_local(&mut self, agent_name: String, cx: &mut Context<Self>) {
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
    fn push_to_cloud(&mut self, agent_name: String, cx: &mut Context<Self>) {
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

    /// Open the roster drill-down for a swarm (item 4). The fetch is
    /// mode-aware: `Local` swarms are read via `swarm_get_local_swarm`
    /// (members are agent ids; agent_type/description are not carried by the
    /// local swarm record, so the roster rows show the id only), while `Cloud`
    /// swarms are read via `swarm_get_swarm` (ABW's server-sanitized roster
    /// payload, parsed defensively across plausible envelope shapes).
    fn open_swarm_detail(
        &mut self,
        workspace_id: String,
        name: String,
        source: AgentSource,
        mission: String,
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
    fn close_swarm_detail(&mut self, cx: &mut Context<Self>) {
        self.swarm_detail = None;
        cx.notify();
    }

    /// Fetch and show a swarm's recent run status (item 3):
    /// `swarm_run_status(workspace_id)`. Rendered as a dismissible strip.
    fn show_run_status(&mut self, workspace_id: String, name: String, cx: &mut Context<Self>) {
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
    fn dismiss_run_status(&mut self, cx: &mut Context<Self>) {
        self.run_status = None;
        cx.notify();
    }

    /// Remove a local-only agent card (item 5 local counterpart of firing).
    /// Calls `swarm_remove_local`, which deletes the card directory. A synced
    /// card's ABW agent is untouched. On success, re-fetches so the list and
    /// source badges update.
    fn remove_local_agent(&mut self, agent_name: String, cx: &mut Context<Self>) {
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
    fn fire_agent(&mut self, workspace_id: String, agent_id: String, cx: &mut Context<Self>) {
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
    fn add_agent_to_swarm(&mut self, swarm_id: String, agent_name: String, cx: &mut Context<Self>) {
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
    fn remove_agent_from_swarm(
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
    fn delete_local_swarm(&mut self, swarm_id: String, cx: &mut Context<Self>) {
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

    /// Fetch the pre-flight hire cost for an agent and open the consent gate.
    /// This is the entry point to the cost/consent flow: read-only, spends
    /// nothing, and populates `pending_hire` so the banner renders.
    fn begin_hire(&mut self, agent_name: String, cx: &mut Context<Self>) {
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
    fn cancel_hire(&mut self, cx: &mut Context<Self>) {
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
    fn begin_publish(&mut self, agent_name: String, cx: &mut Context<Self>) {
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
    fn confirm_publish(&mut self, cx: &mut Context<Self>) {
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
    fn cancel_publish(&mut self, cx: &mut Context<Self>) {
        if self.pending_publish.take().is_some() {
            log::info!("swarm-panel: operator cancelled publish (gate aborted)");
        }
        cx.notify();
    }

    /// Invoke `swarm_publish_agent` and re-fetch on success. Restores the
    /// banner on error so the operator can retry without re-preflighting.
    fn do_publish(
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
        // The Steer conversation bakes the backend mode into its system prompt
        // at construction (`ensure_steer_conversation` reads `current_swarm_mode`
        // once). A mode toggle after Steer is open would leave the curator
        // reading a stale mode (and passing it as `context.mode` to the skill
        // cascade). Drop the conversation so the next Steer selection rebuilds
        // with the new backend.
        if self.steer_conversation.take().is_some() {
            log::info!(
                "swarm-panel: backend mode toggled — Steer conversation rebuilt with the new mode"
            );
        }
        // Re-filter the browse list so the toggle is visually connected to
        // what is shown: ABW mode shows cloud agents/swarms, Local mode shows
        // local agents/swarms. Pass the target `mode` directly —
        // `update_settings_file` above is async, so `current_swarm_mode` is
        // still stale at this point. A `SettingsStore` observer re-runs the
        // filter with the live mode once the settings reload completes.
        self.filter_entries(mode, cx);
        cx.notify();
    }

    /// Lazily construct the `ConversationView` for Steer mode if it doesn't
    /// exist yet. Constructs a `CuratorAgentServer` scoped to the swarm MCP
    /// server, with a system prompt that tells the curator about the
    /// `swarm-intelligence` skill and the active swarm. The curator's
    /// `SkillTool` invokes the cascade when the operator asks to compose/steer
    /// a swarm.
    ///
    /// `window` is required because `ConversationView::new` may focus its
    /// inner `MessageEditor`.
    fn ensure_steer_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.steer_conversation.is_some() {
            return;
        }

        let thread_store = ThreadStore::global(cx);
        let mode = Self::current_swarm_mode(cx);
        let agent_server = std::rc::Rc::new(
            agent::CuratorAgentServer::new(self.fs.clone(), thread_store)
                .with_extra_static_context(steer_system_prompt(
                    self.selected_workspace.as_deref(),
                    mode,
                ))
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

    /// Create a new agent from the authoring form. Mode-aware: in Local mode
    /// the agent is created on the local substrate via `swarm_create_local_agent`
    /// (field `agent_id`, no cost, no consent); in ABW mode it is created in the
    /// ABW catalogue via `swarm_create_agent` (field `agent_name`).
    fn create_agent(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
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
        let is_local = Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local;
        // Mode-aware slug pre-validation. ABW requires `^[a-z0-9_]{3,64}$` —
        // a server-side rejection after the operator has filled every field
        // is a poor round-trip; validate up front so the error is
        // field-specific and immediate. Local mode allows alphanumeric plus
        // `-_.` (the local substrate sanitizes the id), but warn if the name
        // contains chars that would be stripped.
        let trimmed_name = name.trim();
        if is_local {
            if trimmed_name.is_empty() {
                self.author.status = Some("Name is required.".into());
                cx.notify();
                return;
            }
            let has_strippable = trimmed_name
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'));
            if has_strippable {
                self.author.status = Some(
                    "Name contains characters that will be stripped on the local \
                     substrate (allowed: letters, digits, -, _, .)."
                        .into(),
                );
                cx.notify();
                return;
            }
        } else {
            let len = trimmed_name.chars().count();
            let valid = (3..=64).contains(&len)
                && trimmed_name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                self.author.status = Some(
                    "Name must be 3-64 chars: lowercase letters, digits, underscores only \
                     (ABW slug rule)."
                        .into(),
                );
                cx.notify();
                return;
            }
        }
        let agent_type = self.author.agent_type.clone();
        // The selector enforces the agent_type, but double-check — a stale
        // form state (e.g. a future refactor) must not silently send an
        // invalid type to the server.
        if !matches!(agent_type.as_str(), "research" | "creative" | "meta") {
            self.author.status =
                Some("Agent type must be one of: research, creative, meta.".into());
            cx.notify();
            return;
        }
        let tags_raw = self.author.tags.read(cx).text(cx);
        let tags: Vec<String> = tags_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let visibility = self.author.visibility.clone();
        // Parse valence fields. Arousal and valence are optional floats.
        let arousal_raw = self.author.valence_arousal.read(cx).text(cx);
        let valence_raw = self.author.valence_valence.read(cx).text(cx);
        let primary_affect = self.author.valence_primary_affect.read(cx).text(cx);
        let traits_raw = self.author.valence_personality_traits.read(cx).text(cx);
        let personality_traits: Vec<String> = traits_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        // Build the valence object only if at least one field is non-empty.
        let valence = if arousal_raw.trim().is_empty()
            && valence_raw.trim().is_empty()
            && primary_affect.trim().is_empty()
            && personality_traits.is_empty()
        {
            None
        } else {
            Some(json!({
                "arousal": arousal_raw.trim().parse::<f64>().ok(),
                "valence": valence_raw.trim().parse::<f64>().ok(),
                "primary_affect": if primary_affect.trim().is_empty() { None } else { Some(primary_affect.trim()) },
                "personality_traits": personality_traits,
            }))
        };
        self.author.busy = true;
        self.author.status = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = if is_local {
                invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_create_local_agent",
                        json!({
                            "agent_id": name.trim(),
                            "agent_type": agent_type,
                            "system_prompt": system_prompt.trim(),
                            "description": description.trim(),
                            "tags": tags,
                            "visibility": visibility,
                            "valence": valence,
                        }),
                    )
                    .await
            } else {
                invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_create_agent",
                        json!({
                            "agent_name": name.trim(),
                            "agent_type": agent_type,
                            "system_prompt": system_prompt.trim(),
                            "description": description.trim(),
                            "tags": tags,
                            "visibility": visibility,
                            "valence": valence,
                        }),
                    )
                    .await
            };
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

    /// Create a new swarm from the compose form. Mode-aware: in Local mode the
    /// swarm is created on the local substrate via `swarm_create_local_swarm`
    /// (no cost, no consent — members are agent ids); in ABW mode the existing
    /// consent-gated `swarm_create_swarm` path is used, hiring any listed agents.
    fn create_swarm(&mut self, cx: &mut Context<Self>) {
        let Some(invoker) = crate::shared_tool_invoker() else {
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
        // Warn on excessively long names — the server may truncate or reject,
        // and a name over 128 chars is almost certainly a paste error.
        if name.trim().chars().count() > 128 {
            self.compose.status = Some("Swarm name is too long (max 128 characters).".into());
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
        let is_local = Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local;

        self.compose.busy = true;
        self.compose.status = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            // Local mode: create the swarm on the local substrate directly — no
            // cost, no consent tokens. Members are agent ids (the local swarm
            // roster is ids; resolution happens at delegation time).
            if is_local {
                let result = invoker
                    .invoke_tool(
                        SWARM_SERVER,
                        "swarm_create_local_swarm",
                        json!({
                            "name": name.trim(),
                            "mission": mission.trim(),
                            "agents": agents,
                        }),
                    )
                    .await;
                this.update(cx, |this, cx| {
                    this.compose.busy = false;
                    match result {
                        Ok(_) => {
                            this.compose.status =
                                Some(format!("Local swarm '{}' created.", name.trim()).into());
                            this.fetch_all(cx);
                        }
                        Err(err) => {
                            this.compose.status = Some(format!("Create failed: {err}").into());
                        }
                    }
                    cx.notify();
                })
                .ok();
                return;
            }
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
        let Some(invoker) = crate::shared_tool_invoker() else {
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

    // ── AI Assist ─────────────────────────────────────────────────────────
    //
    // The Author and Compose surfaces call the `swarm_ai_assist` MCP tool for
    // two purposes: `action: "suggest"` asks the default model to propose
    // completions for empty or partial fields (offered as an Apply banner),
    // and `action: "validate"` runs a well-formedness check before create
    // (offered as a validation banner). The panel only reads editors here;
    // `apply_ai_suggestions` (which writes editors) takes `&mut Window`.

    /// Call `swarm_ai_assist` with the current form fields. `action` is either
    /// `"suggest"` or `"validate"`. The surface ("agent" / "swarm") is derived
    /// from `self.mode`; only Author and Compose are wired (Browse/Steer are
    /// ignored). Stores the result in `ai_assist_suggestions` or
    /// `validation_result` for the surface's banner to render.
    fn ai_assist(&mut self, action: &str, cx: &mut Context<Self>) {
        let surface = match self.mode {
            PanelMode::Author => "agent",
            PanelMode::Compose => "swarm",
            // AI Assist is only wired for Author and Compose — a call from
            // another mode is a no-op rather than a panic.
            _ => return,
        };
        let Some(invoker) = crate::shared_tool_invoker() else {
            self.author.status = Some("Tool invoker not wired.".into());
            cx.notify();
            return;
        };
        let mode = if Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local {
            "local"
        } else {
            "abw"
        };

        let (name, agent_type, description, system_prompt, mission, agents) = if surface == "agent"
        {
            (
                self.author.name.read(cx).text(cx),
                self.author.agent_type.clone(),
                self.author.description.read(cx).text(cx),
                self.author.system_prompt.read(cx).text(cx),
                String::new(),
                String::new(),
            )
        } else {
            (
                self.compose.name.read(cx).text(cx),
                String::new(),
                String::new(),
                String::new(),
                self.compose.mission.read(cx).text(cx),
                self.compose.agents.read(cx).text(cx),
            )
        };

        self.ai_assist_busy = true;
        self.ai_assist_action = Some(action.to_string());
        // Clear stale banners so the operator doesn't see the previous result
        // while a new call is in flight (mirrors the Xaman Ek stale-suggestion
        // fix, L5).
        self.ai_assist_suggestions = None;
        self.validation_result = None;
        cx.notify();

        let surface_owned = surface.to_string();
        let action_owned = action.to_string();
        cx.spawn(async move |this, cx| {
            let result = invoker
                .invoke_tool(
                    SWARM_SERVER,
                    "swarm_ai_assist",
                    json!({
                        "action": action_owned,
                        "surface": surface_owned,
                        "mode": mode,
                        "name": name,
                        "agent_type": agent_type,
                        "description": description,
                        "system_prompt": system_prompt,
                        "mission": mission,
                        "agents": agents,
                    }),
                )
                .await;
            this.update(cx, |this, cx| {
                this.ai_assist_busy = false;
                this.ai_assist_action = None;
                match result {
                    Ok(output) => {
                        if let Some(content) = parse_tool_response(&output) {
                            if action_owned == "suggest" {
                                let s = content.get("suggestions");
                                let pick = |key: &str| {
                                    s.and_then(|s| s.get(key))
                                        .and_then(|v| v.as_str())
                                        .map(str::to_string)
                                        .unwrap_or_default()
                                };
                                this.ai_assist_suggestions = Some(AiSuggestions {
                                    surface: surface_owned.clone(),
                                    name: pick("name"),
                                    agent_type: pick("agent_type"),
                                    description: pick("description"),
                                    system_prompt: pick("system_prompt"),
                                    mission: pick("mission"),
                                    agents: pick("agents"),
                                });
                            } else {
                                let valid = content
                                    .get("valid")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let issues = content
                                    .get("issues")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(str::to_string))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                this.validation_result = Some(ValidationResult {
                                    surface: surface_owned.clone(),
                                    valid,
                                    issues,
                                });
                            }
                        }
                    }
                    Err(err) => {
                        // Surface the error on the active form's status line so
                        // the operator gets feedback (mirrors create_agent).
                        let msg = format!("AI Assist unavailable: {err}");
                        if surface_owned == "agent" {
                            this.author.status = Some(msg.into());
                        } else {
                            this.compose.status = Some(msg.into());
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Apply the stored AI Assist suggestions to the form editors. Each
    /// non-empty suggestion overwrites the corresponding field. For the agent
    /// surface, `agent_type` is only applied when it is a valid selector value
    /// (research/creative/meta). Clears `ai_assist_suggestions` after applying.
    /// Requires `&mut Window` because `Editor::set_text` needs it.
    fn apply_ai_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.ai_assist_suggestions.clone() else {
            return;
        };
        if s.surface == "agent" {
            if !s.name.is_empty() {
                let editor = self.author.name.clone();
                editor.update(cx, |e, cx| e.set_text(s.name, window, cx));
            }
            if !s.description.is_empty() {
                let editor = self.author.description.clone();
                editor.update(cx, |e, cx| e.set_text(s.description, window, cx));
            }
            if !s.system_prompt.is_empty() {
                let editor = self.author.system_prompt.clone();
                editor.update(cx, |e, cx| e.set_text(s.system_prompt, window, cx));
            }
            if matches!(s.agent_type.as_str(), "research" | "creative" | "meta") {
                self.author.agent_type = s.agent_type;
            }
        } else if s.surface == "swarm" {
            if !s.name.is_empty() {
                let editor = self.compose.name.clone();
                editor.update(cx, |e, cx| e.set_text(s.name, window, cx));
            }
            if !s.mission.is_empty() {
                let editor = self.compose.mission.clone();
                editor.update(cx, |e, cx| e.set_text(s.mission, window, cx));
            }
            if !s.agents.is_empty() {
                let editor = self.compose.agents.clone();
                editor.update(cx, |e, cx| e.set_text(s.agents, window, cx));
            }
        }
        self.ai_assist_suggestions = None;
        cx.notify();
    }

    /// Dismiss the AI Assist suggestions banner without applying.
    fn dismiss_ai_suggestions(&mut self, cx: &mut Context<Self>) {
        self.ai_assist_suggestions = None;
        cx.notify();
    }

    /// Dismiss the validation banner.
    fn dismiss_validation(&mut self, cx: &mut Context<Self>) {
        self.validation_result = None;
        cx.notify();
    }

    /// Filter the browse entries by the active `SwarmFilter` (All/Swarms/Agents),
    /// the search query, and the backend mode. The mode is a parameter rather
    /// than re-read from settings so `set_swarm_mode` can pass the target mode
    /// for immediate feedback — `update_settings_file` is async, so
    /// `current_swarm_mode` is stale until the settings reload completes. A
    /// `SettingsStore` observer re-runs this with the live mode on reload.
    fn filter_entries(&mut self, mode: kask_bridge::SwarmModeConfig, cx: &mut Context<Self>) {
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
                let source_matches = match entry {
                    SwarmEntry::Agent(a) => match mode {
                        kask_bridge::SwarmModeConfig::Abw => {
                            a.source == AgentSource::Cloud || a.source == AgentSource::Synced
                        }
                        kask_bridge::SwarmModeConfig::Local => {
                            a.source == AgentSource::Local || a.source == AgentSource::Synced
                        }
                    },
                    SwarmEntry::Swarm(s) => match mode {
                        kask_bridge::SwarmModeConfig::Abw => s.source == AgentSource::Cloud,
                        kask_bridge::SwarmModeConfig::Local => s.source == AgentSource::Local,
                    },
                };
                if !source_matches {
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

    /// The dismissible run-status strip (item 3): recent ABW workspace
    /// messages for the requested swarm.
    fn render_run_status_strip(
        &self,
        status: &RunStatusView,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let border = cx.theme().colors().border;
        v_flex()
            .w_full()
            .gap_1()
            .p_3()
            .rounded_sm()
            .border_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Label::new(format!("Run Status — {}", status.name)).color(Color::Default),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("dismiss-run-status", "Close")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_run_status(cx);
                            })),
                    ),
            )
            .when(status.loading, |this| {
                this.child(
                    Label::new("Loading…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .when_some(status.error.clone(), |this, err| {
                this.child(Label::new(err).size(LabelSize::Small).color(Color::Warning))
            })
            .when(
                status.messages.is_empty() && !status.loading && status.error.is_none(),
                |this| {
                    this.child(
                        Label::new("No recent activity.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                },
            )
            .when(!status.messages.is_empty(), |this| {
                this.child(v_flex().gap_0p5().children(status.messages.iter().map(|m| {
                    Label::new(m.clone())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                })))
            })
    }

    fn render_search(&self, cx: &mut Context<Self>) -> Div {
        marketplace_search_bar(&self.query_editor, false, cx)
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
                this.filter_entries(Self::current_swarm_mode(cx), cx);
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
        let mode = Self::current_swarm_mode(cx);
        let is_local = mode == kask_bridge::SwarmModeConfig::Local;

        let message: SharedString = if self.is_fetching() {
            "Loading agents and swarms…".into()
        } else if let Some(err) = self.visible_error() {
            format!("Failed to load swarm data: {err}").into()
        } else {
            match self.filter {
                SwarmFilter::All => {
                    if has_search {
                        "No agents or swarms that match your search."
                    } else if is_local {
                        "No local agents or swarms. Create a local agent (Author) or clone a cloud agent to Local."
                    } else {
                        "No agents or swarms. Set HKASK_ABW_API_KEY to see your swarms."
                    }
                }
                SwarmFilter::Swarms => {
                    if has_search {
                        "No swarms that match your search."
                    } else if is_local {
                        "No local swarms. Compose one (Compose) to group local agents."
                    } else {
                        "No swarms. Set HKASK_ABW_API_KEY to see your workspaces."
                    }
                }
                SwarmFilter::Agents => {
                    if has_search {
                        "No agents that match your search."
                    } else if is_local {
                        "No local agents. Create one (Author) or clone a cloud agent to Local."
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

    // ── AI Assist render helpers ───────────────────────────────────────────
    //
    // `render_ai_assist_row` is the two-button row (✨ AI Assist / ✓ Validate)
    // shown on both the Author and Compose surfaces. `render_ai_suggestions_banner`
    // and `render_validation_banner` mirror `render_publish_banner`: a bordered
    // box with Apply/Dismiss (suggestions) or a success/issues list (validation).
    // Each banner is gated by its `surface` field so the Author banner does not
    // render in Compose and vice versa.

    /// The AI Assist button row for a form surface ("agent" or "swarm"). Shown
    /// below the fields and above the Create button. Both buttons are disabled
    /// while the form's create is in flight or an AI Assist call is in flight.
    fn render_ai_assist_row(
        &self,
        surface: &str,
        form_busy: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let disabled = form_busy || self.ai_assist_busy;
        let busy_label = match self.ai_assist_action.as_deref() {
            Some("validate") => "Validating…",
            Some("suggest") => "Assisting…",
            _ => "",
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Button::new(format!("ai-assist-{surface}"), "✨ AI Assist")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .disabled(disabled)
                    .tooltip(Tooltip::text(
                        "Uses the default model to suggest completions for empty or \
                         partial fields based on ABW/Local composition guidance.",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ai_assist("suggest", cx);
                    })),
            )
            .child(
                Button::new(format!("ai-validate-{surface}"), "✓ Validate")
                    .style(ButtonStyle::Subtle)
                    .label_size(LabelSize::XSmall)
                    .disabled(disabled)
                    .tooltip(Tooltip::text(
                        "Runs the inputs through the default model to check \
                         well-formedness and surface issues before creating.",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ai_assist("validate", cx);
                    })),
            )
            .when(!busy_label.is_empty(), |this| {
                this.child(
                    Label::new(busy_label)
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
    }

    /// The AI Assist suggestions banner for a form surface. Returns `None`
    /// when there are no suggestions or they target a different surface.
    fn render_ai_suggestions_banner(
        &self,
        surface: &str,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let s = self.ai_assist_suggestions.clone()?;
        if s.surface != surface {
            return None;
        }
        let border = cx.theme().colors().border;
        // Collect (label, value) pairs for the non-empty suggestions so the
        // operator sees exactly which fields would change.
        let fields: Vec<(&'static str, String)> = [
            ("Name", s.name.clone()),
            ("Agent type", s.agent_type.clone()),
            ("Description", s.description.clone()),
            ("System prompt", s.system_prompt.clone()),
            ("Mission", s.mission.clone()),
            ("Agents", s.agents),
        ]
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .collect();
        if fields.is_empty() {
            // No suggestions to apply — show a note instead of an empty banner.
            return Some(
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
                            .child(Label::new("✨ AI Assist").color(Color::Accent))
                            .child(
                                Label::new("No suggestions — the fields look complete.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        h_flex().gap_2().items_center().child(div().flex_1()).child(
                            Button::new(format!("dismiss-ai-sug-empty-{surface}"), "Dismiss")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_ai_suggestions(cx);
                                })),
                        ),
                    ),
            );
        }
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
                        .child(Label::new("✨ AI Assist").color(Color::Accent))
                        .child(
                            Label::new("Suggested completions:")
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .children(fields.into_iter().map(|(label, value)| {
                            // Truncate long suggestion previews so a full
                            // system-prompt draft doesn't blow out the panel height.
                            let preview = if value.chars().count() > 120 {
                                let truncated: String = value.chars().take(120).collect();
                                format!("• {label}: {truncated}…")
                            } else {
                                format!("• {label}: {value}")
                            };
                            Label::new(preview)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted)
                        })),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1())
                        .child(
                            Button::new(format!("apply-ai-sug-{surface}"), "Apply")
                                .style(ButtonStyle::Filled)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.apply_ai_suggestions(window, cx);
                                })),
                        )
                        .child(
                            Button::new(format!("dismiss-ai-sug-{surface}"), "Dismiss")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_ai_suggestions(cx);
                                })),
                        ),
                ),
        )
    }

    /// The validation banner for a form surface. Returns `None` when there is
    /// no result or it targets a different surface. Shows a success label when
    /// `valid`, or the issues list (Warning color) when not.
    fn render_validation_banner(
        &self,
        surface: &str,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let v = self.validation_result.clone()?;
        if v.surface != surface {
            return None;
        }
        let border = cx.theme().colors().border;
        let warning = cx.theme().status().warning;
        let header = if v.valid {
            "✓ Validation passed"
        } else {
            "✗ Validation found issues"
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
                        .child(Label::new(header).color(if v.valid {
                            Color::Accent
                        } else {
                            Color::Warning
                        }))
                        .when(v.valid, |this| {
                            this.child(
                                Label::new("Inputs look well-formed.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                        }),
                )
                .when(!v.valid, |this| {
                    this.child(v_flex().gap_0p5().children(v.issues.iter().map(|issue| {
                        Label::new(format!("• {issue}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Warning)
                    })))
                })
                .child(
                    h_flex().gap_2().items_center().child(div().flex_1()).child(
                        Button::new(format!("dismiss-validation-{surface}"), "Dismiss")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::XSmall)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_validation(cx);
                            })),
                    ),
                )
                .when(!v.valid, |this| {
                    this.child(div().w_full().h(px(2.)).bg(warning))
                }),
        )
    }

    /// The publish banner (fermi v0.10.15). Mirrors the consent banner: a
    /// Confirm path when `can_publish`, or a force-publish path listing the
    /// failing checks plus a reason input (audited to `admin_bypass_events`).
    fn render_publish_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.pending_publish.clone()?;
        let border = cx.theme().colors().border;
        let warning = cx.theme().status().warning;
        let busy = self.spend_in_flight.is_some();

        let header = if pending.can_publish {
            format!("Publish '{}' to the catalogue?", pending.agent_name)
        } else {
            format!(
                "Cannot publish '{}' yet — fix the checks or force-publish as admin:",
                pending.agent_name
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
                        .child(Label::new("Publish").color(Color::Default))
                        .child(Label::new(header).size(LabelSize::Small).color(
                            if pending.can_publish {
                                Color::Muted
                            } else {
                                Color::Warning
                            },
                        )),
                )
                .when(!pending.can_publish, |this| {
                    this.child(
                        v_flex()
                            .gap_0p5()
                            .children(pending.failing_checks.iter().map(|c| {
                                Label::new(format!("• {c}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Warning)
                            }))
                            .child(
                                div()
                                    .border_1()
                                    .border_color(border)
                                    .rounded_sm()
                                    .child(self.publish_reason.clone()),
                            ),
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1())
                        .child(
                            Button::new(
                                "confirm-publish",
                                if pending.can_publish {
                                    "Confirm"
                                } else {
                                    "Force publish"
                                },
                            )
                            .style(ButtonStyle::Filled)
                            .label_size(LabelSize::XSmall)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_publish(cx);
                            })),
                        )
                        .child(
                            Button::new("cancel-publish", "Cancel")
                                .style(ButtonStyle::Subtle)
                                .label_size(LabelSize::XSmall)
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_publish(cx);
                                })),
                        ),
                )
                .when(!pending.can_publish, |this| {
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
                            })
                            // v2 §15: in local mode the algedonic channel is the
                            // local ledger balance (operator-funded). Shown only
                            // when the backend mode is local; hidden when unknown
                            // — never a fabricated zero.
                            .when(
                                Self::current_swarm_mode(cx) == kask_bridge::SwarmModeConfig::Local,
                                |this| {
                                    this.when_some(self.local_balance, |this, balance| {
                                        this.child(
                                            Label::new(format!("■ {balance} local credits"))
                                                .size(LabelSize::Small)
                                                .color(if balance <= 0 {
                                                    Color::Warning
                                                } else {
                                                    Color::Muted
                                                }),
                                        )
                                    })
                                },
                            ),
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
                    .children(self.render_publish_banner(cx))
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
                                                        this.filter_entries(Self::current_swarm_mode(cx), cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Swarms",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.filter = SwarmFilter::Swarms;
                                                        this.filter_entries(Self::current_swarm_mode(cx), cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Agents",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.filter = SwarmFilter::Agents;
                                                        this.filter_entries(Self::current_swarm_mode(cx), cx);
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
                            // Run-status strip (dismissible) above the list.
                            let content = this
                                .when_some(self.run_status.clone(), |this, status| {
                                    this.child(self.render_run_status_strip(&status, cx))
                                })
                                .when_some(self.swarm_detail.clone(), |this, detail| {
                                    this.child(self.render_swarm_detail(&detail, cx))
                                });
                            if self.swarm_detail.is_some() {
                                content.into_any_element()
                            } else {
                                let count = self.filtered_entry_indices.len();
                                if count == 0 {
                                    content.child(self.render_empty_state(cx)).into_any_element()
                                } else {
                                    let scroll_handle = &self.list;
                                    content
                                        .child(
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

    // Pins the tool name strings the panel calls. The single source of truth
    // is `parse::SWARM_TOOLS` — a rename in `hkask-mcp-swarm` must update that
    // const (the count assertion below catches a stale list). The Steer-mode
    // prompt-token test (`steer_prompt_mentions_only_known_tools`) then catches
    // any `swarm_*` name the prompt still mentions that isn't in the const, so
    // a rename surfaces here rather than degrading to "tool not found" at
    // runtime.
    //
    // TODO: `hkask-mcp-swarm` does not export a canonical tool-name list
    // (no `TOOL_NAMES` const or equivalent). When the rmcp `#[tool_router]`
    // macro exposes a way to enumerate tool names at compile time, wire this
    // test to that canonical list so a rename in the server is caught here
    // rather than degrading to "tool not found" at runtime. Until then,
    // `SWARM_TOOLS` must be kept in sync manually with the `#[tool]` fn names
    // in `hkask-mcp-swarm/src/hkask_mcp_swarm.rs`.
    #[test]
    fn panel_tool_names_match_server() {
        // `SWARM_TOOLS` must match the #[tool] fn names in
        // `hkask-mcp-swarm/src/hkask_mcp_swarm.rs`. Keep it in sync when
        // adding/removing a server tool — a rename in `hkask-mcp-swarm` must
        // be reflected there so the panel's `invoke_tool` call sites don't
        // silently degrade to "tool not found".
        assert_eq!(SWARM_SERVER, "swarm");

        // Pin the count so adding or removing a server tool without updating
        // the const is caught — a count mismatch is the loudest signal short
        // of importing the server's canonical list.
        assert_eq!(
            parse::SWARM_TOOLS.len(),
            51,
            "tool count changed — update SWARM_TOOLS to match hkask-mcp-swarm #[tool] fns"
        );

        for tool in parse::SWARM_TOOLS {
            assert!(
                tool.starts_with("swarm_") && tool.len() > "swarm_".len(),
                "tool name `{tool}` must start with `swarm_` and have a non-empty suffix"
            );
        }

        // No duplicates — a copy-paste error would silently mask a missing
        // tool by doubling another.
        let mut sorted = parse::SWARM_TOOLS.to_vec();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            before,
            "duplicate tool names in SWARM_TOOLS list"
        );
    }

    // The pure parsing helpers (extract_wallet_balance, parse_swarm_roster,
    // parse_run_status_messages, extract_agent_mentions, staleness_chip,
    // parse_publish_checks) and their unit tests live in `parse::tests` —
    // extracted for cohesion. The tests below cover the panel-level wiring
    // (envelope + typed-deserialize paths, the steer system prompt, and the
    // hire/publish banner contracts) that depends on `SwarmPanel` state or
    // the typed response structs defined here.

    #[test]
    fn workspace_list_parses_verified_field_names() {
        // The verified `/workspaces` shape (live, 2026-08-02) carries
        // agent_count / workspace_budget / workspace_remaining under exactly
        // these names — pin the parse contract so a future rename is caught.
        let json = serde_json::json!({
            "workspaces": [{
                "id": "ws-1",
                "name": "alpha",
                "description": "d",
                "slug": "alpha",
                "origin": "create",
                "owner_id": "o1",
                "agent_count": 3,
                "workspace_budget": 500,
                "workspace_remaining": 200,
            }]
        });
        let parsed: WorkspaceListResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(parsed.workspaces.len(), 1);
        let w = &parsed.workspaces[0];
        assert_eq!(w.id.as_deref(), Some("ws-1"));
        assert_eq!(w.agent_count, Some(3));
        assert_eq!(w.workspace_budget, Some(500));
        assert_eq!(w.workspace_remaining, Some(200));
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
        let prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Abw);
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
        let prompt = steer_system_prompt(None, kask_bridge::SwarmModeConfig::Local);
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
        let prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Abw);
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
        let prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Local);
        for tool in [
            "swarm_list_local_agents",
            "swarm_balance_local",
            "swarm_local_history",
            "swarm_fund_local",
            "swarm_delegate_local",
            "swarm_fanout_local",
            "swarm_pipeline_local",
            "swarm_clone_to_local",
            "swarm_remove_local",
            "swarm_create_local_agent",
            "swarm_reconfigure_local_agent",
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

    // The steer prompt must carry the current backend mode and instruct the
    // curator to pass it (plus the workspace) in the skill's `context`
    // argument. Without `mode` in the cascade context, the
    // swarm-intelligence templates default to the abw branch — the skill
    // steers the wrong backend. Pins the G1 fix (mode → skill context).
    #[test]
    fn steer_prompt_carries_mode_and_context_instruction() {
        let abw_prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Abw);
        assert!(
            abw_prompt.contains("\"mode\": \"abw\""),
            "abw prompt must carry mode abw in the context example"
        );
        assert!(
            abw_prompt.contains("\"swarm_id\": \"ws_test\""),
            "abw prompt must carry the workspace id in the context example"
        );
        assert!(
            abw_prompt.contains("`context` argument"),
            "steer prompt must tell the curator to pass context to the skill tool"
        );
        assert!(
            abw_prompt.contains("default to `abw`"),
            "steer prompt must warn that a missing mode defaults to abw"
        );
        // The slash-command context syntax must be documented so the
        // operator knows how to pass mode/swarm_id via `/swarm-intelligence
        // mode=local swarm_id=ws-1 compose my swarm`.
        assert!(
            abw_prompt.contains("key=value"),
            "steer prompt must document the key=value slash-command context syntax"
        );

        let local_prompt =
            steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Local);
        assert!(
            local_prompt.contains("\"mode\": \"local\""),
            "local prompt must carry mode local in the context example"
        );
    }

    // Cybernetic Swarm Plan C0: the steer prompt must describe the optional
    // deterministic `task_success` skill input and the no-LLM-judge constraint.
    #[test]
    fn steer_prompt_describes_task_success() {
        let prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Abw);
        assert!(
            prompt.contains("task_success"),
            "steer prompt must describe the task_success skill input"
        );
        assert!(
            prompt.contains("deterministic"),
            "steer prompt must state the judge must be deterministic"
        );
        assert!(
            prompt.contains("Do NOT"),
            "steer prompt must forbid using an LLM to score the output as task_success"
        );
        assert!(
            prompt.contains("OMIT"),
            "steer prompt must tell the curator to OMIT task_success for open tasks"
        );
    }

    // Cybernetic Swarm Plan C2: the Steer prompt must name the Go See cadence
    // and the second-order monitor so the operator knows the human-check loop
    // is event-driven (on sensor-truth divergence) and what the checklist is.
    #[test]
    fn steer_prompt_describes_go_see_loop() {
        let prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Local);
        assert!(
            prompt.contains("Go See"),
            "steer prompt must name the Go See loop (C2)"
        );
        assert!(
            prompt.contains("second-order monitor"),
            "steer prompt must name the deterministic second-order monitor (C1)"
        );
        assert!(
            prompt.contains("go_see"),
            "steer prompt must name the go_see recommendation that triggers the loop"
        );
        assert!(
            prompt.contains("swarm_reconfigure_local_agent"),
            "steer prompt must name the reconfigure tool (C6)"
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

    // M5: the Steer-mode system prompt must not advertise any `swarm_*`
    // tool that isn't in the canonical `SWARM_TOOLS` const. The const is the
    // single source of truth shared with `panel_tool_names_match_server`;
    // when a tool is renamed in `hkask-mcp-swarm`, the count test fails until
    // the const is updated, and this test then catches any stale name the
    // prompt still mentions — so a rename surfaces here rather than degrading
    // to "tool not found" at runtime. The publish-checks and staleness-chip
    // parsers are unit-tested in `parse::tests`.
    #[test]
    fn steer_prompt_mentions_only_known_tools() {
        let known: std::collections::HashSet<&str> = parse::SWARM_TOOLS.iter().copied().collect();
        for prompt in [
            steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Abw),
            steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Local),
            steer_system_prompt(None, kask_bridge::SwarmModeConfig::Abw),
            steer_system_prompt(None, kask_bridge::SwarmModeConfig::Local),
        ] {
            // Tool names are advertised in backticks; field names like
            // `swarm_id` appear unquoted in prose, so only validate the
            // backtick-delimited segments to avoid false positives.
            for seg in prompt
                .split('`')
                .enumerate()
                .filter_map(|(i, s)| (i % 2 == 1).then_some(s))
            {
                let name: String = seg
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if name.starts_with("swarm_") && name.len() > "swarm_".len() {
                    assert!(
                        known.contains(name.as_str()),
                        "steer prompt advertises `{name}` which is not in SWARM_TOOLS \
                         — update the const or the prompt"
                    );
                }
            }
        }
    }
}
