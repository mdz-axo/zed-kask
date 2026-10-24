#![forbid(unsafe_code)]
//! Swarm Panel — a center-pane `Item` listing local swarm agents and
//! swarms as cards, mirroring the Kask Extensions panel layout.
//!
//! Entities are **agents** (from the local agent registry) and **swarms**
//! (the local swarm registry). Data is fetched through the global
//! `ToolInvoker` hook (the metered MCP runtime path), so all calls flow
//! through `hkask-mcp-swarm` and the kask MCP runtime rather than ad-hoc
//! HTTP from the UI.
//!
//! Layout mirrors `KaskExtensionsPage`: headline, search bar, filter toggle
//! (All / Swarms / Agents), a uniform list of `MarketplaceCard`s, and an
//! empty state that surfaces fetch errors. The panel wires lifecycle
//! actions to `invoke_tool` calls: edit (agent + swarm), remove-local,//! and delete (with a confirmation flow).
//!
//! **Steer mode** hosts a cross-domain `ConversationView` framed around swarm
//! work. The operator asks the curator to compose/steer a swarm; the curator's
//! `SkillTool` invokes the `swarm-intelligence` cascade (see
//! `kask/docs/diataxis/swarm_system/explanation.md`). The conversation is
//! persisted via the global `ThreadStore` — the curator's live state
//! (in-flight plans, collected `delegate_results`, prior iterations)
//! survives panel close and restart, and every turn is ingested into the
//! curator's sovereign memory for cross-composition learning.

mod agent_edit;
mod ai_assist;
mod author;
mod card;
mod compose;
mod fetch;
mod member_turns;
pub mod panel_button;
mod parse;
mod swarm_ops;

use ai_assist::AiAssistState;
use author::AuthorForm;
use compose::ComposeForm;
use fetch::FetchState;

pub use panel_button::SwarmPanelButton;
// The `ToolInvoker` trait + global accessor live in the `hkask-tool-invoker` leaf
// crate (relocated so the kask GPUI widgets can dispatch without depending on
// this heavy panel crate). Re-exported here so existing `swarm_panel::ToolInvoker`
// / `set_tool_invoker` / `shared_tool_invoker` call sites compile unchanged.
pub use hkask_tool_invoker::{InvokeError, ToolInvoker, set_tool_invoker, shared_tool_invoker};

use parse::{AgentCard, SwarmCard};

use std::ops::Range;
use std::time::Duration;

use anyhow::Result;
use editor::Editor;
use fs::Fs;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, Render, ScrollHandle, Task,
    UniformListScrollHandle, WeakEntity, Window, actions, uniform_list,
};
use marketplace_ui_common::{MarketplaceCard, marketplace_empty_state, marketplace_search_bar};
use project::Project;
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

// Steer mode: a cross-domain `ConversationView` framed around swarm work. The
// curator's `SkillTool` invokes the `swarm-intelligence` cascade when the
// operator asks to compose/steer a swarm. See
// `kask/docs/diataxis/swarm_system/explanation.md`.
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

/// The system prompt injected into the Steer mode `ConversationView`. It
/// highlights swarm tools and the `swarm-intelligence` skill while retaining
/// the full cross-domain MCP surface. The
/// curator's `SkillTool` discovers the skill from the `<available_skills>`
/// list in its base system prompt; this prompt adds the swarm-specific
/// context (active swarm, the skill's purpose).
/// Build the Steer-mode system prompt for the curator agent.
///
/// The tool list is generated from the server's canonical const
/// (`hkask_mcp_swarm::TOOL_NAMES`, re-exported as `parse::SWARM_TOOLS`) —
/// never hand-maintained, so a server-side rename/merge propagates at the
/// next build instead of degrading to "tool not found" at dispatch.
fn steer_system_prompt(selected_swarm: Option<&str>) -> SharedString {
    let workspace_note = match selected_swarm {
        Some(id) => format!(
            "The operator's active local swarm is `{id}`. \
             When the operator asks to compose or steer a swarm without naming \
             one, assume this swarm."
        ),
        None => "No swarm is currently selected. If the operator \
            asks to compose a swarm, ask them to select one in the Browse tab first."
            .to_string(),
    };
    let workspace_context = selected_swarm
        .map(|id| format!("\"swarm_id\": \"{id}\""))
        .unwrap_or_else(|| "\"swarm_id\": \"\"".to_string());
    let swarm_tools = hkask_steer::render_tool_names(parse::SWARM_TOOLS);
    let prompt = format!(
        "## Agent Swarm Panel — Steer Mode
         \n\
         You are operating in the Agent Swarm panel's cross-domain Steer mode. \
         The full MCP tool surface remains available. The swarm server exposes \
         the local substrate's tools: {swarm_tools}. \
         These run on the local substrate (`hkask-inference`): local delegation \
         has NO budget and needs NO consent — it runs on the operator's own \
         substrate, so there is nothing to authorize or price (timeouts are \
         the enforcement mechanism). Per-tool behavior is in each tool's \
         description.\n\
         \n\
         {workspace_note}\n\
         \n\
         The `swarm-intelligence` skill is available for swarm composition and \
         steering. When the operator asks to compose, configure, tune, or steer \
         a swarm toward a target condition, invoke the `swarm-intelligence` \
         skill with the operator's task. Pass the active swarm in the skill's \
         `context` argument so the cascade reads the right roster:\n\
         \
         ```json\n\
         {{{workspace_context}}}\n\
         ```\n\
         \
         When invoking via a slash command\n\
         (`/swarm-intelligence ...`), pass context as leading `key=value` pairs\n\
         before the task text — e.g. `/swarm-intelligence swarm_id=atlas\n\
         compose my swarm` sets the swarm and the task.

         **Execution mode: steering (default).** When the skill emits a plan
         (emitted_calls), the manifest's post-Act execute step (step 8) calls
         `swarm_execute_plan_local` deterministically and feeds the returned
         `delegate_results` array into the next LOOP iteration so C5 (fault
         attribution) and C6 (reconfigure) close the loop structurally — no
         prompt instruction needed. The operator can use the \
         \"Launch Plan\" button to inject this instruction if you did not execute
         automatically.

         The skill accepts an optional `task_success` context field — a
         deterministic evaluator's verdict on whether the swarm's last output
         solved the task (test pass/fail, schema validation, exit code,
         regex/reference match). When the operator's task has a deterministic
         oracle, pass `task_success` in the skill's `context` as
         an object whose `pass` field is true or false (and a
         `detail` string if useful). When the task is open-ended with no
         oracle, OMIT `task_success` — the skill falls back to the three
         swarm-health axes and the human Go See loop covers the gap. Do NOT
         use an LLM to score the output as `task_success`; the judge must be
         deterministic (the cybernetic plan's determinism constraint).
         \n\
         Cybernetic Swarm Plan — second-order monitor + Go See (C1/C2). The
         skill's CONVERGE runs a deterministic second-order monitor (C1) over
         the iteration log: it flags reasoning loops (same deficit+action
         repeating with no d improvement) and sensor-truth divergence (d
         improving while s declines — the swarm looks healthier but fails more
         tasks). When the monitor recommends go_see, surface a Go See
         directive (C2): the operator should descend this Steer conversation
         with the section 5 checklist — is s filtering task-failure truth, are
         .rules priors still verified against the codebase, are these Steer
         guides having the intended effect. DECIDE also applies a failed-edit
         memory (C3), per-agent-type influence guards (C7), and a
         reconfigure_agent action (C6) via swarm_reconfigure_local_agent when
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
    // A `swarm_*`/`kanban_*` name in the prompt that the server does not
    // expose degrades to "tool not found" at dispatch time, so catch it here
    // in dev builds. The generated lists cannot drift by construction; this
    // catches the residual risk — prose that names tools by hand. The
    // `steer_prompt_mentions_only_known_tools` and
    // `server_tools_are_all_advertised` tests are the CI enforcement.
    hkask_steer::verify_tool_advertisement(&prompt, parse::SWARM_TOOLS, &["swarm_"]);
    hkask_steer::verify_tool_advertisement(&prompt, parse::KANBAN_TOOLS, &["kanban_"]);
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

/// The three surfaces of the panel: browsing existing agents/swarms, authoring
/// a new agent, and composing agents into a swarm. Sharing (extensions) is
/// represented by the browse surface's discovery role.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PanelMode {
    Browse,
    Author,
    Compose,
    /// Steer: a cross-domain `ConversationView` framed around swarm work. The
    /// operator asks the curator to compose/steer a swarm; the curator's
    /// `SkillTool` invokes the `swarm-intelligence` cascade.
    Steer,
}

// ── View model ─────────────────────────────────────────────────────────────

/// One row in the panel — a local agent or a local swarm.
#[derive(Clone, Debug)]
enum SwarmEntry {
    Agent(AgentCard),
    Swarm(SwarmCard),
}

// ── Panel ──────────────────────────────────────────────────────────────────

/// A destructive action pending operator confirmation. The two-step pattern
/// prevents accidental irreversible ops (delete swarm).
#[derive(Clone, Debug)]
enum DestructiveAction {
    DeleteSwarm { swarm_id: String, name: String },
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
    /// as `author_scroll` — the compose form (name, mission, agents, AI assist,
    /// action row) overflows on smaller panes.
    compose_scroll: ScrollHandle,
    /// The fetch lifecycle — in-flight count, per-source errors, and
    /// the retry/backoff state. Owned by `fetch.rs`;
    /// render reads it through `self.fetch.is_fetching()` /
    /// `self.fetch.visible_error()`.
    fetch: FetchState,
    filter: SwarmFilter,
    entries: Vec<SwarmEntry>,
    filtered_entry_indices: Vec<usize>,
    query_editor: Entity<Editor>,
    _subscriptions: [gpui::Subscription; 2],
    search_task: Option<Task<()>>,
    /// Local identity of the explicitly selected swarm; never infer local
    /// ownership from the Curator thread.
    selected_local_swarm: Option<String>,
    member_turns: member_turns::MemberTurnsState,
    /// Which surface is active: browse, author, compose, or steer.
    mode: PanelMode,
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
    /// Composition form state.
    compose: ComposeForm,
    /// The Steer-mode surface: owns the cross-domain curator `ConversationView`
    /// lifecycle (construction + invalidation). Empty until the operator
    /// first selects Steer. Uses the retained-view pattern (one
    /// `ConversationView`, reused across re-renders).
    steer: hkask_steer::SteerSurface,
    /// The "Open Thread" picker — resumes a database thread in this panel's
    /// Steer surface.
    thread_picker: Entity<hkask_steer::ThreadPicker>,
    /// The session id to resume on the next `ensure_steer_conversation` —
    /// set by `open_thread`, consumed by `ensure_steer_conversation`.
    pending_resume: Option<agent_client_protocol::schema::v2::SessionId>,
    /// The most recent operation error (remove/delete/update failures),
    /// surfaced as a strip under the header.
    op_error: Option<SharedString>,
    /// A destructive action awaiting explicit confirmation (delete swarm).
    /// When `Some`, the browse list renders a confirmation banner.
    pending_destructive: Option<DestructiveAction>,
    /// R2: AI Assist / validation state — owned by `ai_assist.rs`; the
    /// render helpers there read it through the module's `pub(crate)`
    /// surface.
    ai_assist: AiAssistState,
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
            let query_sub = cx.subscribe(&query_editor, Self::on_query_change);
            // Re-filter the browse list whenever settings change. Settings
            // changes can still affect the entry list indirectly (e.g. an MCP
            // server restart re-fetches), so the observer stays as a
            // re-render trigger.
            let settings_sub = cx.observe_global::<SettingsStore>(|this, cx| {
                this.filter_entries(cx);
            });
            let subscriptions = [query_sub, settings_sub];

            let scroll_handle = UniformListScrollHandle::new();
            let author_scroll = ScrollHandle::new();
            let compose_scroll = ScrollHandle::new();

            let author = AuthorForm::new(window, cx);
            let compose = ComposeForm::new(window, cx);

            let panel_handle: gpui::WeakEntity<SwarmPanel> = cx.weak_entity();
            let thread_picker = cx.new(|cx| {
                hkask_steer::ThreadPicker::new(
                    std::rc::Rc::new(move |session_id, window, cx: &mut gpui::App| {
                        if let Err(error) = panel_handle
                            .update(cx, |panel, cx| panel.open_thread(session_id, window, cx))
                        {
                            log::warn!(
                                "swarm panel: failed to open thread — panel entity dropped: {error}"
                            );
                        }
                    }),
                    cx,
                )
            });
            let mut this = Self {
                workspace: workspace_handle,
                project,
                fs,
                list: scroll_handle,
                author_scroll,
                compose_scroll,
                fetch: FetchState::default(),
                filter: SwarmFilter::All,
                entries: Vec::new(),
                filtered_entry_indices: Vec::new(),
                query_editor,
                _subscriptions: subscriptions,
                search_task: None,
                selected_local_swarm: None,
                member_turns: member_turns::MemberTurnsState::default(),
                mode: PanelMode::Browse,
                author,
                pending_author_load: None,
                pending_author_reset: false,
                compose,
                steer: hkask_steer::SteerSurface::new(),
                thread_picker,
                pending_resume: None,
                op_error: None,
                pending_destructive: None,
                ai_assist: AiAssistState::default(),
            };
            this.fetch_all(cx);
            this
        })
    }

    fn set_mode(&mut self, mode: PanelMode, window: &mut Window, cx: &mut Context<Self>) {
        let entering_steer = mode == PanelMode::Steer && self.mode != PanelMode::Steer;
        self.mode = mode;
        if entering_steer {
            self.refresh_thread_choices(cx);
            self.refresh_member_turns(cx);
        }
        // Move focus to the target mode's first field — otherwise focus stays
        // on the now-hidden search editor and keyboard input goes nowhere (the
        // M5 finding). Steer focuses its conversation's editor.
        let handle = match mode {
            PanelMode::Browse => self.query_editor.read(cx).focus_handle(cx),
            PanelMode::Author => self.author.name.read(cx).focus_handle(cx),
            PanelMode::Compose => self.compose.name.read(cx).focus_handle(cx),
            PanelMode::Steer => self
                .steer
                .conversation()
                .map(|c| c.read(cx).focus_handle(cx))
                .unwrap_or_else(|| self.query_editor.read(cx).focus_handle(cx)),
        };
        handle.focus(window, cx);
        cx.notify();
    }

    /// Lazily construct the `ConversationView` for Steer mode if it doesn't
    /// exist yet. Constructs a `CuratorAgentServer` with a system prompt that
    /// tells the curator about the
    /// `swarm-intelligence` skill and the active swarm. The curator's
    /// `SkillTool` invokes the cascade when the operator asks to compose/steer
    /// a swarm.
    ///
    /// `window` is required because `ConversationView::new` may focus its
    /// inner `MessageEditor`.
    fn ensure_steer_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        hkask_steer::ensure_steer(
            &mut self.steer,
            hkask_steer::SteerContext {
                system_prompt: steer_system_prompt(self.selected_local_swarm.as_deref()),
                fs: self.fs.clone(),
                project: self.project.clone(),
                workspace: self.workspace.clone(),
                resume_session_id: self.pending_resume.take(),
            },
            parse::SWARM_TOOLS,
            &["swarm_"],
            window,
            cx,
        );
    }

    /// Resume a database thread in this panel's Steer surface, replacing the
    /// live conversation. The mode-scoped system prompt is rebuilt from the
    /// current settings, so the resumed thread steers against the backend
    /// mode the operator is looking at.
    fn open_thread(
        &mut self,
        session_id: agent_client_protocol::schema::v2::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_resume = Some(session_id);
        self.steer.invalidate();
        self.ensure_steer_conversation(window, cx);
        cx.notify();
    }

    /// Filter the browse entries by the active `SwarmFilter` (All/Swarms/Agents)
    /// and the search query.
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
                                format!("{} {} {}", a.id, a.agent_type, a.description)
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

    /// Render the confirmation banner for a pending destructive action.
    /// Shows the action description and Confirm / Cancel buttons. Confirm
    /// dispatches to `confirm_destructive`; Cancel dispatches to
    /// `cancel_destructive`.
    fn render_destructive_confirmation(
        &self,
        action: &DestructiveAction,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (warning_text, confirm_label, key) = match action {
            DestructiveAction::DeleteSwarm { swarm_id, name } => (
                format!(
                    "Delete swarm '{name}'? This is irreversible — the swarm and its roster will be removed."
                ),
                "Confirm Delete",
                swarm_id.clone(),
            ),
        };
        v_flex()
            .gap_1()
            .p_2()
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_sm()
            .child(
                Label::new(warning_text)
                    .size(LabelSize::XSmall)
                    .color(Color::Warning),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(
                            SharedString::from(format!("confirm-destructive-{key}")),
                            confirm_label,
                        )
                        .style(ButtonStyle::Filled)
                        .label_size(LabelSize::XSmall)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_destructive(cx);
                        })),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("cancel-destructive-{key}")),
                            "Cancel",
                        )
                        .style(ButtonStyle::Subtle)
                        .label_size(LabelSize::XSmall)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_destructive(cx);
                        })),
                    ),
            )
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

        let message: SharedString = if self.fetch.is_fetching() {
            "Loading agents and swarms…".into()
        } else if let Some(err) = self.fetch.visible_error() {
            format!("Failed to load swarm data: {err}").into()
        } else {
            let message: SharedString = match self.filter {
                SwarmFilter::All => {
                    if has_search {
                        "No agents or swarms that match your search.".into()
                    } else {
                        "No agents or swarms. Create one (Author/Compose).".into()
                    }
                }
                SwarmFilter::Swarms => {
                    if has_search {
                        "No swarms that match your search.".into()
                    } else {
                        "No swarms. Compose one (Compose) to group agents.".into()
                    }
                }
                SwarmFilter::Agents => {
                    if has_search {
                        "No agents that match your search.".into()
                    } else {
                        "No agents. Create one (Author).".into()
                    }
                }
            };
            message
        };

        marketplace_empty_state(message, self.fetch.visible_error().is_some())
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
                                    // Always-visible manual refresh, mirroring the
                                    // kanban panel's refresh button. Automatic retries
                                    // are bounded (MAX_FETCH_RETRIES in `fetch.rs`); once exhausted,
                                    // the operator needs a way back without closing
                                    // and reopening the panel. Disabled while a fetch
                                    // is in flight to prevent duplicate dispatch.
                                    .child(
                                        IconButton::new("swarm-refresh", IconName::RotateCw)
                                            .icon_size(IconSize::Small)
                                            .icon_color(Color::Muted)
                                            .disabled(self.fetch.is_fetching())
                                            .tooltip(Tooltip::text(
                                                "Refresh agents and swarms",
                                            ))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_now(cx);
                                            })),
                                    ),
                            ),
                    )
                    // Operation errors surface near the header, where the
                    // removed banners used to render.
                    .when_some(self.op_error.clone(), |this, err| {
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
                    // bounded (`MAX_FETCH_RETRIES` in `fetch.rs`), so once they are exhausted the
                    // operator needs a way back without closing and reopening the
                    // panel. Two elements in this row (label + button), measured
                    // against the kask-seam-audit layout loop's action budget.
                    .when_some(self.fetch.visible_error().cloned(), |this, err| {
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
                                        .disabled(self.fetch.is_fetching())
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
                                            this.reset_compose_form_for_create(window, cx);
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
                            let member_turns = self.render_member_turns(cx);
                            match self.steer.conversation() {
                                Some(view) => this
                                    .child(member_turns)
                                    // The Open Thread affordance sits above the
                                    // conversation so the operator can resume a
                                    // previous steer session at any time.
                                    .child(
                                        h_flex()
                                            .px_2()
                                            .py_1()
                                            .border_b_1()
                                            .border_color(
                                                cx.theme().colors().border_variant
                                            )
                                            .child(self.thread_picker.clone()),
                                    )
                                    .child(view.clone())
                                    .into_any_element(),
                                None => this
                                    .child(member_turns)
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
                            // The destructive confirmation banner, above the
                            // list.
                            let content = this.when_some(
                                self.pending_destructive.clone(),
                                |this, action| {
                                    this.child(
                                        self.render_destructive_confirmation(&action, cx),
                                    )
                                },
                            );
                            {
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
                .steer
                .conversation()
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
        // The panel reconstructs its state from the local registries on first
        // render.
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
        assert!(status_is_warning("Tool invoker not wired."));
        assert!(status_is_warning("AI Assist unavailable: timeout"));
        assert!(status_is_warning("Update failed: connection reset"));
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
            !InvokeError::Failed {
                message: "tool rejected the request".into(),
                kind: None,
            }
            .is_retryable(),
            "a tool that ran and failed must not be re-issued"
        );
        assert!(
            !InvokeError::Interrupted("connection reset".into()).is_retryable(),
            "an interrupted call has an unknown outcome; auto-retrying a \
             state-changing tool could apply it twice"
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
    // The server's own tool-surface test pins the count against the live
    // router surface, and the Steer-mode prompt-token test
    // (`steer_prompt_mentions_only_known_tools`) catches any `swarm_*` name
    // the prompt mentions that isn't in the const — so a rename surfaces at
    // the server test rather than degrading to "tool not found" at runtime.
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

    // The local agents fetch (`swarm_list_local_agents`) detects the server
    // error envelope before attempting to parse the typed response. A server
    // error here is a bug (the tool reads the filesystem, no auth), but it
    // must not be silently swallowed — before the seam it fell through to
    // the typed parse, failed (no `agents` field), and the list silently
    // disappeared. This test pins the detection at the seam.
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
    }

    // Steer mode: the system prompt must name the `swarm-intelligence` skill
    // and the swarm MCP server scope, so the curator knows to invoke the
    // skill for composition/steering requests.
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
            "steer prompt must include the selected swarm id"
        );
    }

    #[test]
    fn steer_system_prompt_handles_no_swarm() {
        let prompt = steer_system_prompt(None);
        assert!(
            prompt.contains("No swarm"),
            "steer prompt must guide the operator when no swarm is selected"
        );
    }

    // KA-04: the steer prompt must not reference MCP tools that do not exist
    // in the swarm server. An advertised tool with no enforcement point is
    // the `.rules` trap. This test pins that the prompt references only tools
    // in the canonical const (checked exhaustively by
    // `steer_prompt_mentions_only_known_tools`).
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
    }

    // The steer prompt must describe the local tools so the curator knows to
    // use them. The full list is rendered from `TOOL_NAMES`, so this samples
    // the load-bearing delegation and management tools.
    #[test]
    fn steer_prompt_describes_local_tools() {
        let prompt = steer_system_prompt(Some("ws_test"));
        for tool in [
            "swarm_list_local_agents",
            "swarm_delegate_local",
            "swarm_fanout_local",
            "swarm_pipeline_local",
            "swarm_remove_local",
            "swarm_create_local_agent",
            "swarm_reconfigure_local_agent",
        ] {
            assert!(
                prompt.contains(tool),
                "steer prompt must describe the local tool {tool}"
            );
        }
        // Local delegation has no budget (operator ruling 2026-09-04): the
        // prompt must say so, or the Curator wastes a turn looking for a
        // funding gate that does not exist.
        assert!(
            prompt.contains("NO budget"),
            "steer prompt must tell the curator local delegation has no budget"
        );
        assert!(
            !prompt.contains("PaymentRequired"),
            "steer prompt must NOT promise a PaymentRequired error — local \
             delegation has no budget, so advertising it would make the curator \
             plan around a gate that does not exist"
        );
    }

    // The steer prompt must carry the active swarm and instruct the curator
    // to pass it in the skill's `context` argument. Without `swarm_id` in the
    // cascade context, the swarm-intelligence templates cannot read the
    // right roster.
    #[test]
    fn steer_prompt_carries_swarm_and_context_instruction() {
        let prompt = steer_system_prompt(Some("ws_test"));
        assert!(
            prompt.contains("\"swarm_id\": \"ws_test\""),
            "prompt must carry the swarm id in the context example"
        );
        assert!(
            prompt.contains("`context` argument"),
            "steer prompt must tell the curator to pass context to the skill tool"
        );
        // The slash-command context syntax must be documented so the
        // operator knows how to pass swarm_id via `/swarm-intelligence
        // swarm_id=ws-1 compose my swarm`.
        assert!(
            prompt.contains("key=value"),
            "steer prompt must document the key=value slash-command context syntax"
        );
    }

    // Cybernetic Swarm Plan C0: the steer prompt must describe the optional
    // deterministic `task_success` skill input and the no-LLM-judge constraint.
    #[test]
    fn steer_prompt_describes_task_success() {
        let prompt = steer_system_prompt(Some("ws_test"));
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
        let prompt = steer_system_prompt(Some("ws_test"));
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

    // M5: the Steer-mode system prompt must not advertise any `swarm_*`
    // tool that isn't in the canonical `SWARM_TOOLS` const. The const is a
    // re-export of the server's `hkask_mcp_swarm::TOOL_NAMES` (asserted by
    // `panel_tool_names_match_server`), and the tool list is rendered from
    // it, so it cannot drift — this test catches the residual risk of prose
    // that names tools by hand, so a rename surfaces here rather than
    // degrading to "tool not found" at runtime.
    #[test]
    fn steer_prompt_mentions_only_known_tools() {
        let known: std::collections::HashSet<&str> = parse::SWARM_TOOLS.iter().copied().collect();
        let kanban_known: std::collections::HashSet<&str> =
            parse::KANBAN_TOOLS.iter().copied().collect();
        for prompt in [
            steer_system_prompt(Some("ws_test")),
            steer_system_prompt(None),
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

    /// Every tool the swarm server exposes must be advertised in the prompt —
    /// the tool list is rendered from `TOOL_NAMES`, so this passes by
    /// construction and pins the renderer's completeness (a regression to a
    /// hand-curated subset would fail here).
    #[test]
    fn server_tools_are_all_advertised() {
        let prompt = steer_system_prompt(Some("ws_test"));
        for tool in parse::SWARM_TOOLS {
            assert!(
                prompt.contains(tool),
                "hkask_mcp_swarm::TOOL_NAMES lists `{tool}` but the Steer prompt \
                 never mentions it"
            );
        }
    }

    #[test]
    fn kanban_tool_names_match_server() {
        // `KANBAN_TOOLS` is re-exported from the server's build.rs-generated
        // `hkask_mcp_kata_kanban::TOOL_NAMES`, so the names cannot drift from the
        // server's `#[tool]` fns; this test pins the count and the naming
        // convention so a server change gets a deliberate look here.
        assert_eq!(hkask_types::kanban_wire::KANBAN_SERVER_NAME, "kata-kanban");

        // Pin the count so adding or removing a server tool without updating
        // this pin is caught. 27 as of 2026-10-23 (`kanban_task_kata_record`,
        // kata-state durability; the tool-count drift from the landed
        // tooling removals surfaced here — the server's own
        // `tool_names_match_live_router` pin keeps its generated TOOL_NAMES
        // honest, so a count change here always reflects a real server
        // change — update this pin in the same pass.
        assert_eq!(
            parse::KANBAN_TOOLS.len(),
            27,
            "tool count changed — update the count pin to match \
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
    // independently of the server, and the `debug_assert!` drift guard checks
    // tool NAMES only. This test pins the deletion: a backticked tool name
    // must not be immediately followed by a parenthesized gloss.
    #[test]
    fn steer_prompt_does_not_gloss_tool_names() {
        for prompt in [
            steer_system_prompt(Some("ws_test")),
            steer_system_prompt(None),
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
        let prompt = steer_system_prompt(Some("ws_test"));
        assert!(
            prompt.contains("kanban panel"),
            "steer prompt must reference the kanban panel as the coordination substrate"
        );
        assert!(
            prompt.contains("kanban_task_spawn"),
            "steer prompt must mention the `kanban_task_spawn` bridge tool"
        );
    }

    // ── D33: swarm panel local CRUD ───────────────────────────────────────
    //
    // The management surface keeps: the confirmation flow for destructive
    // ops, edit-metadata (local), and local swarm delete.

    #[test]
    fn destructive_action_delete_swarm_carries_id_and_name() {
        let action = DestructiveAction::DeleteSwarm {
            swarm_id: "team_alpha".into(),
            name: "Team Alpha".into(),
        };
        let DestructiveAction::DeleteSwarm { swarm_id, name } = &action;
        assert_eq!(swarm_id, "team_alpha");
        assert_eq!(name, "Team Alpha");
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
    }
}
