#![forbid(unsafe_code)]
//! Swarm Panel — a center-pane `Item` listing Agent Bestiary World agents and
//! swarms (workspaces) as cards, mirroring the Kask Extensions panel layout.
//!
//! Entities are **agents** (from the ABW catalogue) and **swarms** (the
//! operator's workspaces), not skills. Data is fetched through the global
//! `ToolInvoker` hook (the metered MCP runtime path), so all
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
//! `kask/docs/plans/abw-swarm-intelligence.md` §13). The conversation is
//! persisted via the global `ThreadStore` — the curator's live state
//! (in-flight plans, collected `delegate_results`, prior iterations)
//! survives panel close and restart, and every turn is ingested into the
//! curator's sovereign memory for cross-composition learning.

mod agent_edit;
mod author;
mod card;
mod compose;
mod detail;
mod fetch;
mod hire;
pub mod panel_button;
mod parse;
mod swarm_ops;

use author::AuthorForm;
use compose::ComposeForm;

pub use panel_button::SwarmPanelButton;
// The `ToolInvoker` trait + global accessor live in the `hkask-tool-invoker` leaf
// crate (relocated so the kask GPUI widgets can dispatch without depending on
// this heavy panel crate). Re-exported here so existing `swarm_panel::ToolInvoker`
// / `set_tool_invoker` / `shared_tool_invoker` call sites compile unchanged.
pub use hkask_tool_invoker::{InvokeError, ToolInvoker, set_tool_invoker, shared_tool_invoker};

use parse::{AgentCard, AgentSource, SwarmCard, extract_agent_mentions, extract_wallet_balance};

use std::ops::Range;
use std::time::Duration;

use anyhow::Result;
use editor::Editor;
use fs::Fs;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, Render, ScrollHandle, Task,
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

/// Minimum roster size and mission requirement for launching a swarm into
/// Steer mode. A swarm with fewer than this many agents, or a blank mission,
/// is not ready for the `swarm-intelligence` PDCA loop — SENSE would converge
/// trivially on an empty/trivial swarm-state. The operator must add agents and
/// a mission in the compose form before the "Create" button is enabled.
const MIN_AGENTS_TO_LAUNCH: usize = 3;

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

/// The kanban MCP server id. References the canonical single source of truth
/// in `hkask_types::kanban_wire::KANBAN_SERVER_NAME` (no duplicated literal) so
/// a rename in the server propagates here. The kanban board is the durable
/// coordination source of truth; tasks link to swarms via `kanban_task_spawn`.
#[allow(dead_code)] // Used in tests only after kanban coordination moved to kanban panel
const KANBAN_SERVER: &str = hkask_types::kanban_wire::KANBAN_SERVER_NAME;

/// The system prompt injected into the Steer mode `ConversationView`. Tells
/// the curator it is scoped to the swarm MCP server and that the
/// `swarm-intelligence` skill is available for composition/steering. The
/// curator's `SkillTool` discovers the skill from the `<available_skills>`
/// list in its base system prompt; this prompt adds the swarm-specific
/// context (active workspace, current backend mode, the skill's purpose).
/// Build the Steer-mode system prompt for the curator agent.
///
/// Design tradeoff (R7): the `mode` variable reaches the `swarm-intelligence`
/// skill execution via the curator's `context` argument, which is prompt-level
/// instruction — not a hard-enforced input. The manifest defaults `mode` to
/// `'abw'` when the context lacks it (`{{ mode | default('abw') }}`). A
/// prompt-injected curator could omit `mode` to force ABW, or pass
/// `mode: "local"` to switch backends. This is a wrong-result risk, not a
/// security violation: both backends have their own spending gates (consent
/// tokens for ABW, ledger balance for local), so a wrong-mode cascade cannot
/// bypass spending controls. Hard enforcement (declaring `mode` as a required
/// manifest input) would change the schema and break
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
         `swarm_authorize_session`, `swarm_hire`, `swarm_delegate`, \
         `swarm_delegate_and_wait`, `swarm_fanout`, `swarm_fire`, \
         `swarm_create_agent`, `swarm_create_swarm`, \
         `swarm_generate_prompt`, `swarm_generate_ontology`, \
         `swarm_fork_agent`, `swarm_run_status`, \
         `swarm_search_knowledge`, `swarm_publish_checks`, \
         `swarm_publish_agent`, `swarm_xaman`. These \
         route to Agent Bestiary World and require the ABW API key. Per-tool \
         behavior is in each tool's description.\n\
         \n\
         **Local tools** (`mode: local`): `swarm_list_local_agents`, \
         `swarm_balance_local`, `swarm_local_history`, `swarm_fund_local`, \
         `swarm_delegate_local`, `swarm_fanout_local`, \
         `swarm_pipeline_local`, `swarm_clone_to_local`, `swarm_remove_local`, \
         `swarm_create_local_agent`, `swarm_reconfigure_local_agent`, \
         `swarm_push_to_cloud`, `swarm_search_knowledge_local`, \
         `swarm_generate_prompt_local`, `swarm_generate_ontology_local`, \
         `swarm_evaluate_local`, `swarm_execute_plan_local`. \
         These run on the local \
         substrate (`hkask-inference` + `hkask-ledger`) with no \
         ABW round-trips. Local delegation needs NO funding and NO consent — it \
         runs on the operator's own substrate, so there is nothing to authorize. \
         The local ledger is accounting only: it records spend so \
         `swarm_balance_local` and `swarm_local_history` can reconcile it, and a \
         negative balance is accumulated local spend, not an error. Do NOT call \
         `swarm_fund_local` before delegating and do NOT treat a low balance as a \
         blocker. Funding and consent gates apply to the CLOUD tools \
         (`swarm_hire`, `swarm_delegate`), where credits buy someone else's \
         compute. `swarm_clone_to_local` and `swarm_push_to_cloud` sync \
         cards between the local registry (`agents/local/curated/<id>/agent_card.json`) \
         and ABW; a cloned card carries `cloud_id` to track the sync link. \
         Per-tool behavior is in each tool's description.\n\
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
         compose my swarm` sets mode, swarm_id, and task.

         **Execution mode: steering (default).** When the skill emits a plan
         (emitted_calls), the manifest's post-Act execute step (step 8) calls
         `swarm_execute_plan_local` deterministically and feeds the returned
         `delegate_results` array into the next LOOP iteration so C5 (fault
         attribution) and C6 (reconfigure) close the loop structurally — no
         prompt instruction needed. In ABW mode, delegate to Xaman Ek via
         `swarm_xaman` with the plan as the message. The operator can use the \
         \"Launch Plan\" button to inject this instruction if you did not execute
         automatically.

         The consent gate (ABW mode only) is enforced by `swarm_request_consent` \
         (mints a single-use, action+target-scoped token) and `swarm_hire`/\
         `swarm_delegate` (consume the token before spending). Do not hire or \
         delegate without first calling `swarm_request_consent` and passing the \
         returned token to the spend tool. The consent gate is the enforcement \
         point — it must actually block, not just warn. In local mode there is no \
         consent token and no funding gate; the per-dispatch ceiling below is the \
         only local bound.\n\
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
    // C1: The kanban panel now hosts all kanban coordination (board/task
    // management, spawn, delegate). The swarm panel's steer prompt references
    // the kanban panel rather than duplicating the kanban tool advertising.
    let prompt = format!(
        r#"{prompt}

## Kanban Coordination

The kanban panel (View → Kanban Board) is the durable coordination source of truth for task state. It hosts all kanban tools — board/task CRUD, task spawn, delegate results, and kata coaching. Open the kanban panel's Steer mode to decompose work, create tasks, and spawn subagents. The swarm↔kanban bridge is `kanban_task_spawn` (pass the swarm_id arg to link the task to this swarm).
"#
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
                if name.starts_with("swarm_") && name.len() > "swarm_".len() {
                    return parse::SWARM_TOOLS.contains(&name.as_str());
                }
                if name.starts_with("kanban_") && name.len() > "kanban_".len() {
                    return parse::KANBAN_TOOLS.contains(&name.as_str());
                }
                true
            }),
        "steer_system_prompt advertises a swarm_* or kanban_* tool not in SWARM_TOOLS/KANBAN_TOOLS"
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

/// Browse-list filter by backend source. Orthogonal to `SwarmFilter` (which
/// selects kind: All/Swarms/Agents) — kind and source are independent axes, so a
/// single combined enum would either explode to 9 variants or lose the
/// ability to show, e.g., all local agents. `Synced` entries (cards that exist
/// in both cloud and local, linked by `cloud_swarm_id`) appear under both `Cloud`
/// and `Local` — they represent a card visible on either backend, so hiding
/// them from either view would be a regression. `All` shows every entry
/// regardless of source. Restores the source filtering that `bc51229ffe`
/// removed when it decoupled the per-form `CreateTarget` (creation target) from
/// the browse view; `CreateTarget` only affects authoring, not browsing.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum SourceFilter {
    All,
    Cloud,
    Local,
}

/// Which backend to target when creating an agent or swarm. This is a
/// per-form choice, not a global setting — both cloud and local backends are
/// always available (the swarm MCP server registers both tool sets in either
/// mode; `kask.swarm.mode` only selects a startup warning, not a capability
/// gate). The prior design gated this on `kask.swarm.mode`, which forced an
/// either/or round-trip through settings + MCP server restart just to create a
/// local agent while cloud was the default.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum CreateTarget {
    Cloud,
    Local,
}

/// A composition prompt waiting to be injected into the Steer conversation
/// after `create_swarm` succeeds and the Steer conversation is constructed.
/// Deferred to `render` because the injector needs `&mut Window`, which the
/// `create_swarm` spawn closure does not have.
struct PendingCompositionPrompt {
    swarm_id: String,
    mission: String,
    agents: Vec<String>,
    is_local: bool,
}

/// The backend a creation form's target should carry when the operator
/// enters a surface. The panel's backend context (`active_backend` — the
/// operator's last explicit cloud/local choice anywhere in the panel) is the
/// default; `None` means the form keeps its current target.
///
/// Compose always syncs (its target has no other source of truth). Author
/// syncs only when not editing — `load_agent_into_author` derives the target
/// from the agent's source and that choice must survive the `set_mode` call
/// it makes right after.
fn target_on_surface_entry(
    mode: PanelMode,
    author_is_editing: bool,
    active_backend: CreateTarget,
) -> Option<CreateTarget> {
    match mode {
        PanelMode::Compose => Some(active_backend),
        PanelMode::Author if !author_is_editing => Some(active_backend),
        _ => None,
    }
}

/// Whether a form status line should render as a warning. Statuses are
/// plain strings set from ~25 call sites; rather than threading a severity
/// enum through all of them, the render sites classify via this one
/// documented convention. A false positive only recolors a line (cosmetic);
/// the convention is pinned by tests so drift is caught.
///
/// Errors/warnings contain: "failed", "cannot", "required", "not wired",
/// "not created", "unavailable", or the strip-warning for local names.
pub(crate) fn status_is_warning(status: &str) -> bool {
    const WARNING_MARKERS: [&str; 7] = [
        "failed",
        "cannot",
        "required",
        "not wired",
        "not created",
        "unavailable",
        "will be stripped",
    ];
    WARNING_MARKERS.iter().any(|marker| status.contains(marker))
}

/// The backend mode string sent to `swarm_ai_assist` for a form surface.
/// Reads ONLY the named surface's own target toggle: a tuple-match that also
/// consulted the author form's target for the swarm surface would send ABW
/// guidance to a Local compose (and vice versa). Pinned by tests including
/// the exact hole (`swarm` + compose Cloud + author Local → `abw`).
fn ai_assist_mode(
    surface: &str,
    compose_target: CreateTarget,
    author_target: CreateTarget,
) -> &'static str {
    let target = if surface == "swarm" {
        compose_target
    } else {
        author_target
    };
    if target == CreateTarget::Local {
        "local"
    } else {
        "abw"
    }
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

/// R2: the algedonic + consent surface — wallet/ledger balances, in-flight
/// spend, the pending hire consent, and hire-flow errors. Grouped so the
/// spend/consent concern is one cohesive state object.
struct SpendState {
    in_flight: Option<String>,
    pending_hire: Option<PendingHire>,
    /// The operator's ABW credit wallet — the only spendable balance. Hires
    /// on cloud swarms draw from it, so it is tracked and always visible
    /// when known. Local swarms have no credit concept (the local ledger is
    /// accounting-only and is not surfaced here).
    wallet_balance: Option<i64>,
    hire_error: Option<SharedString>,
}

/// R2: the browse-mode drill-down state — the swarm roster detail view, the
/// run-status strip, the add-agent editor, the edit-metadata editors, and the
/// pending destructive-action confirmation. Only active in Browse mode.
struct DetailState {
    swarm_detail: Option<SwarmDetailView>,
    run_status: Option<RunStatusView>,
    add_agent_editor: Entity<Editor>,
    /// Editors for the edit-metadata form (local swarms only). Reused across
    /// opens; populated from the loaded swarm when edit mode is entered.
    edit_name_editor: Entity<Editor>,
    edit_mission_editor: Entity<Editor>,
    /// A destructive action awaiting explicit confirmation (delete swarm,
    /// remove/fire agent). When `Some`, the detail view renders a
    /// confirmation banner with Confirm / Cancel buttons instead of firing
    /// immediately.
    pending_destructive: Option<DestructiveAction>,
}

/// A destructive action pending operator confirmation. The two-step pattern
/// prevents accidental irreversible ops (delete swarm, fire/remove agent).
#[derive(Clone, Debug)]
enum DestructiveAction {
    DeleteSwarm {
        swarm_id: String,
        source: AgentSource,
        name: String,
    },
    RemoveAgent {
        swarm_id: String,
        agent_id: String,
        source: AgentSource,
    },
}

/// R2: AI Assist / validation state — shared by the Author and Compose
/// surfaces (suggestions + validation verdict from `swarm_ai_assist`).
struct AiAssistState {
    busy: bool,
    action: Option<String>,
    suggestions: Option<AiSuggestions>,
    validation: Option<ValidationResult>,
}

/// R2: the publish-flow state — the pending publish consent and the
/// force-publish reason editor.
struct PublishState {
    pending: Option<PendingPublish>,
    reason: Entity<Editor>,
}

pub struct SwarmPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    fs: std::sync::Arc<dyn Fs>,
    list: UniformListScrollHandle,
    /// Scroll handle for the Author form's scrollable surface. The form is a
    /// tall vertical stack (name, type, description, system prompt, tags,
    /// visibility, valence, AI assist, action row) that overflows the panel
    /// height when editing an agent with a complex prompt. Without a tracked
    /// scroll handle the `overflow_y_scroll` div has no visible scrollbar and
    /// the operator cannot reach the fields below the fold — keyed off the
    /// same pattern `ConfigureContextServerModal` and `ThreadView` use.
    author_scroll: ScrollHandle,
    /// Scroll handle for the Compose form's scrollable surface. Same pattern
    /// as `author_scroll` — the compose form (name, mission, agents, Xaman Ek
    /// consultant, AI assist, action row) overflows on smaller panes.
    compose_scroll: ScrollHandle,
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
    filter: SwarmFilter,
    /// Browse-list source filter (All/Cloud/Local). Orthogonal to `filter`
    /// (kind). Drives the source half of `filter_entries`; the kind half is
    /// `filter`. Restored by re-adding the source filter control that
    /// `bc51229ffe` removed.
    source_filter: SourceFilter,
    entries: Vec<SwarmEntry>,
    filtered_entry_indices: Vec<usize>,
    query_editor: Entity<Editor>,
    _subscriptions: [gpui::Subscription; 2],
    search_task: Option<Task<()>>,
    /// The workspace (swarm) id new hires target. Defaults to the first
    /// workspace once swarms load; selectable when there are several.
    selected_workspace: Option<String>,
    /// Which surface is active: browse, author, compose, or steer.
    mode: PanelMode,
    /// The panel's last-used backend target — the context the Author and
    /// Compose forms initialize to. Carries the cloud/local choice across
    /// surfaces so the operator never re-answers a question they already
    /// answered (the "doesn't carry over" finding): Browse → Compose with
    /// the Local filter selected lands on a Local create target, and the
    /// Author form's reset keeps the target the operator last used. Both
    /// forms can still override per-form — this is only the default they
    /// start from. Seeded from the `kask.swarm.mode` setting so the panel
    /// opens consistent with the configured backend.
    active_backend: CreateTarget,
    /// Authoring form state.
    author: AuthorForm,
    /// A loaded agent detail waiting to be applied to the author form on the
    /// next `render` (which has `&mut Window` for `Editor::set_text`). Set by
    /// `load_agent_into_author`'s spawn; consumed by `apply_pending_author_load`.
    pending_author_load: Option<crate::agent_edit::AgentDetail>,
    /// Set by `delete_edited_agent`'s spawn on a successful delete. `render`
    /// consumes it (it has `&mut Window`, required by `Editor::clear` and
    /// `set_mode`) to reset the author form to a fresh create state and switch
    /// back to Browse. Mirrors the `pending_author_load` deferred-mutation
    /// pattern — the spawn closure cannot hold a `&mut Window` reference.
    pending_author_reset: bool,
    /// A composition prompt waiting to be injected into the Steer conversation
    /// on the next `render` (after `ensure_steer_conversation` constructs it).
    /// Set by `create_swarm`'s spawn on a successful create; consumed by
    /// `render`. Carries the `swarm_id`, `mission`, `agents`, and `is_local`
    /// flag so the prompt can be built with the correct `mode` context.
    pending_composition_prompt: Option<PendingCompositionPrompt>,
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
    /// Last-seen `kask.swarm.mode` value, used to detect mode changes that
    /// invalidate the Steer conversation's baked-in system prompt. The Steer
    /// prompt interpolates the mode at construction (`ensure_steer_conversation`),
    /// so a toggle after construction leaves the curator steering against the
    /// wrong backend. The `SettingsStore` observer compares against this and
    /// tears down the conversation when the mode actually changes.
    last_swarm_mode: Option<kask_bridge::SwarmModeConfig>,
    /// R2: spend/consent state (balances, in-flight spend, hire consent,
    /// hire-flow errors).
    spend: SpendState,
    /// R2: browse-mode drill-down state (roster detail, run-status strip,
    /// add-agent editor).
    detail: DetailState,
    /// R2: AI Assist / validation state.
    ai_assist: AiAssistState,
    /// R2: publish-flow state (pending consent + reason editor).
    publish: PublishState,
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
    /// Port labels this agent accepts (typed inputs). Empty when the backend
    /// does not carry them (local rosters are ids-only until enriched;
    /// ABW rosters may omit the field).
    accepts: Vec<String>,
    /// Port labels this agent produces (typed outputs).
    produces: Vec<String>,
}

/// The swarm roster drill-down: replaces the browse list while open.
#[derive(Clone, Debug)]
struct SwarmDetailView {
    workspace_id: String,
    name: String,
    /// The swarm's mission / description. Editable for local swarms via
    /// `swarm_update_local_swarm`; read-only for ABW swarms (ABW has no
    /// metadata-edit endpoint — PATCH /workspaces/{id} is 405).
    mission: String,
    /// Which substrate this swarm lives on. Drives the add/remove affordances:
    /// `Local` uses `swarm_add_agent_local` / `swarm_remove_agent_local` /
    /// `swarm_delete_local_swarm`; `Cloud` uses the consent-gated `swarm_hire`
    /// and `swarm_fire`.
    source: AgentSource,
    /// Number of hired agents, copied from `SwarmCard.agent_count` when the
    /// detail is opened. `None` for local swarms (no ABW budget signal) and
    /// when the ABW workspace payload omits the field — rendered as "-",
    /// never a fabricated "0 agents" (mirrors the `SwarmCard.agent_count`
    /// contract at `parse.rs:75-77`).
    agent_count: Option<u64>,
    /// Total workspace budget (credits), copied from `SwarmCard.budget`.
    /// `None` for local swarms and when ABW omits the field.
    budget: Option<u64>,
    /// Remaining workspace budget (credits), copied from `SwarmCard.remaining`.
    /// `None` for local swarms and when ABW omits the field.
    remaining: Option<u64>,
    loading: bool,
    error: Option<SharedString>,
    agents: Vec<SwarmRosterAgent>,
    /// Whether the metadata edit form (name + mission) is open. Local-only —
    /// ABW has no metadata-edit endpoint. Toggled by the "Edit" button in
    /// the detail header.
    editing_metadata: bool,
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
    /// Advisory findings (fermi Warning severity) — reported, never
    /// blocking. Includes the typed-tier notice and the LLM quality review.
    warnings: Vec<String>,
    /// Carrier for out-of-band notes (e.g. "advisory layer unavailable").
    notes: String,
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
            // Re-filter the browse list whenever settings change. The filter no
            // longer depends on `kask.swarm.mode` (both backends are always
            // shown), but settings changes can still affect the entry list
            // indirectly (e.g. an MCP server restart re-fetches), so the
            // observer stays as a re-render trigger.
            //
            // The observer also detects `kask.swarm.mode` changes and tears
            // down the Steer conversation — the Steer system prompt bakes the
            // mode in at construction (`ensure_steer_conversation`), so a
            // toggle would leave the curator steering against the wrong
            // backend. Mirrors `kanban_panel::select_board`, which tears down
            // on `selected_board_id` change for the same reason.
            let settings_sub = cx.observe_global::<SettingsStore>(|this, cx| {
                let mode = Self::current_swarm_mode(cx);
                if this.last_swarm_mode.as_ref() != Some(&mode) {
                    this.steer_conversation = None;
                    // The settings change is an explicit backend declaration —
                    // carry it into the panel's creation context too, and sync
                    // the open form so its toggle doesn't go stale (the
                    // stale-toggle gap: `active_backend` alone would leave the
                    // visible toggle on the old backend until the next mode
                    // switch).
                    this.active_backend = match &mode {
                        kask_bridge::SwarmModeConfig::Local => CreateTarget::Local,
                        kask_bridge::SwarmModeConfig::Abw => CreateTarget::Cloud,
                    };
                    this.sync_open_form_target();
                    this.last_swarm_mode = Some(mode);
                }
                this.filter_entries(cx);
            });
            let subscriptions = [query_sub, settings_sub];

            // Seed the panel's backend context from `kask.swarm.mode` so the
            // Author and Compose forms open consistent with the configured
            // backend rather than always defaulting to Cloud.
            let active_backend = match Self::current_swarm_mode(cx) {
                kask_bridge::SwarmModeConfig::Local => CreateTarget::Local,
                kask_bridge::SwarmModeConfig::Abw => CreateTarget::Cloud,
            };

            let scroll_handle = UniformListScrollHandle::new();
            let author_scroll = ScrollHandle::new();
            let compose_scroll = ScrollHandle::new();

            let mut author = AuthorForm::new(window, cx);
            let mut compose = ComposeForm::new(window, cx);
            // Both forms start on the panel's backend context (seeded from
            // `kask.swarm.mode` above), not a hardcoded Cloud default —
            // otherwise a `kask.swarm.mode: local` operator lands on a Cloud
            // create target on first visit and the context is lost again.
            author.create_target = active_backend;
            compose.create_target = active_backend;
            let swarm_add_agent_editor = cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Agent id to add to this swarm", window, cx);
                e
            });
            let edit_name_editor = cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Swarm name", window, cx);
                e
            });
            let edit_mission_editor = cx.new(|cx| {
                let mut e = Editor::single_line(window, cx);
                e.set_placeholder_text("Mission", window, cx);
                e
            });

            let mut this = Self {
                workspace: workspace_handle,
                project,
                fs,
                list: scroll_handle,
                author_scroll,
                compose_scroll,
                in_flight: 0,
                agents_error: None,
                swarms_error: None,
                cloud_authenticated: None,
                retry_task: None,
                retry_attempt: 0,
                filter: SwarmFilter::All,
                source_filter: SourceFilter::All,
                entries: Vec::new(),
                filtered_entry_indices: Vec::new(),
                query_editor,
                _subscriptions: subscriptions,
                search_task: None,
                selected_workspace: None,
                mode: PanelMode::Browse,
                active_backend,
                author,
                pending_author_load: None,
                pending_author_reset: false,
                pending_composition_prompt: None,
                compose,
                steer_conversation: None,
                steer_connection_store: None,
                last_swarm_mode: Some(Self::current_swarm_mode(cx)),
                spend: SpendState {
                    in_flight: None,
                    pending_hire: None,
                    wallet_balance: None,
                    hire_error: None,
                },
                detail: DetailState {
                    swarm_detail: None,
                    run_status: None,
                    add_agent_editor: swarm_add_agent_editor,
                    edit_name_editor,
                    edit_mission_editor,
                    pending_destructive: None,
                },
                ai_assist: AiAssistState {
                    busy: false,
                    action: None,
                    suggestions: None,
                    validation: None,
                },
                publish: PublishState {
                    pending: None,
                    reason: publish_reason,
                },
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

    /// Re-fetch now, cancelling any pending automatic retry and resetting the
    /// backoff. Bound to the refresh affordance.
    pub(crate) fn refresh_now(&mut self, cx: &mut Context<Self>) {
        self.retry_task = None;
        self.retry_attempt = 0;
        self.fetch_all(cx);
    }

    /// Schedule a re-fetch after a retryable failure, with exponential backoff.
    ///
    /// Called by the fetchers when a failure is transport-level rather than
    /// semantic. Backoff is capped at [`MAX_FETCH_RETRIES`] attempts so a
    /// genuinely broken server produces a bounded number of retries and then a
    /// stable visible error, rather than an unbounded poll.
    pub(crate) fn schedule_fetch_retry(&mut self, cx: &mut Context<Self>) {
        if self.retry_task.is_some() {
            // A retry is already pending; the sibling fetch's failure rides on it
            // rather than stacking a second timer.
            return;
        }
        let Some(delay) = fetch_retry_delay(self.retry_attempt) else {
            log::warn!(
                "swarm-panel: giving up after {MAX_FETCH_RETRIES} retries — \
                 use the Retry button once the MCP server is available"
            );
            return;
        };
        self.retry_attempt += 1;
        log::info!(
            "swarm-panel: fetch failed transiently — retrying in {}s (attempt {}/{})",
            delay.as_secs(),
            self.retry_attempt,
            MAX_FETCH_RETRIES
        );
        self.retry_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                this.retry_task = None;
                this.fetch_all(cx);
            })
            .ok();
        }));
    }

    /// Clear the retry backoff after a successful fetch, so a later transient
    /// failure starts from the short delay again.
    pub(crate) fn note_fetch_success(&mut self) {
        self.retry_attempt = 0;
    }

    /// Classify a swarm-list fetch failure and update `swarms_error`.
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
    /// API-key status is read from `cloud_authenticated` (the `authenticated`
    /// field the server returns in the `swarm_list_agents` response), NOT from
    /// the error message. The error message conflates "no key configured"
    /// (from `require_auth()`) with "key configured but rejected by ABW"
    /// (from the ABW 401 body) — both surface as `permission_denied`, and a
    /// 401 body can contain "no API key" text, which would misclassify a
    /// rejected key as "not configured." The `authenticated` field is the
    /// server's own report of whether `ctx.credentials` has the key, so it is
    /// the same source the MCP server uses.
    pub(crate) fn handle_swarm_fetch_failure(
        &mut self,
        retryable: bool,
        kind: Option<hkask_types::McpErrorKind>,
        message: &str,
        cx: &mut Context<Self>,
    ) {
        if retryable {
            self.swarms_error =
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
            // Distinguish the two causes using `cloud_authenticated` (the
            // server's own report of whether the key is configured) rather
            // than the error message. The message-based check
            // (`message.contains("no API key")`) was a false positive when ABW
            // returned a 401 body containing "no API key" text for a key that
            // WAS configured but rejected — the panel showed "no ABW API key
            // configured" even though the key was present.
            //
            // `cloud_authenticated` is set by the `swarm_list_agents` fetch,
            // which is sequenced BEFORE the `swarm_get_swarm` fetch in the same
            // task (see `fetch_all`). So by the time this runs,
            // `cloud_authenticated` is `Some(_)` in the normal case. `None` is
            // a defensive fallback (e.g. the agents fetch failed before
            // reaching the parse step) — treat it as "key not confirmed" rather
            // than guessing.
            let status: SharedString = match self.cloud_authenticated {
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
            self.swarms_error = Some(status);
            log::warn!("swarm-panel: swarm list unavailable (agents-only mode): {message}");
        } else {
            log::warn!("swarm-panel: could not fetch workspaces (agents-only mode): {message}");
        }
        self.filter_entries(cx);
    }

    fn set_mode(&mut self, mode: PanelMode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        // Entering a creation surface syncs its form target to the panel's
        // backend context (see `target_on_surface_entry`).
        self.sync_open_form_target();
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

    /// Sync the active creation form's target to `active_backend` (see
    /// `target_on_surface_entry` for the rule). Called on surface entry
    /// (`set_mode`) and when `kask.swarm.mode` changes — otherwise a settings
    /// change updates `active_backend` while the open form's toggle keeps
    /// showing the stale backend until the next mode switch (the stale-toggle
    /// gap).
    fn sync_open_form_target(&mut self) {
        if let Some(target) = target_on_surface_entry(
            self.mode,
            self.author.editing_id.is_some(),
            self.active_backend,
        ) {
            match self.mode {
                PanelMode::Author => self.author.create_target = target,
                PanelMode::Compose => self.compose.create_target = target,
                PanelMode::Browse | PanelMode::Steer => {}
            }
        }
    }

    /// Reset the author form to a fresh create state (clear `editing_id`,
    /// make the name field editable again, clear the status). Called when the
    /// operator clicks the Author mode toggle in the header — distinct from
    /// `load_agent_into_author`, which sets `editing_id` and read-only before
    /// calling `set_mode`.
    fn reset_author_form_for_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.author.editing_id = None;
        self.author.editing_source = None;
        // Keep the panel's backend context rather than resetting to Cloud —
        // the operator's last cloud/local choice carries into the next
        // authoring session (the "doesn't carry over" finding).
        self.author.create_target = self.active_backend;
        self.author.status = None;
        self.author.name.update(cx, |e, _| e.set_read_only(false));
        // Clear the text fields so the operator starts fresh.
        self.author.name.update(cx, |e, cx| e.clear(window, cx));
        self.author
            .description
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .system_prompt
            .update(cx, |e, cx| e.clear(window, cx));
        self.author.tags.update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_arousal
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_valence
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_primary_affect
            .update(cx, |e, cx| e.clear(window, cx));
        self.author
            .valence_personality_traits
            .update(cx, |e, cx| e.clear(window, cx));
        self.author.agent_type = "research".to_string();
        self.author.visibility = "private".to_string();
    }

    /// Read the current swarm mode from `kask.swarm.mode` settings. Returns
    /// `Abw` when unset (the default). The panel reads the mode here (not
    /// from the MCP server) because the server's mode is derived from the
    /// same setting via env vars — the setting is the single source of truth.
    /// Used by the header mode toggle to show the active backend.
    fn current_swarm_mode(cx: &mut Context<Self>) -> kask_bridge::SwarmModeConfig {
        kask_bridge::KaskSettings::get_global(cx).swarm.mode.clone()
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
            agent::CuratorAgentServer::new(self.fs.clone(), thread_store.clone())
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
                Some(thread_store),
                AgentThreadSource::AgentPanel,
                window,
                cx,
            )
        });

        self.steer_conversation = Some(conversation_view);
    }

    /// Launch the pending swarm-intelligence plan by injecting a message into
    /// the Steer conversation that tells the curator to execute the plan via
    /// `swarm_execute_plan_local` and feed the results back. Uses the D21
    /// `ConversationInjector` seam — the same mechanism viz widgets use to
    /// compose back into the thread. The operator reviews the injected message
    /// and submits via the existing Send button so the turn-loop's
    /// checkpoints/telemetry are preserved.
    fn launch_plan_in_steer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.selected_workspace.clone().unwrap_or_default();
        let message = format!(
            "Execute the plan above using `swarm_execute_plan_local` with the \
             swarm_id `{workspace}`. After the results return, feed the \
             `delegate_results` array back into the `swarm-intelligence` skill \
             so C5 (fault attribution) and C6 (reconfigure) can close the loop."
        );
        // The D21 injector is a process-global that the active ConversationView
        // publishes. If the Steer conversation is active, this injects the
        // message into its editor; the operator reviews and sends.
        if let Some(injector) = hkask_conversation_injector::shared_injector(cx) {
            let task = injector.inject(message, window, cx);
            cx.spawn(async move |_this, _cx| {
                if let Err(error) = task.await {
                    log::warn!("swarm-panel: launch-plan inject failed: {error}");
                }
            })
            .detach();
        } else {
            log::warn!("swarm-panel: no active conversation injector — is Steer mode active?");
        }
    }

    /// Inject the composition prompt into the Steer conversation after a
    /// successful `create_swarm`. The prompt carries the `mode` and `swarm_id`
    /// as leading `key=value` pairs (parsed by the `SkillTool` into the
    /// cascade context) and the mission + seeded agents as the task text so
    /// SENSE can derive `required_transforms` and assess the initial roster.
    /// The operator reviews and sends — the turn-loop's checkpoints/telemetry
    /// are preserved (same D21 injector mechanism as `launch_plan_in_steer`).
    fn inject_composition_prompt(
        &mut self,
        swarm_id: &str,
        mission: &str,
        agents: &[String],
        is_local: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = if is_local { "local" } else { "abw" };
        let agent_label = if is_local {
            "Seeded agents"
        } else {
            "Hired agents"
        };
        let agents_str = agents.join(", ");
        let message = format!(
            "/swarm-intelligence mode={mode} swarm_id={swarm_id} compose my swarm. \
             Mission: {mission}. {agent_label}: {agents_str}."
        );
        if let Some(injector) = hkask_conversation_injector::shared_injector(cx) {
            let task = injector.inject(message, window, cx);
            cx.spawn(async move |_this, _cx| {
                if let Err(error) = task.await {
                    log::warn!("swarm-panel: composition-prompt inject failed: {error}");
                }
            })
            .detach();
        } else {
            log::warn!(
                "swarm-panel: no active conversation injector — composition prompt not injected"
            );
        }
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
        let is_local = self.author.create_target == CreateTarget::Local;
        // Target-aware slug pre-validation. ABW requires `^[a-z0-9_]{3,64}$` —
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
        // fermi contract fields: sample queries (one per line — they contain
        // commas) and the accepts/produces composition ports (CSV).
        let sample_queries: Vec<String> = self
            .author
            .sample_queries
            .read(cx)
            .text(cx)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let accepts: Vec<String> = self
            .author
            .accepts
            .read(cx)
            .text(cx)
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let produces: Vec<String> = self
            .author
            .produces
            .read(cx)
            .text(cx)
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
                            "sample_queries": sample_queries,
                            "accepts": accepts,
                            "produces": produces,
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
                            "sample_queries": sample_queries,
                            "accepts": accepts,
                            "produces": produces,
                        }),
                    )
                    .await
            };
            this.update(cx, |this, cx| {
                this.author.busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.spend.wallet_balance = Some(b);
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
        let is_local = self.compose.create_target == CreateTarget::Local;

        // Launch gate: a swarm is not ready for the swarm-intelligence PDCA
        // loop unless it has a mission (the task context SENSE derives
        // required_transforms from) and at least MIN_AGENTS_TO_LAUNCH agents
        // (below that, variety_coverage and diversity are trivially 0/1 and
        // the loop converges without doing composition work). The operator
        // must complete the compose form before creating.
        if mission.trim().is_empty() {
            self.compose.status = Some(
                "Mission is required to launch a swarm. Describe what the swarm should do.".into(),
            );
            cx.notify();
            return;
        }
        if agents.len() < MIN_AGENTS_TO_LAUNCH {
            self.compose.status = Some(format!(
                "At least {} agents are required to launch a swarm. Add agents to the roster ({} provided).",
                MIN_AGENTS_TO_LAUNCH,
                agents.len()
            ).into());
            cx.notify();
            return;
        }

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
                this.update_in(cx, |this, window, cx| {
                    this.compose.busy = false;
                    match result {
                        Ok(output) => {
                            // Extract the swarm_id from the response so we can
                            // select it and navigate to Steer.
                            let swarm_id = parse_tool_response(&output)
                                .and_then(|c| c.get("swarm_id").and_then(|v| v.as_str()).map(str::to_string));
                            this.compose.status =
                                Some(format!("Local swarm '{}' created.", name.trim()).into());
                            this.fetch_all(cx);
                            // Navigate to Steer with the new swarm selected.
                            if let Some(id) = swarm_id {
                                this.selected_workspace = Some(id.clone());
                                // Drop any existing Steer conversation so the
                                // next construction bakes in the new swarm.
                                this.steer_conversation = None;
                                this.set_mode(PanelMode::Steer, window, cx);
                                // Queue the composition prompt for injection
                                // after `render` constructs the Steer
                                // conversation. The prompt carries the mode,
                                // swarm_id, mission, and seeded agents so
                                // swarm-intelligence SENSE can derive
                                // required_transforms and assess the initial
                                // roster.
                                this.pending_composition_prompt =
                                    Some(PendingCompositionPrompt {
                                        swarm_id: id,
                                        mission: mission.trim().to_string(),
                                        agents: agents.clone(),
                                        is_local: true,
                                    });
                            }
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
            this.update_in(cx, |this, window, cx| {
                this.compose.busy = false;
                match result {
                    Ok(output) => {
                        if let Some(b) = extract_wallet_balance(&output) {
                            this.spend.wallet_balance = Some(b);
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
                        // Extract the workspace_id so we can select it and
                        // navigate to Steer.
                        let workspace_id = parse_tool_response(&output)
                            .and_then(|c| c.get("workspace_id").and_then(|v| v.as_str()).map(str::to_string));
                        this.fetch_all(cx);
                        // Navigate to Steer with the new swarm selected.
                        if let Some(id) = workspace_id {
                            this.selected_workspace = Some(id.clone());
                            this.steer_conversation = None;
                            this.set_mode(PanelMode::Steer, window, cx);
                            // Queue the composition prompt for injection
                            // (same as the local path).
                            this.pending_composition_prompt =
                                Some(PendingCompositionPrompt {
                                    swarm_id: id,
                                    mission: mission.trim().to_string(),
                                    agents: agents.clone(),
                                    is_local: false,
                                });
                        }
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
                            this.spend.wallet_balance = Some(b);
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
        // The backend mode sent to `swarm_ai_assist` must match the surface's
        // own target toggle — reading the author form's target for the swarm
        // surface sent ABW guidance to a Local compose (and vice versa).
        let mode = ai_assist_mode(
            surface,
            self.compose.create_target,
            self.author.create_target,
        );

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
        // Agent-surface contract fields (fermi `agent_contract`): tags,
        // sample queries, accepts/produces, valence presence. Sent on both
        // actions so `suggest` can propose them and `validate` can check
        // them against the deterministic contract.
        let (tags, sample_queries, accepts, produces, has_valence) = if surface == "agent" {
            let arousal = self.author.valence_arousal.read(cx).text(cx);
            let valence = self.author.valence_valence.read(cx).text(cx);
            let affect = self.author.valence_primary_affect.read(cx).text(cx);
            let traits = self.author.valence_personality_traits.read(cx).text(cx);
            (
                self.author.tags.read(cx).text(cx),
                self.author.sample_queries.read(cx).text(cx),
                self.author.accepts.read(cx).text(cx),
                self.author.produces.read(cx).text(cx),
                !arousal.trim().is_empty()
                    || !valence.trim().is_empty()
                    || !affect.trim().is_empty()
                    || !traits.trim().is_empty(),
            )
        } else {
            (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                false,
            )
        };

        self.ai_assist.busy = true;
        self.ai_assist.action = Some(action.to_string());
        // Clear stale banners so the operator doesn't see the previous result
        // while a new call is in flight (mirrors the Xaman Ek stale-suggestion
        // fix, L5).
        self.ai_assist.suggestions = None;
        self.ai_assist.validation = None;
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
                        "tags": tags,
                        "sample_queries": sample_queries,
                        "accepts": accepts,
                        "produces": produces,
                        "has_valence": has_valence,
                    }),
                )
                .await;
            this.update(cx, |this, cx| {
                this.ai_assist.busy = false;
                this.ai_assist.action = None;
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
                                this.ai_assist.suggestions = Some(AiSuggestions {
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
                                // Advisory tier (fermi Warning severity): reported
                                // but never blocking. Includes the deterministic
                                // typed-tier notice and the LLM's quality review.
                                let warnings = content
                                    .get("warnings")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|v| v.as_str().map(str::to_string))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let notes = content
                                    .get("notes")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                this.ai_assist.validation = Some(ValidationResult {
                                    surface: surface_owned.clone(),
                                    valid,
                                    issues,
                                    warnings,
                                    notes,
                                });
                            };
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
        let Some(s) = self.ai_assist.suggestions.clone() else {
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
        self.ai_assist.suggestions = None;
        cx.notify();
    }

    /// Dismiss the AI Assist suggestions banner without applying.
    fn dismiss_ai_suggestions(&mut self, cx: &mut Context<Self>) {
        self.ai_assist.suggestions = None;
        cx.notify();
    }

    /// Dismiss the validation banner.
    fn dismiss_validation(&mut self, cx: &mut Context<Self>) {
        self.ai_assist.validation = None;
        cx.notify();
    }

    /// Filter the browse entries by the active `SwarmFilter` (All/Swarms/Agents),
    /// the search query, and the active `SourceFilter` (All/Cloud/Local).
    /// Kind and source are orthogonal axes — both are applied. The source
    /// filter restores the cloud/local browse separation that `bc51229ffe`
    /// removed when it moved backend selection onto the per-form
    /// `CreateTarget` (which only affects creation, not browsing). `Synced`
    /// entries (cards present in both backends, linked by `cloud_swarm_id`) appear
    /// under both `Cloud` and `Local` — they are visible on either backend, so
    /// hiding them from either view would be a regression. This matches the
    /// original `kask.swarm.mode`-driven behavior, minus the settings
    /// round-trip: the filter is in-memory state, toggled from the header.
    fn filter_entries(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter;
        let source_filter = self.source_filter;
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
                let source_matches = match source_filter {
                    SourceFilter::All => true,
                    SourceFilter::Cloud => match entry {
                        SwarmEntry::Agent(a) => {
                            a.source == AgentSource::Cloud || a.source == AgentSource::Synced
                        }
                        SwarmEntry::Swarm(s) => s.source == AgentSource::Cloud,
                    },
                    SourceFilter::Local => match entry {
                        SwarmEntry::Agent(a) => {
                            a.source == AgentSource::Local || a.source == AgentSource::Synced
                        }
                        SwarmEntry::Swarm(s) => s.source == AgentSource::Local,
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
        window: &mut Window,
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
            cards.push(self.render_card(entry, window, cx));
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
            // When the API key is not configured, the empty-state messages
            // suggest setting it. When it IS configured (or status is unknown),
            // the hint is dropped — the operator genuinely has no swarms, not
            // a missing-key problem.
            let key_hint = match self.cloud_authenticated {
                Some(false) => " or set HKASK_ABW_API_KEY to see your cloud swarms",
                _ => "",
            };
            let message: SharedString = match self.filter {
                SwarmFilter::All => {
                    if has_search {
                        "No agents or swarms that match your search.".into()
                    } else {
                        format!("No agents or swarms. Create one (Author/Compose){key_hint}.")
                            .into()
                    }
                }
                SwarmFilter::Swarms => {
                    if has_search {
                        "No swarms that match your search.".into()
                    } else {
                        format!("No swarms. Compose one (Compose) to group agents{key_hint}.")
                            .into()
                    }
                }
                SwarmFilter::Agents => {
                    if has_search {
                        "No agents that match your search.".into()
                    } else {
                        "No agents. Create one (Author), or clone a cloud agent to Local.".into()
                    }
                }
            };
            message
        };

        marketplace_empty_state(message, self.visible_error().is_some())
    }

    /// The cost/consent gate banner. Renders only when a hire is pending
    /// operator authorization. Shows the pre-flight estimate and blocks the
    /// spend until the operator explicitly confirms or cancels — the
    /// enforcement point for the `.rules` "advertised invariants need
    /// enforcement points" trap (the gate blocks, it does not just warn).
    fn render_consent_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.spend.pending_hire.clone()?;
        let border = cx.theme().colors().border;

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
                ),
        )
    }

    // ── AI Assist render helpers ───────────────────────────────────────
    //
    // `render_ai_assist_row` is the two-button row (AI Assist / Validate)
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
        let disabled = form_busy || self.ai_assist.busy;
        let busy_label = match self.ai_assist.action.as_deref() {
            Some("validate") => "Validating…",
            Some("suggest") => "Assisting…",
            _ => "",
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                Button::new(format!("ai-assist-{surface}"), "AI Assist")
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
                Button::new(format!("ai-validate-{surface}"), "Validate")
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
        let s = self.ai_assist.suggestions.clone()?;
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
                            .child(Label::new("AI Assist").color(Color::Accent))
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
                        .child(Label::new("AI Assist").color(Color::Accent))
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
        let v = self.ai_assist.validation.clone()?;
        if v.surface != surface {
            return None;
        }
        let border = cx.theme().colors().border;
        let header = if v.valid {
            "Validation passed"
        } else {
            "Validation found issues"
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
                                Label::new("Meets the ABW composition contract.")
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
                // Advisory tier — fermi Warning severity: worth fixing, never
                // blocking. Rendered muted so the visual weight stays on the
                // contract failures above.
                .when(!v.warnings.is_empty(), |this| {
                    this.child(
                        v_flex()
                            .gap_0p5()
                            .children(v.warnings.iter().map(|warning| {
                                Label::new(format!("◦ {warning}"))
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                            })),
                    )
                })
                .when(!v.notes.is_empty(), |this| {
                    this.child(
                        Label::new(v.notes.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
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
                ),
        )
    }

    /// The publish banner (fermi v0.10.15). Mirrors the consent banner: a
    /// Confirm path when `can_publish`, or a force-publish path listing the
    /// failing checks plus a reason input (audited to `admin_bypass_events`).
    fn render_publish_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let pending = self.publish.pending.clone()?;
        let border = cx.theme().colors().border;
        let busy = self.spend.in_flight.is_some();

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
                                    .child(self.publish.reason.clone()),
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
                ),
        )
    }
}

impl Render for SwarmPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Apply a pending agent load (set by `load_agent_into_author`'s spawn)
        // to the author form. Deferred to `render` because `Editor::set_text`
        // requires `&mut Window`, which the spawn closure does not have.
        if self.pending_author_load.is_some() {
            self.apply_pending_author_load(window, cx);
        }
        // Consume a pending author-form reset (set by `delete_edited_agent`'s
        // spawn on a successful delete). Deferred to `render` for the same
        // reason — `Editor::clear` and `set_mode` need `&mut Window`.
        if self.pending_author_reset {
            self.pending_author_reset = false;
            self.reset_author_form_for_create(window, cx);
            self.set_mode(PanelMode::Browse, window, cx);
        }
        // If deserialized into Steer mode (or the operator switched via a
        // path that didn't go through the toggle handler), ensure the
        // conversation exists before rendering.
        if matches!(self.mode, PanelMode::Steer) {
            self.ensure_steer_conversation(window, cx);
        }
        // Consume a pending composition prompt (set by `create_swarm`'s spawn
        // on a successful create). Deferred to `render` because the D21
        // injector needs `&mut Window`, and the Steer conversation must exist
        // before injection. The prompt is injected into the conversation's
        // editor; the operator reviews and sends.
        if let Some(prompt) = self.pending_composition_prompt.take() {
            self.inject_composition_prompt(
                &prompt.swarm_id,
                &prompt.mission,
                &prompt.agents,
                prompt.is_local,
                window,
                cx,
            );
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
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    // The algedonic channel: the operator's ABW credit
                                    // balance is always visible when known, so a spend
                                    // never happens out of sight. Hidden when unknown
                                    // (unauthenticated) — never a fabricated zero.
                                    // Scoped "ABW" because credits are a cloud concept
                                    // only: local swarms have no credit budget, so no
                                    // local balance is shown here.
                                    .when_some(self.spend.wallet_balance, |this, balance| {
                                        this.child(
                                            Label::new(format!("{balance} ABW credits"))
                                                .size(LabelSize::Small)
                                                .color(if balance <= 0 {
                                                    Color::Warning
                                                } else {
                                                    Color::Muted
                                                }),
                                        )
                                    })
                                    // Always-visible manual refresh, mirroring the
                                    // kanban panel's refresh button. Automatic retries
                                    // are bounded (MAX_FETCH_RETRIES); once exhausted,
                                    // the operator needs a way back without closing
                                    // and reopening the panel. Disabled while a fetch
                                    // is in flight to prevent duplicate dispatch.
                                    .child(
                                        IconButton::new("swarm-refresh", IconName::RotateCw)
                                            .icon_size(IconSize::Small)
                                            .icon_color(Color::Muted)
                                            .disabled(self.is_fetching())
                                            .tooltip(Tooltip::text(
                                                "Refresh agents and swarms",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_now(cx);
                                            })),
                                    ),
                            ),
                    )
                    .children(self.render_consent_banner(cx))
                    .children(self.render_publish_banner(cx))
                    // Hire-flow errors surface near the consent banner.
                    .when_some(self.spend.hire_error.clone(), |this, err| {
                        this.child(
                            Label::new(err)
                                .size(LabelSize::Small)
                                .color(Color::Warning),
                        )
                    })
                    // Fetch errors surface as a status strip whenever present,
                    // not only in the empty state (the M3 partial-degradation
                    // finding — a working list can hide a failed source).
                    //
                    // The strip carries a manual retry: automatic retries are
                    // bounded (`MAX_FETCH_RETRIES`), so once they are exhausted the
                    // operator needs a way back without closing and reopening the
                    // panel. Two elements in this row (label + button), measured
                    // against the `ui-layout-discipline` congestion rule.
                    .when_some(self.visible_error().cloned(), |this, err| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Label::new(format!("Load warning: {err}"))
                                        .size(LabelSize::XSmall)
                                        .color(Color::Warning),
                                )
                                .child(
                                    Button::new("swarm-retry-fetch", "Retry")
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::XSmall)
                                        .disabled(self.is_fetching())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.refresh_now(cx);
                                        })),
                                ),
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
                                            this.reset_author_form_for_create(window, cx);
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
                            .size(ToggleButtonGroupSize::Custom(rems_from_px(30.0_f32)))
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
                                        .size(ToggleButtonGroupSize::Custom(rems_from_px(30.0_f32)))
                                        .label_size(LabelSize::Default)
                                        .auto_width()
                                        .selected_index(match self.filter {
                                            SwarmFilter::All => 0,
                                            SwarmFilter::Swarms => 1,
                                            SwarmFilter::Agents => 2,
                                        })
                                        .into_any_element(),
                                    ),
                                )
                                // Source filter (All/Cloud/Local) — orthogonal to the
                                // kind filter above. Restores the cloud/local browse
                                // separation removed by `bc51229ffe`. `All` shows
                                // every entry; `Cloud` shows cloud + synced agents
                                // and cloud swarms; `Local` shows local + synced
                                // agents and local swarms. Synced cards appear in
                                // both because they exist on both backends.
                                //
                                // Choosing Cloud/Local here also updates the
                                // panel's backend context so the choice carries
                                // into the Author/Compose forms (Browse →
                                // Compose lands on the same backend).
                                .child(
                                    div().child(
                                        ToggleButtonGroup::single_row(
                                            "swarm-source-filter-buttons",
                                            [
                                                ToggleButtonSimple::new(
                                                    "All",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.source_filter = SourceFilter::All;
                                                        this.filter_entries(cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Cloud",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.source_filter = SourceFilter::Cloud;
                                                        this.active_backend = CreateTarget::Cloud;
                                                        this.filter_entries(cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                                ToggleButtonSimple::new(
                                                    "Local",
                                                    cx.listener(|this, _event, _, cx| {
                                                        this.source_filter = SourceFilter::Local;
                                                        this.active_backend = CreateTarget::Local;
                                                        this.filter_entries(cx);
                                                        this.scroll_to_top(cx);
                                                    }),
                                                ),
                                            ],
                                        )
                                        .style(ToggleButtonGroupStyle::Outlined)
                                        .size(ToggleButtonGroupSize::Custom(rems_from_px(30.0_f32)))
                                        .label_size(LabelSize::Default)
                                        .auto_width()
                                        .selected_index(match self.source_filter {
                                            SourceFilter::All => 0,
                                            SourceFilter::Cloud => 1,
                                            SourceFilter::Local => 2,
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
                    .flex_1()
                    .min_h_0()
                    .overflow_y_hidden()
                    .map(|this| match self.mode {
                        // Canonical Zed scroll pattern (see settings_ui
                        // `render_nav` / skill_creator `render`): the scrollbar
                        // host is a plain `div` and the scrolling element is a
                        // column-flex child that fills it. Do NOT wrap the
                        // scroll host in `h_flex()` — `h_flex` applies
                        // `items_center()`, which collapses the scroll host to
                        // its content height and centers it, floating the form
                        // mid-panel with blank space above it.
                        PanelMode::Author => this
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .vertical_scrollbar_for(&self.author_scroll, window, cx)
                                    .child(
                                        v_flex()
                                            .id("author-scroll")
                                            .size_full()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.author_scroll)
                                            .child(self.render_author(cx)),
                                    ),
                            )
                            .into_any_element(),
                        PanelMode::Compose => this
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .w_full()
                                    .vertical_scrollbar_for(&self.compose_scroll, window, cx)
                                    .child(
                                        v_flex()
                                            .id("compose-scroll")
                                            .size_full()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.compose_scroll)
                                            .child(self.render_compose(cx)),
                                    ),
                            )
                            .into_any_element(),
                        PanelMode::Steer => {
                            // The `ConversationView` is lazily constructed
                            // in the Steer toggle handler; render it here. If
                            // it's somehow absent (e.g. the panel was
                            // deserialized into Steer mode), render a
                            // placeholder — the operator can re-click Steer.
                            let has_workspace = self.selected_workspace.is_some();
                            let launch_button = h_flex()
                                .w_full()
                                .gap_2()
                                // py only — the content column already carries
                                // the panel's px_4 inset, so px_4 here doubled it.
                                .py_1()
                                .child(
                                    Button::new("swarm-launch-plan", "Launch Plan")
                                        .style(ButtonStyle::Subtle)
                                        .label_size(LabelSize::Small)
                                        .disabled(!has_workspace)
                                        .tooltip(Tooltip::text(
                                            "Execute the pending swarm-intelligence plan via \
                                             swarm_execute_plan_local and feed the results back. \
                                             The curator will run the plan, stamp task-success \
                                             verdicts, and re-invoke with delegate_results."
                                        ))
                                        .on_click(cx.listener(|this, _event, window, cx| {
                                            this.launch_plan_in_steer(window, cx);
                                        })),
                                );
                            match &self.steer_conversation {
                                Some(view) => this
                                    .child(launch_button)
                                    .child(view.clone())
                                    .into_any_element(),
                                None => this
                                    .child(launch_button)
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
                                .when_some(self.detail.run_status.clone(), |this, status| {
                                    this.child(self.render_run_status_strip(&status, cx))
                                })
                                .when_some(self.detail.swarm_detail.clone(), |this, detail| {
                                    this.child(self.render_swarm_detail(&detail, cx))
                                });
                            if self.detail.swarm_detail.is_some() {
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
    use crate::parse::{AgentListResponse, WorkspaceListResponse};

    // ── Fetch retry policy ──────────────────────────────────────────────────
    //
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

    // ── Backend context carry ────────────────────────────────────────────────
    //
    // The cloud/local choice used to be four disconnected states: the Compose
    // form's target (hardcoded Cloud default), the Author form's target
    // (hardcoded Cloud default), the Browse source filter, and the
    // `kask.swarm.mode` setting. Switching surfaces lost the choice — the
    // "doesn't carry over" finding. `active_backend` is now the single
    // context; these tests pin the sync rule.

    /// Entering Compose always syncs to the panel's backend context — the
    /// form's target must never silently reset to a hardcoded default.
    #[test]
    fn compose_entry_syncs_to_panel_backend() {
        assert_eq!(
            target_on_surface_entry(PanelMode::Compose, false, CreateTarget::Local),
            Some(CreateTarget::Local)
        );
        assert_eq!(
            target_on_surface_entry(PanelMode::Compose, true, CreateTarget::Cloud),
            Some(CreateTarget::Cloud),
            "compose has no editing state that could own the target instead"
        );
    }

    /// Entering Author syncs only when not editing. When an agent is loaded
    /// for edit, `load_agent_into_author` derives the target from the agent's
    /// source — the entry sync must not clobber it.
    #[test]
    fn author_entry_preserves_editing_source_target() {
        assert_eq!(
            target_on_surface_entry(PanelMode::Author, false, CreateTarget::Local),
            Some(CreateTarget::Local)
        );
        assert_eq!(
            target_on_surface_entry(PanelMode::Author, true, CreateTarget::Cloud),
            None,
            "editing derives the target from the agent's source — the entry \
             sync must leave it alone"
        );
    }

    /// Browse and Steer own no form target — entering them never syncs.
    #[test]
    fn browse_and_steer_entry_never_syncs() {
        assert_eq!(
            target_on_surface_entry(PanelMode::Browse, false, CreateTarget::Local),
            None
        );
        assert_eq!(
            target_on_surface_entry(PanelMode::Steer, false, CreateTarget::Local),
            None
        );
    }

    // ── AI Assist mode derivation ──────────────────────────────────────────
    //
    // `swarm_ai_assist` must be told the backend of the surface it is
    // advising. The original inline tuple-match had a hole: the arm
    // `(_, _, CreateTarget::Local)` let the AUTHOR form's target win for the
    // swarm surface, so a Local author form + Cloud compose sent "local"
    // guidance to a Cloud compose. These tests pin the surface-specific read.

    #[test]
    fn ai_assist_mode_reads_only_the_named_surface() {
        // The hole: swarm surface must ignore the author form's target.
        assert_eq!(
            ai_assist_mode("swarm", CreateTarget::Cloud, CreateTarget::Local),
            "abw"
        );
        // And symmetrically: agent surface must ignore the compose form's target.
        assert_eq!(
            ai_assist_mode("agent", CreateTarget::Local, CreateTarget::Cloud),
            "abw"
        );
        // Each surface reads its own target.
        assert_eq!(
            ai_assist_mode("swarm", CreateTarget::Local, CreateTarget::Cloud),
            "local"
        );
        assert_eq!(
            ai_assist_mode("agent", CreateTarget::Cloud, CreateTarget::Local),
            "local"
        );
        // Cloud on both surfaces.
        assert_eq!(
            ai_assist_mode("swarm", CreateTarget::Cloud, CreateTarget::Cloud),
            "abw"
        );
        assert_eq!(
            ai_assist_mode("agent", CreateTarget::Cloud, CreateTarget::Cloud),
            "abw"
        );
    }

    // ── Status severity convention ────────────────────────────────────────
    //
    // The form status line renders every message in Muted, so a failed
    // create reads the same as a success (the "reassuring" finding). The
    // render sites classify via `status_is_warning`; these tests pin the
    // marker list against the actual message strings the panel produces.

    #[test]
    fn status_severity_flags_the_real_error_messages() {
        // Validation gates (create_swarm / create_agent).
        assert!(status_is_warning("Swarm name is required."));
        assert!(status_is_warning(
            "Mission is required to launch a swarm. Describe what the swarm should do."
        ));
        assert!(status_is_warning("Name and system prompt are required."));
        // Failure paths.
        assert!(status_is_warning("Create failed: connection reset"));
        assert!(status_is_warning(
            "Consent failed for atlas — swarm not created."
        ));
        assert!(status_is_warning("Tool invoker not wired."));
        assert!(status_is_warning("AI Assist unavailable: timeout"));
        assert!(status_is_warning(
            "ABW agents cannot be updated from this panel. Copy to Local to edit."
        ));
        assert!(status_is_warning(
            "Name contains characters that will be stripped on the local substrate."
        ));
    }

    #[test]
    fn status_severity_leaves_progress_and_success_muted() {
        assert!(!status_is_warning("Loading agent details…"));
        assert!(!status_is_warning("Deleting agent…"));
        assert!(!status_is_warning("Local swarm 'atlas' created."));
        assert!(!status_is_warning("Agent 'researcher' created."));
    }

    /// Only transport-level failures drive a retry. A tool that ran and failed,
    /// or a refusal, must not be re-issued: doing so would repeat a side effect
    /// for no benefit.
    ///
    /// This is the classification the fetchers branch on, so it belongs pinned
    /// next to the panel that consumes it.
    #[test]
    fn only_transport_failures_are_retried() {
        assert!(
            InvokeError::NotWired.is_retryable(),
            "the invoker is wired asynchronously post-login, so a panel opened during \
             startup must retry rather than present a dead end"
        );
        assert!(
            InvokeError::Unavailable("transport closed".into()).is_retryable(),
            "a closed MCP transport is transient - the call never reached the tool"
        );
        assert!(
            !InvokeError::Failed("ABW rejected the request".into()).is_retryable(),
            "a tool that ran and failed must not be re-issued"
        );
        assert!(
            !InvokeError::Interrupted("connection reset".into()).is_retryable(),
            "an interrupted call has an unknown outcome; auto-retrying a spend-bearing \
             tool like swarm_hire could charge credits twice"
        );
    }

    /// A spend whose outcome is unknown is distinguishable from a clean failure.
    ///
    /// `confirm_hire` branches on this to decide whether to restore the one-click
    /// consent banner. Restoring it after a possibly-completed `swarm_hire` is the
    /// double-charge path, so the two cases must not collapse.
    #[test]
    fn interrupted_spend_is_distinguishable_from_a_clean_failure() {
        assert!(
            InvokeError::Interrupted("connection reset".into()).is_outcome_unknown(),
            "confirm_hire relies on this to warn instead of offering a one-click retry"
        );
        assert!(
            !InvokeError::Failed("insufficient credits".into()).is_outcome_unknown(),
            "a refusal that reached ABW has a known outcome: nothing was spent"
        );
        assert!(
            !InvokeError::Unavailable("no live connection".into()).is_outcome_unknown(),
            "a request that never left has a known outcome: nothing was spent"
        );
    }

    /// `NotWired` renders the shared explanation rather than an empty string, so
    /// the startup state is legible instead of looking like a blank failure.
    #[test]
    fn not_wired_error_explains_itself() {
        let message = InvokeError::NotWired.message();
        assert_eq!(message, hkask_tool_invoker::NOT_WIRED_MESSAGE);
        assert!(
            message.contains("kask.mcp.load_default"),
            "the message must name the setting an operator can act on, got: {message}"
        );
    }

    // Pins the tool name strings the panel calls. The single source of truth
    // is `hkask_mcp_swarm::TOOL_NAMES`, re-exported as `parse::SWARM_TOOLS`.
    // The server's own `tool_surface_is_exactly_53_registered_tools` test pins
    // the count against the live `combined_router()` surface, and the Steer-mode
    // prompt-token test (`steer_prompt_mentions_only_known_tools`) catches any
    // `swarm_*` name the prompt mentions that isn't in the const — so a rename
    // surfaces at the server test rather than degrading to "tool not found" at
    // runtime.
    #[test]
    fn panel_tool_names_match_server() {
        assert_eq!(SWARM_SERVER, "swarm");

        // `SWARM_TOOLS` is a re-export of `hkask_mcp_swarm::TOOL_NAMES`, so
        // they are the same slice — no drift is possible. The server's own
        // test pins the count against the live router surface.
        assert_eq!(
            parse::SWARM_TOOLS.as_ptr() as usize,
            hkask_mcp_swarm::TOOL_NAMES.as_ptr() as usize,
            "SWARM_TOOLS must be a re-export of TOOL_NAMES, not a copy"
        );

        for tool in parse::SWARM_TOOLS {
            assert!(
                tool.starts_with("swarm_") && tool.len() > "swarm_".len(),
                "tool name `{tool}` must start with `swarm_` and have a non-empty suffix"
            );
        }

        // No duplicates — the server const is hand-maintained, so a
        // copy-paste error could silently mask a missing tool by doubling
        // another.
        let mut sorted = parse::SWARM_TOOLS.to_vec();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            before,
            "duplicate tool names in TOOL_NAMES list"
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

    // The `swarm_list_agents` response includes an `authenticated` boolean
    // (`self.client.is_authenticated()` on the server). The panel reads this
    // field to determine API-key status from the same source the MCP server
    // uses, rather than inferring it from the `swarm_get_swarm` error message.
    // This test pins the field name and the parse so a server-side rename is
    // caught here first.
    #[test]
    fn agent_list_response_parses_authenticated_field() {
        let json = serde_json::json!({
            "count": 0,
            "authenticated": true,
            "agents": []
        });
        let response: AgentListResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(response.authenticated, Some(true));

        let json = serde_json::json!({
            "count": 0,
            "authenticated": false,
            "agents": []
        });
        let response: AgentListResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(response.authenticated, Some(false));
    }

    // When the server omits `authenticated` (e.g. an older server version), the
    // field must default to `None` — never a fabricated `false` (which would
    // trigger the "no API key" warning even when the key is configured).
    #[test]
    fn agent_list_response_authenticated_defaults_to_none_when_absent() {
        let json = serde_json::json!({
            "count": 0,
            "agents": []
        });
        let response: AgentListResponse = serde_json::from_value(json).expect("parse");
        assert_eq!(response.authenticated, None);
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

    // The swarm server returns tool errors as an Ok string carrying the
    // `{"error": ..., "kind": ...}` envelope (McpToolError::to_json_string),
    // not as an Err from invoke_tool. The canonical case is `permission_denied`
    // with message "no API key configured" when require_auth() fails. Before
    // the error-envelope seam, this fell through to the WorkspaceListResponse
    // parse (no `workspaces` field) and surfaced as the misleading
    // "Failed to parse workspaces: {…}". The seam routes it through the same
    // classification the Err(_) branch uses, so the operator sees the real
    // cause. This test pins the detection at the seam so a regression in
    // either the envelope shape or the kind mapping surfaces here.
    #[test]
    fn fetch_all_detects_server_error_envelope_before_workspace_parse() {
        let out = r#"{"error":"no API key configured","kind":"permission_denied"}"#;
        let err = hkask_types::tool_response::parse_tool_error(out)
            .expect("server error envelope must be detected");
        assert_eq!(err.message, "no API key configured");
        assert_eq!(err.kind, Some(hkask_types::McpErrorKind::PermissionDenied));
        assert!(!err.is_retryable());
        // A successful payload must NOT be misclassified as an error envelope.
        let ok = r#"{"content":{"workspaces":[]}}"#;
        assert!(hkask_types::tool_response::parse_tool_error(ok).is_none());
    }

    // The cloud agents fetch (`swarm_list_agents`) and the local agents fetch
    // (`swarm_list_local_agents`) now also detect the server error envelope
    // before attempting to parse the typed response. The canonical case for
    // `swarm_list_agents` is a 401 from ABW (invalid key) mapped to
    // `permission_denied`; for `swarm_list_local_agents` a server error is a
    // bug (it reads the filesystem, no auth), but it must not be silently
    // swallowed. Before the seam, both fell through to the typed parse, failed
    // (no `agents` field), and either surfaced as "Failed to parse agents"
    // (cloud) or silently disappeared (local). This test pins the detection at
    // the seam for both agent fetches.
    #[test]
    fn fetch_all_detects_server_error_envelope_before_agents_parse() {
        let out = r#"{"error":"no API key configured","kind":"permission_denied"}"#;
        let err = hkask_types::tool_response::parse_tool_error(out)
            .expect("server error envelope must be detected");
        assert_eq!(err.message, "no API key configured");
        assert_eq!(err.kind, Some(hkask_types::McpErrorKind::PermissionDenied));
        assert!(!err.is_retryable());
        // A successful agents payload must NOT be misclassified as an error.
        let ok = r#"{"content":{"agents":[],"total":0}}"#;
        assert!(hkask_types::tool_response::parse_tool_error(ok).is_none());
        // A successful local agents payload must NOT be misclassified either.
        let ok_local = r#"{"content":{"agents":[],"total":0}}"#;
        assert!(hkask_types::tool_response::parse_tool_error(ok_local).is_none());
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
        // Local delegation needs no funding: the local ledger records spend, it
        // does not authorize it (`LocalSwarmRuntime::delegate` has no balance
        // gate). The prompt must say so, or the Curator wastes a turn funding a
        // ledger that was never blocking it — and may refuse to delegate at all
        // when it reads a low or negative balance.
        //
        // This inverts the previous assertion, which required the prompt to
        // promise a `PaymentRequired` that local mode no longer returns.
        assert!(
            prompt.contains("NO funding") || prompt.contains("needs NO funding"),
            "steer prompt must tell the curator local delegation needs no funding"
        );
        assert!(
            prompt.contains("Do NOT call `swarm_fund_local` before delegating"),
            "steer prompt must actively steer the curator away from pre-funding, not \
             merely omit the requirement"
        );
        assert!(
            !prompt.contains("PaymentRequired"),
            "steer prompt must NOT promise a PaymentRequired error on the local path — \
             local delegation no longer refuses for lack of funds, so advertising it \
             would make the curator plan around a gate that does not exist"
        );
        // The cloud path DOES gate, and the distinction must survive: a curator
        // that generalizes "no funding needed" to `swarm_hire` would strand real
        // spend.
        assert!(
            prompt.contains("CLOUD tools") || prompt.contains("swarm_hire"),
            "steer prompt must preserve the cloud/local funding distinction"
        );

        // No surviving phrasing may call the local balance a gate.
        //
        // The prompt is one long string assembled from several paragraphs, and a
        // previous pass fixed only the first occurrence — leaving the prompt
        // self-contradictory (one paragraph said "do NOT treat a low balance as a
        // blocker", another said "the ledger balance check is the gate"). An
        // internally inconsistent prompt is worse for a model than a uniformly
        // stale one, so scan for the *class* of claim rather than one instance.
        for stale in [
            "balance check is the",
            "ledger balance check",
            "balance is the gate",
            "balance gate",
        ] {
            assert!(
                !prompt.contains(stale),
                "steer prompt still calls the local ledger balance a gate ({stale:?}) — \
                 local delegation has no funding gate, and a second occurrence in the \
                 same prompt makes it self-contradictory"
            );
        }
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
    // tool that isn't in the canonical `SWARM_TOOLS` const. The const is a
    // verified copy of the server's `hkask_mcp_swarm::TOOL_NAMES` (asserted by
    // `panel_tool_names_match_server`); when a tool is renamed in
    // `hkask-mcp-swarm`, that test fails until the const is updated, and this
    // test then catches any stale name the prompt still mentions — so a rename
    // surfaces here rather than degrading to "tool not found" at runtime. The
    // publish-checks and staleness-chip parsers are unit-tested in `parse::tests`.
    #[test]
    fn steer_prompt_mentions_only_known_tools() {
        let known: std::collections::HashSet<&str> = parse::SWARM_TOOLS.iter().copied().collect();
        let kanban_known: std::collections::HashSet<&str> =
            parse::KANBAN_TOOLS.iter().copied().collect();
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
                if name.starts_with("kanban_") && name.len() > "kanban_".len() {
                    assert!(
                        kanban_known.contains(name.as_str()),
                        "steer prompt advertises `{name}` which is not in KANBAN_TOOLS \
                         — update the const or the prompt"
                    );
                }
            }
        }
    }

    #[test]
    fn kanban_tool_names_match_server() {
        // `KANBAN_TOOLS` must match the #[tool] fn names in
        // `hkask-mcp-kata-kanban/src/hkask_mcp_kata_kanban.rs`. Keep in sync when
        // adding/removing a server tool — a rename in the kanban server must be
        // reflected here so the steer prompt never advertises a stale name.
        assert_eq!(KANBAN_SERVER, "kata-kanban");

        // Pin the count so adding or removing a server tool without updating
        // the const is caught.
        assert_eq!(
            parse::KANBAN_TOOLS.len(),
            23,
            "tool count changed — update KANBAN_TOOLS to match \
             hkask-mcp-kata-kanban #[tool] fns"
        );

        for tool in parse::KANBAN_TOOLS {
            assert!(
                tool.starts_with("kanban_") || *tool == "contract_propose_expect",
                "tool name `{tool}` must start with `kanban_` or be the \
                 `contract_propose_expect` exception"
            );
        }

        // No duplicates.
        let mut sorted = parse::KANBAN_TOOLS.to_vec();
        sorted.sort();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            before,
            "duplicate tool names in KANBAN_TOOLS list"
        );
    }

    // P2 trim pin: the steer prompt must NOT re-grow per-tool behavioral
    // descriptions — those live in the MCP server's `#[tool]` description
    // fields and reach the model via the tools array in every completion
    // request (`build_completion_request`). Prompt-side glosses drift
    // independently of the server (the `cloud_swarm_id` vs `cloud_id` drift
    // was live evidence), and the `debug_assert!` drift guard checks tool
    // NAMES only. This test pins the deletion: a backticked tool name must
    // not be immediately followed by a parenthesized gloss.
    #[test]
    fn steer_prompt_does_not_gloss_tool_names() {
        for prompt in [
            steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Abw),
            steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Local),
        ] {
            for seg in prompt
                .split('`')
                .enumerate()
                .filter_map(|(i, s)| (i % 2 == 1).then_some(s))
            {
                assert!(
                    !seg.starts_with('(') && !seg.contains(" — "),
                    "steer prompt glosses tool `{seg}` — per-tool behavior belongs \
                     in the server's #[tool] description, not the prompt"
                );
            }
        }
    }

    #[test]
    fn steer_prompt_advertises_kanban_tools() {
        // C7 guard: the steer prompt MUST reference the kanban panel as the
        // coordination substrate and mention the swarm↔kanban bridge tool.
        // The full kanban tool advertising now lives in the kanban panel's
        // own Steer mode (moved from the swarm panel).
        let prompt = steer_system_prompt(Some("ws_test"), kask_bridge::SwarmModeConfig::Local);
        assert!(
            prompt.contains("kanban panel"),
            "steer prompt must reference the kanban panel as the coordination substrate"
        );
        assert!(
            prompt.contains("kanban_task_spawn"),
            "steer prompt must mention the `kanban_task_spawn` bridge tool"
        );
    }

    // Pin the sequencing invariant: the `swarm_list_agents` response carries
    // the `authenticated` field, and `handle_swarm_fetch_failure` uses
    // `cloud_authenticated` (not the error message) to distinguish "no key"
    // from "key rejected." This is a static contract test — it verifies the
    // field exists in the response shape and the handler reads it, without
    // constructing a full GPUI test harness (which would be needed to exercise
    // the parallel-spawn ordering directly). The sequencing itself is enforced
    // by the `fetch_all` structure: the cloud swarms fetch is chained after
    // the cloud agents fetch in the same task, so `cloud_authenticated` is
    // always set before `handle_swarm_fetch_failure` runs.
    #[test]
    fn cloud_authenticated_is_read_from_server_response() {
        // The server's `swarm_list_agents` response includes `authenticated`.
        // The panel must parse it — without this field, the panel cannot
        // distinguish "no key configured" from "key rejected by ABW" without
        // guessing from the error message (which conflates the two).
        let out = r#"{"content":{"count":0,"authenticated":true,"agents":[]}}"#;
        let parsed = parse_tool_response(out).expect("envelope");
        let response: AgentListResponse =
            serde_json::from_value(parsed).expect("inner content deserializes");
        assert_eq!(
            response.authenticated,
            Some(true),
            "the panel must parse `authenticated` from the server response"
        );
    }

    // fermi v0.16.x `display_alias` (forwarded by `swarm_list_agents`'s
    // `map_catalogue_agent`) must deserialize into `AgentInfo.display_alias`
    // so the cloud fetch path can populate `AgentCard.display_name` from it
    // (fetch.rs) instead of the prior hardcoded `String::new()`. Without this,
    // cloud agents always rendered their `agent_id` slug ("xaman_ek") even when
    // ABW carried a human name ("Xaman Ek") — the local path already showed
    // `display_name`, so this unifies cloud and local. The card renderer falls
    // back to `id` when `display_name` is empty (card.rs), so an ABW agent with
    // no `display_alias` is unaffected.
    #[test]
    fn cloud_agent_display_alias_deserializes_into_agent_info() {
        let out = r#"{"content":{"count":1,"authenticated":true,"agents":[{"agent_id":"xaman_ek","display_alias":"Xaman Ek"}]}}"#;
        let parsed = parse_tool_response(out).expect("envelope");
        let response: AgentListResponse =
            serde_json::from_value(parsed).expect("inner content deserializes");
        let agent = response.agents.first().expect("one agent");
        assert_eq!(agent.agent_id.as_deref(), Some("xaman_ek"));
        assert_eq!(
            agent.display_alias.as_deref(),
            Some("Xaman Ek"),
            "display_alias must deserialize so the cloud fetch path can surface \
             the human name instead of the slug"
        );
        // The fetch.rs wiring resolves to `display_alias.unwrap_or_default()`;
        // an absent/empty alias yields an empty `display_name`, and the card
        // renderer falls back to `agent_id`. Pin both branches:
        let resolved = agent.display_alias.clone().unwrap_or_default();
        assert_eq!(resolved, "Xaman Ek");
        let none_agent: crate::parse::AgentInfo =
            serde_json::from_value(serde_json::json!({"agent_id": "no_alias"}))
                .expect("display_alias is #[serde(default)]-optional");
        assert_eq!(none_agent.display_alias, None);
        assert_eq!(none_agent.display_alias.unwrap_or_default(), "");
    }

    // Pin the empty-state message contract: when the API key IS configured
    // (`cloud_authenticated == Some(true)`), the empty-state message must NOT
    // suggest setting `HKASK_ABW_API_KEY` — the operator genuinely has no
    // swarms, not a missing-key problem. When the key is NOT configured
    // (`Some(false)`), the hint must appear.
    #[test]
    fn empty_state_omits_key_hint_when_authenticated() {
        // When authenticated, the `key_hint` is empty — the message should
        // NOT contain "HKASK_ABW_API_KEY".
        let key_hint = match Some(true) {
            Some(false) => " or set HKASK_ABW_API_KEY to see your cloud swarms",
            _ => "",
        };
        assert!(
            !key_hint.contains("HKASK_ABW_API_KEY"),
            "empty-state must not suggest setting the key when it IS configured"
        );

        // When not authenticated, the hint must appear.
        let key_hint = match Some(false) {
            Some(false) => " or set HKASK_ABW_API_KEY to see your cloud swarms",
            _ => "",
        };
        assert!(
            key_hint.contains("HKASK_ABW_API_KEY"),
            "empty-state must suggest setting the key when it is NOT configured"
        );
    }

    // ── Local-agent merge dedup ──────────────────────────────────────────────
    //
    // `swarm_clone_to_local` writes a local clone with `agent_id = <cloud>-clone`
    // and `cloud_swarm_id = Some(<cloud>)`. The panel merge must collapse the
    // clone into the cloud row (marked Synced), not render a duplicate Local
    // row. Before the fix, suppression keyed only on `agent_id`, which never
    // matched the cloud id — the clone always leaked as a second row.

    #[test]
    fn clone_with_matching_cloud_collapses_to_single_synced_row() {
        use crate::fetch::merge_local_agents;
        use crate::parse::{AgentCard, AgentSource, LocalAgentInfo};

        let cloud = SwarmEntry::Agent(AgentCard {
            id: "efra_communication".into(),
            agent_type: "worker".into(),
            description: "desc".into(),
            author: String::new(),
            executions: 0,
            updated_at: None,
            display_name: String::new(),
            source: AgentSource::Cloud,
        });
        let clone = LocalAgentInfo {
            agent_id: "efra_communication-clone".into(),
            agent_type: "worker".into(),
            description: "desc".into(),
            display_name: String::new(),
            cloud_swarm_id: Some("efra_communication".into()),
            accepts: vec![],
            produces: vec![],
        };

        let mut entries = vec![cloud];
        merge_local_agents(&mut entries, vec![clone]);

        let rows: Vec<&AgentCard> = entries
            .iter()
            .filter_map(|e| match e {
                SwarmEntry::Agent(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(rows.len(), 1, "clone must not produce a duplicate row");
        assert_eq!(rows[0].id, "efra_communication");
        assert_eq!(rows[0].source, AgentSource::Synced);
    }

    // ── D33: swarm panel full CRUD ───────────────────────────────────────────
    //
    // The new management surface adds: confirmation flow for destructive ops,
    // edit-metadata (local), copy/duplicate (local + ABW via Compose), cloud
    // swarm delete, and accepts/produces port display in roster rows.

    #[test]
    fn destructive_action_delete_swarm_carries_source_and_name() {
        let action = DestructiveAction::DeleteSwarm {
            swarm_id: "team_alpha".into(),
            source: AgentSource::Local,
            name: "Team Alpha".into(),
        };
        match &action {
            DestructiveAction::DeleteSwarm { swarm_id, source, name } => {
                assert_eq!(swarm_id, "team_alpha");
                assert_eq!(*source, AgentSource::Local);
                assert_eq!(name, "Team Alpha");
            }
            _ => panic!("expected DeleteSwarm"),
        }
    }

    #[test]
    fn destructive_action_remove_agent_carries_source() {
        let action = DestructiveAction::RemoveAgent {
            swarm_id: "ws_123".into(),
            agent_id: "analyst".into(),
            source: AgentSource::Cloud,
        };
        match &action {
            DestructiveAction::RemoveAgent { swarm_id, agent_id, source } => {
                assert_eq!(swarm_id, "ws_123");
                assert_eq!(agent_id, "analyst");
                assert_eq!(*source, AgentSource::Cloud);
            }
            _ => panic!("expected RemoveAgent"),
        }
    }

    #[test]
    fn new_swarm_tools_are_in_swarm_tool_names() {
        // The panel calls these tool names via string literals. They must
        // exist in the server's TOOL_NAMES so the dispatch succeeds and the
        // Steer prompt-token test stays green.
        assert!(
            parse::SWARM_TOOLS.contains(&"swarm_update_local_swarm"),
            "swarm_update_local_swarm must be in TOOL_NAMES"
        );
        assert!(
            parse::SWARM_TOOLS.contains(&"swarm_clone_local_swarm"),
            "swarm_clone_local_swarm must be in TOOL_NAMES"
        );
        assert!(
            parse::SWARM_TOOLS.contains(&"swarm_delete_swarm"),
            "swarm_delete_swarm must be in TOOL_NAMES (cloud delete)"
        );
    }
}
